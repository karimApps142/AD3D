# Adversarial review: can we build a LuBan equivalent?

**Verdict:** yes, it can be built. But both designs are too optimistic, and the web-first plan's main Squama decision fails my benchmark. My range for the 4-feature MVP is **32–48 developer-weeks**. The two designs said 23–27 (web) and 32–34 (desktop). For roughly 15 core methods my range is **100–160 developer-weeks**, about 2–3.5 years for one developer.

## 1. Claims I checked myself

1. **Video date.** `squama.info.json` gives upload_date 20261001 and timestamp 1790820054, which is about 02:00 UTC on 2026-10-01. That is still 2026-09-30 in US time zones, so both reports' dates are correct.

2. **Licences (via gh, today):**

| Repo | Stars | Licence I found | What it means |
|---|---|---|---|
| marcelfarres/lamina | 8 | AGPL-3.0 | Confirmed. Can't be used in a closed product. |
| Goodsmileduck/pasak3d | 0 | none | Confirmed. Reference only. |
| Jack000/Deepnest | 1176 | none | See deepnest-next below: its nesting file is GPLv3. |
| elalish/manifold | 2361 | Apache-2.0 | Confirmed. |
| MeshInspector/MeshLib | 836 | "NON-COMMERCIAL & education LICENSE AGREEMENT" (AMV Consulting) | Confirmed: not usable commercially. |
| Snapmaker/Luban | 512 | AGPL-3.0 | Confirmed. |
| gregstarr/pychop3d | 23 | MIT | Confirmed. |
| ErikSom/Clipper2-WASM | 112 | BSL-1.0 | Confirmed. |
| JeroenGar/sparrow | 408 | MIT | Licence confirmed, but see correction 9. |

3. **deepnest-next licence: the research is wrong.** The research called it "MIT (LICENSE file read)". Its `LICENSES.md` says the repo has mixed licences, and it lists **`/main/deepnest.js` as GPLv3**. The npm package `@deepnest/calculate-nfp` is MIT. So deepnest-next cannot be bundled as-is. Use SVGnest (MIT) or calculate-nfp alone.

4. **manifold-3d WASM speed: the web plan's Squama approach fails.**
   - Setup: npm `manifold-3d@3.5.4`, node 22, i9-9980HK. The `.wasm` file is 541 KB and the JS has no pthread or SharedArrayBuffer references, so it is **single-threaded**.
   - Benchmark script: `prototypes/wasm-bench/bench.mjs`

| Operation | Mesh size | Time |
|---|---|---|
| Boolean subtract | 131k triangles | 0.41 s |
| Boolean subtract | 524k triangles | 1.77 s |
| splitByPlane | 131k triangles | 0.58 s |
| splitByPlane | 524k triangles | 2.63 s |
| 40 slices | 524k triangles | 0.04 s |
| Squama-style batch union | 50×63 cells | **5.4 s** |
| Squama-style batch union | 100×126 cells | **23.8 s** |
| Squama-style batch union | 150×189 cells | **162 s** |

   - All unions returned NoError.
   - Caveat: my facet prisms overlap the base by 0.5 mm, so the geometry is not identical to the web plan's.
   - **Conclusion:** WASM is fine for single operations on 100k–500k triangles. A 1M+ triangle scan with dozens of cuts and connector booleans will take minutes, so it needs progress reporting.
   - The web plan's MVP rule "ship the union approach" fails its own go/no-go of under 3 s, even at the smallest grid. WASM was about 5–25× slower than the native numbers the plan reported, not the 1.5–3× it assumed. **Building the mesh directly is required from day one.**

5. **Slicer for Fusion 360: status UNCONFIRMED.** A 2020 Autodesk forum thread says it was in maintenance mode. An undated reply from an Autodesk community manager says it is back on the App Store. An undated Japanese guide says it cannot be downloaded there. Either way it is unmaintained. The market research **missed "JT 3D Slicer"**, a paid Fusion add-in on the Autodesk Marketplace with a trial limited to 10 slices: a live commercial competitor.

