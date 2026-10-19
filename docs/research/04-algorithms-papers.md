RESEARCH RESULT: LuBan (luban3d.com): algorithms and academic background

Status: done. The best source turned out to be the official 123-page manual, "Hello LuBan.pdf" (LaTeX, dated 19 Aug 2025). It sits in the public Google Drive download folder under Doc/English. I downloaded and read it as data only. The other main sources were the CAD'15, CAD'16 and CAD'19 extended abstracts by Chen and Sass, which give the actual algorithms, and the Sass 2025 TAD paper. Anything I could not confirm is marked UNCONFIRMED.

Local files:
- Hello LuBan.pdf (the manual)
- docs/reference/hello-luban.txt (clean text extract)
- docs/research/papers/CAD16-chen-sass.txt, c19_358.txt, c15.txt (CAD'16, CAD'19, CAD'15 abstracts)
- docs/research/papers/Sass-2025.txt (Sass 2025 paper text)
- docs/research/squama-video/captions.txt (Squama video transcript, downloaded by a sibling task)

Corrections to the brief:
- **Not Windows-only.** The Drive folder has "Win 32", "Win 64", "Mac 64" and "Ubuntu 64" builds. The manual has install sections for Windows, Mac and Linux.
- **Prices (license page):** $30/month, $150/year, $750 permanent, $1200 USB license.
- **No eyeglasses, Squama or "module" sections in the manual.** The feature set in the manual is grouped as File→Create→Lithophane, File→Create→Photo magic, Mesh→Cut and Mesh→LuBan.
- **Recent features exist only as blog posts and videos.** These are: Squama (1 Oct 2026), Thermal, Teacup, Photo clicker, Clicker, Moire, Fence, Pandora, Mold, Lap joint v1/v2, Fidget, Projection, Lithophane v20/v21, Glider, Dual grid, Flexi cut and Block v2. Every news post is authored by Lujie Chen.
- **Sister product:** MeshSafe, a separate program that embeds an invisible signature in a mesh. It costs $240 per year or $960 permanent.

---

## A. Publications and patents

**Core Chen and Sass papers.** Confirmed via Crossref, OpenAlex and the MIT DDF publications page.

1. Chen, L. & Sass, L. (2016). "Fresh Press Modeler: A generative system for physically based low fidelity prototyping." *Computers & Graphics* 54:157–165. https://doi.org/10.1016/j.cag.2015.07.003
   - The abstract is paywalled. A ResearchGate snippet seen via search describes it as approximating a 3D textured mesh with a small number of planar polygons that form a closed surface, joined by interlocking finger joints. This is the origin of the Plate method.
   - The CAD'19 paper cites it as prior work on segmenting large parts.
2. Sass, L., Chen, L. & Sung, W.K. (2016). "Embodied prototyping: exploration of a design-fabrication framework for large-scale model manufacturing." *CAD&A* 13(1):124–137. https://doi.org/10.1080/16864360.2015.1059202
   - Free extended abstract: "Exploration of a Design Framework for Large-scale Model Manufacturing," CAD'15, 12–15. https://doi.org/10.14733/cadconfP.2015.12-15 (PDF: http://www.cad-conference.net/files/CAD15/CAD15_12-15.pdf)
   - Content: "Scalable Planar Structure" (SPS), meaning interlocking slotted planes with symbol labels, contours split into sub-planes under a maximum part length (45 cm in the example), and a "planar volume" of ring-like horizontal layers with male/female slots.
3. Chen, L. & Sass, L. (2016). "Generative computer-aided design: multi-modality large-scale direct physical production." *CAD&A* 14(1):83–94. https://doi.org/10.1080/16864360.2016.1199758
   - Free extended abstract: "Generative CAD: New Design Tool for Direct Physical Production," CAD'16, 1–4. https://doi.org/10.14733/cadconfP.2016.1-4 (PDF: http://www.cad-conference.net/files/CAD16/CAD16_1-4.pdf)
   - Defines the three core methods:
     - 1D contouring = Stack. Parallel plane–mesh intersection, partitioning of contours larger than the cutter bed, hollow "inner rim", automatic alignment holes. Example: a bust of 5425 parts.
     - 2D contouring = Hash. Orthogonal planes plus automatic slots. Slot size comes from local geometry, slot width from sheet thickness, plus a user tolerance tied to material and laser settings.
     - Plate forming. Planar faces with finger joints on shared edges; the result is watertight and usable as casting formwork.
   - Cites Schulz et al. 2014 and Schwartzburg & Pauly 2013.
4. Su, Z., Chen, L., He, X., Yang, F. & Sass, L. (2018). "Planar structures with automatically generated bevel joints." *Computers & Graphics* 72:98–105. https://doi.org/10.1016/j.cag.2018.02.005 (MIT DSpace: https://dspace.mit.edu/handle/1721.1/124169)
   - Plate-like parts with friction-locking angled edges. The bevel angle comes from the dihedral angle and the sheet thickness. Because the edges are angled, they need a 3-axis CNC or a 3D printer, not a laser.
5. Camburn, B., Wee, Y., Chen, L., Sass, L. & Wood, K. (2019). "Recursive Segmentation: An Approach to Produce Large and Complex Designs." CAD'19, 358–362. https://doi.org/10.14733/cadconfP.2019.358-362 (PDF: http://www.cad-conference.net/files/CAD19/CAD19_358-362.pdf)
   - Contains the explicit LuBan fit-to-bed algorithm (see B.12).
6. Dritsas, S., Chen, L. & Sass, L. (2017). "Small 3D Printers / Large Scale Artifacts – Computation for Automated Spatial Lattice Design-to-Fabrication with Low Cost Linear Elements and 3D Printed Nodes." CAADRIA 2017, 821. https://doi.org/10.52842/conf.caadria.2017.821
   - Lattice made of dowels plus 3D-printed nodes. It is related to "wireframe" in spirit, but LuBan's Wireframe works differently.
7. Chen, L., Sass, L., Sung, W.K. & Noel, V. (2013). "Surface skeleton generation based on 360-degree profile scan." Proc. SPIE. https://doi.org/10.1117/12.2022162
   - Earliest version of Hash: horizontal and vertical ribs from scan data, laid out on a 2D canvas for laser cutting, assembled by codes, at any scale.
8. Sass, L. (2025). "Plane Delivery: Towards a Physical Grammar for Large-Scale Digital Fabrication." *TAD* 9(2):279–287. https://architecture.mit.edu/sites/default/files/publications/2025-Sass.pdf
   - Defines a 9-function "Planar Grammar":
     - Stage 1: contouring (dowels), hashing (slots), plate forming (fingers).
     - Stage 2: integration (mortise and tenon), splitting, rejoining (buttons).
     - Stage 3: numbering (bottom-up, then left to right), development (3D→2D), bin packing (sort largest to smallest, place the largest on a sheet first).
   - It states that Stages 1 and 3 "have been computer-programmed in LuBan3D", while Stage 2 is not coded.

**Project pages (MIT DDF):**
- "Planar Structures / Very Large Scale Prototyping" (2015): biaxial contouring, tiling of contours to the machine limit, "segment lapping" with Cross Overlapping Depth (Od) and Split Vertex (spl). https://ddf.mit.edu/?p=1042
- Scalable Contouring (2014): https://ddf.mit.edu/?p=677
- Lu Ban Planar Modeling Tool (2015): https://ddf.mit.edu/?p=689
- Project LuBan (2018): contouring, cross-contour, finger jointing, sheet packing. https://ddf.mit.edu/?p=814
- LuBan Stacking (2019): an alignment key between each pair of layers, every element numbered. https://ddf.mit.edu/?p=824

**SUTD:** "Very Large Scale Prototyping" news item: https://www.sutd.edu.sg/technical-release-listing/very-large-scale-prototyping/

**"Fresh Press Modeler"** is confirmed as item 1 above (it is not a product name).

**Patents.** I searched Google Patents by inventor for "Lawrence Sass", "Larry Sass" and "Lujie Chen", and also SUTD plus Chen. I found **no patent covering LuBan or its methods.** The only Lujie Chen / SUTD hit is unrelated: US20210059762A1, a medical motion-compensation platform. Absence of evidence is not clearance, so I mark freedom-to-operate as UNCONFIRMED.

Third-party patents and prior art worth knowing:
- US9754412B2 / US20140253550, "Techniques for slicing a 3D model for manufacturing." Autodesk; inventors Bachrach and Griffith; priority 2013-03-11. Google lists it as Active, adjusted expiry 2033-08-12. It covers slices with connection points plus a material-minimizing layout, which overlaps conceptually with Stack/Hash and 123D Make. https://patents.google.com/patent/US9754412
- A CNR (Cignoni) Italian filing on cross-joined interlocking planar structures (field-aligned mesh joinery).

**Academic prior art for re-implementation:**
- Schwartzburg & Pauly 2013, "Fabrication-aware design with intersecting planar pieces," CGF. https://doi.org/10.1111/cgf.12051
- Hildebrand, Bickel & Alexa 2012, "crdbrd: shape fabrication by sliding planar slices," CGF (slot direction and assemblability).
- McCrae, Singh & Mitra 2011, "Slices."
- Cignoni et al. 2014, "Field-aligned mesh joinery," TOG.
- Chen, Sitthi-amorn, Lan & Matusik 2013, "Computing and fabricating multiplanar models," CGF (Plate).
- Luo, Baran, Rusinkiewicz & Matusik 2012, "Chopper," TOG 31(6):129 (fit-to-bed partitioning). https://gfx.cs.princeton.edu/pubs/Luo_2012_CPM/chopper.pdf
- Vanek et al. 2014, "PackMerger," CGF. UNCONFIRMED: the search did not surface it; this is from memory.
- Mitra & Pauly 2009, "Shadow Art" (= Photo-magic Morphing).
- Alexa & Matusik 2010, "Reliefs as Images," and Bermano et al. 2012, "ShadowPix" (= Squama).
- Weyrich et al. 2007, "Digital bas-relief from 3D scenes" (Relief).

## B. Per-feature algorithm spec

Difficulty scale: 1 = trivial, 5 = research-grade. Facts from the manual are marked [M] and facts from papers [P].

**1. Lithophane** (Flat, Arc, Poly-face, Cylinder/Cone/Vase, Sphere, Spiral, Stencil, Balloon, Tattoo, Color, Tile, double-sided). Difficulty 2 for flat/arc/cylinder, 3 for sphere/balloon, 4 for tattoo on an arbitrary mesh.
- Algorithm:
  - Convert the photo to grayscale.
  - Resample at the resolution r in mm/pixel; the default is 0.1 [M].
  - Set thickness t = tmin + (1 − I)(tmax − tmin), i.e. inverse luminance. "Negative" flips this [M].
  - Build a heightfield grid mesh: the front surface is displaced and the back is smooth. "Exterior" puts the rough side toward the viewer, "interior" toward the light [M].
  - For curved shapes, map pixel (u,v) onto the parametric surface (cylinder, cone, sphere, spiral) and offset along the surface normal by t. Each point emits two vertices, inner and outer, stitched into a closed shell.
  - Options: border (dark border thickness must exceed tmax by 5% [M]), base/socket, hanger, anti-wobble support, backlight preview.
- Balloon: inflate a black-and-white template mask into a 2-sided pillow with an "expansion" parameter [M], then map up to two photos onto the front and back. UNCONFIRMED implementation: likely distance-transform or Poisson inflation, as in Teddy-style inflation.
- Tattoo: wrap the photo around an axis (±X/±Y/±Z) onto any closed mesh and offset along the normals. It needs dense uniform remeshing. It is slow, defaulting to 10 mm/px for preview. A hollow input produces four surfaces and printing errors [M].
- Color lithophane: export a color image with the intensity component removed, to print on transparency and use as a filter. The physical size is written into the filename [M].
- Edge cases:
  - aspect ratio is locked to the photo;
  - very large triangle counts (the manual advises against mesh simplification because it makes resolution non-uniform);
  - thin spots below the nozzle width;
  - sphere poles and seam stitching;
  - self-intersection when offsetting concave tattoo regions.

**2. Stack** (stacked slices = "1D contouring"). Difficulty 3.
- Algorithm [P][M]:
  - Intersect the mesh with N parallel planes.
  - Chain segments into closed polylines; nested loops give outer and holes.
  - Optionally hollow each slice into a ring by inward offset with "ring width" [M].
  - Place square alignment dowel holes inside the overlapping region of adjacent slices. A hole is rejected if it is larger than about 1/3 of the smallest contour dimension [M].
  - If a contour exceeds the sheet or bed size, segment it into pieces. CAD'15 calls this splitting into sub-planes; Sass 2025 adds that splits avoid vertices and corners.
  - Label every piece layer.contour[.segment], e.g. "10.3.4" [M], engrave the labels, nest the parts on sheets, and export DXF/SVG/SHP plus STL [M].
- Edge cases:
  - non-manifold or open meshes give unclosed loops; repair or close them first;
  - tangent planes and degenerate edges;
  - tiny islands;
  - thin rings breaking;
  - segment seams lining up across layers (stagger them);
  - kerf;
  - dowels that do not fit thin layers.

**3. Contour** (laser-engraved acrylic stack, distinct from Stack). Difficulty 2.
- Slice into contours, engrave each on a transparent sheet inside a frame shape with clearance, add an optional base light-socket tab, and nest [M].

**4. Hash** (interlocking waffle = "2D contouring"). Difficulty 4.
- Algorithm [P][M]:
  - Slice in X (vertical) and Z (horizontal) layers, or radially around a Z axis, with optional tilt [M].
  - For each X–Z contour pair, compute the 2D segment where their planes intersect inside both polygons.
  - Cut a slot of width = sheet thickness + tolerance. The slot is half the overlap length deep: from the top on one part and from the bottom on the other (Sass 2025 calls the overlap depth "Od").
  - Tolerance sign [M]: positive for laser, so the slot is narrower than the sheet to compensate for burn (kerf); negative for 3D printing.
  - Label "X…" or "Z…", ring-hollow, segment to sheet size, and nest.
- Edge cases:
  - contours that do not intersect any perpendicular piece cannot attach (the manual admits this [M]);
  - a slot cutting a thin piece fully in two;
  - multiple slots on one intersection line when the polygons are non-convex;
  - insertion order and assemblability: slots must all open in one direction per family (crdbrd);
  - slot collisions with holes;
  - radial variant geometry;
  - kerf and material springback.

**5. Plate** (planar faces with finger joints). Difficulty 4 (5 with bevel joints).
- Algorithm [P][M]:
  - Input must be a thin surface, not a solid [M].
  - "Low poly" simplification to large planar faces, with the rate auto-chosen by LuBan [M]. Fresh Press approximates the mesh by a few planar polygons; I suggest variational shape approximation or QEM plus coplanar merging.
  - For each shared edge, compute the dihedral angle and offset each plate by thickness: inward when "Mold" = Yes so the interior volume matches; otherwise outward [M].
  - Split each edge into an equal number of fingers, adjusting the width per edge so both sides pair [M]. Alternate tab and notch, with the depth set by thickness and dihedral angle.
  - Tolerance sign as in Hash.
  - The bevel-joint variant cuts angled edges for CNC or printing [P].
- Edge cases:
  - very acute or reflex dihedral angles;
  - edges shorter than one finger;
  - vertices where three or more plates meet (corner clearance);
  - non-planar input faces;
  - plate self-overlap after offset.

**6. Relief** (2.5D from a 3D model). Difficulty 2 (3 with real bas-relief compression).
- Algorithm [M]:
  - Choose a direction (±X/±Y/±Z), rasterize a depth map (z-buffer), and build a heightfield.
  - Options: single- or double-sided, positive or negative (sunken, usable as a mold), relief thickness, "bury" (clip the lower depths so only the top survives), base shape and thickness, border size and thickness, clearance.
  - No undercuts, so it is CNC-friendly.
- UNCONFIRMED: whether it uses gradient-domain bas-relief compression (Weyrich et al.) or plain linear depth scaling. "Relief thickness" plus "bury" suggests linear scaling plus clipping.
- Edge cases: depth discontinuities give vertical cliffs; thin features are lost at low resolution.

**7. Module / Mesh→Cut** (Planar, Regional, Natural, Grid, Radial, Modular cut, plus connectors). Difficulty 4.
- Planar cut: plane Ax+By+Cz+D = 0. Split triangles and triangulate the cut cross-section ("close cut") so each part is solid [M].
  - Regional cut: the plane is limited to a disk, and only components fully separated by it are cut [M].
  - Natural cut: searches for local concave regions near the guide plane to make a curved cut, falling back to planar if that fails [M]. UNCONFIRMED: probably a min-cut or graph cut on a band around the plane.
- Grid cut: N×M×K planes, optionally staggered per Z layer for strength [M]. Radial cut: planes around an axis [M].
- Modular cut (the fit-to-bed guarantee): see item 12.
- Connectors [M]:
  - Plug (male/female, auto-sized) or dowel (female/female plus a separate pin, user-sized; one user used 1.7 mm filament as dowels).
  - Designs: terrace, pyramid, claw. Several cross-section shapes; a circular shape does not stop rotation.
  - Depth ratio = depth / cross-section size, with the sign flipping male and female.
  - Each connector is collision-checked against the part surface. A failing plug is halved and rechecked, then dropped; a failing dowel is dropped.
  - Placement is automatic, so the count cannot be predicted. Rule of thumb: dowel size < 1/3 of the section width.
  - Tolerance: −0.2 mm gives a 0.4 mm gap. There is an end gap for leftover support material, and the walls are slightly drafted for a press-fit.
  - "Lap joint" (2026) covers thin-wall sections where plugs or dowels cannot fit.
- Edge cases:
  - non-watertight input gives cap triangulation failure (the manual mentions "Tiny N" debris parts caused by model defects);
  - cross-sections with holes or nesting;
  - connector punch-through;
  - assembly blocking, which is handled by Sequence.txt.

**8. Wireframe** (2D and 3D). Difficulty 2 for 2D, 3 for 3D.
- Uses the Hash layer system (orthogonal X/Y/Z or radial, with tilt) [M].
- 2D: project the section curves to a drawing, with the red boundary cut and the black lines engraved, giving a laser-engraved 3D illusion.
- 3D: sweep each section polyline as a tube or square wire of normalized thickness 1–5 and union them into a printable lattice. Prints with no support, relying on bridging [M].
- Edge cases: robust union of thousands of tubes (use a Manifold-type boolean library or a voxel/SDF approach); wire thickness relative to bridging distance.

**9. Eyeglasses.** Difficulty 3. Mostly UNCONFIRMED.
- The only official statement is the homepage line "Designs eyeglasses frames from a template image." It is not in the manual.
- Inferred pipeline:
  - threshold the template, trace the lens outline, offset to rim width, mirror for left and right;
  - add a bridge, nose pads and hinge geometry;
  - extrude the front, add temple arms of a given length and bend, add a lens groove;
  - export STL.
- Edge cases: hinge tolerance; lens groove fit; fit to face (pupillary distance and bridge width parameters).

**10. Photo Magic.** Difficulty 2–3 per method. It is HEURISTIC, NOT ML (see section C).
- Common pipeline [M]: the photo is "first manually converted to black and white", by threshold of a chosen channel. Then come contour extraction (outer boundary vs "inner pattern", with an auto-generated boundary when features are disjoint), polygon offset and extrusion. 2D tools include threshold, distance transform, labeling and crop [M].
- Methods [M]:
  - Balloon: a solid inflated mask.
  - Box: base plus lid. Lid types basic/twist/flat (magnet sinks)/inlay; inner pattern ignore/extrude/indent/hollow; nested boxes; chamfer; tolerance; box light.
  - Cell: halftone, with holes sized by intensity; resolution = cell/10.
  - Cutter: cookie cutter with blade/base, flip, text.
  - Extrusion: with elevation, angle, taper "expansion", base and keychain hole.
  - Layering: multiple photos stacked into frames; DXF/SVG/STL output.
  - Morphing: 2–3 silhouettes along orthogonal axes, intersected as extrusions. This is Shadow Art; there is also a negative version.
  - Painting: "under development".
  - Pillar: grayscale-driven pillar profiles (line, ellipse, sawtooth, sine, random) that show the photo from the front only.
  - Puzzle: a grayscale heightfield (min/max thickness) cut into X×Y jigsaw pieces with tolerance. It also works on a 3D model through Mesh→Depth map exported as FLT [M].
- Newer posts only: Teacup, Photo clicker, Fidget, Block v2 chainmail, Projection, Fence, Moire.

**11. Squama** (fish-scale shading panel). Difficulty 2–3. Source: video transcript.
- Under Projection mode = Squama. It reproduces an Instagram artist's hand-placed tilted wooden panels (the artist is not named in the transcript; UNCONFIRMED who), as a single 3D print.
- Parameters:
  - light side: left, right, top or bottom;
  - model size, snapped to an integer multiple of cell size in X and Z;
  - cell size;
  - light angle: elevation above the plane, 0–20°;
  - surface angle: maximum ± tilt, 1–20°, which sets contrast;
  - thickness.
- Algorithm (my reconstruction, consistent with the video):
  - Make a grid of cells, each one planar panel.
  - Per pixel or cell intensity I, tilt the panel about the axis perpendicular to the light direction by α ∈ [−αmax, +αmax].
  - Under grazing light at elevation θ, Lambertian irradiance ∝ sin(θ + α), and it is 0 (self-shadow, black) when α ≤ −θ. Invert for α given the target I.
  - Each tilted panel rises from a common base, giving a sawtooth profile with step height ≈ cell·tan α, which also casts shadows on its neighbors.
- The video recommends printing vertically for resolution and capping tilt at 20° to avoid supports and distortion (top/bottom light gives overhangs). The in-app preview needs a "move light" mode.
- Related academic work: Reliefs as Images; ShadowPix.
- Edge cases: ambient light hides the image (by design); cast-shadow coupling between cells; printer resolution relative to cell size.

**12. Fit-to-bed guarantee / partitioning** (Modular cut). Difficulty 3 for the basic version, 4 with multiple printers and assembly order.
- Explicit algorithm from CAD'19 [P]:
  1. Compute bounding extents in X, Y, Z and the ratio to the build volume per axis.
  2. Pick the axis with the largest excess factor.
  3. Intersect the mesh with one or several evenly spaced cutting planes, splitting triangles.
  4. Generate new cap triangles to close each piece.
  5. Re-check the size of each new closed mesh and cut again if needed, until all pieces fit on that axis.
  6. Repeat for the next axis.
  - The criterion is pluggable (recursive segmentation); a "no overhang" mold criterion was done manually in LuBan.
- Manual additions [M]:
  - single printer (box or cylinder bed) or multiple printers with relative speed and quantity;
  - greedy load balancing so all printers finish at about the same time;
  - guarantee that each part fits a printer, allowing a 90° rotation about Z;
  - assembly order = the reverse of the cut tree, written to "* Sequence.txt";
  - optional imprinted part numbers with prefix and start number;
  - tiny-part debris output.
- Edge cases:
  - non-manifold input breaks capping;
  - cuts through thin necks leave fragile slivers;
  - plane through a vertex or edge (needs epsilon handling);
  - connector placement on small sections.
- Chopper (Luo 2012) is the optimization-based research alternative (BSP search with seam, connector and assemblability objectives).

**13. Part labeling.** Difficulty 2 for 2D engrave labels, 3 for 3D imprinted labels.
- Hierarchical IDs [M]: layer.contour.segment for Stack, with an X/Z prefix for Hash, and "Part N" or "P{printer} Part N" for Modular cut.
- Numbering order (Sass 2025): bottom-front first, increasing with elevation, then left to right.
- 2D: single-stroke font engraved inside the polygon. Find a placement point with an inward offset or pole-of-inaccessibility so the text fits.
- 3D: boolean-subtract text on a cut face.
- Edge cases: text too large for a small part; text crossing a slot or hole; mirrored text on flipped parts.

**14. Nesting.** Difficulty 3 for a greedy heuristic, 4–5 for good true-shape packing.
- Modes [M]: compact (pack regardless of order), ordered (consecutive labels per sheet), balanced (a compromise).
- Sass 2025 describes the original method: sort polygons largest to smallest and place the largest on a sheet first, i.e. first-fit-decreasing bin packing.
- Modern implementations: no-fit-polygon plus bottom-left fill plus GA (SVGnest/Deepnest, libnest2d). Use Clipper2 for offsets and kerf.
- Edge cases: kerf and part spacing; holes inside parts (nest small parts into holes); rotation constraints for wood grain; sheet margins.

**15. Mesh processing.** Difficulty 3 using libraries, 5 robust from scratch.
- Menu items in the manual: Simplify, Densify, Depth map, Statistics, Transform, Explode/Restore [M]. Also Cocoon (outer shell plus clearance around the model, a "native" offset, mold use), Spin (revolve, duplicate, deform), Spring (layered compressible shell), Wave (point-source sinusoidal displacement), Mold, Thermal, Flexi cut, Dual grid [M plus blog].
- Building blocks: QEM simplification, Loop or midpoint subdivision, SDF offset (cocoon, mold), robust booleans, hole filling.
- The non-manifold input repair pipeline is the real cost.

**16. Kerf and tolerance (cross-cutting).** A single user-calibrated "tolerance" is reused everywhere [M]:
- positive for laser (compensates for burned material);
- negative for FDM, starting at −0.2 mm;
- it applies to slots, fingers, plugs and lids.

## C. Photo Magic: heuristic or ML?

**Verdict: HEURISTIC / classical image processing.** I found no evidence of ML.
- The manual says photos are "first manually converted to black and white", for example by thresholding the blue channel of a sunflower photo.
- Pillar and Puzzle "directly use intensity information", meaning luminance to height or profile.
- The 2D tools are threshold, distance transform, labeling and crop.
- Nothing in the manual mentions depth estimation, neural networks or AI.
- The 3D "puzzle of a model" uses a rendered z-buffer depth map, not monocular depth.
- So Photo Magic = binarize, then contour or vectorize, then offset, extrude or inflate, or luminance to heightfield.

**Modern options for our version** (licenses checked via GitHub, Oct 2026):
- Monocular depth (photo to relief, lithophane-plus, or real 3D relief):
  - Depth Anything V2 (repo Apache-2.0; the Small weights are Apache-2.0, Base/Large/Giant are CC-BY-NC-4.0, so commercial use means Small only);
  - Marigold (Apache-2.0);
  - Apple Depth Pro (custom license, check before use).
- Image-to-3D mesh:
  - TripoSR (MIT);
  - Microsoft TRELLIS and TRELLIS.2 (MIT);
  - Stable Fast 3D (Stability community license);
  - Hunyuan3D-2 / 2.1 (Tencent license that excludes the EU, UK and South Korea).
- Background removal and segmentation to replace manual thresholding: BiRefNet (MIT), SAM-family.
- Vectorizing: OpenCV findContours plus Douglas-Peucker, or potrace (GPL; avoid if closed-source).
- Geometry stack:
  - Manifold (Apache-2.0) for robust booleans;
  - Clipper2 (Boost) for 2D offset, kerf and slots;
  - libigl (MPL-2.0);
  - trimesh (MIT) for slicing and capping prototypes;
  - CGAL (GPL or commercial);
  - libnest2d (LGPL) or a Deepnest-style no-fit-polygon nester.

## D. Overall build difficulty

- **Low (1–2):** Flat lithophane, Contour, Relief, Extrusion/Cutter/Box, Squama, Cell/Pillar, Morphing, 2D Wireframe, 2D labels.
- **Medium (3):** Stack with segmentation and dowels; curved and balloon lithophanes; Puzzle; 3D wireframe; greedy nesting; basic fit-to-bed axis cutting; Photo Magic with ML depth.
- **Hard (4):** Hash with slots and assemblability; Plate with low-poly and finger joints; Modular cut with connectors, multiple printers and assembly order; tattoo lithophane; good true-shape nesting.
- **Risk drivers:** robust handling of non-manifold, self-intersecting real-world STLs (slice, cap, boolean), plus kerf and tolerance calibration. LuBan exposes a single tolerance knob instead of solving these automatically.

**UNCONFIRMED items:**
- the eyeglasses pipeline;
- the internals of Natural cut and Balloon inflation;
- whether Relief compresses depth;
- the Squama shading formula (reconstructed, not stated);
- the PackMerger citation;
- the absence of patents (search-based only).