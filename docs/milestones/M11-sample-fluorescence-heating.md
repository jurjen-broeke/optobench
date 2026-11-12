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

- [ ] Scattered light in the diffusion approximation: the fluence rate from the
      scattered power as a source, per layer, with the layers' boundaries (refractive-index
      mismatch at each face); the absorbed power density µa × fluence rate, ballistic and
      diffuse
- [ ] Graphs: power and fluence rate against depth, absorbed power density as an x–z
      map
- [ ] Checks: the diffusion fluence of a point source in an infinite medium,
      Φ = P exp(−µ_eff r) / (4π D r), µ_eff = √(3 µa (µa + µs′)), D = 1/(3(µa + µs′))
      (A. J. Welch and M. J. C. van Gemert, Optical-Thermal Response of Laser-Irradiated
      Tissue, 2nd ed., 2011, ch. 6), and of a collimated beam on a half-space against the
      same book's closed forms

## M11b Excitation

- [ ] Fluorophores (user data, with provenance): excitation and emission spectra,
      extinction coefficient or one-photon cross-section, two-photon cross-section
      spectrum, quantum yield, lifetime, concentration; placed in a sample layer
- [ ] Import from FPbase (fpbase.org; T. J. Lambert, Nat. Methods 16, 277 (2019)) through
      its documented REST API: spectra, extinction coefficient, quantum yield, two-photon
      cross-section where given. Imported by the user like refractiveindex.info data,
      never bundled: FPbase states CC BY-SA 4.0 for the site and also that its data are
      "free of all copyright restrictions", and asks that each protein's original authors
      be credited, so the import shows that notice and stores the protein's references
      with the data (licence review entry first). FPbase's source code is GPL-3.0 and is
      not read; only the API documentation is
- [ ] Lasers: continuous, or pulsed with repetition rate and pulse width (sech² or
      Gaussian)
- [ ] One-photon excitation rate per molecule σ₁ I / (h ν) from the local irradiance;
      saturation check against the lifetime (σ₁ I τ / (h ν) ≪ 1)
- [ ] Two-photon excitation: rate ½ σ₂ ⟨I²⟩ / (h ν)² per molecule (σ₂ the two-photon
      absorption cross-section, two photons per excitation), with ⟨I²⟩ = g_p ⟨I⟩² / (f τ) for
      pulses (g_p 0.664 Gaussian, 0.588 sech²; C. Xu and W. W. Webb, J. Opt. Soc. Am. B 13,
      481 (1996)); per-pulse saturation check
- [ ] The excitation volume from the M9c2 vectorial PSF at the actual depth (index
      mismatch and its aberration included), attenuated by the ballistic loss on the way;
      total excitation per second ∫ rate × concentration dV, and the volume's sizes
- [ ] Pulse broadening: the group-delay dispersion of every glass on the path from the
      same dispersion models the trace uses (GDD = λ³ / (2π c²) · d²n/dλ² · L); a Gaussian
      pulse stretched to τ √(1 + (4 ln 2 · GDD / τ²)²) (e.g. J.-C. Diels and W. Rudolph,
      Ultrashort Laser Pulse Phenomena, 2nd ed., 2006, ch. 1), which lowers ⟨I²⟩
- [ ] Graphs: excitation rate against depth, excitation maps (x–z), pulse width along
      the path
- [ ] Checks: the one-photon rate against σ₁ I / (h ν) for a uniform beam; the two-photon
      total of a Gaussian–Lorentzian focus against its closed form (Xu and Webb 1996);
      GDD of fused silica and BK7 against the dispersion formulas differentiated by hand;
      Gaussian pulse broadening by a given GDD

## M11c Emission and the photon budget

- [ ] Emission from the excited volume as isotropic point emitters (the volume's
      centroid, or sampled through it), traced back through the sample's layers, the
      objective and the emission path, through filters (measured curves) to a detector
      with a quantum-efficiency curve
- [ ] Photon budget: photons per second emitted, collected by the objective, through the
      emission path, detected; and each loss on the way (Fresnel, filters, clipping,
      absorption and scattering in the sample)
- [ ] Checks: an emitter in a uniform medium under an aperture of half-angle θ collects
      (1 − cos θ) / 2 of its light (before Fresnel losses); the ledger closes to 10⁻⁹; a
      filter curve's transmission folded with an emission spectrum by hand

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

- [ ] Excitation rates match the closed forms above (one- and two-photon)
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
      J. Biomed. Opt. 15, 057005 (2010). Both are needed next (corpus). It
      models light by Monte Carlo transport; at the millimetre scale of this heating the
      diffusion approximation should do, so the check is our steady-state heating per
      100 mW at 250 µm against 1.86 ± 0.25 (within its stated error), the ratio of 500 to
      150 µm, and the field-size dependence; a larger gap is reported, and the light
      transport part of it moves to M15. Their code (available on request, no licence) is
      not used

## Open questions

- Which water thermal property values to adopt and how to cite them (IAPWS formulations
  are published for free use; check their terms against the bundling rules before
  bundling any table).
- Fluorophore data: FPbase (decided 2026-11-12: imported by the user, notice shown,
  never bundled). Other dye databases to be reviewed the same way when needed.
- The diffusion approximation fails close to sources and boundaries and at shallow
  depth for strongly forward-scattering tissue (g ≈ 0.9); report where it is used and
  how far it can be trusted, until M15.
