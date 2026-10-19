# AD3D plan

## 1. Architecture

**Pick: a Tauri 2 desktop app. The UI is React + TypeScript with react-three-fiber. Geometry runs in a native Rust core.**

```
┌──────────────────────── AD3D.app (Tauri 2) ────────────────────────┐
│  WebView UI (React + TS)                                           │
│   • parameter panels generated from each generator's schema        │
│   • 3D viewport: react-three-fiber + three-mesh-bvh                │
│     – movable directional light with shadow maps (Squama preview)  │
│     – backlight preview for lithophanes                            │
│   • 2D sheet view (SVG) for laser / CNC parts                      │
│   • state: zustand, undo/redo, project file (.ad3d = JSON + assets)│
│        ▲  binary IPC (tauri::ipc::Response → ArrayBuffer)          │
│        │  progress + cancel (tauri::ipc::Channel)                  │
│  Rust app crate (src-tauri)                                        │
│   • job queue: latest-wins, cancellable, preview vs final quality  │
│   • file I/O, dialogs, menus, export streaming                     │
│        ▼                                                           │
│  ad3d-core (Rust library crate, no UI deps)                        │
│   • generators: squama, lithophane, stack, hash, cut-to-bed, …     │
│   • mesh: builder, weld, validate (watertight/winding), repair     │
│   • io: STL (binary), 3MF, OBJ, SVG, DXF                           │
│   • rayon for parallel work; glam for maths                        │
│   • FFI where needed: manifold (C API), Clipper2                   │
└────────────────────────────────────────────────────────────────────┘
   ad3d-cli (Rust binary): the same generators run headless for
   tests, benchmarks and batch jobs
```

### Why this one

| Option | Verdict |
|---|---|
| **Tauri 2 + React UI + Rust core** | ✅ The core runs at native speed on all cores. The app is small (around 10–20 MB) and starts fast. The UI uses skills the owner already has (React/TS). It runs on macOS and Windows from one codebase, and the universal macOS build covers Intel and Apple Silicon. |
| Electron | ❌ Large and memory-hungry. It runs geometry in JS or Node, which fails "fast and optimized". |
| Rust-native UI (egui + wgpu) | Fastest viewport, but polished panel-heavy UIs are harder to build, and the owner can't tweak it easily. Keep it as the fallback if the WebView viewport fails the spike. |
| Qt / C++ | The professional standard and what LuBan-class apps use. It brings heavy C++ build and packaging overhead for a personal tool. |
| SwiftUI + Metal | Best on the Mac, but Mac-only. It rules out a Windows desktop. |

### Key design points

- **Mesh transfer:** the core returns packed `Float32Array` positions + `Uint32Array` indices as raw bytes, which become a three.js `BufferGeometry`, with no JSON in between. Very large meshes get decimated for display; export always uses full resolution.
- **Live preview, no Enter key** (LuBan makes you type a value and press Enter):
  - While a slider is being dragged, regenerate at preview resolution on a latest-wins queue that cancels stale jobs.
  - On release, regenerate at full resolution.
- **Generators are pure and deterministic**, so results can be cached by a hash of their parameters. Undo/redo then just restores the parameters.
- **Validation everywhere:** every generated mesh goes through `validate()` (edge-manifold, watertight, consistent winding, no degenerate triangles) in debug builds and in tests.
- **Library choices** (confirm in the spike):
  - manifold through its C API (check crates.io for maintained bindings first; otherwise bindgen + cmake in `build.rs`).
  - Clipper2: the `clipper2` crate, or pure-Rust `i_overlay`.
  - `image` + `fast_image_resize` for photos.
  - `stl_io`, or a hand-rolled binary STL writer; 3MF via the `zip` crate + XML.
  - Later: `ort` (onnxruntime, CoreML on Mac) for Depth-Anything-V2-Small depth from photos.
- **Personal use:** GPL/AGPL code (CGAL, Lamina, deepnest, meshfix) may be linked, ported or read.

## 2. Spike: prove the stack first (aim for 2–4 days)

**Acceptance criteria.** Measure on the owner's Intel i9 MacBook Pro and report the numbers:

1. A Tauri 2 app launches, with a react-three-fiber viewport and a movable directional light casting shadows.
2. The Rust Squama mesher, release build, makes a 100×126-cell plate in **< 50 ms**. For comparison, the Python direct mesher took 6.57 s and manifold-union 1.41 s.
3. The output passes our `validate()`: watertight, consistent winding, and no pinched vertices once coincident vertices are welded.
4. A 1M-triangle mesh goes from Rust to the viewport in **< 150 ms**.
5. Dragging a slider on a 50×63 plate updates the preview at **≥ 30 fps**.
6. `tauri build --target universal-apple-darwin` produces a runnable `.app`.

