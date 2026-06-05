"""Emit the Python `condition_world` parity ORACLE fixture.

`condition_world` (mountain_world_layer.py:48-65) is the whole-region normalization that tames raw
recipe output into the accepted bounded-relief "mountain chunk network" look: a percentile-robust
rescale (p05/p50/p95 via np.percentile, default LINEAR interpolation) followed by a tiny Gaussian
smooth (scipy gaussian_filter, sigma=0.55, default mode='reflect') and a tanh squash. It runs over a
baked REGION (a finite tile) so the GLOBAL percentiles it takes are valid.

The live runtime producer lacked this transform (and the carve), which is why it read ~2x too tall.
We are porting `condition_world` to Rust bit-faithfully where possible; this script produces the parity
oracle the Rust port must reproduce: a committed JSON fixture holding a deterministic input field + the
exact Python `shaped` output + the full stats dict.

Run from repo root:
    python tools/dem_pack/export_condition_world_fixture.py

Writes:
    tools/dem_pack/fixtures/condition_world_fixture.json

Determinism: the input field is a fixed analytic ridged sum-of-sines (NO numpy global RNG), so re-runs
are bit-identical. n=193. The field is deliberately given real spread (NOT flat) so the p05/p50/p95
percentiles are meaningful and distinct -- a flat field would make the robust rescale degenerate and the
oracle worthless.

Parity notes for the Rust port (these drive a TOLERANCE gate, not bit-exact):
  * np.percentile default interpolation='linear' is deterministic and bit-portable -> the Rust port
    must match p05/p50/p95 EXACTLY (to ~1e-9). This script asserts the three are distinct.
  * scipy gaussian_filter default mode='reflect'; the existing Rust gaussian is mode='nearest'. At
    sigma=0.55 the kernel is ~3 taps (truncate=4.0 default), so the mode difference only touches the
    1-2 border rows/cols. Interior is near-exact; border cells differ slightly -> tolerance gate. The
    tanh squash keeps even those border diffs small (output is in ~[-1,1]).
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np

# mountain_world_layer imports sibling flat modules (mountain_pass_network, mountain_synthesis) by bare
# name -- put tools/dem_pack on sys.path before importing it.
sys.path.insert(0, str(Path(__file__).resolve().parent))

import mountain_world_layer as L  # noqa: E402

HERE = Path(__file__).resolve().parent
OUT_FIXTURE = HERE / "fixtures" / "condition_world_fixture.json"

# Field tuning (mirrors export_pass_network_fixture.build_height shape). Centered/normalized to ~unit
# std then scaled, so the field has the broad, structured spread of a raw mountain source -> meaningful
# percentiles. No RNG; pure analytic f(u, v) so re-runs are bit-identical.
AMPLITUDE_MULT = 2.0
FREQ_MULT = 2.0


def build_height(n: int) -> np.ndarray:
    """Deterministic synthetic source field: a ridged sum-of-sines with real, structured spread.

    No RNG -- pure analytic f(u, v) so re-runs are bit-identical. Explicit f64 (this field is the Rust
    port's oracle). Centered/normalized to ~unit std then scaled by AMPLITUDE_MULT, giving a field whose
    p05/p50/p95 are distinct (NOT a flat field, which would make the robust rescale degenerate)."""
    ys, xs = np.mgrid[0:n, 0:n].astype(np.float64)
    u = xs / (n - 1)
    v = ys / (n - 1)
    f = FREQ_MULT
    h = (
        np.sin(u * 9.0 * f) * np.cos(v * 7.0 * f)
        + 0.5 * np.sin(u * 17.0 * f + 1.3) * np.cos(v * 13.0 * f - 0.7)
        + 0.25 * np.sin(u * 31.0 * f) * np.cos(v * 29.0 * f)
    )
    h = (h - h.mean()) / (h.std() + 1e-9)
    return h * AMPLITUDE_MULT


def main() -> None:
    n = 193
    z = build_height(n)

    # VERIFY the field is non-degenerate: the three percentiles must be distinct so the robust rescale
    # (z - p50) / (p95 - p05 + 1e-9) is meaningful. A flat field collapses them and the oracle would be
    # satisfied by any constant output.
    p05 = float(np.percentile(z, 5.0))
    p50 = float(np.percentile(z, 50.0))
    p95 = float(np.percentile(z, 95.0))
    if not (p05 < p50 < p95):
        raise SystemExit(
            f"[cond-fixture] DEGENERATE field: percentiles not strictly increasing "
            f"(p05={p05} p50={p50} p95={p95}) -> robust rescale is degenerate. "
            f"Adjust AMPLITUDE_MULT/FREQ_MULT."
        )
    if not ((p95 - p05) > 1e-3):
        raise SystemExit(
            f"[cond-fixture] FLAT field: p95-p05={p95 - p05} too small -> percentile rescale near-trivial. "
            f"Increase AMPLITUDE_MULT."
        )

    shaped, stats = L.condition_world(z)
    shaped = np.asarray(shaped, dtype=np.float64)

    # Sanity: the conditioned output must use a real fraction of the tanh range (not collapse to ~0),
    # otherwise the gate is vacuous (a zero-output bug would pass against a near-zero oracle).
    if not (float(np.ptp(shaped)) > 0.1):
        raise SystemExit(
            f"[cond-fixture] VACUOUS output: conditioned ptp={float(np.ptp(shaped))} too small."
        )

    payload = {
        "n": int(n),
        "height": z.ravel().tolist(),
        "shaped": shaped.ravel().tolist(),
        "stats": {k: float(v) for k, v in stats.items()},
    }
    OUT_FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    OUT_FIXTURE.write_text(json.dumps(payload), encoding="utf-8")

    smin = float(shaped.min())
    smax = float(shaped.max())
    print(
        f"[cond-fixture] wrote {OUT_FIXTURE} n={n} "
        f"p05={p05} p50={p50} p95={p95} shaped_range=[{smin},{smax}]"
    )


if __name__ == "__main__":
    main()
