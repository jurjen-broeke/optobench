# M11 Sample, fluorescence and heating

Exit criterion (docs/design.md, milestone table): excitation rates match closed-form
cases; temperature rise matches the analytic Gaussian-source solution and a published
multiphoton heating benchmark.

The model is in docs/design.md, Samples, fluorescence and heating. It builds on M9 (the
objective, the sample's layers, the vectorial PSF at the actual depth) and M10 (graphs,
fields). Every value a user gives (optical and thermal properties of tissue, fluorophore
data) carries its provenance; nothing about tissue is bundled. Scattering is treated in
the diffusion approximation here; photon transport by Monte Carlo replaces it in M15.

## M11a Sample media and the beam in them

### M11a1 Turbid layers and the direct beam

- [x] Sample layers gain absorption µa (in place of the material's own), scattering µs
      (1/mm) and anisotropy g, each a number or a table over wavelength, with `source`
      required (`optobench_materials::Turbid`, `Spectrum`); every material can scatter
      (`Material::scattering_coefficient`, `anisotropy`, zero by default); a layer of
      `water` takes its absorption from the bundled Hale & Querry table
- [x] Water's absorption verified: the bundled table equals Table I of Hale & Querry,
      Appl. Opt. 12, 555 (1973), in all 56 rows from 0.2 to 3.35 µm and at spot checks to
      200 µm (`bundled.rs` tests); marked verified in the asset manifest and ATTRIBUTION.md
- [ ] Water's index against Daimon & Masumura 2007, once that paper is available (M2c's
      item stays open for it)
- [x] The direct beam: rays lose power at µa + µs, the loss shared in proportion; the
      ledger gains `scattered` (handed to the diffuse light, not followed as rays) and
      still closes; each ray records what it left in each medium (`Deposit`), and the
      trace reports it per part of each sample (`above`, `layer 1`, …, `below`)
- [x] Checks (commands `tests/sample.rs`): a two-layer turbid stack against Beer–Lambert
      and its shares µa/µt and µs/µt, layer by layer, to 10⁻¹²; water at 920 nm against
      µa from Table I; µs given as a spectrum, interpolated; layers without a source or
      with values out of range refused

### M11a2 Diffuse light

- [x] A finite-volume solver on rings about an axis (`optobench_analysis::diffusion`):
      −∇·(D ∇Φ) + µa Φ = S, D = 1/(3 (µa + µs′)), harmonic D between cells, conjugate
      gradients; the side dark, top and bottom dark or a partial-current face with
      A = (1 + R_eff)/(1 − R_eff) from the index step (Groenhuis et al. 1983); power
      absorbed and leaving each face from the same terms, so the balance closes. Built to
      be reused by M11d's heat solver
- [x] In a sample (`optobench_analysis::sample`, command `fluence`): every ray segment in
      the turbid layers walked in short pieces; the direct fluence rate by track length;
      the power scattered along each piece as the diffuse light's source (first
      scattering taken as isotropic, g only in D); the absorbed power density
      µa (Φ_direct + Φ_diffuse); totals per layer, the diffuse light's balance, the peak.
      The turbid layers must touch (a clear layer between them is refused); their stack's
      faces lose light by their index step to the media beyond
- [x] Graphs (`graph` kinds `fluence-depth`, `absorption`): direct, diffuse and total
      fluence rate along the axis; the absorbed power density as an x–z map on a
      logarithmic colour scale (maps now share one colour mapping, `Map::level`, between
      files and the interface, log scales included)
- [x] Checks: a point source in an infinite medium against Φ = P exp(−µ_eff r)/(4π D r)
      to 2 % along the axis; under a matched surface against the extrapolated-boundary
      image solution (T. J. Farrell, M. S. Patterson and B. Wilson, Med. Phys. 19, 879
      (1992)) to 3 %; A ≈ 1 matched and ≈ 3.25 for n = 1.4; power balance to 10⁻⁹; in a
      traced slab, the scattered and directly absorbed power on the grid equal the
      trace's own accounts to 10⁻⁹ and the diffuse light's absorbed and escaping power
      add up to the scattered power to 10⁻⁹
