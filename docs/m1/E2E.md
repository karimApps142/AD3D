# M1 end-to-end proof (verify round 2)

2026-10-20, on `main` at `d807bf9` ("M1: fix verification findings (round 1)"). Every output, log, screenshot and bench number below is from this run. The machine was shared with other agents (load 2–4.5), so the timings are indicative.

## What was tested

- **Input:** `tests/fixtures/mona-lisa.jpg`, the Mona Lisa (C2RMF retouched, public domain) from Wikimedia Commons. It is 1006 × 1500 px, 770 KB, and tracked in git.
- **Printer:** the default profile, Creality CR-6 SE: 235 × 235 × 250 mm, bed moves in Y, 8 mm margin.
- **Two code paths:**
  - **CLI:** release `target/release/ad3d`, every run with `--strict` (self-intersection stage on).
  - **App:** the `SquamaState` methods behind the Tauri commands (`squama_image_open`, then `generate_squama` at preview and then final quality, `squama_target`, `squama_tile_plan`, `export_squama`).
    - Harness: `src-tauri/src/e2e_m1.rs`. Run it with `cargo test -p ad3d-app e2e_m1_app_exports -- --ignored --nocapture`.
    - The live app run in the Screenshot section covers the IPC layer itself.
- **Logs:** `docs/m1/logs/`:
  - `cli-*.log`, `app-e2e.log`
  - `validate-ad3d.log`, `validate-summary.tsv`
  - `check_mesh.log`, `check_3mf_trimesh.log`
  - `bench.log`, `tauri-dev.log`

## Result in one paragraph

Generation and export work end to end on both paths.

Every part of every output passes all three checks:
- Each part is a closed, consistently wound mesh with positive volume.
- Each part has 0 self-intersecting triangle pairs.
- Each part sits inside its bed (except the LuBan default, which is expected and warned).
- The three checkers (`ad3d validate --strict`, `tools/check_mesh.py`, and an independent trimesh check of each 3MF object) agree on all 56 files.

The four round-1 fixes I could check end to end hold:
- The tile plan shown equals the plan cut.
- The validator accepts the splints.
- The CLI output is rounded.
- The photo entry point loads the image size, and the first plate is generated at final quality.

There are **two new problems and one unfixed one**, ranked at the end. None of them breaks the geometry; both of the top two are about downward ledges, which can sag in a standing print.

## Outputs

All paths are relative to `docs/m1/out/`. Each STL has a matching 3MF in the same folder, with every part placed on its bed.

| Case | CLI flags (beyond `--image … --format stl,3mf --strict`) | CLI output | App output | Plate / parts |
|---|---|---|---|---|
| LuBan default (300 / 6 / 5° / 10° / 3) | `--tiles none --orientation flat` | `cli/default/` | `app/luban-default/` | 50 × 75 cells, 300.0 × 450.0 mm, 25 154 tris. Warns `part_exceeds_bed`, `fitsBed: false` (expected) |
| CR-6 SE, one piece, standing | `--width 160 --tiles none --orientation standing` | `cli/cr6-onepiece/` | `app/cr6-onepiece/` | 27 × 40 cells, 162 × 240 mm. Prints 3.8 × 162 × 240 (face along Y), 2 quarter turns, ledge 0.33 mm. Warns `tall_standing_tile` (63×) and `brim_recommended` |
| 300 × 378, tiled, keys, stand | `--width 300 --height 378 --tiles auto --keys pegs --stand --orientation standing` | `cli/tiled-300x378/` | `app/tiled-300x378/` | 2 × 2 tiles, cols 25/25 × rows 27/36 (Y 162/216 × Z 150 mm, height/footprint 39.6, ledges ≤ 0.52 mm). `key_splints` (21 shells), 2 stands. Two beds, two 3MFs: `squama.3mf` (4 tiles + splints), `squama_bed2.3mf` (stands) |
| Hex cells | `--width 160 --layout hex --tiles none --orientation standing` | `cli/hex/` | `app/hex/` | 53 × 46 pointy hex, 159.0 × 239.0 mm, 17 116 tris, ledge 0.26 mm |
| Point light (−400, 120, 80) mm | `--width 160 --light-point=-400,120,80 …` | `cli/point/` | `app/point/` | 162 × 240 mm, depth 4.3 mm, ledge 0.28 mm, match r 0.9999 (lamp ≈ 11° up, so almost no shadowing) |
| Optimised | `--width 160 --optimise shadow …` | `cli/optimised/` | `app/optimised/` | 8 sweeps, match r 0.974 → 0.992, rel. RMSE 0.030 → 0.017. **Ledge 0.70 mm, warns `downward_ledge`** (finding 2) |
| Calibration tile, standalone | `ad3d calibration --out … --strict` | `cli/calibration/` (`calibration.{stl,3mf,key.json}`) | — | 120 × 84 mm, 12 steps −5.7°…10°, light side Left, 1 quarter turn, inside the bed at (8, 8) |
| Calibration tile with the plate | `--width 160 --tiles none --orientation standing --calibration-tile` | `cli/cr6-cal/` | `app/calibration/` | Plate + calibration tile + key on one bed. **The tile takes the plate's 2 quarter turns: 4.1 × 120 × 84, ledge 0.83 mm, warns `downward_ledge`** (finding 1) |
| Extra: hex, tiled, optimised, calibration tile, project | `--width 300 --height 378 --layout hex --optimise shadow --tiles auto --keys pegs --stand --calibration-tile --save-project …/job.ad3d` | `cli/hex-tiled-opt-cal/` | — | Plate 300 × 374.1 mm (hex height snaps down). 2 × 2 tiles (Y 187.1 × Z 150), splints (22 shells), 2 stands, calibration tile (1 quarter turn here, ledge 0.60 mm), two beds / two 3MFs, `job.ad3d`. r 0.969 → 0.982 |

