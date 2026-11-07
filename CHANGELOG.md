# Changelog

All notable changes, newest first. File-format bumps (`optobench/<N>`) are always listed
with the release that introduced them.

## Unreleased

- M0: workspace with layered crates, `optobench version`, repository checks
  (`cargo xtask check`: layers, naming deny-list, bundled assets), CI, design note.
- File format: `optobench/1` (defined, not yet written by any command).
- M0: command registry (`optobench-commands`): JSON parameters in, JSON result out, strict
  parameter keys, JSON errors with stable codes. Commands `version` and `commands`; the
  CLI is `optobench <command> [parameters]`. First third-party dependency: `serde_json`.
- CI: checks on GitHub Actions; `cargo xtask naming` also checks command ids.
