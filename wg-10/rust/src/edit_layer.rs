//! Pluggable terrain-edit layer (DESIGN M4). Pure, no `godot` imports. An EditProvider returns a
//! signed height DELTA (metres) at a world point; the Facts layer composes it onto the base height
//! and clamps. `NoEdits` (the default) returns 0 everywhere at zero cost. Swapping providers
//! (circular stamps now; a denser cave representation later) needs NO change in any consumer.

/// A pluggable source of terrain-height edits. `delta(x,z)` is the signed metres to add to the
/// base height at that world point (negative = dig/crater, positive = mound). Pure + deterministic.
pub trait EditProvider {
    fn delta(&self, x: f64, z: f64) -> f32;
}

/// The default: no edits. Always 0 — the zero-cost "this game has no terrain editing" case.
pub struct NoEdits;

impl EditProvider for NoEdits {
    fn delta(&self, _x: f64, _z: f64) -> f32 {
        0.0
    }
}

/// One circular edit stamp: a crater (negative depth) or mound (positive) centred at (cx,cz) with
/// the given radius (m), depth (signed m), and falloff in [0,1] (0 = flat dent to the edge,
/// 1 = smooth cosine fade to 0 at the edge).
#[derive(Clone, Copy)]
pub struct Stamp {
    pub cx: f64,
    pub cz: f64,
    pub radius: f64,
    pub depth: f32,
    pub falloff: f32,
}

/// Circular-stamp edits: the concrete edit provider M4 ships. An empty list behaves like `NoEdits`
/// (delta 0). `delta` sums every overlapping stamp (M4 slice 2 fills in the real summation; the
/// slice-1 stub returns 0 so the seam compiles and base parity holds).
pub struct StampEdits {
    stamps: Vec<Stamp>,
}

impl StampEdits {
    pub fn new() -> Self {
        Self { stamps: Vec::new() }
    }

    /// Add a circular stamp. `radius <= 0` is ignored; `falloff` is clamped to [0,1].
    pub fn add(&mut self, cx: f64, cz: f64, radius: f64, depth: f32, falloff: f32) {
        if radius <= 0.0 {
            return;
        }
        self.stamps.push(Stamp {
            cx,
            cz,
            radius,
            depth,
            falloff: falloff.clamp(0.0, 1.0),
        });
    }

    pub fn clear(&mut self) {
        self.stamps.clear();
    }

    pub fn len(&self) -> usize {
        self.stamps.len()
    }
}

impl EditProvider for StampEdits {
    fn delta(&self, x: f64, z: f64) -> f32 {
        let mut sum = 0.0f32;
        for s in &self.stamps {
            let dx = x - s.cx;
            let dz = z - s.cz;
            let dist = (dx * dx + dz * dz).sqrt();
            if dist >= s.radius {
                continue; // outside this stamp
            }
            let t = (dist / s.radius) as f32; // 0 at centre .. 1 at edge
            // weight: 1 in the flat core, easing to 0 at the edge over the `falloff` fraction.
            // falloff 0 -> flat dent (weight 1 until the edge); falloff 1 -> cosine from centre.
            let w = if s.falloff <= 1e-6 {
                1.0
            } else {
                let inner = 1.0 - s.falloff; // start of the fade band, as a fraction of radius
                if t <= inner {
                    1.0
                } else {
                    let u = (t - inner) / s.falloff; // 0..1 across the fade band
                    0.5 * (1.0 + (std::f32::consts::PI * u).cos()) // cosine 1 -> 0
                }
            };
            sum += s.depth * w;
        }
        sum
    }
}
