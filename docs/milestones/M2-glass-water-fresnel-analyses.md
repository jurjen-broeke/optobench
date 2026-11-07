# M2 Glass, water, Fresnel, analyses

Exit criteria: Cooke triplet and doublet match literature; water index and absorption
match the cited sources; licence review recorded.

Worked in three parts, one pull request each. M2a and M2b need no outside data; M2c is
where datasets and literature values come in, and every number there is cited.

## M2a Dispersion models and power on rays
### optobench-materials (L1)
- [ ] `Material` trait: refractive index and extinction coefficient at a wavelength (µm), valid range, errors outside it
- [ ] Models: constant, Sellmeier, Schott, Cauchy, tabulated n and k (linear interpolation)
- [ ] Bulk absorption coefficient from k (Beer–Lambert)
### optobench-trace (L2)
- [ ] Fresnel s/p amplitude and power coefficients, with total internal reflection
- [ ] Media in the sequential trace come from materials, evaluated at each ray's wavelength; paraxial analysis at a chosen wavelength
- [ ] Power on rays: Fresnel transmission at uncoated refracting surfaces (unpolarised until Jones vectors arrive), bulk absorption along each segment, ideal mirrors
- [ ] Power ledger per ray: carried + reflected + absorbed = launched, to 1e-12 relative

## M2b Analyses (optobench-analysis, L4)
- [ ] Aperture stop, entrance pupil and field definition; ray aiming for on- and off-axis fields
- [ ] Spot diagram: pupil sampling, centroid, RMS and geometric radius
- [ ] Ray fan: transverse aberration against pupil coordinate, tangential and sagittal
- [ ] OPD against a reference sphere centred on the chief ray's image point
- [ ] Zernike fit of the OPD over the unit pupil (indexing scheme documented)

## M2c Data, water, fetch, literature cases
- [ ] Licence review of candidate open datasets (glass, refractive index, water), recorded in `docs/licence-review.md` for a decision
- [ ] Formula-defined materials that pass the review (fused silica, air), with cited coefficients
- [ ] Water: dispersion, absorption, temperature, from the sources the review admits
- [ ] `optobench fetch <source>` and file import into the user data directory, with the licence shown first
- [ ] Literature cases: a Cooke triplet and an achromatic doublet against published paraxial and aberration values
