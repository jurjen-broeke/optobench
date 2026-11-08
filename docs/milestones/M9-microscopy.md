# M9 Microscopy pack

Exit criterion (docs/design.md, milestone table): a 4f scan/tube relay with a galvo
matches the hand calculation; the focal shift and PSF in water match published
stratified-media results.

The model is in docs/design.md, Optical model (Microscopy elements) and Samples
(Media and immersion). Everything here is from first principles and the cited
literature; no objective prescriptions are needed.

## M9a Ideal objective, immersion and index mismatch

An ideal objective obeys the Abbe sine condition: a ray through its pupil at height ρ
leaves towards the focus with n sin θ = ρ / f, the same invariant in every flat layer it
crosses (Snell). Each design ray is traced backwards from the design focus through the
design immersion and cover glass to the objective's front plane, and forwards from there
through the layers that are actually present: a matched design focuses perfectly, a
mismatched one shows its focal shift, spherical aberration and loss of aperture as traced
(the picture of Gibson and Lanni, J. Opt. Soc. Am. A 8, 1601 (1991)). Optical path is
set so that design rays reach the design focus in phase, for the PSF of M9c.

- [x] `objective` element: focal length, NA, pupil distance (pupil plane to front plane),
      working distance, design immersion, design cover glass (thickness, material) or
      none, design sample medium; a field point at f · tan(field angle); rays beyond
      f · NA stopped at the pupil
- [x] `sample` element: layers (thickness, material) with the medium `above` (the
      immersion) and `below`; the objective's rays continue in the medium in front of it
- [x] Cases: dry objective through a coverslip into water; water or oil immersion through
      a coverslip; water dipping without a coverslip; a coverslip-free well-plate bottom
- [x] `focus` analysis on a detector: where rays cross its axis by pupil height,
      paraxial and marginal focus, best focus (least RMS spot) and its RMS radius
- [x] Checks: a matched objective focuses at its design focus with no aberration; the
      paraxial focal shift into water, d (n₂/n₁ − 1), and the marginal one,
      d (tan θ₁ / tan θ₂ − 1); an oil objective into water passes no ray beyond
      n sin θ = n_water

Checked per ray, to 1e-9 mm, against Snell's law with the invariant (commands
`tests/objective.rs`): matched water immersion through its cover glass and matched water
dipping focus without aberration; a dry objective 0.5 mm into water puts each ray at
d tan θ₁ / tan θ₂, its paraxial focus at d n₂ / n₁; a polymer well-plate bottom in place
of the design glass moves each ray by t (tan θ_design − tan θ_actual) / tan θ_water; an
oil objective (NA 1.4) through glass into water passes no ray beyond n sin θ = 1.333.

A detector placed exactly on a refracting face meets the rays at the same distance as
the face, so they are counted before refracting or not at all: place screens off
interfaces (the tests use 0.05 mm).

## M9b Scanning and relays

- [x] Scan mirror: a mirror with a tilt DOF (for an in-plane scan in a planar layout,
      about the scene's z axis: `frame = "scene"`, `axis = [0, 0, 1]`), turning the beam
      by twice the angle
- [x] `ideal_lens` element: a perfect thin lens (slopes change by −h / f exactly), for
      relays and tube lenses checked against hand calculations
- [x] `sweep` command: trace over a DOF's values (`values`, or `from`, `to`, `steps`):
      per value every detector's direct power, centroid, RMS radius and pointing, and
      each source's chief ray element by element (where it crosses each element's plane
      and its slopes on arrival) and where it lands on its detector, at what slopes
- [x] Pupil conjugation and telecentricity from the sweep: the chief ray's height at an
      objective's pupil (pupil walk) and its slopes at the image
- [x] Exit check (commands `tests/scan.rs`): a galvo through a 4f relay (f₁ = 50 mm,
      f₂ = 200 mm) into an ideal objective, swept over ±2° in nine steps: at the scan
      lens height f₁ tan 2α; telecentric between the lenses; no pupil walk; at the pupil
      slope −(f₁/f₂) tan 2α; at the image f · that slope, telecentric and in sharp
      focus; each to 1e-9

## M9c Gaussian beams and the vectorial PSF

### M9c1 Objectives in CAD, Gaussian beams

- [x] An objective in a sidecar, placed by `position`/`axis` or a `frame` on a body (its
      pupil plane); `body` names its bodies (barrel and glass), which the ideal model
      replaces in the trace; samples in sidecars likewise
- [x] Ray records line up again: an objective records its face twice (arrival at the
      pupil, departure from the front plane), and every point carries its optical path
      (the index between two points is the difference over their distance); footprints
      and `sweep` skip the departure (before, points after an objective were attributed
      to the wrong faces)
- [x] `gaussian` command: a Gaussian source's beam at every element and detector its
      axis reaches: 1/e² radius, wavefront curvature, waist and distance to it per
      transverse direction, the index there, and an objective's pupil fill
      w / (f · NA). The paraxial ABCD of the traced system from the chief ray and four
      neighbours (so any optics the trace knows), q' = (A q + B) / (C q + D) with real
      slopes and the wavelength in the medium (Kogelnik and Li, Appl. Opt. 5, 1550
      (1966)); M² as an embedded Gaussian (Siegman, Lasers, §17.6)
- [x] Checks (commands `tests/gaussian.rs`): free space w(z) and R(z); an ideal lens
      moving and shrinking the waist as the q law says; a 4f expander filling an
      objective's pupil with the waist magnified f₂/f₁ (fill w / (f · NA)) and the
      objective focusing it to λ f / (π w); a fold mirror changing nothing. A sidecar
      objective replaces the CAD body it names (commands `tests/objective.rs`)

The DL40X-PA air objective and the SM1-to-M25 adapter (corpus/basic_cage_parts.txt) are
the real-world reference for objectives on a cage plate, once their STEP files and the
objective's specifications are in the corpus.

### M9c2 The vectorial PSF and the exit check

- [ ] Richards–Wolf vectorial PSF fed by the traced pupil field, in the stratified-media
      form (Török et al., J. Opt. Soc. Am. A 12, 325 (1995); Gibson and Lanni 1991)
- [ ] Correction collar as a DOF (the design cover-glass thickness it corrects for)
- [ ] Exit check: focal shift and PSF in water against published stratified-media
      results
