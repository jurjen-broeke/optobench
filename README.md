# optobench

A clean-room, pure-Rust optics bench for checking light paths: rays and power traced
through SolidCraft assemblies (via STEP) or through CAD-free planar and optical-table
layouts. Microscopy is a first-class use: immersion and dipping objectives, water,
fluorescence excitation, photon budget and laser heating.

Status: **M3 Standalone scenes**. Planar and table layouts are traced non-sequentially
from `.optobench` files, with clipping by mechanical proxies, detector results, a power
ledger and an SVG layout view. M2 is complete except for checking the bundled material
values against their original papers and the literature lens cases. See [docs/design.md](docs/design.md) for the design and roadmap.

![Roadmap](docs/roadmap.svg)

## Getting started

Requires a stable Rust toolchain (`rust-toolchain.toml` pins the components and the wasm32
target).

```
cargo build
cargo test --workspace
cargo run -p optobench-cli -- version          # prints JSON
cargo run -p optobench-cli -- commands
cargo xtask check
```

Trace an example scene and write its layout view:

```
cargo run -p optobench-cli -- trace '{"path": "examples/planar-relay.optobench", "svg": "relay.svg"}'
```

`examples/` holds a folded planar relay, the same relay clipped by a narrow tube, and a
periscope on a breadboard. The file format is documented in
`crates/optobench-io/src/scene_file.rs`.

Open the folder in VS Code and accept the recommended extensions
(rust-analyzer, Even Better TOML).

## Layout

```
apps/optobench-cli      the `optobench` command
crates/optobench-*      engine and I/O crates, one per layer (see AGENTS.md)
crates/optobench-commands  the command registry the CLI and later front ends call
xtask/                  repository checks: layers, naming deny-list, bundled assets
assets/manifest.toml    every bundled data file and its licence
docs/design.md          the design note
docs/milestones/        task lists per milestone
examples/               example .optobench scenes, also used by the exit tests
corpus/                 local examples and source data (git-ignored, never committed)
```

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option. Bundled third-party material is listed in
[ATTRIBUTION.md](ATTRIBUTION.md).

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual
licensed as above, without any additional terms or conditions.
