# M12 Live read-only

Started 2026-11-20, when the user moved on to the live link. SolidCraft's plugin system
is out, but the user keeps a plugin implementation separate while it settles; M12 uses
SolidCraft's control channel, as Phase B was designed (`docs/design.md`, Phase B).

Exit criteria (`docs/design.md`, Milestones): re-trace within 2 s of a design change on a
golden scene; refuse to run on a schema mismatch.

The channel as SolidCraft documents it (`docs/control-protocol.md` in its repository,
MIT OR Apache-2.0, read 2026-11-20 for SolidCraft 0.45.0), with the schemas its
`engine.describe` gives: plain TCP on 127.0.0.1, one JSON object per line; requests
`{id, method, params}`, the first on a connection with the launch's `token`; replies
`{id, ok, result | error}`; events without an `id`, `design.changed` after
`control.subscribe`. The app listens only while Preferences › Assistant › Let AI agents
connect is on.

## M12a Finding and talking to SolidCraft

- [x] `optobench-bridge::discovery`: the control folder (`SOLIDCRAFT_CONTROL_DIR`, else
      the platform's: macOS `~/Library/Application Support/SolidCraft/control`, Linux
      `$XDG_DATA_HOME/solidcraft/control`, Windows `%LOCALAPPDATA%\SolidCraft\control`),
      its `<port>.token` files tried `current.port` first, then newest first, until one
      accepts a connection (a force-quit app leaves its files); or
      `SOLIDCRAFT_CONTROL_PORT` with `SOLIDCRAFT_CONTROL_TOKEN`. The token is never shown:
      an endpoint prints its port only, and errors never carry it
- [x] `connection`: requests by id, the token on the first only; events kept in order
      between replies; a line cut by a timeout completed by the next read; SolidCraft's
      unauthorized, busy, still-running and refused replies told apart
- [x] `contract`: before anything is read, every command M12 uses
      (`document.revision`, `component.list`, `joint.list`, `parameters.list`,
      `file.export_step`) must have a typed contract whose schema still has each field
      optobench reads; otherwise the link is refused, the differences named

## M12b Reading the design

- [x] `read::snapshot`: the design's id and revision; every occurrence by its path of ids
      from the design, with its placement in its parent and in the world (composed down
      the component tree, so a component placed twice has both); the joints with their
      type, ends, solved values and limits; the parameters with their expressions,
      values and units. Read again when the revision moved while reading
- [x] `read::export_step`: the whole design as a STEP assembly into a folder the caller
      gives, the only folder the request lets SolidCraft write in. Nothing else writes;
      the design is not changed

## M12c Tracing again on each change

- [x] `trace` (and `view`, `optimize`, `graph`) take `step`: a sidecar's assembly read
      from that STEP file instead of the one it names, so the user's STEP file is never
      overwritten
- [x] `optobench-bridge::live::Live`: connects, checks the contract, subscribes, then
      for the design as it is and after each change (waiting 150 ms for it to settle:
      SolidCraft sends up to ten changes a second while something is dragged) reads it,
      exports it into the session's own scratch folder and traces the sidecar against
      the export through the command registry (`optobench-commands` knows nothing of
      SolidCraft). Each update gives the revision, the seconds from the change noticed
      to the trace done, the design read and the trace
- [x] `optobench live {"path": "<sidecar>"}`: one JSON line per trace (revision, seconds,
      ledger, clipping, each detector's power and spot; the whole update with
      `"full": true`), until the connection ends; `"changes": n` stops after n changes
      (0: the design as it is, once)

## M12d In the window

- [x] Live (CAD, SCENE › SOLIDCRAFT), for a sidecar: on, the link traces the design open
      in SolidCraft and the view reads the parts from its export, again after each change
      (the runs strip says "SolidCraft revision n"; the status the revision and the time
      the trace took); edits to the sidecar while Live is on are traced against the
      export too. Off, or when the link ends (with its reason), the sidecar's own STEP
      file again
- [x] The application's icon: the title bar's mark (a lens outline with the axis
      through it) on a dark rounded square, the axis as the beam in the view's ray red;
      drawn in code at any size

## M12e A move traced without exporting again

Asked for by the user (2026-11-20) after the first runs against SolidCraft: on the
Thorlabs expander periscope each update took 8–15 s, most of it SolidCraft's STEP export
of the whole design (21 MB, 4.6 s and more) and optobench reading and converting it
(1.4 s; the trace itself 0.07 s).

- [x] A geometry fingerprint with every snapshot (`Snapshot::geometry`): the component
      tree without its placements, the bodies with their features and visibility and the
      timeline (`document.inspect` without measurements: 4 ms, where measuring takes 30 s
      on that assembly), the parameters' expressions and values. `document.inspect`'s
      contract is checked with the others