The CLI and the app give identical meshes for the same settings: the same triangle counts, sizes and ledges.

**App timings:**
- The photo opens on the first call.
- Preview: 1.0–2.6 ms with the photo cached. The first, cold call is 45.7 ms, with the target taking 42.2 ms.
- Final quality: 15–41 ms, or 74 ms for the 300 × 450 plate. `validated: true` every time.
- Exports: 107–465 ms.

## Validation

56 files: 37 STLs and 19 3MFs (one 3MF per bed).

| Check | Result |
|---|---|
| `ad3d validate --strict` (every STL and 3MF) | **56/56 OK**, 0 self-intersecting pairs in every file (`validate-summary.tsv`, `validate-ad3d.log`). Splint parts are checked as N genus-0 shells (Euler = 2·N) |
| `tools/check_mesh.py` (trimesh, every STL) | **37/37 OK**, exit 0 (`check_mesh.log`) |
| Independent trimesh check of every 3MF object, with the build-item transform applied: watertight, consistent winding, positive volume, Euler 2 (2·N for splints), inside 235 × 235 × 250 with min z = 0, no XY overlap between parts on a bed | **37 objects: all watertight and wound.** 35 are inside their bed with no overlaps. The 2 outside are the LuBan-default 300 × 450 plates (expected, `part_exceeds_bed`) (`check_3mf_trimesh.log`; script at the end) |
| App harness `e2e_m1_app_exports` | Passes. Every final preview returns `validated: true`. Exported STLs are re-validated with the self-intersection check. Tile plan = export cut for every case, e.g. `[tiled-300x378] tile plan cols [25, 25] rows [27, 36]`, the same as the export (`app-e2e.log`) |
| Test suites | `cargo test --workspace --no-fail-fast`: 313 passed, 2 failed. `pnpm test`: 190 passed, 4 failed. **Every failure is in a new, untracked round-2 reviewer file** (`src-tauri/src/m1_review2_tests.rs`, `crates/ad3d-core/tests/printability_r2.rs`, `src/app/m1Review2.test.ts`). Every committed test passes |

My mesh checks cannot detect one case: a splint piece nested entirely inside another. `printability_r2.rs::splints_do_not_overlap_at_seam_crossings`, from a parallel reviewer, covers it.

## Screenshot: live preview in the real app

**How it was run:**
- `pnpm tauri dev` (debug build) with `VITE_AD3D_E2E_PHOTO=…/tests/fixtures/mona-lisa.jpg`, plus the one-line hook in `src/main.tsx` from round 1. I removed the hook after the run.
- The harness `src/dev/e2eM1.ts` now opens the photo through the app's real entry point, **`useImage(path)`** (the one File → Open, Import, drop and the Image panel use). It then sets width 160 and saves the canvas through **Save preview image** (`save_preview_image` → `~/Pictures/AD3D`).

