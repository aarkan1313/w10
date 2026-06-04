//! Godot API entrypoints for biome compose and blend readback gates.

use godot::prelude::*;

use super::{f32s_to_packed_f64, Wg10BiomePageCompute};

#[godot_api(secondary)]
impl Wg10BiomePageCompute {
    /// Load the biome fragment used to satisfy `biome_pass()` during compose gates.
    #[func]
    pub fn load_compose_fragment(&mut self, fragment_path: GString) -> GString {
        match std::fs::read_to_string(fragment_path.to_string()) {
            Ok(s) => {
                self.compose_fragment = Some(s);
                GString::new()
            }
            Err(e) => GString::from(format!("compose fragment glsl: {e}").as_str()),
        }
    }

    /// GPU port of `biome_compose::compose_biomes` for windowed parity gates.
    #[allow(clippy::too_many_arguments)]
    #[func]
    pub fn compose_fields(
        &self,
        fields_flat: PackedFloat64Array,
        weights_flat: PackedFloat64Array,
        n_fields: i64,
        rows: i64,
        cols: i64,
        mode_is_field: bool,
        favor_strength: f64,
        relief_confidence_floor: f64,
    ) -> PackedFloat64Array {
        let rows = rows as usize;
        let cols = cols as usize;
        let n = rows * cols;
        let nf = n_fields as usize;
        if nf == 0 {
            godot_error!("compose_fields: n_fields must be >= 1");
            return PackedFloat64Array::new();
        }
        if fields_flat.len() != nf * n || weights_flat.len() != nf * n {
            godot_error!(
                "compose_fields: fields/weights flat len mismatch (got {}/{}, expected {})",
                fields_flat.len(),
                weights_flat.len(),
                nf * n
            );
            return PackedFloat64Array::new();
        }

        let ff = fields_flat.as_slice();
        let wf = weights_flat.as_slice();
        let mut fields: Vec<Vec<f32>> = Vec::with_capacity(nf);
        let mut weights: Vec<Vec<f32>> = Vec::with_capacity(nf);
        for k in 0..nf {
            fields.push(ff[k * n..(k + 1) * n].iter().map(|&x| x as f32).collect());
            weights.push(wf[k * n..(k + 1) * n].iter().map(|&x| x as f32).collect());
        }

        match self.run_compose_inner(
            &fields,
            &weights,
            rows,
            cols,
            mode_is_field,
            favor_strength as f32,
            relief_confidence_floor as f32,
        ) {
            Ok(out) => f32s_to_packed_f64(&out),
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::compose_fields error: {e}");
                PackedFloat64Array::new()
            }
        }
    }

    /// GPU port of one `blend_field` or `blend_height_favored` operation.
    #[allow(clippy::too_many_arguments)]
    #[func]
    pub fn blend_pair(
        &self,
        a: PackedFloat64Array,
        b: PackedFloat64Array,
        w_a: PackedFloat64Array,
        rows: i64,
        cols: i64,
        mode_is_field: bool,
        favor_strength: f64,
        relief_confidence_floor: f64,
    ) -> PackedFloat64Array {
        let rows = rows as usize;
        let cols = cols as usize;
        let n = rows * cols;
        if a.len() != n || b.len() != n || w_a.len() != n {
            godot_error!(
                "blend_pair: a/b/w_a len mismatch (got {}/{}/{}, expected {})",
                a.len(),
                b.len(),
                w_a.len(),
                n
            );
            return PackedFloat64Array::new();
        }
        let a32: Vec<f32> = a.as_slice().iter().map(|&x| x as f32).collect();
        let b32: Vec<f32> = b.as_slice().iter().map(|&x| x as f32).collect();
        let w32: Vec<f32> = w_a.as_slice().iter().map(|&x| x as f32).collect();
        match self.run_blend_inner(
            &a32,
            &b32,
            &w32,
            rows,
            cols,
            mode_is_field,
            favor_strength as f32,
            relief_confidence_floor as f32,
        ) {
            Ok(out) => f32s_to_packed_f64(&out),
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::blend_pair error: {e}");
                PackedFloat64Array::new()
            }
        }
    }
}
