# Squama mesher: design

Status: design for M0/M1, revised 2026-10-20 after three adversarial reviews (topology, numerics, requirements; see the review log in §18). This is the contract for `ad3d-core::squama` and the generic `ad3d-core::relief` cell mesher. It fixes the two defects that made every prototype STL fail a watertight check (T-junctions and saddle pinches; see `prototypes/README.md` and PLAN §6). It also makes the output survive slicers that merge nearby vertices (CuraEngine merges within 0.03 mm).

The M0 spike implements §1–§11 for a square grid, one directional light, one rectangular plate, and `Quality::Preview` without the optimiser. §6.2–§6.3 and §12–§16 are the M1 contract; the spike's data types must already have their shape (§12), so M1 adds code, not breaking changes.

## 1. Coordinates and units

- Plate space: `u` to the right, `v` up the plate, `w` out of the front face toward the viewer and the light. Millimetres. Angles are radians in code and degrees in the DTO and UI.
- The solid occupies `back(u, v) ≤ w ≤ surface(u, v)` over the plate outline. Without back regions (§14.3) the back face is the plane `w = 0` and the outline is the rectangle `[0, W] × [0, H]`.
- Export orientation is a separate transform (section 9). The viewport maps plate space to three.js as `x = u, y = v, z = w`.

### 1.1 Tolerances (named constants in `ad3d-core::relief::Tol`)

| Name | Value | Meaning |
|---|---|---|
| `q` | 0.001 mm | Height quantum. Heights are `i64` micrometres. |
| `δ` `MIN_FEATURE` | 0.05 mm | Minimum distance between any two distinct output vertices. It covers CuraEngine's 0.03 mm merge plus its integer-µm rounding, with margin. It is a printer-resolution constant and never scales with cell size. |
| `τ` | 50 µm (= δ) | Height-cluster radius (§5.1). Distinct heights on one vertical line differ by at least `τ + q` = 51 µm. |
| `ε` | 0.05 mm (= δ) | Saddle-split offset (§6.1). The split pair is `2√2·ε` = 141 µm apart. |

All four are fields of `Tol` with these defaults; tests may vary them, the app does not expose them.

## 2. Inputs

| Name | Meaning | Default | Range (generator) |
|---|---|---|---|
| `W`, `H` | Plate size, snapped (§2.1). | W = 300 | `cell ≤ W, H ≤ 2048` mm |
| `cell` | Cell size: square side, or hexagon width across flats. | 6 | `0.5 ≤ cell` (= 10ε) |
| `layout` | `Square`, or `Hex { orientation: Pointy \| Flat }` (M1). | Square | |
| `light_azimuth` φ | Direction the light comes from, in the plate plane. LuBan's Left/Right/Top/Bottom are φ = 180°/0°/90°/270°. Unit vector `a = (cos φ, sin φ)` points from the plate toward the light. | 180° (Left) | any |
| `light_elevation` α | Grazing angle of the light above the plate plane. LuBan: default 5°, range 0–20°. | 5° | 1°–45°; a UI value of 0 maps to 1° |
| `max_tilt` S | Surface angle, the largest facet tilt. LuBan: default 10°, range 1–20°. | 10° | 1°–20° |
| `base` T | Minimum material behind any facet (LuBan "Thickness"). | 3 | `T ≥ 0.4` mm |
| target | One brightness `g ∈ [0, 1]` per facet, in linear light (§16). | | |

With these ranges `α + S ≤ 65° < 90°`, so `E_max = sin(α + S)` is the true maximum and `asin` never sees an argument above 1.

Parameter checks (`Error::InvalidParams` unless marked warn):
- `W, H ≤ 2048` mm, so every coordinate's `f32` half-ulp is ≤ 0.122 µm, over 400 times below δ.
- Relief guard. The centre-pivot relief is `2·e_max·tan S` (§4). Below 8δ = 0.4 mm: **warn** ("relief below printable detail; raise max tilt or cell size"). Below 3δ = 0.15 mm: **error**, because §5.1 collapses almost every vertex to one height. The UI shows this as the smallest allowed `max_tilt` for the chosen cell size.
- Warn when `cell < 3` mm (PLAN §6, unconfirmed) and when `α < 3°`.

### 2.1 Size snapping

All snapping is done in integer micrometres with half-up rounding, never with `f64` division (f64 breaks decimal ties: 100.8/3.2 = 31.499999999999996). Inputs are parsed to `W_um = round(W·1000)`.

- **Square:** `nx = max(1, (W_um + cell_um/2) / cell_um)` (integer division). This reproduces LuBan (200 @ 6 → 198, 202 @ 6 → 204).
- **Square height:** from the **snapped** `nx` and the crop size in pixels: `ny = max(1, (2·nx·crop_h_px + crop_w_px) / (2·crop_w_px))`. If the caller gives `heightMm`, snap it like W and crop the image to fill (centred) instead.
- **Hex:** see §6.2.

## 3. Brightness to tilt (Lambert inverse)

Light vector `L = (cos α · a, sin α)`. A facet tilted by θ toward the light has normal `n = (sin θ · a, cos θ)`, so `L · n = sin(α + θ)`.

- `E_min = max(0, sin(α − S))`, `E_max = sin(α + S)`.
- Sanitise first: a non-finite `g` becomes 0, then `g = clamp(g, 0, 1)`. (A NaN would otherwise reach `round() as i64` and silently become height 0.)
- `E = E_min + g · (E_max − E_min)`.
- `θ = clamp(asin(E) − α, −S, S)`. The clamp only absorbs rounding.

When `α < S`, tilts in `[−S, −α)` are never used (g = 0 already gives E = 0); when `α > S` the darkest level is `sin(α − S) > 0`. Both are physics, not bugs.

The facet gradient is `∇w = −tan θ · a`: a facet facing a light from the left (a = (−1, 0)) rises to the right, as in the prototype.

This is `Response::Lambert { ambient: 0 }`. The general mapping goes through a `Response` (§15.2); the per-cell inverse above is what `Quality::Preview` uses. The shadow-aware optimiser (§15.1) produces the same `Facet` values, so the mesher does not change. After computing heights the generator fails early (`Error::Internal`) if any height is non-finite.

