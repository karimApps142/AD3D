# Architecture spike report (PLAN §2)

Status 2026-10-20, after audit round 1. Every number below comes from a file in this folder or in `docs/wip/spike/`, named next to it.

**Machine.** Intel i9-9980HK MacBook Pro (8 cores / 16 threads, 32 GB), macOS 26.5. In every run that checked, the built-in 3072×1920 panel was driven by the **Intel UHD 630** (`c6-launch.txt`, `probe-runs/run*/display-gpu-during-run.txt`). The Radeon Pro 5500M was never seen driving the display. WebKit masks the GPU as "Apple GPU" and reports `hardwareConcurrency` 8.

**Window.** The window is 1440×811 pt. The canvas is 2280×1566 px at DPR 2 (`criteria.C1.canvas` in every report). That is not a maximised window on the 3072×1920 panel. With `MAXIMIZE=1` the canvas only grew to 2472×1566 (`postfix-r1/run3`), because the side panels take the rest.

**Load matters.** This Mac normally runs Docker Desktop and a Virtualization.framework VM, plus parallel agent builds. C2, C4 and C5 slow down clearly under that contention, and C4/C5 fail under heavy contention. Every result below carries its 1-minute load average. "Quiet machine" means a 1-minute load under about 5 with Docker and the VM idle. A load figure alone does not predict C4/C5:
- probe run 1 at load 14 gave 117 ms (C4) and 59 updates/s (C5);
- audit-run at load 13, with the VM at 262% CPU, gave 241 ms and 14/s.

Docker and VM contention on WindowServer and the GPU is what hurts.

## Summary

| | Target | Result | Verdict |
|---|---|---|---|
| C1 | Tauri 2 app, react-three-fiber viewport, movable directional light casting shadows | App launches. Shadows darken 4.94% of pixels; a 90° light swing changes 30.3%. Every run, including after the fixes, gives these numbers. The shadows are facets shading each other; no shadow is visible on the floor. | **Pass** (self-shadowing only) |
| C2 | 100×126 plate in < 50 ms (release) | 9.17 ms median (criterion, load 2–17). An auditor run at load 86–107 gave a 50.6 ms mean. | **Pass on a lightly loaded machine**; at the limit under extreme load |
| C3 | `validate()` passes: watertight, consistent winding, no pinched vertices after welding | After the fixes: 29 of 29 meshed sweep cases pass `ad3d validate --strict` (self-intersection stage on) and trimesh, plus 1 expected refusal by the relief guard. Manifold import test 3/3. All property suites pass. | **Pass**. Until this round, "watertight" meant "closed 2-manifold" only. |
| C4 | 1M triangles, Rust → viewport, in < 150 ms | Quiet machine (load ≈ 4–5): 84 ms median, but measured on reused warm buffers; the cold warm-up took 107–158 ms. After the fixes the harness times cold and warm runs separately: 121 ms cold / 124 ms warm in one run at load 20–28 (pass), 166–388 ms in two more loaded runs (fail). | **Pass on a quiet machine only.** The quiet cold-path re-run is still owed. |
| C5 | ≥ 30 updates/s while dragging a slider on a 50×63 plate | Quiet machine: 60/s, the rAF ceiling. After the fixes, at load 20–28: 60/s with input paced by rAF and 59/s with input every 4 ms from a timer. Heavier contention: 12–38/s. | **Pass on a quiet machine only** |
| C6 | `tauri build --target universal-apple-darwin` gives a runnable .app | Rebuilt after the fixes (28 min at load 24–45). Runs on x86_64. The arm64 half is statically valid but has never been run. | **Pass for x86_64; arm64 verified statically only** |

## C1: viewport, movable light, shadows

**Method.** `measureC1` in `src/spike/runSpike.ts` runs inside the app:
- It reads back the WebGL canvas for the same frame twice, once with shadow intensity 1 and once with 0, and counts the pixels that get darker.
- It swings the preview light 90° in azimuth through `setView`, and counts the pixels that change. The View panel's "Light azimuth" slider and Alt+drag (`useLightDrag`) use the same store path.
- It saves `viewport.png`.

