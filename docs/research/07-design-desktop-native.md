A Python prototype of the Squama-style generator works (details in §0). One experienced developer could build this desktop-native version in about 7 weeks for a Squama-only alpha, 32 to 34 weeks for the 4-feature MVP, about 23 more for v1 and about 31 more for v2. That is roughly 2 years in total, against what LuBan built over 7 years.

# Desktop-native plan for a LuBan-equivalent (clean-room)

## 0. Squama prototype (built and run, 2026-10-19)
I wrote a Squama-style generator in Python from what the video shows. Settings: 200 mm wide, cell 4, light angle 5°, surface angle 10°, thickness 3. Results:

- **Output:** a 50×63 grid, 200×252 mm. The plate is 3.0–3.71 mm deep, which is shallow. It exports as a binary STL.
- **Two ways to build the mesh:**
  - **(a) Merge one small solid per cell with manifold3d.** The result is watertight with consistent face winding (checked by edge counts, without merging vertices). It takes 1.3–4 s for 3,150 cells and 39 s for 28,350 cells (300 mm at cell 2). Most of that time is Python's per-cell loop.
  - **(b) Build the mesh directly.** It is faster (0.5–21 s), but my first attempts were not watertight. I hit three bugs:
    - a vertex sitting in the middle of a neighbouring edge (a T-junction) where two tilted facets cross;
    - inverted wall normals;
    - **a real non-manifold "pinch"**: when diagonal cells are both higher than the other two, four faces share one vertical edge. 62 edges stayed bad. The fix is to duplicate the vertices along that edge, or to merge with manifold.

  This is concrete evidence for risk R2 below.
- **What the shading formula implies:**
  - The formula I assumed (lit facet brightness = sin(light angle + tilt), the standard diffuse-surface model) only ever uses tilts between −light angle and +surface angle. With the defaults that is −5° to +10°. A facet tilted further away is already fully in its own shadow.
  - The brightest facet reaches only sin(15°) ≈ 0.26 of head-on brightness. So printed contrast depends heavily on the material and the real lamp, and a calibration step is worth building in.
- **Files:**
  - Code: `prototypes/squama-rising/squama.py`
  - Output: `prototypes/squama-rising/out/squama_manifold.stl` and `prototypes/squama-rising/out/squama_direct.stl`
  - Rendered vs target image: `prototypes/squama-rising/out/squama_render.png` and `prototypes/squama-rising/out/squama_target.png`
  - Python environment: `(session scratchpad, not copied)/protoenv`

## (a) Architecture
```
+---------------------------- Desktop app: Tauri 2 ------------------------------+
| Web UI (TypeScript + React)                                                    |
|  - Parameter panels built from a JSON schema per generator                     |
|    (Enter regenerates the model; values are validated, not parsed loosely)     |
|  - 3D viewport: three.js + three-mesh-bvh. Orbit, edge/height-colour view,     |
|    movable directional light with shadow map (Squama preview), outline of the  |
|    printer's build volume, exploded/select view of parts                       |
|  - 2D sheet view: SVG preview of nested sheets, cut/engrave layers, labels     |
|  - Job panel: progress, cancel, low-res preview first, then full resolution    |
|        | Tauri commands + binary channel (mesh buffers, never JSON)            |
| Rust host: file dialogs, project files (.json + binary blobs in a zip),        |
|   updater, licensing, job manager, process watchdog                            |
+--------|-----------------------------------------------------------------------+
         | spawn sidecar; framed stdio (JSON header + binary payload); timeouts
         v
+------------------ fabcore: C++20 sidecar process (crash-isolated) -------------+
| api/        request dispatch, cancellation tokens, progress events             |
| generators/ squama | lithophane | stack | modular_cut | hash | relief | ...    |
| geom/       mesh type (manifold MeshGL), weld/repair, plane slice + cap,       |
|             booleans and split-by-plane (manifold), 2D offset and clip         |
|             (Clipper2), simplify (meshoptimizer), remesh/parameterize          |
|             (pmp-library), heightfield mesher, stroke-font labels              |
| layout/     split oversized polygons to sheet size, nesting (packingsolver,    |
|             or sparrow as a 2nd sidecar), bed-fit tests incl. Z-rotation       |
| io/         STL, OBJ, PLY, 3MF (lib3mf), SVG, DXF (own writer), PNG/JPG (stb)  |
+--------------------------------------------------------------------------------+
   ^ the same library exposed to Python through nanobind:
     golden-corpus tests, fuzzing, research notebooks (trimesh/shapely for checks)
   ^ the same binary run as a CLI: batch mode for free (LuBan has a Batch menu)
```

