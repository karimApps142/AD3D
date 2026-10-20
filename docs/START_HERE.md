# Start here: handoff (2026-10-19)

This project began as a research session in another workspace and moved here. No app code exists yet. Everything below is what that session produced and verified.

## What was done

A 9-agent research workflow covered:
- the LuBan website, its 150 blog posts and the 123-page manual;
- the "LuBan Squama" video (226 frames looked at, plus the narration captions) and the whole YouTube channel (171 videos);
- open-source building blocks on GitHub (licences and star counts checked with `gh`);
- the Chen & Sass papers behind LuBan, plus competitors.

Two independent architecture proposals were written and then attacked by an adversarial critic. The owner then chose **personal use only** and **a desktop app, not a website**, so the plan in [PLAN.md](PLAN.md) replaces the report's browser-first recommendation.

## Key facts (verified unless marked)

- **LuBan:**
  - C++ desktop app (wxWidgets / OpenGL / CGAL / Clipper2) with about 100 generator methods in four groups: Lithophane, Photo magic, Mesh→Cut and Mesh→LuBan, plus Create items such as Eyeglasses and Projection.
  - The manual (`Hello LuBan.pdf`, Aug 2025) documents behaviour and parameters chapter by chapter, which makes it our spec source.
- **Squama** (video `Wd26qsSIRY8`, 2026-10-01):
  - Turns one photo into one plate covered in a grid of small tilted facets. Under a single low-angle light the photo appears; under normal room light the plate looks blank.
  - Menu path: File → Create → Projection → Mode = Squama.
  - Parameters seen in the video:
    - model size X and Z (snapped to whole cells);
    - cell size (default 6);
    - light source: Left / Right / Top / Bottom;
    - light angle (default 5°, range 0–20);
    - surface angle, the maximum tilt (default 10°, range 1–20);
    - thickness (default 3).
  - Print it standing up.
  - Export format UNCONFIRMED (probably STL).
- **Squama algorithm** (our reconstruction):
  - Average the image to one brightness value E per cell.
  - Tilt θ = asin(E) − α (from the Lambert shading model), limited to ±S, where α is the light angle and S the surface angle.
  - Each facet pivots about its cell centre.
  - Weld everything into one watertight surface on a base plate.
  - Prior art: `amirhertz/shadowpix` (MIT) and the papers "Reliefs as Images" (2010) and "ShadowPix" (2012).
- **Prototype results** (simulated, not printed):
  - The shaded result matched the target image at 0.89 (5° light, 10° tilt) and 0.95 (10° light). Facets shadowing their neighbours are the limit.
  - Contrast ≈ (sin(α+S)+a)/a, where a is the ambient light level. That is about 3.6:1 at 10% ambient, so a dark room matters.
- **⚠️ Prototype STLs are not print-clean.** All of them, the manifold-union ones included, fail trimesh's watertight check after welding (T-junctions and saddle pinches). The Rust mesher must fix this. See PLAN §6.
- **Useful open source:**
  - `marcelfarres/lamina`: an open-source clone of Autodesk's Slicer for Fusion 360 (stack, interlock, radial, folded, nesting). AGPL; fine to port for personal use.
  - `gregstarr/pychop3d`: cut a model to fit the printer bed (MIT).
  - Nesting: SVGnest and deepnest.
  - Geometry: manifold and Clipper2.
  - Full tables in `research/03-github-oss.md`.

## What's in this folder

