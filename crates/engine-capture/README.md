# legaia-engine-capture

Standing the engine where a retail capture stood.

A retail save state is a frame taken somewhere inside a script, a fight or a
camera move. Comparing the engine against it means putting the engine at the
same phase, and two processes do that: the headless comparison in
[`legaia-parity`](../parity/README.md), which reads the phase out of the
state's RAM, and the `play-window` image child it launches, which reads the
same phase back from its environment
([`docs/tooling/retail-compare.md`](../../docs/tooling/retail-compare.md)).
This crate is the vocabulary the two share: the seed records, the gates the
engine is run to, the pad drives that reach them, and each one's environment
form in both directions.

- [`phase_gate`](src/phase_gate.rs) - `PhaseGate`, the point in an in-flight
  battle action a capture sits at (`LEGAIA_CAPTURE_GATE`).
- [`battle_drive`](src/battle_drive.rs) - `BattleDrive`, the pad path that
  reaches a battle capture past the round prompt (`LEGAIA_BATTLE_DRIVE`),
  with its `SpanGate` and `ActionSteer`.
- [`script_gate`](src/script_gate.rs) - `ScriptGate`, a field script's phase
  (`LEGAIA_SCRIPT_GATE`), the drive that resumes its record and pages its
  dialog boxes, and the engine's own field contexts it is matched against.
- [`bar_seeds`](src/bar_seeds.rs), [`hud_glide`](src/hud_glide.rs),
  [`camera_align`](src/camera_align.rs) - a fight's HP / MP / status bars,
  HUD glides and camera tween at the capture instant.
- [`formation_cell`](src/formation_cell.rs) - `FormationCell`, a fight named
  by its formation cell where no scene MAN row carries it
  (`play-window --battle cell:[s:]ids`), registered the same way on both sides.
- [`field_seeds`](src/field_seeds.rs) - the field-side seeds: placed-object
  clips, ambient walkers, morph envelopes, the fog pool, sprite arms,
  CLUT-cell cyclers, scroller rects, the panel widget and the camera block.
- [`clip_cursors`](src/clip_cursors.rs) - `ClipCursorSeed`, where each
  combatant's playing clip stood in the capture (the anim node's cursor
  `+0x68`), so a seated fight draws its combatants at retail's keyframe.
- [`ghost_ring`](src/ghost_ring.rs) - `GhostRingSeed`, a party seat's
  after-image ghosts as the capture's history ring held them
  (`LEGAIA_SEAT_GHOST_RING`), seated on the replay's ring on the capture
  frame.
- [`ground_snap`](src/ground_snap.rs) - the combatants' ground positions at
  the captured phase (`LEGAIA_BATTLE_GROUND_SNAP`), which a replayed push or
  approach lands near rather than on.
- [`melee_hits`](src/melee_hits.rs) - the hits a captured melee action had
  already landed, replayed at retail's amounts (`LEGAIA_BATTLE_MELEE_HITS`).
- [`cast_parts`](src/cast_parts.rs) - a captured cast's live effect parts
  (`LEGAIA_SEAT_CAST_PARTS`), seated over the replay's own parts on the
  capture frame.

## Layering

Engine-side only. It sits above `engine-core` and the kernel crates and
below the hosts: `engine-shell` reads the seeds from the environment and
applies them in its capture path, `legaia-parity` builds them.

It must not depend on a save-state reader (`legaia-mednafen`,
`legaia-pcsx-redux`), on `legaia-ram-image`, on the renderer, or on
`legaia-parity`. Nothing here takes a RAM image: deriving a seed from retail
RAM is `legaia-parity`'s, which is what lets a play host link this crate
without linking the oracles.
