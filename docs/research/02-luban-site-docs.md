# LuBan (luban3d.com): product documentation report

Researched 2026-10-19. I executed no downloaded binaries. The Mac DMG was mounted read-only to read its plist, then ejected.

**What's in it:** Windows-first C++ desktop app, also built for Mac (Intel only) and Ubuntu. Proprietary freemium: the free tier processes and views anything but saves only files up to 1MB. Licenses cost $30/month, $150/year, $750 permanent or $1200 for a USB key. It has about 100 generator methods across four modules, a feature blog with 150 posts (Feb 2019 to Oct 2026), and one apparent lead developer (Lujie Chen).

## 1. Sources fetched
- Website pages (a Wix site): https://www.luban3d.com, `/license`, `/news`, `/gallery`, `/meshsafe`, `/thanks`, `/blog-feed.xml`, `/sitemap.xml`. The gallery page is images only; no text came out of it.
- All 150 blog posts at `https://www.luban3d.com/post/<slug>` (dates and text are in section 4).
- Wayback captures of `/license` for 2020-09, 2021-01, 2023-03, 2024-02, 2025-01 and 2026-01, for price history. The earliest site capture is 2019-06-22.
- The Google Drive download folder (`1_vFE2oWGcxXdm_Tla_05nGKzRggFsuoM`), listed in full, and the MeshSafe folder (`1QBK3qSYRxGuf2z-mSd3jFfGm3Y3aJw5k`).
- The OneDrive share needs a login, so its contents are UNCONFIRMED. It is presumably the same as the Drive folder.
- Downloaded documents: "Hello LuBan.pdf" (English, dated Aug 19, 2025, 123 pages, the main manual), readme.txt, Win64 "Please read.txt", "LuBan Mac Installation Guide.pdf", and the Chinese "Get started" and "Introduction" PDFs.
- There is no English "LuBan Get started.pdf" in the Drive any more, although the site FAQ and readme still point to it. Only the Chinese version (29 pages) is there.
- YouTube channel UChbeOwTaHYc8sxpPFp6g-nA and the Squama video Wd26qsSIRY8.
- Other pages:
  - MIT project pages: https://ddf.mit.edu/?p=814 and https://ddf.mit.edu/?p=689
  - SUTD project page: https://www.sutd.edu.sg/very-large-scale-prototyping
  - Make: article, Oct 2018: https://makezine.com/article/maker-news/luban-helps-you-make-big-stuff-with-your-3d-printer-or-laser/
  - Maker Forums post, Sep 2021: https://forum.makerforums.info/t/luban-a-swiss-army-knife-of-a-program-for-3d-printing-laser-cutting-and-cnc/83812
  - AlternativeTo listing: https://alternativeto.net/software/luban3d/about/
  - Fabbaloo articles (May 2019, Feb 2025; both returned 403, so only search snippets were read): https://www.fabbaloo.com/2019/05/luban-3d-printing-very-large-objects and https://fabbaloo.com/?p=228317
- No Reddit, Thingiverse or Instructables pages about LuBan turned up. Search results for "Luban" are mostly about Snapmaker's Luban, which is a different product.

## 2. Platforms and builds (from inspecting the files)

Files on the Drive are dated Oct 16, 2026.

| Build | What it is |
|---|---|
| Win 64 `LuBan.exe` (20.6MB) and `Setup.msi` (81.6MB) | Native x64 program built with Visual Studio 2022 (linker 14.44), timestamp 2026-10-17. Not code-signed (the site warns about antivirus alerts). The installer was made with Visual Studio Installer Projects, has a fixed version number 1.0.0, manufacturer "LuBan3D", and installs to `C:\Program Files (x86)\LuBan3D\LuBan`. |
| Win 32 `LuBan.exe` / `Setup.msi` | Present on the Drive; not downloaded. |
| Mac: `LuBan MacOS 10.dmg` and `LuBan MacOS 11.dmg` | I inspected the macOS 11 one: Intel (x86_64) only, so Apple Silicon Macs would run it under Rosetta. Minimum macOS 11.0, unsigned, version string "LuBan 2.0", copyright line "LuBan Rapid Prototyping", ships with 17 language folders. |
| Ubuntu 64 `LuBan` (24MB) | Linux x86-64 program using GTK3, OpenGL and TBB. English only. The Linux version was announced 2020-07-30. |
| `vc_redist` x86/x64 | Microsoft Visual C++ runtime installers. |

