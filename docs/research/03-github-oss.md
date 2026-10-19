## OSS building blocks and existing clones for LuBan, by feature (checked via gh on 2026-10-19)

All star counts, languages, licenses and last-push dates below come from `gh api repos/...` or `gh search repos` today. Where GitHub reports the license as NOASSERTION or none, I read the LICENSE file myself. "Fit" means how directly the repo covers the LuBan feature.

### Things actually tied to LuBan3D on GitHub (no official LuBan source exists)
| Repo | Stars | Lang | License | Pushed | Note |
|---|---|---|---|---|---|
| [Goodsmileduck/pasak3d](https://github.com/Goodsmileduck/pasak3d) | 0 | TS | **none (all rights reserved)** | 2026-10-18 | README says: "Free public alternative to LuBan3D". It cuts big meshes into dowel-joined parts sized to the printer bed. Stack is three.js, three-mesh-bvh, manifold-3d and Tauri. Useful as a design reference only; the code can't be reused. |
| [ncl-mars/MeshSplitter](https://github.com/ncl-mars/MeshSplitter) | 0 | Python | none | 2024-09-11 | Blender add-on that splits meshes into printable parts. Lists luban3d.com as a reference. |
| [ponyboypopart/luban-sequence-assembler](https://github.com/ponyboypopart/luban-sequence-assembler) | 1 | JS | GPL-3.0 | 2024-12-15 | Reads LuBan3D "Modular Cut" Sequence.txt output and builds assembly instructions. This confirms the module is called "Modular Cut". |

**Name clash:** [Snapmaker/Luban](https://github.com/Snapmaker/Luban) (512★, JS, AGPL-3.0, pushed 2025-06-03) is Snapmaker's 3-in-1 printer/laser/CNC host software and has nothing to do with LuBan3D. Other unrelated "Luban" repos: Curzibn/Luban (Android image compression, 13.7k★), focus-creative-games/luban (game config), ly525/luban-h5 (page builder). A code search for "luban3d" found no source code, only link lists and the repos above. The two Squama searches ("squama" in round 1, "LuBan" "Squama" on the web) found nothing related to the product.

### 1. Stack (stacked slices) — covered
| Repo | Stars | Lang | License | Pushed | Fit |
|---|---|---|---|---|---|
| [marcelfarres/lamina](https://github.com/marcelfarres/lamina) | 8 | Python (Pyodide in browser) | AGPL-3.0 | 2026-10-18 | **Direct clone of Slicer for Fusion 360 / 123D Make.** Modes: stacked (dowels, pegs, spacers), interlocked, radial, curve-rib and folded. It checks the assembly, nests parts, and exports SVG/DXF/PDF/EPS. Alpha, one developer. Deps: trimesh, manifold3d, shapely, ezdxf, rectpack. |
| [GridSpace/grid-apps](https://github.com/GridSpace/grid-apps) (Kiri:Moto) | 921 | JS | MIT | 2026-10-04 | Browser slicer with a laser mode (`src/kiri/mode/laser`) that turns a mesh into per-layer laser outlines. Partial fit. |
| [rgsouthall/laser_slicer](https://github.com/rgsouthall/laser_slicer) | 106 | Python | GPL-2.0 | 2022-03-23 | Blender add-on that exports slices as SVG. Partial fit. |
| [frezik/laser_slicer](https://github.com/frezik/laser_slicer) | 17 | Python | BSD-2 | 2021-04-03 | Slices 3D objects for laser cutting. Partial fit. |
| [tintando/millefoglie](https://github.com/tintando/millefoglie) / [Kikketer/object-slicer](https://github.com/Kikketer/object-slicer) | 0 / 0 | TS | MIT / MIT | 2026-10 / 2026-09 | Small browser STL-to-layer-SVG tools. |
| [tynanpurdy/stacker](https://github.com/tynanpurdy/stacker) | 0 | Python | none | 2024-02-04 | "Open source revival of Slicer for Fusion". Stub. |

### 2. Hash / interlocking / waffle / radial — partly covered
| Repo | Stars | Lang | License | Pushed | Fit |
|---|---|---|---|---|---|
| marcelfarres/lamina | 8 | Python | AGPL-3.0 | 2026-10-18 | **Direct:** egg-crate interlocked plus radial half-slices with locking rings. |
| [whatmakeart/make-slice](https://github.com/whatmakeart/make-slice) | 1 | TS | GPL-3.0 | 2026-10-10 | Direct: interlocking slices, layers and finger-joint boxes. Very new. |
| [execuc/LCInterlocking](https://github.com/execuc/LCInterlocking) | 198 | Python | LGPL-2.1 | 2025-12-16 | FreeCAD module for laser-cut tab/slot joints between panels. Covers the joints, not the slicing. |
| [yqlu/sliceformstudio](https://github.com/yqlu/sliceformstudio) | 16 | JS | MIT | 2017-06-13 | Paper sliceforms. Covers the slot math only. |
| [fdxmw/inkscape_sliceforms](https://github.com/fdxmw/inkscape_sliceforms) | 3 | Python | MIT | 2023-08-15 | Sliceform templates. Partial fit. |

### 3. Lithophane — well covered (simple heightmap math)
| Repo | Stars | Lang | License | Pushed | Fit |
|---|---|---|---|---|---|
| [vycdev/Kromacut](https://github.com/vycdev/Kromacut) | 294 | TS | AGPL-3.0 | 2026-10-19 | Color-layered image prints (HueForge-style). Adjacent fit. |
| [s1lentCommit/stl-ready-image](https://github.com/s1lentCommit/stl-ready-image) | 176 | Python | MIT | 2026-08-08 | Image to STL relief/lithophane. Direct. |
| [muldjord/lithomaker](https://github.com/muldjord/lithomaker) | 135 | C++ | GPL-3.0 | 2022-03-11 | Lithophane to STL. Direct. |
| [yomboprime/lithofun](https://github.com/yomboprime/lithofun) | 49 | JS | MIT | 2019-09-29 | Web lithophane generator. Direct. |
| [colbrydi/Lithophane](https://github.com/colbrydi/Lithophane) | 43 | Jupyter | MIT | 2020-10-04 | Python generator. Direct. |
| [furti/FreeCAD-Lithophane](https://github.com/furti/FreeCAD-Lithophane) | 37 | Python | MIT | 2021-11-05 | FreeCAD workbench. Direct. |

### 4. Relief (photo or mesh to bas-relief) — covered
| Repo | Stars | Lang | License | Pushed | Fit |
|---|---|---|---|---|---|
| [opensourcecnc/CarveAnything](https://github.com/opensourcecnc/CarveAnything) | 28 | Python | other (NOASSERTION) | 2026-10-01 | Image to bas-relief mesh. Direct. |
| [wingated/digital-bas-relief](https://github.com/wingated/digital-bas-relief) | 14 | Python | MIT | 2023-04-10 | Depth image to bas-relief. Direct. |
| [GistNoesis/StlToRelief](https://github.com/GistNoesis/StlToRelief) | 12 | JS | NOASSERTION | 2020-02-19 | Mesh to bas-relief. Direct. |
| [GuillaumeBissieres/blender-bas-relief](https://github.com/GuillaumeBissieres/blender-bas-relief) | 4 | Python | GPL-3.0 | 2026-09-08 | Blender relief and heightmaps. |
| [DepthAnything/Depth-Anything-V2](https://github.com/DepthAnything/Depth-Anything-V2) | 8978 | Python | Apache-2.0 code | 2026-03-24 | Photo to depth map, which feeds the relief. **Only the Small model is Apache-2.0; Base/Large/Giant are CC-BY-NC-4.0** (stated in its README). |
| [prs-eth/Marigold](https://github.com/prs-eth/Marigold) | 3242 | Python | Apache-2.0 | 2026-09-06 | Alternative depth model. |

### 5. Photo Magic (photo to 3D; exact behaviour UNCONFIRMED)
| Repo | Stars | Lang | License | Pushed | Fit |
|---|---|---|---|---|---|
| [VAST-AI-Research/TripoSR](https://github.com/VAST-AI-Research/TripoSR) | 7101 | Python | MIT | 2026-06-04 | Single image to mesh. |
| [microsoft/TRELLIS](https://github.com/microsoft/TRELLIS) | 14005 | Python | MIT | 2026-06-26 | Image to 3D. I did not check its third-party dependency licenses (UNCONFIRMED). |
| [Tencent-Hunyuan/Hunyuan3D-2](https://github.com/Tencent-Hunyuan/Hunyuan3D-2) | 15157 | Python | Tencent custom | 2025-10-28 | **Its LICENSE excludes the EU, UK and South Korea.** |
| [visioncortex/vtracer](https://github.com/visioncortex/vtracer) | 7328 | Rust | MIT | 2026-10-17 | Raster to vector, for 2D cut paths. |
| [fogleman/primitive](https://github.com/fogleman/primitive) | 13541 | Go | MIT | 2024-07-22 | Image to geometric primitives (low-poly look). |
| [kilobtye/potrace](https://github.com/kilobtye/potrace) | 345 | JS | GPL-2.0 | 2017-03-17 | Potrace port. GPL. |

### 6. Module / "Modular Cut" (split a big model into parts that fit the bed, with connectors) — partly covered
| Repo | Stars | Lang | License | Pushed | Fit |
|---|---|---|---|---|---|
| [gregstarr/pychop3d](https://github.com/gregstarr/pychop3d) | 23 | Python | MIT | 2023-07-06 | **Implements the "Chopper" paper**: partitions a model into printable parts with connectors. Direct algorithm. |
| Goodsmileduck/pasak3d | 0 | TS | none | 2026-10-18 | Direct LuBan clone, but no license (reference only). |
| [Tassohub/split-for-print-studio](https://github.com/Tassohub/split-for-print-studio) | 2 | Python | MIT | 2026-09-05 | Splitter that knows the build volume. |
| [MickaelDescamps/stl_splitter](https://github.com/MickaelDescamps/stl_splitter) | 0 | – | GPL-3.0 | 2025-02-26 | Splits to fit printer limits and adds connectors. |
| [Jasongod0916/stl-split-and-pin](https://github.com/Jasongod0916/stl-split-and-pin) | 2 | HTML | none | 2026-08-25 | Cut planes plus alignment pins. |

### 7. Folded panels / unfolding / papercraft (Lamina "folded", Pepakura-like) — covered
| Repo | Stars | Lang | License | Pushed | Fit |
|---|---|---|---|---|---|
| [addam/Export-Paper-Model-from-Blender](https://github.com/addam/Export-Paper-Model-from-Blender) | 444 | Python | GPL-3.0-or-later (from its manifest) | 2026-05-26 | Mature Blender unfolder with nesting. |
| [osresearch/papercraft](https://github.com/osresearch/papercraft) | 307 | C | GPL-2.0 | 2018-03-04 | STL unfold to laser SVG. Also contains `wireframe.c` (see §8). |
| [rodrigorc/papercraft](https://github.com/rodrigorc/papercraft) | 288 | Rust | GPL-3.0 | 2026-10-10 | Pepakura-style unwrapper with a GUI and PDF output. |
| [AntonFlorey/PolyZamboni](https://github.com/AntonFlorey/PolyZamboni) | 64 | Python | GPL-3.0 | 2026-07-13 | Blender low-poly papercraft. |
| [jmlien/mesh_unfolder](https://github.com/jmlien/mesh_unfolder) | 2 | C++ | MIT | 2020-09-01 | Edge-unfolding algorithm (permissive license). |
| [GeometryCollective/boundary-first-flattening](https://github.com/GeometryCollective/boundary-first-flattening) / [jpcy/xatlas](https://github.com/jpcy/xatlas) | 852 / 2582 | C++ | MIT / MIT | 2023-06 / 2024-06 | Flattening and chart segmentation. |

### 8. Wireframe (struts plus 3D-printed node connectors) — weak
| Repo | Stars | Lang | License | Pushed | Fit |
|---|---|---|---|---|---|
| osresearch/papercraft (`wireframe.c`) | 307 | C | GPL-2.0 | 2018-03-04 | STL to OpenSCAD vertex connectors. Rough, and the author says it fails on complex shapes. |
| [robertkovax/TrussFab](https://github.com/robertkovax/TrussFab) | 8 | Ruby | GPL-3.0 | 2023-04-03 | SketchUp truss design with generated hubs. |
| [thehans/UniversalVertexConnector](https://github.com/thehans/UniversalVertexConnector) | 2 | OpenSCAD | none | 2018-06-05 | Vertex connector library. |
| [mhsenkow/geodesic](https://github.com/mhsenkow/geodesic) | 0 | TS | MIT | 2026-06-21 | Geodesic hub generator. |
| [ythea/telescoping-structures](https://github.com/ythea/telescoping-structures) | 36 | C# | MIT | 2019-12-17 | Mesh or curves to telescoping struts. Adjacent fit. |

The algorithm itself is easy to build: decimate (meshoptimizer), then at each vertex take the union of sleeves along the incident edges (manifold).

### 9. Plate (planar plates; LuBan's exact meaning UNCONFIRMED) — building blocks only
[florianfesti/boxes](https://github.com/florianfesti/boxes) (1282★, Python, GPL-3.0, 2026-10-17: finger joints), execuc/LCInterlocking (LGPL-2.1), [brainsmoke/polymap](https://github.com/brainsmoke/polymap) (6★, Python, no license, 2025-03-20: laser-cut polyhedra), [mbugert/laserscad](https://github.com/mbugert/laserscad) (53★, LGPL-3.0, 2022-01-10). I found no full mesh-to-plates generator.

### 10. Eyeglasses frames — **no real OSS coverage**
Only [f1yn/project-helios](https://github.com/f1yn/project-helios) (1★, NOASSERTION, 2025-06-18: one parametric frame design), [chrystalquek/parametric-glasses](https://github.com/chrystalquek/parametric-glasses) (0★, no license) and cvonszcz/Compliant-Spectacle-Frames (0★, CAD files only). Everything else that matched is face-shape recommendation apps. It would be built from parts: trace the template image (vtracer, or OpenCV contours), offset with Clipper2, extrude and add hinge/temple templates with manifold.

### 11. Squama — **no OSS coverage; what the feature does is UNCONFIRMED**
The YouTube page gave only the title; the description didn't load. I searched panelization, shingles, scales, surface tiling, planarization and kirigami and found nothing about tiling a surface with fabrication scales. The likely building blocks are libigl, geometry-central and pmp-library (surface sampling, remeshing, offsets), plus [huxingyi/autoremesher](https://github.com/huxingyi/autoremesher) (3589★, MIT) or [wjakob/instant-meshes](https://github.com/wjakob/instant-meshes) (6260★, BSD-3) for an even panel layout.

### 12. Nesting / fitting parts on the bed — well covered
| Repo | Stars | Lang | License | Pushed |
|---|---|---|---|---|
| [Jack000/SVGnest](https://github.com/Jack000/SVGnest) | 2611 | JS | MIT | 2024-02-01 |
| [Jack000/Deepnest](https://github.com/Jack000/Deepnest) | 1176 | JS | none detected (UNCONFIRMED) | 2020-07-07 |
| [deepnest-next/deepnest](https://github.com/deepnest-next/deepnest) | 204 | JS | MIT (LICENSE file read) | 2026-10-18 |
| [JeroenGar/sparrow](https://github.com/JeroenGar/sparrow) + [jagua-rs](https://github.com/JeroenGar/jagua-rs) | 408 / 212 | Rust | MIT / MPL-2.0 | 2026-10 |
| [tamasmeszaros/libnest2d](https://github.com/tamasmeszaros/libnest2d) (+ Ultimaker/pynest2d) | 441 (41) | C++ | LGPL-3.0 | 2024-05-03 |
| [fontanf/packingsolver](https://github.com/fontanf/packingsolver) | 302 | C++ | MIT | 2026-10-18 |

### 13. Core geometry (mesh processing, booleans, clipping, remeshing, browser CAD)
| Repo | Stars | Lang | License | Pushed |
|---|---|---|---|---|
| [elalish/manifold](https://github.com/elalish/manifold) (npm manifold-3d, pip manifold3d) | 2361 | C++/WASM | Apache-2.0 | 2026-10-18 |
| [mikedh/trimesh](https://github.com/mikedh/trimesh) | 3704 | Python | MIT | 2026-10-14 |
| [libigl/libigl](https://github.com/libigl/libigl) | 5105 | C++ | MPL-2.0 core; GPL files in repo (GitHub labels it GPL-3.0) | 2026-09-21 |
| [CGAL/cgal](https://github.com/CGAL/cgal) | 6081 | C++ | GPL-3/LGPL-3 per file, commercial license from GeometryFactory | 2026-10-17 |
| [MeshInspector/MeshLib](https://github.com/MeshInspector/MeshLib) | 836 | C++ | **Non-commercial only; commercial use needs a paid license** | 2026-10-18 |
| [Open-Cascade-SAS/OCCT](https://github.com/Open-Cascade-SAS/OCCT) / [donalffons/opencascade.js](https://github.com/donalffons/opencascade.js) | 3032 / 940 | C++ / WASM | LGPL-2.1 | 2026-10-18 / 2023-08-15 |
| [gkjohnson/three-bvh-csg](https://github.com/gkjohnson/three-bvh-csg) / [three-mesh-bvh](https://github.com/gkjohnson/three-mesh-bvh) | 951 / 3518 | JS | MIT | 2026-09 / 2026-10 |
| [AngusJohnson/Clipper2](https://github.com/AngusJohnson/Clipper2) / [ErikSom/Clipper2-WASM](https://github.com/ErikSom/Clipper2-WASM) | 2511 / 112 | C++ / WASM | BSL-1.0 | 2026-10-17 / 2026-05-18 |
| [shapely/shapely](https://github.com/shapely/shapely) | 4528 | Python | BSD-3 | 2026-10-16 |
| [pmp-library/pmp-library](https://github.com/pmp-library/pmp-library) / [nmwsharp/geometry-central](https://github.com/nmwsharp/geometry-central) | 1510 / 1342 | C++ | MIT | 2026-08 / 2026-06 |
| [zeux/meshoptimizer](https://github.com/zeux/meshoptimizer) / [sp4cerat/Fast-Quadric-Mesh-Simplification](https://github.com/sp4cerat/Fast-Quadric-Mesh-Simplification) | 8582 / 1773 | C++ / C | MIT | 2026-10 / 2024-07 |
| [cnr-isti-vclab/meshlab](https://github.com/cnr-isti-vclab/meshlab) / PyMeshLab / vcglib | 5872 / 983 / 1295 | C++ | GPL-3.0 | 2026 |
| [mrdoob/three.js](https://github.com/mrdoob/three.js) / [sgenoud/replicad](https://github.com/sgenoud/replicad) / [jscad/OpenJSCAD.org](https://github.com/jscad/OpenJSCAD.org) | 116604 / 705 / 3259 | JS/TS | MIT | 2026 |
| [mozman/ezdxf](https://github.com/mozman/ezdxf) | 1480 | Python | MIT | 2026-10-18 |

## Recommended core stack (all permissive licenses)
- **Browser or Tauri app:** three.js, three-mesh-bvh, manifold-3d (booleans, plane slicing, guaranteed-manifold output), Clipper2-WASM (2D offsets for slots, kerf and tabs), meshoptimizer (decimation for low-poly, plate and wireframe), and a sparrow/jagua-rs WASM build or the SVGnest algorithm for nesting. SVG and DXF writers can be custom or use dxf-writer. This is the same core pasak3d uses.
- **Python/desktop alternative:** trimesh, manifold3d, shapely, ezdxf, the pychop3d approach for Modular Cut, plus geometry-central or pmp-library for remeshing. This matches Lamina's dependencies.
- **Photo side:** Depth-Anything-V2-Small (Apache) or Marigold for depth to relief/lithophane, vtracer for vectors, and optionally TripoSR (MIT) for Photo Magic.

## License notes
- **Safe for closed-source commercial use:** MIT, BSD, Apache-2.0, BSL-1.0 and ISC. That covers manifold, trimesh, three.js, three-*-bvh, Clipper2, shapely, pmp, geometry-central, meshoptimizer, SVGnest, deepnest-next, sparrow, packingsolver, pychop3d, ezdxf, vtracer, TripoSR, xatlas and BFF.
- **Weak copyleft, OK if linked dynamically or file-level changes are shared:** jagua-rs (MPL-2.0), libigl core (MPL-2.0), OCCT/opencascade.js (LGPL-2.1), libnest2d/pynest2d (LGPL-3.0), LCInterlocking (LGPL-2.1).
- **Strong copyleft:**
  - GPL (use only if the product is GPL; fine for studying algorithms): CGAL (unless you buy a commercial license), MeshLab/vcglib/PyMeshLab, boxes.py, both papercraft unfolders, Export-Paper-Model, PolyZamboni, potrace, lithomaker, make-slice, TrussFab.
  - AGPL (also triggers when offered as a web service): **Lamina, the most complete clone**, plus Kromacut and Snapmaker Luban.
- **Not usable commercially as-is:**
  - MeshLib: non-commercial license.
  - Depth-Anything-V2 Base/Large/Giant weights: CC-BY-NC-4.0.
  - Hunyuan3D-2: excludes the EU, UK and South Korea.
  - pasak3d, MeshSplitter, Jack000/Deepnest, UniversalVertexConnector and polymap have no license, so default copyright applies.

## Gaps with no good OSS coverage
1. **Squama:** nothing found, and what it does is UNCONFIRMED.
2. **Eyeglasses frame generator from a template image:** nothing.
3. **Plate (mesh to planar plates with joints):** only joint libraries, no generator.
4. **Wireframe node connectors:** only rough or very small projects (osresearch `wireframe.c` GPL, TrussFab GPL, a 0★ MIT geodesic hub tool).
5. **Modular Cut with hollow shells and connectors:** pychop3d (MIT) is the only permissive algorithm, and it hasn't been pushed since 2023. The direct LuBan clone (pasak3d) has no license.
6. **One integrated "guaranteed to fit the bed" multi-mode tool:** only Lamina (AGPL, 8★, alpha) comes close, and it covers the laser modes only, not lithophane, relief or 3D-print modules.

Search outputs are in `(session scratchpad, not copied)/ghsearch/` (results.json, results2.json, results3.json).

Sources: [LightBurn forum post announcing Lamina](https://forum.lightburnsoftware.com/t/free-open-source-tool-to-turn-3d-models-into-laser-cut-parts-slicer-for-fusion-360-replacement/192975), [PyPI lamina3d](https://pypi.org/project/lamina3d/), [AlternativeTo LuBan3D](https://alternativeto.net/software/luban3d/about/), [MIT DDF Project LuBan](https://ddf.mit.edu/?p=814), [trmm.net Wireframe](https://trmm.net/Wireframe), [luban3d.com](https://www.luban3d.com/).