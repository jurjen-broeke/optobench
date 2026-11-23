# M21 A web version (outline, not scheduled)

Noted 2026-11-23, after M20, at the user's request: a build that runs in a browser, for
quick testing on a server. Porting the whole desktop interface now would be too much of
a compromise; the user is examining which components suit a web version and will fill
in this outline once its scope is clearer.

Exit criterion: to be decided.

## Where things stand (2026-11-23)

- The engine (L0-L4) builds for `wasm32-unknown-unknown`, checked by CI.
- `optobench-commands` (L5) does not: `Gpu::blocking` and `Gpu::walk_blocking` are not
  there on wasm32 (M15's tissue walk; on the web it would fall back to the CPU or use
  the browser's asynchronous WebGPU).
- `optobench-ui` has no web entry point (`eframe::run_native` only, no `index.html` or
  `wasm-bindgen` set-up) and about 95 uses of what a browser does not offer: `rfd`'s
  blocking dialogs, `std::fs`, the SolidCraft control channel (`optobench-bridge`),
  threads, `std::time::Instant` and the `--data-directory` argument.

## Direction (the user, 2026-11-23)

- Limited to the simple design process: table layouts on a breadboard with mirrors and
  lenses. No STEP import or CAD parts, so no sidecars and no SolidCraft control channel.
- Getting designs in and out of a browser is more cumbersome than on the desktop, and a
  browser keeps nothing between visits unless the web version stores it; both weigh on
  how useful it is.

## Questions to settle

- Which parts of the layout process come along (tracing, analyses and graphs, the
  optimiser) and whether the parts library and downloaded data stay desktop-only
- How designs and data reach the browser (file picker and download, bundled examples)
  and whether work is kept between visits (the browser's own storage)
- Renderer (WebGPU with a WebGL fallback) and running on one thread
- How the build is made and published (tooling, CI, hosting)