**Code stack** (from strings and imports in the binaries):
- C++ with the wxWidgets UI toolkit (3.2.4 on Windows, 3.1.4 on Linux) and OpenGL/GLU.
- CGAL 6.0.1 geometry library: exact-arithmetic booleans, hole filling, repair, self-intersection checks, surface parameterization, 2D/3D triangulations, bounding boxes, spatial search trees.
- Eigen 3.4.0, Boost 1.86.0, Clipper2 (polygon clipping), tinyxml2.
- dlib 19.9, including its neural-net and face detection/landmark code.
- DCMTK 3.6.0 (medical DICOM images), libtiff 4.2.0, libjpeg, libpng, zlib 1.2.13.1.
- Windows Media Foundation, used for the webcam virtual try-on.
- No Qt, .NET, Unity or Python.


**Older code in the same program:** the 2D window still has optical-metrology tools: phase shifting, deflectometry, phase unwrapping, white-light scanning, speckle processing, camera calibration and visual hull. The codebase is clearly one person's long-running research framework.

**Bundled face model:** `MU_EG_VTO.dat` (99,693,937 bytes) has the same SHA-256 (`fbdc2cb8…b92f`) as the public dlib file `shape_predictor_68_face_landmarks.dat`, renamed. It powers the eyeglasses virtual try-on.

**Licensing risk:** this model was trained on the iBUG 300-W dataset, whose terms are generally understood to bar commercial use. That is my reading, not legal advice. The CGAL modules used are GPL-licensed unless a commercial CGAL license is bought, and I found no notice either way.

**Languages:** 17 interface languages on Windows and Mac (zh_CN, zh_TW, fr, de, el, it, lt, ms, pl, pt_BR, pt_PT, ru, sk, sl, es, th, plus English).

## 3. Features

Settings are stored in `MU_Setting.txt`. The app has two windows: a 2D one for images and a 3D one for meshes. The UI has no Apply button; typing a value and pressing Enter regenerates the model.

**A. Lithophane** (`File → Create → Lithophane`): turns a photo into a 3D-printable lithophane.
- Shapes: Flat (with slant and base), Arc, Rectangle, Hexagon, Octagon, Dodecagon, Cylinder, Cone, Vase (control points), Sphere, Spiral (print long, then flatten in hot water), Stencil (shaped by a template image), Balloon, Tattoo (photo wrapped onto any 3D model), Frustum and perspective frustum, Box and double-sided box, Bronze (from a 3D solid), Dual (two photos), Tile (from a 3D model), double-sided stencil.
- Settings:
  - positive or negative, exterior or interior;
  - min/max thickness, resolution in mm/pixel, predicted file size;
  - border, base and socket (O or U), hanger, edge support;
  - backlight simulation with light and filament colour;
  - multiple photos on poly-face shapes, upright or inverted, one body or assembly with tolerance.
- Colour: a colour-filter image for colour lithophanes, CMYK lithophanes with custom filament colours, and DXF/SHP vector templates for stencils.
- "Save model" writes a folder containing `<name> LP Model.stl`, `LuBan Model.fgd/.fgs`, `Color W H.jpg`, `<name> Setting.txt` and `MU Setting.txt`.

**B. Photo magic** (`File → Create → Photo magic`): one photo in, many kinds of model out. Most methods need a black-and-white image.
- Balloon, Box (lids: basic, twist, flat, inlay; inner pattern; nested boxes; magnet sinks; box light)
- Block (connected blocks; chain mail in v2), Cell, Channel
- Cutter (cookie cutter with imprint pattern and stopper)
- Extrusion (positive/negative, keychain hole, colour)
- Fabric, Fidget, Flexi, Halftone 3D, Layering (multi-layer frames)
- Lenticule (two images that change with viewing angle), Moire (animation)
- Morphing (two photos or text strings, negative, three-way, grid)
- Painting (colour layers; painting box, single and double sided), Pendant, Pillar
- Puzzle (true-colour OBJ export, colour layers, puzzle of a 3D model from its depth map)
- Ridge, Teacup, Whistle, Wire, Wormhole, Photo clicker

