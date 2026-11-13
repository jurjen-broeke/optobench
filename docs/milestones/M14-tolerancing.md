# M14 Tolerancing

Exit criterion (docs/design.md, milestone table): the sensitivity ranking is stable
across seeds; the runtime is documented.

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

- [ ] Command `tolerance` (sensitivity or Monte Carlo; trials, seed, threads) and a
      report next to the design (`<design>.tolerance.json` and `.md`): tolerances, the
      sensitivity table, distributions, yield, the ranking
- [ ] Graphs: histograms of each criterion with its pass limit, the sensitivity ranking
      as bars, the yield against the number of trials
- [ ] Exit check: on the golden periscope (and the local Thorlabs one), the ranking of
      the top tolerances is the same for several seeds; the runtime per trial and for a
      full run is measured and written down (as M7's benchmarks are)

## Open questions

- Part tolerances on STEP solids (a lens body from CAD): its prescription is not
  modelled, only its faces; tolerancing them needs the library prescription (M5c1) or is
  left out.
- Whether to add more metrics than the merit terms (PSF Strehl, coupling efficiency into
  a fibre from M10) as criteria.
