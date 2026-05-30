//! Stream-ahead scheduler math (DESIGN §5.3), pure — no `godot` imports.
//!
//! Decides which page textures the clipmap rings need soon (coverage), a bounded
//! per-frame acquire/release plan, and a coarser-page fallback for any page that
//! is needed but not yet resident. Operates entirely on `page_policy::PageKey`
//! (world-metre origins) so policy / pool / scheduler share ONE key vocabulary.
//! Owns no RIDs, calls no pool, dispatches nothing, and NEVER assumes a page is
//! resident the same frame it was requested (the async-ready seam, spec §1.1).

use crate::page_policy::PageKey;
use std::collections::HashSet;

/// All scheduler tunables. No magic numbers live in the policy body.
#[derive(Debug, Clone, Copy)]
pub struct ScheduleConfig {
    /// Clipmap levels: 0 = finest .. num_levels-1 = coarsest.
    pub num_levels: i32,
    /// World-space span (metres) of one level-0 page. Level L spans base_span * 2^L.
    pub base_span: f64,
    /// Ring half-extent in pages, per level (radius 1 -> 3x3 ring).
    pub radius_pages: i32,
    /// Velocity lead: bias coverage centre this many frames ahead of position.
    pub lead_frames: f64,
    /// Hard cap on acquires dispatched per update.
    pub max_per_frame: u32,
}

/// A bounded per-frame plan: pages to acquire (capped at max_per_frame, coarsest +
/// nearest-ahead first — the coarse never-black blanket leads) and pages to release
/// (resident but no longer covered).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FramePlan {
    pub acquire: Vec<PageKey>,
    pub release: Vec<PageKey>,
}

/// Pure scheduling policy. Constructed once from a config; methods are pure
/// functions of (config, pos, vel, resident).
pub struct SchedulePolicy {
    cfg: ScheduleConfig,
}

impl SchedulePolicy {
    pub fn new(cfg: ScheduleConfig) -> Self {
        assert!(cfg.num_levels >= 1, "num_levels must be >= 1");
        assert!(cfg.base_span > 0.0, "base_span must be > 0");
        assert!(
            cfg.base_span.fract() == 0.0,
            "base_span must be an exact integer (got {})", cfg.base_span
        );
        assert!(cfg.radius_pages >= 0, "radius_pages must be >= 0");
        assert!(cfg.num_levels <= 32, "num_levels must be <= 32 (level spans use 2^level)");
        assert!(cfg.max_per_frame >= 1, "max_per_frame must be >= 1");
        Self { cfg }
    }

    pub fn config(&self) -> ScheduleConfig { self.cfg }

    /// World-space span of one page at `level`.
    pub fn level_span(&self, level: i32) -> f64 {
        self.cfg.base_span * 2f64.powi(level)
    }

    /// Floor-quantize a world-space centre to the page corner (origin) at `level`,
    /// in world metres. Same floor semantics as grammar.rs (seam-exact).
    pub fn page_origin(&self, level: i32, cx: f64, cz: f64) -> (i64, i64) {
        let span = self.level_span(level);
        let ox = (cx / span).floor() as i64 * span as i64;
        let oz = (cz / span).floor() as i64 * span as i64;
        (ox, oz)
    }

    /// The set of page keys the rings need this frame: for each level, a
    /// (2*radius+1)^2 ring of pages around the velocity-biased centre. Union
    /// across levels. Deduplicated. Pure function of (cfg, pos, vel).
    pub fn coverage(&self, pos_x: f64, pos_z: f64, vel_x: f64, vel_z: f64) -> Vec<PageKey> {
        let (cx, cz) = self.coverage_center(pos_x, pos_z, vel_x, vel_z);
        let r = self.cfg.radius_pages;
        // `seen` dedups defensively: today levels never overlap in key-space
        // (PageKey includes `level`), but this guards future multi-ring overlap.
        let mut seen: HashSet<PageKey> = HashSet::new();
        let mut out: Vec<PageKey> = Vec::new();
        for level in 0..self.cfg.num_levels {
            let span = self.level_span(level) as i64;
            let (centre_ox, centre_oz) = self.page_origin(level, cx, cz);
            for dz in -r..=r {
                for dx in -r..=r {
                    let key = PageKey {
                        level,
                        origin_x: centre_ox + dx as i64 * span,
                        origin_z: centre_oz + dz as i64 * span,
                    };
                    if seen.insert(key) {
                        out.push(key);
                    }
                }
            }
        }
        out
    }

