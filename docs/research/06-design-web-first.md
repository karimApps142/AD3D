# Web-first plan for a LuBan-equivalent (browser app, client-side geometry)

## 0. Two corrections to the research, found while prototyping Squama

I wrote a clean-room Squama prototype with Python plus manifold3d (the same engine as the npm package `manifold-3d`) to check the geometry before estimating it.

**1. Brightness and tilt direction.** A facet that faces a light coming from −X at grazing elevation α rises toward +X. Brightness follows Lambert's law: E = sin(α+θ). Invert it to get the tilt: θ = asin(E) − α, with θ limited to [−S, +S]. Here S is "Surface angle" and α is "Light angle". My first draft had the tilt sign backwards. This is a reconstruction, not LuBan's formula (UNCONFIRMED).

**2. Cast shadows are the real Squama problem.** A facet's high edge throws a shadow about step/tan α long onto the facets after it. I simulated a 50×63 grid with a 1D ray march and measured how well the result matches the target image:

| Strategy | α / S | Match to target (correlation) | Max relief |
|---|---|---|---|
| Each facet pivots on its centre (seems to be what LuBan does, "small steps") | 5° / 10° | 0.89 | 4.5 mm |
| Same | 5° / 20° | 0.80 | 5.6 mm |
| Same | 10° / 10° | 0.95 | 4.5 mm |
| Continuous profile, no drops | 5° / 10° | 0.999 | 29 mm (too thick) |
| Continuous, naive reset at 8 mm | 5° / 10° | 0.70 | worse than pivoting |

- So a centre-pivot facet with a shadow-aware preview is the right MVP.
- A per-row optimiser that hides the height resets inside dark cells is a v1 feature LuBan does not appear to have (UNCONFIRMED).

**Mesh construction, both ways checked:**
- **Union approach.** Make one prism per cell plus a base slab, then `Manifold.batch_boolean(Add)`. Status was NoError at every size tested:
  - 50×63 cells: 14.5k triangles, 0.2–1.0 s natively;
  - 100×126 cells: 54k triangles, 0.6–4.2 s.

  Timings varied with machine load. WASM speed is UNCONFIRMED; assume 1.5–3× slower.
- **Direct O(n) construction.** The volume matches the union exactly (428,526 mm³). Getting it right needs three things:
  - split walls where neighbouring heights cross;
  - insert up to 4 heights where cells meet at a grid corner, to remove T-junctions;
  - build walls as earcut polygons.

  About 200 pinch edges remain where four cells form a saddle. They need an epsilon or a separate-topology rule.
- **Decision:** ship the union approach in the MVP. Move to the direct build only if the union is too slow in WASM.

## (a) Architecture

```
┌──────────────────────────── Browser (all core features work offline) ───────────────────────────┐
│ Next.js (marketing/docs/auth pages; editor route = client-only, dynamic import ssr:false)       │
│ ┌──────────────── UI thread ────────────────┐      ┌──────── Geometry worker pool (N=cores-1) ────────┐ │
│ │ React + zustand (project state, undo)     │      │ Comlink RPC, Transferable ArrayBuffers           │ │
│ │ react-three-fiber + drei viewer           │◄────►│ manifold-3d WASM: booleans, splitByPlane,        │ │
│ │  · DirectionalLight + shadow map          │ jobs │   trimByPlane, slice→CrossSection, extrude,      │ │
│ │    (Squama / lithophane light preview)    │ +    │   levelSet (SDF remesh fallback), status()       │ │
│ │  · 2D sheet view (SVG) for laser parts    │cancel│ CrossSection (Clipper2 inside manifold)/         │ │
│ │  · parameter panels (schema-driven)       │      │   clipper2-wasm: offsets, kerf, slots            │ │
│ │ Image tools: crop, levels, gamma, thresh  │      │ earcut, polylabel, meshoptimizer (simplify)      │ │
│ └───────────────────────────────────────────┘      │ Generators (TS, pure functions):                 │ │
│        │ import STL/OBJ/PLY/3MF (three loaders)    │  squama, lithophane, stack, modularCut, hash,    │ │
│        ▼                                            │  relief, wireframe, plate, eyeglasses …          │ │
│ Import pipeline (worker): weld → manifold check →   │ Nester worker: skyline (MVP) → NFP/GA (SVGnest   │ │
│  repair-lite → levelSet remesh fallback → report    │  algorithm port) or sparrow/jagua-rs WASM        │ │
│        │                                            └──────────────────────────────────────────────────┘ │
│        ▼                                                                                                 │
│ Exporters (worker): binary STL (streamed), 3MF (fflate zip), SVG, DXF (@tarikjabiri/dxf),               │
│  zip bundles (one STL per part + Sequence.txt + settings.json)                                           │
│ ML (optional, in-browser): transformers.js + onnxruntime-web WebGPU → Depth-Anything-V2-Small            │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────┘
        │ optional HTTPS
┌───────▼───────── Server (thin, optional) ─────────┐
│ Auth + licensing (Postgres/Supabase), Stripe      │
│ GPU job queue (v2): TripoSR (MIT) image→mesh,     │
│  large depth models; returns GLB/STL              │
│ Static hosting with COOP/COEP headers             │
└───────────────────────────────────────────────────┘
```

