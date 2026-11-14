# M16 Table layouts in the interface

Started 2026-11-14 from the user's own test: building a beam path on a breadboard by
hand in the interface (a source at a height, a target, mirrors on kinematic mounts,
alignment irises), to find what the interface lacks. These are the first five findings;
the rest of that test follows on this page.

Exit criterion: a folded beam path built on a breadboard in the interface alone, with
every item's position readable and movable by its handles, and the bench advice on where
the alignment irises go.

## M16a Positions in Properties

- [x] Each entry's position as X, Y and Z in mm at the top of Properties: on a table
      layout typed and written back as its hole (fractions kept) and height; on planar
      layouts and sidecars as the scene placed it, to read
- [x] Vector fields name their numbers (x y z, column row, width height) and the main keys
      show their units; on a table layout the height is the position's Z
- [x] `view` gives each layout item's origin and axis, and the file's `[table]` and
      `[planar]`

## M16b The breadboard

- [x] `[table]` takes an optional breadboard: `columns` and `rows` (holes along x and y,
      together) and `corner` (the hole at its corner nearest the origin, default [0, 0]);
      its edge lies half a pitch beyond its outer holes. Only the interface uses it
- [x] Drawn under the items: a plate with its holes as filled circles (an M6 hole for the
      pitch), sized by the file or fitted around the items with two holes to spare; Fit
      and the view's depth take it in
- [x] Each item and source stands on a post down to the board, with a ring where it
      stands: its hole and height at a glance
- [x] The browser lists the Breadboard (show, hide, select); its Properties set the
      pitch, the size in holes and the corner hole, or keep the fitted size
- [x] New asks what to start: a planar layout, or a table layout with its pitch (25 mm,
      1 in or typed); before, New made only planar layouts, so a new design never showed
      a board (found by the user, 2026-11-14)
- [x] `edit` sets keys in `[table]`, `[planar]` and `[trace]` (`section`), making the
      section when it is missing

## M16c Graphs

Decided with the user (2026-11-14):

- [x] Graph asks what to draw: tick any number of kinds, Draw draws them all at once
- [x] One graph shown at a time, at half the strip's width; the list above it switches
      between the graphs open (one per kind; drawing a kind again replaces it); it never
      runs anything
- [x] A graph drawn before the design's last edit carries the reload icon with a warning
      dot ("out of date" in the list); Redraw draws the shown graph again, × closes it
- [ ] Side-by-side comparison of two graphs (later, if wanted)

## M16d Move

- [x] Move (Layout, MODIFY), as SolidCraft's: with an item or source of a table layout
      selected, arrows along the scene's x, y and z and rings about them at the item; a
      drag that starts on a handle moves the item instead of turning the view
- [x] Snapped to a step: one pitch to start (Pitch sets it again), or typed; turns to
      15° or what is typed; 0 for none
- [x] Drags add up into a pending move (asked for by the user, 2026-11-14): the item's
      outline, its handles, and its post with the ring where it stands all go where the
      move takes it, while dragging and after; the panel lists the steps. Enter (or
      Apply) writes the whole move as one edit (Undo puts it back) and ends Move;
      Escape (or Cancel, or Move pressed again) drops it and ends Move. While a move is
      pending the selection stays on the item
- [x] Checks: moves write holes (fractions kept), heights and turned axes, a whole move
      only the keys it changed; in the window, the periscope's screen dragged two holes
      along x and a quarter turn about z (its ring taken where the slide left it) leaves
      the file as it was until Enter writes hole (10, 0) and axis +y and ends Move; a
      move dropped by Escape leaves the file unchanged

## M16e Alignment irises

Decided with the user (2026-11-14): optimisation aligns on the target detector (the beam
centred on it, `centring`, with `pointing` for its direction); where to put irises on the
bench for the fine alignment is an output, not part of the design.

- [x] `trace` on a table layout gives `alignment_irises`: along the central ray of the
      first source, the last straight run before the detector (from, to, length,
      direction), the two holes on it furthest apart that keep 1.5 pitches clear of the
      mounts at its ends, at the beam's height there (one when only one fits, none on a
      short run), or on a run between holes a third and two thirds along, by their
      nearest holes; the mirrors that steer through them and the advice (close them down,
      the earlier mirror onto the near iris, the later onto the far one, repeat)
- [x] Shown in the view as rings across the beam on posts (I1, I2) and listed with the
      advice under Detectors