    /// The velocity-led world point coverage (and the renderer) centre their rings on. Exposed
    /// so `Wg10TerrainView` displays EXACTLY the ring the scheduler maintains — the slice-8
    /// flicker bug was the view centring on the raw camera position while the scheduler centred
    /// on the led point, so the view referenced pages the scheduler had released (churn + miss).
    /// One centre, one ring: never-black budget math (coarsest column <= max_per_frame) is intact.
    pub fn coverage_center(&self, pos_x: f64, pos_z: f64, vel_x: f64, vel_z: f64) -> (f64, f64) {
        (pos_x + vel_x * self.cfg.lead_frames, pos_z + vel_z * self.cfg.lead_frames)
    }

    /// For a needed-but-not-resident page, walk UP the levels (coarser) and return
    /// the first coarser page that contains `missing`'s area AND is resident.
    /// Returns None only if no coarser resident page covers the area. This is the
    /// never-black guarantee: every coverage gap with a resident coarse ancestor
    /// resolves to lower-detail-but-correct terrain instead of a hole.
    pub fn coarser_fallback(
        &self,
        missing: PageKey,
        resident: &HashSet<PageKey>,
    ) -> Option<PageKey> {
        // Use the centre of the missing page so quantization to the coarser grid
        // lands on the page that contains it (corner + half-span avoids the seam).
        let span = self.level_span(missing.level);
        let cx = missing.origin_x as f64 + span * 0.5;
        let cz = missing.origin_z as f64 + span * 0.5;
        for level in (missing.level + 1)..self.cfg.num_levels {
            let (ox, oz) = self.page_origin(level, cx, cz);
            let ancestor = PageKey { level, origin_x: ox, origin_z: oz };
            if resident.contains(&ancestor) {
                return Some(ancestor);
            }
        }
        None
    }

    /// Diff coverage against the observed resident set and produce a bounded,
    /// prioritized acquire/release plan. NEVER assumes acquired pages become
    /// resident this frame — residency is observed next frame (async-ready seam).
    pub fn plan_frame(
        &self,
        pos_x: f64,
        pos_z: f64,
        vel_x: f64,
        vel_z: f64,
        resident: &HashSet<PageKey>,
    ) -> FramePlan {
        let needed = self.coverage(pos_x, pos_z, vel_x, vel_z);
        let needed_set: HashSet<PageKey> = needed.iter().cloned().collect();

        // release = resident - needed (uncapped: releasing is cheap bookkeeping).
        // Sorted so the whole FramePlan is deterministic — HashSet iteration order
        // is process-randomized, and the spec (§2.5) requires deterministic plans.
        let mut release: Vec<PageKey> = resident
            .iter()
            .filter(|k| !needed_set.contains(k))
            .cloned()
            .collect();
        release.sort();

        // missing = needed - resident, prioritized then truncated.
        let cx = pos_x + vel_x * self.cfg.lead_frames;
        let cz = pos_z + vel_z * self.cfg.lead_frames;
        let mut missing: Vec<PageKey> = needed
            .into_iter()
            .filter(|k| !resident.contains(k))
            .collect();

        // Priority: COARSEST level first (the coarse pages ARE the never-black
        // blanket — they must be acquired before fine detail or a fast camera
        // outruns the blanket and coverage goes black). Then nearest the led centre.
        // Distance is page-centre to led-centre, integerized for a total order.
        // (A windowed gate falsified the original finest-first priority: it starved
        // the coarse ring under motion. Coarsest-first makes never-black structural.)
        missing.sort_by_key(|k| {
            let span = self.level_span(k.level);
            let kcx = k.origin_x as f64 + span * 0.5;
            let kcz = k.origin_z as f64 + span * 0.5;
            let d2 = (kcx - cx) * (kcx - cx) + (kcz - cz) * (kcz - cz);
            // (-level, distance^2, origin) — `-(k.level as i64)` so coarser (higher
            // level) sorts first; origin breaks remaining ties for a deterministic
            // total order. `d2 as i64` is a saturating cast (Rust 1.45+); for any
            // realistic world coordinate d2 stays well under i64::MAX.
            (-(k.level as i64), d2 as i64, k.origin_x, k.origin_z)
        });
        missing.truncate(self.cfg.max_per_frame as usize);

        FramePlan { acquire: missing, release }
    }
}
