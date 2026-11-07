# M3 Standalone scenes and non-sequential trace

Exit criteria: a planar relay and a table periscope traced from scene files alone;
clipping by a tube proxy detected.

Worked in three parts, one pull request each. Status: done. Exit tests in
`crates/optobench-commands/tests/scenes.rs`, scenes in `examples/`.

## M3a Non-sequential trace (optobench-trace, L2)
- [x] Faces in the scene frame: an analytic surface or a cylinder, placed by a transform, bounded by a region (disk, annulus, rectangle), between two media
- [x] Roles: refracting (Fresnel loss, total internal reflection followed), mirror, absorber (clipping), detector
- [x] Nearest-hit search over all faces (brute force; a BVH comes with meshes in M4), interaction limit
- [x] Lens faces: hits on the profile outside the clear aperture are clipping at the element edge
- [x] Ray history for drawing; end state per ray (detected, clipped, escaped, stopped)
- [x] Power ledger: launched = detected + clipped + absorbed in media + reflected out + escaped, to 1e-12

## M3b Scene model (optobench-scene, L3)
- [x] Elements: lens (inline surfaces, thicknesses, materials, diameter, clear aperture), mirror; sources: collimated beam, point source; detectors with size and pixels
- [x] Mechanical proxies: tube (inner diameter, length), plate with hole (aperture stop)
- [x] Planar layout: positions by distance along a folded optical axis, folds by angle
- [x] Table layout: positions by grid hole and height on a breadboard, orientations by direction vector
- [x] Building the non-sequential system and launching source rays

## M3c Scene files, trace command, report, layout view
- [x] `.optobench` layout file (TOML): format tag, `kind = "layout"`, strict keys with the closest valid key named
- [x] Materials by name (bundled or imported) or index
- [x] `trace` command: per-detector power and spot, clipping report per proxy and element, power ledger (JSON)
- [x] 2D layout view as SVG (top or side), elements and rays
- [x] Exit tests: planar relay and table periscope from scene files; clipping by a tube proxy detected
