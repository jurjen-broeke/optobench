# M14 Tolerancing

Exit criterion (docs/design.md, milestone table): the sensitivity ranking is stable
across seeds; the runtime is documented. Met 2026-11-13 (M14c).

Scope: Monte Carlo over DOFs, mount and part tolerances; a yield and sensitivity report.
It builds on M6 (the DOF model, merit terms, the damped least-squares optimiser and the
move report) and runs on standalone layouts and on STEP scenes with sidecars alike. It
needs nothing from SolidCraft.

The method follows the usual practice of optical tolerancing, written from first
principles: each tolerance is a random error with a stated distribution; a trial draws
every error, applies it, lets the compensators (the adjustments made at assembly) take
out what they can, and measures what is left against the criteria; the yield is the
share of trials that pass. Sensitivities come one tolerance at a time; the Monte Carlo
gives the joint result. References for the conventions (uniform, normal truncated at
±3σ, end-point distributions; RSS estimates; compensators): W. J. Smith, Modern Optical
Engineering, 4th ed., 2008, ch. 21 (tolerancing), checked once it is in the corpus;
until then every number in a test is from theory (closed forms of the linear cases).

## M14a The tolerance model and sensitivities

- [x] Tolerances in layouts and sidecars: top-level `[[tolerance]]` tables
      (`optobench_io::tolerance`, `optobench_scene::tolerance`), each with a name, a kind,
      a limit and a distribution:
      - placement errors `slide` (mm) and `tilt` (degrees) of an element, source or
        detector (`target`; axes in its nominal frame or the scene's, the pivot by default
        its origin) or of a CAD occurrence (`occurrence`, scene frame, pivot given), never
        adjusted; `dof`: an error added to a DOF's value
      - part errors of lens elements: `radius` (mm, or a fraction with `relative`; flat
        surfaces refused), `thickness` (mm), `index` (Δn); of an ideal lens
        `focal_length` (mm or a fraction)
      - `uniform` (default), `normal` (σ = limit / 3, truncated) or `end` (±limit)
