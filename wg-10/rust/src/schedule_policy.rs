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
        let cx = pos_x + vel_x * self.cfg.lead_frames;
        let cz = pos_z + vel_z * self.cfg.lead_frames;
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
}
