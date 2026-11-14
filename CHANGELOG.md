# Changelog

All notable changes, newest first. File-format bumps (`optobench/<N>`) are always listed
with the release that introduced them.

## Unreleased

- Rays in parallel (M7): the non-sequential trace runs its rays on every core
  (`[trace] threads`, default 0 for every core; 1 for the calling thread), with records
  and power ledger the same bit for bit as on one thread; wasm32 stays on one. The
  Thorlabs expander periscope traces 5.8× faster on 6 cores (0.065 s), its optimisation
  4.9×.
- M15d light by Monte Carlo: `fluence` and `heating` (and their graphs) take
  `transport = "monte-carlo"` (`packets`, `seed`, `gpu`, `threads`), run on the GPU when
  one is found, else on the CPU's threads; diffusion stays the default. The brain heating
  benchmark with Monte Carlo light gives 0.58 K per 100 mW (diffusion 0.47, measured
  1.86 ± 0.25).
- M15b tissue Monte Carlo on the GPU: new crate `optobench-gpu` runs the photon walk as a
  wgpu compute kernel (f32, xoshiro128**, deposits in exact 64-bit fixed point), with the
  CPU walk when no adapter is found. It meets the published slab benchmarks and equals
  the CPU within statistical error; about 30× one CPU core on a laptop GPU.
- M15a tissue Monte Carlo: `optobench_analysis::photon` walks weighted photon packets
  through a sample's layers (Henyey–Greenstein scattering, Fresnel at index steps, Russian
  roulette), launched from the traced rays where they enter the sample, recording
  absorbed power and fluence on the r–z grid, reflectance and transmittance. Checked
  against an absorbing slab's multiple reflections and Chandrasekhar's semi-infinite
  reflectance, and against the published slab benchmarks (van de Hulst's tables and
  Giovanelli's semi-infinite reflectance, as MCML and Prahl et al. quote them) and the
  transport equation's deep-fluence decay. The seeded generator moved to
  `optobench_math::random`.
- Mounts: `[[mount]]` tables describe a mount's adjusters once (kind, axis, `range` or
  `travel`, `source`); `mount = "<id>"` on an element, source or detector gives it those
  DOFs before the solve. DOFs take `travel` (the part placed in its middle) and
  `source`. `trace`, `optimize` and `tolerance` warn about slides that adjust 1 mm or
  less either way (`small_range_mm`).
- M14d adjustment range: merit term `headroom` keeps adjusters near the middle of their
  ranges where the optics leave a choice; `optimize` and the move report give each
  ranged DOF's adjustment room (travel used and left, the range after adjusting, a
  status, re-placement advice, warnings; `headroom_warning`); in tolerancing a
  compensator that runs out of range fails the trial, and the Monte Carlo reports each
  compensator's travel with advice.
- M14c tolerance report and graphs: `tolerance` writes `<design>.tolerance.json` and
  `.tolerance.md` with `report`; graph kinds `tolerance-histogram`,
  `tolerance-sensitivity` and `tolerance-yield`. Monte Carlo shares are now taken over
  each criterion's residual components, which keeps the ranking stable; sidecar
  tolerances can name a CAD element by its id. Exit check met on the golden periscope;
  runtimes recorded in the milestone.
- M14b Monte Carlo: new command `tolerance` (sensitivity, or `mode = "monte-carlo"` with
  `trials`, `seed`, `threads`): trials in parallel, each from its own random stream (the
  same results for any number of threads), per-criterion statistics, the yield and its
  error, and each tolerance's first-order share of each criterion's variance with the
  ranking.
- M14a tolerances: layouts and sidecars take `[[tolerance]]` (placement slides and tilts,
  DOF errors, lens radius, thickness and index, ideal-lens focal length; uniform, normal
  or end-point) and `[tolerancing]` (criteria with pass limits, compensators). New
  `optobench_optimize::tolerance`: a seeded generator (xoshiro256**), applying errors,
  evaluation with compensators re-optimised, and one-at-a-time sensitivities with RSS and
  ranking.
- M11 heating benchmark against Podgorski and Ranganathan (2016): the depth and
  field-size dependences agree; the absolute rise per 100 mW is about 4× lower than
  measured (0.47 against 1.86 ± 0.25 °C), reported in the milestone with where the gap
  may lie. The light in a sample now uses the delta-Eddington primary by default (the
  forward-scattered part kept in the primary beam), which the benchmark showed is needed
  for strongly forward-scattering tissue; `fluence` takes `primary = "ballistic"` for the
  former behaviour.
