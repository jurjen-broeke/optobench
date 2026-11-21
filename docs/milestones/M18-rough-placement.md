# M18 Rough placement, then fine adjustment

Started 2026-11-21 from the user's test of M13's write-back on the Thorlabs periscope: a
part sliding along its rods (or placed on a breadboard in a layout) is limited only by
the space around it, while a mount's adjusters have their travel and are for the fine
alignment once the rough placement has brought the beam within their reach.

Decided with the user (2026-11-21):

- a DOF with `range = "free"` is a rough placement: its limits are where its part would
  overlap another part; every other DOF, and every adjuster a mount gives, is a fine
  adjuster with its travel
- a rough placement moves its part with its group (in a sidecar a `[[group]]` names the
  untagged CAD parts that go with it, such as the lens's mount); touching is allowed,
  only an overlap deeper than 0.05 mm stops it
- Optimise works in two stages: the rough placements first, every adjuster held in the
  middle of its travel; then the adjusters alone. Apply (and Apply in SolidCraft) writes
  both

Exit criterion: on the Thorlabs periscope with L1 moved 60 mm along its rods, one
Optimise brings it back within its mount's reach and centres the beam on `out`, without
any part overlapping another.

## M18a Free ranges and groups of CAD parts

- [x] `range = "free"` on a slide or tilt (not with `travel`, `follows` or a collar)
- [x] A sidecar's `[[group]]` takes `occurrences = [...]`: untagged CAD parts that follow
      the group's DOFs one to one, as its other members do
- [x] Checks (commands `tests/golden.rs`, `tests/groups.rs`): on the golden relay, L2
      and its cage plate (untagged) in a group with a free slide along z: at 1 mm the
      DOFs are `L2_rail` and `L2_rail_cage-plate:2` and the trace is the moved
      re-export's (both moved 1 mm); a free range with a travel, a part the assembly does
      not have, a range of another word, and `occurrences` in a layout refused

## M18b How far a part can move

- [x] `optobench_scene::clearance`: for a DOF, how far its part (with everything moving
      with it) can go each way before a point of it lies deeper than the tolerance inside
      another part, or a point of another part inside it: points from the parts'
      tessellations (vertices, and their triangles subdivided to a spacing), inside and
      outside from the exact faces (the nearest hit's normal along a ray), each point's
      path a ray for a slide and short chords for a tilt; what stops it named
- [x] `clearance` command: every free DOF's range and what stops it each way
- [x] Fast enough for vendor assemblies: each part tessellated once; on a slide, points
      on one line parallel to it share a ray, cut to the other part's box, and points
      whose line never passes the other part are left out; the smallest limit found so
      far, shared between the cores, cuts every later path short (nearest parts first).
      On the Thorlabs periscope (L1 with its CXY1A, 440 000 surface points): 61 s at
      first, now 7.8 s; L1 with its mount can slide from −30.93 mm (a CP33B bracket) to
      +38.28 mm (L2)
- [x] Found on the way: an entry was dropped when the same segment left the part again
      further on (a plate's far side), so plates passed through each other; the depth is
      now checked against the next hit
- [x] Checks (commands `tests/golden.rs`): on the golden relay, L2 with its cage plate
      slides down to −117.55 mm (the plates 117.5 mm apart, plus the 0.05 mm tolerance,
      faces meeting square), stopped by `cage-plate:1`, and upwards nothing stops it;
      tilted about x through L2, the rods through the plate's holes stop it within a few
      degrees each way

## M18c Two-stage optimisation

- [ ] `optimize`: free DOFs take their clearance as their range; stage one moves only
      them with every adjuster at the middle of its range, stage two only the adjusters
      from there; the report gives both stages
- [ ] In the window: the two stages in the results; Apply writes both

## M18d Layouts

- [ ] The same for table layouts: elements, detectors and proxies as solids for the
      clearance (a lens's or mirror's disc, a screen, a tube, a plate)
