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
    ],
    "gpu": [
        "worldgen_terrain/tests/gpu_parity_check.gd",
        "worldgen_terrain/tests/gpu_parity_dem_check.gd",
    ],
    "m3": [
        "worldgen_terrain/tests/m3_slice1_check.gd",
        "worldgen_terrain/tests/m3_pool_check.gd",
        "worldgen_terrain/tests/m3_stream_check.gd",
        "worldgen_terrain/tests/m3_rings_check.gd",
    ],
}


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
    ap.add_argument("--suite", choices=sorted(CHECKS), default="fast")
    args = ap.parse_args()
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
