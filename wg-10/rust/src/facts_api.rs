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
}
