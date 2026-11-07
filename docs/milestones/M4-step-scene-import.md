# M4 STEP scene import

Exit criteria: golden cage relay traced from STEP; intersection residual < 1e-9 mm;
re-export keeps matches.

Worked in four parts, one pull request each.

## Test files (decided 2026-11-07)

- **Vendor parts, local only.** Thirteen Thorlabs STEP files (cage plates, rods, post,
  rod bracket, right-angle mirror mount, XY translator, fiber port adapter and FiberPort,
  mirror, two achromats, a plano-convex lens bare and mounted), listed in `corpus/basic_cage_parts.txt` and downloaded to
  `corpus/vendor/thorlabs/`. SolidWorks exports (2009–2022), AP203 and AP214, inch, metre
  and mm units. Never committed; tests that use them skip when they are absent.
- **Golden Thorlabs assembly, local only.** `corpus/solidcraft/thorlabs-cage.sc` and its
  STEP export, built in SolidCraft 0.30.1 through its control channel. Optical axis at
  51.58 mm above the table. On the x arm, from the far end: a PAF2-A4C FiberPort
  (collimated output) on a CP08FP adapter at the rod ends (x = 302.26 mm); a relay of an
  LA1951-ML in a CP33 plate (curved side to the FiberPort, focus 17.6 mm beyond its flat
  face) and an AC254-030-A in a CXY1A translator (flint side to that focus, front focal
  length 22.2 mm), which re-collimates; a PF10 mirror at 45° in a KCB1 cube folds the
  axis into the −y arm, where an AC254-060-A sits in a CP33 plate. Lens spacings are
  nominal, from catalogue glass indices, and are checked by the trace once real glass
  data is imported. Eight ER10 rods; on each
  arm a CP33B bracket clamped to the lower rods with a TR30 post. Contains vendor
  geometry, so it is not committed.
- **Committed golden assemblies.** Built in SolidCraft from its own primitives (plates,
  rods, lenses with exact spherical faces), geometrically equivalent to the Thorlabs
  assembly, exported to STEP and committed with their sidecars and expected reports.
- **Hand-written STEP files** for unit tests of single entities and edge cases.

## Findings so far

- SolidCraft writes AP242 (`AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF`) in mm,
  one entity per line. Products are named after components; each placement is a
  `NEXT_ASSEMBLY_USAGE_OCCURRENCE` named after its occurrence (`er10:3`).
- Its export is deterministic: two exports with a move and its reversal between them
  differ only in the file name in the header. Product names, tree and entity numbers are
  stable (answers the open question in `docs/design.md`).
- Vendor files name products and bodies after internal file numbers (`3245-E0W`,
  `TTN034507-1-solid1`), not catalogue numbers, so matching vendor parts needs the
  sidecar or geometry, not the product name.
- Vendor files mix length units across contexts (inch, metre, mm); one body has
  inward-facing faces (negative volume).

## M4a STEP reader (optobench-io)

Our own reader, not SolidCraft's crates (decided 2026-11-07). Reads all eleven
Thorlabs files, LA1951-ML and the golden assembly (3341 faces, 116 placed bodies,
0.4 s) with no unsupported entities.

- [x] ISO 10303-21 reader: header, entities, complex (multi-type) entities, strings and enumerations
- [x] AP203 / AP214 / AP242 product structure: products, assembly tree, placements per occurrence
- [x] Units per representation context, converted to mm at the boundary
- [x] B-rep: solids, shells, faces with bounds, surfaces (plane, cylinder, cone, sphere, torus, B-spline, surfaces of revolution and linear extrusion), edges and curves (line, circle, ellipse, B-spline)
- [x] Unsupported entities reported by type, never skipped silently

## M4b Geometry
- [x] Exact faces (`optobench_surfaces::brep`): plane, cylinder, cone, sphere, torus, B-spline (rational too), surfaces of revolution and extrusion; trimmed by their boundary loops (seams left out)
- [x] BVH over the faces' bounding boxes (`optobench_trace::bvh`), used by the non-sequential trace; STEP faces as `Shape::Brep`
- [x] Every hit computed on the exact surface; residual < 1e-9 mm: on the golden assembly (5197 faces, 20 000 rays) the worst is 3.7e-10 mm, and the BVH agrees with a search over every face
- Changed from the plan: no triangle mesh. Exact intersection over a face BVH gives the accuracy directly; a mesh is only needed for display (design.md updated).

## M4c Sidecar v1 and matching
- [ ] `kind = "sidecar"`: elements, sources, detectors referring to bodies
- [ ] Matching by product name, then assembly path, then geometry fingerprint; unmatched entries are errors
- [ ] Optical faces from STEP surfaces with sidecar materials; untagged bodies absorb

## M4d Trace and round trip
- [ ] `trace` for STEP + sidecar
- [ ] `export-placements` from standalone scenes
- [ ] Golden cage relay traced from STEP; matches survive a re-export