**Key rules:**
- Every generator is a pure function `(params, inputs) → {meshes[], sheets[], report}`. It runs in a worker and can be cancelled through a job token.
- The preview runs first at low resolution, then refines.
- The host must serve COOP/COEP headers so SharedArrayBuffer works for multithreaded WASM. There is a single-threaded fallback.

## (b) Libraries

All were checked with `gh` or npm on 2026-10-19.

| Purpose | Library | License |
|---|---|---|
| Viewer | three.js (+ STLLoader, OBJLoader, PLYLoader, 3MFLoader, STLExporter), react-three-fiber, drei | MIT |
| State and RPC | zustand (MIT), Comlink (Apache-2.0) | permissive |
| Solid geometry | manifold-3d 3.5.4 (Apache-2.0) | main engine; manifold output guaranteed |
| 2D offsets and booleans | manifold CrossSection; clipper2-wasm 0.4.0 (BSL-1.0) for open-path or kerf work | permissive |
| Triangulation, label placement | earcut (ISC), mapbox/polylabel (ISC, LICENSE read) | permissive |
| Simplify | meshoptimizer (MIT) | for wireframe and plate |
| Text | opentype.js (MIT) for embossed labels; Hershey single-stroke font data (Hershey license; do not use the GPL-2 hershey-fonts repo code) for laser engraving | check attribution |
| Vectorise | vtracer (MIT, Rust→WASM); OpenCV.js (Apache-2.0) for contours and thresholding | permissive |
| Nesting | SVGnest algorithm (MIT) port; sparrow (MIT) + jagua-rs (MPL-2.0, use unmodified) to WASM in v1 | permissive or weak copyleft |
| Export | fflate (MIT) for 3MF and zip; dxfjs/writer (MIT) | permissive |
| Depth ML | transformers.js (Apache-2.0), onnxruntime-web (MIT), onnx-community/depth-anything-v2-small (Apache-2.0, confirmed through the HF API). Do not use the Base/Large/Giant weights (CC-BY-NC) | permissive |
| Image→3D (v2, server) | TripoSR (MIT) | permissive |
| Algorithm references only (do not copy code) | pychop3d (MIT, Chopper; code use is allowed), CAD'15/'16/'19 papers, Sass 2025, VSA paper | – |

## (c) Roadmap

**Phase 0, spike (1 wk):**
- Next app, an R3F viewer, and manifold-3d WASM inside a Comlink worker, with COOP/COEP set up.
- Port the Squama prototype to TS and measure WASM timing at 50×63 and 100×126 cells.
- Go/no-go: the union must finish in under 3 s.

**MVP-A, "photo studio" (ends about week 10–11).** Image-only inputs: no dirty meshes, the lowest risk, and the fastest demo.
1. **Squama.** The user's interest. It is algorithmically small, and it proves the light preview.
   - Parameters as in LuBan: size X with Z from the image aspect ratio, both snapped to multiples of the cell size; cell size; light side L/R/T/B; light angle 0–20; surface angle 1–20; thickness.
   - Additions: gamma/levels, a cast-shadow preview, a "print vertical" orientation, and auto-tiling into bed-sized panels with alignment pins.
2. **Lithophane** (flat, arc, cylinder) with a backlight preview. It reuses the image → heightfield → watertight plate path almost entirely.

**MVP-B (ends about week 22).** Adds the two features nobody currently serves well.