6. **Autodesk patents: the research found only one.** Google Patents shows US9754412B2 (Bachrach and Griffith) as Active. It also shows **US9740989B2** and **US10054932B2, "Techniques for two-way slicing of a 3D model for manufacturing"**, both Active. The second one maps directly onto Hash (interlocking slices). I could not read the claims (Google Patents returned 503, Justia 403), and the exact expiry dates are UNCONFIRMED.

7. **Squama has open-source prior art.** The claim "no OSS coverage" is too strong.
   - amirhertz/shadowpix: MIT, 3★, Python, described as a "3d print project".
   - haimsaw/ShadowPix and talshechter/ShadowPix: no licence.
   - The right academic references are Alexa & Matusik 2010 ("Reliefs as Images") and ShadowPix 2012. The GitHub search looked for panelization and kirigami, which is the wrong field.

## 2. Corrections to the research and designs

1. **Squama parameters (site report).** The site report lists "film size/distance, projection distance, socket, light height" as Squama settings. Those strings belong to the shared Projection group in the binary. The video shows only Mode, Shadow, Model size X/Z, Cell size, Light source, Light angle, Surface angle and Thickness. Trust the video.

2. **Squama mesh method (web plan).** The batch-union MVP choice is refuted (check 4 above). Make the direct heightfield mesher the default, with pinch handling: duplicate vertices at saddle corners, or a minimum-step epsilon. Keep manifold only for validation.

3. **Squama contrast (desktop plan).** "Brightest facet is only 0.26 of head-on" is not the real limit. What matters is the contrast ratio once ambient light is added: (sin(α+S)+a)/a, where a is the ambient level. At α=5° and S=10°, with ambient at 10% of the grazing light, the ratio is about 3.6:1. Contrast depends far more on how dark the room is than on the formula. The calibration tile is the right idea, but it must be measured under the expected ambient light.

4. **Squama and the printer bed.** LuBan's default 300×378 plate, printed standing up as the author advises, exceeds the Z height of most consumer printers. A 3 mm plate standing 250 mm tall will also wobble.
   - The desktop plan has no tiling. The web plan tiles but adds no stability features.
   - Add automatic tiling with alignment keys, and a printable stand or brim rib.
   - "Fits the bed" is LuBan's headline claim, yet Squama apparently doesn't apply it (UNCONFIRMED).

