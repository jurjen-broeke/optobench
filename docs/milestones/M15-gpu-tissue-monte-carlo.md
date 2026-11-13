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

- [ ] Photon packets from the traced rays where they enter a sample (position, direction,
      power), so a focused or scanned beam is launched as it arrives; also a pencil beam
      and a uniform disk for benchmarks
- [ ] Steps s = −ln ξ / µt; at each interaction the weight times µa / µt deposited (on
      the sample's r–z grid) and the rest scattered by Henyey–Greenstein (cos θ sampled
      in closed form; ch. 5 Eq. (5.45)); layers with their own µa, µs, g and n, Fresnel
      reflection at every index step (the packet reflected or transmitted by chance,
      unpolarised), total internal reflection; Russian roulette below a weight threshold
      (survival 1/m, weight × m; Eq. (5.49)); escape through the top, the bottom and the
      side recorded
- [ ] Results as for M11a2: absorbed power density per cell, fluence rate µa⁻¹ × that
      (and from the track lengths where µa = 0), reflectance and transmittance (diffuse
      and specular), per layer absorption; the balance closes
- [ ] The seeded generator moved to `optobench-math` (L0; xoshiro256** from M14),
      independent streams per batch, so results do not depend on threads
- [ ] Checks against exact results: an absorbing slab between index steps (no
      scattering) against the multiple-reflection formula; a semi-infinite isotropic
      scatterer under a matched boundary against its total reflectance from
      Chandrasekhar's H-function (S. Chandrasekhar, Radiative Transfer, 1950; the
      H-equation solved in the test); the Henyey–Greenstein sampler's mean cosine and
      distribution; the balance; diffusion's µ_eff far from the source
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
