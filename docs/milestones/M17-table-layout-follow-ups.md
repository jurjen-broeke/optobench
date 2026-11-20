# M17 Table layouts: follow-ups

Started 2026-11-20 from the list left at the end of M16 ("Next, from the same test"):
what building a folded path on a breadboard by hand still needed the file for, or had to
be worked out by hand.

Exit criterion: a folded beam path with alignment irises, mirrors on library mounts and
an achromatic doublet built in the interface alone, without typing a mirror's normal,
a mount's name or a doublet's radii.

## M17a Aiming a mirror

- [x] Properties of a table layout's mirror: where the light comes from and where it
      should go (any other item or source), and Aim sets its axis to the outgoing
      direction minus the incoming one, scaled so its largest number is 1 (the
      reflection law: the normal halves the turn); the axis as written stays editable
- [x] Checks: the folds of a hand-built layout (+y to −x gives (−1, −1, 0); −y to +z,
      a periscope's lower mirror, gives (0, 1, 1)); a 60° turn leaves at the angle it came
      in; straight back faces the light; straight on, or an item at its own place, is
      refused with a note; in the window (`a_mirror_is_aimed_from_its_properties`), a
      mirror facing the wrong way aimed from S1 to D1 gets (−1, 1, 0) and the beam
      reaches the screen

## M17b Proxies added from the interface

- [ ] Add for proxies on a layout: an iris (a `plate` with a hole) and a tube, placed by
      a click on a hole like the other items; `plate` among a proxy's kinds in
      Properties
- [ ] Checks: an iris added on a hole traces and clips; a tube added on a planar layout
      runs between two places along the path

## M17c Mounts from Properties

- [ ] `mount` set from Properties: none, one of the file's `[[mount]]` tables, or a
      mount of the parts library; the DOFs it gives listed
- [ ] Checks: a library mount chosen writes its key and the item gets its adjusters

## M17d An achromatic doublet from its focal length

- [ ] A model glass, `material = { nd = …, vd = … }`: its index at the d line and Abbe
      number, the index between them from a two-term Cauchy formula (n = A + B/λ²)
      through nd with nF − nC = (nd − 1)/Vd
- [ ] The lens editor designs a cemented achromat from its focal length and two glasses
      (a crown and a flint, each a name or nd and Vd): the thin-lens powers
      φ₁ = φ V₁/(V₁ − V₂), φ₂ = −φ V₂/(V₁ − V₂), an equi-convex crown, then the radii
      corrected for the thicknesses so the paraxial focal length is f at the d line and
      equal at the F and C lines
- [ ] Checks: the model glass gives back nd and nF − nC; the thin-lens powers add up to φ
      and cancel their chromatic change; the thick doublet's f at d is the one asked for
      and its f at F and C agree
