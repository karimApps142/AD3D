# Prototypes (reference only)

These are throwaway Python prototypes from the research session. They show that the Squama method works, but **they are not print-clean**. Every STL here, the manifold-union ones included, fails trimesh's watertight check after welding, because of T-junctions and saddle pinches where neighbouring facets cross. The real implementation goes in the Rust core (see `docs/PLAN.md`).

| Folder | What |
|---|---|
| `squama-centre-pivot/` | Facets pivot about their cell centre, as in the video. `squama.py <out_dir>` writes direct-mesh STLs for cell sizes 6, 4 and 2 (it builds its own target image) and prints timings. `shadow.py` / `shadow2.py` simulate how facets shadow their neighbours. `chk.py` / `own.py` weld the mesh and list non-manifold edges. All four take this folder's path as their first argument. The `squama_union_c*.stl` files come from the manifold-union path. |
| `squama-rising/` | Facets rise from the base. `squama.py [image] [out_dir]` (pass `""` for the built-in target) compares the manifold-union and direct meshers and reports how well Lambert shading matches the target. Sample outputs are in `out/`. |
| `wasm-bench/` | manifold-3d 3.5.4 WASM benchmark from the dropped web plan. It showed batch-union of a Squama grid takes 5.4 s / 23.8 s / 162 s at 50×63 / 100×126 / 150×189 cells. That is why Squama must use a direct mesher. Run `npm i && node bench.mjs`. |

Timings measured on the dev Mac (Intel i9-9980HK), 2026-10-19:
- `squama-centre-pivot`, direct mesh: 1.9 s at 50×63 cells, 6.6 s at 100×126.
- `squama-centre-pivot`, manifold union: 0.2 s at 50×63, 1.4 s at 100×126.
- `squama-rising`, direct: 0.8 s at 50×63.

The target for the Rust mesher is < 50 ms at 100×126.

## Run

```sh
python3 -m venv .venv && . .venv/bin/activate
pip install -r requirements.txt
python squama-rising/squama.py "" /tmp/sq-out
```