**Why it is built this way:**
- **Tauri rather than Electron or Qt.** A full-stack developer builds UI fastest on the web. Tauri installers are about 10–20 MB, against Electron's 100 MB or more.
  - Electron is the fallback if Linux WebGL in WebKitGTK turns out poor (UNCONFIRMED; test it in week 1).
  - Qt 6 would mean writing all the UI in C++. Its LGPL terms or commercial licence also add friction (Qt commercial pricing UNCONFIRMED).
- **C++ core rather than Rust.** manifold, Clipper2, meshoptimizer, pmp, Eigen and lib3mf are all C++, so a C++ core means no wrapper code to maintain.
- **A separate sidecar process.** Geometry code crashing on a bad STL must not take down the UI.
- **Squama data flow:**
  1. Load the image, convert to grayscale with gamma/levels, area-average into nx × nz cells.
  2. Brightness → tilt angle, using either the diffuse formula or a measured calibration table.
  3. Direct heightfield mesher with crossing-point splits and pinch vertex duplication.
  4. Watertight check, then STL or 3MF.

## (b) Libraries and licences

**Core stack.** Licences come from the GitHub research unless marked † (my own knowledge, not re-checked today).

| Purpose | Library | Licence |
|---|---|---|
| Booleans, plane slice, split-by-plane, capping, guaranteed manifold output | manifold | Apache-2.0 |
| 2D offset, clip, kerf, slots, ring hollowing | Clipper2 | BSL-1.0 |
| Decimation (low-poly for Plate and Wireframe) | meshoptimizer | MIT |
| Remeshing, parameterization, curvature | pmp-library or geometry-central | MIT |
| Linear algebra | Eigen | MPL-2.0 † |
| 3MF export | lib3mf | BSD-2 † |
| Image I/O | stb_image | public domain / MIT † |
| Python bindings | nanobind | BSD-3 † |
| Viewport | three.js, three-mesh-bvh | MIT |
| App shell | Tauri 2 | MIT / Apache-2.0 †; React MIT † |
| Nesting | packingsolver | MIT |
| Nesting | sparrow + jagua-rs | MIT / MPL-2.0 (MPL is fine if kept unmodified or changes published) |
| Raster to vector (Photo Magic, v2) | vtracer | MIT |
| Photo → depth map (v2) | Depth-Anything-V2-**Small** | Apache-2.0 |
| Running that depth model | onnxruntime | MIT † |
| Python prototyping | trimesh (MIT), shapely (BSD-3), manifold3d (Apache), ezdxf (MIT) | — |
| Algorithm reference only (port the idea, not the code) | pychop3d (Chopper approach) | MIT |

**Avoid in a closed-source product:**
- CGAL: GPL unless a GeometryFactory licence is bought. LuBan links it, and I found no licence notice.
- MeshLab, vcglib, libigl's GPL files, potrace, boxes.py, the papercraft unfolders: GPL.
- **Lamina and Kromacut: AGPL.** Do not read their source.
- MeshLib: non-commercial only.
- Depth-Anything Base/Large/Giant weights: CC-BY-NC.
- Hunyuan3D: its licence excludes the EU, UK and South Korea.
- dlib's 68-point face landmark model, which LuBan ships renamed: trained on iBUG 300-W, generally understood as non-commercial.
- pasak3d and Jack000/Deepnest: no licence, so normal copyright applies.

## (c) Roadmap

**M0: Squama alpha, weeks 1–7.** Image in, STL/3MF out. No mesh import is needed yet.
- Why Squama first:
  - the user showed it;
  - no open-source project covers it;
  - the geometry risk is low and the prototype exists;
  - it exercises the whole stack: shell, panel, viewport, light preview, sidecar, export.
