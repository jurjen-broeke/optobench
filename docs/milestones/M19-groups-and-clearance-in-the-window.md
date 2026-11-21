# M19 Groups of CAD parts and the clearance in the window

Started 2026-11-21, after M18, asked for by the user: what M18 put in the file format
made from the window, so a rough placement can be set up and tried on a CAD design
without editing the sidecar.

Exit criterion: on the Thorlabs periscope, L1's CXY1A made to move with L1, the group
given a free slide along z, and its clearance shown, all in the window.

## M19a Moving with a part

- [x] Properties of an untagged CAD part (a sidecar's absorber): "moves with" none or one
      of the design's elements; choosing one puts that element in a group (its own, or a
      new one named after it, `<id> assembly`) and the part's occurrence in the group's
      `occurrences`; none takes it out
- [x] The browser lists a group with its items and its CAD parts; clicking it opens the
      group's Properties (and selects its first item, so Move moves the group)

## M19b A group's degrees of freedom

- [x] A group's Properties list its DOFs as an item's do, with Add for slides along the
      scene's X, Y and Z, free to start (a group's DOFs are in the scene's frame: no
      `frame`)

## M19c The space around a free DOF

- [x] Under a free DOF in Properties: Find the space runs `clearance` for it (in the
      background; with Live on, against the design as SolidCraft has it); the range found
      and what stops it each way stay shown until the design changes

## Checks

- [x] `move_with` (interface `editors.rs`): a part added to a lens's group, moved from one
      group to a new one (the lens given `<id> assembly`), taken out of its group, and
      left where it is when asked again
- [x] In the window (`a_mount_moves_with_its_lens_and_the_space_is_found`), on the golden
      relay: cage plate 2 selected, "moves with" L2 writes `group = "L2 assembly"` on L2
      and the group with `occurrences = ["cage-plate:2"]`; the browser lists it as
      `L2 assembly · L2, cage-plate:2`; its Properties' slide Z writes a free slide; Find
      the space shows −117.55 mm, stopped by `cage-plate:1/Plate`, up to +513.27 mm with
      nothing in the way
- [x] Exit, by the user on the Thorlabs periscope (2026-11-21): the CXY1A made to move
      with L1, the group given a free slide along Z, and its space found, "as expected"
