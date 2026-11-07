# M4 STEP scene import

Exit criteria: golden cage relay traced from STEP; intersection residual < 1e-9 mm;
re-export keeps matches.

Worked in four parts, one pull request each.

## Test files (decided 2026-11-07)

- **Vendor parts, local only.** Eleven Thorlabs STEP files (cage plates, rods, post,
  right-angle mirror mount, XY translator, fiber port adapter, mirror, two achromats, a
  plano-convex lens), listed in `corpus/basic_cage_parts.txt` and downloaded to
  `corpus/vendor/thorlabs/`. SolidWorks exports (2009–2022), AP203 and AP214, inch, metre
  and mm units. Never committed; tests that use them skip when they are absent.
- **Golden Thorlabs assembly, local only.** `corpus/solidcraft/thorlabs-cage.sc` and its
  STEP export: a fiber adapter and f = 30 mm achromat in an XY translator, a 45° mirror in
  a right-angle cube, an f = 60 mm achromat in a cage plate, eight cage rods, and on each
  arm a CP33B bracket clamped to the two lower rods with a 30 mm post screwed into it
  (optical axis at 51.58 mm above the table). Built in SolidCraft 0.30.1 through its
  control channel. Contains vendor geometry, so it is not committed.
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