3. **Stack** (laser):
   - `Manifold.slice(z)` → CrossSection → optional ring hollow with `offset(−w)`.
   - Dowel holes at the polylabel of the eroded intersection of layers k and k+1.
   - Split to the sheet with grid cuts staggered every other layer.
   - Hershey labels "layer.contour.segment", then nest, then SVG/DXF (cut and engrave layers).
4. **Modular cut, basic (fit-to-bed guarantee).** Uses the CAD'19 recursion:
   - pick the axis with the largest excess over the bed;
   - cut with n = ceil(extent/bed) evenly spaced `splitByPlane` cuts (caps close automatically);
   - re-check each piece, allowing a 90° rotation about Z, and recurse;
   - emboss part numbers on the cut faces;
   - assembly order = reverse of the cut tree.

**v1 (about +22 wk, around week 44):**
- Hash/waffle: X and Z cross-sections; slot segments found by overlapping the line intervals; half-depth slots, from the top on X pieces and from the bottom on Z pieces; width = t + tol; checks for slot splitting and assemblability.
- Modular-cut connectors: dowel or plug at the cap's eroded polylabel, with a wall-thickness check; multiple printers and Sequence output.
- Relief: mesh z-buffer, and photo relief using in-browser Depth-Anything-Small.
- Wireframe, 2D and 3D: meshoptimizer decimation, then tubes merged with batch union.
- Squama v1: Top/Bottom light, a per-row dynamic-programming shadow optimiser, and Fence mode.
- More lithophane shapes.
- A kerf/tolerance test-coupon generator.
- Accounts and paywall.

**v2:**
- Plate: VSA low-poly + finger joints.
- Eyeglasses: template contour → offset/mirror → bridge/hinge → extrude.
- Photo Magic set: extrusion, cookie cutter, box.
- Natural and conic cuts; tattoo lithophane.
- Server image→3D (TripoSR).
- Optional Tauri desktop wrapper.

## (d) Effort

One experienced full-stack developer with some geometry background. Units are developer-weeks.

| Item | Wk | Confidence |
|---|---|---|
| Phase 0 spike | 1 | High |
| App shell, viewer, state/undo, worker/job/cancel framework | 2.5 | High |
| Import/export: STL, 3MF, zip; image pipeline | 1.5 | High |
| Squama MVP: generator 0.5, UI and snapping 0.5, shadow preview and light control 0.75, tests/validation 0.5, panel tiling 0.25 | 2.5 | High |
| Lithophane flat/arc/cylinder + backlight preview | 2 | High |
| MVP-A polish, deploy, golden-file tests | 1 | Medium |
| Mesh import hygiene (weld, validate, levelSet fallback, diagnostics) | 1.5 | Medium |
| 2D plumbing (CrossSection ops, SVG/DXF, sheet view) | 1 | High |
| Stack (slices, rings, dowels, split, 3D preview) | 3.5 | Medium |
| Labels (Hershey engrave, embossed text) | 1 | Medium |
| Nesting (skyline MVP, then NFP/GA worker) | 2 | Medium |
| Modular cut, basic | 3 | Medium |
| **MVP total** | **≈23**, with a 20% buffer about 27 (6–7 months) | |
| Hash | 5 | Low–Medium |
| Connectors + multi-printer + sequence | 4 | Medium |
| Relief (mesh + photo depth) | 3 | Medium |
| Wireframe 2D/3D | 3 | Medium |
| Squama v1 (T/B, DP optimiser, Fence) | 3 | Medium |
| Lithophane extra shapes | 2 | High |
| Accounts, Stripe, licensing | 2 | High |
| **v1 total** | **≈22** | |
| Plate | 7 | Low |
| Eyeglasses | 4 | Low |
| Photo Magic (3 methods) | 3 | Medium |
| Natural/conic cut | 3 | Low |
| Tattoo lithophane | 3 | Low |
| Server image→3D | 3 | Medium |
| **v2 total** | **≈23** | |

That is roughly 70 developer-weeks for about 15 core methods. LuBan has around 100 methods built over 8 years, so full parity is not a realistic target.

## (e) Top risks and mitigations

1. **Dirty real-world STLs** (non-manifold or self-intersecting) break slicing, capping and booleans.
   - Weld and check `manifold.status()` on import, then repair, then fall back to `levelSet` SDF remeshing.
   - Show diagnostics, and keep a regression set of about 100 public test models.
