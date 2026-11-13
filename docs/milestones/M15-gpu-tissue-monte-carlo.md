# M15 GPU and tissue Monte Carlo

Exit criterion (docs/design.md, milestone table): the GPU equals the CPU within
statistical error; tissue fluence matches published slab benchmarks.

Scope: photon transport in scattering tissue by Monte Carlo (Henyey–Greenstein
scattering) replacing the diffusion approximation of M11 for fluence and heating, and
wgpu compute for the batches that are worth it. The engine stays pure Rust and builds for
wasm32; the GPU is optional everywhere (no adapter: the CPU runs, with the same results
within statistical error).

## M15a Tissue Monte Carlo on the CPU

The method is the standard weighted photon random walk (S. A. Prahl, M. Keijzer,
S. L. Jacques and A. J. Welch, SPIE IS 5, 102 (1989); L. Wang, S. L. Jacques and L. Zheng,
Comput. Methods Programs Biomed. 47, 131 (1995), "MCML"; as written out by S. L. Jacques
and L. Wang, ch. 5 of A. J. Welch and M. J. C. van Gemert, Optical-Thermal Response of
Laser-Irradiated Tissue, 2nd ed., 2011, in the local corpus), written from those
descriptions; no code is adapted.

- [x] `optobench_analysis::photon`: packets from the traced rays where they cross a
      sample's first face going in (`sample_launches`: position, direction just inside,
      power after that face's Fresnel loss), or a beam from above (`entering`, the
      specular part split off); the stack from the sample's layers at the rays'
      wavelength (`sample_tissue`)
- [x] The walk (`walk`): steps −ln ξ / µt with the optical depth carried across layer
      boundaries; w µa / µt left at each interaction; Henyey–Greenstein in closed form
      (ch. 5, Eq. (5.45)); Fresnel (unpolarised) at every index step, the packet
      reflected or refracted by chance, total internal reflection included; Russian
      roulette (one in 10 survives with ten times its weight below 10⁻⁴ of the launch
      weight), its net effect kept so the balance closes exactly
- [x] Results (`Transport`): absorbed per cell of the r–z grid and outside it, per layer,
      weighted track length per cell (the fluence rate, also where µa = 0), reflected and
      transmitted; batches add up (`absorb`)
- [x] The generator moved to `optobench_math::random` (`tolerance::draw` keeps the
      distributions), independent streams per batch
- [x] Checks: the Henyey–Greenstein sampler's mean cosine (g = 0, 0.5, 0.9, −0.3) and its
      cumulative distribution against the phase function integrated; turning keeps unit
      vectors at the drawn angle; an absorbing slab of n = 1.5 in air against the
      multiple-reflection formulas (specular exact, R and T within 4σ); a semi-infinite
      isotropic scatterer of albedo 0.9 against Chandrasekhar's plane albedo
      1 − H(1) √(1 − ω), the H-function solved in the test (its zeroth moment
      (ω/2) ∫ H = 1 − √(1 − ω) to 10⁻⁹), within 4σ; packets launched from a traced layout
      carry (1 − r) of the beam into a slab and cross it as the formula says; the
      balance closes to rounding (10⁻¹⁰)
- [ ] Published slab benchmarks (exit criterion): van de Hulst's tables as MCML compares
      with them, once those sources are in the corpus

## M15b The tissue Monte Carlo on the GPU

- [ ] wgpu (MIT OR Apache-2.0) behind a feature; the photon walk as a WGSL compute
      kernel (f32 on most GPUs: its accuracy against the f64 CPU measured), absorbed
      power accumulated per cell, one generator stream per invocation
- [ ] No adapter, or the feature off: the CPU path, the same interface
- [ ] Check (exit criterion): the GPU's reflectance, transmittance and absorbed profile
      equal the CPU's within their statistical errors (several seeds, a χ² over the
      grid); the speed-up measured and written down

## M15c Non-sequential batches on the GPU

- [ ] Ray batches through faces with analytic surfaces (planes, spheres, conics, ideal
      lenses and objectives) on the GPU; B-spline and other exact B-rep faces stay on the
      CPU (decided here: their Newton iterations and trimming loops are a large port for
      little gain while STEP scenes are mostly analytic)
- [ ] Check: the same detector powers, centroids and ledgers as the CPU trace, within f32
      accuracy, on the golden scenes

## M15d Fluence and heating from the Monte Carlo

- [ ] `fluence` and `heating` take the light from the Monte Carlo (`transport =
      "monte-carlo"`, then the default) or from diffusion (M11a2)
- [ ] The Podgorski and Ranganathan benchmark (M11 exit check, left open) run again with
      Monte Carlo light; its gap reported

## Open questions

- Which published slab benchmarks to use and where their numbers come from (van de
  Hulst's tables; MCML's comparisons): the papers are needed in the corpus.
- Polarisation in the tissue walk (ch. 7 of Welch and van Gemert): not planned.