- Deliverable: Windows and macOS builds, Left/Right/Top/Bottom light, size snapped to whole cells, aspect ratio from the image, a movable-light shadow preview, and an "orient vertically when printing" hint.

**MVP: 4 features, weeks 8–34.**
1. **Squama** (done in M0) plus a calibration tile: print a 16-step tilt ramp, photograph it under the real lamp, and build a lookup table.
2. **Lithophane:** flat, arc and cylinder, with border, base and hanger. It is expected in this category, reuses the image→heightfield code, and is low risk.
3. **Stack:** stacked laser-cut slices that fit the sheet. Labels, alignment dowel holes, ring hollowing, nesting, SVG/DXF export. This delivers the "fits the machine bed" promise for laser, and since 123D Make and Fusion Slicer died nobody fully serves it.
4. **Modular cut:** recursive cutting until every part fits the printer bed (the method published at CAD'19), capped solids, a 90° Z-rotation fit test, imprinted part numbers, female/female dowel holes, and an assembly-order file. This delivers the same promise for 3D printing.
- Infrastructure that lands in the MVP: mesh import and repair, 2D sheet view, project files and undo, code signing, updater, crash reports, licensing.

**v1: about 23 more weeks.**
- Hash (waffle, then radial), Relief, Contour, Wireframe 2D and 3D.
- Plug connectors (terrace and pyramid, with draft and tolerance).
- Multi-printer load balancing, more lithophane shapes.
- Squama v1: hexagonal and triangular cells, and using shadows cast between neighbouring cells to reach the full ±tilt range.

**v2: about 31 more weeks.**
- Plate (low-poly panels with finger joints).
- A Photo Magic subset: extrusion, cookie cutter, box, shadow-art morphing, pillar, halftone cell, puzzle.
- Eyeglasses, ML depth→relief, Fence/Projection, tattoo lithophane (photo wrapped onto any model), natural cut, translations.

## (d) Effort: one experienced developer, in developer-weeks
Confidence: H = ±20%, M = ±40%, L = could double. Each estimate already includes about 30% Python prototyping for the algorithm-heavy features.

| Item | Weeks | Confidence | Notes |
|---|---|---|---|
| Tauri shell, schema-driven panels, Enter-to-regenerate, job cancel | 2.5 | M | |
| 3D viewport, light/shadow preview, bed overlay | 2 | M | |
| fabcore skeleton: CMake/vcpkg, CI on Win/macOS arm64+x64/Linux, sidecar protocol, nanobind | 2 | M | |
| Squama, production version | 2 | H | Native mesher with pinch handling; the prototype exists |
| Squama calibration lookup table and test tile | 0.5 | M | |
| STL/3MF/SVG/DXF export, project save/load | 1.5 | H | |
| Mesh import (STL/OBJ/PLY/3MF) and repair pipeline | 3 | L | Weld, remove degenerates, orient, fill holes; voxel remesh fallback |
| Lithophane: flat, arc, cylinder | 2 | H | |
| Stack: slice, loops, rings, dowels, split to sheet, labels | 5 | M | |
| Nesting integration (packingsolver or sparrow spike, then fallback) | 2 | M | |
| 2D sheet view | 1 | H | |
| Modular cut, MVP scope | 4 | M | |
| Signing/notarization, updater, crash reporting | 1.5 | M | |
| Licensing/payments (if commercial) | 1.5 | M | |
| Golden test corpus, fuzz tests, MVP buffer | 4 | — | |
| **M0 + MVP total** | **≈32–34** | **M** | About 7.5–8 months. M0 alone is about 7 weeks |
| Hash, then radial | 5 + 1.5 | M/L | |
| Relief (plus bas-relief depth compression) | 1.5 (+1.5) | H (M) | |
| Contour | 1 | H | |
| Wireframe 2D / 3D | 1 / 2 | H / M | |
| Plug connectors | 3 | M | |
| Multi-printer | 1.5 | M | |
| More lithophane shapes | 2 | M | |
| Squama v1 | 1.5 | M | |
| **v1 total, including about 3 weeks buffer** | **≈23** | | |
| Plate | 6 | L | |
| Photo Magic set (7 methods) | 7 | M | |
| Eyeglasses | 4 | L | No open-source reference; hinge and fit parameters |
| ML depth | 1.5 | M | |
| Fence/Projection | 2 | M | |
| Tattoo lithophane | 3 | L | |
| Natural cut | 3 | L | |
| Translations | 1 | H | |
| **v2 total, including buffer** | **≈31** | | |

## (e) Top technical risks and mitigations
1. **Messy real-world meshes** (open, non-manifold, self-intersecting) break slicing, capping and booleans.
   - Run every input through a repair step that converts it to a manifold solid.
   - Fall back to rebuilding the mesh from voxels (OpenVDB; licence MPL or Apache depending on version, UNCONFIRMED).
   - Show the user a clear report, and keep a corpus of 30 or more broken test meshes I own or that are CC0.
2. **Degenerate topology in generated geometry** (proven by the prototype: T-junctions, exact-equal heights, diagonal pinches).
   - Every generator's output must pass a watertight and consistent-winding check in CI, with randomized parameter fuzzing.
   - Break exact ties with a 1e-4 mm offset, or duplicate vertices at pinches, or send the result through manifold.
3. **Interactivity.** Python took 20–40 s at 28k cells. Use native code, a low-res preview first, cancellation, and C++ loops parallelized with TBB or OpenMP.
4. **Physical fit** of kerf, slot and dowel tolerances.
   - One tolerance setting per material, with a generator for test coupons.
   - Positive tolerance for laser and negative for FDM printing, following LuBan's approach.
5. **Squama looks different in reality**: plastic is not a perfectly matte surface, layer lines show, ambient light washes it out. Use the calibration table, the shadow-map preview and vertical-print guidance.
6. **Nesting quality and speed.** Give the solver a time limit. If it runs out, fall back to bounding-box shelf packing.
7. **Hash slots must actually assemble.** Use one insertion direction per family of slices, check the order, and flag pieces that intersect nothing.
8. **Webview differences and large mesh transfers.**
   - Send meshes as binary buffers and use level-of-detail for display.
   - Test WebKitGTK in week 1; Electron is the plan B on Linux.
9. **Crashes on bad input.** The sidecar keeps them isolated; add a watchdog with timeouts.
10. **Scope creep.** LuBan has about 100 methods. Commit only to the roadmap above.
11. **Antivirus false positives.** LuBan is unsigned and its site warns users about AV alerts. Budget for Authenticode and Apple Developer ($99/yr) signing.

## (f) What we must NOT copy
- **No LuBan code, binaries or extracted artefacts.**
  - Do not decompile or disassemble LuBan.
  - The implementing developer must not see the earlier binary-strings dumps, the internal "MU" names or the source paths. Example: `(session scratchpad, not copied)/win_strings.txt`.
  - Work only from a behaviour spec written from public videos, the public manual's descriptions of behaviour (not its wording) and the academic papers.
  - LuBan's EULA terms on reverse engineering are UNCONFIRMED.
- **The name.**
  - Not "LuBan". Snapmaker holds the US LUBAN mark (Reg. 6,494,587) in the same market.
  - Avoid "Squama" as a feature name too. Its trademark status is UNCONFIRMED, and a generic name like "Facet Shade" is safer.
  - Run a clearance search in Classes 9 and 42.
- **Assets.**
  - No icons, screenshots, UI layout or menu tree, manual text, gallery images or sample models.
  - No language files, eyeglasses data, or the bundled dlib/iBUG face model.
- **File formats and licensing.**
  - Do not clone .fgs/.fgd/.shp or the MU_License scheme. Use our own formats plus open STL, 3MF, SVG and DXF.
- **What is fine to reuse:**
  - Generic engineering terms (cell size, light angle, tolerance, thickness).
  - Algorithms published in the papers: CAD'15/16/19, Chopper, Schwartzburg & Pauly 2013, Reliefs as Images, ShadowPix.
- **Patents.**
  - No Chen or Sass patent was found. Freedom to operate is still UNCONFIRMED.
  - Autodesk's US9754412B2 (model slicing with connection points plus material-minimizing layout, active until about 2033) needs a patent attorney's review before Stack and Hash ship in the US.
- **Artist credit.** Credit the shadow-shading idea (João Paulo Gonçalves's "Shadow Pointillism") generically. Do not use his name or artworks in marketing.