**C. Other File → Create generators:**
- Image to cylinder, Image to sphere, 3D shape
- Bamboo-copter, Bottle (from two cross-section images), Dice, Glider
- Eyeglasses: a frame generated from a template image, plus webcam virtual try-on (Windows only)
- Projection: Fence, Pandora (light box with lithophane lid), Squama, Torch, plus projection from a photo
- Collage, ID Photo

**D. Mesh → Cut:** splits big models into parts. The site's headline claim is that every part fits the machine's working size.
- Planar cut: regional (disk) cut, open or closed cut sections, "natural" curved cut, multiple planes at once, planes movable with the mouse.
- Conic, Flexi, Grid (staggered, planes adjustable) and Radial cuts.
- Modular cut:
  - fits parts to the build volume of one printer or several;
  - rectangular or circular printer beds;
  - assigns parts so multiple printers finish around the same time;
  - multi-threaded; axial or non-axial planes; produces fewer parts since v2025;
  - prints part numbers on the surface;
  - writes the assembly order to `*Sequence.txt`.
- Connectors:
  - plug (male-female) or dowel (female-female with a separate dowel);
  - designs: terrace, pyramid, claw, native (follows the cut section), lips;
  - cross-sections from triangle to octagon, circle or D-shape;
  - lap joints (v2 works on solid polygons);
  - depth ratio, size, tolerance, cut chamfer.
- Every part is saved as its own STL.

**E. Mesh → LuBan:** turns one 3D model into new models.
- Methods listed in the binary: Block, Clicker, Cocoon, Contour, Folding, Grill, Hash (plus hash surface), Helix, Pipe, Relief, Revolving, Slide, Spinner, Spring, Stack, Thermal, Tiling, Tumbler, Wave, Wireframe.
- Also described in the manual or blog: Plate, Spin, Star, Grid, Dual grid, Cell, Mold, Voronoi wireframe, Natural cut, Module.
- Key methods:
  - **Stack:** slices the model into layers, with hidden alignment dowels, hollow rings, labels, and contours split to fit the sheet.
  - **Hash:** interlocking X/Z slices with slots ("waffle"), orthogonal or radial, tilt, and slot tolerance (positive for laser, negative for printing).
  - **Plate:** finger-jointed panels from a low-poly version of the model, with a mold option.
  - **Contour:** engraved acrylic sheets that stack into a 3D image.
  - **Relief:** six directions, single or double sided, positive or negative, bury, base and border.
  - **Wireframe:** 2D laser version (red lines cut, black lines engrave) or 3D printed version.
  - **Thermal** (Oct 2026): a positive mold, a negative mold, and a flat piece that is heated and pressed between them.
- Nesting on sheets: compact, ordered or balanced. The panel shows parts count, area, weight, machine time and assembly time.
- Output: laser files as DXF, SVG and SHP; print files as STL or OBJ; plus the FGD/FGS scene pair.

**F. Mesh utilities:** Nesting (3D bin packing), Slicer (G-code, batch slicing with nesting), Boolean, Depth map, Hole fill, Hollow, Imprint, Inflate, Repair, Reverse, Densify, Simplify, Separate, Box, Transform, Align, and Explode/Select views.

**G. 2D window:** crop, join, threshold, labeling, halftone and halftone lines, bitmap-to-vector, string art, face detection/landmarks, drawing tools, Draw → Nest, Draw → Toolpath, plus the leftover metrology tools mentioned above.

**H. Model protection:** `File → Set password` saves a locked .fgs/.fgd pair that others can view but not export. Setting a password is free; unlocking a file over 5MB needs a license. MeshSafe is a separate paid product that invisibly watermarks meshes. It has a command-line version (`mscmd.exe`) and costs $240/year or $960 permanent; its unregistered limit is 10MB and its trial is 6 months.

