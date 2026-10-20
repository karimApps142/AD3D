#!/usr/bin/env python3
"""Independent mesh oracle (trimesh) for AD3D outputs.

Usage:  .venv/bin/python tools/check_mesh.py [--multi-shell] FILE.stl [FILE.stl ...]

Loads each STL without processing, merges coincident vertices, and prints
is_watertight, is_winding_consistent, is_volume, euler_number, body count and
volume. Exit code 1 if any file is not a watertight, consistently wound,
single-body volume with Euler number 2 (one genus-0 shell).

Multi-shell parts (files named `*key_splints*`, or every file with
`--multi-shell`) may have N >= 1 bodies; each must be a genus-0 shell, so the
Euler number must be 2 * N.

Setup (venv is gitignored):
    python3.11 -m venv .venv && .venv/bin/pip install numpy trimesh scipy
"""
import sys

import numpy as np
import trimesh


def check(path, multi=False):
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
    bodies_ok = (res["body_count"] >= 1 and res["euler_number"] == 2 * res["body_count"]) if multi else (
        res["euler_number"] == 2 and res["body_count"] == 1)
    ok = (res["is_watertight"] and res["is_winding_consistent"] and res["is_volume"]
          and bodies_ok and res["volume"] > 0)
    return ok, res


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    args = argv[1:]
    multi_all = "--multi-shell" in args
    args = [a for a in args if a != "--multi-shell"]
    all_ok = True
    for path in args:
        ok, res = check(path, multi_all or "key_splints" in path)
        all_ok &= ok
        print(f"{path}")
        for k, v in res.items():
            print(f"  {k}: {v}")
        print(f"  trimesh check: {'OK' if ok else 'FAILED'}")
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
