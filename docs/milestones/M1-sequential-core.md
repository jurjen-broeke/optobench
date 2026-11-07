# M1 Sequential core

Exit criteria: thin/thick lens and parabola cases pass; EFL matches theory to 1e-9.

Status: done. Reference tests in `crates/optobench-trace/tests/sequential_reference.rs`.

## optobench-math (L0)
- [x] `Frame` (origin + orthonormal basis) and rigid `Transform` (rotation + translation), with inverse and composition
- [x] Rotation from axis-angle; conversion helpers for degrees at the boundary only
- [x] Scalar root finding: safeguarded Newton with bisection fallback, with iteration and tolerance reporting

## optobench-surfaces (L1)
- [x] `Surface` trait: `sag(r)`, `normal(p)`, `intersect(ray) -> Option<Hit>` in the local frame
- [x] Plane, sphere (closed form, both roots, correct branch for concave/convex), conic (closed form)
- [x] Even asphere: Newton on sag, starting from the conic solution
- [x] Clear aperture and edge handling (miss vs vignetted)

## optobench-trace (L2)
- [x] `Ray` (position, unit direction, wavelength in µm, power in W, optical path length)
- [x] Vector Snell's law with total internal reflection
- [x] Sequential trace through an ordered list of surfaces and gaps with constant indices
- [x] Paraxial y-nu trace and cardinal points (EFL, BFL, FFL, principal planes)

## Tests (cite every reference value)
- [x] Thin lens in air: lensmaker's equation
- [x] Thick singlet: EFL from the thick-lens formula, to 1e-9
- [x] Parabolic mirror focuses collimated on-axis rays to one point (residual < 1e-12 mm)
- [x] Snell: normal incidence, Brewster geometry, critical angle and TIR
- [x] Invariant: every traced direction stays unit length