**Numbers.** 176,466 pixels darkened (fraction 0.0494); the light move changed fraction 0.3031. These are identical in all 13 pre-fix runs (5 clean, 6 probe, 2 audit), in the auditor's re-run and in postfix-r1 runs 1–2. The maximised postfix-r1 run 3 gave 0.0456 and 0.2796 on its larger canvas. The shadow map is PCF-soft, with the frustum fitted to the plate (`criteria.C1.shadows`).

**Caveats.**
- **The darkened pixels are plate facets shading each other.** The floor (#181b21) renders at about 5/255, so a floor shadow cannot be seen. At the 5° default light a floor shadow would be metres long anyway. For a Squama plate, self-shadowing is the effect that matters.
- **The light move went through the store, not through the slider or a drag.** The UI controls use the same path.
- **No screenshot was taken from outside the app.** `screencapture -l` needs Screen Recording permission for the terminal, and the terminal does not have it. The visual evidence is the in-app WebGL readback, `viewport.png`.

## C2: Rust mesher speed

**Method.** `cargo bench -p ad3d-core --bench squama`:
- criterion, using the bench profile (thin LTO, `codegen-units = 1`);
- `black_box` around the call;
- the same `generate_ex` that the app calls.

The raw output is in `bench-squama.txt`. `c2-criterion.json` holds the **median** and the **mean** for each bench. The headline below uses the median; criterion's printed `time:` line is the mean.

**Numbers** (load 2–17, single-threaded):

| Bench | Median | Mean |
|---|---|---|
| end-to-end generate 100×126 (synthetic image) | **9.17 ms** | 9.27 ms |
| end-to-end generate 100×126 (noise image, the worst case) | 13.51 ms | 13.94 ms |
| quantise + mesh 100×126 | 8.71 ms | 8.76 ms |
| mesh_relief 100×126 | 7.13 ms | 7.16 ms |
| payload encode 100×126 | 0.15 ms | 0.15 ms |
| end-to-end generate 50×63 | 1.97 ms | 2.01 ms |
| end-to-end generate 150×189 | 22.69 ms | 23.74 ms |

For comparison, the Python direct mesher took 6.57 s on this plate and the manifold union 1.41 s.

**Under load.** The auditor ran `cargo bench ... 100x126` at a 1-minute load of 86–107 and got these **means**:

| Bench | Mean at load 86–107 |
|---|---|
| end-to-end generate 100×126 | 50.6 ms [44.4–57.5] |
| noise image | 69.8 ms |
| quantise + mesh | 43.5 ms |
| mesh_relief | 36.8 ms |
| payload encode | 0.92 ms |

`ad3d-core` does not use rayon yet, so the mesher runs on one thread and slows down about 5× under heavy contention.

That run overwrote `target/criterion/*100x126*/new/estimates.json`. **Do not regenerate `c2-criterion.json` from `target/criterion`** until the full bench has been re-run on a quiet machine. Alternatively, run it with `--save-baseline spike` and read that baseline.

**This round's change.** The audit fixes touch the mesher: the crossing margin, the partition check and the back-face strip. I compared the old and new `mesher_timing` binaries interleaved, taking the minimum of 300 runs each, at load 7–21:

| Binary | generate 100×126 (min of 300) |
|---|---|
| old | 14.1–15.8 ms |
| new | 14.9–17.2 ms |

The difference is inside the run-to-run noise at that load. The full partition check cost 0.4 ms at 100×126, so square grids get an O(1) check instead (see A2). **A clean criterion re-run on a quiet machine is still owed** before new numbers replace the table above.

## C3: validate()

**Method.** `validate()` (`crates/ad3d-core/src/mesh/validate.rs`) runs three stages:
1. the raw indices;
2. the written f32 positions, welded bit-exactly;
3. a CuraEngine-style 30 µm integer meld.

Each stage checks for degenerate triangles, that every edge is used exactly twice in opposite directions, that no vertex fan is pinched, the Euler characteristic and the component count.

Cross-checks:
- the C3 sweep (`c3_sweep.py`): 30 Squama plates through the release CLI, then `ad3d validate` and trimesh (`tools/check_mesh.py`). The plates cover the defaults, edge cases (S = 20°, α = 0, off-axis azimuths, 1 mm and 20 mm cells) and 24 random cases;
- the manifold MeshGL import (`c3-manifold-oracle.txt`);
- the property suites in `cargo test --workspace` (`c3-cargo-test.txt`).

**What "watertight" meant before this round (audit L1).** `validate()`, trimesh's `is_volume` and manifold's import all check topology only. So "watertight" meant a **closed, consistently wound, unpinched 2-manifold**, not a solid proven free of self-intersections.

**What this round added:**
- An opt-in **triangle-triangle self-intersection stage** (`mesh::intersect`, `ValidateOpts::check_self_intersection`). It is on in every property test and in `ad3d validate --strict` and `ad3d squama --strict`.
- Rejection of non-finite or out-of-range coordinates.
- A per-shell volume sign check.
- Rejection of unreferenced vertices.
- A sliver gate: minimum triangle altitude δ/10 = 5 µm.

**Results.**
- **Before the fixes:** 29 of 30 sweep cases passed. The one "failure", rand06, was the relief guard correctly refusing a 0.027 mm relief (design §2), not a mesh defect.
- **After the fixes:** `c3-results.json` was regenerated. The sweep now counts such refusals separately and validates with `--strict`. 29 of 29 meshed cases pass `validate --strict` and trimesh, with 0 self-intersecting pairs; rand06 is again an expected refusal. The smallest triangle altitude across the sweep is 48 µm.
- The manifold oracle passes 3/3, and `cargo test --workspace` passes.
- The auditor's independent oracle (now the core stage) also found 0 intersecting pairs, across about 2,900 Squama plates and about 600 generic-mesher meshes.

## C4: 1M triangles, Rust → viewport

**Method.** `measureC4` in `runSpike.ts` times one path from request to rendered frame:
1. `engine.syntheticMesh(1_000_000)` invokes the Tauri command. The command encodes a torus that was generated and cached at startup (V = T/2, 18,000,064 bytes). **Generation is excluded; encoding (about 3 ms, 6–10 ms under load) is included.**
2. The JS side parses the payload zero-copy. `payloadZeroCopy` refers to payload parsing, not GPU buffers.
3. It attaches the mesh through `showMesh` → `applyPayload`. It calls `showMesh` directly, skipping the preview's latest-wins loop.
4. It waits for the next frame plus a forced GPU sync: `gl.render` followed by a 1-pixel `readPixels`. **Present-to-screen is not measured.**

**What the pre-fix headline measured.** `ZERO_COPY_INDEX_THRESHOLD` was compared with `>`, and a 1M-triangle mesh has exactly 3,000,000 indices. So:
- the warm-up took the "new" path with 1.25× headroom buffers;
- all 5 timed runs took the **"copy" path**: a JS memcpy plus `bufferSubData` into GPU buffers that already existed. No GPU buffer allocation was timed.

The realistic case, the first big mesh arriving, is the warm-up column below.

| Runs (files) | 1-min load | Warm-up (cold) | Timed runs, warm "copy" (median) | IPC / geometry / frame (medians) |
|---|---|---|---|---|
| clean run1–5 (`run1..5/report.json`) | 3.9–5.6 | 110, 107, 110, 131, 158 ms | 81, 82, 84, 85, 97 → **84 ms** | 34–43 / 2–3 / 44–52 ms |
| probe run1–6 (`probe-runs/`, PROBE=1) | 14.0 → 5.9 | 166, 180, 250, 148, 152, 1255* ms | 117, 124, **151**, 92, 99, 92 ms | |
| audit-run (`docs/wip/spike/audit-run`) | 13.2, VM at 262% | 384 ms | **241 ms** (272/218/241/217/304) | 103 / 5 / 133 ms |
| audit-run2 (`docs/wip/spike/audit-run2`) | 27–32, Docker at 450% | 291 ms | **224 ms** | 91 / 9 / 126 ms |
| auditor's run (not in the repo) | ≈ 157 | | **227 ms** (198–297) | IPC 86–101 ms, GPU sync 65–78 ms |

\* Two probe numbers are GPU stalls caused by `system_profiler` running alongside (PROBE=1): run 6's 1.25 s warm-up and run 5's 1238 ms first timed run. Primary runs therefore leave the probe off.

The clean runs were chosen after the probe runs were set aside. They are valid for a quiet machine, and only for one.

**Harness changes this round.**
- The threshold test is now `>=`, so a 1M-triangle mesh attaches zero-copy.
- C4 reports two sets of runs, and **both must pass**:
  - **cold:** a small plate is shown before each run, so every 1M-triangle mesh needs new GPU buffers ("new" mode);
  - **warm:** runs go back to back and reuse the same-size buffers in place ("swap" mode).
- The warm-up row is kept.
- `C4_medianTotalMs` in the summary is now the cold median.
- The 18 MB shared-vertex torus is the smallest possible payload for 1M triangles. An imported triangle soup would be about 2–3× larger.

## C5: slider drag on a 50×63 plate

**Method.** `measureC5` drives the real `maxTiltDeg` range input with the native value setter plus an `input` event. The path under test:
1. React `onChange`
2. `store.setParams`
3. `LatestWinsLoop`
4. Tauri `generate_squama`
5. `PlateMesh` "swap"

An update is a rendered frame with `meshChanged` on a verified 50×63 plate (20,836 triangles). Updates are counted in 1-second windows over 5 s.

**The preview stays at full resolution while dragging.** The CLAUDE.md behaviour (low-res while dragging, full-res on release) is not implemented yet, so it is neither used nor measured.

| Runs | 1-min load | Median updates/s (per-second windows) | Frame interval p50 | Rust gen p50 | Render CPU p50 |
|---|---|---|---|---|---|
| clean run1–5 | 3.9–5.6 | **60** (58, 60, 60, 60, 60) | 17 ms | 1.6–2.0 ms | 1 ms |
| probe run1–6 | 14 → 6 | 59, 55, **38**, 60, 60, 60 | 17–26 ms | 1.8–3.6 ms | 1 ms |
| audit-run | 13.2, VM at 262% | **14** (18, 20, 14, 12, 14) | 61 ms | 6.4 ms | 2 ms |
| audit-run2 | 27–32, Docker at 450% | **19** (20, 20, 19, 18, 18) | 50 ms | 5.2 ms | 2 ms |
| auditor's run | ≈ 157 | **26** (29, 26, 27, 25, 24) | 35 ms | | |

Under contention, Rust generation (5–6 ms) and render CPU (2 ms) stay small. What limits the rate is the rAF/compositor cadence, which falls to 16–29 Hz. Rendered updates are really lost, but the loss does not come from our code path.

**Limits of the pre-fix method.** One value was set per `requestAnimationFrame`, so updates/s could never exceed the rAF rate: 60/s is the driver's ceiling as well as vsync's. Input never arrived faster than frames, so latest-wins coalescing was never exercised (`coalescedFraction` was 0).

**Harness changes this round.**
- C5 runs twice, and both runs must reach 30/s:
  - **raf:** one input per frame, as before;
  - **timer:** one input every 4 ms from `setInterval`, independent of rAF (about 250 Hz, like a fast trackpad). Inputs land between frames, so the loop has to coalesce.
- Before C5 the harness records `document.hasFocus()`, `visibilityState` and the measured rAF rate. It labels the run `throttled` when the compositor is not delivering about 60 Hz.

## C6: universal build

**Method and evidence.**
- Command: `pnpm tauri build --target universal-apple-darwin --bundles app`, now also available as `pnpm build:mac`.
- Logs: `c6-build.txt` (11 min at load ≈ 22), `c6-bundle.txt`, `c6-launch.txt`.
- After this round's fixes the build was redone with `pnpm build:mac`: 1702 s wall time at load 24–45, exit 0.

**Results.**
- The binary is a fat Mach-O with x86_64 and arm64 halves (the arm64 half is 8.7 MB), minimum macOS 11.0, SDK 15.5.
- It links only `/System` frameworks and `/usr/lib`.
- It carries an ad-hoc signature that verifies (`TeamIdentifier` not set, no notarisation).
- The rebuilt binary no longer contains the build-machine `docs/spike` path.
- **x86_64:** launched with `open`, showed one on-screen window owned by AD3D (1440×811 pt), and quit cleanly. Every spike run used this bundle.
- **arm64: never run.** This Intel Mac cannot execute it, so "runnable" is proven for x86_64 only. Still owed:
  1. copy the .app to the Apple Silicon Mac without the quarantine flag;
  2. run `open -n AD3D.app`;
  3. do one spike run there;
  4. run it once with `arch -x86_64` to check the x86_64 half under Rosetta.

  See `docs/START_HERE.md` → "Building and running the app".
- **Fresh machines:** `rust-toolchain.toml` pins Rust 1.99.1 with both targets, and `package.json` pins pnpm 10.33.2. CMake is needed only for the optional `ad3d-manifold` crate.

## Post-fix runs (audit round 1)

These are the rebuilt universal app with all of this round's fixes, run through `run_spike.sh` with FORCE=1. They are flagged `"quiet": false` in `report.json` → `host`, because the load never fell below 18 during this round. Docker, the VM and other agents' builds were running (see `postfix-r1/run*/top-cpu-*.txt`). Because of that, **they are evidence that the new harness, the CSP and the spike-mode gating work, not quiet-machine results.**

| Run (`docs/spike/postfix-r1/`) | Load before → after | C4 warm-up | C4 cold (median) | C4 warm (median) | C5 raf | C5 timer (coalesced) | Verdict |
|---|---|---|---|---|---|---|---|
| run1 | 19.7 → 27.9 | 142 ms | 111/121/117/135/127 → **121 ms** | 138/124/129/122/103 → **124 ms** | **60** (56, 60, 60, 60, 60) | **59** (59, 60, 59, 48, 55), 9% coalesced | C1/C4/C5 pass |
| run2 | 27.9 → 26.2 | 271 ms | **166 ms** | **198 ms** | 34 | **22**, 53% coalesced | C4, C5 fail |
| run3 (MAXIMIZE=1, canvas 2472×1566) | 29.0 → 35.1 | 397 ms | **388 ms** | **364 ms** | 18 | 12, 64% coalesced | C4, C5 fail |

**Observations.**
- **Every timed C4 run took the intended path.** All cold runs were "new" and all warm runs were "swap", so the cold path is now measured. Under the same load, cold is no slower than warm. The cost is IPC plus GPU sync, not buffer allocation.
- **The timer driver exercises coalescing.** In run1, 565 inputs gave 560 parameter changes and 508 requests (9% coalesced, 0 dropped). In run2, 426 changes gave 202 requests (53%). The preview stayed at the display rate when it had headroom.
- **The `throttled` label did not fire in any run.** rAF measured 58.8 Hz before C5 each time, while contention showed up only once the drag started rendering. The label catches an unfocused or hidden window, not contention. `host.quiet` is the flag to trust.
- **C1 was unchanged in all three runs**, so IPC, rendering and `save_artifact` all work under the new CSP.

## Audit notes (round 1)

Each finding, what was done, and where I disagree or could not finish.

**Mesher and validate()**
- **A1 (crossing clamped to exactly δ): fixed.**
  - The clamp now keeps a 1.05δ margin (`CROSSING_MARGIN`; 2.5 µm, about 20 f32 ulps at 2048 mm).
  - `clamped_crossing_exactly_delta_far_from_origin` is un-ignored. It passes all 2,400 far-origin cases and both minimal reproducers.
  - `only_known_a1` is removed, so the random suite must pass outright.
- **A2 (clamp panic on edges shorter than 2δ): fixed.**
  - The crossing code returns `InvalidParams` for an edge shorter than 2.2δ.
  - `mesh_relief` also checks the partition up front in release builds:
    - general partitions get the full `Partition::check` (convex CCW pieces, edges ≥ 4δ). It now returns `InvalidParams`, has no modulo in its loop, and was sped up from 2.5 ms to 0.4 ms at 100×126;
    - square grids get an O(1) shape and cell-size check. The full pass would still cost 4% of the C2 budget on a lattice that the mesher already trusts.
  - `tiny_cell_with_crossing_does_not_panic` is un-ignored and asserts `InvalidParams`.
- **A3 (back-area check too strict off the origin): fixed.**
  - The tolerance is now 1e-6·area + 2·ulp_f32(max |coord|)·(perimeter of the back region).
  - The f64 outline area now passes for a 3.3 mm cell at (1000.123, 7.77).
  - An area 0.1% off still fails.
- **A4 (i64 overflow on Inf): fixed.**
  - `validate()` first rejects NaN, ±Inf and |x| > 1e9 mm, and returns early.
  - `cura_meld` clamps and uses saturating adds.
  - The NaN/Inf test is un-ignored and covers both soup and indexed meshes.
- **A5 (back-face needles): fixed.**
  - The convex strip now advances by normalised arc length.
  - The minimum back altitude is about 0.447·cell (cell/√5, at the corner triangle). On a 1770² outline at 1.13 mm it went from 0.64 µm to 505 µm.
  - The generated 2000 mm / 1.13 mm plate (24.8 M triangles) now has a 48 µm minimum altitude overall, against the previous 0.64 µm.
- **L1 (no self-intersection check): fixed as specified.** See C3.
  - The auditor's broad phase put each triangle into every cell of its bounding box. That took 60 s on a 78k-triangle plate, because the long diagonal back triangles have huge, overlapping boxes.
  - Now each triangle goes only into the cells its surface touches (a padded separating-axis test). The same plate takes 1.3 s.
  - A unit test compares the result with an all-pairs brute force on random triangle soups.
  - C3 is reworded: without `--strict`, OK means "closed 2-manifold".
- **L2 (slivers not gated): fixed, with one disagreement on scope.**
  - `ValidateOpts::min_altitude` defaults to δ/10 = 5 µm, the auditor's second step now that A5 is fixed. `ad3d validate --min-altitude` overrides it.
  - Squama plates measure ≥ 48 µm in the sweep.
  - **New finding A6 (not fixed): tall, narrow walls in the generic mesher make unavoidable slivers.**
    - Where it shows up: `random_heights_wide_range_with_random_apex`, with walls up to 1 m tall on 0.5 mm cells.
    - Why: the §7.4 ladder adds no Steiner points. It must join a 51 µm step on one side of a wall to a vertex up to a metre away on the other side. Altitude ≈ step × width / wall height, and 1.6 µm was found.
    - That test now turns the sliver gate off and says why.
    - It matters for future generators with very tall, narrow walls. The fix is Steiner points on tall walls.
- **L3 (no per-shell volume check): fixed.**
  - Every connected shell must have positive signed volume (`negative_shells`).
  - Limitation: a deliberate cavity (an inner shell wound inward) now fails too. M0 has none; revisit for hollow parts.
- **L4 (unreferenced vertices): fixed.** A non-soup mesh fails when the raw stage has vertices that no triangle uses.

**Measurements and harness**
- **REPORT.md missing: fixed** by this file.
- **C4 and C5 fail under load: the report is fixed; the quiet re-runs were not done.**
  - The quiet-machine condition and the audit numbers are stated above.
  - `run_spike.sh` now logs the load and the top CPU processes before and after each run, and merges them into `report.json` as `host`.
  - It **refuses to start a run while the 1-minute load is above MAX_LOAD = 8**, unless FORCE=1 is set.
  - I did **not** stop Docker Desktop or the VM for 5 quiet re-runs: they are the owner's processes, other agents were building at the same time, and the load stayed between 18 and 45 for this whole round.
  - **Still owed:** the 5 quiet re-runs, plus one with MAXIMIZE=1.
- **C2 holds only at low load: fixed in the report.**
  - The load conditions are stated, and medians and means are labelled.
  - `c2-criterion.json` was **not** regenerated, so the auditor's loaded estimates sitting in `target/criterion` cannot leak in.
- **C4 measured only the warm path: fixed in the harness.**
  - Cold and warm runs are timed separately, and both must pass.
  - The threshold test is now `>=`, and `zeroCopy` is renamed `payloadZeroCopy`.
  - The report states that generation is excluded, that the topology is shared-vertex, and that present-to-screen is not measured.
- **C5 capped at the rAF rate: fixed in the harness.**
  - A timer-driven mode was added, plus focus and rAF labelling.
  - A manual drag with the stats overlay was not done; it needs a person at the machine.
  - There is no low-res drag mode yet, and the report says so.
- **Display GPU and window size: stated above.**
  - `AD3D_SPIKE_MAXIMIZE=1` (`MAXIMIZE=1` in `run_spike.sh`) maximises the window.
  - I could not make the dGPU drive the display, and it was never seen active.
- **C1 light move driven programmatically: kept as a caveat.** The harness uses the store path that the UI uses; driving the range input instead would add little.
- **C1 has no floor shadow: kept as a caveat, not changed.** Self-shadowing is the Squama effect.
- **arm64 half never run:** recorded as "C6 passed for x86_64, arm64 verified statically". The steps for the Apple Silicon Mac are in START_HERE.
- **Ad-hoc signature and quarantine: documented in START_HERE.**
- **Toolchain not pinned: fixed.**
  - `rust-toolchain.toml` pins Rust 1.99.1 with both targets. rustup has installed that toolchain on this machine.
  - `package.json` has `packageManager` and a `build:mac` script.
  - START_HERE notes that CMake is needed only for the `ad3d-manifold` crate.
- **Spike-only commands available in normal mode: fixed.**
  - `finish_spike` and `save_artifact` return `Err` unless `AD3D_SPIKE=1`.
  - Output goes to `AD3D_SPIKE_OUT`, or `<app data dir>/spike` when it is unset. No build-machine path is compiled in any more.
  - `tauri.conf.json` now sets a strict CSP: `default-src 'self' ipc: http://ipc.localhost; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; worker-src 'self' blob:; connect-src 'self' ipc: http://ipc.localhost`. The post-fix runs work under it.
- **No external screenshot: not changed.** It needs Screen Recording permission for the terminal, which is the owner's decision. `run_spike.sh` only ever kills the PID it started.
- **Generated files not ignored by git: fixed.**
  - `.gitignore` now covers `**/core-out/`, `**/out/**/*.stl`, and the PNGs in the run, post-fix and audit folders.
  - The small run evidence (`report.json`, logs, top-CPU snapshots) stays tracked, so this report's sources are in the repo. So does the reference image `docs/spike/viewport.png`.

## Re-measurement by the orchestrator (2026-10-20, after the audit fixes)

No agents were running; the owner's VM (≈1.4 cores) and Docker Desktop were. Raw files: `docs/spike/quiet/`.

| Run | Background | C4 cold median (target < 150 ms) | C5 median (target ≥ 30/s) |
|---|---|---|---|
| 1 | VM + Docker light | **108 ms** (IPC 50, frame + GPU sync 58) — pass | **59/s** (timer driver 38/s) — pass |
| 2 | Docker spiked to ≈3.7 cores | 258 ms (IPC 124, frame 123) — fail | 24/s — fail |
| 3 | not started: 1-minute load above the run gate | — | — |

Reading: C4 and C5 pass with margin on a machine in normal use, and fail when another process takes about a quarter of the CPU. Both IPC and GPU wait roughly double under that contention, so this is host contention rather than a WebView limit. No architecture change is proposed. M1 adds margin: coarser drag previews on large plates (design §13) and smaller payloads.
