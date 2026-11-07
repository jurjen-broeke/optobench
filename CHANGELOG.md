# Changelog

All notable changes, newest first. File-format bumps (`optobench/<N>`) are always listed
with the release that introduced them.

## Unreleased

- M3c scene files and the `trace` command: the `.optobench` layout file (TOML; format tag
  and kind on the first two lines; strict keys naming the closest valid key; planar and
  table layouts; materials by bundled or imported name, or by index); `trace` reports
  detectors, clipping per element or proxy and the power ledger, and writes a top or
  side SVG layout view. Example scenes in `examples/`. New dependency: `toml` (MIT OR
  Apache-2.0).
- M3b scene model (`optobench-scene`): lenses (inline surfaces, thicknesses, materials,
  diameter, clear aperture, ground edge as an absorber), mirrors, collimated and point
  sources, tube and plate proxies, detectors with irradiance maps; planar layouts along a
  folded axis and table layouts on a breadboard grid; building the non-sequential system,
  launching rays, per-detector results and a clipping list per item.
- M3a non-sequential trace (`optobench_trace::nonsequential`): faces (surface profile or
  cylinder, placed and bounded by a disk, annulus, rectangle or square with hole) between
  two media; refracting (Fresnel loss, total internal reflection followed), mirror,
  absorber and detector roles; clear-aperture clipping at element edges; nearest-hit
  search; ray history and end state; power ledger over all rays.
- M2c import: commands `import-material` (a refractiveindex.info YAML file the user
  downloaded; first call shows its notice and references, `accept_licence: true` stores
  it under `<data directory>/optobench/sources/refractiveindex.info/<sha256>/`, marked
  `redistribute = false` unless asked) and `materials` (bundled and imported).
  `optobench-materials`: all nine refractiveindex.info dispersion formulas
  (`DispersionFormula`) and `Composite` (n and k from separate models).
  `optobench-io`: strict reader for the database's YAML subset, the material store, and
  SHA-256 (FIPS 180-4) for checksums. No network download (decided 2026-11-07).
- M2c licence review (`docs/licence-review.md`), decided 2026-11-07: four literature
  datasets bundled as CC0 transcriptions from the refractiveindex.info database, not yet
  verified against the original papers; manufacturer catalogues are fetch-only.
  `optobench_materials::bundled`: fused silica (Malitson), standard air (Ciddor), water
  at 19–24 °C (index Daimon & Masumura, extinction Hale & Querry) and the Hale & Querry
  table, each with its provenance. First bundled asset: the water n,k table.
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
