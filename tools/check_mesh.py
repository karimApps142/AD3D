#!/usr/bin/env python3
"""Independent mesh oracle (trimesh) for AD3D outputs.

Usage:  .venv/bin/python tools/check_mesh.py FILE.stl [FILE.stl ...]

Loads each STL without processing, merges coincident vertices, and prints
is_watertight, is_winding_consistent, is_volume, euler_number, body count and
volume. Exit code 1 if any file is not a watertight, consistently wound,
single-body volume with Euler number 2 (one genus-0 shell).

Setup (venv is gitignored):
    python3.11 -m venv .venv && .venv/bin/pip install numpy trimesh scipy
"""
import sys

import numpy as np
import trimesh


def check(path):
    m = trimesh.load(path, force="mesh", process=False)
    n_raw = len(m.vertices)
    m.merge_vertices()
    bodies = m.split(only_watertight=False)
    res = {
        "faces": len(m.faces),
        "vertices_raw": n_raw,
        "vertices_merged": len(m.vertices),
        "is_watertight": bool(m.is_watertight),
        "is_winding_consistent": bool(m.is_winding_consistent),
        "is_volume": bool(m.is_volume),
        "euler_number": int(m.euler_number),
        "body_count": len(bodies),
        "volume": float(m.volume),
        "bounds": np.round(m.bounds, 4).tolist(),
    }
    ok = (res["is_watertight"] and res["is_winding_consistent"] and res["is_volume"]
          and res["euler_number"] == 2 and res["body_count"] == 1 and res["volume"] > 0)
    return ok, res


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    all_ok = True
    for path in argv[1:]:
        ok, res = check(path)
        all_ok &= ok
        print(f"{path}")
        for k, v in res.items():
            print(f"  {k}: {v}")
        print(f"  trimesh check: {'OK' if ok else 'FAILED'}")
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
