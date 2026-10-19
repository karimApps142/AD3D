# AD3D

A personal desktop app for generative fabrication design. It turns photos and 3D models into parts you can 3D-print, laser-cut or CNC-mill. It is a clean-room, better-engineered take on LuBan (luban3d.com). It runs only on the owner's 1–2 machines and is never sold or distributed.

## Status (2026-10-19)

The research is done and no app code exists yet. Start with [docs/START_HERE.md](docs/START_HERE.md).

## Decisions already made

- **A desktop app, not a website.** macOS comes first, Windows later.
  - The dev machine is an Intel i9 MacBook Pro (x86_64) on macOS 26.5. Ship a universal x86_64 + arm64 build.
- **Stack:**
  - Tauri 2 shell.
  - React + TypeScript UI with a react-three-fiber viewport.
  - A Rust geometry core as its own workspace crate, also exposed as a CLI, using rayon for multi-core work.
  - C++ libraries (manifold, Clipper2) through FFI where Rust has no equivalent.
  - [docs/PLAN.md](docs/PLAN.md) explains why, and lists the spike that has to confirm the stack before we build on it.
- **Build order:**
  1. Squama-style shadow plate (photo → tilted-facet plate).
  2. Lithophanes.
  3. Mesh cut-to-bed with connectors.
  4. Laser slicing (stack / waffle) with nesting.
  5. The rest of the manual.
- **Personal use only.** GPL/AGPL code may be used or ported. Do no work on licensing, accounts, payments, telemetry or updaters.

## Ground rules

- **Clean room.**
  - Build only from the manual (`Hello LuBan.pdf`; text in `docs/reference/hello-luban.txt`), public videos, blog text and papers.
  - Do not decompile, disassemble or run LuBan binaries, and do not work around its licence. None of its binaries are in this repo.
  - Use our own names, file formats and UI.
- **Generators are pure functions.** Each takes `(params, inputs)` and returns a mesh or 2D paths, and lives in the core crate.
  - It runs off the UI thread and can be cancelled.
  - It must output a watertight mesh with consistent winding. Back that with a test that randomises the parameters.
  - The Python prototypes do NOT pass this check yet.
- **Speed.**
  - While a slider is being dragged, the preview updates in ≤ 50 ms at typical sizes: show a low-res preview, then full-res when the slider is released.
  - Never block the UI.
  - Stream large exports to disk.
  - Benchmark hot paths with criterion.
- **Units** are millimetres everywhere. Angles are degrees in the UI and radians in code.
- **Physical testing is the owner's job.** When a feature is ready, hand over the output file path (STL/3MF/SVG) and a short checklist for printing or cutting it.

## Map

- `docs/START_HERE.md`: the handoff. What's done, what's verified, what's next.
- `docs/PLAN.md`: architecture, the spike's acceptance criteria, the roadmap and the "better than LuBan" list.
- `docs/research/`: the research. Read `00-final-report.md` first.
  - That report assumes a commercial product, so ignore its licensing, legal and pricing sections.
  - `06-design-web-first.md` is superseded, because we chose desktop.
- `docs/reference/`: the manual text and table of contents, plus LuBan's official Chinese docs.
- `prototypes/`: the Python Squama prototypes and a manifold WASM benchmark, kept for reference only.