| Path | What |
|---|---|
| `Hello LuBan.pdf` | The manual (the owner's copy) |
| `docs/reference/hello-luban.txt`, `hello-luban-toc.txt` | Manual text and table of contents (pdftotext) |
| `docs/reference/luban-docs/` | Official Chinese "Get started" and "Introduction" PDFs |
| `docs/research/00-final-report.md` | Final report: feature table, algorithms, effort, risks. Ignore its commercial and legal parts. |
| `docs/research/01-squama-video.md` | Squama walkthrough, parameters, UI notes, channel catalog |
| `docs/research/02-luban-site-docs.md` | Website, manual and blog survey; feature list per module |
| `docs/research/03-github-oss.md` | Open-source building blocks per feature (gh-verified) |
| `docs/research/04-algorithms-papers.md` | Per-feature algorithm specs and difficulty ratings; papers |
| `docs/research/05-market.md` | Competitors (mostly irrelevant for personal use) |
| `docs/research/06-design-web-first.md` | Superseded web plan; the core and library analysis is still useful |
| `docs/research/07-design-desktop-native.md` | Desktop plan (input to PLAN.md) |
| `docs/research/08-adversarial-critique.md` | Corrections, revised estimates, WASM benchmark results |
| `docs/research/squama-video/` | 226 frames, scene-change frames, contact sheets, captions |
| `docs/research/channel/` | All channel videos with dates, plus transcripts of 4 more videos |
| `docs/research/luban-blog/` | Text of all 150 blog posts |
| `docs/research/papers/` | Chen & Sass CAD'15/'16/'19 and Sass 2025 |
| `prototypes/` | Python Squama prototypes and the WASM benchmark (see the README there) |

**Deliberately not copied:** LuBan's binaries and installers, strings dumped from them, and the video file itself. Re-download the video with `yt-dlp https://www.youtube.com/watch?v=Wd26qsSIRY8` if needed.

## Owner's machines (answered 2026-10-19)

- **3D printer:** Creality CR-6 SE, build volume 235 × 235 × 250 mm (X × Y × Z). It is a bed-slinger (the bed moves in Y), so tall, thin parts printed standing should have their face parallel to Y.
- **Laser:** a "3015" machine (3000 × 1500 mm class). Model not given; whether it is a fiber (metal) or CO2 laser is UNCONFIRMED.
- **CNC:** a "1325" router (1300 × 2500 mm class). Model, spindle and tooling not given.

## Open questions for the owner

1. **Laser type:** is the 3015 a fiber laser (metal sheet) or a CO2 laser (wood/acrylic)? It sets the kerf and material defaults for M4.
2. **Order after Squama:** lithophane → cut-to-bed → laser slicing is the default. Change it?

## Next steps

1. Write `docs/spec/FEATURES.md` from the whole manual, blog and transcripts.
2. Install rustup and cmake, then run the spike (PLAN §2) and report the numbers.
3. Build M1 Squama end to end, with tests and benchmarks.
4. Hand the owner a test STL and a print-and-light checklist.

## Prompt to start the next session

Paste this into a new Claude Code session opened in `/Users/apple/Desktop/workspace/nextjs/AD3D`:

```text
ultracode

This is AD3D, my personal desktop app (macOS first, Windows later): a faster, better, clean-room take on LuBan (luban3d.com). Read CLAUDE.md, docs/START_HERE.md and docs/PLAN.md first. The research is already done in docs/research/ and the full manual is "Hello LuBan.pdf" (text in docs/reference/hello-luban.txt).

Do this, in order:
1. Feature spec: go through every chapter of the manual, plus the blog text and video transcripts, and write docs/spec/FEATURES.md. Cover every feature and sub-mode: its inputs, every parameter (name, meaning, default and range where stated), outputs, and how we will do it better. Mark anything the sources don't state as UNCONFIRMED. Finish with a recommended build order.
2. Initialise git and commit the research. Install what's missing (rustup with the aarch64-apple-darwin target, cmake via brew). Then run the architecture spike in docs/PLAN.md §2 (Tauri 2 + React/TS + react-three-fiber + Rust core crate) and report the measured numbers against each acceptance criterion. If a criterion fails, tell me the alternative before switching.
3. Build Squama end to end in the app: photo in → live preview with a movable light → watertight STL/3MF out, including the improvements in PLAN.md §4. Prove it with watertightness tests, a criterion benchmark and a screenshot of the preview. Commit at each milestone.
4. Give me the path to a test STL sized for my printer and a short print-and-light checklist.

My printer: <MODEL>, bed <X × Y × Z> mm. Laser/CNC: <none / model>.
Ask before anything destructive; otherwise keep going.
```
