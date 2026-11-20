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
      frame (from the file's values), and for bound DOFs the parameter and joint changes;
      sources and detectors of a sidecar, which are not CAD parts, left to the file
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

- [ ] `Live::write`: DOF values written to the design open in SolidCraft, then the
      written DOFs' values in the sidecar set back to 0 (the design now has the move);
      the next trace follows from the change
- [ ] `optobench write-back {"path", "dofs"}`, and the window's Apply in SolidCraft after
      Optimise with Live on, asked first

## M13d The contract test

- [ ] A test of every command M12 and M13 send, on a design built for it, run against an
      installed SolidCraft (`solidcraft-cli run`); nightly in CI against the latest
      release
