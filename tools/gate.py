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
    godot = godot_bin()
    ensure_extension_imported(godot)
    failures = 0
    for script in CHECKS[args.suite]:
        res = subprocess.run(
            [godot, "--headless", "--path", str(PROJECT), "--script", f"res://{script}"],
            capture_output=True, text=True,
        )
        tag = "pass" if res.returncode == 0 else "fail"
        if res.returncode != 0:
            failures += 1
        print(f"[gate] check={script} status={tag} rc={res.returncode}")
        if res.returncode != 0:
            sys.stdout.write(res.stdout[-2000:])
            sys.stderr.write(res.stderr[-2000:])
    print(f"[gate] suite={args.suite} checks={len(CHECKS[args.suite])} fail={failures}")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