**What the live app reported** (`tauri-dev.log`):
- `ui.image after useImage: 1006x1500; params.quality final`. So round-1 finding 6 (photo entry point) and finding 8 (quality at launch) are fixed on this path.
- The first plate arrived 1011 ms after the patch, already final (flags 3). 7812 tris.
- Debug build: generated in 191 ms (optimise 47 ms, mesh 4 ms). `validated: true`, r 0.974 → 0.992, 8 sweeps.
- The optimiser was on because `DEFAULT_PARAMS` turns it on (`src/state/store.ts:19`).

**`docs/m1/preview.png`:** the photo loaded, with the preview light moved off the design light to a free light at azimuth 160°, elevation 9°. The design light is 180° / 5°.

![preview, light moved](preview.png)

Other captures:
- `preview-design-light.png`: perspective view, design light.
- `preview-front-moved.png`: front view, moved light.
- `preview-front-design.png`: front view, design light.

The other images in `docs/m1/` are from round 1.

**What I see:**
- At the design light (front view), the plate clearly reads as the Mona Lisa: bright sky, dark hair framing a bright face, bright chest, hands bottom-left.
- With the light moved, the image washes out as expected. The lamp marker and its ray are drawn.
- The face and chest still show dark vertical cast-shadow slits. Round 1 showed they are physical, and that the core's forward model counts them (`src/viewport/shadowAgreement.test.ts`), so I am not reporting them again.

## Benchmarks

Criterion, `cargo bench -p ad3d-core --bench squama --bench squama_layouts --bench sim -- --warm-up-time 1 --measurement-time 3`, median (`bench.log`).

| Bench | 50×63 | 100×126 | 150×189 |
|---|---|---|---|
| end-to-end generate | 1.34 ms | 6.02 ms | 10.12 ms |
| end-to-end generate, noise target | 2.00 ms | 7.39 ms | 14.74 ms |
| quantise+mesh | 1.18 ms | 4.47 ms | 9.18 ms |
| mesh_relief | 0.89 ms | 3.84 ms | 8.45 ms |
| back-face CDT | 10.3 µs | 24.3 µs | 30.1 µs |
| payload encode | 29.3 µs | 81.0 µs | 356.9 µs |
| sim: shade left / oblique / point | 1.08 / 4.11 / 6.33 ms | 5.55 / 18.36 / 25.14 ms | — |
| optimiser: 1 sweep, left light | 5.74 ms | 17.93 ms | — |
| sim: render 2 px/mm ss2 | 26.70 ms | 39.94 ms | — |

| Bench (layouts) | square (12 600 facets) | hex-pointy (14 673) | hex-flat (14 616) |
|---|---|---|---|
| solve, Lambert, directional | 0.325 ms | 0.540 ms | 0.528 ms |
| solve, Lambert, point | 0.935 ms | 1.36 ms | 1.23 ms |
| solve, measured response, directional | 14.07 ms | 14.89 ms | 17.42 ms |
| solve, measured response, point | 14.32 ms | 20.73 ms | 20.53 ms |
| generate_plate, directional | 5.28 ms | 9.63 ms | 8.23 ms |
| generate_plate, point | 5.27 ms | 9.45 ms | 9.07 ms |

Sampling `cell_brightness`, 12 MP: 22.9 ms (→ 100×75), 23.4 ms (→ 150×113).

Every drag-path stage fits the ≤ 50 ms budget at the §13 size (about 100 × 126 cells), including a measured response and a point light. The optimiser (8 sweeps ≈ 145 ms at 100 × 126) runs at final quality only.

## Round-1 findings: status

| Round-1 finding | Status in round 2 |
|---|---|
| Preview slits vs the core simulation | Closed in round 1 (both models agree, fixture test). Not re-reported |
| Tile plan shown ≠ cut | **Fixed.** `squama_tile_plan` equals the export cut in every app case |
| 0.70 mm downward ledge on the optimised plate | **Not fixed, and wider than reported.** See finding 2 |
| Validator rejects our splints | **Fixed.** Both validators accept the 21- and 22-shell splints |
| CLI raw floats, hex overshoot, timings label | **Fixed.** Sizes print to 0.1 mm, hex 378 → 374.1, the label reads "whole-plate stage timings" |
| Photo entry points skip imageInfo | **Fixed** on the `useImage` path: the live app shows `ui.image` = 1006 × 1500 |
| Undo after a slider drag | Not exercised end to end here. The committed tests pass |
| Plate at launch generated at Preview | **Fixed.** `params.quality` is `final` after `useImage`, and the first live plate had flags 3 (final) |
| Point light + measured response read differently | **Consistent now.** With the optimiser, the "before" score drops as the lamp goes lower, identically for point and directional lamps at the same elevation (point at 11°: r 1.000; directional at 11°: 0.998; point at 5°: 0.989; directional at 5°: 0.974). With a measured response, the point plate gives 1.000 and the directional plate 0.985 → 0.993. No sign of solver/forward-model mismatch |

