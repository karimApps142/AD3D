# LuBan Squama: what the video shows, and a catalog of the LuBan channel

**How sure this is:** the description is empty, but the video has auto-captions with the author's spoken narration (`en-orig`). Everything below comes from those captions plus 226 frames I looked at. Inferred points and anything I could not see are marked as such.

## 1. Video metadata
- "LuBan Squama", ID Wd26qsSIRY8, channel LuBan (UChbeOwTaHYc8sxpPFp6g-nA). Uploaded 2026-10-01, 15:04 long, 716 views. Downloaded at 1280x720, 30 fps.
- The screen recording shows a Windows 11 clock reading "1/10/2026, 8:05–8:20 am". That is day/month order, so 1 Oct 2026, which matches the upload date.
- YouTube's auto-chapters:
  - 0:00 Intro
  - 1:06 Shadow Art Mechanics
  - 2:58 3D Printing Integration
  - 3:54 Feature Settings
  - 5:56 Refining Visual Parameters
  - 7:54 Printing/Quality Tips
  - 10:40 Visualization and Controls
  - 14:40 Conclusion

## 2. What Squama is
**It is not a mesh tool and not a laser-cut kit.** Squama is an image-to-relief "shadow art" generator. It turns one photo into one 3D-printable plate made of many small tilted facets. When a single light shines across the plate at a low angle, each facet looks lighter or darker depending on its tilt, and the photo appears. Under ambient light the plate looks blank.

**Where the idea came from (0:00–3:20).** The author shows the Instagram account of artist João Paulo Gonçalves (@jpgoncalvesart, "Creator of Shadow Pointillism", Deerfield Beach FL). He glues hundreds of small wooden panels by hand onto a large frame, each at a slightly different angle. The author's words: "we can use 3D printer to print the panels at slightly different angle… the process can be fully automated."

**The name.** "Squama means fish scale. I didn't use the word scale because scale is often used in 3D printing to refer to the size."

### Workflow, step by step
1. Start LuBan. A "Tip" dialog lists shortcuts, e.g. Eyeglasses / Lithophane / Photo magic under File > Create, and Mesh > Cut and Mesh > LuBan after opening a model. Click No.
2. Choose **File > Create > Projection**. The Create submenu holds: 3D shape, Bamboo-copter, Bottle, Dice, Eyeglasses, Glider, Lithophane, Projection, Photo magic.
3. A "Projection" panel opens on the left. Its **Mode** dropdown was showing "Fence"; the author switches it to **Squama**. Squama is a sub-mode of Projection, alongside Fence (and presumably the original projector mode; I never saw the dropdown open, so that is UNCONFIRMED).
4. Click **Shadow: Browse**. A standard Windows "Select image" dialog opens with the filter "All supported format (*.bmp;*.j…". He picks MonaLisa.jpg. **The input is a 2D photo**, not a mesh.
5. A plate is generated immediately in the 3D view. Five magenta spheres beside it mark which side the light comes from. They are only a visual marker.
6. He adjusts the parameters (next section). Pressing Enter regenerates the plate.
7. He turns on edge display (toolbar "Show / Hide &Edge"). This draws the cell grid in a jet colormap coloured by height. "Show / Hide &Surface" also exists.
8. He checks the result using **View > Move light** (a mouse mode) and **View > Lighting angle**, which opens a dialog: "X angle is between [0,180]. Y angle is between [-180,180]. Example: 30 20". Dragging horizontally swings the render light around the vertical axis; dragging vertically swings it around X. The Mona Lisa face shows up clearly once the render light grazes the plate from the same side as the Light source setting.
9. **Save model** becomes active once an image is loaded, but **he never clicked it in this video**. No export dialog and no file list appear.

### Parameters (defaults as shown)

| Field | Default | Behaviour from narration and frames |
|---|---|---|
| Mode | Squama | Projection sub-mode (Fence is another) |
| Shadow | Browse | Input image |
| Model size X | 300 | Snapped to a whole multiple of Cell size. Typing 200 gave 198; 202 gave 204 |
| Model size Z | 300 | Becomes 378 after the Mona Lisa loads, so it follows the image's aspect ratio. Also snapped to Cell size. 300×378 at cell 6 = a 50×63 grid; 200×252 at cell 4 = also 50×63 |
| Cell size | 6 | Size of one facet. Smaller means more cells and a sharper image. He used 6, then 4 |
| Light source | Left | Left / Right / Top / "Botton" (a typo in the UI). Sets which axis the facets tilt about. Left/Right gives vertical columns of facets; Top/Bottom gives horizontal rows, with a sawtooth visible on the side edge (f_148) |
| Light angle | 5 | Spinner. How far the light sits out of the plane, i.e. its grazing angle. Range 0–20°, capped at 20 "for now" because larger values distort the panels too much for printing |
| Surface angle | 10 | Spinner. Range 1–20°. Each facet can tilt up to ±this value. A larger value means more contrast. Above 20 creates overhangs that need support, especially with Top/Bottom light |
| Thickness | 3 | Base plate thickness (presumably mm) |
| Save model / Close | — | Buttons |

