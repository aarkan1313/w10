//! Wg10Streamer — the live frame-loop driver (DESIGN §5.4). Thin godot binding:
//! holds a SchedulePolicy + a handle to a Wg10PagePool, and on each update asks
//! the policy for a frame plan, releases departing pages, then acquires <= N pages
//! synchronously. Owns no RIDs, contains no scheduling math, holds no meshes.
//!
//! Synchronous production this slice; the policy never assumes same-frame
//! residency, so a background producer drops in behind the pool's acquire_page
//! later with zero change here (spec §1.1).

use godot::prelude::*;
use crate::page_pool::Wg10PagePool;
use crate::page_policy::PageKey;
use crate::schedule_policy::{ScheduleConfig, SchedulePolicy};
use std::collections::HashSet;

#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10Streamer {
    policy: Option<SchedulePolicy>,
    pool: Option<Gd<Wg10PagePool>>,
    acquired_this_frame: i64,
    released_this_frame: i64,
    full_events: i64,
    last_coverage_size: i64,
    frame: i64,
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10Streamer {
    fn init(base: Base<RefCounted>) -> Self {
        Self {
            policy: None,
            pool: None,
            acquired_this_frame: 0,
            released_this_frame: 0,
            full_events: 0,
            last_coverage_size: 0,
            frame: 0,
            base,
        }
    }
}

#[godot_api]
impl Wg10Streamer {
    /// Wire up the streamer with a configured pool and the scheduler tunables.
    /// `pool` must already have had `configure(...)` called on it.
    #[func]
    pub fn configure(
        &mut self,
        pool: Gd<Wg10PagePool>,
        num_levels: i64,
        base_span: f64,
        radius_pages: i64,
        lead_seconds: f64,
        max_per_frame: i64,
    ) {
        self.policy = Some(SchedulePolicy::new(ScheduleConfig {
            num_levels: num_levels as i32,
            base_span,
            radius_pages: radius_pages as i32,
            lead_seconds,
            max_per_frame: max_per_frame as u32,
        }));
        self.pool = Some(pool);
        self.frame = 0;
    }

    /// One frame of the §5.4 loop. Release departing pages, then acquire up to
    /// max_per_frame pages (synchronous this slice). Records stats. Acquires are
    /// already capped by the policy; we re-assert the cap defensively.
    /// `acquired_this_frame` counts successful acquires; pool-Full (null) outcomes
    /// are counted in `full_events`.
    #[func]
    pub fn update(&mut self, camera_x: f64, camera_z: f64, vel_x: f64, vel_z: f64) {
        // Guard: configured?
        if self.policy.is_none() || self.pool.is_none() {
            godot_error!("Wg10Streamer: update called before configure()");
            return;
        }

        // Read residency from the pool, build the plan from the policy.
        let resident = self.resident_set();
        let plan = {
            let policy = self.policy.as_ref().unwrap();
            policy.plan_frame(camera_x, camera_z, vel_x, vel_z, &resident)
        };
        let cap = self.policy.as_ref().unwrap().config().max_per_frame as usize;
        self.last_coverage_size = self.policy.as_ref().unwrap()
            .coverage(camera_x, camera_z, vel_x, vel_z).len() as i64;

        // Release departing pages (cheap, order-independent).
        // Clone the Gd handle into a local before bind_mut() so we don't hold a
        // borrow of self.pool while mutably borrowing the pool — the clone is a
        // cheap refcount bump pointing at the SAME object (see gdext note).
        self.released_this_frame = 0;
        {
            let mut pool = self.pool.as_ref().unwrap().clone();
            for key in &plan.release {
                pool.bind_mut().release_page(
                    key.level as i64,
                    key.origin_x as f64,
                    key.origin_z as f64,
                );
                self.released_this_frame += 1;
            }
        }

        // Acquire up to `cap` pages synchronously. A null (Full) is served by
        // coarser fallback this frame — not an error.
        self.acquired_this_frame = 0;
        {
            let mut pool = self.pool.as_ref().unwrap().clone();
            for key in plan.acquire.iter().take(cap) {
                let tex = pool.bind_mut().acquire_page(
                    key.level as i64,
                    key.origin_x as f64,
                    key.origin_z as f64,
                );
                if tex.is_none() {
                    self.full_events += 1;
                } else {
                    self.acquired_this_frame += 1;
                }
            }
        }

        self.frame += 1;
    }

    /// Per-frame stats window (the gate's and the future overlay's view in).
    #[func]
    pub fn stats(&self) -> Dictionary<GString, Variant> {
        let mut d = Dictionary::<GString, Variant>::new();
        d.set("acquired_this_frame", self.acquired_this_frame);
        d.set("released_this_frame", self.released_this_frame);
        d.set("full_events", self.full_events);
        d.set("frame", self.frame);
        d.set("coverage_size", self.last_coverage_size);
        let resident = self.pool.as_ref()
            .map(|p| {
                let ps = p.bind().stats();
                ps.get("resident").map(|v| i64::from_variant(&v)).unwrap_or(0)
            })
            .unwrap_or(0);
        d.set("resident", resident);
        d
    }

    /// Coverage for a frame as a flat (level, origin_x, origin_z) triple array —
    /// lets the gate assert "every covered page is resident OR has a coarser
    /// fallback" without real ring meshes.
    #[func]
    pub fn coverage_keys(
        &self,
        camera_x: f64,
        camera_z: f64,
        vel_x: f64,
        vel_z: f64,
    ) -> PackedInt64Array {
        let mut out = PackedInt64Array::new();
        if let Some(policy) = self.policy.as_ref() {
            for k in policy.coverage(camera_x, camera_z, vel_x, vel_z) {
                out.push(k.level as i64);
                out.push(k.origin_x);
                out.push(k.origin_z);
            }
        }
        out
    }

    /// Display coverage for a frame as a flat (level, origin_x, origin_z) triple array. This is the
    /// camera-centred visible ring only; `coverage_keys` also includes velocity-led prefetch pages.
    #[func]
    pub fn display_keys(
        &self,
        camera_x: f64,
        camera_z: f64,
    ) -> PackedInt64Array {
        let mut out = PackedInt64Array::new();
        if let Some(policy) = self.policy.as_ref() {
            for k in policy.display_coverage(camera_x, camera_z) {
                out.push(k.level as i64);
                out.push(k.origin_x);
                out.push(k.origin_z);
            }
        }
        out
    }

    /// The clamped velocity-led prefetch centre, as a Vector2(x,z). Display consumers should use
    /// `display_keys` / the camera-centred ring; this remains useful for diagnostics.
    #[func]
    pub fn coverage_center(&self, camera_x: f64, camera_z: f64, vel_x: f64, vel_z: f64) -> Vector2 {
        match self.policy.as_ref() {
            Some(p) => {
                let (cx, cz) = p.coverage_center(camera_x, camera_z, vel_x, vel_z);
                Vector2::new(cx as f32, cz as f32)
            }
            None => Vector2::new(camera_x as f32, camera_z as f32),
        }
    }
}

impl Wg10Streamer {
    /// Read the pool's resident keys and rebuild the PageKey set for the policy.
    fn resident_set(&self) -> HashSet<PageKey> {
        let flat = self.pool.as_ref().unwrap().bind().resident_keys();
        let s = flat.as_slice();
        let mut set = HashSet::new();
        let mut i = 0usize;
        while i + 3 <= s.len() {
            set.insert(PageKey {
                level: s[i] as i32,
                origin_x: s[i + 1],
                origin_z: s[i + 2],
            });
            i += 3;
        }
        set
    }
}
