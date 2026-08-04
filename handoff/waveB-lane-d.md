# Wave B lane D - the third door consumer, and where an interaction really starts

## Headline

Two findings, one small and one that reframes a whole subsystem.

1. **The third consumer carried a sub-id too.** Measured, not assumed. It is
   now routed as a door warp, and the misnomer is gone from the type.
2. **The port has been entering every placement interaction at the wrong PC.**
   A placement record is two consecutive scripts under one cursor; the
   interaction begins past the spawn section's `0x21`, not at `script_pc0`.
   Entering at the start trips that terminator immediately and falls through to
   the record's first text segment - which for a casino cabinet is the line its
   **failed** coin check jumps to. That is why "the door never opened".

---

## Task 1 - what the third consumer really carries

### The chain, and where it ends

`SceneHost::enter_world_map_scene` classifies the overworld scene's partition-1
placements and turns each `PlacementKind::Portal { target_map }` into a
world-map entity config. The entity SM's transition arm
(`WorldMapEntityHostImpl::on_scene_transition`) turned that into
`FieldEvent::WorldMapTransition { target_map }`, and `SceneHost::tick` drained
it through `MapIdResolver::resolve(target_map as u8)` - the CDNAME-ordinal
table - and entered whatever scene sat at that ordinal.

`PlacementKind::Portal` is produced by exactly one expression,
`classify_placement`'s arm gated by `is_genuine_warp`: base `0x3E` (no `0x80`
cross-context prefix) with `op0` in `100..=106`, payload `op0 - 100`. That is
the mode-24 door-warp id space and nothing else. **Same bug, third arm.**

### Measured, because "same shape" is not evidence

A corpus walk of every CDNAME scene MAN finds **12** partition-1 placements
classified as `Portal`, and every one carries a `sub_id` in `0..=6`:

| scene | placements | sub_id | minigame |
|---|---|---|---|
| `koin1` | P1[9] | 5 | Muscle Dome |
| `koin1` | P1[51], P1[52], P1[53] | 4 | Baka Fighter |
| `koin1` | P1[54], P1[55], P1[56] | 3 | slot machine |
| `koin3` | P1[16] | 6 | dance |
| `balden`, `balden2` | P1[24] each | 3 | slot machine |
| `map02` | P1[7] | 0 | fishing |
| `map03` | P1[19] | 0 | fishing |

Only two of the twelve sit in a world-map scene: `map02` P1[7] and `map03`
P1[19], the overworld fishing signboards. So the third consumer's whole live
population is *the fishing doors*, and walking onto one resolved sub-id `0`
through the CDNAME ordinal and warped the player to an unrelated scene.
**(disc-measured)**

### What changed

- `WorldMapEntityConfig::Portal { target_map: u16 }` →
  `MinigameDoor { sub_id: u8 }`. The variant is now named for what it is, and
  the payload type narrowed to the id space it actually holds.
- `on_scene_transition`'s door arm runs `World::arm_minigame_warp()` and
  publishes `pending_minigame_warp` - the identical staging the field-VM arm
  (`FieldHostImpl::minigame_door_warp`) and the walk-touch arm use. It emits no
  `WorldMapTransition`.
- `FieldEvent::WorldMapTransition { target_map }` → `{ dest_index }`. Its only
  producer is now `WorldMapEntityConfig::OverworldPortal`, whose number is the
  `0x3F` named-scene-change index, not a map id. The drain reads the CDNAME
  destination off the config row and no longer consults `MapIdResolver` at all
  on this path.
- `WorldMapEntityKind::Portal` (the render marker) keeps its name: both
  walk-onto shapes draw the same marker, and that is a render fact, not an id
  space.

### One rename this lane could not make

`PlacementKind::Portal { target_map }` still carries the old field name. Its
only remaining out-of-crate consumer is `crates/web-viewer/src/field_npc.rs:235`,
which is **off limits** to this lane, and a variant/field rename breaks that
compile. Its doc comment now says plainly what the payload is, and every
consumer inside `engine-core` reads it as a sub-id. The rename is one line in
web-viewer plus the mechanical follow-through in `engine-core` and two disc
tests; it should be done by whoever owns that crate.

### Tests that were asserting the defect

Three, and one of them did not even compile.

- `crates/engine-core/src/world/tests/field_npc_motion.rs::walk_touch_warp_posts_once_per_contact_and_queues_transition`
  - **it never compiled on this branch.** Wave 1's integration commit
    (`3134444c`) renamed `WalkTouchEvent::Warp`'s field to `sub_id` and left
    this in-tree unit test on `target_map`, so `cargo test -p legaia-engine-core
    --lib` has been failing to build since. It also asserted
    `pending_scene_transition == Some(3)` - the defect. Renamed to
    `…_and_arms_the_door_warp`, fixed, and it now also asserts the sub-id never
    reaches `pending_scene_transition`.
