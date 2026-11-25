# optobench: instructions for agents and contributors

optobench is a standalone, clean-room, pure-Rust optics bench: ray tracing, power and
light-path simulation for SolidCraft assemblies and for CAD-free planar or table layouts.
The design is in `docs/design.md`; read the section for the area you touch before changing it.

## Non-negotiables

- **Clean-room.** Design from first principles, textbooks, published papers and public
  specifications. Never drive, script or extract data from commercial optics or CAD
  products (no test data, reference values or screenshots from them).
- **Licences.** The project is MIT OR Apache-2.0. Adapted code must be MIT or Apache-2.0
  and gets a row in `ATTRIBUTION.md`. Never read GPL code for adaptation.
- **Bundled data** only when truly open (clear licence compatible with MIT OR Apache-2.0:
  CC0, CC-BY-4.0, MIT, Apache-2.0). Every file under `assets/` is listed in
  `assets/manifest.toml` and `ATTRIBUTION.md`. Everything else (glass catalogues, parts
  libraries, coating curves, vendor STEP files) is downloaded by the user and never
  committed.
- **Rust only.** f64, `#![forbid(unsafe_code)]` everywhere. Internal units: mm, radians,
  µm for wavelength, W for power. Each element's local +z is its optical axis; the scene
  frame is Z up, as in SolidCraft.
- **Layers.** A crate depends only on crates in its own or a lower layer
  (`[package.metadata.optobench] layer` in each manifest). The engine (L0-L4) has no file
  system or network access and must build for `wasm32-unknown-unknown`.
- **SolidCraft stays outside the engine.** Only `optobench-io` (STEP) and
  `optobench-bridge` (control channel, Phase B) may know SolidCraft exists. Never read
  SolidCraft's `.sc` files; STEP is the interchange format.
- **Naming.** Write words in full, never abbreviate (analysis, assembly, layout, detector).
  The extension is `.optobench`; kinds are `sidecar`, `layout`, `part`. New names must pass
  `cargo xtask naming` and a read-aloud check in Dutch, German, French and Spanish.

## Versions

The application's version (`version` in the root `Cargo.toml`, shown by `optobench
version` and in the window's title bar) follows semantic versioning; it is separate from
the file-format number (`optobench/<N>`, see `docs/design.md`). 0.1.0 was set on
2026-11-14. Every merged pull request raises it once:

- a pull request that adds features: the minor number, +0.1.0 (0.1.0 → 0.2.0);
- one with only bug fixes or performance improvements: the patch number, +0.0.1;
- the first releasable version will be 1.0.0. From then on, once ten feature pull
  requests have been merged since the last major version (the minor number would reach
  10), the next one raises the major number instead: +1.0.0, minor and patch back to 0.

The pull request sets the new version in `Cargo.toml` and moves its CHANGELOG lines from
"Unreleased" under a heading with that version and the date. Raising the version publishes
a release (`.github/workflows/release.yml`), so a pull request that changes only CI (files
under `.github/`) keeps the version and needs no CHANGELOG line.

## Before you open a pull request

```
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask check          # layers, naming deny-list, bundled assets
```

## Tests

Reference values come from theory, cited literature, or MIT/Apache tools (Optiland
fixtures with the script and version that produced them). Every trace test also checks the
invariants in `docs/design.md`, Tests and validation (power conservation, unit directions).
Cite the source of every reference value in the test.

## Local corpus

`corpus/` holds examples and source data for reference and testing (STEP files, material
data, datasheets, papers). It is git-ignored and must never be committed, and the
`cargo xtask` checks skip it. Tests must pass without it; any value taken from it into a
committed test is cited there. See `corpus/README.md` on a machine that has one.

## Where things go

| Area | Crate |
| --- | --- |
| vectors, frames, root finding | `optobench-math` (L0) |
| surfaces / materials | `optobench-surfaces`, `optobench-materials` (L1) |
| rays and interactions | `optobench-trace` (L2) |
| scene, DOFs, proxies, sample / library lookup | `optobench-scene`, `optobench-library` (L3) |
| analyses / optimizer, tolerancing | `optobench-analysis`, `optobench-optimize` (L4) |
| graphs as data (built from results, no rendering) | `optobench-analysis` (L4) |
| sampled fields, propagation, rays to fields and back | `optobench-field` (L4, from M10) |
| file formats, STEP, reports, graphs as SVG and CSV / SolidCraft control channel | `optobench-io`, `optobench-bridge` (L5) |
| command registry (JSON in, JSON out) | `optobench-commands` (L5) |
| command line | `apps/optobench-cli` |