## 4. Facet planes

A **facet** is a logical plane; a **piece** is a convex polygon of the partition that carries one facet (§6). Several pieces may share one facet (hex cells clipped by a tile seam, §14).

```rust
pub struct Facet { pub pivot: DVec2, pub h: f64, pub grad: DVec2 }   // w(p) = h + grad·(p − pivot)
```

- **Pivot:** the centroid of the facet's full cell (for a hex clipped by the plate edge or a seam: the full hexagon's centre, never the piece's centroid).
- **Centre pivot (M0 and default):** every facet has the same `h = c = T + e_max · tan S + q`. Here `e_max` is the largest `|a · (corner − pivot)|` over all facets and full-cell corners. The `+ q` margin keeps every corner at `≥ T` after rounding to the nearest µm (§5).
- **General rule (optimiser, point light):** each facet may have its own `h` and `grad`. The requirement is per corner, not global: every quantised corner height `≥ T_q = ceil(T / q)`, and `≤ T_q + max_relief` when a relief cap is set (§15.1). The mesher asserts `corner_q ≥ T_q` for every corner and returns `Error::InvalidParams` otherwise.

## 5. Height quantisation

- Every corner height is evaluated **once per (facet, 2D partition vertex)** from the facet plane and rounded half-away-from-zero to integer micrometres, stored as `i64`.
- All topology decisions (equal heights, which side is higher, whether two edges cross) use these integers after §5.1, never floats.

Why: float ties (two facets meeting at "almost" the same height) create near-coincident vertices, slivers and pinches. With integer heights "equal" is exact; with §5.1 "different" means at least 51 µm apart.

### 5.1 Height clustering (minimum separation)

At each 2D vertex, take the corner heights of all incident pieces, sorted **descending**. Walk them: the first height opens a cluster with top `t`; every following height `h ≥ t − τ` is set to `t`; the first `h < t − τ` opens a new cluster. Each corner moves **up** by at most `τ`, so `corner ≥ T` is preserved, and distinct heights on any vertical line differ by at least `τ + q`.

- Never snap to the cluster midpoint or the lowest member: the midpoint breaks the gap between clusters, and snapping down can go below T.
- Clustering runs on the whole plate's partition including tile cut lines (§14.1), once, before any tile is meshed. The saddle test (§6.1) and the crossing test (§7.2) use the clustered heights.
- Cost: each facet deviates from its plane by up to `q/2 + τ` at clustered corners, plus `ε√2·tan S` at split vertices (§6.1): at most about 76 µm at S = 20°. Measured local-Lambert brightness error (no shadows; `final/cost.out`): rms 0.5–0.7 % (max 2.3 %) at cell 6 / S 10°, rms 1.8 % at cell 3, rms 3 % (max 10 %) at cell 2, and rms 8 % at relief ≈ 0.1 mm, which is what the relief guard (§2) is for.

## 6. 2D cell partition

```rust
pub struct Piece { pub facet: u32, pub ring: Range<u32> }      // indices into Partition::rings
pub struct Partition {
    pub verts: Vec<DVec2>,          // f64, computed from integer lattice indices where possible
    pub rings: Vec<u32>,            // concatenated CCW vertex loops (seen from +w)
    pub pieces: Vec<Piece>,
    pub outline: Vec<u32>,          // CCW boundary loop
    pub kind: PartitionKind,        // SquareGrid { nx, ny } | Hex { .. } | General
}
```

Requirements (asserted by the partition builder in debug builds and tests; `Error::Internal` on failure):
- every piece is strictly convex and CCW;
- every edge is shared by exactly two pieces (as a twin pair) or lies on the outline;
- every interior vertex has valence 3, or is a square-grid vertex of valence 4 (handled by §6.1);
- every outline vertex is shared by at most 2 pieces;
- every edge is at least 4δ = 0.2 mm long, except the split edges `P1P2` that §6.1 creates;
- distinct 2D vertices are at least δ apart.

The invariant behind these rules (used in §8): **around every vertical line, the pieces that are solid at any height form one contiguous arc**, counting the outside of the outline as a permanently empty piece. Valence 3 always satisfies it; a square-grid vertex satisfies it after §6.1; an outline vertex with 3 pieces, or an interior vertex of valence ≥ 4 that is not a grid vertex, does not (topology review: `boundary_fan` and the star partitions in `docs/wip/spike/design-review/topology/repro_all.out`).

Layouts:
- **Square grid.** `nx × ny` cells, one piece per facet (one per tile when tiled, §14).
- **Hex grid (M1).** §6.2.

### 6.1 Saddle (checkerboard) corners

At a square-grid vertex `P` interior to the piece set being meshed, four cells meet: A (upper-left), B (upper-right), C (lower-right), D (lower-left), with clustered integer heights `hA…hD` at `P`.

At a height `y` on the vertical line through `P`, cell X is solid iff `y < hX`. If at some `y` exactly one diagonal pair is solid, the line is shared by four wall sheets: a non-manifold edge. No exact geometry can fix that. It happens iff

`min(hA, hC) > max(hB, hD)` **or** `min(hB, hD) > max(hA, hC)`.

At such a vertex only, split `P` into two vertices:
- `P1 = P + (−ε, −ε)`, where D, A and C meet;
- `P2 = P + (+ε, +ε)`, where A, B and C meet;
- with `ε = 0.05 mm` (`Tol::eps`). Requires `cell ≥ 10ε`.

Then:
- The A–C diagonal pair now shares the short edge `P1 → P2`.
- The edges that ended at `P` end at whichever of `P1` and `P2` keeps their cells convex: A|D and C|D at `P1`, A|B and B|C at `P2`.
- All four cells keep the heights at the **original** `P`, so the short edge has `d0 = d1` and never a crossing.

The change in geometry is 50 µm per axis and unprintable. After the split every vertex has 3 cells, or 4 cells in a non-saddle arrangement, so the contiguous-arc invariant holds.