**File formats** (from the binary's file dialogs and the manual):
- 3D in: STL, OBJ, PLY, VRML (.wrl), .fgs/.fgd, .im3, point cloud .txt, raw volume data.
- 2D in: BMP, JPG, PNG, GIF, TIFF, PCX, ICO, CUR, XBM, XPM, FLT (LuBan float image), SHP (LuBan vector shapes, not ESRI), DXF, SVG, raw u8/s8/u16/s16/u32/s32/dbl, AVI frames.
- Out: STL (one per object), OBJ (with colour), DXF, SVG, SHP, FGD/FGS, PNG/JPG/BMP/TIFF/FLT, EPS, G-code, plus Sequence.txt and Setting.txt.
- No 3MF, STEP or IGES strings appear in the binary.

## 4. Version history
- There are no version numbers. Releases are continuous: you overwrite the exe, and there is no auto-update. The Mac plist says "LuBan 2.0" and the MSI says 1.0.0; neither means anything.
- The News blog works as the changelog. It has 150 posts, all by Lujie Chen.

| Period | Main additions |
|---|---|
| 2019 | Natural cut, circular printer bed, grid cut, 2D/3D wireframe, D-shape dowels, bitmap-to-vector, staggered cut, exploded view, lithophane releases 1–7 (Apr–Sep), contour, depth map, relief and double-sided relief, cutting region, wave, cookie cutter 1–2, halftone, pillar, radial cut, morphing |
| 2020 | Extrusion 1–3, collage, cookie cutter 3–4, password-protected files, MeshSafe command line, puzzle and true-colour puzzle, spin, claw connector, spring, Mesh→Box shapes, lithophane 8–10 (balloon, colour, tattoo), Linux version (Jul 30), box 1–3, three-way morphing, cocoon |
| 2021 | Layering, multi-printer modular cut, stencil lithophane, cell, moire, eyeglasses and virtual try-on, pipe, painting, puzzle v2, block, bronze lithophane |
| 2022 | Halftone, folding, dice, dual lithophane, lenticule, bamboo-copter, native connector, bottle, frustum lithophane |
| 2023 | Wire, Slicer/G-code, flexi, batch slicing, multi-threaded modular cut, lithophane box |
| 2024 | CMYK lithophane, vector templates, star, double-sided box, painting box, string, string art, 3D halftone, Boolean, block, channel, helix, perspective frustum, pendant, non-axial cut, conic cut |
| 2025 | Voronoi wireframe, tumbler, lips connector, CMYK filament colours, hash surface, grid v1–3, multi-planar cut, separate, fewer modular parts, tiling, mouse-movable planes, whistle, spinner, slide, morphing grid, grill, ID photo, ridge, fabric, wormhole, chain mail, flexi cut, dual grid, cutter stopper, glider |
| 2026 | Lithophane 20–21, Projection (Mar 5), fidget, lap joint and v2, mold, Pandora (Jul 28), fence, moire v2, clicker and photo clicker, teacup, **Squama (Oct 1)**, Thermal (Oct 14) |

**Squama:**
- Blog text, in full: "Generate a projection image based on a structure of squama pattern."
- In the binary it sits in the Projection group (Fence / Pandora / Squama / Torch). Its settings include light source, light angle, surface angle, film size and distance, projection distance, socket, and light height.
- The YouTube video "LuBan Squama" is 15:04 long, published 2026-09-30, with 716 views and an empty description.

## 5. Pricing and licensing
- **How licenses work:**
  - A license is tied to a hardware ID shown in `Help → About`.
  - You send the ID through a web form or by email.
  - The developer adds it by hand to `MU_License.dat` in the public Drive "License" folder within 48 hours, and you download the file next to the exe.
  - Timed licenses need internet for a date check; permanent and USB licenses don't.
  - Moving a license to another computer is free. Updates are free during the licensed period, and forever for permanent or USB licenses.
  - Payment is by PayPal buttons on the site.
- **Tiers now:**
  - Unregistered: everything works, but saved results are limited to files of 1MB or less.
  - Free 30-day trial, on request.
  - 1 month $30, 1 year $150, permanent $750.
  - USB key $1200: runs on Windows 7+ and Mac, not Linux; shipping takes 1–2 months.
  - Upgrading pays only the difference, and renewal payments count toward a permanent license.
  - The manual also mentions a 3-month education license.
- **Price history:**

| Date | 1 month | 1 year | Permanent | USB | Source |
|---|---|---|---|---|---|
| Oct 2018 | – | – | – | – | Make: says pricing was announced and export needs a license |
| May 2019 | – | $30 | $150 | – | Fabbaloo snippet only (UNCONFIRMED) |
| Sep 2020 | – | $60 | $300 | – | Wayback; also a free 6-month education license |
| Jan 2021 | $14 | $70 | $350 | – | Wayback |
| Mar 2023 | $18 | $90 | $450 | $720 | Wayback |
| Feb 2024 | $20 | $100 | $500 | $800 | Wayback |
| Jan 2025 | $25 | $125 | $625 | $1000 | Wayback |
| Jan 2026 to now | $30 | $150 | $750 | $1200 | Wayback and live page |

The upgrade table on the license page also shows past 1-year prices from $20 up to $150.

## 6. Team, origin and user base
- **People:**
  - The site says development is led by Lujie Chen (Singapore) and Larry Sass (USA).
  - The Chinese Introduction PDF says the concept came from MIT and development was done at SUTD (Singapore University of Technology and Design). It also says a C++ development library is offered for third-party development.
  - Research history:
    - SUTD project "Very Large Scale Prototyping", 2012-01-18 (Chen and Sass).
    - MIT DDF "Lu Ban: Planar Modeling Design Tool", 2015 (Sass lead, Chen co-PI, with Calvin Zhong, Elisabeth Boles and Ines Ariza).
    - MIT DDF "Project LuBan", 2018 (Chen at SUTD, Sass at MIT).
  - All blog posts are by Chen, and one is written in the first person ("I will also bring Mac and Linux…"). In practice this looks like one developer.
- **Community:** Facebook group 1067858546711484, Facebook page, Telegram `t.me/luban_en`, MeWe, Instagram `@luban3d`. User-translated manuals exist in 13 languages (Fred Chan, Arthur ARobotics, Cristian Begnardi and others), and the manual credits dozens of named users' builds.
- **User signals:**
  - YouTube: 7.28K subscribers, 171 videos, 328,169 total views, channel joined 2019-01-28.
  - AlternativeTo: 0 likes, no reviews, added 2026-09-05.
  - Press: Make: (Oct 2018), Fabbaloo (May 2019 and Feb 2025), Maker Forums (Sep 2021).
  - The public `MU_License.dat` (202KB, encoded) holds about 3,100 brace-delimited records. That suggests roughly hundreds to around 1,000 registered machines, trials included. UNCONFIRMED.

## 7. UNCONFIRMED
1. OneDrive download folder and the OneDrive "Hello LuBan" link: login required, contents not seen.
2. Exact current menu placement of Plate, Spin, Star, Grid, Dual grid, Mold and Cell. They are in the manual or blog, but not in the method list I pulled from the binary.
3. What Squama physically produces beyond its one-sentence description and parameter names. I didn't watch the video or get a transcript.
4. Team size and Larry Sass's current involvement. A search summary said his MIT profile calls him a LuBan3D co-founder, but the fetched page says nothing about LuBan.
5. Facebook group size, Reddit, Thingiverse or Instructables presence: nothing found.
6. The 2019 $30/$150 prices (403 page; snippet only) and the exact date licensing started (between Oct 2018 and Jun 2019).
7. The number of licensed users (estimate from an encoded file).
8. Whether DICOM import is exposed in the UI. The DCMTK library is linked in.
9. Whether the "C++ development library" for third parties actually exists.
10. Whether CGAL is used under a commercial license (no notice found).
11. The Win32 build and the macOS 10 DMG were not inspected.
12. That no 3MF/STEP export exists: I inferred it from those strings being absent, not from testing.

Everything is in `(session scratchpad, not copied)/`. The most useful files:
- Manual as text: `docs/reference/hello-luban.txt`
- All 150 blog posts as text: `docs/research/luban-blog/posts-all.txt`
- Drive folder listing: `(session scratchpad, not copied)/gd/tree.txt`
- Downloads, each in its own folder: `(session scratchpad, not copied)/download/`