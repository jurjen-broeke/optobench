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
- [ ] Against SolidCraft itself, by the user: Let AI agents connect on, the golden cage
      relay open, `optobench live` on its sidecar, a lens moved

## Next (M13, write-back)

DOF bindings (parameter, joint, move) from what M12 reads: a DOF named after a user
parameter, a joint's value, or an occurrence's placement; batched writes, one rebuild,
read back.
