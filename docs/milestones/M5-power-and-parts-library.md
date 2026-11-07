# M5 Power and parts library

Exit criteria: power conserved to 1e-9; a library part is tagged automatically on STEP
import; a shared design traces identically on a machine without the library.

Worked in three parts, one pull request each.

Decided 2026-11-07: the parts library lives in the user's data directory
(`optobench/library/`), built and moved between installations by the user; the project
hosts none.

## M5a Power physics

On by default in layout and sidecar files (`[trace] split = false` or `split_cutoff`
change it). Detectors report the direct image (rays' main paths) beside the total, so
ghosts show; footprints use the direct beam. The golden Thorlabs relay: 217 launched rays
become 6684, 28.8 % of the power ends in the mounts as stray light, the direct image is
unchanged; tracing takes 10.7 s (see design.md, Improvements: performance).
- [x] Ray splitting at partly reflecting faces: the reflected part becomes a child ray, down to a cutoff (default 1e-6 of the source power); power below it is booked as such
- [x] Power ledger: launched = detected + clipped + absorbed + escaped + stopped + below cutoff, to 1e-9
- [x] Coatings: thin-film stacks by the characteristic-matrix method (s, p; absorbing layers), and tabulated R/T curves against wavelength for filters, dichroics and beamsplitters
- [x] Footprints: where the beam lands on each optical element, and how much of the clear aperture it fills

## M5b Sources and detectors
- [x] Gaussian beam (waist, M²), fiber (core diameter, NA), Lambertian LED (emitter size), each with power and a spectrum
- [x] Detectors: angular distribution in addition to the irradiance map

## M5c Parts library

Worked in three steps, one pull request each: c1 parts and tagging, c2 R/T curves and
coatings in files, c3 embedded parameters.

Vendor STEP product names are internal numbers (`20527-E0W`), but SolidCraft names a
component after its file, so a part is recognised by product name: its part number or
one of the `products` it lists. Fingerprints do not survive SolidCraft (it merges faces).
Within the product, the glass bodies are the ones whose exact faces have the part's radii
and thicknesses. On the local golden Thorlabs assembly, tagging its three lenses
automatically traces exactly like the hand-written sidecar.
- [x] c1: Part format (`kind = "part"`), library in the user data directory, `import-part` and `parts`
- [x] c1: `library` on layout and sidecar elements
- [x] c1: Automatic tagging of library parts on STEP import; radius check against the STEP geometry (> 0.05 mm reported)
- [ ] c2: Import of R/T curves; coatings (curves, thin-film stacks) in part, layout and sidecar files, for filters, dichroics and beamsplitters
- [ ] c3: Embedded parameters with provenance on save, so a shared design traces without the library

## Local test parts (corpus)
Beyond the M4 cage parts: DMLP730B longpass dichroic, CP20D cage iris, CFW6 filter wheel,
MVF52A 525/40 bandpass filter, FBH488-10 cleanup filter (all read completely by the STEP
reader).
