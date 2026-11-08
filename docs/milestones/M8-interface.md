# M8 Interface

Exit criterion (docs/design.md, milestone table): a layout built, traced, optimised and
saved from the interface alone; the golden periscope's CAD workflow (tag, DOFs, optimise,
move report) done in the interface.

The design is in docs/design.md (Decided, 2026-11-08) and, on a machine with the corpus,
corpus/interface/DESIGN.md.

## M8a Frame and declarations

- [x] `apps/optobench-ui`: eframe with wgpu, SolidCraft's dark theme tokens (adapted,
      ATTRIBUTION.md) and optobench's own workspace accents: CAD blue, Layout turquoise,
      Simulate amber
- [x] Workspaces declared as data (`workspaces.rs`): tabs › sections › controls for CAD
      and Layout; numbered stages, a primary command and tools for Simulate. Every
      control is a registry command or an action of the interface; a test checks each
      command exists
- [x] Command metadata in the registry: label, icon name, effect (reads or writes files);
      buttons say why they cannot run yet
- [x] `view` command and `optobench_io::wireframe`: outlines of every body's faces,
      thinned for drawing; `trace` returns ray paths
- [x] Frame: title bar with path and data directory fields, workspace tile and switcher,
      ribbon, browser (sources, optics, detectors, DOFs, mechanics; show and hide),
      view (orthographic wireframe, rays coloured by how they ended, orbit, pan, zoom
      about the pointer, picking, gizmo, view toolbar), properties with a ready-to-trace
      card, results strip (messages, power ledger, detectors)
- [x] Commands run in the background; commands that write files ask first
- [x] Screenshot hook for checks without a person at the screen
      (`OPTOBENCH_UI_SCREENSHOT`, `_TRACE`, `_WORKSPACE`, `_SELECT`)
- [x] Checked on the golden periscope and on the Thorlabs beam-expander periscope (166
      items; view in 2.1 s, trace 99.9 % detected)

## M8b The 3D view

- [ ] Shaded view on wgpu: display tessellation of the exact faces, depth, edges on top
- [ ] Picking on the tessellation; hover highlight; selection shared with the browser
- [ ] Perspective and orthographic; section plane

## M8c Workspaces at work

- [ ] Layout tools: place, move and rotate elements, edit their properties, save
- [ ] Simulate stepper: per-stage editors (sources, optics, detectors and merit), Ready to
      trace from the design's checks, a strip of runs to compare
- [ ] CAD: tag bodies as elements, add DOFs, optimise and show the move report
- [ ] optobench's own icon set in SolidCraft's design language
- [ ] Import by dropping files on the window
- [ ] Exit test
