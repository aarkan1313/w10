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


def run_pytest_suite() -> int:
    """Run the dem_pack pytest suite from the repo root. Returns process rc (0 = all pass).

    cwd is the REPO ROOT (PROJECT is wg-10/, but tools/dem_pack/ lives at the repo root, and the
    tests use repo-root-relative fixture paths — running from anywhere else collects nothing / fails).
    """
    repo_root = Path(__file__).resolve().parents[1]
    cmd = [sys.executable, "-m", "pytest", "tools/dem_pack/", "-q"]
    res = subprocess.run(cmd, cwd=str(repo_root), capture_output=True, text=True)
    sys.stdout.write(res.stdout[-3000:])
    if res.returncode != 0:
        sys.stderr.write(res.stderr[-3000:])
    fail = res.returncode != 0
    print(f"[gate] suite=pytest fail={1 if fail else 0} (dem_pack pytest, from repo root)")
    return res.returncode


def godot_bin() -> str:
    env = os.environ.get("GODOT_BIN")
    if env and Path(env).exists():
        return env
    raise SystemExit("set GODOT_BIN to the Godot 4.6 console executable")


def ensure_extension_imported(godot: str) -> None:
    """Editor import pass so the GDExtension is discovered and loaded."""
    res = subprocess.run(
        [godot, "--headless", "--import", "--path", str(PROJECT)],
        capture_output=True, text=True,
    )
    # --import can exit non-zero for unrelated import warnings; only treat a
    # missing/failed extension as fatal.
    if "GDExtension dynamic library not found" in (res.stdout + res.stderr):
        sys.stderr.write(res.stdout[-2000:])
        sys.stderr.write(res.stderr[-2000:])
        raise SystemExit("[gate] extension import failed: native lib not found "
                         "(did you `cargo build` the rust crate?)")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--suite", choices=sorted(list(CHECKS) + [PYTEST_SUITE]), default="fast")
    args = ap.parse_args()
    if args.suite == PYTEST_SUITE:
        return run_pytest_suite()
    headless = args.suite not in ("gpu", "m3")   # GPU compute (RenderingDevice) needs a windowed device
    godot = godot_bin()
    ensure_extension_imported(godot)   # the import pass is always headless; that's fine
    failures = 0
    skips = 0
    for script in CHECKS[args.suite]:
        cmd = [godot]
        if headless:
            cmd.append("--headless")
        cmd += ["--path", str(PROJECT), "--script", f"res://{script}"]
        res = subprocess.run(cmd, capture_output=True, text=True)
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
