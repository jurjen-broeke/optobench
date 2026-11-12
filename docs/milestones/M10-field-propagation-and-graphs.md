# M10 Field propagation and graphs

Exit criterion (docs/design.md, milestone table): a circular aperture's on-axis Fresnel
diffraction, a truncated Gaussian's transmitted power and far field, the Talbot distance
and Gaussian-to-fibre coupling match their closed forms; rays → field → rays reproduces
the ray trace with no aperture in the way, and a field focused by an ideal lens the M9c2
PSF; every analysis result drawn as a graph and compared with its reference.

The design is in docs/design.md, Optical model (Field propagation) and Graphs. Methods
from J. W. Goodman, *Introduction to Fourier Optics* (3rd ed., 2005), chs. 3–5;
A. E. Siegman, *Lasers* (1986), chs. 16–20; D. G. Voelz, *Computational Fourier Optics*
(SPIE, 2011) for sampling.

Graphs come first: the field work is inspected with them.

## M10a Graphs

- [ ] Graphs as data in `optobench-analysis` (no rendering, builds for wasm32): line,
      scatter and map graphs; axes with quantity, unit and a readable scale (µm in a
      focal region, mm on a bench; linear, logarithmic, dB); series with a label and a
      palette colour that stays the same across graphs; maps with a colour scale
- [ ] Colour scales: perceptually uniform sequential (intensity), cyclic (phase, masked
      below an amplitude threshold), diverging centred on zero (differences); readable
      in grey and by colour-blind viewers
- [ ] Graphs from existing results: PSF axial and lateral profiles and an x–z section;
      focus crossings against height; a Gaussian beam's radius along its path with the
      elements marked; sweeps over a DOF; spot diagrams and footprints; detector
      irradiance maps; wavefront and OPD over the pupil; merit against iteration
- [ ] Comparison: results on shared axes, side by side on the same scales, and as
      differences (residual profiles, difference maps); a result saved as JSON compared
      with a new one; a closed form or table as a reference series
- [ ] SVG and CSV output in `optobench-io` (deterministic); `graph` command: a graph of
      a design's result, optionally against a saved result or a reference, written as
      SVG and CSV
- [ ] In the UI: graphs in the results area, hover readout, zoom and pan, a map cursor
      with its row and column profiles, linked cursors across graphs sharing an axis
- [ ] Checks: SVG and CSV of fixed results byte for byte; the CSV holds the result's
      numbers exactly; colour scales monotonic in lightness (sequential), equal in
      lightness at both ends (cyclic), symmetric about zero (diverging)

## M10b Fields and propagation

- [ ] `optobench-field` (L4): a sampled complex field per polarization component on a
      square grid across a plane, with wavelength, medium and an optional spherical
      reference (curvature carried analytically); power Σ n |E|² dA checked at every
      step
- [ ] FFT: `rustfft` (MIT OR Apache-2.0, builds for wasm32; decided 2026-11-12) behind a
      transform interface in `optobench-field`, with a row in the licence checks; an own
      transform stays open for later performance work
- [ ] Propagation: angular spectrum for short distances, Fresnel transfer or
      single-transform for long ones, chosen from the grid's Fresnel number; zero padding;
      sampling rules enforced with a clear message when the grid is too coarse; tilted
      planes by rotating the angular spectrum
- [ ] Masks and screens: apertures and mechanics cut by the plane (proxy plates and
      tubes, STEP bodies) as amplitude masks with exact edges; ideal thin lenses, the
      ideal objective's pupil and Zernike phase maps as phase screens
- [ ] A Gaussian source as its exact field
- [ ] Checks: FFT against a direct DFT; a Gaussian's w(z), R(z) and Gouy phase against the
      q law (Siegman ch. 17); a circular aperture's on-axis irradiance
      4 I₀ sin²(π a² / (2 λ z)) (Goodman §4.5); its far field the Airy pattern; a
      truncated Gaussian's transmitted power 1 − exp(−2a²/w²); a grating's Talbot
      self-image at 2d²/λ; power conserved by every propagator

## M10c Rays to fields and back

- [ ] Rays to field at a chosen plane (a `field` entry in layouts and sidecars: plane,
      grid size, spacing or padding): amplitude from each ray's power over the area it
      stands for, phase from its optical path, as in the M9c2 PSF; sampling checked
      against the phase gradient
- [ ] Field to rays: one ray per cell above a power threshold, along the local phase
      gradient, with the cell's power and optical path; rays from rough phase (near a
      dark ring) marked
- [ ] Field detectors: irradiance n |E|² and the field itself, as graphs
- [ ] Mode overlap: coupling into a single-mode fibre (Gaussian of its mode-field
      diameter) or any field, |∫ E₁* E₂ dA|² / (∫ |E₁|² dA ∫ |E₂|² dA)
- [ ] Exit check: Gaussian-to-fibre coupling (2 w₁ w₂ / (w₁² + w₂²))² for two waists
      in the same plane on one axis; rays → field → rays with no aperture reproduces the
      ray trace to the sampling error; a field focused by an ideal lens reproduces the M9c2 PSF at
      low NA; a Gaussian beam clipped by an iris in a lens tube, shown as rays and as a
      field with the difference graphed