If (1), (4) or (5) fails because of the WebView, fall back to egui + wgpu and keep `ad3d-core` unchanged. If (2) or (3) fails, fix the mesher, because the architecture isn't the problem there.

**Prerequisites to install:** `rustup` (stable, plus the `aarch64-apple-darwin` target for universal builds) and `brew install cmake`. Node 22, pnpm 10 and the Xcode Command Line Tools are already installed.

## 3. Roadmap

The effort ranges are the research report's per-feature estimates for one developer, including physical test prints (see `docs/research/00-final-report.md` §3). Personal scope removes the report's licensing, signing, updater and accounts work.

| Milestone | Scope | Report's effort range |
|---|---|---|
| **M0 Spike** | Section 2 | 2–4 days |
| **M1 Squama** | All LuBan parameters, plus the improvements below; STL/3MF export; tiling to the bed with alignment keys and a stand; calibration tile | 3–5 wk (+ extras 3–6) |
| **M2 Lithophane** | Flat, arc, cylinder/cone/vase and sphere first; backlight preview; base/socket/hanger | 2–4 wk (+ more shapes 2) |
| **M3 Mesh in → cut to bed** | Import + repair (on the critical path for every mesh feature); modular cut; connectors; labels; assembly sequence | 4–8 + 4–8 + 5–10 wk |
| **M4 Laser slicing** | Stack, Hash (waffle) and Contour; kerf; measured sheet thickness; nesting; SVG/DXF; CNC dogbones | 5–9 + 8–14 + 3–7 wk |
| **M5 Photo Magic** | Box, Cutter, Extrusion, Layering, Cell, Morphing | 3–8 wk |
| **Later** | Relief (AI depth), wireframe, Plate, Projection (Fence/Pandora), Mold, Thermal, eyeglasses, natural cut, tattoo lithophane | see report |

Write the full feature list with every parameter into `docs/spec/FEATURES.md`, built from the manual, before M1.

## 4. Better than LuBan

- **Native and fast:**
  - Universal macOS build (Intel + Apple Silicon), Windows later.
  - Every generator uses all cores.
  - Jobs can be cancelled, so the UI never freezes.
- **Live preview:** sliders update the model while you drag. There is no "type, then press Enter".
- **Realistic preview:**
  - Real shadow maps and a movable light for Squama.
  - Light shining through the plate for lithophanes.
  - An ambient-light slider that shows how much room light washes the image out.
- **Smarter Squama:**
  - A shadow-aware tilt optimiser. The research found self-shadowing between neighbouring cells limits image accuracy (0.89 match at 5°/10°).
  - Light from any side or angle.
  - Hexagonal cells.
  - Image controls: gamma, contrast and crop.
  - A calibration tile that prints a grey ramp so you can tune for your room.
  - Automatic tiling to the bed with alignment keys and a stand.
- **Projects, not one-offs:** undo/redo, saved project files, presets per printer, laser and material, and recent files.
- **Machine profiles:**
  - Printer bed size.
  - Laser kerf and measured sheet thickness.
  - CNC tool diameter, which drives dogbones automatically.
- **Better exports:**
  - 3MF with units and multiple parts (LuBan has none).
  - Streamed binary STL.
  - SVG/DXF with separate cut and engrave layers.
  - PDF assembly guides.
- **Mesh doctor:** shows what's wrong with an imported STL (holes, non-manifold edges, self-intersections) and fixes it.
- **Batch / CLI:** run any generator from a script, for example a lithophane for every photo in a folder.
- **Modern UX:** drag-and-drop, keyboard shortcuts, a command palette, dark/light themes, and per-feature help drawn from our spec.

## 5. Testing strategy

- **Core unit tests:**
  - Every generator: `validate()` passes for randomised parameters (property tests with `proptest`).
  - Bounds and volume match expectations.
- **Golden tests:** fixed inputs, with checksummed output meshes and SVGs, kept under `tests/golden/`.
- **Benchmarks:** criterion for each generator at preview and full resolution, so speed regressions show up.
- **Mesh corpus:** 30–100 CC0 meshes, including deliberately broken ones, for import and repair.
- **Physical tests:** the owner prints or cuts, using the handed-over files and checklist. Record results in `docs/test-log.md`.

## 6. Known risks

- **Generated mesh topology:** both Python prototypes fail a watertight check (T-junctions and saddle pinches where neighbouring facets cross). The Rust mesher must split side walls at crossings and break height ties with an epsilon.
- **Squama in real life:**
  - Facet relief is only about 0.35–0.7 mm.
  - Cells under about 3 mm may not show (UNCONFIRMED).
  - It needs a dark room and a low-angle light.
  - Printed upright, a large plate needs tiling.
  - So: calibration tile first.
- **Dirty imported STLs** break slicing and booleans. Repair everything on import, with a voxel remesh as the last resort.
- **WebView viewport limits** on multi-million-triangle scans. Decimate for display; egui + wgpu remains the fallback.
