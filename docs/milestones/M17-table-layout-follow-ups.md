# M17 Table layouts: follow-ups

Started 2026-11-20 from the list left at the end of M16 ("Next, from the same test"):
what building a folded path on a breadboard by hand still needed the file for, or had to
be worked out by hand.

Exit criterion: a folded beam path with alignment irises, mirrors on library mounts and
an achromatic doublet built in the interface alone, without typing a mirror's normal,
a mount's name or a doublet's radii; and the light coming back into the laser kept low
by the optimiser (M17e, asked for during the milestone).

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

- [x] `mount` set from Properties (elements, sources, detectors): none, one of the
      file's `[[mount]]` tables, or a mount of the parts library (a file's table with a
      library key's id stands in for it, as when tracing); each mount's `source` on
      hover. A stack (`mount = [...]`) is shown as one on the other and kept until one
      mount is chosen in its place
- [x] The DOFs a mount gives listed under it in words (`M1_tip: tilt about x, -4 to
      4°`, `slide along z, 12.7 mm of travel`, `unlimited`); a name found in neither is
      marked. The library's mounts are read again after each change to the design
- [x] Checks: the file's mounts first, then the library's without those it stands in
      for; adjusters in words; in the window (`a_mount_is_chosen_from_the_parts_library`),
      a kinematic mount imported into the library chosen for M1 writes
      `mount = "example/KM1"` and the scene's M1 gets M1_tip and M1_tilt

## M17d An achromatic doublet from its focal length

- [x] A model glass, `material = { nd = …, vd = … }` (`optobench_materials::ModelGlass`):
      its index at the d line and Abbe number, the index between them from a two-term
      Cauchy formula (n = A + B/λ²) through nd with nF − nC = (nd − 1)/Vd, from 0.35 to
      2.5 µm (only indicative away from the visible); the d, F and C lines as ISO 7944
      gives them; `abbe` gives any material's nd and Vd from its own model, and `view`
      gives them for each glass of a lens
- [x] The lens editor's types include "achromat (cemented doublet)": from the lens's
      focal length, a crown (nd 1.5168, Vd 64.17) and a dense flint (nd 1.6727, Vd
      32.25), typical catalogue values, 6 and 2.5 mm thick. `lens::Achromat`: the
      thin-lens powers φ₁ = φ V₁/(V₁ − V₂), φ₂ = −φ V₂/(V₁ − V₂) (Hecht, ch. 6), an
      equi-convex crown, then its curvature and the flint's last by Newton's method on
      the paraxial trace until the power at d is 1/f and the powers at F and C are equal;
      the diameter kept where both glasses keep a 0.5 mm edge
- [x] A cemented doublet gets an Achromat section in Properties: f and each glass's nd
      and Vd (a named glass's from the scene); a change designs it again with its
      thicknesses, a glass typed becomes a model glass, one left keeps its name. The
      prescription's glass fields take `{ "nd": …, "vd": … }` too
- [x] Checks: the model gives back nd, nF − nC and Vd, and falls with wavelength; a
      scene reads `{ nd, vd }` and refuses a bad one; the thin powers add up to φ and
      cancel φ/V; the thick doublet's f at d is 100 mm and equal at F and C to 1e-9, its
      first radius within 5 % of the thin one; in the window
      (`a_singlet_becomes_an_achromat_of_the_same_focal_length`), the f 50.68 mm singlet
      made an achromat keeps f 50.68 mm at d in the engine's own `view`, and the same f
      at F and C to 1e-4

## M17e Light coming back into the laser

Asked for by the user (2026-11-20): a detector placed to measure backscattered light and
a merit term on it, so the optimiser places elements to keep that light low. What comes
back in a ray trace is the ghosts of partly reflecting faces (Fresnel reflections at
uncoated or coated faces, with splitting on); diffuse scatter from surfaces is not
modelled.

- [x] `returning = true` on a detector (layouts and sidecars): it stops nothing, and
      records the light crossing its plane against its axis, found afterwards on every
      ray's path (each step between hits, and an escaping ray's last direction). Placed
      at a laser and facing along its beam, it sees what comes back into the laser: its
      power, spot, angles and irradiance as any detector's, from the crossings inside it
- [x] Its `returned` power (`returned_w`, `returned_percent` in `trace`): every crossing
      anywhere in its plane weighted by exp(−2r²/w²), w half its smaller side, the light
      a Gaussian mode of 1/e² radius w takes in. It changes smoothly as the returning
      light moves, where a hard edge would give the optimiser no slope while the light
      lies wholly inside or outside
- [x] Merit term `backscatter` (`detector` a returning one, else refused): 100 ×
      returned / launched, in %. Its slope is zero where the returning light is centred
      (a face square to the beam), so a tilt that should send it away starts a little off
- [x] In the window: backscatter among the merit terms; Add a term puts it on the first
      returning detector or, with none, adds one (`R1`) at the first source, facing
      along its beam and as wide as it. Terms other than backscatter go on the first
      detector that is not returning
- [x] Checks (commands `tests/backscatter.rs`, theory: Fresnel R = ((n − 1)/(n + 1))² =
      0.04 at normal incidence, Hecht §4.6): an uncoated window square to a 2 mm beam
      returns R + T²R/(1 − R²) = 0.076923 of it to 1e-8 (reflections followed to 1e-12),
      the screen still gets T² directly, and the weighted share is the disc's mean of
      exp(−2r²/w²) to 0.005; from a 0.2° tilt the optimiser takes the backscatter from
      3.8 % to below 1e-3 % (at about 1.8°) with the beam still on the screen; a term on
      an ordinary detector is refused; power is conserved in every trace. In the window
      (`backscatter_is_added_with_a_detector_at_the_laser`), Add a term puts R1 at the
      laser and the trace weighs the returning light while the screen gets T²
