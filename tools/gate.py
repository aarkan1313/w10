"""WorldGen10 headless gate runner. Runs *_check.gd scripts via Godot.

Before running the checks it performs one editor `--import` pass: Godot only
loads a GDExtension after it has scanned addons/ and written
.godot/extension_list.cfg, which a bare `--script` run on a clean checkout has
not done. Without this, every check fails with "Wg10Hash not registered".
"""
import argparse
import os
import subprocess
import sys
from pathlib import Path

PROJECT = Path(__file__).resolve().parents[1] / "wg-10"
CHECKS = {
    "fast": [
        "worldgen_terrain/tests/hash_parity_check.gd",
        "worldgen_terrain/tests/determinism_check.gd",
        "worldgen_terrain/tests/grammar_check.gd",
        "worldgen_terrain/tests/height_check.gd",
        "worldgen_terrain/tests/dem_pack_check.gd",
        "worldgen_terrain/tests/facts_check.gd",
    ],
    "gpu": [
        "worldgen_terrain/tests/gpu_parity_check.gd",
        "worldgen_terrain/tests/gpu_parity_dem_check.gd",
        "worldgen_terrain/tests/facts_collision_parity_check.gd",
        "worldgen_terrain/tests/facts_bake_check.gd",
    ],
    # GPU-flow feasibility gate (windowed). A REAL gate now: flow_spike_check returns nonzero on
    # over-budget OR non-convergence. Kept as its OWN suite (not folded into `gpu`) because it
    # measures HARDWARE perf (p99 budget), which is device-dependent — `gpu` gates parity (device-
    # independent). Run on the dev box to confirm live GPU drainage still fits the frame budget.
    "gpu_flow": [
        "worldgen_terrain/tests/flow_spike_check.gd",
    ],
    # Slice-4a per-page cost MEASUREMENT gate (windowed). Decides spec 3.1 pipeline
    # (per-page-live vs coarse-drainage-fact). Measurement gate: succeeds at producing a
    # non-degenerate number; both pipeline outcomes are valid (device-dependent perf).
    "page_measure": [
        "worldgen_terrain/tests/page_measure_check.gd",
    ],
    # Task 4a.3 GLSL noise/warp primitive PARITY gate (windowed). Proves the i64-emulated
    # GLSL lattice hash (uvec2 64-bit wrapping math, since #version 450 has no int64) and
    # the f32 primitives built on it match the f64 oracle (worldgen_proto.py) within an f32
    # budget -- including negative-coord (arithmetic-shift) and large-coord (i64-wrap) paths.
    # PARITY gate (device-independent), but RenderingDevice compute is windowed-only here.
    "biome_page": [
        "worldgen_terrain/tests/primitive_parity_check.gd",
        # Task 4a.5 two-tier parity: the full GLSL mountain apron PAGE pipeline (noise/warp +
        # gaussian + flow relaxation + crop) vs the committed f64 fixture, per record. WINDOWED
        # (RenderingDevice compute). Tier-2 height within a normalized-unit epsilon.
        "worldgen_terrain/tests/biome_page_parity_check.gd",
        # Task 4b.11 COMPOSE parity: the GPU compose layer (blend_field / blend_height_favored /
        # compose_biomes fold) vs the committed f64 fixture (input + weight fields stored directly,
        # so it is independent of recipe noise + grammar). WINDOWED (RenderingDevice compute).
        "worldgen_terrain/tests/biome_compose_parity_check.gd",
    ],
    # Slice-4 DRAINAGE convergence MEASUREMENT (windowed). How many flow relaxation iters does the
    # REAL 576 production page need to converge? Decides whether live-per-page flow fits the budget
    # (-> no coarse-drainage-fact subsystem) or not. Measurement gate (non-degenerate number = pass).
    "flow_converge": [
        "worldgen_terrain/tests/page_flow_convergence_check.gd",
    ],
    "m3": [
        "worldgen_terrain/tests/m3_slice1_check.gd",
        "worldgen_terrain/tests/m3_pool_check.gd",
        "worldgen_terrain/tests/m3_stream_check.gd",
        "worldgen_terrain/tests/m3_view_check.gd",
        "worldgen_terrain/tests/m3_b2_capacity_check.gd",
        "worldgen_terrain/tests/m3_accept_check.gd",
        "worldgen_terrain/tests/m3_continuity_check.gd",
        "worldgen_terrain/m5/m5_detail_check.gd",
        "worldgen_terrain/tests/m5_perf_hardened_check.gd",
    ],
}


# The Python side (offline worldgen: 11 seam-safe biomes, biome_compose/registry,
# recipe_noise/array_ops parity, dem_pack tools). NOT Godot *_check.gd — a pytest run.
# MUST run from the repo ROOT (tests use repo-root-relative fixture paths; running from
# inside tools/dem_pack breaks them -> false failures). This is its own suite so
# "gate green" covers the Phase-5 Python work, not just the Godot render checks.
PYTEST_SUITE = "pytest"
PYTEST_FAST_SUITE = "pytest_fast"
# Bounded fast profile: the port-critical composition-layer tests (seconds), for a quick
# "did the Slice-3 core regress?" gate. The Rust parity oracles (recipe_noise/array_ops/recipes/
# biome_compose) live in `cargo test` and are fast there. `pytest` runs the FULL ~10min synthesis
# suite; `pytest_fast` runs just these.
PYTEST_FAST_PATHS = [
    "tools/dem_pack/test_biome_compose.py",
    "tools/dem_pack/test_biome_registry.py",
]


