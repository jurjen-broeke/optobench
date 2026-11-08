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
      (`OPTOBENCH_UI_SCREENSHOT`, `_TRACE`, `_WORKSPACE`, `_SELECT`; from M8b also `_VIEW`,
      `_PERSPECTIVE`, `_ZOOM`, `_SECTION`)
- [x] Checked on the golden periscope and on the Thorlabs beam-expander periscope (166
      items; view in 2.1 s, trace 99.9 % detected)

## M8b The 3D view

- [x] Display tessellation of the exact faces (`BrepFace::tessellate`): boundary loops
      mapped to the surface's parameters, cut at a seam on periodic surfaces (whole
      period, reaching poles and apexes), thinned to the chord tolerance; a constrained
      Delaunay triangulation (`spade`, MIT OR Apache-2.0) with the loops as constraints and
      a grid fine enough for the curvature; each region between constraints decided by
      the side of its loops (the face lies left of them, seen along its normal), the exact
      inside test only for regions the boundary does not touch. Boundary vertices keep
      their exact points
- [x] `optobench_io::mesh::meshes` for a whole scene (B-rep faces, lens and mirror
      surfaces, proxy tubes); `view` with `mesh` returns them;
      `examples/mesh_survey` counts triangles and time per surface type
- [x] Checks: the tessellated fixtures enclose their volumes (block exact; cylinder,
      sphere, torus, cone and plano-convex lens within 0.5 % at 0.005 mm); surface tests
      for a plate with a hole, a reversed plane, a cylinder band without a seam edge, a
      half cylinder, a whole sphere, an unbounded plane
- [x] Shaded view on wgpu (`render.rs`): offscreen, 4× multisampled, depth; lit from the
      viewer with a little sky from +z; edges and rays as depth-tested lines moved a hair
      towards the eye; lenses as translucent glass drawn last; per-item style buffer for
      hidden, selected and hovered
- [x] Picking and hover on the triangles (the nearest hit along the line of sight,
      skipping hidden items and parts cut away); selection shared with the browser
- [x] Perspective and orthographic (30° field); section plane across x, y or z with a
      slider, cutting away the viewer's side
- [x] Thorlabs beam-expander periscope: 191 000 triangles in 1.0 s; the window opens,
      traces and shows it in about 6 s

Found on the way, for the engine (not changed here): on strongly curved faces (a torus
patch 180° round its tube) `BrepFace::contains` accepts points outside the face, because
the nearest boundary segment in 3D can lie across the surface. Closed bodies trace
correctly (their faces cover the surface together), but a lone curved patch could give
hits where there is no face. The tessellation no longer depends on it.

## M8c Workspaces at work

### M8c1 Editing through the registry

- [x] `optobench_io::edit`: edits that keep a file's comments and layout (`toml_edit`):
      `set` and `unset` a key of an entry (by table and id, `[[merit]]` by index, or a
      degree of freedom by name), `add` and `remove` entries, `add_dof` and `remove_dof`,
      `replace` the whole text; `[resolved]` stays last; new planar or table layouts
- [x] `edit` command: writes only when the result still reads as a scene with the
      user's data (a sidecar is read with its STEP file), through a temporary file;
      returns the previous text for undoing; `dry_run`; `create` for a new layout
- [x] Checks: an edit written and undone; breaking edits refused with the file
      untouched; a layout built from nothing and traced; the optimiser's suggestions
      written back as DOF values realign the misaligned golden periscope

### M8c2 Editors in the interface

- [x] `view` returns the file's entries as written (`design`) and each CAD body's own
      axes; editors for every key (numbers, vectors, words with their choices, text,
      anything else as JSON), committed when typing or dragging ends, through `edit`;
      the scene is read and traced again after each change
- [x] Layout tools: New, Undo, Remove, and add lens, mirror, source or detector after
      the last item (planar: 50 mm further; table: two holes further); place, move and
      rotate by their keys (`at`, `fold`, `hole`, `height`, `axis`)
- [x] Degrees of freedom on each entry: value within its range, add slide or tilt along
      the scene's axes, remove
- [x] Simulate stepper: editors per stage (sources; optics; detectors and merit terms,
      add a term by kind); a runs strip comparing each trace with what changed before it
- [x] CAD: Tag Lens, and Tag Mirror on the face clicked (its normal in the body's own
      frame becomes `reflect`); Untag; Optimise, then Apply writes the suggested values
- [x] Dropped files: designs open; parts, curve tables (xlsx, csv, txt, zip) are
      imported; a material's licence notice is shown to accept before importing
- [x] Checks: an edit written, read again, refused when it breaks the design, undone;
      the golden periscope's M2 untagged (the light misses) and tagged again by its face
      (all of it arrives); new entries get free ids and places after the rest

### M8c3 Icons and the exit test

- [ ] optobench's own icon set in SolidCraft's design language
- [ ] Exit test: a layout built, traced, optimised and saved from the interface alone;
      the golden periscope's CAD workflow (tag, DOFs, optimise, move report) in the
      interface