2. **Memory and size.** A 200×150 mm lithophane at 0.1 mm/px is about 3M pixels, about 6M triangles, and about 300 MB as binary STL.
   - Default to 0.2 mm/px (about 75 MB).
   - Decimate flat regions, stream the STL writer, prefer 3MF, and estimate the size before generating.
   - wasm32 is capped at 4 GB, and mobile Safari is much lower.
3. **Squama physics.** Cast shadows reduce fidelity at low light angles (numbers in §0), and the formula is reconstructed.
   - Shadow-mapped preview, a per-row DP optimiser in v1, and recommended setups (α ≥ 10° gives a correlation of 0.95 or better).
   - Print a physical calibration plate early.
4. **Pinch, non-manifold edges** in the direct heightfield construction: use the manifold union, or apply a minimum-step epsilon.
5. **Nesting quality and speed:** time-boxed GA in a worker, a skyline fallback, holes filled with small parts.
6. **Hash assemblability and thin-piece splitting:** connectivity check after slotting, warnings, and a slot-direction rule.
7. **Kerf/tolerance calibration:** one tolerance setting with sign conventions (+ for laser, − for FDM) and printable or cuttable test coupons.
8. **COOP/COEP requirements** break third-party embeds: use your own domain and a single-threaded fallback.
9. **License contamination:** a CI license allowlist (license-checker); no GPL or AGPL code in the bundle.
10. **Market:** free vendor tools (MakerLab, Atomm) and LightBurn adding STL slicing. Compete on breadth, the fit-to-bed guarantee, and support for every machine brand.

## (f) What we must not copy

**Name, branding and assets:**
- No "LuBan" name, logo, icons, UI screenshots, manual text, figures, videos or sample files.
- Snapmaker holds US Reg. 6,494,587 for LUBAN.
- Do not use "Squama" or other LuBan feature names as product branding. Choose our own names and run a clearance search in Classes 9 and 42.

**Code:**
- No decompiling or disassembling LuBan binaries.
- Implementers must not read `win_strings.txt`, `linux_strings.txt` or `msi_strings.txt`, or the internal source paths. Keep researchers and implementers separate and keep a clean-room log: the specification comes only from public videos, the manual's described behaviour, and papers.
- No AGPL code (Lamina, Kromacut, Snapmaker Luban).
- No GPL code (CGAL unless we buy a license, boxes.py, the papercraft tools, potrace, lithomaker).
- No unlicensed code (pasak3d, MeshSplitter, Jack000/Deepnest).

**File formats and data:**
- Do not reproduce MU_License.dat or the .fgs/.fgd formats, or the eyeglasses data file.
- Do not use the dlib 68-landmark model (iBUG 300-W, non-commercial) or MeshLib (non-commercial).
- Do not use the Depth-Anything Base/Large/Giant weights (CC-BY-NC) or Hunyuan3D (territory exclusions).

**Art and people:**
- Do not use João Paulo Gonçalves's name or artworks in marketing. The technique itself is fine; his artworks are not.

**Patents:**
- Autodesk US9754412B2 (slices with connection points plus layout) is listed as active until 2033. Get a freedom-to-operate review before shipping Stack/Hash commercially.
- Patents on ShadowPix or Reliefs-as-Images: UNCONFIRMED, check before marketing Squama.

**What is fine to use:** parameter concepts and behaviour seen in public videos (ideas and facts), published algorithms (CAD'15/'16/'19, Sass 2025, Chopper, VSA), and permissively licensed libraries with their notices kept.

## Files
- Prototype (generator, direct and union mesh builders): `prototypes/squama-centre-pivot/squama.py`
- Cast-shadow study: `prototypes/squama-centre-pivot/shadow2.py`
- Manifold edge checker: `prototypes/squama-centre-pivot/own.py`
- Synthetic test image: `prototypes/squama-centre-pivot/target.png`
- Squama STLs (union method, manifold status NoError; they contain coincident-vertex pinch points, so naive welders report "not watertight"):
  - `prototypes/squama-centre-pivot/squama_union_c6.stl`
  - `prototypes/squama-centre-pivot/squama_union_c4.stl`
- Python venv used for the prototype: `(session scratchpad, not copied)/sqenv`