## New and unfixed problems (ranked)

1. **Minor, new. The calibration tile exported with a one-piece plate inherits the plate's quarter turn and gets a 0.83 mm downward ledge.**
   - **What happens:** `export_plate` (`crates/ad3d-core/src/squama/export.rs`, around lines 761–776) measures and places the calibration tile with `plan.quarter_turns`, the plate's turn, and forbids rotation (`may_rotate: plan.quarter_turns.is_none()`).
   - **Results by case:**
     - CR-6 one-piece plate (2 turns): the tile stands 120 wide × 84 tall with a **0.83 mm** ledge, and the export warns `downward_ledge` on both paths (`cli/cr6-cal/`, `app/calibration/`).
     - Tiled plate (1 turn): the tile stands 84 × 120 with 0.60 mm, within its design.
   - **Contradicts:** round 1's "ledges ≤ 0.60 mm" guarantee in `calibration.rs`, which only holds in that one turn.
   - **Fix:** give the calibration part its own best standing turn (the one with the smallest ledge that keeps the steps facing the light). Or keep the plate's turn but choose the ramp axis to match. Add a test that the ledge stays ≤ 0.6 mm for one-piece and tiled exports.
2. **Minor, unfixed and broader than reported. The optimiser raises downward ledges past the 0.6 mm sag limit.**
   - **Scale:** it is not just the 0.70 mm on the 160 mm plate. On the same CR-6 one-piece plate (2 quarter turns):

     | Light | Ledge with optimiser |
     |---|---|
     | Directional, 11° | 0.73 mm |
     | Point (−900, 120, 79) | 0.75 mm |
     | Point (−400, 120, 80) + measured response | 0.84 mm |
     | Point (−400, 120, 35) | **0.92 mm**, against 0.19 mm with the optimiser off |

   - **Why it is not seen on tiled plates:** they print with 1 quarter turn and stay ≤ 0.52 mm with the optimiser.
   - **Why it matters more now:** the optimiser is **on by default** in the app (`DEFAULT_PARAMS`), so the default CR-6 export carries this warning.
   - **Cause:** the optimiser does not know the print's down direction.
   - **Fix:** pass the planned standing turn into the optimiser and clamp each move so the downward ledge stays ≤ 0.6 mm in that direction. Alternatively, run a final ledge-clamp pass after optimising. Test across directional and point lamps.
3. **Note: the app harness and the live capture now use the real photo entry point.** `src/dev/e2eM1.ts` calls `useImage` instead of setting `ui.image` by hand, so round-1 finding 6 is covered by the live run.

## Hand-over checklist for printing (CR-6 SE)

- Brim 6–8 mm, outer only, brim-object gap 0.1 mm on every standing part. 3–4 perimeters or 100 % infill. Layers 0.12–0.16 mm.
- Elephant-foot compensation 0.2 mm (Cura: Initial Layer Horizontal Expansion −0.2).
- One 3MF per bed: print `…_bed2.3mf` as a second job.
- Keys: holes Ø3.5 for Ø3.0 pegs. Stand slot = plate thickness + 0.5 mm.
- Watch the 0.70 mm steps on the optimised 160 mm plate. If they sag, re-export with the optimiser off.
- Until finding 1 is fixed, print the calibration tile from `ad3d calibration` (`cli/calibration/`), not alongside a one-piece plate.

## Uncommitted files from this round

- `src/dev/e2eM1.ts`: now uses `useImage`. The `main.tsx` hook was added for the run and removed again.
- `docs/m1/`: this report, the four new screenshots (`preview.png`, `preview-design-light.png`, `preview-front-moved.png`, `preview-front-design.png`), the logs, and the regenerated `out/`. `out/cli/cr6-cal/` is new.
- The 3MF checker used here: `/private/tmp/claude-501/-Users-apple-Desktop-workspace-nextjs-AD3D/56351967-942a-4feb-8aa2-59554c373fd2/scratchpad/check3mf.py`. It is not in the repo; copy it to `tools/` if you want to keep it.
