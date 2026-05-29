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
}