5. **Squama attachment and overlap.** Not a real risk. The output is one solid on a shared base, and cells tile without overlap. The real risks are:
   - cast shadows from neighbouring cells (the web plan's study covers this);
   - pinch edges where four cells meet in a saddle;
   - stair-stepping when printed flat;
   - facet relief of only about 0.35–0.7 mm, which is close to FDM's lateral precision on small cells. Cell sizes under about 3 mm will probably lose the image (UNCONFIRMED; needs a test print).

6. **Kerf compensation is under-specified everywhere.** LuBan's single tolerance knob only changes slot width. Real compensation needs:
   - offsetting every cut path by kerf/2: outer contours outward, holes, dowel holes and slots inward;
   - leaving engrave paths and labels un-offset;
   - nesting spacing of at least the kerf;
   - a **measured** sheet thickness input, because plywood sold as 3 mm runs about 2.7–3.2 mm;
   - slot width = measured thickness − kerf + fit allowance.

   Clipper2 makes this cheap, but it must be in the spec and in the tests.

7. **CNC is missing from both designs.** The brief lists CNC output. On a router, a slot cut with a round bit cannot have a sharp inside corner, so Hash, Plate and box joints need **dogbone or T-bone corner reliefs**. Also missing: tool-diameter compensation and, optionally, toolpaths (LuBan has Draw → Toolpath). Without dogbones, CNC-cut waffle parts will not assemble.

8. **Hash is harder than both designs allow.** Missing cases:
   - **ribs larger than the sheet**: splitting one breaks the slot structure and needs lap or finger joins plus re-slotting (CAD'15 handles this with "sub-planes"; neither design does);
   - two slots close together leaving a thin bridge that snaps;
   - a slot running into an internal hole or a ring-hollowed area;
   - several intersection intervals on one non-convex rib;
   - ribs that touch nothing perpendicular (LuBan's manual admits this);
   - insertion-order deadlock (the crdbrd problem).

9. **Nesting.** Sparrow solves **strip packing** (one strip, minimize length), not packing onto several fixed-size sheets. A browser build exists (sparrowstudio.app), so WASM is feasible. Multi-sheet packing needs jagua-rs's bin-packing support or a wrapper around sparrow (UNCONFIRMED effort). True-shape nesting with a no-fit-polygon search over thousands of parts (the CAD'16 bust had 5425) is too slow in a browser. Use skyline or greedy packing for large jobs.

10. **Mesh repair in the browser.** "levelSet fallback" is not a ready-made feature. manifold's `levelSet` needs a signed-distance callback, and in WASM a per-sample JS callback is slow. More importantly, a robust inside/outside test on a dirty mesh needs a generalized or fast winding number, which neither design budgets for (it exists in libigl, MPL-2.0). The web plan's 1.5 weeks for import cleanup is unrealistic. The desktop plan's 3 weeks at Low confidence is closer to honest.

11. **Modular cut.** The CAD'19 recursion guarantees a bounding-box fit, but the designs omit:
   - splitting a cut result into its separate pieces (manifold `decompose()`);
   - moving the cut plane away from thin necks to avoid slivers;
   - an oriented fit test using more than one 90° Z rotation, plus margins for brim and supports;
   - collisions between connectors and labels on the same cut face;
   - a minimum cross-section for a connector.

    All of this needs a manifold input, so **import repair is on the critical path for every mesh feature**.

12. **Small inconsistencies.**
    - The web plan names both `@tarikjabiri/dxf` and `dxfjs/writer`; pick one.
    - The two prototypes disagree on Squama depth: max relief 4.5 mm (web, centre pivot) against a 3.0–3.71 mm plate (desktop). The desktop plan's facets rise from the base, giving steps of cell·tanθ, which roughly doubles the step height and the cast shadows. They are different geometries, and the web plan's centre pivot is better.
    - The algorithms report says the Instagram artist is not named. The video report names João Paulo Gonçalves from the frames. The frames are the better source.

13. **Licences the research missed.**
    - deepnest-next is GPLv3 in part (check 3).
    - Hershey font data has a custom attribution licence; keep the notice.
    - opentype.js is fine, but any bundled font needs its own licence (OFL is fine).
    - three.js loaders are MIT.
    - Depth-Anything-V2-Small: the web plan says it confirmed Apache-2.0 via the Hugging Face API; I did not re-check.

## 3. Revised effort per feature

One experienced developer, in developer-weeks. Each range includes physical test prints or cuts.

| Feature | Design A (web) | Design B (desktop) | My range | Why |
|---|---|---|---|---|
| App shell, viewer, workers or sidecar, export | 4 | 8 | 5–9 | |
| Squama MVP (direct mesher, light preview, tiling, calibration) | 2.5 | 2.5 | 3–5 | 2–3 print-and-test cycles |
| Lithophane: flat, arc, cylinder | 2 | 2 | 2–4 | Memory and STL size at 0.1 mm/px |
| Mesh import and repair (winding-number fallback, diagnostics, test corpus) | 1.5 | 3 | **4–8** | Critical path |
| Stack (rings, dowels, staggered splits, labels) | 3.5 | 5 | 5–9 | |
| Labels (engraved and embossed) | 1 | (incl.) | 1.5–3 | |
| Nesting (greedy, then true-shape, multi-sheet) | 2 | 2 | 3–7 | |
| Modular cut, basic | 3 | 4 | 4–8 | |
| **MVP total** | 23–27 | 32–34 | **32–48** | |
| Hash (including oversize-rib splitting and assemblability) | 5 | 6.5 | **8–14** | |
| Connectors, multi-printer, assembly sequence | 4 | 4.5 | 5–10 | |
| Relief, mesh and photo | 3 | 1.5–3 | 2–5 | |
| Wireframe 2D and 3D | 3 | 3 | 3–6 | |
| CNC dogbones and tool compensation (missing from both) | – | – | 2–4 | Full toolpaths 6–12; better to defer to external CAM |
| Squama v1 (top/bottom light, shadow optimiser, hex cells) | 3 | 1.5 | 3–6 | |
| Accounts, licensing, signing, updater | 2 | 3 | 2–5 | |
| Plate (low-poly + finger joints) | 7 | 6 | **8–16** | Research-grade |
| Eyeglasses | 4 | 4 | 5–10 | Hinge and fit iteration; no OSS reference |
| Photo Magic, 3–7 methods | 3 | 7 | 3–8 | |
| Natural cut / tattoo lithophane | 3 + 3 | 3 + 3 | 4–8 each | |

On top of the developer-weeks, budget for a test printer and laser (or a makerspace), materials, a freedom-to-operate opinion and a trademark clearance search. Neither design includes these.

## 4. Recommended architecture

A **hybrid**: one C++ geometry core compiled to both WASM and native, with a web UI shipped in the browser first.

- **Core:** C++ with manifold, Clipper2 and meshoptimizer. Build it with Emscripten for the browser, and natively for a CLI and a later Tauri sidecar.
- **UI:** React and three.js, shared between the browser app and the Tauri desktop app.

Why:
1. My benchmark shows single-threaded WASM is good enough for single operations on 100k–500k triangles. That covers image-based features (Squama, lithophane, relief) and modest mesh jobs, so the browser can host the photo-studio MVP with no install and low distribution cost.
2. Big unions and repeated cuts on 1M+ triangle scans scale badly in WASM (24–162 s in my tests). They need native threads, which manifold supports through TBB natively, or a native or server fallback. TS-only generators, as in the web plan, would have to be rewritten to get that. Writing generators once in C++ avoids the rewrite and gives batch and CLI mode for free.
3. Shipping web first avoids code signing and antivirus friction (LuBan's own site warns about AV alerts) and gives zero-install demos, a real advantage against LuBan's unsigned Windows app.
4. Running jobs in a worker or sidecar isolates crashes on bad STLs in both setups.

Cost: the developer needs C++ and Emscripten skills. Add about 2–3 weeks to the web plan's shell estimate.

Phasing:
- **Phase 1:** Squama and lithophane, browser only. Direct meshers, no booleans needed.
- **Phase 2:** Stack, Modular cut, mesh repair. Browser for normal-sized models; offer the Tauri build or server processing for large scans.
- **Phase 3:** Hash, connectors, CNC dogbones.

## 5. Remaining unknowns

1. LuBan's actual Squama formula (linear or Lambertian inverse), its export format, and whether its facets pivot on the centre. Its Save model step was never shown.
2. The claims and expiry of the three Autodesk slicing patents, especially US10054932B2 (two-way slicing) against Hash. This needs an attorney.
3. Whether ShadowPix or Reliefs-as-Images are patented (not searched).
4. Speed of a multithreaded WASM build of manifold (would need a custom build) and memory limits on mobile Safari.
5. How much effort it takes to turn sparrow or jagua-rs into multi-sheet bin packing, and its speed with more than 1,000 parts.
6. Whether thin-plate Squama prints are physically readable at cell sizes of 3 mm or less, under realistic ambient light. Only a test print will tell.
7. Regulatory status of generated eyeglass frames. Spectacle frames are classed as medical devices in the US and EU; how that applies to design software is UNCONFIRMED.
8. **The goal behind "can we build it?"**: an internal tool, a client deliverable, or a commercial product. This decides whether licensing, patents, trademark and payments are in scope at all. Ask the user.
9. Current Slicer for Fusion availability, and JT 3D Slicer's price and features.
10. The OneDrive folder's contents (login required), and whether the CGAL modules LuBan links are licensed commercially. That doesn't affect a clean-room build, but it matters for any partnership with LuBan.

Sources: [Google Patents US9754412](https://patents.google.com/patent/US9754412), [US9740989](https://patents.google.com/patent/US9740989), [US10054932](https://patents.google.com/patent/US10054932), [Autodesk forum: Slicer coming back?](https://forums.autodesk.com/t5/fusion-support-forum/slicer-for-fusion-360-when-is-it-coming-back/m-p/8462824), [Autodesk forum: Slicer for Fusion 360](https://forums.autodesk.com/t5/fusion-360-support/slicer-for-fusion-360/m-p/9690376), [pc-koubou Slicer guide](https://www.pc-koubou.jp/magazine/39987), [Autodesk Marketplace: JT 3D Slicer](https://marketplace.autodesk.com/apps/55761de7-a290-4c4d-a557-9e88e599510b); GitHub API for all repo and licence data.