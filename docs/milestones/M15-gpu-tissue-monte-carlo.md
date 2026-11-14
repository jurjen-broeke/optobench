# M15 GPU and tissue Monte Carlo

Exit criterion (docs/design.md, milestone table): the GPU equals the CPU within
statistical error; tissue fluence matches published slab benchmarks. Both met
2026-11-14 (M15a, M15b).

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
- [x] Published slab benchmarks (exit criterion, read 2026-11-14 from L. Wang, S. L. Jacques
      and L. Zheng, Comput. Methods Programs Biomed. 47, 131 (1995), Tables 1 and 2, and
      S. A. Prahl et al., SPIE IS 5, 102 (1989), Sect. 10; van de Hulst's book itself was
      not available): an index-matched slab, µa = 10 /cm, µs = 90 /cm, g = 0.75,
      d = 0.02 cm: diffuse reflectance 0.09739 and total transmittance 0.66096 (van de
      Hulst's tables); a semi-infinite medium of relative index 1.5, same µa and µs,
      g = 0: total reflection 0.2600 (Giovanelli 1955), the specular 0.04 included (Prahl
      et al. call it total reflection; MCML adds the specular reflectance to its
      unscattered reflectance, Sect. 4.1): ours within 4σ of each (commands
      `tests/photon.rs`; MCML's own results 0.09734 ± 0.00035, 0.66096 ± 0.00020,
      0.25907 ± 0.0017)
- [x] Fluence: deep in a semi-infinite isotropic scatterer (albedo 0.9) the fluence decays
      at k µt with k the root of (ω / 2k) ln((1 + k)/(1 − k)) = 1, the transport
      equation's asymptotic solution (k = 0.5254; diffusion's √(3 (1 − ω)) = 0.5477 is 4 %
      off): the walk's slope 0.5261 (to 3 %)

## M15b The tissue Monte Carlo on the GPU

- [x] Crate `optobench-gpu` (L4; wgpu 30, MIT OR Apache-2.0, the version eframe already
      brings; pollster on desktop): `Gpu::new` (async; `blocking` on desktop) takes the
      first adapter, `None` without one, and the caller runs the CPU walk; `Gpu::walk`
      (`walk_blocking`) gives the same `Transport` as `photon::walk`
- [x] The kernel (`walk.wgsl`): one invocation per packet, the CPU walk step by step in
      f32; random numbers from xoshiro128** (Blackman and Vigna's 32-bit member), each
      invocation seeded from (seed, batch, packet) by a 32-bit mixer; every deposit
      (absorbed per cell, track per cell, outside, reflected, transmitted, per layer,
      roulette, interactions) summed in 64-bit fixed point, 2⁻³⁰ of the largest launch
      weight, from two u32 atomics with carry: exact and independent of the order the
      invocations finish in; dispatched in chunks of 65 535 workgroups of 64
- [x] Builds for wasm32 (in CI's engine build); on the web `walk` would need the browser's
      event loop to finish (it waits by polling): not used there yet
- [x] Checks (`optobench-gpu/tests/walk.rs`; skipped with a message without an adapter,
      as on CI runners): van de Hulst's slab (0.09677, 0.66179 at 10⁶ packets; 0.09741
      and 0.66096 at 1.6 × 10⁷: no f32 bias) and Giovanelli's medium (0.25929) within 4σ;
      the exit criterion, the GPU against the CPU: a two-layer, forward-scattering,
      index-mismatched stack, the absorbed depth profile (12 slices), reflectance and
      transmittance from 8 independent batches each, χ² = 9.4 over 14 quantities (below
      36.1, p = 0.999). Speed on an AMD Radeon Pro 555X against one core of an Intel Core
      i7-8750H (both release): 6.5 µs against 195 µs per packet, 30×

## M15c Non-sequential batches on the GPU

- [ ] Ray batches through faces with analytic surfaces (planes, spheres, conics, ideal
      lenses and objectives) on the GPU; B-spline and other exact B-rep faces stay on the
      CPU (decided here: their Newton iterations and trimming loops are a large port for
      little gain while STEP scenes are mostly analytic)
- [ ] Check: the same detector powers, centroids and ledgers as the CPU trace, within f32
      accuracy, on the golden scenes

## M15d Fluence and heating from the Monte Carlo

- [x] `fluence` and `heating` (and their graphs) take `transport = "monte-carlo"` with
      `packets` per traced ray (default 200), `seed`, `gpu` (default true: use one if
      found) and `threads`: the walk on the GPU (found once per process), else on the
      CPU's threads, one batch and stream each, the power shared between them
      (`commands::transport`); the light built as for diffusion
      (`sample::light_from_transport`: absorbed density, fluence from the track,
      absorption per layer, the run's account), heating fed through `heating_with`.
      Heating's walk records no fluence (`WalkSettings::track`, also on the GPU), and its
      default focal depth then follows the absorption. Decided: diffusion stays the
      default, being deterministic and fast without a GPU; Monte Carlo is asked for
- [x] Check: an evenly lit absorbing layer heated by Monte Carlo light (CPU, 4 threads):
      Beer–Lambert's absorbed power within 4σ, the heat balance closed, the rise at 0.5 mm
      as with diffusion (3 %); the fluence command's account closes
- [x] The Podgorski and Ranganathan benchmark again, light by Monte Carlo (commands
      `tests/podgorski.rs`, on the GPU; skipped without one): per 100 mW at 250 µm
      0.578 K (0.519 at 150 µm, 0.667 at 500), against 0.47 by diffusion and 1.86 ± 0.25
      measured. About 60 % of the light comes back out of the brain (diffusion theory
      gives as much for µs′ 0.65 /mm, µa 0.02 /mm), 1 % goes through; over µa 0.015 to
      0.02 /mm the rise is 0.50 to 0.66 K. The light transport accounts for about a fifth
      of the gap; the rest, a factor of about 3, points to µa (in vivo blood) or to where
      the paper's 100 mW is measured. 500 µm against 150 µm: +29 % (measured +20 ± 7 %)

## Open questions

- Polarisation in the tissue walk (ch. 7 of Welch and van Gemert): not planned.