- M11d2 heating of a sample: sample layers take `thermal` properties (with their source)
  and samples `thermal_boundaries`; new command `heating` gives the temperature rise from
  the light the sample absorbs, steady or in time, parked or over a scanned field, with
  perfusion: power at the surface and the focal plane, peak and field-averaged rise, the
  heat balance, warnings; graph kinds `temperature-map` and `temperature-time`. Fixed:
  `fluence` refused a sample with no scattering layer.
- M11d1 the thermal solver: `optobench_analysis::thermal` gives the temperature rise from
  an absorbed power density, steady or in time (Crank–Nicolson, with an on/off or duty
  drive), with Pennes perfusion and fixed, insulated or heat-transfer boundaries, on a
  graded grid of rings; checked against the Gaussian source, its time course, the image
  method and the Pennes kernel.
- M11c emitters through the excited volume: `emission` with `excited_by`, `focus` and
  `fluorophore` spreads the emitter over a grid around a laser's focus weighted by I²,
  its photons those of the two-photon excitation, and adds up the budgets. The PSF can
  give intensities on a grid around its peak (`cloud`).
- M11b3 FPbase: new command `import-fluorophore` reads a fluorescent protein from the
  JSON the user downloaded from FPbase's API (spectra, extinction coefficient, quantum
  yield, lifetime, primary reference), shows FPbase's terms first, stores the files with
  their provenance and gives (or appends to a design) a `[[fluorophore]]` entry carrying
  FPbase's citation and the protein's DOI. The API has no two-photon data: a two-photon
  excitation spectrum exported from FPbase's website is imported from its own file
  (`two_photon`, the protein's `2P` column of FPbase's CSV export; relative with a
  cited peak, applied at the wavelength it was quoted at, or in GM). Fluorophores take
  `two_photon_spectrum` (relative) shaped by a peak cross-section. Licence review entry
  for FPbase.
- M11b2 the excitation volume: new command `excitation` gives a fluorophore's one- and
  two-photon excitation at a laser's focus (rates per molecule at the peak, saturation
  and per-pulse checks, two-photon excitations in the focus and photons emitted,
  one-photon excitations per depth, the pulse width at the focus). New graph kinds
  `excitation-depth`, `excitation-map` and `pulse-width`. The PSF can attenuate each
  plane wave by its ray's ballistic loss (`attenuated`), reports that transmission and
  the widths of I², and integrates I and I² over planes and the whole depth (`volume`);
  checked against Xu and Webb's (1996) thick-sample closed forms.
