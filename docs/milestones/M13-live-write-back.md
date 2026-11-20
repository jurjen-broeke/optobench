# M13 Live write-back

Started 2026-11-20, after M12. The degrees of freedom an optimisation sets are made in
the design open in SolidCraft: by the parameter or joint a DOF is bound to, or by moving
its part; as one batch, read back, and taken back whole when anything is not where it
was asked to be (`docs/design.md`, Phase B, Write).

Exit criterion (`docs/design.md`, Milestones): a nightly contract test green against the
latest SolidCraft release for 4 weeks.

What SolidCraft does, checked before the design (SolidCraft 0.45.0, 2026-11-20, headless
`solidcraft-cli run` and the running app with read-only requests):

- a placement given as an expression follows the parameters it names
  (`occurrence.move` with `translate: [0, 0, "10 mm + L_z"]`; `parameters.change` of
  `L_z` to 3 mm puts the part at z 13)
- `joint.drive` sets a joint's value and moves the parts it holds; values outside its
  limits are clamped (`held`)
- `occurrence.move` with `space: "world"` and a row-major `matrix` moves a part from where
  it is, its expressions kept
- `edit.checkpoint` and `edit.restore_checkpoint` take the design back exactly, undo
  history included
- `engine.batch` (a control method) runs commands in order and answers
  `{completed, results, undone}`; `rollback: true` takes back those done when one fails

## M13a Bindings

- [x] A DOF's `binding`: `{ parameter = "<name>" }` (a user parameter, changed by the
      DOF's change, mm or degrees) or `{ joint = "<name>" }` (with `index` for a joint of
      several values); otherwise its part is moved. The DOFs of one part are all bound or
      none
- [x] `cad-moves`: for DOF values asked for, each CAD occurrence's move in the assembly
      frame, and for bound DOFs the parameter and joint changes; sources and detectors of
      a sidecar, which are not CAD parts, left to the file. Moves and changes are from 0:
      a DOF's value in the file is what optobench adds to the design, which stands where
      every DOF is 0 (found by M13c's test: from the file's values, a part at 0.5 mm in
      the file asked to 2 mm moved only 1.5 mm). A part whose DOFs are not asked for
      moves by the file's values, so the design comes to match the model
- [x] Checks (commands `tests/golden.rs`): on the golden relay, L2's unbound 2 mm slide
      is its occurrence moved 2 mm along z; L1's 1.5° tilt bound to a joint's second value
      is that joint changed by 1.5°; the screen's slide skipped; a part half bound, and a
      binding of neither kind, refused

## M13b Writing, reading back, taking back

- [x] `optobench-bridge::write`: a checkpoint; one batch of `parameters.change` (all
      parameters at once, one rebuild), `joint.drive` and `occurrence.move` (by numeric
      ids and paths); the design read back: every parameter and joint value, and every
      moved part's placement against the move asked for; any difference beyond the
      tolerance, or a failed command, restores the checkpoint and says what differed
- [x] The contracts of the commands written and read for it checked as M12's
- [x] Checks (bridge `tests/write.rs`, against the stand-in, which now keeps parameters
      and joints that move parts, world moves, checkpoints and batches): a part moved
      2 mm; a parameter and a joint set by their DOFs' changes, the parameters first and
      unchanged parts not written; a parameter that moves its part twice its change
      (`2 * L1_z`) caught by the read-back and taken back, the design as it was; a joint
      stopped at its limit taken back; an unknown joint, an unknown part and a changed
      write contract change nothing

## M13c Writing from the live link

- [x] `Live::write`: DOF values written to the design open in SolidCraft, then the
      written DOFs' values in the sidecar set back to 0 (the design now has the move);
      the next trace follows from the change
- [x] `optobench write-back {"path", "dofs"}`, and the window's Apply in SolidCraft after
      Optimise with Live on, asked first
- [x] Checks, against the stand-in: `Live::write` of L2_z = 2 (0.5 in the file) and a
      screen's slide = 1 moves L2 2 mm, sets L2_z to 0 and the screen's to 1 in the
      sidecar, and the next trace (no export) is the design traced with those values (all
      but the power by angle, whose bins jump at a ray on a bin's edge, to 1e-12 mm plus
      1e-9 relative); `optobench write-back` moves L2 and prints the trace after; in the
      window (`optimised_values_are_written_into_solidcraft`), Apply in SolidCraft asks
      first, writes, logs it, and the design is traced again

## M13d The contract test

- [x] `optobench-bridge` `tests/contract.rs` (ignored unless asked for, with
      `OPTOBENCH_SOLIDCRAFT_CLI`): a design built by `solidcraft-cli run` (a base, a lens
      placed at `10 mm + L_z`, a carriage on a slider `stage`), then every engine command
      M12 and M13 send, with the parameters the bridge sends: each command's
      `engine.describe` checked by `contract::differences`, the reads read by the
      bridge's readers, and on the saved design a checkpoint, a world move (the geometry
      fingerprint unchanged: the fast path's premise), `parameters.change`, `joint.drive`
      by the joint's id, the read-back of each, and the checkpoint restored
- [x] Passed against SolidCraft 0.45.0 (macOS, 2026-11-20). `engine.batch` and
      `control.subscribe` are methods of the running app's channel, which a script cannot
      call: checked by the stand-in's tests and against the app by hand
- [x] Nightly in CI (`.github/workflows/solidcraft-contract.yml`, and by hand from the
      Actions tab): the latest release's Linux build downloaded, checked against its
      `SHA256SUMS.txt`, and the contract test run with its `solidcraft-cli`
- [ ] Exit: green every night for 4 weeks (from the first run after this is merged)