The same fixed `(−ε,−ε)/(+ε,+ε)` split is used whichever diagonal pair is the higher one; the reference implementations do this and pass every exhaustive and random test.

### 6.2 Hex layout (M1)

- **Orientation:** pointy-top by default; flat-top is the same construction with `u` and `v` swapped. `cell` is the width across flats; circumradius `R = cell/√3`; row pitch `p = 1.5R = (√3/2)·cell`.
- **Phase:** a hex centre sits at plate `(0, 0)`. Row `j` has centres at `v = j·p`, `u = (i + ½(j mod 2))·cell`.
- **Size snap** (integer µm, half-up as §2.1): `W = m·cell/2` with `m = max(2, round_half_up(2W/cell))`; `H = n·p` with `n = max(1, floor(H/p))` for an explicit height (it snaps down, so a requested height is a limit; M1 verify round 1); from an image aspect, `n = round_half_up(W_snapped·crop_h/crop_w / p)`.
- With this phase and snap, the outline runs only along hex centre lines (vertical lines `u = k·cell/2`, horizontal row lines `v = j·p`) or along hex edges, never near a hex vertex. Every outline vertex then has at most 2 pieces and the shortest partition edge is `R/2` (re-checked: `final/recheck.out` (C), 300 random sizes, 0 failures, classes `boundary:1/2 cells`, `interior:3 cells` only, shortest edge 1.5 mm at R = 3).
- Vertex coordinates are computed from integer lattice indices in units of `cell/2` (u) and `R/2` (v), so clip tests are exact.
- A facet's brightness is the area-weighted mean, in linear light, over its whole region inside the plate (the union of its pieces).
- Tile cuts use the same lines (§14.1).

### 6.3 Normalising other partitions (generic fallback)

M1 layouts never need this, because §6.2 avoids the failing cases. A future partition that has them must be normalised before meshing:
- **Interior vertex of valence k ≥ 4 that is not a square-grid vertex:** replace it by a cap piece, a k-gon with vertices `ε` along each spoke. Spoke pieces keep their height at the original centre on both new corners; the cap is its own flat facet (`grad = 0`) at the minimum of the centre heights. Every new vertex has valence 3. Tested: 0 failures over all weak orders of the centre heights for valence 4, 5, 6 and 1,500 random valence-8 configs, for min and max caps (`topology/generic_fix.out`, ε = 0.05).
- **Outline vertex with 3 pieces:** move the middle piece's corner `ε` inward along the outline's inward normal; the two outer pieces gain that vertex. Tested 0/300 failures on pointy and flat hex clipped through a vertex row (`topology/hex_fix.py`), but only at ε = 0.002; re-run it at `ε = δ` before relying on it.

## 7. Mesh construction

All topology is built from indices. Faces never depend on welding floats.

1. **Vertical-line vertices.** For each 2D vertex `v`, collect the distinct clustered heights of all pieces incident to `v`, plus the back level (`0`) if `v` lies on the outline. Create one 3D vertex per (v, distinct height). A piece's corner at `v` is the 3D vertex for its height there.
2. **Crossings.** For each interior edge `a → b` of length `L` with piece A on its left and B on its right:
   - Let `d0 = hA(a) − hB(a)` and `d1 = hA(b) − hB(b)` (integers).
   - If `d0 · d1 < 0`, the two facet edges cross at `t = d0 / (d0 − d1)`. **Clamp** `t·L` to `[δ, L − δ]` (possible because `L ≥ 4δ`).
   - Create one crossing vertex at `a + t (b − a)`, with height `hA(a) + t (hA(b) − hA(a))` at the clamped `t`. It belongs to both facets and both sub-walls.
   - Topology does not depend on where the crossing lies inside the edge; the clamp costs at most `δ · |slope|` of geometry. For Squama, `|d0|, |d1| ≥ 51 µm` already puts crossings at least `51 µm / (2 tan S)` ≥ 70 µm from the ends (S ≤ 20°), so the clamp matters only for general reliefs (topology review F6: a crossing 5 nm from its end, below one f32 ulp at u = 200 mm).
3. **Facets.** Walk each piece's corners in counter-clockwise order. After each corner, insert that edge's crossing vertex if it has one. Then fan the loop from a centroid vertex at `(centroid, w_facet(centroid))`, where `centroid` is the piece's own centroid (strictly inside a convex piece, so no fan triangle is degenerate even when loop vertices are collinear). Normals point `+w`.
4. **Interior walls** (edge `a → b`, pieces A and B):
   - `d0 = d1 = 0`: no wall. Both facets use the same 3D vertices along the edge, so they share it directly.
   - No crossing, otherwise: one wall.
     - Its side at `a` is every vertical-line vertex of `a` with a height between `min(hA(a), hB(a))` and `max(hA(a), hB(a))`, inclusive. The side at `b` is defined the same way.
     - These intermediate heights are the T-junction fix: every wall that touches a vertical line uses every vertex on that line within its span.
     - A side may be a single point when the heights there are equal (a wedge).
   - Crossing: two sub-walls, `a → X` and `X → b`. Each has one vertical side and the apex `X`.
   - Triangulate each wall as a **ladder** between its two sides, which are points on two different vertical lines: each triangle uses two points on one line and one on the other, so none is degenerate. Indices run bottom to top. At each step, if both sides can advance, advance the side that gives the **shorter new diagonal**: advance R iff `|L_i R_{j+1}| ≤ |L_{i+1} R_j|` (3D lengths in `f64`; ties advance R).
   - **Orientation.** The wall faces from the higher piece toward the lower one. If A is higher, the outward normal is the right-hand normal of `a → b`: emit `(L_i, R_j, R_{j+1})` when advancing R and `(L_i, R_j, L_{i+1})` when advancing L, where L is the side at `a` and R the side at `b`. If B is higher, reverse every triangle.
   - **Which piece is higher** comes from the integer signs only, never from float cross products:
     - wall without a crossing: A is higher iff `d0 + d1 > 0` (without a crossing `d0` and `d1` never have opposite signs and are not both 0, so this equals `sign(d0)` if `d0 ≠ 0`, else `sign(d1)`);
     - crossing: `sign(d0)` for the sub-wall `a → X`, `sign(d1)` for `X → b`.
     - Unit tests: a wedge with its point at `a` (`d0 = 0`) and one with its point at `b` (`d1 = 0`).