- [x] `[tolerancing]`: `criteria` (merit terms with `max`, the largest magnitude that
      passes: the residual vector's length) and `compensators` (DOFs re-optimised per
      evaluation on the design's `[[merit]]`, every other DOF held)
- [x] The generator (`optobench_optimize::tolerance::Random`): xoshiro256** seeded by
      SplitMix64, independent streams per trial, Box–Muller normals, draws
- [x] Applying errors (`tolerance::apply`): placement errors as locked DOFs of their own
      (`tolerance <name>`, after the target's DOFs), DOF errors on the values, part errors
      on the prescription (Δn by a shifted material)
- [x] Sensitivities (`tolerance::sensitivity`): nominal, each tolerance alone at ±limit
      with compensation, the larger change per criterion, the RSS and the ranking
- [x] Checks (commands `tests/tolerance.rs`; the generator in its unit tests):
      SplitMix64's first output for seed 0 as published; uniform mean, variance and χ²;
      normal moments; the truncated normal's variance 0.9733 (L/3)²; end draws ±L evenly;
      a decentred ideal lens moves the focus by the decentre (10⁻⁹) and the RSS and
      ranking follow; a fold mirror tilted θ turns the beam 2θ (10⁻⁹); radius,
      thickness, index and relative-radius errors of a thick singlet move the
      compensated detector by the thick-lens formula's change of the back focus
      (2 µm, best against paraxial focus of a 1 mm beam); a despace and a focal-length
      error taken out by the detector compensator (to the optimiser's reach on the RMS
      radius's V-shaped minimum, < 1 µm); refusals

## M14b Monte Carlo and yield

- [x] Trials (`tolerance::trial`): every error drawn from the trial's own stream
      (`Random::stream(seed, index)`), applied, compensated, measured; a trial that cannot
      be evaluated (no light on a detector, a thickness reaching 0) fails, with the reason
- [x] `tolerance::summarise`: per criterion mean, standard deviation, the 50th to 99th
      percentiles, the largest, its passing share; the yield and its binomial standard
      error √(p (1 − p) / n)
- [x] Trials in parallel (command `tolerance`, `mode = "monte-carlo"`, threads in the
      command layer pulling trial numbers; the engine single-threaded)
- [x] Sensitivity from the Monte Carlo: each tolerance's first-order share of each
      criterion's variance, Var(E[y | x]) / Var(y) (Pearson's correlation ratio over
      quantile bins, less its bias under independence), which sees symmetric effects a
      linear regression misses; the ranking by share
- [x] Checks: the share estimator on y = a + b/2 (0.8, 0.2), y = a² (all of it) and an
      unrelated c (none); two uniform decentres of ±L against the triangular difference:
      E|X| = 2L/3 (within 4 standard errors), the yield 3/4 at |X| ≤ L (within 4 binomial
      errors), each share exactly 1/10 (E[|X| | u] = (1 + u²)/2: Var 1/45 against 2/9);
      the same results for 1 and 4 threads, other results for another seed, the larger
      decentre first for four seeds

## M14c Report, graphs and the exit check

- [x] Command `tolerance` (`mode` `sensitivity` or `monte-carlo`; `trials`, `seed`,
      `threads`, `max_iterations`) and its report next to the design with `report`
      (`<design>.tolerance.json`, without run times, and `.tolerance.md`: tolerances,
      criteria, compensators, the sensitivity table or the Monte Carlo's statistics,
      yield and shares by rank); the Monte Carlo result holds every trial's values
- [x] Graphs: `tolerance-histogram` (a criterion's distribution with its pass limit
      marked), `tolerance-sensitivity` (changes, or shares, in rank order as bars with the
      tolerances' names), `tolerance-yield` (the running yield and its ±2σ band);
      `criterion` picks the criterion
- [x] Ranking by the shares of each criterion's residual vector (its components'
      first-order shares weighted by their variances), not of its magnitude: the
      magnitude √(a² + b²) mixes the errors (on the periscope the first-order shares of
      the pointing magnitude added up to 0.1 and its ranking changed with the seed),
      while the components are near linear in them
- [x] Exit check (commands `tests/tolerance.rs`): on the golden periscope (the M6 STEP
      sidecar with setting errors of its five adjusters, the decentres of both lenses and
      a tilt of the first; centring, pointing and collimation), three seeds of 600
      trials: the leading tolerance of every criterion is the same (M1's setting about y,
      L1's decentre, L2's height: as one at a time), and every two tolerances whose mean
      shares differ by more than 0.05 keep their order in every run. At 2000 trials and
      five seeds only tolerances within 0.01 of each other in share swapped. On the local
      Thorlabs periscope (eight tolerances), 200 trials and three seeds: the same leaders
      for centring (Mb_tip setting, share 0.35–0.45) and collimation (L2_z setting,
      0.95); pointing's two leaders, tied at 0.26, swap
- [x] Runtime (release, Intel Core i7-8750H, 6 cores, 12 threads, 2026-11-13): golden
      periscope 8.3 ms per trial on one thread, 2.2 ms on four (500 trials: 4.2 s and
      1.1 s), 2000 trials in 3.1–3.4 s on 12; its sensitivity analysis (17 evaluations)
      0.15 s. Local Thorlabs periscope (165 solids): 200 trials in 13–15 s on 12 threads
      (about 0.75 s of trace per trial). A trial with compensators costs one trace per
      optimiser evaluation (about 10–20 per trial)

## Open questions

- Part tolerances on STEP solids (a lens body from CAD): its prescription is not
  modelled, only its faces; tolerancing them needs the library prescription (M5c1) or is
  left out.
- Whether to add more metrics than the merit terms (PSF Strehl, coupling efficiency into
  a fibre from M10) as criteria.