- `crates/engine-core/src/world/tests/worldmap.rs::world_map_portal_engage_surfaces_target_map`
  and `…walking_onto_portal_auto_engages` - both asserted the
  `WorldMapTransition` carrying a door sub-id. Renamed to
  `world_map_minigame_door_engage_arms_the_door_warp` /
  `…walking_onto_minigame_door_auto_engages` and re-pointed at
  `pending_minigame_warp` + the scene backup.
- `crates/engine-shell/tests/world_map_live.rs::world_map_walking_onto_real_portal_transitions`
  - the disc-gated sibling, asserting the same thing on the real `map02` /
    `map03` signboards. Renamed to
    `world_map_walking_onto_real_minigame_door_arms_the_warp`; it now asserts
    the warp arms, that `pending_scene_transition` stays `None`, and that no
    `WorldMapTransition` is emitted.

---

## Task 2 - placement-interaction dispatch

### Retail's dispatch *(disassembly-grounded)*

A partition-1 placement record is **one byte stream carrying two consecutive
scripts under one cursor**.

- The scene setup spawns a script context per record - `FUN_8003A1E4`, buffer
  base `actor[+0x90]`, PC `actor[+0x9E]`. That context runs the record's
  **spawn section** and stops at the first raw `0x21`.
- `FUN_80039B7C` state 0 resumes *that same* `actor[+0x9E]` on an interaction
  and calls the field-VM dispatcher `FUN_801DE840` in a loop until the byte
  under the PC has `& 0x7F < 0x20` (a `0x1F` text lead or a terminator), then
  hands to the pager.

The `0x21` stop is explicit: `s4` holds `0x21` across the dispatcher call and
`beq s0,s4` at `0x80039E20` breaks on it - *after* the delay slot
`sh a1,0x9e(s2)` has already stored the forwarded PC, so the cursor left behind
points one instruction past the terminator. The arm it breaks to (`0x80039E5C`,
`a2 == 0x21`) is the conversation-end teardown, not the pager. The same raw-`0x21`
rule is `FUN_8003CF7C`'s run-to-next-text helper, which the port already mirrors.

**Nothing in the SM gates on the record containing text.** A record whose
interaction section is camera work, an affordability check and a `0x3E` warp
runs exactly the way a talk NPC's does. A minigame door is not a special case of
the interaction pipeline; it is an ordinary member of it.

### The consequence the port was living with

`placement_inline_prologue` entered at `script_pc0`. The runner therefore hit
the record's spawn terminator on its first step and took the
prologue-fallback - straight to the record's **first `0x1F` segment**. Two
things follow:

- the entire interaction section is skipped, for **every** placement, so no
  record's story-flag segment selection runs (still true for talk NPCs - see
  the scope note below);
- for a door, the first text segment is the wrong branch. `koin1`'s cabinet
  records are the clearest shape: spawn section, then a cross-context player
  `EXEC_MOVE`, camera framing, `0x4E` sub-9 comparing the **casino coin bank**
  against `1` (value loader `0x801E0B34`), a white fade, then `3E 68 …`. The
  compare's skip target is the record's own "no tokens" line - and that line is
  the first `0x1F`. So the fallback landed the player in the refusal branch of
  a check they had already passed. *(disassembly-grounded + disc-measured)*

This also corrects `minigame-slot-machine.md`, which called that gate "an `0x4E`
inventory compare … a player without the item never reaches mode 24". It is a
coin-bank compare, not an item check.

### What was wired

1. `man_field_scripts::placement_interaction_entry_pc(body, script_pc0, limit)` -
   the retail terminator rule, bounded by the first text segment so the walk can
   never desync inside message bytes and read an ASCII `!` as a terminator.
   Returns `script_pc0` unchanged when the record has no spawn section.
2. `placement_interaction_record` - `placement_inline_prologue` entered at that
   cursor. **Door records only.** The rule is general and the SM is one SM, but
   moving the talk-NPC path onto it regresses a pinned, working conversation:
   `retock`'s innkeeper resolves its 2-option picker and then runs neither its
   `0x3A` gold debit nor its `4C 82` restores (`inn_stay_field_vm_disc`, gold
   stayed at 1000 instead of 760). Whatever that record does between its spawn
   terminator and its first line, the port's VM does not come out of it where
   the old entry did. That is a second, separate finding and it is left named
   rather than guessed at; `placement_inline_prologue` keeps `script_pc0`, which
   is the entry every NPC conversation's behaviour is currently pinned against.