5. **Boundary walls.** For each outline edge `a → b` of piece A, emit a ladder whose side at `a` runs from the back level up to `hA(a)` through every vertical-line vertex on the way, and likewise at `b`. The outward normal is the right-hand normal, which points outside the plate.
6. **Back face.** Triangulate the outline polygon at the back level (`w = 0`) with a constrained Delaunay triangulation (e.g. the `spade` crate) whose constraint loop is the outline's boundary vertices in order, with back-region polygons as holes (§14.3). Emit triangles so the normal points `−w`. Its boundary edges are exactly the bottom edges of the boundary walls. Do not fan from the centre: it folds over itself on non-star outlines and makes needle triangles on long plates.

## 8. Why the result is a closed 2-manifold

**Each edge has exactly two faces, with opposite orientation:**
- A facet boundary segment is shared either with the neighbouring facet (when `d0 = d1 = 0`) or with exactly one wall, as its top or bottom edge.
- Fan, ladder and back-face interior edges belong to exactly two triangles of the same face.
- Back-face boundary edges match the bottom edges of the boundary walls.
- Vertical sub-segments: around an interior line, the solid pieces at height `y` form one contiguous arc (§6 invariant), so exactly two neighbouring pairs differ at `y`, which gives exactly two walls. Each of those walls contains that sub-segment, because its side includes every height on the line within its span.
- On an outline line (at most 2 pieces), the segment from the back level to `min(h)` belongs to the two boundary walls, and the segment from `min(h)` to `max(h)` belongs to the taller piece's boundary wall plus the interior wall.

**Every vertex link is a single cycle:**
- Interior corner at a middle height: facet → wall → wall → … → facet.
- Crossing: facet A → sub-wall 1 → facet B → sub-wall 2.
- Base vertex: boundary wall → boundary wall → back face.

**Distinct vertices are at least δ = 50 µm apart** (re-checked: minimum 51.0 µm over 300 random cases and four full-size plates, `final/recheck.out`):
- Heights on one vertical line differ by at least `τ + q` = 51 µm (§5.1).
- 2D partition vertices are at least δ apart (§6), and split vertices `2√2·ε` = 141 µm apart.
- Crossings lie at least δ inside their edges; two crossings on edges that meet at a corner are then at least `δ·√2` (square) or `δ·√3` (hex) apart.
- Centroid vertices lie well inside their convex pieces, whose edges are at least 4δ long (and the δ gate in §10 checks it).

So neither bit-exact welding of `f32` positions nor CuraEngine's 30 µm merge changes the mesh. (Measured with the design as originally written, without §5.1 and with ε = 2 µm: Cura's merge dropped 10–14 % of faces and left 57–395 non-manifold edges on 100×126 plates. With the rules here: 0 melded vertices, 0 dropped faces, 0 bad edges.) PrusaSlicer only merges with tolerance when its exact check fails, so it is unaffected either way.

**No self-intersection:** walls lie in the vertical planes of piece edges, facets lie over their own pieces, every facet corner (and so every facet point) has `w ≥ T ≥ 0.4` mm, and back regions stay at least 0.8 mm below `T` (§14.3).

## 9. Export orientation

```rust
pub enum QuarterTurns { Auto, Q0, Q1, Q2, Q3 }
pub enum Orientation { Standing(QuarterTurns), Flat }
```

- **Standing (default, as LuBan advises):** first rotate the plate in its own plane by `k·90°` (counter-clockwise, about the origin, then translate back to `u, v ≥ 0`), giving `(u', v')`; then map `X = u'`, `Y = −w` (front faces −Y), `Z = v'`, with the bottom edge on `Z = 0`. Every option is a proper rotation (det +1). Example, k = 2: `(u, v, w) → (W − u, −w, H − v)`.
- **Auto:** choose the `k` that minimises `sin(φ + k·90°)`, so the light comes from below (or, for side light, from the lower side) in print space. Then every main step is an upward-facing ledge and downward ledges are capped at about `cell·tan α`. Ties pick the smaller `k`. If the part does not fit the bed with that `k`, try the other `k` with `sin(φ + k·90°) ≤ 0`, then fall back to tiling (§14).
  - Measured (requirements review s4): Top light at α 5°/S 10°, as exported unrotated: max downward ledge 1.03 mm; rotated so the light comes from below: 0.52 mm.
- Every export reports the maximum downward ledge and warns above 0.6 mm. The back carries an engraved UP arrow (§14.3) so a rotated print is hung the right way.
- **Flat:** `X = u`, `Y = v`, `Z = w`.
- The calibration tile (§15.3) uses the same orientation path as the plate.

## 10. `validate()`

Runs per part, in three stages. Every stage runs the full check list; any failure fails `validate()`.

1. **Raw index buffer** (positions in `f64`, no weld): this is what the viewport payload and 3MF see. A facet corner that got a duplicate vertex instead of the shared vertical-line vertex fails here (numerics t10: it passed the old weld-first validate).
2. **As written** (the transformed `f32` values actually written to STL/3MF, §14.4): weld bit-identical positions (treat `−0.0` as `0.0`). The weld must merge **0** vertices, and the counts must equal stage 1. (This is spike criterion C3.)
3. **Slicer merge:** a CuraEngine-style loader (convert to integer µm, merge any vertex within 30 µm of an earlier one, drop faces with repeated indices; reference `docs/wip/spike/design-review/numerics/t2_cura_meld.py`). It must merge **0** vertices.

