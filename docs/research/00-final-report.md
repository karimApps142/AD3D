# Can we build LuBan? Research answer, 2026-10-19

## 1. Verdict

**Yes, partly, and the part that matters is very buildable.** The core methods are all doable in a clean-room build from permissively licensed libraries and published papers: Squama, lithophane, relief, stacked slices, waffle slices, splitting a model to fit the bed, and nesting. One experienced developer would need **32–48 developer-weeks for a 4-feature MVP** and **about 90–160 for roughly 15 core methods** (2–3.5 years solo). Matching LuBan feature for feature is not realistic: it has about 100 methods built over 8 years. We must not use the "LuBan" name, and three Autodesk slicing patents need an attorney's review before Stack or Hash ships in the US.

## 2. What LuBan is

- **What it does:** a generative-design desktop app. It turns a 3D mesh or a 2D photo into parts you can make on a 3D printer, laser cutter or CNC. Its headline promise is that every part fits the machine's bed.
- **Who makes it:** Lujie Chen (SUTD, Singapore) and Larry Sass (MIT). The research behind it runs from 2012 to 2025 (CAD'15/'16/'19, *Computers & Graphics* 2016/2018, Sass TAD 2025). In practice one developer writes it: all 150 blog posts are by Chen.
- **Platforms (from inspecting the binaries):**
  - Windows x64 is the main build: C++, wxWidgets, OpenGL, CGAL 6.0.1, Eigen, Boost, Clipper2 and dlib, built with VS2022. It is not code-signed.
  - There is also a Win32 build, a Mac build (Intel only, macOS 11+, unsigned) and an Ubuntu x64 build (GTK3).
  - It has 17 interface languages and no version numbers; you update by replacing the exe.
- **Pricing (freemium):**
  - Without a licence everything runs and previews, but saved files are capped at 1 MB.
  - $30/month, $150/year, $750 permanent, $1,200 USB key. A 30-day trial is available on request.
  - Licences are tied to a hardware ID and added by hand to a public `.dat` file.
  - Prices have roughly doubled since 2021.
- **Activity:** the YouTube channel (@luban9604) has about 7.28K subscribers and 171 videos. A new feature comes out every 2–4 weeks; Thermal was released 2026-10-14.
- **Features, in four groups:**
  - **Lithophane:** about 20 shapes, including flat, arc, cylinder, sphere, balloon, stencil, tattoo (photo wrapped onto any model), box, frustum, bronze, dual, tile and CMYK colour.
  - **Photo magic:** about 30 photo-based generators, including box, block, cookie cutter, extrusion, halftone, lenticule, moire, morphing (shadow art), painting, pillar and puzzle. These use classical image processing; there is no machine learning.
  - **Mesh → Cut:** planar, regional, natural, conic, grid, radial and flexi cuts. "Modular cut" fits parts to one or more printers, adds connectors and lap joints, prints part numbers and writes the assembly order to `Sequence.txt`.
  - **Mesh → LuBan:** Stack, Hash (waffle), Plate (finger-jointed panels), Contour, Relief, Wireframe 2D/3D, Thermal, Mold, Grid and others.
  - **Other:** eyeglasses frames with a webcam virtual try-on, Projection (Fence, Pandora, Squama, Torch), 3D bin-packing, a G-code slicer, Boolean/repair/hollow tools, and a 2D image and vector window.
  - **Outputs:** STL, OBJ, DXF, SVG, SHP, G-code and the FGD/FGS scene pair. There is no 3MF or STEP export (inferred from the binary strings, not tested).

### What Squama does (from the video narration and 226 frames)

The video is "LuBan Squama" (Wd26qsSIRY8), uploaded 2026-10-01 UTC, which is 2026-09-30 in US time zones.

- **What it makes:** one photo becomes **one solid 3D-printable plate** covered in a grid of small tilted facets. Under a single low-angle light, each facet looks lighter or darker depending on its tilt, and the photo appears. Under normal room light the plate looks blank.
- **Where the idea comes from:** artist João Paulo Gonçalves's "Shadow Pointillism", where hundreds of wooden panels are glued by hand at different angles. Squama automates this as a print. The name means "fish scale".
- **Where it sits in the app:** File → Create → Projection, then Mode = Squama. The input is a 2D image, not a mesh.
- **Parameters (from the video only):**
  - Model size X: snapped to a whole number of cells.
  - Model size Z: follows the image's aspect ratio and is also snapped.
  - Cell size: default 6.
  - Light source: Left / Right / Top / Bottom.
  - Light angle: default 5°, range 0–20°.
  - Surface angle: maximum ± tilt, default 10°, range 1–20°.
  - Thickness: default 3.
  - The site report's "film size / projection distance / socket" belong to the wider Projection group, not to Squama.
- **Checking the result:** there is a preview with View → Move light / Lighting angle. The author advises printing the plate **standing up**, as with a lithophane.
- **Export format: UNCONFIRMED.** Save model is never clicked in the video; STL is likely.
- **Our reconstruction of the algorithm (inferred):**
  - Average the image into one brightness value per cell.
  - Use the diffuse (Lambert) shading model: brightness = sin(α+θ), where α is the light angle and θ the facet's tilt. So θ = asin(E) − α, limited to [−S, +S], where S is the surface angle.
  - Each facet pivots on its centre around the axis perpendicular to the light.
  - The facets join into one watertight surface on a base plate.
- **What our prototype study found:**
  - The real limit is the shadow each facet casts on its neighbours. Match to the target image was 0.89 at α=5°/S=10° and 0.95 at α=10°.
  - Contrast depends mostly on how dark the room is: about (sin(α+S)+a)/a, where a is the ambient light level. That is about 3.6:1 at 10% ambient.
  - **Open-source prior art does exist:** amirhertz/shadowpix (MIT). The matching papers are Alexa & Matusik 2010 ("Reliefs as Images") and Bermano et al. 2012 ("ShadowPix").

## 3. Feature by feature

Difficulty runs from 1 (trivial) to 5 (research-grade). Effort is in developer-weeks for one developer and includes physical test prints or cuts.

| Feature | Algorithm in one line | Best OSS building block (licence) | Diff. | Effort |
|---|---|---|---|---|
| **Squama** | Brightness per cell → tilt via inverse Lambert; centre-pivot facets built directly as a mesh (handle saddle pinches); bed tiling with alignment keys and a stand | amirhertz/shadowpix (MIT, reference); manifold (Apache-2.0) for validation only | 2–3 | 3–5 (v1 extras 3–6) |
| Lithophane (flat/arc/cylinder) | Thickness = tmin + (1−I)(tmax−tmin) on a parametric surface; closed two-sided shell | s1lentCommit/stl-ready-image (MIT), yomboprime/lithofun (MIT) | 2 | 2–4 (more shapes 2) |
| Relief (mesh/photo) | Depth map from a z-buffer or a depth model → heightfield; bury/clip and base | wingated/digital-bas-relief (MIT); Depth-Anything-V2-**Small** (Apache-2.0) | 2–3 | 2–5 |
| Mesh import / repair | Weld, orient, fill holes; winding-number inside/outside test; rebuild from voxels as a last resort | manifold (Apache-2.0); libigl winding number (MPL-2.0) | 4 | **4–8** (critical path for every mesh feature) |
| Stack | Parallel plane slices → loops → ring offset → dowel holes at polylabel → split to sheet (staggered seams) → labels | manifold slice, Clipper2 (BSL-1.0), mapbox/polylabel (ISC) | 3 | 5–9 |
| Labels (engraved/embossed) | Single-stroke text placed with polylabel; boolean emboss on cut faces | opentype.js (MIT), Hershey font data (keep its attribution notice) | 2–3 | 1.5–3 |
| Nesting | Greedy or skyline packing for large jobs; no-fit-polygon search with a GA for quality; multiple fixed-size sheets | SVGnest (MIT), @deepnest/calculate-nfp (MIT), fontanf/packingsolver (MIT). sparrow (MIT) only solves single-strip packing | 3–4 | 3–7 |
| Modular cut (fit to bed) | CAD'19 recursion: cut along the axis that most exceeds the bed, cap, split into pieces, re-check with Z rotation; avoid thin necks | gregstarr/pychop3d (MIT, Chopper); manifold splitByPlane/decompose | 3–4 | 4–8 |
| Connectors, multi-printer, sequence | Dowel or plug at the eroded cap's polylabel with a wall check; greedy printer load balancing; assembly order = reverse of the cut tree | manifold, Clipper2 | 4 | 5–10 |
| Hash / waffle | X/Z slices; slot on each plane-pair intersection interval, half depth, width = measured thickness − kerf + fit allowance; split oversize ribs; check assembly order | yqlu/sliceformstudio (MIT, slot math); execuc/LCInterlocking (LGPL-2.1) | 4 | **8–14** |
| CNC dogbones / tool compensation | Corner reliefs on inside corners of slots and fingers; offset paths for the tool | Clipper2 (BSL-1.0) | 2 | 2–4 (full toolpaths 6–12; defer to external CAM) |
| Contour | Slices engraved on acrylic sheets inside a frame | manifold + Clipper2 | 2 | ~1 |
| Wireframe 2D/3D | Hash-style section curves → engrave drawing, or sweep tubes → union | meshoptimizer (MIT), manifold; mhsenkow/geodesic (MIT) | 2–3 | 3–6 |
| Plate | Low-poly approximation (VSA/QEM + coplanar merge) → offset by thickness → finger joints per shared edge | meshoptimizer (MIT); no OSS generator exists | 4–5 | **8–16** |
| Eyeglasses | Trace template → offset rim → mirror → bridge, hinge, temples → extrude | vtracer (MIT), Clipper2; no OSS reference | 3 | 5–10 |
| Photo Magic (3–7 methods) | Threshold → contours → offset/extrude/inflate, or brightness → height | OpenCV.js (Apache-2.0), vtracer (MIT); optional TripoSR (MIT) | 2–3 | 3–8 |
| Fence / Projection | Shadow geometry from a point light → bars or film plus socket | manifold | 2–3 | ~2 |
| Natural cut / tattoo lithophane | Curved cut through concave regions near a guide plane / photo mapped along an axis onto a remeshed model and offset along normals | pmp-library (MIT), manifold | 4 | 4–8 each |

## 4. Recommended architecture

**Pick: a hybrid. One C++ geometry core, compiled to WebAssembly for the browser and natively for a CLI and a later Tauri desktop app. The UI is React plus three.js, shared between both, and ships in the browser first.**

- **Core:** C++20 with manifold (Apache-2.0), Clipper2 (BSL-1.0), meshoptimizer (MIT) and pmp-library (MIT). Generators are pure functions of (parameters, inputs) and run in a worker or sidecar so they can be cancelled. Native builds get threads through manifold's TBB support.
- **UI:**
  - React, zustand, react-three-fiber and three-mesh-bvh.
  - A movable directional light with a shadow map, for the Squama and lithophane previews.
  - An SVG sheet view for laser parts.
  - Parameter panels generated from a schema.
- **I/O:** STL, OBJ, PLY and 3MF import; binary STL, 3MF (lib3mf or fflate), SVG and DXF export (pick one DXF writer).
- **Optional ML:** Depth-Anything-V2-Small through onnxruntime-web. Image-to-3D with TripoSR would run on a server, later.

**Why this one:**
1. **Benchmark result:** manifold-3d 3.5.4 in WASM is single-threaded. It handles single operations on 100k–500k triangles in 0.4–2.6 s, which is fine for the photo features and modest meshes. But its batch union took 5.4 s, 23.8 s and 162 s on Squama-sized grids (50×63, 100×126, 150×189 cells). So:
   - the web plan's "union approach" for Squama fails;
   - Squama needs the **direct heightfield mesher from day one**;
   - large scans need native threads.
2. Writing generators once in C++ avoids rewriting TypeScript generators later, and gives a batch/CLI mode for free.
3. Browser first means no installer, no code signing and no antivirus warnings, and instant demos. That is a real edge over LuBan's unsigned Windows app.
4. Both the worker and the sidecar keep a crash on a bad STL away from the UI.
5. **Cost:** the developer needs C++ and Emscripten skills. Add 2–3 weeks over a pure web shell.

## 5. Roadmap

| Phase | Scope | Effort |
|---|---|---|
| **Spike (week 1)** | Shell, viewer, WASM core in a worker; port the Squama direct mesher and measure its speed; test WebGL in a Tauri/Linux webview | 1 |
| **MVP** | 1. **Squama** (direct mesher, shadow-aware light preview, size snapped to cells, tiling to the bed with alignment keys and a stand, calibration tile measured under the expected room light). 2. **Lithophane** flat/arc/cylinder with backlight preview. 3. **Stack** with kerf offsets, measured sheet thickness, labels and nesting. 4. **Modular cut, basic**, plus import/repair (critical path), the sheet view and exports | **32–48** |
| **v1** | Hash with oversize-rib splitting and assembly checks; connectors, multi-printer and Sequence output; relief (mesh and photo); wireframe 2D/3D; CNC dogbones; Squama v1 (Top/Bottom light, per-row shadow optimiser, hex cells, Fence); more lithophane shapes; accounts, licensing, signing, updater | **~27–52** |
| **v2** | Plate; eyeglasses; Photo Magic subset; natural cut; tattoo lithophane; server-side image-to-3D; optional Tauri desktop build for large scans | **~27–55** |
| **Total** | About 15 core methods | **~90–160** (2–3.5 years solo) |

Not included in those numbers: a test 3D printer and laser (or makerspace access), materials, a freedom-to-operate opinion, and a trademark clearance search.

## 6. Risks and mitigations

**Technical**
- **Dirty real-world STLs** (open, non-manifold, self-intersecting) break slicing, capping and booleans.
  - Repair every input into a manifold solid.
  - Use a winding-number fallback and voxel remeshing as a last resort.
  - Show diagnostics, and keep a regression set of 30–100 CC0 test meshes.
- **Bad topology in generated meshes:** T-junctions, exactly equal heights and saddle pinches were all seen in both prototypes. Every generator must pass a watertight check in CI with randomised parameters. Fix pinches with a tie-break epsilon or duplicated vertices.
- **WASM speed and memory:**
  - A lithophane at 0.1 mm/px can be about 6M triangles, roughly a 300 MB STL. Default to 0.2 mm/px, write the STL as a stream and prefer 3MF.
  - Show progress on long jobs.
  - Large scans go to the native build.
- **Physical fit:**
  - Offset every cut path by kerf/2 (outer contours outward, holes and slots inward), and leave engrave paths alone.
  - Ask for the measured sheet thickness, since "3 mm" plywood runs 2.7–3.2 mm.
  - CNC needs dogbones, and every material needs test coupons.
- **Squama in real life:**
  - Room light washes the image out.
  - Facet relief is only about 0.35–0.7 mm, and cells under about 3 mm may not read (UNCONFIRMED; needs a test print).
  - Printed upright, the default 300×378 plate is taller than most printers, so tiling is required.
  - Mitigations: a calibration tile and early test prints.
- **Hash assembly:** slot thin bridges, slots running into holes, ribs that touch nothing, and insertion-order deadlock. Check connectivity and assembly order, and warn the user.
- **Nesting:** thousands of parts are too many for no-fit-polygon search in a browser. Use greedy or skyline packing with a time limit for large jobs, and a wrapper for multiple sheets.

**Legal / IP**
- **Name clash:** Snapmaker holds US Reg. 6,494,587 for **LUBAN** (Class 7, machines), filed further LUBAN applications covering software, and ships the well-known free "Snapmaker Luban" in this exact market.
  - Do not use "LuBan". Avoid "Squama" as a feature name too.
  - Run a clearance search in Classes 9 and 42.
- **Patents:**
  - Autodesk's **US9754412B2** (slices with connection points plus layout), **US9740989B2** and **US10054932B2** ("two-way slicing", which maps directly onto Hash) are listed as Active. Their claims and expiry dates are UNCONFIRMED.
  - No Chen or Sass patent was found, but that was a search only.
  - Whether ShadowPix or Reliefs-as-Images are patented was not checked.
  - Get a freedom-to-operate opinion before shipping Stack or Hash commercially.
- **Clean room:**
  - Do not decompile LuBan.
  - The implementer must not read the binary-strings dumps (`win_strings.txt`, `linux_strings.txt`) or the internal "MU" names and source paths.
  - The spec should come only from public videos, the manual's described behaviour (not its wording) and papers.
  - Do not reuse LuBan's assets, UI layout, file formats (.fgs/.fgd/.shp) or licence scheme.
- **Licences to keep out of a closed product:**
  - GPL: CGAL (unless licensed), MeshLab/vcglib, boxes.py, potrace, lithomaker, the papercraft unfolders.
  - **deepnest-next: `main/deepnest.js` is GPLv3** (the research had called the repo MIT).
  - AGPL: Lamina, Kromacut, Snapmaker Luban.
  - Non-commercial only: MeshLib, Depth-Anything-V2 Base/Large/Giant weights (CC-BY-NC), dlib's 68-point face model (iBUG 300-W).
  - Hunyuan3D's licence excludes the EU, UK and South Korea.
  - No licence (default copyright): pasak3d, Jack000/Deepnest, haimsaw/talshechter ShadowPix.
  - Enforce this with a licence allowlist in CI.
- **Artist:** credit the shadow-shading idea generically. Do not use João Paulo Gonçalves's name or artworks in marketing.
- **Eyeglasses:** spectacle frames are medical devices in the US and EU. How that applies to design software is UNCONFIRMED.

**Market**
- Free tools from hardware vendors could widen their scope: Bambu MakerLab, xTool's Atomm, Glowforge.
- LightBurn has STL slicing "coming" (secondhand report).
- Free open-source tools cap what people will pay: Kiri:Moto, BurntStuff, Lamina.
- A live paid Fusion add-in exists that the market research missed: **JT 3D Slicer**.
- Hardware makers bundling free software pushes prices down, toward roughly $5–10/month or $49–79 one-time.
- **The gap a new product could own:** no maintained tool that works with any brand does stacked + waffle + folded slicing, splitting to the bed, nesting and assembly guides together. 123D Make is discontinued and Slicer for Fusion 360 is unmaintained.
- **How to compete:** breadth of methods, the bed-fit and assembly guarantee, machine profiles for every brand, and zero-install web delivery.

## 7. Open questions for you

1. **What is the goal?** An internal tool, a client deliverable or a commercial product? This decides whether licensing, patents, trademark and payments are in scope at all.
2. **Target platform:** browser first (recommended), Windows desktop only, or both?
3. **Which features matter most?** Is Squama the main interest, or the "fits the bed" slicing (Stack, Modular cut, Hash)? Do lithophane and eyeglasses matter?
4. **Which processes:** 3D printing only, or laser and CNC too? CNC adds dogbones and possibly toolpaths.
5. **Team and budget:** is there a developer comfortable with C++ and Emscripten? Is there access to a printer, laser or CNC for test prints and cuts? What is the deadline?
6. **Pricing model,** if it is a product: free tier limits, Pro pricing, education licences?

## 8. Sources

- **LuBan:**
  - https://www.luban3d.com/ · https://www.luban3d.com/license · https://www.luban3d.com/news · https://www.luban3d.com/meshsafe
  - https://www.youtube.com/watch?v=Wd26qsSIRY8 · https://www.youtube.com/@luban9604
  - Downloads: https://drive.google.com/open?id=1_vFE2oWGcxXdm_Tla_05nGKzRggFsuoM · https://1drv.ms/u/s!ArS_QR3BBDa-kj68Dl5k0d10-gaU?e=fb4hfo (OneDrive needs a login; contents UNCONFIRMED)
- **Research:**
  - CAD'15: http://www.cad-conference.net/files/CAD15/CAD15_12-15.pdf
  - CAD'16: http://www.cad-conference.net/files/CAD16/CAD16_1-4.pdf
  - CAD'19: http://www.cad-conference.net/files/CAD19/CAD19_358-362.pdf
  - Sass 2025: https://architecture.mit.edu/sites/default/files/publications/2025-Sass.pdf
  - https://doi.org/10.1016/j.cag.2015.07.003 · https://doi.org/10.1016/j.cag.2018.02.005
  - MIT DDF: https://ddf.mit.edu/?p=814 · https://ddf.mit.edu/?p=689
  - SUTD: https://www.sutd.edu.sg/very-large-scale-prototyping
  - Chopper: https://gfx.cs.princeton.edu/pubs/Luo_2012_CPM/chopper.pdf
- **Press:**
  - https://makezine.com/article/maker-news/luban-helps-you-make-big-stuff-with-your-3d-printer-or-laser/
  - https://forum.makerforums.info/t/luban-a-swiss-army-knife-of-a-program-for-3d-printing-laser-cutting-and-cnc/83812
  - https://alternativeto.net/software/luban3d/about/
- **Patents:** https://patents.google.com/patent/US9754412 · https://patents.google.com/patent/US9740989 · https://patents.google.com/patent/US10054932
- **Trademark:** https://trademarks.justia.com/900/35/luban-90035504.html · https://github.com/Snapmaker/Luban
- **Market:**
  - Autodesk Slicer forum threads: https://forums.autodesk.com/t5/fusion-support-forum/fusion-360-slicer-alternative/td-p/10430957 · https://forums.autodesk.com/t5/fusion-support-forum/slicer-for-fusion-360-when-is-it-coming-back/m-p/8462824
  - JT 3D Slicer: https://marketplace.autodesk.com/apps/55761de7-a290-4c4d-a557-9e88e599510b
  - https://www.atomm.com/creativetools/3d-model-layer-slicer · https://www.burntstuff.com/tools/slicer/ · https://grid.space/ · https://itslitho.com/
  - LightBurn 3D Slice: https://docs.lightburnsoftware.com/galvo/3DSlice.html
  - https://www.contextworld.com/quarterly-industrial-3d-printer-shipments-rebound-amid-continued-record-breaking-entry-level-growth
- **OSS:**
  - https://github.com/elalish/manifold · https://github.com/AngusJohnson/Clipper2 · https://github.com/zeux/meshoptimizer
  - https://github.com/gregstarr/pychop3d · https://github.com/Jack000/SVGnest · https://github.com/fontanf/packingsolver
  - https://github.com/JeroenGar/sparrow · https://github.com/deepnest-next/deepnest (mixed licences; see its LICENSES.md)
  - https://github.com/amirhertz/shadowpix · https://github.com/marcelfarres/lamina (AGPL, reference only)
  - https://github.com/DepthAnything/Depth-Anything-V2 · https://github.com/VAST-AI-Research/TripoSR · https://github.com/visioncortex/vtracer

**Representative frames from the Squama video:**
- `docs/research/squama-video/frames/f_060.jpg`: Squama parameter panel (3:56)
- `docs/research/squama-video/frames/f_055.jpg`: File > Create submenu, Projection (3:36)
- `docs/research/squama-video/frames/f_096.jpg`: cell grid in a jet colormap (6:20)
- `docs/research/squama-video/frames/f_122.jpg`: close-up of tilted facet columns, Left light (8:04)
- `docs/research/squama-video/frames/f_167.jpg`: View menu, Lighting angle / Move light (11:04)
- `docs/research/squama-video/frames/f_225.jpg`: final 200×252, cell-4 Mona Lisa under grazing light (14:56)
- `docs/research/squama-video/frames/f_148.jpg`: Top light, horizontal facet rows
- `docs/research/squama-video/frames/f_059.jpg`: Fence mode panel

**Prototypes and benchmark:**
- Squama prototype, centre pivot (web plan): `prototypes/squama-centre-pivot/squama.py`
- Squama prototype, facets rising from the base (desktop plan): `prototypes/squama-rising/squama.py`
- WASM benchmark: `prototypes/wasm-bench/bench.mjs`