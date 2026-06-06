//! GATE 1: super-region OUTER-border percentile seam-exactness across DIFFERENT super-keys.
//!
//! `super_region_tests.rs` proves INTERNAL (within one super-region) slice seams are bit-exact.
//! This file proves the layer ABOVE it: two INDEPENDENT super-regions in DIFFERENT super-keys —
//! super-A at world [0, SUPER_SPAN], super-B at world [SUPER_SPAN, 2*SUPER_SPAN], abutting in X —
//! driven by a SHARED world-position macro sampler, agree at their shared OUTER border at the
//! PERCENTILE layer (0-ULP). That is the smooth percentile field being seam-exact ACROSS
//! super-regions, not just within one (the within-one version lives in percentile_seam_tests.rs).
//!
//! SCOPE (be precise about WHICH layer this owns): this gates the PERCENTILE field only. The full
//! conditioned-height outer seam still carries the carve + condition_world edge-clamp gaussian
//! residual — a SEPARATE, known, k-knob-tunable tradeoff. We MEASURE+PRINT it (so it is visible),
//! and assert only that the percentile border is exact; we do NOT fail on the conditioned residual.
use crate::pass_network::{PassNetworkParams, RampParams, TraverseParams};
use crate::region_bake::{
    bake_super_region, PercentileProvider, SmoothFieldPercentiles,
};

// ONE continuous, deterministic, non-trivial world function over BOTH super-regions. Continuous
// everywhere => the macro (and hence the RAW we build from it) is seam-exact across the outer border.
fn world_macro(wx: f64, wz: f64) -> f64 {
    (wx * 1e-4).sin() * (wz * 1e-4).cos() * 1.5 + (wx * 3e-4).cos() * 0.5
}

// Build a super_raw by sampling the SAME world function at each super-grid cell's ABSOLUTE world
// position (texel-corner), so the RAW is continuous across the shared outer border by construction.
fn super_raw_from_world(super_x0: f64, super_z0: f64, super_span: f64, super_n: usize) -> Vec<f64> {
    let cell = super_span / (super_n as f64 - 1.0);
    let mut z = vec![0.0f64; super_n * super_n];
    for r in 0..super_n {
        let wz = super_z0 + r as f64 * cell;
        for c in 0..super_n {
            let wx = super_x0 + c as f64 * cell;
            z[r * super_n + c] = world_macro(wx, wz);
        }
    }
    z
}

fn make_provider() -> SmoothFieldPercentiles<impl Fn(f64, f64) -> f64> {
    // SAME sampler for BOTH super-regions (the world fn is global / key-free), keyed to absolute
    // world position. Params mirror percentile_seam_tests' smooth_prov style.
    SmoothFieldPercentiles {
        macro_sampler: |wx, wz| world_macro(wx, wz),
        coarse_stride_m: REGION_SPAN, // = region_span_m (one region)
        window_radius_m: REGION_SPAN * 0.5,
        window_samples: 33,
    }
}

const N: usize = 33; // region grid side
const K: usize = 2; // 2x2 super-region
const REGION_SPAN: f64 = 25600.0;
const HS: f64 = 260.0;

#[test]
fn outer_super_region_border_percentiles_are_seam_exact() {
    let super_n = K * (N - 1) + 1; // 65
    let super_span = REGION_SPAN * K as f64; // 51200
    // super-A core origin at world (0,0); super-B core origin at world (SUPER_SPAN, 0) — they abut
    // at world x = SUPER_SPAN (pure-CPU test uses core origins directly, no apron — same convention
    // as percentile_seam_tests, which passes region origins straight in).
    let super_x0_a = 0.0;
    let super_x0_b = super_span;
    let super_z0 = 0.0;

    let raw_a = super_raw_from_world(super_x0_a, super_z0, super_span, super_n);
    let raw_b = super_raw_from_world(super_x0_b, super_z0, super_span, super_n);

    let pass = PassNetworkParams::default();
    let traverse = TraverseParams {
        scene_width_m: super_span,
        height_scale_m: HS,
        ..Default::default()
    };
    let ramp = RampParams::default();

    // ---- THE PROOF: query the PERCENTILE FIELDS over each WHOLE super-grid directly. ----
    // super-A's rightmost super-grid column (super_n-1) lives at world x = super_x0_a + super_span =
    // SUPER_SPAN; super-B's leftmost super-grid column (0) lives at world x = super_x0_b = SUPER_SPAN.
    // Same world column, shared sampler => bit-identical percentile values, per row.
    let prov_a = make_provider();
    let prov_b = make_provider();
    let fa = prov_a.percentiles(&raw_a, super_x0_a, super_z0, super_span, super_n);
    let fb = prov_b.percentiles(&raw_b, super_x0_b, super_z0, super_span, super_n);

    let mut perc_border_max = 0.0f64;
    let mut all_bit_exact = true;
    for r in 0..super_n {
        let ia = r * super_n + (super_n - 1); // A's rightmost column
        let ib = r * super_n; // B's leftmost column
        for (ga, gb) in [(&fa.p05, &fb.p05), (&fa.p50, &fb.p50), (&fa.p95, &fb.p95)] {
            let d = (ga[ia] - gb[ib]).abs();
            perc_border_max = perc_border_max.max(d);
            if ga[ia].to_bits() != gb[ib].to_bits() {
                all_bit_exact = false;
            }
        }
    }
    let perc_label = if all_bit_exact { "0 ULP (bit-exact)".to_string() } else { format!("{perc_border_max:.3e} (<1e-9)") };

    // ---- DOCUMENT (measured, NOT gated): the conditioned-height outer seam. ----
    // Bake both super-regions; the abutting OUTER border is super-A slice (k-1,0) right column vs
    // super-B slice (0,0) left column. This residual = carve + condition_world edge-clamp gaussian,
    // the known k-knob tradeoff — print it so the tradeoff is visible, do NOT fail on it.
    let slices_a = bake_super_region(&raw_a, super_n, N, K, REGION_SPAN, HS, super_x0_a, super_z0,
        &pass, &traverse, &ramp, &prov_a);
    let slices_b = bake_super_region(&raw_b, super_n, N, K, REGION_SPAN, HS, super_x0_b, super_z0,
        &pass, &traverse, &ramp, &prov_b);
    let idx = |gi: usize, gj: usize| gj * K + gi;
    let a_right = &slices_a[idx(K - 1, 0)]; // super-A's right-edge region
    let b_left = &slices_b[idx(0, 0)]; // super-B's left-edge region
    // sanity: these two region slices actually abut at world x = SUPER_SPAN.
    assert!((a_right.origin_x_m + a_right.span_m - b_left.origin_x_m).abs() < 1e-6,
        "outer-seam test mis-set: a_right ends at {} but b_left starts at {}",
        a_right.origin_x_m + a_right.span_m, b_left.origin_x_m);
    let mut cond_border_max = 0.0f64;
    for row in 0..N {
        let a = a_right.grid[row * N + (N - 1)];
        let b = b_left.grid[row * N + 0];
        cond_border_max = cond_border_max.max((a - b).abs());
    }

    println!(
        "[outer-seam] percentile_border={perc_label} conditioned_border={:.4}m (carve+gaussian residual, k-knob tradeoff)",
        cond_border_max * HS
    );

    // ---- ASSERT only the layer this gate owns: the percentile field is seam-exact. ----
    assert!(
        perc_border_max < 1e-9,
        "outer super-region percentile border NOT seam-exact: {perc_border_max:.3e} (>= 1e-9) -> provider keyed super-local, not world"
    );
}
