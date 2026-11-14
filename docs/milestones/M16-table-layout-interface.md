# M16 Table layouts in the interface

Started 2026-11-14 from the user's own test: building a beam path on a breadboard by
hand in the interface (a source at a height, a target, mirrors on kinematic mounts,
alignment irises), to find what the interface lacks. These are the first five findings;
the rest of that test follows on this page.

Exit criterion: a folded beam path built on a breadboard in the interface alone, with
every item's position readable and movable by its handles, and the bench advice on where
the alignment irises go.

## M16a Positions in Properties

- [x] Each entry's position as X, Y and Z in mm at the top of Properties: on a table
      layout typed and written back as its hole (fractions kept) and height; on planar
      layouts and sidecars as the scene placed it, to read
- [x] Vector fields name their numbers (x y z, column row, width height) and the main keys
      show their units; on a table layout the height is the position's Z
- [x] `view` gives each layout item's origin and axis, and the file's `[table]` and
      `[planar]`

## M16b The breadboard

- [x] `[table]` takes an optional breadboard: `columns` and `rows` (holes along x and y,
      together) and `corner` (the hole at its corner nearest the origin, default [0, 0]);
      its edge lies half a pitch beyond its outer holes. Only the interface uses it
- [x] Drawn under the items: a plate with its holes as filled circles (an M6 hole for the
      pitch), sized by the file or fitted around the items with two holes to spare; Fit
      and the view's depth take it in
- [x] Each item and source stands on a post down to the board, with a ring where it
      stands: its hole and height at a glance
- [x] The browser lists the Breadboard (show, hide, select); its Properties set the
      pitch, the size in holes and the corner hole, or keep the fitted size
- [x] `edit` sets keys in `[table]`, `[planar]` and `[trace]` (`section`), making the
      section when it is missing

## M16c Graphs

Decided with the user (2026-11-14):

- [x] Graph asks what to draw: tick any number of kinds, Draw draws them all at once
- [x] One graph shown at a time, at half the strip's width; the list above it switches
      between the graphs open (one per kind; drawing a kind again replaces it); it never
      runs anything
- [x] A graph drawn before the design's last edit carries the reload icon with a warning
      dot ("out of date" in the list); Redraw draws the shown graph again, × closes it
- [ ] Side-by-side comparison of two graphs (later, if wanted)

## M16d Move

- [x] Move (Layout, MODIFY), as SolidCraft's: with an item or source of a table layout
      selected, arrows along the scene's x, y and z and rings about them at the item; a
      drag that starts on a handle moves the item instead of turning the view
- [x] Snapped to a step: one pitch to start (Pitch sets it again), or typed; turns to
      15° or what is typed; 0 for none. The item's outline is previewed where it will go,
      with the amount; letting go writes its hole, height or axis (one edit, undone by
      Undo)
- [x] Checks: moves write holes (fractions kept), heights and turned axes; in the
      window, the periscope's screen dragged by its x arrow lands two holes along, and a
      quarter turn of its z ring turns its axis from +x to +y

## M16e Alignment irises

Decided with the user (2026-11-14): optimisation aligns on the target detector (the beam
centred on it, `centring`, with `pointing` for its direction); where to put irises on the
bench for the fine alignment is an output, not part of the design.

- [x] `trace` on a table layout gives `alignment_irises`: along the central ray of the
      first source, the last straight run before the detector (from, to, length,
      direction), the two holes on it furthest apart that keep 1.5 pitches clear of the
      mounts at its ends, at the beam's height there (one when only one fits, none on a
      short run), or on a run between holes a third and two thirds along, by their
      nearest holes; the mirrors that steer through them and the advice (close them down,
      the earlier mirror onto the near iris, the later onto the far one, repeat)
- [x] Shown in the view as rings across the beam on posts (I1, I2) and listed with the
      advice under Detectors
- [x] Checks (commands `tests/irises.rs`, from the layout's geometry): a beam folded by
      two 45° mirrors gets irises at holes (8, 8) and (18, 8), 250 mm apart, steered by
      M1 then M2; a diagonal run through holes gets them on (8, 10) and (14, 16), clear of
      its ends; a run along (1, 0.3), 0.1 holes or more from every hole, gets them off the
      grid by (9, 9) and (13, 10)

## Next, from the same test

Noted while reading the code before the user's test; not yet confirmed by it:

- Click a hole to place a new item there (items are added two holes after the last)
- A mirror's normal worked out from where the beam comes from and should go
- Add for proxies (irises as plates, tubes) and `plate` among a proxy's kinds
- `mount = …` (and the library's mounts) set from Properties