### Output geometry
- **One solid plate**, not separate parts. There are no part numbers or labels, no flat sheets, and no nesting.
- The base plate has a front surface divided into square cells. Each cell is a flat facet tilted about one axis, perpendicular to the light direction.
- Neighbouring facets are joined, with small steps where their tilts differ. Those steps show up as stripes in the render (close-up in f_122).
- **Printing advice from the author:** print it **standing vertically** for best facet resolution, "just as with a lithophane". If you print it flat, the tilted facets come out poorly.
- **Export format: UNCONFIRMED** (no save shown). By analogy with the sibling "LuBan Projection" video (2026-03-05), Save model writes an "all in one" record plus STL files, and LuBan's 2D window also writes SHP (LuBan's own format), SVG and DXF. For Squama, STL is the likely output.

### My reconstruction of the algorithm (inferred, not confirmed)
1. Load the image and convert it to grayscale.
2. Set nx = round(X / cell). Set nz from the image aspect ratio. Snap X and Z to nx·cell and nz·cell.
3. Average the image over each cell to get a brightness value g in [0,1].
4. Map each cell's brightness to a tilt θ in [−SurfaceAngle, +SurfaceAngle]. The likely rule is Lambertian: the light comes in at elevation α = Light angle, so brightness ∝ max(0, sin(α + θ)). Solve for the θ that gives the target brightness, so bright cells tilt toward the light and dark cells tilt away into shadow.
   - The author confirms that panels tilting back against the light read as black. Whether the mapping is linear or inverts this formula is UNCONFIRMED.
5. Build each facet as a plane tilted by θ about the axis perpendicular to the light: the Z axis for Left/Right light, the X axis for Top/Bottom. Its depth is offset above the base Thickness.
6. Join facets into a watertight surface with step walls between neighbours, add the back plate, and emit a triangle mesh.

Contrast is limited by SurfaceAngle and LightAngle, so the result depends heavily on how the real light is mounted. The author says as much.

## 3. UI and technology hints
- **Native Windows desktop app.** It has a classic Win32-style menu bar: File, Batch, Edit, View, Mesh, Object, Draw, Help. It uses the standard Windows file dialog and a MessageBox-style Tip dialog.
- **Status-bar text shows raw menu mnemonics**, e.g. "&Viewing angle: 30, -15, 0", "&Move light", "Show / Hide &Edge", "&Clip". Menu label strings are evidently reused as status text without stripping the "&".
- **The 3D view looks very much like MATLAB axes**: a black background, a dotted grid box, a default −1..1 axis range, a jet colormap, and the "Viewing angle az, el" convention. The install video argues against an actual MATLAB runtime, though: the MSI is about 76 MB, there is a VC++ redistributable "if crashes", and the files are a single LuBan.exe plus language folders. That points to a C++/MSVC build with a custom OpenGL-style renderer made to look like MATLAB (UNCONFIRMED).
- **Lenient number parsing.** "Cell size" accepted "4j" and treated it as 4, which looks like C atof/strtod parsing.
- **File menu:** New, Open, Assemble, Set password, Save, Close, Invoke 2D window (U), Data receiver, Create, Import, Export, Multi-view sequence, Exit, Exit all.
- **View menu:** Program output, Restore window (Alt+Enter), Full screen (F5), Window size, Zoom, Viewport, Viewing angle, Lighting angle, Reset object coordinates, Float, Shift, Rotate, Clip, Move light, Copy, Paste.
- There is a toolbar slider (probably transparency; UNCONFIRMED). One frame (f_092) shows a white background with the title "Squama".
- No version number appears in Squama. The install video says Help > About shows the exe build date.
- **From the install video (DzvYxTm8pbo, 2025-02-22):**
  - The Google Drive folders are: docs, License (holds the latest license `.dat`), mac64, linux64 ("yonto 64" in the captions, probably Ubuntu), VC redistribution, Windows 32-bit (deprecated) and Windows 64.
  - Windows 64 contains `LuBan.exe` and `setup.msi`. It installs to `C:\Program Files (x86)\LuBan3D\LuBan`.
  - The install folder has language folders (DE, EL, ES, FR, IT, MS, LT, PL, PT-BR, zh-CN, zh-TW…) and an eyeglasses data file.
  - Licensing uses a license `.dat` file that LuBan only finds under an exact name. The captions garble the name ("muore license dat", maybe my_license.dat or mu_license.dat; UNCONFIRMED).
  - A settings file is written next to the exe, so the first run should be as administrator.
  - Updating means replacing `LuBan.exe` only.