- [x] A collimated beam on a half-space against a published closed form: W. M. Star,
      "Diffusion theory of light transport", ch. 6 of A. J. Welch and M. J. C. van Gemert,
      Optical-Thermal Response of Laser-Irradiated Tissue, 2nd ed. (Springer, 2011)
      (local corpus), Eqs. (6.72), (6.73), (6.77), (6.82a, b), boundary F₊(0) = r₂₁ F₋(0),
      which is our partial-current face with A = (1 + r₂₁)/(1 − r₂₁) (his h = 2D(1 + r₂₁)/
      (1 − r₂₁), Eq. (6.83)). Taken with g = 0, where his source terms and our isotropic
      first scattering coincide; matched and n = 1.4, to 1 % on the axis
      (`diffusion::tests::a_collimated_beam_on_a_half_space`)

## M11b Excitation

### M11b1 Fluorophores, lasers and the rates

- [x] Fluorophores (user data, with provenance; `[[fluorophore]]` in layouts and
      sidecars, `optobench_scene::Fluorophore`): emission spectrum, one-photon
      cross-section (from the extinction coefficient at the peak shaped by a relative
      excitation spectrum, σ₁ = 1000 ln 10 ε / N_A, or given directly in cm²), two-photon
      cross-section spectrum (GM), quantum yield, lifetime; placed in sample layers with a
      concentration (`concentrations = [{ sample, layer, micromolar }]`)
- [x] Lasers: continuous, or pulsed (`pulse = { repetition_rate_mhz, width_fs, shape,
      prechirp_fs2 }`, sech² or Gaussian; `power_w` the mean)
- [x] One-photon excitation rate per molecule σ₁ I / (h ν)
      (`optobench_analysis::excitation`); the saturation check (σ₁ I τ / (h ν) ≪ 1)
      reported with the excitation volume (M11b2)
- [x] Two-photon excitation: rate ½ σ₂ ⟨F²⟩, ⟨F²⟩ = g_p ⟨F⟩² / (f τ) for pulses, g_p
      computed from the shapes (√(2 ln 2 / π) = 0.664 Gaussian, 2 ln(1 + √2) / 3 = 0.588
      sech², Xu and Webb 1996, Eqs. (13), (14); the ½ for two photons per excitation is
      their Eq. (3)); per-pulse probability
      rate / f reported with the volume (M11b2). Two-photon spectra may be given as
      action cross-sections η σ₂ (`two_photon_action_cross_section_gm`, divided by the
      quantum yield), the form most are published in (Xu et al. 1996, PNAS, below)
- [x] Pulse broadening: the group-velocity dispersion of each material from its own
      formula (`optobench_materials::dispersion`: five-point differences at h and h/2,
      Richardson extrapolated; a tabulated index is refused), summed along the chief ray
      (ray records gain `media`; an ideal objective adds the GDD its maker states,
      `group_delay_dispersion_fs2`) after the source's prechirp; a Gaussian pulse widened
      to τ √(1 + (4 ln 2 · GDD / τ²)²), applied to sech² pulses as an approximation
      (`excitation::pulse_path`)
