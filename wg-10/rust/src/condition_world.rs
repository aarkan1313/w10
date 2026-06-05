//! condition_world port (mountain_world_layer.py:48-65): whole-region percentile-robust + tanh
//! normalization that tames raw recipe output into the accepted bounded-relief look. Runs over a
//! baked REGION (finite tile) so global percentiles are valid. Reuses array_ops::gaussian_filter_nearest.
//!
//! NOTE: scipy gaussian_filter default mode='reflect'; the Rust gaussian is mode='nearest' -> border
//! cells differ slightly (tolerance-gated). np.percentile linear-interpolation is ported exactly
//! (deterministic + bit-portable), so the p05/p50/p95 stats match the Python oracle to ~1e-9.

use crate::array_ops::gaussian_filter_nearest;

/// Whole-region statistics emitted alongside the conditioned field (mirror of the Python stats dict).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConditionStats {
    pub source_min: f64,
    pub source_max: f64,
    pub source_ptp: f64,
    pub p05: f64,
    pub p50: f64,
    pub p95: f64,
    pub conditioned_min: f64,
    pub conditioned_max: f64,
    pub conditioned_ptp: f64,
}

/// `np.percentile(sorted, q)` with numpy's default `'linear'` interpolation.
///
/// `sorted` must be ascending and non-empty; `q` is a percentile in `[0, 100]`. For `N` values:
/// `rank = q/100 * (N-1)`; `lo = floor(rank)`; `frac = rank - lo`; result interpolates between
/// `sorted[lo]` and `sorted[lo+1]` (the `lo+1` index clamped to `N-1`, which also covers `q == 100`
/// where `frac == 0`). This is deterministic and bit-portable, so it reproduces numpy exactly.
pub fn percentile_linear(sorted: &[f64], q: f64) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return 0.0;
    }
    if n == 1 {
        return sorted[0];
    }
    let rank = q / 100.0 * (n as f64 - 1.0);
    let lo = rank.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    let frac = rank - lo as f64;
    sorted[lo] + frac * (sorted[hi] - sorted[lo])
}

/// Field-valued conditioning: `p05f/p50f/p95f` are EITHER length 1 (scalar, broadcast to every
/// cell) OR length `n*n` (a per-cell percentile field, for seam-exact cross-region normalization).
/// Identical math to `condition_world_with_percentiles` when the fields are length 1.
///
/// # Panics
/// Panics if `z.len() != n*n`, or if any percentile field is neither length 1 nor length `n*n`.
pub fn condition_world_with_percentile_fields(
    z: &[f64], n: usize, p05f: &[f64], p50f: &[f64], p95f: &[f64],
) -> Vec<f64> {
    let nn = n * n;
    assert_eq!(z.len(), nn, "condition_world_with_percentile_fields: z.len() != n*n");
    for (name, f) in [("p05f", p05f), ("p50f", p50f), ("p95f", p95f)] {
        assert!(f.len() == 1 || f.len() == nn,
            "condition_world_with_percentile_fields: {name} len {} not 1 or {nn}", f.len());
    }
    let at = |f: &[f64], i: usize| -> f64 { if f.len() == 1 { f[0] } else { f[i] } };
    let robust: Vec<f64> = (0..nn).map(|i| {
        let denom = at(p95f, i) - at(p05f, i) + 1.0e-9;
        (z[i] - at(p50f, i)) / denom * 2.10
    }).collect();
    let smoothed = gaussian_filter_nearest(&robust, n, n, 0.55, 4.0);
    smoothed.iter().map(|v| v.tanh()).collect()
}

/// Condition a region field using EXTERNALLY SUPPLIED percentiles (for cross-region seam
/// reconciliation). Identical math to `condition_world` from the `robust` step onward; only the
/// p05/p50/p95 source differs. The returned ConditionStats reports the SUPPLIED percentiles.
///
/// # Panics
/// Panics if `z.len() != n * n`.
pub fn condition_world_with_percentiles(z: &[f64], n: usize, p05: f64, p50: f64, p95: f64) -> Vec<f64> {
    condition_world_with_percentile_fields(z, n, &[p05], &[p50], &[p95])
}

/// Port of `mountain_world_layer.condition_world`. `z` is a flat row-major `f64` field of length
/// `n*n` (the grid side is `n`). Returns the conditioned field `shaped` (same layout) plus the
/// whole-region [`ConditionStats`].
///
/// Steps (identical to the Python):
/// 1. p05/p50/p95 via [`percentile_linear`] over a sorted copy of `z`.
/// 2. `robust = (z - p50) / (p95 - p05 + 1e-9) * 2.10` (per-cell).
/// 3. `shaped = tanh(gaussian_filter(robust, sigma=0.55))` — scipy default `truncate=4.0`.
///    Uses the mode='nearest' Rust gaussian (scipy default is 'reflect'); at sigma=0.55 the kernel
///    spans ~3 taps so only the 1-2 border rows/cols differ (tolerance-gated downstream).
///
/// # Panics
/// Panics if `z.len() != n * n`.
pub fn condition_world(z: &[f64], n: usize) -> (Vec<f64>, ConditionStats) {
    assert_eq!(
        z.len(),
        n * n,
        "condition_world: z.len() ({}) != n*n ({}*{})",
        z.len(),
        n,
        n
    );

    // Sorted copy drives both the percentiles and the source min/max (sorted ends).
    let mut sorted = z.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("condition_world: NaN in input field"));
    let p05 = percentile_linear(&sorted, 5.0);
    let p50 = percentile_linear(&sorted, 50.0);
    let p95 = percentile_linear(&sorted, 95.0);

    // Delegate to the injected-percentile variant (bit-identical math from the robust step onward).
    let shaped = condition_world_with_percentiles(z, n, p05, p50, p95);

    // Source stats from the sorted ends (np.min / np.max / np.ptp over z).
    let source_min = sorted[0];
    let source_max = sorted[n * n - 1];
    let source_ptp = source_max - source_min;

    // Conditioned stats from shaped (np.min / np.max / np.ptp over shaped).
    let mut conditioned_min = shaped[0];
    let mut conditioned_max = shaped[0];
    for &v in &shaped {
        if v < conditioned_min {
            conditioned_min = v;
        }
        if v > conditioned_max {
            conditioned_max = v;
        }
    }
    let conditioned_ptp = conditioned_max - conditioned_min;

    let stats = ConditionStats {
        source_min,
        source_max,
        source_ptp,
        p05,
        p50,
        p95,
        conditioned_min,
        conditioned_max,
        conditioned_ptp,
    };
    (shaped, stats)
}