- M11b1 fluorophores, lasers and excitation rates: layouts and sidecars take
  `[[fluorophore]]` entries (quantum yield, lifetime, extinction coefficient with a
  relative excitation spectrum or a one-photon cross-section, two-photon cross-section
  in GM or as an action cross-section η σ₂, emission spectrum, concentrations in sample layers; a `source` is required).
  Sources take `pulse = { repetition_rate_mhz, width_fs, shape, prechirp_fs2 }`;
  objectives take `group_delay_dispersion_fs2`. New `optobench_materials::dispersion`
  (group index and group-velocity dispersion from a material's own formula; tables are
  refused) and `optobench_analysis::excitation` (one- and two-photon rates, pulse
  factors, Gaussian pulse broadening, the dispersion along a pulsed source's chief ray).
  Ray records gain `media`.
- M11c emission and photon budget: new command `emission` traces an emitter in a sample
  on its own and gives its photon budget in watts and photons per second: emitted,
  collected by the objective, lost in each part of the sample, at each element's faces
  (Fresnel, filter rejection) and clipped by each item, arriving at and counted by each
  detector, and where the rest ended; it closes. New source kind `isotropic` (equal
  solid angles into a cone of up to 180°; `power_w` is the total over all directions);
  layout sources may be `reversed`; detectors take an optional `quantum_efficiency`
  (number or table, with its source). An ideal objective now also takes light back from
  the sample through its front plane within its NA (sine condition reversed), where it
  used to clip it; a source inside a sample layer starts in that layer's medium. Ray
  records gain `face_losses`.
- M11a2 diffuse light: new command `fluence` gives the light in a sample, the direct beam
  from the traced rays and the diffuse light it scatters (diffusion approximation on rings
  about the axis, partial-current faces from the index step), with the absorbed power by
  layer and its density; new `graph` kinds `fluence-depth` and `absorption` (an x–z map on
  a log scale). Map colours are now computed once for files and the window alike.
- M11a1 turbid sample layers: a sample layer may give absorption µa, scattering µs and
  anisotropy g (numbers or spectra, with their source); the direct beam loses power at
  µa + µs, and the trace reports what it left in each part of each sample (absorbed,
  scattered); the power ledger gains `scattered_w`. The bundled water absorption is
  verified against Hale & Querry's Table I.
- M10c rays to fields and back: new command `field` carries a beam as a field from the
  source (its exact field) or a detector (from the traced rays) to a target detector,
  through ideal lenses and mechanics (plates, tube bores), with fibre coupling at the
  target and rays on from it through the rest of the scene. New `graph` kinds `field`,
  `field-phase` and `field-difference` (field minus rays on the target's pixels). Fixed:
  an ideal lens now adds the optical path of a perfect lens (rays unchanged); it is
  recorded twice in ray records, like an objective. M10's exit criterion is met.
- M10c (completed): absorbing bodies from CAD cut the field at nine planes through their
  depth, by counting crossings with their own faces; a lens or mirror body on the way is
  refused. Fields onto planes tilted about x or y, exactly (no interpolation), with the
  oblique incidence carried as a tilt and the obliquity in irradiance and power; `field`
  targets may be tilted. The sampling check now looks at the share of the lit field at
  coarse phase steps, so a hard edge's diffraction passes. M10 Field propagation and
  graphs is complete.
- M10b fields and propagation: new engine crate `optobench-field` (L4). Sampled fields
  with a spherical reference; propagation by the angular spectrum or Fresnel's integral
  in one transform, chosen from the grid's Fresnel number, through foci and past them;
  apertures with exact edges, thin lenses, Zernike phase maps; exact Gaussian beams;
  irradiance and phase maps. Checked against the q law, Fresnel's on-axis formula for a
  circular aperture, the Airy pattern, a truncated Gaussian, the Talbot effect and
  Maréchal's approximation. New dependency `rustfft` (MIT OR Apache-2.0), behind the
  crate's own transform interface.
- M10a2 more graphs, and graphs in the window: `graph` draws a Gaussian beam along its
  path, sweeps over a DOF, the wavefront error at the PSF peak and the optimiser's
  progress (`optimize` now returns its `history`). The window's Graph button (CAD,
  Layout, Simulate) draws the kind chosen in the results strip, with readouts, zoom, pan,
  map cursors with row and column profiles, and cursors linked across graphs. The
  `graph` command's saved-graph parameter is `saved`.
- M10a1 graphs: any result as a graph, built as data in the engine
  (`optobench_analysis::graph`, colours in `optobench_analysis::colour`) and written by
  the new command `graph` as SVG, CSV and JSON: PSF along the focus line, across the peak
  and as an x–z section, axial crossings by ray height, spot diagrams and irradiance
  maps. Graphs compare with saved graphs on shared axes, with a reference curve, or as
  a difference. Units are chosen for reading (nm, µm, mm; powers of ten taken out),
  colours have one job each (series, magnitude, difference, phase) and keep neighbours
  apart for colour-blind readers. Saved JSON now reads back bit for bit (`serde_json`
  exact float parsing).
- Roadmap: new milestone M10 Field propagation and graphs, before the sample. Fields are
  propagated between chosen planes where diffraction matters (apertures, pinholes, fibre
  coupling): rays are turned into a sampled field, propagated, and turned back into rays.
  Every result can be drawn as a graph and compared with theory, another design or an
  earlier run. The sample, fluorescence and heating milestone and those after it are now
  M11–M15; roadmap figure redrawn. docs/milestones/M10-field-propagation-and-graphs.md.
- M9c2 vectorial PSF and the exit check: new command `psf` gives the Richards–Wolf
  vectorial PSF behind an objective through the layers in front of it (stratified media),
  fed by the traced pupil field: peak irradiance, Strehl ratio, widths, axial and lateral
  profiles, for x, y, circular, radial or azimuthal pupil polarization. New DOF kind
  `collar`: an objective's correction collar, the change in the cover-glass thickness it
  corrects for. The focal shift into water matches the depth-dependent scaling of Loginov
  et al. (2024) to within 1 % for a dry NA 0.85 and an oil NA 1.4 objective. M9
  Microscopy is complete.
- M9c1 objectives in CAD and Gaussian beams: sidecars place `objective` and `sample`
  elements (an objective may name its CAD bodies, which the ideal model replaces); new
  command `gaussian` gives a Gaussian source's radius, curvature and waist at every
  element and detector, and an objective's pupil fill, from the traced system's ABCD and
  the q law. Fixed: ray records after an objective attributed points to the wrong faces;
  records now carry the optical path at every point.
- M9b scanning and relays: layout element `ideal_lens` (a perfect thin lens); new command
  `sweep` traces over a DOF's values and reports per value the detectors and each
  source's chief ray element by element (pupil walk, scan slopes, landing point and
  angle). The 4f galvo relay into an ideal objective matches the hand calculation to
  1e-9.
