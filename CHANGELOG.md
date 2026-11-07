# Changelog

All notable changes, newest first. File-format bumps (`optobench/<N>`) are always listed
with the release that introduced them.

## Unreleased

- M2b analyses (`optobench-analysis`): aperture stop, paraxial entrance and exit pupils,
  fields as angles (object at infinity), paraxial or real ray aiming; spot diagram
  (hexapolar sampling, centroid, RMS and geometric radius); tangential and sagittal ray
  fans; OPD against a reference sphere on the chief ray's image point; Zernike fit in
  ANSI/OSA order with unit-RMS normalisation. Supporting changes: Householder QR least
  squares in `optobench-math`, `trace_ignoring_apertures`, and a 1e-9 mm edge tolerance
  on clear apertures.
- M2a dispersion models and power on rays:
  - `optobench-materials`: `Material` trait (n, k, absorption coefficient; errors outside
    the valid range), models Constant, Sellmeier, Schott, Cauchy and Tabulated n,k.
  - `optobench-trace`: Fresnel s/p amplitude and power coefficients; media are
    materials evaluated at each ray's wavelength; uncoated surfaces lose the Fresnel
    reflection, media absorb by Beer–Lambert, and every ray carries a power ledger.
  - API change: `object_index`/`index_after` become `object_medium`/`medium_after`
    (a plain `f64` still works), and paraxial analysis takes a wavelength.
  - Literature citations now name the chapter, not the section.
- M1 sequential core:
  - `optobench-math`: `Rotation` (axis-angle), rigid `Transform`, `Frame`, degree helpers
    for the boundary, Newton and bracket-safeguarded Newton root finding.
  - `optobench-surfaces`: `Surface` trait (sag, slope, normal, intersection); plane,
    sphere, conic (closed form, vertex-sheet root) and even asphere (Newton from the base
    conic); circular clear aperture with hit / vignetted / miss.
  - `optobench-trace`: `Ray`; vector Snell's law with total internal reflection;
    sequential real-ray trace with optical path length; paraxial y-nu trace and cardinal
    points (EFL, BFL, FFL, principal planes), mirrors by the signed-index convention.
  - Reference tests: lensmaker, thick lens to 1e-9, parabola focus < 1e-12 mm, Snell,
    Brewster, critical angle, Lagrange invariant, equal optical path to a perfect focus.
- M0: workspace with layered crates, `optobench version`, repository checks
  (`cargo xtask check`: layers, naming deny-list, bundled assets), CI, design note.
- File format: `optobench/1` (defined, not yet written by any command).
- M0: command registry (`optobench-commands`): JSON parameters in, JSON result out, strict
  parameter keys, JSON errors with stable codes. Commands `version` and `commands`; the
  CLI is `optobench <command> [parameters]`. First third-party dependency: `serde_json`.
- CI: checks on GitHub Actions; `cargo xtask naming` also checks command ids.
