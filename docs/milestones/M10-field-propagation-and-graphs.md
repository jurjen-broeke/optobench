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

### M10a1 Graphs as data, files and the `graph` command

- [x] Graphs as data in `optobench-analysis::graph` (no rendering, builds for wasm32):
      line, dot and map graphs; axes with quantity, unit and scale (linear,
      logarithmic), shown in a readable unit chosen from the range (nm, µm, mm, m; mrad,
      degrees; a power of ten taken out of long numbers) with ticks at 1, 2 and 5 × 10ⁿ;
      at least 1 nm (or a millionth of the values) across, so a perfect focus is drawn
      and not a cloud of rounding
- [x] Colours (`optobench-analysis::colour`), one job each: eight series hues in a fixed
      order, never cycled (the reference data-visualisation palette, chosen for
      colour-blind separation of neighbours; scatter graphs stay clear with three);
      sequential one-hue blue for magnitude; diverging blue–grey–red for differences;
      cyclic at constant OKLab lightness for phase; light (files) and dark (interface)
      variants
- [x] Graphs from existing results: PSF along the focus line, across the peak (x and y)
      and an x–z section (`PsfSettings::section`); axial crossings by ray height; spot
      diagrams (a series per source, to scale); detector irradiance maps
- [x] Comparison: graphs on shared axes, each named (`combine`); a reference series
      (closed form, published curve) drawn dashed in neutral ink; differences of
      matching series (interpolated where the other has values) and of maps on the
      same grid (diverging scale)
- [x] `optobench-io::graph`: deterministic SVG (hairline grid, recessive axes, legend
      from two series up, colour bar beside a map, `<title>` per series for hover), CSV
      of every number in engine units, JSON that reads back bit for bit (`serde_json`
      with exact float parsing)
- [x] `graph` command: `of` a design's result, or `from` a saved graph; `compare`,
      `difference`, `reference`; written to `svg`, `csv`, `json`
- [x] Checks (commands `tests/graph.rs`, unit tests in `graph`, `colour` and
      `optobench-io::graph`): a PSF profile against the Airy pattern within 0.5 % of the
      peak; saved, read back and redrawn byte for byte; a graph minus itself is zero; a
      collar set and not set compared on one graph and as a difference (peaks 0.11 mm
      apart, as 0.33 mm × (1 − 1/n) says); maps and spot diagrams to scale; colour
      scales monotonic in lightness (sequential), grey and balanced (diverging),
      constant in lightness and closed (cyclic)

### M10a2 More results, and graphs in the interface

- [x] Graphs of a Gaussian beam's radius along its path (each free stretch from the
      beam there by the q law, the planes marked); sweeps over a DOF (centroid, power,
      RMS radius, pointing, or the chief ray at an element); the wavefront error at the
      PSF peak over the beam; the optimiser's merit by accepted step
- [x] In the interface: a Graph button (CAD, Layout and Simulate) drawing the kind chosen
      in the results strip, the last three graphs there on the dark surface; readout of
      the nearest point or the cell under the pointer; zoom about the pointer, pan,
      double-click to fit; on maps, a crosshair with the row and column profiles through
      the cell; a cursor linked across graphs whose x measures the same quantity;
      `OPTOBENCH_UI_GRAPHS` draws graphs before a screenshot
- [x] Checks (commands `tests/graph.rs`, the window test in `app.rs`): the Gaussian curve
      against w(z) and the `gaussian` radius at the screen; a galvo sweep with no pupil
      walk, landing at f (f₁/f₂) tan 2α; the merit falling at every step; the RMS
      wavefront error against the Strehl ratio by Maréchal's exp(−(2πσ)²), to 0.005 at
      σ = 0.045 waves; in the window, graphs drawn newest first and the pointer on a
      plot setting the linked cursor

## M10b Fields and propagation

- [x] `optobench-field` (L4): a sampled complex field per polarization component on a
      square grid across a plane, with wavelength, medium and a spherical reference (its
      curvature carried analytically); power Σ n |E|² dA conserved by every step
- [x] FFT: `rustfft` 6.4 (MIT OR Apache-2.0, builds for wasm32; decided 2026-11-12) behind
      `optobench_field::transform`, the only module that knows it; `cargo deny` passes.
      An own transform stays open for later performance work
- [x] Propagation: through expanding coordinates (a field of curvature c over z is its
      flat samples over z / M, M = 1 + z c, scaled by M), then the angular spectrum on
      the same grid while the grid's Fresnel number N Δ² / (λ z) is at least 1, else
      Fresnel's integral in one transform onto a grid of λ z / (N Δ) with the outgoing
      sphere as the new reference; at a focus the incoming sphere and the chirp cancel
      (a lens and its focal distance give the far field); past a focus the grid inverts.
      Fields whose phase steps by more than π/2 between samples are refused with a
      message; the report gives the method, the Fresnel number and the power at the
      grid's edge (wrap-around); `padded` widens the grid; the angular spectrum can be
      forced for periodic fields
- [x] Masks and screens: apertures (circle, annulus, rectangle, stop) with exact edges
      (a cell an edge crosses keeps its open fraction of the field); ideal thin lenses as
      a change of the reference sphere (nothing to sample); Zernike phase maps (ANSI);
      any amplitude or phase by `modulate`
- [x] A Gaussian beam as its exact field anywhere along it (radius, curvature, Gouy
      phase, carrier)
- [x] Field maps: irradiance and phase (relative to the reference sphere, blank where
      dark), cropped to the lit region
