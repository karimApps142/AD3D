#!/usr/bin/env python3
"""C3 sweep: generate Squama plates with the release `ad3d` CLI and validate each
with `ad3d validate --strict` (our validate() including the self-intersection
stage) AND tools/check_mesh.py (trimesh).

A case the generator refuses with "invalid parameters: relief ... below" (the relief
guard, design §2) is an expected refusal, not a mesh failure; it is counted apart.

Usage: .venv/bin/python docs/spike/c3_sweep.py OUT_DIR
Writes OUT_DIR/*.stl (not kept in git) and docs/spike/c3-results.json.
"""
import json
import math
import random
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CLI = ROOT / "target" / "release" / "ad3d"
PY = ROOT / ".venv" / "bin" / "python"
CHECK = ROOT / "tools" / "check_mesh.py"


def cases():
    out = [
        # name, width, cell, azimuth, elevation, max_tilt, base
        ("luban-default-50x63", 300.0, 6.0, 180.0, 5.0, 10.0, 3.0),
        ("c2-100x126", 300.0, 3.0, 180.0, 5.0, 10.0, 3.0),
        ("edge-S20-a0", 300.0, 6.0, 180.0, 0.0, 20.0, 3.0),
        ("edge-S20-a0-az37", 300.0, 3.0, 37.0, 0.0, 20.0, 3.0),
        ("edge-cell1", 120.0, 1.0, 213.7, 5.0, 10.0, 3.0),
        ("edge-cell20", 300.0, 20.0, 301.3, 20.0, 1.0, 3.0),
    ]
    rng = random.Random(20261020)
    for i in range(24):
        cell = round(rng.uniform(1.0, 20.0), 2)
        # keep cell counts sane for 1 mm cells
        width = round(rng.uniform(max(60.0, cell * 8), 300.0 if cell >= 2 else 150.0), 1)
        az = round(rng.uniform(0.0, 360.0), 2)
        if i % 6 == 0:
            az = round(az // 90 * 90 + rng.uniform(1, 89), 2)  # explicitly off-axis
        el = rng.choice([0.0, 20.0, round(rng.uniform(1.0, 20.0), 2)])
        s = rng.choice([20.0, 1.0, round(rng.uniform(1.0, 20.0), 2)])
        base = round(rng.uniform(1.0, 5.0), 2)
        out.append((f"rand{i:02d}", width, cell, az, el, s, base))
    return out


def main():
    od = Path(sys.argv[1])
    od.mkdir(parents=True, exist_ok=True)
    results = []
    for name, w, c, az, el, s, b in cases():
        stl = od / f"{name}.stl"
        if stl.exists():
            stl.unlink()
        gen = subprocess.run(
            [str(CLI), "squama", "--synthetic", "--width", str(w), "--cell", str(c),
             "--azimuth", str(az), "--elevation", str(el), "--max-tilt", str(s),
             "--base", str(b), "--out", str(stl)],
            capture_output=True, text=True)
        val = subprocess.run([str(CLI), "validate", "--strict", str(stl)], capture_output=True, text=True) \
            if stl.exists() else None
        tm = subprocess.run([str(PY), "-I", str(CHECK), str(stl)], capture_output=True, text=True) \
            if stl.exists() else None
        grid = next((l for l in gen.stdout.splitlines() if l.startswith("grid:")), "")
        mesh = next((l for l in gen.stdout.splitlines() if l.startswith("mesh:")), "")
        tms = next((l for l in gen.stdout.splitlines() if l.startswith("timings")), "")
        warn = [l for l in gen.stdout.splitlines() if l.startswith("warning")]
        r = {
            "name": name, "widthMm": w, "cellMm": c, "azimuthDeg": az, "elevationDeg": el,
            "maxTiltDeg": s, "baseMm": b, "grid": grid, "mesh": mesh, "timings": tms,
            "warnings": warn,
            "generateExit": gen.returncode,
            "generateStderr": gen.stderr.strip()[-500:],
            "validateExit": val.returncode if val else None,
            "validateOut": val.stdout.strip() if val else None,
            "trimeshExit": tm.returncode if tm else None,
            "trimeshOut": tm.stdout.strip() if tm else None,
        }
        r["refused"] = gen.returncode == 2 and "invalid parameters: relief" in gen.stderr
        r["pass"] = gen.returncode == 0 and r["validateExit"] == 0 and r["trimeshExit"] == 0
        results.append(r)
        print(f"{name:24s} gen={gen.returncode} validate={r['validateExit']} trimesh={r['trimeshExit']} {grid}")
    n = sum(r["pass"] for r in results)
    refused = sum(r["refused"] for r in results)
    meshed = len(results) - refused
    print(f"PASS {n}/{meshed} meshed cases ({refused} expected refusals by the relief guard)")
    (ROOT / "docs" / "spike" / "c3-results.json").write_text(
        json.dumps({"passed": n, "meshed": meshed, "refusedByReliefGuard": refused, "total": len(results),
                    "validate": "ad3d validate --strict (self-intersection stage on)", "cases": results}, indent=2))
    return 0 if n == meshed else 1


if __name__ == "__main__":
    sys.exit(main())
