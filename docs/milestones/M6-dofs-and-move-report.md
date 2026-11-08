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
- [ ] Damped least squares (Levenberg–Marquardt) over the free DOFs, within their ranges
- [ ] Merit terms: focus (rms spot), centring (centroid on target), collimation, pointing,
      power; weights and targets in the file or the command
- [ ] `optimize` command; tests on analytic cases (refocus to the back focal length, steer a
      periscope back onto a target)

## M6c Move report and re-export diff
- [ ] Move report: per element and DOF the current value, the suggested one and the change,
      in mm and degrees in the CAD frame (layout coordinates for layouts); JSON and Markdown
- [ ] Re-export diff: after the moves are made in CAD and exported again, each pose is
      compared with the suggestion and mismatches are flagged
- [ ] Golden periscope (committed, primitives) and the local Thorlabs one: the report's moves
      applied by hand restore alignment in one iteration