## 4. The channel: 166 uploads + 5 Shorts, no live streams
Dates are approximate, taken from YouTube's relative "x ago" labels. Videos older than about a year resolve to the year only. Exact dates I confirmed: Squama 2026-10-01, Thermal 2026-10-14, Fence 2026-08-08, Projection 2026-03-05, MeshSafe Intro 2025-07-10, Install-Win 2025-02-22, What can I do 2021-07-20, Mac install 2019-12-10.

**Descriptions:** all the video descriptions I checked are empty (7 videos). I read auto-captions instead for Projection, Fence, Thermal and Install-Win.

**Projection / shadow / light family (2026)**
- Squama 15:04 (2026-10)
- Fence 6:31 (2026-08): a 3D piece whose shadow from a point LED forms the image. Parameters: number of fence bars, fence width, shadow length, shadow offset, light distance, light height, thickness.
- Projection 10:21 (2026-03): a slide projector made of a socket and a film. Parameters: positive/negative, circle/square socket, film size, film distance, projection distance, socket size and depth, thickness, tolerance. Output: "all in one" record, film.stl, socket.stl, plus SHP/SVG/DXF.
- Moire 10:40 (2026-09); Moire 19:37 (2021)
- Lenticule 5:30 (2022)

**Thermoforming / mold**
- Thermal 12:12 (2026-10): a positive mold, a negative mold and a flat net printed flat on the bed, then heated and pressed into shape. Parameters: projection direction ±X/Y/Z, relief thickness, bury %, base thickness, border size, cell shape (triangle/rectangle/hexagon/circle), cell size, line width, thickness.
- Mold 21:05 (2026-06); Mold making 3:18 (2021)

**Lithophane** (the largest group)
- Lithophane 0:45 and Lithophane 2 13:05 (2019)
- Relief on Lithophane 1:38
- Easter egg lithophane 2:37
- Add text to Lithophane 3:35
- Lithophane Balloon 6:11, with Balloon template prep 4:35 and the MeshMixer light-socket video 13:28
- v9 Color 11:15
- v10 3:50
- Stencil lithophane 4:08
- v12 Bronze 2:19 and 12th release Bronze 4:46
- v13 Dual 2:52
- v14 Frustum 1:54
- v15 Box 4:18
- v16 CMYK 4:41
- v17 CMYK Stencil & Box 3:11
- Vector template for lithophane box 1:42
- v18 Double-sided box 4:43
- CMYK lithophane filament color 16:11
- v19 Perspective Frustum 8:12
- v20 Double-sided 7:40
- v21 Tile 13:19
- Tile: Secret Recipe 4:12
- Made with LuBan lithophane, Bronze mode 1:33

**Slicing / fabrication forms (the original core)**
- Stack 3:32, Hash 6:27, Radial 3:00, Plate 5:24, Plate 2 2:30
- Relief 4:47, Contour 5:09
- Wireframe 1:03 and 4:22, Voronoi wireframe 10:16
- 2D Nesting 6:08
- Radial cut 3:07, Pillar 3:59
- Wave 1/2/3 and Wave v2
- Cell 2:48, Cocoon 4:14, Layering 9:20
- Photo display stack 6:25
- Hash Surface 9:25
- Grid 9:29, Grid v2 2:50, Grid v3 10:29, Grid v4 Dual Grid 10:02, Morphing Grid 7:47
- Slicer 4:10, Batch slicing 11:54
- Folding 3:29, Fabric 10:29, Ridge 3:25

**Mesh cutting / modular / repair**
- Mesh Cut 25:59
- Multi printer modular cut 6:46
- Modular cut producing fewer parts 6:59
- Multi planar cut 13:21
- Non Axial cutting 9:18
- Conic cut 10:24, Conic cut 2 6:36
- Mouse movable cutting planes 3:24
- Separate 14:15
- Hollow and Cut 17:02
- Flexi 5:33, Flexi cut 12:30
- Mesh Repair 10:51
- Mesh Boolean 16:38, Application of Mesh Boolean 15:20
- Assembly Sequence 13:52
- Tiling 10:47
- Material and time estimate for big prints 11:48

**Connectors / joints**
- Native Connector 5:16
- Lips connector 3:51
- Lap joint 7:17, Lap joint v2 10:40
- Channel 14:42, Channel 2 11:20
- Cutter Stopper 3:55

