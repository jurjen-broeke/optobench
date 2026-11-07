# M1 Sequential core

Exit criteria: thin/thick lens and parabola cases pass; EFL matches theory to 1e-9.

## optobench-math (L0)
- [ ] `Frame` (origin + orthonormal basis) and rigid `Transform` (rotation + translation), with inverse and composition
- [ ] Rotation from axis-angle; conversion helpers for degrees at the boundary only
- [ ] Scalar root finding: safeguarded Newton with bisection fallback, with iteration and tolerance reporting

## optobench-surfaces (L1)
- [ ] `Surface` trait: `sag(r)`, `normal(p)`, `intersect(ray) -> Option<Hit>` in the local frame
- [ ] Plane, sphere (closed form, both roots, correct branch for concave/convex), conic (closed form)
- [ ] Even asphere: Newton on sag, starting from the conic solution
- [ ] Clear aperture and edge handling (miss vs vignetted)

## optobench-trace (L2)
- [ ] `Ray` (position, unit direction, wavelength in µm, power in W, optical path length)
- [ ] Vector Snell's law with total internal reflection
- [ ] Sequential trace through an ordered list of surfaces and gaps with constant indices
- [ ] Paraxial y-nu trace and cardinal points (EFL, BFL, FFL, principal planes)

## Tests (cite every reference value)
- [ ] Thin lens in air: lensmaker's equation
- [ ] Thick singlet: EFL from the thick-lens formula, to 1e-9
- [ ] Parabolic mirror focuses collimated on-axis rays to one point (residual < 1e-12 mm)
- [ ] Snell: normal incidence, Brewster geometry, critical angle and TIR
- [ ] Invariant: every traced direction stays unit length
