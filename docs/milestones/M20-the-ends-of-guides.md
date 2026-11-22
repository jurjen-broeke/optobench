# M20 The ends of guides

Started 2026-11-22, after M19, asked for by the user (the third of the follow-ups after
M18): a rough placement along rods stops where the part would leave them, not only
where it would overlap another part.

Exit criterion: in the window, on the golden cage relay with cage plate 2 moving with
L2 on a free slide along Z, Find the space stops the plate at the rods' ends; on the
Thorlabs periscope the ER rods are found as the CXY1A's guides and nothing else is.

## M20a Guides and their ends

- [x] `Scene::clearance` on a slide: a stationary part whose surface runs alongside a
      moving part's (within 6° of the slide, within `guide_gap`, 0.5 mm by default) and
      holds it from opposing sides guides it; of the two surfaces in contact the shorter
      stays within the longer (a plate on its rods, a post in its holder), which limits
      the slide each way; `Stop { part, end }` says what stops it and whether that is the
      end of a guide
- [x] Not on a tilt (a tilting part meets its rods first), not for a flat face a part
      rests on or touches from one side
- [x] `clearance` command: `guide_gap` parameter, `end_of_guide` per DOF; the window
      shows "at the end of <part>"

## M20b Stand-ins' faces

- [x] The layout stand-ins' loops run as `BrepFace::contains` expects (a face's inside
      to the left of its boundary seen along its normal): a cylinder's upper circle was
      the wrong way round (its upper part missing from the clearance's points) and a
      bore's two (a tube's bore and an iris plate's hole were not there at all)

## M20c Opening and saving by dialog

- [x] From the user's test (2026-11-22: Open showed no dialog, and a typed path was
      cumbersome and easy to mistype): Open, ⌘O / Ctrl+O and New use the system's file
      dialogs (`rfd`, MIT), starting in the open design's folder; New adds `.optobench`
      and replaces a file the dialog confirmed; the title bar's path field is gone; the
      data directory has a folder button

## Checks

- [x] Scene (`clearance.rs`): a Ø12.7 post from z −20 to 80 in a holder's Ø12.8 bore from
      0 to 30 goes 20 mm up and 50 mm down, both at the end of the holder; a block
      0.02 mm above a plate slides along it with nothing stopping it; a stand-in cylinder
      and bore are whole and met from either side
- [x] Golden relay (`a_free_slide_stops_where_its_parts_would_overlap`): L2 and its plate
      go up 56.5 mm, the plate's top (188.5) to the rods' ends (245), at the end of a rod;
      in the window, "+56.50 mm (at the end of rod:1/Rod)"
- [x] Layout (`a_free_slide_in_a_layout_stays_in_its_tube`): a lens in a tube with 0.1 mm
      of play goes 31 mm on and 25 mm back, at the tube's ends
- [x] Thorlabs periscope (corpus, command line): the CXY1A's two bodies are guided by
      `er10:1`, `3`, `5` and `7` (ends 159 to 162 mm up, 79.5 to 92 mm down: a 254 mm
      rod), nothing else; parts in the way stop it first (−30.93, +36.29 mm, unchanged);
      2 s more on a 13 s search
- [x] Window (`open_and_new_ask_where_in_a_file_dialog`, the dialog answered by the
      test): a cancelled New makes nothing, New adds the extension and replaces a file,
      Open and the shortcut open what was chosen, a cancelled Open keeps the design
- [ ] Exit, by the user