Checks:
- no degenerate triangles: no repeated indices, area > 0 in `f64`;
- every undirected edge is used by exactly two triangles, in opposite directions (watertight, consistent winding);
- the triangles around each vertex form a single fan (no pinch);
- signed volume > 0 (outward-facing);
- Euler characteristic and connected components as expected (one genus-0 shell per part);
- **minimum distance between distinct vertices ≥ δ** (gate, computed with a uniform grid hash of cell δ);
- every facet vertex (corner, crossing, centroid) has `w ≥ T − q/2`;
- back face: every back triangle has the same 2D orientation sign and `Σ|area|` equals the back's area (catches folds that edge counting misses).

It reports counts, the first offending elements, the minimum distinct-vertex distance and the minimum triangle altitude.

Tests also cross-check results against independent implementations: manifold's `MeshGL` import status, and trimesh's `is_watertight`/`is_winding_consistent` via `tools/check_mesh.py`. The randomised-test domain is §17.

## 11. Performance

- Everything is O(cells). Build with flat `Vec`s, without per-cell allocation.
- Measured mesh size at 100×126 cells (Python reference with the final rules, `final/recheck.out`): about 59k vertices and 118k triangles (cell 2–6, several images). Without §5.1 it was 77–82k vertices and 155–164k triangles. One closed genus-0 shell always has `T = 2V − 4`. Payload size is `64 + 12V + 12T` bytes ≈ 2.1 MB.
- Target ≤ 10 ms for the mesher at 100×126 (part of the drag budget in §13), well inside C2's 50 ms.
- Parallelise across rows with rayon (count, prefix-sum, then fill) only if the criterion benchmark shows it is needed. Output must be bit-identical regardless of thread count.
- Check `cancel` once per row of cells (or every 4,096 pieces for non-grid partitions).

## 12. Module API

```rust
pub struct Cancel(Arc<AtomicBool>);                       // cheap to clone; is_cancelled()
pub enum Error { Cancelled, InvalidParams(String), Internal(String) }
pub enum Quality { Preview, Final }
pub struct Warning { pub code: &'static str, pub message: String }

pub struct LinearLuma { pub w: u32, pub h: u32, pub px: Vec<f32> }  // linear relative luminance, decoded by the app crate
pub struct Layout { pub kind: LayoutKind, pub w_mm: f64, pub h_mm: f64, pub cell_mm: f64, pub nx: u32, pub ny: u32 }
pub enum Light { Directional { az: f64, el: f64 }, Point { pos_mm: DVec3 } }
pub struct Limits { pub max_tilt: f64, pub base_mm: f64, pub max_relief_mm: Option<f64> }
pub struct Mesh { pub pos: Vec<DVec3>, pub tri: Vec<[u32; 3]>, pub kind: Vec<FaceKind> }  // FaceKind: Facet, Wall, Back, Key, Stand
pub struct Part { pub name: String, pub mesh: Mesh, pub transform: DAffine3 }

pub fn layout(p: &SquamaParams, img_aspect: (u32, u32)) -> Result<Layout, Error>;          // §2.1, §6.2
pub fn build_partition(l: &Layout, cuts: &[TileCut]) -> Result<Partition, Error>;         // §6, §14.1
pub fn prepare_target(img: &LinearLuma, opts: &ImageOpts, l: &Layout, p: &Partition) -> Result<Target, Error>;  // §16
pub fn solve_facets(t: &Target, p: &Partition, light: &Light, lim: &Limits, resp: &Response,
                    opt: &OptimiserOpts, q: Quality, cancel: &Cancel) -> Result<Vec<Facet>, Error>;  // §3, §15
pub fn quantise(p: &Partition, f: &[Facet], lim: &Limits, tol: &Tol) -> Result<Heights, Error>;    // §5, §5.1
pub fn mesh_relief(p: &Partition, pieces: &[u32], h: &Heights, back: &[BackRegion],
                   tol: &Tol, cancel: &Cancel) -> Result<Mesh, Error>;                     // §6.1, §7
pub fn squama_preview(p: &SquamaParams, img: &LinearLuma, q: Quality, cancel: &Cancel)
    -> Result<(Mesh, Vec<Warning>), Error>;                                                 // whole plate, one part
pub fn squama_export(p: &SquamaParams, img: &LinearLuma, e: &SquamaExport, cancel: &Cancel,
                     sink: &mut dyn PartSink) -> Result<ExportReport, Error>;              // §9, §14
pub fn validate(m: &Mesh, transform: &DAffine3, tol: &Tol, opts: &ValidateOpts) -> Report; // §10
```

- Generators are pure and deterministic: the same inputs give bit-identical output. They do no file I/O; the app crate resolves `image` paths, decodes, applies EXIF orientation, linearises (§16) and caches `LinearLuma`. `"synthetic"` is generated in the core from the layout.
- Cancellation returns `Err(Cancelled)` with no partial output. `cancel` is checked once per row in the mesher, once per sweep in the optimiser and once per tile or part in export.
- Stages are cacheable by a hash of their inputs: `prepare_target` by (image hash, `ImageOpts`, layout); `solve_facets` by (target, light, limits, response, optimiser); `mesh_relief` is cheap enough not to cache.
- `Quality::Preview`: same cells, Lambert (or measured response) inverse, zero optimiser sweeps, no `validate()`. `Quality::Final`: optimiser when enabled, `validate()` always before export, and in debug builds and tests otherwise.

### 12.1 DTO (JS → Rust)

The spike's v1 DTO is unchanged: `{ jobId, widthMm, cellMm, lightAzimuthDeg, lightElevationDeg, maxTiltDeg, baseMm, image }`. M1 adds only **optional** fields with defaults, so v1 payloads stay valid:

