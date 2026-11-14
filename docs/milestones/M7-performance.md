# M7 Performance

Exit criterion: the local Thorlabs beam-expander periscope (165 solids, 6132 exact faces,
13 MB STEP; 7 DOFs) optimised in under 2 minutes (about 25 at the end of M6), with trace
results unchanged to 1e-12.

## M7a Where the time goes, and the inside test

Measured with `examples/trace_timing` and macOS `sample`: of 3.4 s per trace, 91 % went
to `BrepFace::contains`, the test whether a hit on a face's surface lies inside its
trimmed boundary, which scanned every boundary segment for the nearest one. A trace made
about 90 000 such tests (65 per ray: rays pass close to many mount faces, threads and
holes), on faces with up to thousands of sampled boundary points.

- [x] Segment tree: each face's boundary segments in a bounding-volume tree, so the
      nearest segment is found by branch and bound; the side test runs once, for the
      winner. The answer is the one the scan gave, ties included.
- [x] Plane grid: flat faces (most mount faces) decide inside or outside by counting
      crossings with the edges in the point's row of a grid in the face's plane; within
      1e-6 mm of an edge the exact nearest-segment test decides, so edges classify as before.
- [x] Distance-limited intersection: the BVH tells each face the nearest hit so far;
      a face tests its surface hits nearest first and skips those beyond it.
- [x] Checks: the tree against the scan (5000 points), the grid against crossings over all
      edges, `contains` against the old method on a flat face with holes and a curved
      band; whole-trace reports of the three corpus assemblies (expander periscope,
      Thorlabs cage relay, iris cage) unchanged.

Result on the expander periscope: a trace takes 0.40 s instead of 3.4 s (8.6×); its
optimisation 50 s instead of about 25 minutes. Reading the STEP file and converting its
faces takes 1.4 s, once. **The exit criterion is met.**

## Further candidates (not needed for the exit criterion)
- [x] Rays in parallel (2026-11-14): `NonSequentialSystem::threads` (from `Scene::threads`,
      `[trace] threads`; default 0, every core the platform reports) traces the launched
      rays on the standard library's scoped threads, no new dependency: chunks handed out
      as threads come free, each ray's tree traced whole, the trees put back in the rays'
      order and booked into the ledger in that order, so records and ledger are the same
      bit for bit on any number of threads (`threads_give_the_same_trace_bit_for_bit`, and
      `examples/trace_timing` compares the two traces). Fewer than 32 rays a thread use
      fewer threads; wasm32 traces on the calling thread. Tolerancing trials, already on
      threads, trace on one each. On the expander periscope (Intel Core i7-8750H, 6 cores,
      12 threads, release): 0.377 s a trace on one thread, 0.065 s on 12 (5.8×); its
      optimisation 309 s on one thread, 64 s on 12 (4.9×).
      The optimisation's 309 s is not a regression: built from the M7a commit the same
      design takes 300 s, 100 iterations and 787 traces, the cost the same (it stops at
      the iteration limit with 0.09 mrad of collimation left). The 50 s above is not
      reproduced with the design as it now stands in the local corpus; fewer traces per
      iteration and a stopping rule for such slow tails are the next item
- [ ] Fewer traces per optimiser iteration (forward differences, reusing the Jacobian
      across rejected steps)
- [ ] Cache converted faces per STEP file and product; seed B-spline intersection from a
      coarse mesh instead of a fixed 7 × 7 grid
