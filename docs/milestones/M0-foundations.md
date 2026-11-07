# M0 Foundations

Exit criteria: empty crates build everywhere; licence and asset checks green.

- [x] Workspace with layered crates (L0-L5, apps) and `[package.metadata.optobench] layer`
- [x] `optobench version` and `help`
- [x] `cargo xtask check`: layers, naming deny-list, bundled assets
- [x] CI workflow: fmt, clippy, tests on Linux/macOS/Windows; wasm32 engine build; cargo-deny; xtask
- [x] AGENTS.md, ATTRIBUTION.md, asset manifest, dual licence
- [x] Push to a git host and confirm all CI jobs pass (PR #1, 2026-11-07)
- [x] Command registry skeleton (JSON in, JSON out): `optobench-commands` with `serde_json` (MIT OR Apache-2.0)