```ts
type SquamaParams = {
  jobId: number; quality?: 'preview' | 'final';                         // default 'preview'
  widthMm: number; heightMm?: number; cellMm: number;                   // height default: from crop aspect
  layout?: { kind: 'square' } | { kind: 'hex'; orientation: 'pointy' | 'flat' };
  lightAzimuthDeg: number; lightElevationDeg: number;                   // v1 fields = directional light
  light?: { kind: 'point'; posMm: [number, number, number] };           // overrides the v1 light fields
  maxTiltDeg: number; baseMm: number; maxReliefMm?: number;
  image: 'synthetic' | string;
  imageOpts?: { crop?: [number, number, number, number];               // normalised x0, y0, x1, y1
               gamma?: number; contrast?: number; brightness?: number; invert?: boolean;
               autoLevels?: [number, number] | null };
  response?: { kind: 'lambert'; ambient: number } | { kind: 'measured'; tiltDeg: number[]; brightness: number[] };
  optimiser?: { kind: 'none' } | { kind: 'shadow'; maxSweeps?: number };
};
type SquamaExport = {
  params: SquamaParams; outDir: string; formats: ('stl' | '3mf')[];
  orientation: 'standing-auto' | 'standing-q0' | 'standing-q1' | 'standing-q2' | 'standing-q3' | 'flat';
  bedMm?: [number, number, number]; marginMm?: number;
  keys?: 'none' | 'pegs'; stand?: boolean; calibrationTile?: boolean;
};
```

### 12.2 Mesh payload (Rust → JS)

The binary layout is the shared spike contract (magic `AD3M`, version 1, header 64 bytes, f32 positions, u32 indices). M1 uses the spare `flags` word: bit 0 = final quality; bit 1 = a `u16` face kind per triangle (§12 `FaceKind`, plus part index in the high byte) follows the indices, padded to 4 bytes. The spike sends `flags = 0`.

The viewport must render with **flat per-triangle normals** (derivative normals in the shader, or de-indexed geometry), never `computeVertexNormals`: facets and walls share vertices, and smoothing would average facet normals with vertical walls (≈ 12° error, ≈ 3.5× shading error at α = 5°). Flat triangle normals show the geometry that will actually print, including the §5.1 clustering. Fan triangles at split vertices are 141 µm slivers whose normals can be up to S off the facet's; they are visually negligible. An analytic per-facet normal mode is optional once face kinds exist.

## 13. Preview and drag budget

- While a slider drags: `Quality::Preview` on a latest-wins queue; on release: `Quality::Final`. Preview uses the **same cells** as final (fewer cells would show a different picture).
- Viewport-only changes (orbiting, moving the viewport preview light for C1) never regenerate. Changing the design light (φ, α) does.
- Budget per stage at 100×126 cells, total ≤ 50 ms: target lookup (cached SAT) ≤ 2 ms; solve (Lambert inverse) ≤ 1 ms; quantise + mesh ≤ 10 ms; encode ≤ 3 ms; IPC ≤ 20 ms (≈ 2.1 MB); JS upload and shadow render ≤ 14 ms.
- Fallback: if the preview mesh would exceed about 300k triangles, drag previews use a coarser grid and show a visible PREVIEW badge; release always shows the real cells.
- The spike measures C5 at 50×63 as specified, and additionally reports the drag rate at 100×126 (not an acceptance criterion).

## 14. Tiling, back profile and parts (M1)

### 14.1 Tiles

```rust
pub struct Bed { pub x_mm: f64, pub y_mm: f64, pub z_mm: f64, pub margin_mm: f64 }
pub struct Tile { pub id: (u32, u32), pub rect_mm: DRect, pub pieces: Vec<u32>, pub transform: DAffine3 }
pub fn plan_tiles(l: &Layout, bed: &Bed, o: Orientation, stand: Option<&StandSpec>) -> Result<TilePlan, Error>;
```

- Facets are solved **once** for the whole plate; tiles never recompute them.
- Square grid: cuts on cell lines with near-equal counts (63 rows → 32 + 31).
- Hex: straight cuts along the §6.2 centre lines (`u = k·cell/2`, `v = j·p`), so tile outlines stay rectangles. A hex cut by a seam becomes two pieces of the same facet (same pivot, same plane), so there is no step across the seam.
- The plate partition is built **with** the cut lines (`build_partition(l, cuts)`), and §5.1 clustering runs once on it. Each tile is then `mesh_relief` over its subset of pieces: a seam vertex sees a subset of the clustered heights, which keeps the ≥ 51 µm gap; the saddle split applies only to vertices interior to the tile.
- The bed is an input; the owner's printer is still an open question (START_HERE), so ask before producing the test STL.

### 14.2 Keys and stand

- Keys: blind holes Ø3.2 × 2.0 mm in the back (back-region pockets, §14.3), 6 mm either side of each seam and at most 40 mm apart, plus separately generated splints with Ø3.0 pegs printed flat. Requires `T ≥ 2.8` mm (pocket floor ≥ 0.8 mm below T); otherwise keys are refused with `InvalidParams`.
- Stand: `StandSpec { slot_mm: max_relief + T + 0.3, base_depth_mm, height_mm }`, a separate part.
- Each tile's id and an UP arrow are engraved as 0.4 mm pockets in the back.

### 14.3 Back regions

```rust
pub struct BackRegion { pub poly: Vec<DVec2>, pub w_mm: f64 }   // w > 0: pocket floor; w < 0: rib face
```

- Region polygons are simple, pairwise at least 1 mm apart, and inset at least 1 mm from the outline, so outline vertices and boundary walls are unchanged (back level 0).
- Pockets: `0 < w ≤ T − 0.8` mm. Ribs: `w < 0` (e.g. −2 mm stiffeners stopping 1 mm short of the tile edges).
- Mesh: the back face (§7.6) gets each region as a hole; each region's floor/face is a CDT of its polygon at `w`, facing `−w`; each region edge gets a vertical quad (2 triangles) between `0` and `w`, facing into the pocket (pockets) or outward from the rib (ribs). Region vertices are ≥ δ apart and region edges ≥ 4δ.

### 14.4 Output and export

- Export produces `Vec<Part>`: tiles, splints, stand, calibration tile. Each part is validated on its own, on the transformed `f32` data actually written (§10 stage 2).
- STL: binary, one file per part, streamed through a `BufWriter` (never built whole in memory for large parts).
- 3MF: `unit="millimeter"`, one object per part, build items carrying the orientation transforms and a bed layout, metadata with the params JSON; coordinates written with shortest round-trip `f32` formatting so 3MF and STL carry identical values.
- Each export also cross-checks by importing into manifold (tests and debug builds).