def run_pytest_suite(fast: bool = False) -> int:
    """Run the dem_pack pytest suite from the repo ROOT (PROJECT is wg-10/, but tools/dem_pack/ is
    at the repo root + tests use root-relative fixture paths). fast=True = bounded port-critical
    subset (seconds); else the full suite (~10min). Both have a timeout so the gate can't hang."""
    repo_root = Path(__file__).resolve().parents[1]
    label = PYTEST_FAST_SUITE if fast else PYTEST_SUITE
    targets = PYTEST_FAST_PATHS if fast else ["tools/dem_pack/"]
    timeout = 180 if fast else 900
    cmd = [sys.executable, "-m", "pytest", *targets, "-q"]
    try:
        res = subprocess.run(cmd, cwd=str(repo_root), capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        print(f"[gate] suite={label} fail=1 (TIMEOUT >{timeout}s)")
        return 1
    sys.stdout.write(res.stdout[-3000:])
    if res.returncode != 0:
        sys.stderr.write(res.stderr[-3000:])
    fail = res.returncode != 0
    print(f"[gate] suite={label} fail={1 if fail else 0} (dem_pack pytest from repo root)")
    return res.returncode


def godot_bin() -> str:
    env = os.environ.get("GODOT_BIN")
    if env and Path(env).exists():
        return env
    raise SystemExit("set GODOT_BIN to the Godot 4.6 console executable")


def ensure_extension_imported(godot: str) -> None:
    """Editor import pass so the GDExtension is discovered and loaded."""
    try:
        res = subprocess.run(
            [godot, "--headless", "--import", "--path", str(PROJECT)],
            capture_output=True, text=True, timeout=180,
        )
    except subprocess.TimeoutExpired:
        raise SystemExit("[gate] extension import pass timed out (>180s) — Godot may be hung")
    # --import can exit non-zero for unrelated import warnings; only treat a
    # missing/failed extension as fatal.
    if "GDExtension dynamic library not found" in (res.stdout + res.stderr):
        sys.stderr.write(res.stdout[-2000:])
        sys.stderr.write(res.stderr[-2000:])
        raise SystemExit("[gate] extension import failed: native lib not found "
                         "(did you `cargo build` the rust crate?)")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--suite", choices=sorted(list(CHECKS) + [PYTEST_SUITE, PYTEST_FAST_SUITE]), default="fast")
    args = ap.parse_args()
    if args.suite == PYTEST_SUITE:
        return run_pytest_suite(fast=False)
    if args.suite == PYTEST_FAST_SUITE:
        return run_pytest_suite(fast=True)
    headless = args.suite not in ("gpu", "m3", "gpu_flow", "page_measure", "biome_page", "flow_converge")   # GPU compute (RenderingDevice) needs a windowed device
    godot = godot_bin()
    ensure_extension_imported(godot)   # the import pass is always headless; that's fine
    failures = 0
    skips = 0
    # Per-check wall timeout so a hung Godot can never stall the gate indefinitely (a hung process
    # is a FAIL with its output tailed, not an infinite wait). gpu/m3/gpu_flow do real GPU work + can
    # be slower, so they get a longer ceiling than the headless CPU checks.
    per_check_timeout = 240 if not headless else 120
    for script in CHECKS[args.suite]:
        cmd = [godot]
        if headless:
            cmd.append("--headless")
        cmd += ["--path", str(PROJECT), "--script", f"res://{script}"]
        try:
            res = subprocess.run(cmd, capture_output=True, text=True, timeout=per_check_timeout)
        except subprocess.TimeoutExpired as exc:
            # Kill the hung Godot, tail whatever it printed, count it a failure (NOT a hang).
            failures += 1
            out = (exc.stdout or "")
            err = (exc.stderr or "")
            out = out.decode() if isinstance(out, bytes) else out
            err = err.decode() if isinstance(err, bytes) else err
            print(f"[gate] check={script} status=fail rc=TIMEOUT (>{per_check_timeout}s)")
            sys.stdout.write(out[-2000:])
            sys.stderr.write(err[-2000:])
            continue
        rc = res.returncode
        if rc == 0:
            tag = "pass"
        elif rc == 2:
            tag = "skip"
            skips += 1
        else:
            tag = "fail"
            failures += 1
        print(f"[gate] check={script} status={tag} rc={rc}")
        # For the gpu suite, always surface the check's own status line (it prints
        # the [wg10-gpu-parity] line to stdout) so pass/skip detail is visible.
        if not headless or rc != 0:
            # print the check's stdout tail (status line + any push_error)
            sys.stdout.write(res.stdout[-2000:])
        if rc != 0:
            sys.stderr.write(res.stderr[-2000:])
    print(f"[gate] suite={args.suite} checks={len(CHECKS[args.suite])} fail={failures} skip={skips}")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