- [x] When the fingerprint is the latest export's, only placements changed: that export
      is traced with each occurrence whose world placement changed moved by
      `now · then⁻¹` (its path of names, as the STEP export names occurrences), and
      nothing is exported. Otherwise, or when an occurrence is new, the design is exported
      again. Each update says which (`geometry`: `moved` or `exported`, with its `moves`)
- [x] `trace`, `view`, `optimize` and `graph` take `moves`: `[{ "occurrence": "<names
      joined by />", "matrix": [[4 × 4]] }]`, rigid moves in the assembly's frame applied
      after the STEP file's placements, each body (and each of its faces, which carry
      their own placements) taking the move of the deepest occurrence it is in; an
      occurrence the file does not have, or a matrix that is not a rotation and a
      translation, refused
- [x] The STEP files read lately are kept converted in memory (the last two, by path,
      size and time of change), so a fast-path trace, and every view and trace of an
      unchanged file in the window, skips reading and converting it
- [x] The window keeps an update's `moves` with its export, so the view and the traces
      after edits place the parts as SolidCraft has them
- [x] Checks: the golden relay's STEP file with the moves its moved re-export was made
      with (`pc-lens:2` and `cage-plate:2` 1 mm along z) traces as the moved export does,
      to 1e-12 mm plus 1e-9 relative (the export's coordinates went through STEP's
      text); a missing occurrence and a stretch refused; against the stand-in, the same
      two moves in "SolidCraft" traced with no export, as the moved export traces, in
      under 2 s, and a feature added afterwards exported again; the fingerprint the same
      after a move and changed by a feature; in the window, parts moved back are placed by
      two moves

## Checks

Against a stand-in for SolidCraft (`optobench_bridge::fake`: the protocol as documented,
the schemas cut down to the fields optobench reads; CI has no SolidCraft):

- [x] Discovery: `current.port` first, then newest; stale token files and an empty folder
      found as such; the token never in a printed endpoint or an error
- [x] Placements composed down a tree (a quarter turn and an offset); a placed component
      that is not listed is a protocol error
- [x] The design found by its control folder, checked and read (occurrences, world
      placements, a slider's limits, a parameter), with nothing but reads asked for; a
      schema without `transform` refused with the difference named; a wrong token
      refused; three quick changes taken as one; the STEP export written where asked
- [x] Exit (`crates/optobench-bridge/tests/live.rs`): the golden cage relay's sidecar
      traced against the "open" design gives the trace of its own STEP file; after a lens
      moved, traced again in under 2 s from the change, the same as tracing the moved
      relay's STEP file; a changed contract refused having asked for nothing but the
      contract; a layout refused
- [x] `optobench live` prints the trace, then the trace after a change, and ends after
      `"changes": 1`; in the window (`live_traces_the_design_open_in_solidcraft`), Live on
      traces, a change is traced again and shows, Live off is back to the sidecar's file
- [x] Against SolidCraft 0.45.0 itself (2026-11-20, the Thorlabs beam-expander periscope
      open, 22 occurrences): the first run was refused by the contract check, rightly:
      `engine.describe` is an engine command run through `engine.execute`, not a method
      of its own as read from the documentation; fixed, and the stand-in now answers it
      the same way. Then the design read (occurrences, 6 parameters, no joints) in a few
      milliseconds, exported, and the corpus sidecar `expander-periscope.optobench`
      traced against it: 99.9 % of the laser on `out`, the ledger balanced to 6e-16
- [x] Time per update on that assembly: 8–15 s at first, of which SolidCraft's STEP export
      of the whole design (21 MB) takes 4.6 s and more, and reading and converting it
      1.4 s; hence M12e. With it (2026-11-20): L1 and its mount moved 5 mm along z in
      SolidCraft traced in 0.17 s with no export, the result on `out` the same to every
      digit printed (centroid (0.7736065, −0.0020996) mm, RMS radius 0.0650859 mm,
      0.0998973 W) as a fresh export of that state traced the ordinary way. A change that
      exports takes about 3.5 s
- [x] Found on the way: SolidCraft writes placements as the columns of the matrix (the
      translation in the last row); the reader and the stand-in had assumed rows. Some
      earlier moves in that session left `MoveN` features in the design's root component
      (how they were made is not known); a change of features counts as a change of
      geometry and is exported again, with the reason given

## Next (M13, write-back)

DOF bindings (parameter, joint, move) from what M12 reads: a DOF named after a user
parameter, a joint's value, or an occurrence's placement; batched writes, one rebuild,
read back.
