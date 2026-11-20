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

- [x] Iris and Tube (Layout, ADD; Tube under the section's menu): an iris is a `plate`
      with a 5 mm hole, 30 mm square; a tube 25.4 mm across, 50 mm long on a table
      layout, from 50 mm to 100 mm past the last item on a planar one. On a table layout
      they wait for a hole to be clicked like the other items
- [x] The browser lists them (Irises and tubes: `I1 · iris`, `T1 · tube`); before, a
      proxy could be selected only in the view
- [x] `plate` among a proxy's kinds in Properties; a new kind replaces the old kind's
      keys (a tube's bore and length or run, a plate's hole and size, a planar plate's
      `at`) in one edit, so the design still reads
- [x] Aim for lenses, irises, tubes and screens too: they face the light from the item
      chosen (their axis along it)
- [x] Checks: new irises and tubes on table and planar layouts, a planar tube made a
      plate, facing; in the window (`an_iris_is_placed_on_a_hole_and_clips_the_beam`), an
      iris clicked on hole (3, 0) in an 8 mm beam passes about (5/8)² of the power, and
      made a tube in Properties the file still reads

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
