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

- [ ] Tolerances in layouts and sidecars (`[[tolerance]]`), each with a name, a target
      (an element, source, detector or, in a sidecar, a CAD occurrence, as DOFs name
      them) and a limit:
      - placement (mount) errors: `slide` along an axis (mm) and `tilt` about an axis
        through a pivot (degrees), as DOF motions but never adjusted; on a DOF itself:
        an error added to its value (`dof = "<name>"`), for an adjustment that cannot be
        set exactly
      - part errors of lens elements: `radius` (of a surface, mm or a fraction), `thickness`
        (mm), `index` (Δn) and `abbe` later if needed; of an ideal lens: `focal_length`
      - distribution: `uniform` (default), `normal` (σ = limit / 3, truncated at the
        limit) or `end` (±limit, equal chances)
- [ ] Criteria: the merit terms of M6 (focus, centring, pointing, collimation, power at a
      detector) with a pass limit each (`[[criterion]]`); compensators: DOFs named in
      `[tolerancing] compensators = [...]`, re-optimised on the file's `[[merit]]`
- [ ] A seeded random generator in the engine, no new dependency: xoshiro256** seeded by
      SplitMix64 (D. Blackman and S. Vigna, "Scrambled linear pseudorandom number
      generators", ACM Trans. Math. Softw. 47, 36 (2021); written from the paper), normal
      deviates by Box–Muller; the same seed gives the same trials on every platform
- [ ] Applying a trial to a scene (`optobench_optimize::tolerance`): placement errors as
      moves of the target on top of its DOFs, part errors on the element's prescription;
      the nominal scene unchanged
- [ ] Sensitivity analysis: each tolerance alone at ±limit (with and without
      compensation), the change of every criterion, its RSS estimate and the ranking
- [ ] Checks: the generator's statistics (mean, variance, a χ² test of uniformity, the
      normal's moments and truncation); a decentred ideal lens moves the focus by the
      decentre, a tilted fold mirror turns the beam by twice its tilt, a lens thickness
      and radius change moves the paraxial focus as the lensmaker's formula says; a
      compensator (a focus slide) takes out a despace exactly

## M14b Monte Carlo and yield

- [ ] Trials: draw, apply, compensate, measure; per criterion the distribution (mean,
      standard deviation, percentiles) and the yield; failures of a trial (a detector
      the light misses) counted as failing, with the reason
- [ ] Trials in parallel on desktop (threads in the command layer; the engine stays
      single-threaded and builds for wasm32); the same results whatever the number of
      threads (each trial's draws from its own stream)
- [ ] Sensitivity from the Monte Carlo itself: each tolerance's share of each
      criterion's variance (squared standardised regression coefficients, or rank
      correlation where the response is not linear), and the ranking
- [ ] Checks: a linear case with a closed form (a centroid that is the sum of uniform
      decentres: its variance Σ L²/3 and, for two, the triangular distribution's yield);
      the yield's sampling error within its binomial bound; identical results for 1 and
      several threads and for the same seed

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