- [x] Checks (`optobench-field` `tests/propagation.rs`, unit tests): FFT against the direct
      DFT; a Gaussian beam at 50 mm, z_R and 4 z_R from its waist and through a 100 mm
      lens before, at and past its focus, against the q law, every sample to 10⁻⁶ of the
      peak, radius w(z) to 10⁻⁴; a circular aperture's on-axis irradiance
      4 I₀ sin²(π a² / (2 λ z)) at Fresnel numbers 1 to 4 to 0.2 %; its far field the Airy
      pattern (peak and seven samples out) to 0.2 %; a truncated Gaussian's transmitted
      power 1 − exp(−2a²/w²) to 0.2 % and its focal peak to 0.1 %; a grating's Talbot
      self-image at 2d²/λ and its half-period shift at half that, to 10⁻⁶; Maréchal's
      exp(−(2πσ)²) for 0.05 waves of spherical aberration to 0.01; power conserved by
      every propagation to 10⁻⁹; a field too coarse for its phase refused

## M10c Rays to fields and back

- [x] Rays to a field (`optobench_field::rays`): rays from a regular launch grid, each
      with its power over the area its launch cell covers on the plane (from its
      neighbours) and its optical path as phase; spread over the grid by triangles
      between neighbours (the eikonal field); the rays' mean sphere taken out as the
      reference; triangles folding over (a caustic) refused
- [x] A field to rays: one ray per lit cell along the unwrapped phase gradient, with the
      cell's power and an optical path from the unwrapped phase; rays by a dark cell or a
      phase jump marked rough
- [x] In a scene (`optobench_field::scene`, command `field`): the field starts as the
      source's exact field or from the rays traced to a detector, goes along that plane's
      axis to a target detector through ideal lenses (a change of the reference sphere,
      their rim an aperture) and mechanics (a plate as an aperture with exact edges, a
      tube as apertures of its bore at its ends and seven planes between), anything else
      on the axis refused; at the target, fibre coupling, and rays on through the rest
      of the scene from just past it, summarised per detector
- [x] Field detectors: the field at the target as irradiance and phase maps, and its
      difference from the rays on the target's pixels (each field cell's power shared by
      overlap area, no moiré); `graph` kinds `field`, `field-phase`, `field-difference`
- [x] Mode overlap: coupling into a single-mode fibre (Gaussian of its mode-field
      diameter), |∫ E* M dA|² / (∫ |E|² dA ∫ |M|² dA), both polarizations
- [x] Fixed on the way: an ideal lens now adds the optical path of a perfect lens
      (Fermat: a plane wave of any slope meets in its focal plane in phase) and is
      recorded twice like an objective (`Role::records_twice`); before, a field after an
      ideal lens had the wrong phase. Ray positions and slopes are unchanged
- [x] Exit check (commands `tests/field.rs`): two Gaussian modes overlap by
      (2 w₁ w₂ / (w₁² + w₂²))² to 10⁻⁷; a Gaussian focused by a 50 mm lens couples into a
      fibre at its q-law waist as that formula says, to 10⁻⁴; rays → field → rays with
      nothing in the way give the screen's RMS radius 1/√2 mm to 1 % (the trace's own
      rings to 1.5 %); a field focused by an ideal lens of NA 0.1 has the M9c2 PSF's peak
      per watt to 1 %; a Gaussian clipped by an iris (1 mm, at its waist) passes
      1 − e⁻² to 0.3 %, a lens tube's bore beyond clips nothing, and the axis at 1 m is lit
      as Fresnel's integral over the clipped beam says, to 1 %; the difference from the
      rays is drawn
- [x] Masks from STEP bodies: every absorbing body on the way is cut at nine planes
      through its depth; four lines per grid row, their crossings with the body's own
      faces (`Face::crossings` in the trace) telling inside from outside, so a cell keeps
      its open fraction exactly along x and to a quarter cell along y. A lens or mirror
      body on the way is the trace's and refused. Checks: the STEP fixtures' cylinder
      (Ø12) in a 20 mm beam passes 1 − (6/10)², the block a quadrant less (3/4), each to
      0.3 % at the first cutting plane, the later planes taking the light diffracted into
      the shadow
- [x] Tilted field planes (`Field::tilted`): a plane tilted about the field's x or y,
      every sample the angular spectrum summed exactly at its point on the tilted plane
      (each plane wave propagated to its height there; N³ operations, nothing
      interpolated), the oblique incidence carried as a tilt (direction cosines) so the
      samples stay smooth, irradiance and power with the obliquity factor; a tilted field
      is an end point. Targets tilted about the start plane's x or y. Checks: a Gaussian
      beam on planes tilted 30° about y and −20° about x equals the q-law beam at every
      point of the plane to 10⁻⁶ of the peak, the same power crosses, the footprint
      stretches by 1 / cos α; in a layout, a screen tilted 30° by its DOF takes all the
      power, footprint w(z) / cos 30° by w(z), and the rays from it reach a further screen
      as those from an untilted one (power to 10⁻³, RMS radius to 1 %)
- [x] Sampling: a field is refused when more than 2 % of its lit power sits at phase
      steps beyond π/2 between neighbours; a hard edge's diffraction (a few coarse steps
      by the edge) passes, a smooth phase too steep for the grid does not

M10's exit criterion is met: each closed form and the rays → field → rays and PSF
comparisons above. M10 Field propagation and graphs is complete.
