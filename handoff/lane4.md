# LANE 4 handoff - town01 south gate

## What the gate actually is

Read record 10's bytecode, as the thread asked. It is **five bytes**:

```
town01 P2[10]  (start=0x09CF7 pc0=16 body=21b)
  +0x10  21        Nop
  +0x11  21        Nop
  +0x12  26 FE FF  JmpRel delta=0xFFFE -> +0x11
```

No `0x3F`, no `0x47`/`0xC7` walk, no writes. It is a resident park - exactly
the shape the choreography-wrap rule exists for. So:

- **The opcode that terminates the timeline is `0x26` JmpRel at body `+0x12`
  (MAN offset `0x09D09`), and the wrap rule is correct to end it there.**
- The `+8` nudge is one frame of ordinary pad locomotion, not a force-walk.
  There is no force-walk leg in the record to model.
- **The thread's next-step hypothesis is falsified.** Nothing in the timeline
  runner, the dispatch, the standoff or the grid was owed.

The exit is `P2[0]` on tiles `(24..26, 46)`, gates `C1=[] C2=[]` - ungated.
What seals it is the collision grid, and the grid cell is the **gate**, opened
by `town01` `P0[20]`: the gate object's own record, bound by the `.MAP` gate-0
kind-1 trigger at tile `(23, 43)` and executed by the scene-init bind prologue
(`FUN_8003A55C`). Three unconditional `4C 70` clears, then a branch on system
flags `327` / `321`; only the both-set arm runs
`4C 70 18 2D 19 2E` (cols `24..25`, rows `46..47`) and cuts row 47.

**The port already executes all of this correctly.** Measured on the loaded
grid: cold = sealed, `327` only = re-blocked further north, `327`+`321` =
doorway open at cols 24/25 with col 26 re-blocked. Pinned by the new
`crates/engine-core/tests/south_gate_disc.rs`.

## Out-of-scope notes (do not have a home in Lane 4's paths)

### 1. `docs/subsystems/world-map.md` attributes the gate script to the wrong record for `town01`

The page's "Rim Elm's south gate is a story gate" paragraph says the paints
live in **`town0c` `P1[0]`**. That is true for `town0c`, which carries the
sequence *twice* (its entry script `P1[0]` at MAN `0x00c9b..` **and** `P0[20]`
at `0x0077e..`). `town01` carries it **only** in `P0[20]` - there is no copy in
its entry script. Suggested amendment: name `P0[20]` as the carrier and say the
entry-script copy is a `town0c`/`town0b` rendition detail.

This matters for a port: an engine that applies nibble-7 deltas only from the
entry script leaves `town01`'s gate sealed in every story state. (I added this
as a short subsection in `docs/subsystems/script-vm.md`, which is in scope, and
linked back to `world-map.md`.)

### 2. Rung 3 of `critical_path_replay` - the overworld walk goes the wrong way in Z

With rung 2 cleared, rung 3 now runs and fails for reasons that are **not** the
south gate. Two harness defects were in my file and are fixed:

- `portal_tile` returned `world_map_entity_positions` (world units) where
  `walk_to` wants a **tile**; `tile_center` then overflowed `t * 128` on `i16`.
- The follower inverted the *field* remap on the overworld. The overworld
  walk uses `world_map_camera_relative_bits`. Fixed via `pad_for_step`, with a
  disc-free unit test that checks the inversion against the engine's own
  forward remap.

**The residual is engine-side and outside Lane 4's paths.** After both fixes,
starting from the correct arrival (`mode WorldMap`, world `(12352, 3264)` =
tile `(96, 25)`, `world_map_ctrl.azimuth == 0`) and steering at tile
`(64, 68)`, the player still ends at world `(0, -825)` = tile `(-1, -7)`: X
moves the right way and overshoots, **Z moves the wrong way**. At azimuth 0 the
inversion is exact (`Left` should be world `Z+`), and the unit test proves the
harness asks for the right press - so the suspect is the consumer of the
direction bits in `World::step_world_map_locomotion`
(`crates/engine-core/src/world/worldmap.rs`, the `0x1000` = Z+ / `0x4000` = Z-
application around line 210) or the sign convention in
`world_map_camera_relative_bits` itself
(`crates/engine-core/src/world/config.rs`). Neither file is Lane 4's.

Note the asymmetry is the useful clue: a wrong *azimuth* would rotate both
axes; only a sign error on one axis reproduces "X right, Z inverted".

### 3. `crates/engine-core/src/world/state.rs` - `seed_free_roam_story_baseline`

The free-roam picker baseline seeds `562` for `town01`. `562` is `P2[10]`'s C2
gate - it enables the **content-free park**, not the door, and installing that
park as a modal cutscene timeline steals a frame or two of control from a
player walking the gate. If the picker is meant to hand a player a usable Rim
Elm, the flags it wants are `327` + `321`. LANE 5 owns that file.