- M9a ideal objective and immersion: layout elements `objective` (focal length, NA,
  pupil distance, working distance, design immersion, cover glass and sample medium; the
  Abbe sine condition, rays stopped beyond f · NA) and `sample` (flat layers with the
  medium above and below). Design rays are traced backwards from the design focus and
  forwards through the layers actually present, so focal shift, spherical aberration and
  loss of aperture from index mismatch come from the trace; optical path is set for the
  PSF. New command `focus`: axial crossings by ray height, paraxial, marginal and best
  focus. Checked per ray for matched, dry-into-water, oil-into-water, dipping and
  well-plate cases.
- Fixed: on strongly curved exact faces (a torus patch 180° round its tube) the inside
  test accepted points across the surface from the face, so a lone curved face could
  block rays it should not. Such points are now decided in the face's parameter plane
  (the tessellation's boundary regions, with a grid for speed). The corpus assemblies
  trace exactly as before, within 3 % of the time.
- M8c3 icons and exit test: optobench's own icon set in SolidCraft's icon language
  (40 icons, drawn with egui's painter; an optics `beam` role), on every ribbon button and
  workspace tile; DOF buttons name their frame (a layout item's own axes, a sidecar's
  scene axes); the M8 exit test drives the window with `egui_kittest`: a layout built,
  traced, optimised and saved, and the golden periscope tagged, given DOFs, optimised with
  its move report and realigned, from the window alone. M8 Interface is complete.
- M8c2 editors: the interface edits designs through `edit`: every key of the selected
  entry, its degrees of freedom, New and Undo, adding lenses, mirrors, sources and
  detectors to a layout, the Simulate stages' editors and a runs strip, tagging CAD bodies
  as lenses or (by the face clicked) mirrors, applying optimised values, and importing
  files dropped on the window. `view` also returns the file's entries (`design`) and each
  CAD body's axes.
- M8c1 editing: new command `edit` changes a layout or sidecar in place and keeps its
  comments (`optobench_io::edit`, on `toml_edit`): set or unset keys of entries by id
  (merit terms by index, degrees of freedom by name), add and remove entries and DOFs,
  replace the whole text (undo), or start a new planar or table layout. An edit is
  written only when the design still reads, and returns the previous text.
- M8b shaded 3D view: the interface draws the scene shaded on wgpu (depth, 4× multisampling,
  edges and rays as lines, lenses as translucent glass), picks and highlights items under
  the pointer, switches between perspective and orthographic, and cuts the scene with a
  section plane across x, y or z. Exact faces are tessellated for display
  (`BrepFace::tessellate`, `optobench_io::mesh`; constrained Delaunay triangulation from
  the `spade` crate); `view` with `mesh` returns the triangles. The tessellated STEP
  fixtures enclose their volumes to 0.5 %.
- M8a interface frame: a desktop app, `optobench-ui [design.optobench] [--data-directory
  <dir>]` (egui 0.37, eframe, wgpu), in SolidCraft's dark theme with three workspaces:
  CAD and Layout as ribbons of tabs and sections, Simulate as stages with Trace as the
  primary button. Every button runs a command of the registry in the background; a
  browser (sources, optics, detectors, DOFs, mechanics), a view with the scene's
  wireframe and the traced rays (orbit, pan, zoom, pick, view gizmo), properties and a
  results strip (messages, power ledger, detectors). Commands now carry a label, an icon
  name and whether they write files (`commands` lists them). New command `view`: the
  scene's outlines for drawing (`optobench_io::wireframe`). `trace` takes `ray_paths` and
  returns the paths of that many rays. A missing material now names the data directory
  that was searched.
- M7a performance: the inside test of trimmed faces (91 % of trace time) now uses a
  bounding-volume tree of boundary segments, and on flat faces a row grid with crossing
  counts (the exact test still decides next to edges); faces skip surface hits beyond the
  nearest hit found so far. The Thorlabs beam-expander periscope traces in 0.40 s instead
  of 3.4 s and optimises in 50 s instead of about 25 minutes, with unchanged results.
  `examples/trace_timing` times a design. Design decisions for the interface recorded.
- Roadmap: two milestones after M6, M7 Performance (converted-face cache, parallel rays,
  faster B-spline seeding, fewer traces per optimiser step; exit: the Thorlabs
  beam-expander periscope optimised in under 2 minutes) and M8 Interface (a stand-alone
  desktop app in SolidCraft's style with CAD, LAYOUT and SIMULATE workspaces). The
  microscopy pack and later milestones are now M9–M14; roadmap figure redrawn.
- M6c move report and re-export diff: `optimize` with `report` writes
  `<design>.moves.json` and `.moves.md` (per DOF the current and suggested value; per
  moved part the move from its STEP pose as translation, axis and angle, 3 × 4 matrix and
  SolidCraft `occurrence.move` command; for layouts the DOF values to set). The new
  command `check-moves` compares every body of a re-exported assembly with the report and
  gives the merit as it is now. Golden periscope fixtures (`periscope*.step`: two 45°
  mirrors and a 2× Galilean expander, aligned, misaligned and realigned by the report's
  moves): the M6 exit criterion holds. `Scene::movement`, `Scene::dof_targets`.
- M6b optimiser: damped least squares (Levenberg–Marquardt with bounds,
  `optobench_optimize::lm`) over a scene's free DOFs; merit terms focus, centring,
  pointing, collimation and power (`[[merit]]` in layout and sidecar files, or the
  command's `merit`); the `optimize` command returns each DOF's start and suggested value
  and every term before and after. Detectors report the direct beam's mean slope and RMS
  slope spread (`direct_mean_slope_mrad`, `direct_rms_slope_mrad`). Fixed: a scene's poses
  now always match its DOF values (`Scene::placed` at reading, `moved` by the change), so
  moving twice no longer adds the offsets twice.
- M6a degrees of freedom (`optobench_scene::dof`): `dof = [...]` on elements, sources and
  detectors in layout and sidecar files: slides and tilts about a pivot, with ranges,
  `locked`, and coupled DOFs (`follows`, `ratio`). In a sidecar a DOF moves the element's
  whole CAD occurrence. `trace` takes `dofs` values (mm, degrees) and reports them. The
  closest-key hint counts swapped letters as one edit (`rnage` → `range`).
- M5c3 embedded parameters: the `embed` command writes a `[resolved]` section at the end
  of a layout or sidecar with the library parts, R/T curves and imported materials the
  trace used, each with its file's SHA-256 (`optobench_io::resolved`). Readers use
  embedded values first, so a shared design traces as saved without the library; `trace`
  reports how each compares with the user's data (`embedded`). Values from sources marked
  `redistribute = false` are listed, not embedded, unless `include_restricted`. Curve
  units are converted by division, so round values stay round. M5 is complete.
- `import-curve` imports a vendor's filter set from a ZIP archive of tables, one curve
  per table named after its file (`name` becomes an optional prefix), and reads filter
  spectra as text with a described format (`Data format: Wavelength (nm) <tab>
  Transmission (0 to 1)`, as Semrock publishes them). The record keeps the angle of
  incidence the curve is for (`AOI: 45`) and the set it came from; note lines are kept
  whole. `name` is optional for single files too (named after the file). Checked on
  Semrock FF03-525/50 and the LF488/561-2X2M-C set (two exciters, two emitters, the
  Di03-R488/561 dichroic at 45°).
- `import-curve` reads spreadsheets (`.xlsx`) as vendors publish measured curves, with
  our own ZIP reader (APPNOTE), Deflate (RFC 1951), CRC-32 and a small XML reader for the
  sheet cells (`optobench_io::xlsx`); no new dependencies. Columns and units now come from
  a header row naming the wavelength (`Wavelength (nm) | Optical Density |
  % Transmission`), for delimited text too, which also reads quoted cells. New parameter
  `sheet`; the record keeps the header and the vendor's notes (description, disclaimer,
  citation request). Checked on Thorlabs' MVF52A data: CWL 525.5 nm, FWHM 39 nm.
- M5c2 coatings in files: `coating` is `"uncoated"`, `"ideal"`, `{ curve = "<name>" }`
  or `{ layers = [...] }` (thickness in nm or a quarter wave at a wavelength, listed from
  the air side), and `coatings` gives one per surface, in part, layout and sidecar files.
  On a filter part, and on the new sidecar element kind `filter`, a curve is the whole
  filter's measured response: it goes on one face and the others are ideal, so it counts
  once. Library parts' coatings land on the STEP faces that are each surface. New
  commands `import-curve` (vendor tables as CSV: nm or µm, percent or fraction, R = 1 − T
  or 0 without a reflectance column, noise clipped and reported) and `curves`. The scene
  model takes per-surface coatings (`ElementKind::Lens::coatings`,
  `SolidRole::Lens::face_coatings`).
- M5c1 parts library: part files (`kind = "part"`: prescription, glasses, outline, spec
  sheet source) in `<user data directory>/optobench/library/parts/<vendor>/`; commands
  `import-part` and `parts`. Layout elements take `library = "<vendor>/<part>"` (with
  `reversed`); sidecar lens elements take `library` and pick the part's glass bodies by
  shape, leaving its mount to absorb. STEP products named after a part are tagged
  automatically (`[library] automatic = false` to switch off). Radii, centre thicknesses
  and diameters read from STEP are checked against the part and differences above
  0.05 mm reported in the trace report (`parts`). `optobench-library` gets its first
  content: `Part`, `Library`, `profile`, `compare`.
- M5b sources and detectors: Gaussian beams (1/e² waist diameter, M²; the ray bundle's
  second-moment width follows w(z) = w₀√(1 + (z/z_R)²) exactly), fiber ends (core
  diameter, NA) and Lambertian LEDs (square emitter), each with an optional `spectrum` of
  weighted lines sharing the power; source kinds `gaussian`, `fiber`, `led` in layout and
  sidecar files. Detectors report the power by angle from their normal in 5° bins
  (`angular_power_w` in the trace report).
- M5a power physics: ray splitting at partly reflecting faces down to a cutoff (default
  1e-6 of the source power, `[trace]` in layout and sidecar files), with the power
  ledger gaining "below cutoff"; coatings as thin-film stacks (characteristic matrices,
  absorbing layers) and measured R/T curves (`optobench_trace::coating`); footprints of
  the direct beam on every optical element; detectors report the direct image beside the
  total. `optobench_math::Complex`. Splitting is on by default, so reports change: total
  detector power now includes ghosts.
- M4d: golden cage relay from SolidCraft primitives with its sidecar and expected report
  (committed), re-export test, and the `export-placements` command (poses and a sidecar
  skeleton from a layout). The geometry fingerprint now includes the body's size. M4 is
  complete.
- SVG layout view of CAD scenes: one outline (convex hull) per optical element and per
  component occurrence instead of every face edge; optical elements labelled. The golden
  Thorlabs view went from 16 MB to 33 KB. Exact silhouettes and STEP performance are on
  the roadmap (design.md, Improvements).
- M4c sidecar v1 (`optobench_io::sidecar`): lens and mirror elements on STEP bodies
  matched by product, occurrence path or geometry fingerprint; sources and detectors in
  the STEP frame or relative to a body; untagged bodies absorb. `trace` takes sidecars;
  new command `bodies`. Non-sequential trace: coincident faces of touching solids
  (cemented doublets) refract once, glass to glass. Scene model: `Solid` with absorber,
  lens and mirror roles. Material import accepts the database's `PROPERTIES` block. SVG
  view thins solid outlines.
- M4b exact geometry: `optobench_surfaces::brep` (trimmed B-rep faces on analytic,
  B-spline, revolution and extrusion surfaces; closed-form, bracketed or Newton ray
  intersection with the residual of every hit; 3D trimming by the nearest boundary
  segment), `optobench_trace::bvh` and its use in the non-sequential trace (`Shape::Brep`,
  `build_index`), and `optobench_io::step::faces` (STEP bodies to placed exact faces,
  seams left out of the boundary). Golden assembly: worst residual 3.7e-10 mm over 20 000
  rays.
- M4a STEP reader (`optobench_io::step`), our own: ISO 10303-21 exchange structure
  (simple and complex entities, typed parameters, encoded strings); AP203/AP214/AP242
  products, assembly tree and placements; length and angle units per context to mm and
  radians; B-rep solids and surface models with plane, cylinder, cone, sphere, torus,
  B-spline (rational too), surfaces of revolution and linear extrusion, and line,
  circle, ellipse and B-spline edges; unsupported entities counted by type. STEP test
  fixtures made by SolidCraft from committed scripts.
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
