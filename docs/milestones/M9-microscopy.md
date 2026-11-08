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

- [ ] Scan mirror: a mirror whose tilt is a DOF swept over a range
- [ ] `sweep` command: trace over a DOF's values (positions on a detector per angle)
- [ ] Pupil conjugation and telecentricity: chief-ray height at a pupil plane and its
      angle at the image, per scan angle
- [ ] Exit check: a 4f scan/tube relay with a galvo against the hand calculation
      (scan angle to field position, pupil walk)

## M9c Gaussian beams and the vectorial PSF

- [ ] Gaussian beams by the complex q parameter through the paraxial elements; aperture
      fill at the objective's pupil
- [ ] Richards–Wolf vectorial PSF fed by the traced pupil field, in the stratified-media
      form (Török et al., J. Opt. Soc. Am. A 12, 325 (1995); Gibson and Lanni 1991)
- [ ] Correction collar as a DOF (the design cover-glass thickness it corrects for)
- [ ] Exit check: focal shift and PSF in water against published stratified-media
      results