3. `World::install_field_carriers_from_man` installs a
   `field_npc_dialog_prologue` entry for every `PlacementKind::Portal`
   placement as well as every text-bearing NPC. Deliberately **not**
   `field_npc_dialog`: the simplified path would type the record's refusal line
   as if it were a greeting. Before this a door placement had no interaction
   record at all - `trigger_field_interact` on a cabinet faced the actor, posted
   `FieldInteract`, and did nothing else.
4. `World::field_interact_probe_slot` (retail `FUN_801cf9f4`) now admits door
   placements. Retail's probe walks the **actor list** with no "is this a talk
   NPC" filter; the engine seeded its probe set from `field_npc_positions`,
   which only text-bearing NPC placements reach, so pressing the action button
   at a cabinet hit nothing at all. The door's placement anchor already rides
   `field_walk_touch`; the probe takes the slots there that also carry an
   interaction record and are not already NPC anchors - which is the door set
   and nothing else. Widening `field_npc_positions` instead was rejected: that
   map is also the NPC **body-collision** and motion-anchor set. Measured: 10 of
   the 12 doors become probe-reachable; `balden` / `balden2` P1[24] are parked
   at the sentinel tile and so have no touchable body for either path.
5. The walk-touch `Warp` arm drops the record run it armed
   (`active_inline_prologue` / `active_inline_slot`) before arming the warp.
   Body contact takes the *decoded* effect and the warp drains that same frame,
   so a run left armed would hang suspended for the whole minigame and then
   resume, on the return frame, as a box the player never opened. So: walking
   into a door still enters the minigame immediately (unchanged from wave 1),
   and the button probe is the path that runs the record.

Measured entries, all twelve doors, all strictly inside the record between
`script_pc0` and the first text segment:

| record | `script_pc0` | interaction cursor | first `0x1F` |
|---|---|---|---|
| `koin1` P1[9] | `0x11` | `0x18` | `0x1E` |
| `koin1` P1[51..56] | `0x0F` | `0x19` | `0xC8` |
| `koin3` P1[16] | `0x0F` | `0x16` | `0x1A` |
| `balden` P1[24] | `0x0D` | `0x31` | `0x84` |
| `map02` P1[7] | `0x0F` | `0x15` | `0x24` |
| `map03` P1[19] | `0x0F` | `0x13` | `0x26` |

*(disc-measured)*

### The honest boundary - none of them reaches its own `0x3E` yet

Running each door's record from the corrected cursor through the port's inline
runner, with a stocked coin bank so the affordability branch passes:

All twelve reach a decision point (a box or the warp); none reaches the warp.
Six carry an interaction-section flag probe and all six latch it.

| door | where the run comes to rest |
|---|---|
| `koin1` P1[51..56] | latches its leading `SystemFlag.Set` (`1075..1077`, `1206..1208`), then dies on the `0xC7` at `entry+0x13` - the cross-context `0x47` yield. The VM's `0x47` arm calls `ctx.halt()` and returns `Yield`; the runner keeps stepping in the same frame, the next step sees the halt bit, and reads it as conversation-over. Retail's SM loop has no such check - the halt bit lives on the actor and the dialog SM does not consult it. |
| `balden` / `balden2` P1[24] | takes its **story gate**: `75 AB …` tests system flag `0x5AB` and, clear, jumps past the whole slot-machine arm into the closed-casino line. Faithful - the Vidna casino is not open on a fresh world - not a defect, and the reason the flag probe is keyed on the record's *leading* instruction rather than the first `Set` anywhere in the section. |
| `koin3` P1[16] | runs the attendant's conversation and ends past it, short of the warp at `0x29A`. |
| `koin1` P1[9] | runs to `0xB1D`; its three warps are at `0xB3D` / `0xB66` / `0xB89`. The closest of the twelve. |
| `map02` / `map03` | show the signboard's text and loop back to the record's top selector - a clean pass end. Their warp sits behind a further branch. |

Two named blockers, in priority order:

1. **The runner ends on a post-`Yield` halt.** `vm::field::step`'s `0x37`/`0x41`/
   `0x47` arms set `ctx.halt()`; `step_inline_dialogue` continues the fast-forward
   loop after a `Yield`, so the *next* step returns a halt the runner treats as
   the end of the conversation. Retail continues past a yield in the same frame
   (the SM loop tests only "PC changed" and "byte `< 0x20`"). This is the one
   that costs the six `koin1` cabinets.