- [x] Checks (commands `tests/irises.rs`, from the layout's geometry): a beam folded by
      two 45° mirrors gets irises at holes (8, 8) and (18, 8), 250 mm apart, steered by
      M1 then M2; a diagonal run through holes gets them on (8, 10) and (14, 16), clear of
      its ends; a run along (1, 0.3), 0.1 holes or more from every hole, gets them off the
      grid by (9, 9) and (13, 10)

## M16f Placing by a click, and lenses as they are bought

Asked for by the user (2026-11-14), after building a path by hand:

- [x] Lens, Mirror, Source and Detector on a table layout wait for a hole: a marker
      follows the pointer on the board (the nearest hole, at the height new items get);
      a click places the item there, Enter after the last item as before, Escape
      nothing. Planar layouts add at once
- [x] A lens's own editor in Properties: its shape (biconvex, plano-convex,
      convex-plano, plano-concave, biconcave, meniscus, doublet, …), focal length (from
      the rear principal plane) and back focal length at the first source's wavelength
      (`optobench_scene::lens_cardinal_points`, the paraxial trace's cardinal points);
      a singlet designed from its focal length (the thick-lens equation, Hecht §6.2: a
      plano lens R₁ = (n − 1) f, an equi-convex or -concave one
      R₁ = −R₂ = (n − 1) f (1 + √(1 − t / (n f)))); the prescription radius by radius
      (R1…Rn, flat or not) with each glass's centre thickness and material, a cemented
      glass added or taken away
- [x] Lenses from their STEP files (`optobench_library::prescription`,
      `part-from-step`): the vendor's model of a bare or mounted lens gives its kind
      (singlet, cemented or air-spaced doublet, triplet), radii in order, centre
      thicknesses, glass or air between them and diameter; a body is glass when both its
      optical faces cover the axis (an odd number of each face's loops go round it, so a
      mount's rings do not); touching surfaces of one radius are cemented; the more
      strongly curved outer surface first, as spec sheets list them. With the vendor,
      part number, spec sheet, a glass per body and the clear aperture (a STEP file
      gives none) it becomes a library part (air as standard-air, its STEP products kept
      so sidecars recognise it). In the window: a lens's STEP file dropped shows what was
      found and asks for those
- [x] Lens opens a chooser: a singlet, or a lens of the parts library (with its kind and
      diameter, searchable), then placed as above
- [x] Checks: the singlet designs give back their focal lengths through the thick-lens
      equation; the relay's lens (R ±100 mm, n 1.5, 4 mm) has f 100.67 mm and fb
      99.33 mm; prescriptions from built bodies (a cemented doublet, an air-spaced doublet
      in a housing, a triplet turned round, a lone housing, overlapping bodies); the
      golden lens.step read as R 30, flat, 5 mm, Ø25.4 and imported; in the window, a
      hole clicked, Enter and Escape, and the golden lens dropped, imported and placed
      from the chooser. On the local corpus (not committed): AC254-030-A reads as a
      cemented doublet, LA1951 and the mounted LA1951-ML (its tube and ring left out) as
      the same plano-convex lens

## M16g A beam expander, and groups

Asked for by the user (2026-11-14): a 5× expander from an f −12 mm plano-concave and an
f 60 mm plano-convex lens, placed by the optimiser for a 1 mm coherent beam leaving
collimated and 5 mm across; and groups of items that move together.

- [x] Merit term `diameter` with a `target` (mm): 2√2 times the direct beam's
      power-weighted RMS radius, the edge diameter of an evenly filled disc and the 1/e²
      diameter of a Gaussian beam (exact for `gaussian` sources, whose rays carry
      Gaussian weights: 1.003 mm for a 1 mm waist after 100 mm; a `collimated` source's
      rings reach its edge with equal weights and read √((N + 1)/N) high, 8 % at the
      default 6 rings); among the merit terms in the window
- [x] The expander checked (commands `tests/expander.rs`): with thick plano lenses
      (Hecht §6.2: R₁ = (n − 1) f, back focal length f − t/n), afocal when L1's rear
      focal point meets L2's front one, L2's first vertex 68.681 mm along; a thin pencil
      collimates there to 1 µm. A 1 mm laser beam (Gaussian, collimation and a 5 mm
      target) converges in 13 iterations to 4.99 mm across and 0.11 mrad, 0.35 mm
      shorter than paraxial: the R −6.2 mm surface's spherical aberration, growing as the
      beam's radius squared (0.013, 0.08, 0.33 mm for 0.2, 0.5, 1 mm). Note: a Gaussian
      beam's rays carry its divergence, so the spread they minimise is the beam's far-
      field divergence (second moments obey the same ABCD law); for beams that diverge
      fast (waists of 0.2 mm or less here) the best spacing leaves the geometric one, as
      Gaussian-beam optics says it should
- [x] Groups (`optobench_io::groups`): an item (element, source, detector, proxy) joins
      one with `group = "<id>"`; an optional `[[group]]` gives it DOFs in the scene's
      frame (slides; tilts about a `pivot` every member turns about), each the first
      DOF-carrying member's, the others following it one to one; proxies move with Move
      only. Checked: a group's slide moves both lenses 3 mm and nothing else; a quarter
      turn about (75, 0, 75) takes L1 from x 50 to (75, −25) and L2 from x 100 to
      (75, 25); a tilt without a pivot, a group DOF with a frame, a group of proxies
      only, and unknown keys refused
- [x] In the window: Properties sets an item's group (none, one of the design's, or a
      new one typed); the browser lists the groups with their members (a click selects
      the first); Move on any member moves the whole group (every post follows, the
      panel names the group), its turns about the selected item; a window test drags a
      group of two lenses two holes along and leaves the detector where it was

## M16h Merit in the Layout workspace; the lens editor in the order a lens is chosen

From the user's test (2026-11-14): merit terms could be set only in Simulate; setting a
lens's parameters reset others instead of adjusting them; plano lenses needed one flat
side and one radius; a lens could not be turned round. The order asked for: name, type,
focal length with radius, thickness and index, then the diameter.

- [x] The browser always lists the Merit, with Add a term (centring, pointing,
      collimation, diameter with a 5 mm target, focus, power, headroom; each explained);
      a term opens in Properties; a note when there are terms but no degrees of freedom
- [x] Properties: the name first; for a lens its type (biconvex, plano-convex,
      plano-concave, biconcave, meniscus, or custom radius by radius for doublets and
      more), then f, R (one for plano and equi lenses, R1 and R2 for a meniscus), tc and
      the glass (a name, its index from the scene, or an index), fb, then the diameter
      and clear aperture
- [x] Linked (`lens.rs`, the thick-lens relations of Hecht §6.2): f redesigns R (keeping
      the type, thickness and glass); R gives f; a new tc or index keeps f and redesigns
      R; a new type keeps |f| (its sign from the type); a meniscus keeps its convex
      radius and bends the other. The diameter is kept where the edge stays 0.5 mm thick
      and both surfaces reach the rim: a redesign that leaves no edge makes it smaller,
      with a note; the clear aperture stays within it
- [x] Flip turns a lens round (its surfaces in the other order; a library lens by
      `reversed`); the type says which side comes first
- [x] Checks: every type gives back its focal length either way round; a plano lens has
      R = (n − 1) f; the relay's lens (R ±100, n 1.5, 4 mm) has f 100.6711 mm and its
      radii back from it; the largest diameter keeps a 0.5 mm edge; in the window, a
      collimation term added from the browser, a biconvex singlet made plano-concave
      (R −25.3 mm, flat) and flipped

## M16i Degrees of freedom on the board; evenly weighted beams

From the user's test of the expander (2026-11-14): Optimise said there were no degrees
of freedom, and the only way found to move L2 along the beam was "slide x", which on a
table layout slid it across (the item's own axes, z along its optical axis) and only
±5 mm.

- [x] On a table layout (and in a sidecar) Add gives slides along the scene's X, Y and Z
      (the board's axes, as the view's gizmo shows them), "along its axis" (its own
      optical axis, any orientation), and tilts about X, Y and Z; slides start at two
      holes either way, tilts at 5°. Planar layouts: along its axis, across x and y,
      tilts. Each DOF's range is edited where it is listed
- [x] Optimise without any DOF says how to add one
- [x] `collimated` and `point` sources weight their rings by the area each stands for
      (`hexapolar_weighted`: ring k the annulus (k − ½)/N to (k + ½)/N, cut at the rim):
      equal weights had put too much light at the rim (the second moment 15 % high at 6
      rings, the `diameter` term 7.6 % high; now 0.7 %, 2 mm reads 2.014 mm). The golden
      cage relay's report was made again: its theory checks hold (image −2 mm, 0.96⁴
      through), only weight-dependent values moved (RMS spot 0.036 → 0.033 mm; the
      ghosts' clipped powers within 6 %). On the golden periscope the collimation best
      for L2 moved 11 µm (it is flat there): the report now asks −1.989 mm where it asked
      −2.000, so its tests take 20 µm (check-moves' `tolerance_mm` 0.02 against the
      realigned export made from the old report), and the realigned L2 collimates a little
      better than the nominal design (0.01284 against 0.01291 mrad). Corpus traces with
      these sources change the same way
- [x] Check (commands `tests/expander.rs`): the expander on a 25 mm board, L2 with a
      slide along X (±50 mm) from 26 mm off, a collimated 1 mm source, collimation and a
      5 mm diameter at D1: converged a little short of the paraxial 98.681 mm (the 1 mm
      beam's spherical aberration), 5 mm across within 1 %, collimated

## Next, from the same test

Noted while reading the code before the user's test; not yet confirmed by it:

- A mirror's normal worked out from where the beam comes from and should go
- An achromatic doublet designed from its focal length (needs the glasses' dispersion)
- Add for proxies (irises as plates, tubes) and `plate` among a proxy's kinds
- `mount = …` (and the library's mounts) set from Properties
