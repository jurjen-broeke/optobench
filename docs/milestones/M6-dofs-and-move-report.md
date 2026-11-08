# M6 DOFs and move report

Exit criterion: on a golden periscope, applying the report's moves by hand restores
alignment in one iteration.

Worked in three parts, one pull request each (split agreed 2026-11-08). The golden
periscope comes in two kinds: one built from SolidCraft primitives and committed, like
the cage relay, and a local one built in SolidCraft from Thorlabs parts (KCB1 mounts,
PF10 mirrors). The move report is JSON plus a Markdown table next to the design.

## M6a Degrees of freedom

`dof = [...]` on elements, sources and detectors in layouts and sidecars
(`optobench_scene::dof`). A DOF is a slide (mm) or a tilt about a pivot (degrees in
files), with an optional range, `locked`, or `follows` another DOF with a `ratio`
(coupled). Axes and pivots are in the item's own frame, or the scene frame with
`frame = "scene"`; in a sidecar an element's DOF moves its whole CAD occurrence (the part
and its mount, as it is moved in CAD), with axes in the assembly frame by default and
tilts about the centre of the element's bodies. Poses are the nominal pose plus the DOF
offsets. `trace` takes `dofs = { name: value }` and reports every DOF's value.
- [x] DOF model: slide, tilt, coupled, locked; ranges; nominal pose plus offsets
- [x] DOFs in layout and sidecar files; `trace` applies DOF values
- [x] Tests: a fold mirror tilted by θ turns the beam by 2θ; a detector coupled to a lens
      keeps the focus; a 1 mm slide of the golden relay's second lens traces like the
      SolidCraft re-export moved by 1 mm

## M6b Optimiser and merit functions

Residuals are in mm (focus, centring), mrad (pointing, collimation) or a fraction
(power), so weights stay near 1 for ordinary benches. A scene's poses always match its
DOF values (readers place items at the file's values; `moved` moves by the change). On
the local iris cage, the LA1951-ML put 2 mm out along the rods is brought back to best
collimation, 0.074 mm from the paraxial position (the spherical aberration of its
convex-to-focus orientation), in 9 iterations and 14 traces.
- [x] Damped least squares (Levenberg–Marquardt, Marquardt scaling, central differences)
      over the free DOFs, within their ranges (`optobench_optimize::lm`)
- [x] Merit terms: focus (rms spot), centring (centroid on target), pointing (mean slope),
      collimation (rms slope), power; `[[merit]]` in the file or `merit` in the command
- [x] `optimize` command; tests: refocus behind a Descartes (aberration-free) lens, steer a
      periscope back onto its target, stop at a range limit

## M6c Move report and re-export diff

The golden periscope "with optics": two 45° mirrors and a 2× Galilean expander between
them, built from SolidCraft primitives. Misaligned (M1 0.4°, M2 −0.3°, L2 2 mm), the
optimiser finds the inverse moves (to 1e-4° and 1 µm) in 3.9 s; the report's
`occurrence.move` commands, run in SolidCraft and exported again, put every body where
the report expects (to 1e-14 mm) and restore centring (3e-8 mm), pointing (3e-7 mrad) and
the nominal collimation: one iteration.
- [x] Move report (`optimize` with `report`): per DOF the current value, the suggested one
      and the change; per moved item the move from the STEP pose as a translation, an axis
      and angle, the 3 × 4 matrix and the SolidCraft `occurrence.move` command; JSON and
      Markdown (`<design>.moves.json`, `.moves.md`). Layouts: the DOF values to set
- [x] Re-export diff (`check-moves`): every body's pose in the new export against the
      report's expectation, with tolerances, and the merit as it is now
- [x] Golden periscope (committed, primitives): the report's moves applied in SolidCraft
      restore alignment in one iteration. M6 exit criterion met.
- [x] Local Thorlabs version: the beam-expander periscope of `corpus/solidcraft/agent_designs.md`
      (2 mm 800 nm beam, 80 → 630 mm, BB1-E03 mirrors in KCB1 cubes, LC1715-B in a CXY1A and
      LA1509-B in an SM1ZB/M, ER10 rods, CP33B brackets on TR30 posts into a 20 × 60 profile).
      Misaligned in SolidCraft (bottom mirror 0.5°, L1 0.3 mm, L2 1 mm), the report's four
      moves made in SolidCraft and exported again put all 165 solids where expected (1e-13 mm)
      and restore centring (1.2 µm), pointing (0.24 µrad) and nominal collimation: one
      iteration. With 7 DOFs and 5 residuals the solution is not unique; the optimiser takes
      the smallest moves. Its 116 traces of the 13 MB assembly take about 25 minutes, which
      is the case for the performance work on the roadmap (design.md, Improvements).
