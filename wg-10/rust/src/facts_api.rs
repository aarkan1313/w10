//! Wg10Facts (DESIGN §6.2) — the drop-in authoritative Facts API node. RefCounted (a pure query
//! object, like Wg10Height — no scene-tree behaviour). Loads its OWN pack/seed (independent of the
//! renderer, so a game can use facts with no renderer), holds a clamp config + an edit provider,
//! and answers the sparse questions gameplay reads: get_height (point), get_collision_field (grid).
//! All authoritative, CPU, no GPU readback on this path (the WG9-safe rule). Reuses the
//! parity-gated `height::height`; never changes the formula.

use godot::prelude::*;
use crate::pack::{self, Pack};
use crate::height;
use crate::edit_layer::{EditProvider, StampEdits};
use crate::facts;
use std::path::Path;

#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10Facts {
    pack: Option<Pack>,
    seed: i64,
    edits: StampEdits,   // the concrete provider; empty == NoEdits behaviour (delta 0)
    floor: f64,          // bedrock clamp (default: unbounded)
    ceil: f64,
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10Facts {
    fn init(base: Base<RefCounted>) -> Self {
        Self {
            pack: None,
            seed: 0,
            edits: StampEdits::new(),
            floor: f64::NEG_INFINITY,
            ceil: f64::INFINITY,
            base,
        }
    }
}

#[godot_api]
impl Wg10Facts {
    /// Load + validate the pack and set the seed. Returns "" on success or the error message.
    /// `dir` is an OS path (GDScript resolves res:// via ProjectSettings.globalize_path).
    /// The Facts node loads its OWN pack — independent of the renderer/pool — so it is a true
    /// standalone drop-in. The in-memory grammar constants are tiny, so the independence is free.
    #[func]
    fn configure(&mut self, dir: GString, file: GString, seed: i64) -> GString {
        match pack::load_pack_dir(Path::new(&dir.to_string()), &file.to_string()) {
            Ok(p) => {
                self.pack = Some(p);
                self.seed = seed;
                GString::new()
            }
            Err(e) => GString::from(&e),
        }
    }

    /// Authoritative composed height at (x,z): clamp(base + edit delta, floor, ceil).
    /// Returns 0.0 if not configured (logs an error — never silently computes garbage).
    #[func]
    fn get_height(&self, x: f64, z: f64) -> f64 {
        let Some(p) = &self.pack else {
            godot_error!("Wg10Facts: get_height called before configure()");
            return 0.0;
        };
        let base = height::height(x, z, self.seed, p);
        let delta = self.edits.delta(x, z) as f64;
        facts::composed_height(base, delta, self.floor, self.ceil)
    }

    /// Add a circular edit stamp (crater/mound). `depth` signed metres (neg = dig), `radius` m,
    /// `falloff` in [0,1] (0 = flat dent, 1 = smooth cosine fade). Takes effect on the next
    /// get_height/get_collision_field (no cache). Ignored if radius <= 0.
    #[func]
    fn apply_edit(&mut self, cx: f64, cz: f64, radius: f64, depth: f64, falloff: f64) {
        if radius <= 0.0 {
            godot_warn!("Wg10Facts: apply_edit ignored, radius <= 0 ({radius})");
            return;
        }
        self.edits.add(cx, cz, radius, depth as f32, falloff as f32);
    }

    /// Remove all edits (terrain returns to pure base).
    #[func]
    fn clear_edits(&mut self) {
        self.edits.clear();
    }

    /// Set the bedrock floor + ceiling clamp (metres). Rejected (config unchanged) if floor > ceil.
    /// Defaults are unbounded; e.g. set_bedrock(-2.0, 1e30) gives "dig down 2 m then hit bedrock".
    #[func]
    fn set_bedrock(&mut self, floor: f64, ceiling: f64) {
        if floor > ceiling {
            godot_error!("Wg10Facts: set_bedrock rejected, floor {floor} > ceiling {ceiling}");
            return;
        }
        self.floor = floor;
        self.ceil = ceiling;
    }

    /// Authoritative collision heights over a square patch: an n×n row-major PackedFloat32Array
    /// (raw metres) of composed height, centred at (center_x, center_z) spanning `world_size` m.
    /// The CALLER builds the Jolt HeightMapShape3D (map_width=n, map_depth=n, map_data=this) + body
    /// and owns its lifetime — the Facts API stays a pure sparse query (no physics state here).
    /// CPU only, no GPU readback (the hot-path rule). Empty array (+ error) on bad args / not
    /// configured (Jolt needs samples_per_side >= 2).
    #[func]
    fn get_collision_field(
        &self,
        center_x: f64,
        center_z: f64,
        world_size: f64,
        samples_per_side: i64,
    ) -> PackedFloat32Array {
        let mut out = PackedFloat32Array::new();
        let Some(p) = &self.pack else {
            godot_error!("Wg10Facts: get_collision_field before configure()");
            return out;
        };
        if samples_per_side < 2 || world_size <= 0.0 {
            godot_error!(
                "Wg10Facts: get_collision_field bad args (n={samples_per_side}, size={world_size}); need n>=2, size>0"
            );
            return out;
        }
        let seed = self.seed;
        let edits = &self.edits;
        let (floor, ceil) = (self.floor, self.ceil);
        let grid = facts::collision_field(
            center_x,
            center_z,
            world_size,
            samples_per_side as usize,
            |x, z| {
                let base = height::height(x, z, seed, p);
                facts::composed_height(base, edits.delta(x, z) as f64, floor, ceil)
            },
        );
        for h in grid {
            out.push(h);
        }
        out
    }
}