## 15. Optimiser, response, calibration and light hooks (M1)

### 15.1 Shadow-aware optimiser

- Forward model: `fn shade(p: &Partition, f: &[Facet], light: &Light, resp: &Response, samples_per_piece: u32) -> Vec<f64>`. It rasterises along the light direction with a running-max horizon, so it never divides by `tan α` and handles any azimuth and hex cells. Tests and a UI match score use it too.
- `fn optimise(t: &Target, p: &Partition, light: &Light, lim: &Limits, resp: &Response, opt: &OptimiserOpts, cancel: &Cancel) -> Result<Vec<Facet>, Error>`:
  - processes facets in light order (a topological order of "is up-light of", which exists for disjoint convex cells and a fixed direction, so it works for hex cells and any azimuth, unlike a per-row DP);
  - chooses `(θ_k, h_k)` per facet subject to `T ≤ corner ≤ T + max_relief` at every corner (§4 general rule);
  - refines with a bounded number of sweeps (`maxSweeps`), checking `cancel` per sweep.
- `Quality::Preview` runs zero sweeps. The algorithm itself is specified in a separate optimiser design before M1 work starts; this doc fixes only its interface and constraints.

### 15.2 Response

```rust
pub enum Response { Lambert { ambient: f64 }, Measured { tilt_rad: Vec<f64>, brightness: Vec<f64> } }
```

`θ = resp.inverse(target)`, restricted to the response's monotone range. Target luminance maps into `[ambient + E_min, ambient + E_max]` for Lambert, or into the measured table's range.

### 15.3 Calibration tile

- `fn calibration_tile(spec: &CalibrationSpec, light: &Light, lim: &Limits, l: &Layout) -> (Partition, Vec<Facet>, CalibrationKey)`: for example 12 uniform patches of 4 cells across × 6 cells along the light, using the plate's own pivot rule and print orientation (§9); corner patches at minimum and maximum tilt as black/white fiducials, plus an asymmetric orientation marker; sized to fit the bed.
- `fn fit_response(key: &CalibrationKey, measured: &[f64]) -> Response`, fed by a photo of the lit tile (exposure locked) or by manual entry.

### 15.4 Light

- `Light::Directional` is the M0 model. `Light::Point { pos_mm }` gives each facet its own light vector `L_k`, elevation `α_k`, azimuth `a_k` and irradiance `I_k`.
- Point-light solve (M1.x): `B_hi = min_k I_k·sin(α_k + S)`, `B_lo = max_k I_k·max(0, sin(α_k − S))`, `B_k = B_lo + g_k (B_hi − B_lo)`, `θ_k = clamp(asin(B_k / I_k) − α_k, −S, S)`, `grad_k = −tan θ_k · a_k`. Thickness then uses the §4 per-corner rule, not a global `e_max`. Report the achievable contrast and suggest a lamp distance.
- The data model (`Facet.grad` per facet) supports this now; the solver can wait.

## 16. Image to target

Pipeline order (app crate up to `LinearLuma`, core from there):
1. Decode; apply EXIF orientation.
2. Convert sRGB to linear per channel, then relative luminance `Y = 0.2126 R + 0.7152 G + 0.0722 B` (on linear values).
3. Crop (normalised rectangle).
4. Working copy: area-downsample in linear light to at most 2048 px on the long side (cached).
5. Tone operations, in this order, on the perceptual value `v = srgb_encode(Y)`, through a 4096-entry LUT: invert; auto-levels (percentiles, default (0.01, 0.99), off by default); brightness (−1…1, default 0, added); contrast (0–3, default 1, pivot 0.5); gamma (0.2–5, default 1, `v ← v^(1/γ)`). Then `g_px = srgb_decode(v)`, linear again.
6. Summed-area table of `g_px` (rebuilt on tone changes; ≤ 5 ms at 2048²; while a tone slider drags, use a 1024 px level).
7. Per facet: area mean of `g_px` over the facet's region (O(1) per square cell; exact polygon coverage for hex pieces).

`g` is linear radiance, which is what Lambert reproduces. The prototypes' `g = pixel/255` (code values) made mid-tones 2.3× too bright (requirements review s6). The tone domain defaults must be confirmed with the calibration tile.

## 17. Randomised test domain

Property tests (`proptest`) must cover, and every generated part must pass all three `validate()` stages:
- square and hex layouts, both hex orientations, plate sizes on and off the snap grid;
- any azimuth in [0°, 360°); α at 1° and 45°; S at 1° and 20°; T at 0.4 mm; cells from 0.5 mm (with the relief guard disabled for the test) to 10 mm;
- random per-facet `h_k` and `grad_k` (standing in for the optimiser and point lights), within the §4 per-corner rule;
- adversarial integer heights: ties, near-ties within τ, a saddle at every vertex, wedges with the point at either end, crossings that would land closer than δ to an edge end;
- plate offsets up to 2048 mm (f32 behaviour);
- tiles, seams through hexes, and back regions;
- the generic normalisations of §6.3 on star partitions of valence 4–8.

The Python reference used for the reviews lives in `docs/wip/spike/design-review/` (`numerics/squama_ref.py`, `topology/sqmesh.py`); the Rust tests should port the counterexamples in `topology/repro_all.py` and `final/recheck.py` as fixed regression cases.

## 18. Review log

Reviews: topology (`docs/wip/spike/design-review/topology/`), numerics (`…/numerics/`), requirements (`agent-results.json` key `review:requirements`, scripts in `…/requirements/`). The fixes for the counterexamples were re-checked with `docs/wip/spike/design-review/final/recheck.py` (output `recheck.out`) and the brightness cost with `final/cost.py` (`cost.out`). Re-check results: 300 random cases + four full-size plates, 0 `validate()` failures, 0 Cura merges, minimum distinct-vertex distance 51.0 µm, minimum altitude 48 µm, every corner ≥ `ceil(T/q)`; the F6 ulp crossing with the clamp passes; both wedge cases pass; 0/1,500 adversarial small grids fail after clustering; the hex phase rule gives 0/300 failures.