**Photo-based generators**
- Photo magic Block 4:43
- Block 3:02, Block v2 5:30
- Make a coin from a photo 2:50
- Photo to Template 13:08
- Photo to depth image by Depth Anything V2 7:40
- ID Photo 10:09
- Apply a face image on a 3D model 7:35
- Overlaying photos 5:22
- Collage 5:48
- Add background shape to an image 3:15
- Halftone 11:17 and 10:11
- Painting 8:21, Painting v2 13:31, Painting v3 13:01, double sided painting box 12:14
- String 25:38, string v2 8:35, String Art 18:52
- Text outline 9:04
- Extrusion 2:30, Extrusion 2 4:04, Extrusion v3 6:43
- Morphing 3:40, Morphing v3 4:01, Morphing v4 3-way 3:55
- Importing and rendering 3D voxel images 9:36

**Boxes / containers**
- Box 9:43, Box v2 9:23, Box v3 23:15, Box v3 amendment 7:06
- Box intrude inner pattern 10:51
- Bottle 12:26
- Tumbler 4:36
- Teacup 9:31 (2026-09)
- Pandora 24:22 (2026-08)

**Toys / gadgets**
- Puzzle 6:23, A puzzle without base 8:06
- Spin 6:00 (plus a user video by Mike Settles 8:33)
- Spring 4:03
- Cookie cutter v1–v4
- Dice 3:01
- Bamboo copter 3:30
- Glider 10:40
- Star 2:03
- Wire 4:02
- Pipe 19:47
- Helix 13:47
- Spinner 9:53
- Whistle 8:47, plus "Look like a whistle. Work like a whistle." 20:10
- Slide 15:04
- Wormhole 10:30
- Grill 13:16, Grill base thickness 4:38
- Fidget 6:00
- Clicker 11:22, Photo clicker 15:35

**Wearables**
- Eyeglasses 17:57
- Eyeglasses Virtual Try on 2:06
- Pendant 4:48
- Making a necklace 7:07
- RingTutorial (user Amanda Napthine) 3:45

**MeshSafe** (a separate 3D-model signing and leak-tracking tool)
- MeshSafe command line 1:39 (2020)
- MeshSafe Introduction 34:07 (2025-07-10). Chapters: signing a 3D model, tracking leaked models, batch, scan.

**Overview / install / promo**
- What can I do with LuBan 36:19 (2021-07-20)
- Magic of LuBan 3:14
- LuBan for makers ad 0:27
- Mac install 2:56
- install and update on Win 16:42

**Shorts (5):** Bronze Lithophane, Tile lithophane, Moon Light, Block made by LuBan user Jared Wade, Connector.

Rough upload counts by year: 2019: 16, 2020: 35, 2021: 18, 2022: 10, 2023: 6, 2024: 20, 2025: 43, 2026: 18. The channel is still active, with a new feature roughly every 2–4 weeks in 2026.

## 5. The 6 most representative frames
1. `docs/research/squama-video/frames/f_060.jpg`: the Squama parameter panel, empty (3:56)
2. `docs/research/squama-video/frames/f_055.jpg`: File > Create submenu with Projection selected (3:36)
3. `docs/research/squama-video/frames/f_096.jpg`: edge view, the cell grid in a jet colormap (6:20)
4. `docs/research/squama-video/frames/f_122.jpg`: close-up of the tilted facet columns with Left light (8:04)
5. `docs/research/squama-video/frames/f_167.jpg`: the View menu, with Lighting angle and Move light (11:04)
6. `docs/research/squama-video/frames/f_225.jpg`: final 200×252, cell-4 Mona Lisa rendered with grazing light (14:56)

Also useful: `docs/research/squama-video/frames/f_148.jpg` (Top light, horizontal facet rows) and `docs/research/squama-video/frames/f_059.jpg` (the Fence mode panel).

## Files
- Video: `(session scratchpad, not copied)/video/squama.mp4`
- Metadata: `(session scratchpad, not copied)/video/squama.info.json`
- Captions: `docs/research/squama-video/captions.txt`
- Frames, every 4 s (226 total): `docs/research/squama-video/frames/`
- Scene-change frames (93): `docs/research/squama-video/scenes/`
- Contact sheets with timestamps: `docs/research/squama-video/sheets/`
- Channel list with approximate dates: `docs/research/channel/videos_dated.txt`
- Shorts list: `docs/research/channel/shorts.txt`
- Other videos' captions as text: `docs/research/channel/subs/` (VA67Jm3ijHg, t2-1zMjWUAw, F20FZvL0r9g, DzvYxTm8pbo)
- yt-dlp venv: `(session scratchpad, not copied)/tools/venv`