2. **`0x4C` outer-12 / outer-14 arms and the `0xAD F8 08` clip-end spin** stop
   the run further along the same records.

Both live in `crates/engine-vm/src/field/**` and
`crates/engine-core/src/world/narration.rs` - the shared dialogue runner, whose
behaviour every NPC conversation on the disc depends on. Changing the halt
policy there is a subsystem change with its own oracle, not a lane-D edit, and
guessing at it would have been exactly the "fake wire" failure. It is written up
here so the next pass starts from a named cause instead of a symptom.

### What the ladder's per-leg caveat now says

`crates/engine-shell/tests/minigame_replay.rs` still supplies the trigger itself
(`load_field_script_at` from the record start, retrying from the warp PC), and
still reports "every door clears only past its prologue". What changed is the
*reason*: it is no longer "the port has no placement-interaction dispatch". The
dispatch exists, is entered at the retail cursor, and is measured by
`crates/engine-core/tests/placement_interact_disc.rs`; the ladder keeps its own
trigger because a rung driven through `trigger_field_interact` would score lower
today, and lowering a ratchet to record a better-founded path is the wrong trade.

---

## Tests added

- `crates/engine-core/tests/placement_interact_disc.rs`
  - `door_records_carry_a_spawn_section_the_interaction_entry_skips` - every
    door record's cursor is `script_pc0 < entry < first_segment`.
  - `interaction_entry_is_script_pc0_without_a_spawn_terminator` - disc-free;
    the rule degrades rather than guesses.
  - `the_action_probe_reaches_a_door_placement` - the button probe returns the
    door's slot from its own anchor, and returns `None` from the same seat once
    the interaction record is removed. The contrast is what makes the hit
    attributable to the new arm rather than to a neighbouring actor.
  - `interacting_with_a_door_runs_its_record` - the discriminating assertion is
    a flag probe: a record whose interaction section opens with a
    `SystemFlag.Set` must have that flag latched after the interaction, which is
    only reachable **past** the spawn terminator. Asserted non-vacuous (at least
    one door carries such a probe, and the flag is clear before the run). Also
    reports per door the resting `(pc, byte)` and the warp state.

## Docs touched

- `docs/subsystems/script-vm.md` - new § "The interaction cursor: one record,
  two consecutive scripts", under the existing field-dialogue section.
- `docs/subsystems/field-locomotion.md` - the walk-touch bullet now names the
  payload a sub-id, plus a new bullet for the door interaction record.
- `docs/subsystems/minigame-slot-machine.md` - the cabinet entry gate is a
  **coin-bank** compare (`0x4E` sub-9, loader `0x801E0B34`), not an item check,
  and its failure branch is the record's first text segment.
- `docs/subsystems/world-map.md` (outside this lane's declared doc set, but it
  described the renamed type by name) - the config variant, the drain seam, the
  auto-engage rule, and the corpus count corrected from 11 to 12 door
  placements with their per-scene breakdown.

## Residue

1. `PlacementKind::Portal { target_map }` → `MinigameDoor { sub_id }`, blocked
   on `crates/web-viewer/src/field_npc.rs`.
2. The post-`Yield` halt policy in `step_inline_dialogue` (blocker 1 above).
3. `DefaultMapIdResolver` now has **no** producer inside the engine: op `0x3E`,
   the walk-touch arm and the world-map entity arm all route to the door warp,
   and the `WorldMapTransition` drain reads its config row. Its only remaining
   callers are tests that inject `pending_scene_transition` by hand. Its doc
   comment still describes a retail map-id → scene-name table in an uncaptured
   overlay; no such table exists. It belongs in
   `docs/reference/re-do-not-re-walk.md` and the type belongs in the bin.
4. **The talk-NPC path still enters at `script_pc0`, and the inn says why.**
   Applying the corrected cursor to every placement is the faithful end state,
   but it breaks `retock`'s innkeeper today. That is the highest-value follow-up
   in this area: it is one record, one disc-gated test, and a bisect of the
   record between `0x0D`-ish and its first `0x1F` will name the op the port gets
   wrong. Doing it also retires the fallback-to-first-segment net, which is what
   currently makes an NPC's story-flag segment selection unreachable.
5. `tick_field_interaction_probe` still short-circuits on
   `field_npc_positions.is_empty()`, so a scene whose only interactables are
   doors would not probe. Every door scene on the disc also has talk NPCs, so
   nothing is unreachable today - but the guard is keyed on the wrong set now
   that the probe considers two.