| # | Review | Severity | Issue | Disposition |
|---|---|---|---|---|
| T1 | topology | blocker | µm features re-welded by Cura's 30 µm merge | **Accepted.** δ = 0.05 mm, clustering §5.1 (snap **up**, per numerics, instead of to the cluster start, so corners stay ≥ T), ε = δ, crossing clamp §7.2, δ gate and Cura stage in §10. |
| T2 | topology | major | "any conforming partition" false (valence ≠ 4, boundary vertices with 3 cells, clipped hex) | **Accepted.** Invariant and builder assertions in §6; hex phase rule §6.2 avoids the cases; cap cell and boundary split kept as the generic fallback §6.3. "Never need splitting" deleted. |
| T3 | topology | major | wall orientation inverts wedges | **Accepted.** §7.4 rule `d0 + d1 > 0`, plus wedge unit tests. |
| T4 | topology | minor | no guard against heights ≤ 0 | **Accepted.** T ≥ 0.4 mm (§2), per-corner assert (§4), validate check (§10). |
| T5 | topology | minor | crossing bound only holds for bounded slopes | **Accepted.** Clamp `t·L ∈ [δ, L − δ]` (§7.2); bound documented as Squama-specific. |
| T6 | topology | minor | §11 size estimate off | **Accepted.** §11 updated with measured counts under the final rules. |
| N1 | numerics | blocker | Cura re-weld (same as T1) | **Accepted**, merged with T1. Chose δ = τ = ε = 0.05 mm rather than 0.04/40 µm/0.025 mm for extra margin over Cura's bucketed merge; re-checked (`final/recheck.out`). |
| N2 | numerics | major | ε = 2 µm below slicer tolerance | **Accepted.** ε = 0.05 mm, `cell ≥ 10ε`, costs restated (§6.1, §5.1). |
| N3 | numerics | major | orientation rule at d0 = 0 (same as T3) | **Accepted**, merged with T3. |
| N4 | numerics | major | validate welds before checking | **Accepted.** Three-stage validate (§10). |
| N5 | numerics | major | clipped hex slivers | **Accepted via R5's phase rule** (no slivers by construction, shortest edge R/2). The snap-and-merge fallback is rejected: unnecessary once the phase is fixed; the builder still asserts edges ≥ 4δ. |
| N6 | numerics | minor | low-relief settings | **Accepted** with thresholds 0.4 mm warn / 0.15 mm error (§2), rather than refusing below 4δ. |
| N7 | numerics | minor | ladder slivers | **Accepted.** Shortest-diagonal rule (§7.4). |
| N8 | numerics | minor | rounding below T | **Accepted.** `+ q` pivot margin and per-corner assert (§4); snap-up never lowers a corner. (`ceil` on c alone would not stop a corner rounding down, so the margin is used instead.) |
| N9 | numerics | minor | T near 0 | **Accepted** with T4. |
| N10 | numerics | minor | NaN g, α = 0 | **Accepted.** Sanitising in §3; α ≥ 1° in the generator (§2). Optional "c from the actual max tan θ" rejected for now: it changes the common pivot height, which the shadow behaviour depends on; revisit with the optimiser. |
| N11 | numerics | minor | planarity claim wrong; sliver fan normals | **Accepted** (bound restated in §5.1). Analytic facet normals in the viewport **rejected as the default**: flat per-triangle normals show the printed geometry (§12.2); analytic mode optional. |
| N12 | numerics | minor | back-face fan needles | **Accepted.** CDT back face (§7.6), which also fixes R1(b). |
| N13 | numerics | minor | f32 wording | **Accepted.** W, H ≤ 2048 mm assert and half-ulp bound (§2). |
| R1 | requirements | blocker | tiling, keys, stand cannot be expressed | **Accepted.** Facet/Piece split (§4, §6), tiles with cut lines in the partition (§14.1), back regions inset from the outline (§14.3; changed from the review's "boundary walls span [back, front]" to keep outline topology unchanged), keys/stand (§14.2; stand slot widened to `max_relief + T + 0.3` because it must hold the whole plate), multi-part output and per-part validate (§14.4, §10). |
| R2 | requirements | major | no API, cancellation, quality | **Accepted.** §12. |
| R3 | requirements | major | DTO too small | **Accepted** as optional v1-compatible fields (§12.1); the spike DTO is unchanged. |
| R4 | requirements | major | export orientation ignores light | **Accepted.** §9 quarter-turn Auto, ledge report. |
| R5 | requirements | major | hex phase and size snap | **Accepted.** §6.2. |
| R6 | requirements | major | optimiser has no model/interface; global e_max | **Accepted** as interface and constraints (§15.1, §4 per-corner rule); the algorithm is deferred to its own design doc. |
| R7 | requirements | major | no calibration tile; Lambert hard-wired | **Accepted.** `Response`, `calibration_tile`, `fit_response` (§15.2–15.3). |
| R8 | requirements | major | image controls, sRGB vs linear | **Accepted.** §16, with one correction: luminance is computed on linear RGB, not as Rec.709 luma on encoded values. |
| R9 | requirements | major | only a distant light | **Accepted** for `Point` (§15.4, data model now, solver M1.x). `Line` light **rejected** for now: approximate a bulb row by its nearest point until physical tests show it is needed. |
| R10 | requirements | major | preview vs final, drag budget | **Accepted.** §13. The 100×126 drag check is added as a reported number, not as a change to the fixed acceptance criteria. |
| R11 | requirements | minor | snap rounding, defaults, ranges | **Accepted.** §2, §2.1. α range is 1°–45° (not 0°) because of N10. |
| R12 | requirements | minor | slicer-welding claim false | **Accepted**, superseded by T1: clustering is the default, not an optional mode. |
| R13 | requirements | minor | smooth normals break preview | **Accepted.** §12.2, face-kind flag bit. |
| R14 | requirements | minor | orientation at one-sided ties | **Accepted**, merged with T3. |
| R15 | requirements | minor | export details | **Accepted.** §14.4, §10 stage 2. |
| R16 | requirements | minor | random-test domain | **Accepted.** §17. |