- [x] Checks: GDD of fused silica (Malitson's formula) and of a Schott-form glass against
      the formulas differentiated by hand (n′ to 10⁻⁹, n″ to 10⁻⁶; catalogue BK7 is not
      committed, so a Schott formula with chosen coefficients stands in); g_p of both
      shapes by numerical integration (10⁻¹⁰); Gaussian broadening against the chirped
      spectrum summed back into time (10⁻⁶); the rates against σ₁ F and ½ σ₂ F² g_p/(f τ);
      a layout's dispersion along its chief ray (window, objective, water) against the
      sum by hand (commands `tests/excitation.rs`); ε, GM and µM conversions; refusals

### M11b2 The excitation volume

- [x] The excitation volume from the M9c2 vectorial PSF at the actual depth (index
      mismatch and its aberration included), each plane wave attenuated by its own ray's
      ballistic loss exp(−Σ (µa + µs) L) from the front plane (`PsfSettings::attenuated`,
      from the rays' `media`; `Psf::transmission` reports it always)
- [x] Volume integrals (`PsfSettings::volume`, `Psf::volume`): ∫ I dA and ∫ I² dA on
      planes across the focus line, spaced z = z_peak + a tan t (a the half axial FWHM),
      each sampled on a grid that widens with the cone, out to where the discrete
      plane-wave sum starts to repeat (λ₀ / Δp; too few rays are refused); the tails
      beyond by K / (z² + b²) through the two outermost planes on each side, exact for
      a Gaussian focus. Widths of I² (`fwhm_squared`, I at 1/√2 of its peak)
- [x] `optobench_analysis::excitation::excitation`: where the focus is (sample layer,
      from the chief ray) and the fluorophore's concentration there; one-photon peak rate,
      saturation k τ_f and excitations per depth σ₁ C P / (h ν) at the focal plane (it
      is not confined; the diffuse light's share is not counted); two-photon peak rate,
      per-pulse probability and excitations in the focus ½ σ₂ C g ∫ F² dV (thick-sample
      limit: the layer taken as much thicker than the focus); photons emitted (× η), the
      emitter power M11c's isotropic source takes; the pulse width at the focus after the
      path's dispersion
- [x] Command `excitation`; graphs `excitation-depth` (excitations per depth per plane,
      one- and two-photon, against z), `excitation-map` (rate per molecule, x–z),
      `pulse-width` (along the chief ray); units `time`, `rate`, `rate-per-length`
- [x] Checks (commands `tests/excitation.rs`): a Gaussian focus (1/e² n sin θ ≈ 0.17)
      against Xu and Webb's note 15, ∫ I² dV = π n P² / λ (0.998 found; to 1 %); a filled
      NA 0.5 pupil against their relations (9), (10), 8 n P² / (π λ) (0.993; to their
      3 %); ∫ I dA = P on every plane (1 %); the pulsed total against ½ σ₂ C g π n P² /
      (λ (h ν)²) (1 %); the one-photon rate σ₁ I / (h ν) and per depth σ₁ C P / (h ν);
      a turbid layer's ballistic transmission between exp(−µt d) and exp(−µt d / cos θ_max)
      and the peak falling by it (1 %); too few rays refused; the command and its graphs

### M11b3 FPbase

- [x] Licence review entry (docs/licence-review.md, FPbase, recommendation 5): FPbase's
      terms quoted from its own pages; data "free of all copyright restrictions" with
      attribution of the original authors asked; the site under CC BY-SA 4.0, its reach
      to the data not stated (an open decision there); some data may carry patents or
      other rights. FPbase's source code (GPL-3.0) was not opened; the reader follows the
      API documentation and the API's own output
- [x] Import from FPbase's REST API output (`optobench_io::fpbase`, command
      `import-fluorophore`): the user downloads `/api/proteins/spectra/?name__iexact=…`
      (spectra, extinction coefficient, quantum yield) and optionally `/api/proteins/…`
      (lifetime, the primary reference's DOI); a state is chosen (`default`), every field
      checked (the API is "subject to change"). Nothing is stored before the user accepts
      the terms shown; then the files are kept unchanged with a `provenance.json` in
      `<data>/optobench/sources/fpbase/<version>/`, and a `[[fluorophore]]` entry is
      returned or appended to a design (before its `[resolved]` section), its `source`
      carrying FPbase's citation, the protein, its state and its DOI, so the attribution
      travels with the values. Never bundled
- [x] Two-photon: the API gives no cross-section, only (per its schema) a relative `2p`
      spectrum; it is written commented out with a note to give a published peak.
      Designs take `two_photon_spectrum` (relative) shaped by a peak
      `two_photon_cross_section_gm` or `two_photon_action_cross_section_gm`
- [x] Checks (commands `tests/fluorophores.rs`, with made-up files shaped like the API's):
      terms shown and nothing written before acceptance; stored files and provenance;
      the appended design reads back with σ₁ from ε shaped by the excitation spectrum,
      quantum yield, lifetime and the attribution; a second import under the same id, an
      ambiguous file, an unknown state and broken JSON refused; a relative two-photon
      spectrum shaped by an absorption or an action peak

## M11c Emission and the photon budget

Built ahead of M11b: the emitter's power (its emission over all directions) is given
by the user; M11b2's `excitation` now reports the photons emitted per second at a focus
(times the photon energy, that power).

- [x] An isotropic point emitter (source kind `isotropic`: equal solid angles, the disk
      samples mapped onto the cap by cos θ = 1 − ρ² (1 − cos θ_max); `power_w` the total
      over 4π, rays only in the cone, which `reversed = true` turns to face the
      objective), placed by the user at the excited volume's centroid; a source inside a
      sample layer starts in that layer's medium. Traced back through the sample's
      layers, the objective and the emission path, through filters (measured curves), to
      detectors with a quantum efficiency (`quantum_efficiency = { value, source }`, a
      number or a table)
- [x] The ideal objective takes light back from the sample: a face on its front plane
      (`Role::ObjectiveFront`, `IdealObjective::pass_back`) accepts rays with
      n sin θ ≤ NA and sends them from the pupil at ρ = −f p along the reverse of their
      field's direction, the same optical path added (forward and back are each other's
      inverse to 10⁻¹²). Light reflected back into the objective in forward traces now
      passes back too, where it was clipped
- [ ] Emitters sampled through the excited volume (with M11b's excitation volume)
- [x] Photon budget (`optobench_analysis::emission`, command `emission`), in W and
      photons per second (P λ / (h c)): emitted, outside the traced cone, traced,
      collected by the objective; per sample part absorbed and scattered; per item the
      power not transmitted at its faces (Fresnel, a filter's rejection; ray records gain
      `face_losses`) and clipped; per detector arriving and counted (× QE(λ)); where the
      rest ended (the trace's ledger, absorption split into samples, other media and
      coatings); the imbalance
- [x] Checks (commands `tests/emission.rs`): an emitter in a matched medium (n 1.33)
      under an NA 1.0 objective collects (1 − cos θ) / 2, n sin θ = NA, within the one
      ring of the sampling the cut falls in (2.4 % at 100 rings; 1.1 % found), and all of
      it reaches the camera; the budget closes to 10⁻⁹ and the cone's share is exact;
      a filter curve folded with a five-line spectrum by hand (camera and filter loss to
      10⁻¹⁰, counted photons with a QE table likewise); an emitter 0.5 mm deep in a turbid
      layer loses ½ ∫ (1 − e^{−µt d/u}) du over the cone (to 10⁻³) in the ratio µa : µs
      (10⁻¹²); missing sources, values above 1 and wavelengths off the QE table refused

## M11d Heating

- [ ] The absorbed power density (M11a) as the heat source in the layer stack, with
      thermal conductivity, density and heat capacity per layer (water's from published
      values with their source, e.g. the IAPWS releases; tissue's from the user);
      boundaries at fixed temperature, insulated, or with a heat transfer coefficient
- [ ] Steady state and time-resolved temperature rise (a parked beam switched on and
      off, a beam scanned over a field of view at a line rate), by a finite-volume solver
      on a cylindrical grid about the axis, or a 3-D grid for scans
- [ ] Optional perfusion (Pennes bioheat: ρ_b c_b w_b (T − T_a)) for tissue in vivo
- [ ] Reports: power at the sample surface and at the focal plane; peak and field-averaged
      temperature rise; warnings when the user's heating or saturation thresholds are
      passed
- [ ] Graphs: temperature maps (r–z, x–z), temperature against time at chosen points
- [ ] Checks: steady temperature rise of a Gaussian heat source in an infinite medium
      against its analytic solution (the Green's function 1 / (4π κ r) integrated over
      the source: ΔT(0) = P_abs / ((2π)^{3/2} κ σ) for a spherical Gaussian of width σ, and
      the beam-shaped case along the axis); the time course at the centre against the
      transient solution; a point source in a half-space at fixed surface temperature
      against the image method; energy conserved by the solver (absorbed = conducted
      out + stored, to its tolerance)

## Exit check

- [x] Excitation rates match the closed forms above (one- and two-photon): M11b2's checks
- [ ] The temperature rise matches the analytic Gaussian-source solution
- [ ] A published multiphoton heating benchmark: K. Podgorski and G. Ranganathan, "Brain
      heating induced by near-infrared lasers during multiphoton microscopy",
      J. Neurophysiol. 116, 1012 (2016) (local corpus). Values from its text only:
      - conditions: 920 nm, 80 MHz, ~120 fs; a 1 mm² field raster-scanned at 30 Hz (70 %
        duty cycle), focal plane 250 µm below the dura, NA 0.8 or 0.08 (no difference in
        heating); a cranial window with immersion water at room temperature (in their
        model a 350 µm glass window, the water held at 25 °C 1.5 mm above the glass),
        body at 37 °C, perfusion
      - measured: steady-state heating 1.86 ± 0.25 °C per 100 mW at 250 µm (quantum-dot
        thermometry, N = 4); after 180 s, 1.50 ± 0.02 °C per 100 mW at 150 µm and
        1.73 ± 0.04 at 500 µm (thermocouple), steady state ~6 % above the 180 s value;
        linear in power
      - their simulation: the largest rise ~500 µm below the surface; a field 4× smaller
        heats 20.7 % more, 4× larger 23.6 % less
      The brain's optical and thermal properties are not in the paper: its model is
      adapted from J. M. Stujenske, T. Spellman and J. A. Gordon, Cell Rep. 12, 525 (2015)
      (local corpus), with constants "from published tables". Stujenske et al. give the
      model (Monte Carlo light with Henyey–Greenstein scattering, brain n = 1.36; Pennes
      bioheat with the light as source, ρc ∂T/∂t = ∇·(k∇T) + ρ_b c_b w_b (T_A − T) + q_m
      + φ µa, baseline 37 °C, arterial blood 36.7 °C) but put their thermal and perfusion
      values in Supplementary Table S1, and take µa, µs and g from J. D. Johansson,
      J. Biomed. Opt. 15, 057005 (2010). Both are now in the corpus; what they give is
      under "Brain properties from the sources" below. It
      models light by Monte Carlo transport; at the millimetre scale of this heating the
      diffusion approximation should do, so the check is our steady-state heating per
      100 mW at 250 µm against 1.86 ± 0.25 (within its stated error), the ratio of 500 to
      150 µm, and the field-size dependence; a larger gap is reported, and the light
      transport part of it moves to M15. Their code (available on request, no licence) is
      not used

## Brain properties from the sources

Read from the local corpus on 2026-11-13; each value cited where a test uses it.

- **Stujenske, Spellman and Gordon, Cell Rep. 12, 525 (2015), Supplemental Table 1**
  (Pennes model): brain ρ = 1.04 × 10⁻⁶ kg/mm³, c = 3.65 × 10⁶ mJ/(kg °C),
  k = 0.527 (printed "mW m⁻¹ °C⁻¹"; it must be W m⁻¹ °C⁻¹ = mW mm⁻¹ °C⁻¹: their
  Eq. (10), Δt = (Δx)² ρc / (6k) = 1.1 ms with Δx = 30 µm, holds only with that unit);
  blood ρ_b = 1.06 × 10⁻⁶ kg/mm³, c_b = 3.6 × 10⁶ mJ/(kg °C); perfusion
  w_b = 8.5 × 10⁻³ s⁻¹ (≈ 51 mL/100 g/min); metabolic heat q_m = 9.7 × 10⁻³ mW/mm³;
  arterial 36.7 °C, initial brain 37 °C. Main text: brain n = 1.36; µa, µs, g "linearly
  interpolated" from Johansson (2010); Henyey–Greenstein Monte Carlo; tissue a
  cylinder 6 mm in radius, 10 mm thick
- **Johansson, J. Biomed. Opt. 15, 057005 (2010), Table 1** (from human ex vivo data,
  his ref. 17), µs (1/mm), g, n:
  - gray matter, n 1.36: 480 nm 11, 0.89; 580 nm 9.7, 0.89; 640 nm 9.0, 0.89;
    700 nm 8.2, 0.9; 780 nm 7.8, 0.90; 900 nm 6.6, 0.9 (µs′ 1.21 … 0.66)
  - white matter, n 1.38: 480 nm 43, 0.80; 580 nm 41, 0.83; 640 nm 41, 0.84;
    700 nm 40, 0.86; 780 nm 38, 0.87; 900 nm 33, 0.88 (µs′ 8.60 … 3.96)
  - µs′ fits ∝ λ^−0.96 (gray), λ^−1.16 (white), Sect. 3.1
  - µa is given only as measured spectra (Figs. 6b, 7b, 480–900 nm), not as numbers.
    Read off the figure, at 900 nm gray matter is ~0.015–0.02 /mm and white matter
    ~0.06 /mm; figure readings, not reference values. Chromophore fits in the text:
    gray matter 0.54 % deoxygenated blood, 0.08 % melanosomes, reduced cytochromes c and
    aa3 2.2 and 4.0 µM; white matter 0.12 % oxygenated and 0.14 % deoxygenated blood,
    0.11 % melanosomes, oxidised cytochrome aa3 2.4 µM
  - 920 nm (Podgorski and Ranganathan) lies outside Table 1: µs and g must be
    extrapolated (e.g. by the λ^−0.96 fit: gray-matter µs′ ≈ 0.646 /mm), and Podgorski and
    Ranganathan do not say how they did it. The benchmark states the values it uses and
    reports its sensitivity to µa over the figure's range
- **Welch and van Gemert (2011)**, beyond Star's chapter: J. W. Valvano, ch. 12, Table 12.9
  (water, k 6.233 mW/(cm °C) at 310 K, 5.818 … 6.797 from 280 to 370 K) and Table 12.12
  (perfusion, mL/100 g/min: e.g. rat frontal cortex 106 ± 6, baboon gray matter
  56.2 ± 7.7); K. R. Diller, ch. 10, Table 10.2 gives water at 37 °C k = 9.49 × 10⁻² W/(m K),
  which disagrees with Table 12.9 (0.623) and must be a misprint, and ρ = 993 kg/m³,
  c = 4.178 kJ/(kg K). Star's Table 6.2: r₂₁ for n = 1.4 is 0.49 (step) or 0.53 (Fresnel),
  as our A ≈ 3.25 gives (r₂₁ = 0.529)

## Two-photon excitation from the sources

Read from the local corpus on 2026-11-13.

**C. Xu and W. W. Webb, "Measurement of two-photon excitation cross sections of
molecular fluorophores with data from 690 to 1050 nm", J. Opt. Soc. Am. B 13, 481 (1996)**

- N_abs = ∫ δ C I² dV photons absorbed per second (Eq. (1), I in photons/(area s));
  fluorescence F = ½ φ η₂ N_abs, the ½ for two photons per excitation (Eq. (3)); with
  g = ⟨I₀²⟩ / ⟨I₀⟩², ⟨F⟩ = ½ g φ η₂ C δ ⟨I₀⟩² ∫ S² dV (Eq. (5))
- Uniformly filled pupil, paraxial (adequate for sin θ < 0.7): I₀ = π NA² P / λ²
  (Eq. (9)); thick sample ∫ S² dV ≈ 8 n λ³ / (π³ NA⁴) (relation (10), numerical, 3 %,
  note 14); so ⟨F⟩ ≈ ½ g φ η₂ C δ 8 n ⟨P⟩² / (π λ) (relation (11)), independent of NA
- Gaussian beam, thick sample: ⟨F⟩ = ½ g φ η₂ C δ (n π / λ) ⟨P⟩², independent of the
  waist (note 15)
- g = g_p / (f τ) (Eq. (13)), g_p = τ ∫ I₀² dt / (∫ I₀ dt)² over one period (Eq. (14));
  0.664 Gaussian, 0.588 sech²; g ≈ 10⁵ at ~100 MHz and ~100 fs; g = 1 for single-mode
  cw. Their own laser measured g_p = 0.41 ± 0.09 against 0.588 expected for sech² (Sect.
  4): the nominal shape can overstate ⟨I²⟩ by ~40 %, worth stating in reports
- 1 mW average at NA 0.3 gives a pulse intensity of ~10²⁸ photons/(cm² s) (Sect. 3A)

**C. Xu, W. Zipfel, J. B. Shear, R. M. Williams and W. W. Webb, "Multiphoton
fluorescence excitation: new spectral windows for biological nonlinear microscopy",
Proc. Natl. Acad. Sci. USA 93, 10763 (1996)**

- 1 GM = 10⁻⁵⁰ cm⁴ s per photon (Fig. 1). Figs. 1, 3 and 4 and Table 1 give action
  cross-sections η σ₂ (all but Bis-MSB in Fig. 1, rhodamine B and fluorescein in Fig. 2);
  the two- and three-photon quantum efficiencies are assumed equal (Table 1 note);
  uncertainties ~30 % for σ₂, a factor ~3 for σ₃
- Emission spectra are the same after one-, two- and three-photon excitation (text): one
  emission spectrum per fluorophore serves every excitation mode
- Saturation: σₙ I_peakⁿ τ ≈ 1 for a square pulse, I in photons/(cm² s) (Eq. 1);
  P_avg ≈ (0.61)² λ² (f τ) I_peak / NA² for a diffraction-limited focus (Eq. 2). Eq. 3,
  the saturation power, prints (σₙ τ)^(−1/n) in the denominator; combining Eqs. 1 and 2
  puts it in the numerator. Its example (NA 1.3, 100 fs, 80 MHz, 1.0 µm; ~30, ~150,
  ~300 mW for two-, three- and four-photon) does not follow from Eq. 2 with the
  cross-sections it estimates (σ₂ ~ 10⁻⁴⁹ cm⁴ s gives ~3.5 mW); not used as a reference
- In the thick-sample limit the two-photon fluorescence of a Gaussian–Lorentzian beam is
  independent of the NA, the three-photon one is not (∝ σ₃ P³ / ((f τ)² w₀²), Eq. 4);
  stated here, derived in the JOSA B paper above

## Open questions

- Which water thermal property values to adopt and how to cite them (IAPWS formulations
  are published for free use; check their terms against the bundling rules before
  bundling any table).
- Fluorophore data: FPbase (decided 2026-11-12: imported by the user, notice shown,
  never bundled). Other dye databases to be reviewed the same way when needed.
- The diffusion approximation fails close to sources and boundaries and at shallow
  depth for strongly forward-scattering tissue (g ≈ 0.9); report where it is used and
  how far it can be trusted, until M15.
