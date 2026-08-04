# Rung 4: the probe and the lattice were both innocent - the route was missing a scene

Question posed: is the rung-4 seal in `World::field_dir_blocked`'s probe
geometry, or in `plan_path`'s lattice?

**Answer: neither.** Both are faithful to the instruction. `map01`'s wall bits
genuinely split the kingdom into two walk components, the crossing between them
is the `suimon` scene, and which of `suimon`'s two chambers you arrive in is a
story flag the port was silently discarding. **Rung 4 now clears; the baseline
is raised to `reached = 4`.**

---

## 1. Probe geometry - disassembly-grounded, matches the port point for point

`FUN_801cfe4c(actor, scene, dir)` (`ghidra/scripts/funcs/overlay_0897_801cfe4c.txt`,
217 instructions) is **six** probe points per direction, not three:

- Three calls to `FUN_801cfc40` with the halfword pairs at
  `DAT_801f21b4 + dir*0x10 + {0, 4, 8}` (`801cfe90`, `801cfeac`, `801cfecc`) -
  the actor/prop box arm, results OR'd and masked `& 5` at `801d0184`.
- Three **inline** grid reads with the pairs at `DAT_801f2214 + dir*0x10 +
  {0, 4, 8}` (`801cfef4`, `801cffd0`, `801d00ac`) - the static-wall arm, any hit
  setting `s6 = 2` (`801d017c`).

Each row is `0x10` bytes and only its first three pairs are read; the fourth
(offsets `+0xC`/`+0xE`) is never touched by this function. `dir` spans `0..3`:
the two table bases are `0x60` apart, i.e. exactly four `0x10`-byte rows plus
the two-row `DAT_801f2254` interact table that starts at `0x801f2254`.

Decoded off the disc (`extracted/overlays/overlay_field_0897.bin`, base
`0x801CE818`, file `0x239FC` / `0x2399C`), both tables are **byte-identical to
`crates/engine-core/src/world/config.rs`'s `FIELD_WALL_PROBES` and
`FIELD_ACTOR_PROBES`**, including the unread fourth pairs the port omits.

The inline per-point index math at `801cfef4..801cffbc` decodes to:

```
col  = ((x + dx + 0x3f) >> 6) - 1          ; 801cff2c..801cff40
row  = trunc((z - dz) / 64) + 2            ; 801cfef8..801cff20
byte = grid[ (col/2 & 0x7f) + (row/2 & 0x7f)*0x80 ]   ; 801cff44..801cff74
hit  = (byte >> 4) & (1 << ((col & 1) + 2*(row & 1))) ; 801cff7c..801cffbc
```

with `col/2` and `row/2` truncating toward zero (the `srl/addu/sra` idiom) and
the row term written as `((row + signbit) << 6) & 0x3f80`, which is
`((row/2) & 0x7f) * 0x80`. That is `World::field_tile_is_wall`
(`crates/engine-core/src/world/field_movement.rs:442`) line for line, including
the `& 0x7f` on both axes.

**Sub-question (a), lateral spread: correct.** ±16 in the perpendicular axis,
47 units of reach in the negative directions and 48 in the positive ones - the
asymmetry the biased mapping needs to put each edge one sub-cell ahead.

**Sub-question (b), overworld reach: no, it is the same probe.**
`ghidra/scripts/funcs/overlay_world_map_top_801cfe4c.txt` is base-tagged
(`base=0x801C0000`) and its disassembly is byte-for-byte identical to the 0897
copy. Both tables are data *inside* overlay 0897, which hosts the world-map
subsystem, so there is no second table either. And the world-map-walk copy of
the controller (`.../overlay_world_map_walk_801d01b0.txt`, 491 instructions)
commits its axis steps in **2-unit** increments exactly like the field
(`801d0680` `addiu v0,v0,0x2`, `801d06e0` `-0x2`, `801d0740` `+0x2`,
`801d07b0` `-0x2`, cursor `801d0758` `addiu s5,s5,0x2`), looping while
`s5 < s4`. A faster overworld speed buys more sub-steps per frame, not a longer
reach.

(Note: `ghidra/scripts/funcs/overlay_0897_801d01b0.txt` is a 40-byte,
**untagged** fragment - not the controller. Per
`docs/tooling/dump-corpus-integrity.md`, only the header tag is evidence of a
dump's base; the world-map-walk copy is the one with a tag and a prologue.)

## 2. Lattice hypothesis - falsified empirically

`plan_path` walks a 32-unit lattice; retail's mover is continuous in 2-unit
steps. A lattice can only ever *under*-report reachability against that, so the
test is: re-flood at finer pitch until the pitch **is** the stepper's, and see
whether the components merge.

Re-implemented `field_tile_is_wall` + `field_dir_blocked` offline in C against
`map01`'s own `.MAP` (extraction `0083`, the `define 85 - 2` slot 0) and flooded
from the arrival at world `(12352, 3264)`:

| pitch | nodes | tile-equivalents | reaches retail's `(8266, 8700)` |
|---|---|---|---|
| 32 | 13,713 | 857 | no |
| 16 | 54,743 | 855 | no |
| 8 | 217,781 | 850 | no |
| 4 | 867,905 | 847 | no |
| **2** | 3,464,345 | **845** | **no** |

Pitch 2 is the retail stepper's own granularity and the arrival is on an even
coordinate, so that row is the exact set of positions retail's `FUN_801d01b0`
can walk to. Nothing coarser can be more connected. The area is flat across the
whole sweep - the lattice is not costing anything.

The 32-pitch row also reproduces the port's own number (the ablation reports
13,700; the 13-node delta is the port's prop/NPC arm, which the offline replica
does not model), which is what makes the replica trustworthy.

Two further ablations, same conclusion: dropping the footprint to a single
centre probe gives 873 tile-equivalents, dropping it to a bare
destination-point test gives 891, and neither reaches. Running the whole thing
under **plain floor indexing** instead of the `+2` / `ceil-1` bias changes the
counts by under 0.2% and reaches nothing either. The topology is not an
indexing artifact.

An 8-connected planner cannot help for a structural reason: at pitch 32 the
intermediate node of any diagonal is itself on the lattice, and
`advance_with_collision` resolves a diagonal as an independent Z step then an X
step from the Z-advanced position - which is exactly the two 4-connected edges.

## 3. The collision data is retail's, verified over the whole grid

The `.MAP` buffer sits at VA `0x80139530` in all three `map01` mednafen states
(located by matching the disc file's object-record head). Comparing
`RAM[+0x4000..+0x8000]` against `extracted/PROT/0083_*.BIN`'s same window:

| state | position | bytes differing | wall-nibble bytes differing |
|---|---|---|---|
| `mode_17` arrival | `(12352, 3264)` | 6 | 6, all at tiles `(95..97, 24..25)` |
| mid-walk | `(9340, 4772)` | 6 | same six |
| `keikoku_chest_preload` | `(8266, 8700)` | 6 | same six |

The six are the arrival-clearing paint around Rim Elm's landing tile, and they
are identical in the state standing next to the Ravine. **Retail's live grid at
the Ravine is the disc's grid**, so no script paint opens the barrier.

The trigger tables are equally exact: `map01`'s live `+0x10000` primary block
and its live `+0x12000` fallback are byte-identical to disc entries `0083` (at
`+0x10000`) and `0084` (whole), which is where `Scene::field_tile_triggers`
reads them from. The port's fallback-from-`idx + 1` resolution is correct.

## 4. What the barrier actually is

A 0-1 BFS over the 64-unit sub-cell lattice (cost 1 to enter a wall sub-cell)
puts the **minimum** crossing at **four** wall sub-cells, at sub-column 118,
sub-rows 120..123 - world `(7584, 7584..7776)`, tiles `(59, 59..60)`. Four
sub-cells is 256 world units of solid authored wall; no probe subtlety crosses
it.

Retail's own observed positions agree with the port on every point but the
last. Flooding from the arrival: `(12194, 3550)` reached, `(9340, 4772)`
reached, `(9598, 4544)` reached, `(8266, 8700)` **not** reached. All four are
real `map01` save-state positions.

## 5. The seal: the route needs `suimon`

Flood-testing all 192 gate-1 trigger tiles from the arrival, the reachable
portal records are `0`, `17`, `18`, `19` (adjacent), `34`, `35`, `36`, `37`,
`38`. Disassembling `map01`'s partition-2 records for their first `0x3F`:

| record | trigger tiles | destination |
|---|---|---|
| `18` | `(55, 62)`, `(56, 61)` | `suimon`, entry `(0x44,0x2C)` / `(0x15,0x54)` |
| `19` | `(57, 61)`, `(58, 62)` | `suimon`, entry `(0x20,0x57)` |
| `17` | `(70, 39)` | `izumi` |
| `21`, `23`, `25`, `27` | `(53,93) (53,94) (64,68) (77,69) (81,82) (81,83)` | `keikoku` |
| `34`, `35`, `36` | the road bands | no `0x3F` (mist-wall force-walk) |

All six `keikoku` mouths are on the southern component. `suimon`'s own
partition-2 records close the loop: records `0`/`1` return to `map01` tile
`(54, 61)` (northern side, reachable), record `2` returns to `map01` tile
`(59, 61)` - **southern side**.

So the chapter-1 approach is `map01 -> suimon -> map01 -> keikoku`, and the
port's `portal_hazards` puts `suimon` in the avoid set for the `keikoku` leg,
so `plan_path` routes around the only way through. `portal_tile`'s own doc
comment already records the symptom - "the follower's best-effort push toward
it runs it over a **different** scene's portal on the way (`suimon`)" - and
reads it as a goal-selection failure. It is a route failure: `suimon` is the
route.

Neither `suimon` record is gated (`C1`/`C2` empty), so no extra story flag is
needed. `keikoku`'s records are `C1 = 0x193` (blocked only *after* the Ravine),
and the mist-wall records `34`/`35`/`36` are `C1 = 0x482` - which is exactly
the flag the ladder already seeds.

The 54-sub-cell residual the ablation reported is consistent: 54 x 32 = 1728
world units = 13.5 tiles, which is the distance from the neck at `(55..56, 61..62)`
to the nearest `keikoku` mouth at `(64, 68)`.

## 6. …and which chamber of `suimon`, which is flag `0x27B`

Routing leg 2 through `suimon` still came back to `map01` tile `(54, 61)` -
the component it started from. `map01` P2[18] is a **two-armed** `0x3F`:

```
0x000E  72 7B 1D 00   SysFlag.Test idx=0x027B -> 0x002D
0x001C  3F ... suimon entry=(0x44,0x2C)      <- flag CLEAR (fall-through)
0x0037  3F ... suimon entry=(0x15,0x54)      <- flag SET   (branch target)
```

The field VM's `0x7_` route takes the branch **on set**
(`crates/engine-vm/src/field/step.rs`, the `0x50..=0x77` arm), so a cold boot
enters at `(0x44, 0x2C)` = `(68, 44)`.

Those two entries are opposite ends of the scene. Flooding `suimon`'s own grid
(extraction `0075`) with its exact on-disc trigger tiles honoured:

| entry | nodes | record-2 southern-door tiles reached |
|---|---|---|
| `(68, 44)` (flag clear) | 6,425 | **0 of 20** |
| `(21, 84)` (flag set) | 1,018,114 | **20 of 20** |

The northern chamber's only exits are records `0`/`1`, which return to `map01`
`(54, 61)`. So with `0x27B` clear the crossing is a deliberate dead end -
`suimon` is a sluice-gate puzzle, and its own scene-entry script `P1[0]` sets
the flag (`man-scripts --system-flag-census`: `Set scene=suimon PROT[0078]
P1[0] (op 0x52)`).

**The port was discarding that arm.** `partition2_scene_changes` recovered a
conditional destination only when the two arms named *different scenes*:

```rust
(dest.1 != primary.1).then_some((flag, dest))   // scene name only
```

which is the `dolk` -> `dolk2` shape (flag `0x142`). A two-ended pass names one
scene twice and differs only in the arrival tile, so the conditional was
dropped and the flag-clear arm stood forever. The consumer
(`SceneHost`'s overworld-entity seeder) already swapped entry tiles correctly -
only the recovery guard was too narrow. Comparing the whole destination tuple
still rejects the degenerate shape where the branch target falls through to the
same `0x3F`.

## 7. Verdict

**Rung 4 clears. `scripts/replays/critical_path_baseline.toml` is raised to
`reached = 4`.**

```
[ok] town01 free-roam, input released
[ok] pad-walk town01 south gate -> map01
[ok] pad-walk map01 across the continent      77 tiles, 9 encounters fought
[ok] pad-walk map01 -> suimon -> map01 -> keikoku (Ravine)
     map01 -> suimon: entered suimon | suimon -> map01: entered map01
     | map01 -> keikoku: entered keikoku
  score 4 / 4
```

Three edits carried it, and none of them is in the collision probe:

1. `crates/engine-core/src/man_field_scripts/scene_triggers.rs` - the
   conditional-destination guard compares the whole destination, not the scene
   name. This is the engine fix; everything else is the ladder catching up to
   the disc.
2. The ladder seeds `WATER_GATE_FLAG` (`0x27B`) alongside the two gates it
   already seeds, on the same charter: it scores locomotion, collision and the
   pad remap, not story progression.
3. Rung 4 is a three-leg route (`map01 -> suimon -> map01 -> keikoku`) with
   `suimon`'s two northern doors handed to `plan_path` as hazards, the way
   `portal_hazards` already treats other scenes' portals on the overworld.

Regression check: `engine-shell/tests/field_collision_discriminator.rs` passes
7/7 unchanged, including the byte-exact `rimelm_wall_press_left` / `_down` rest
positions - as it must, since the probe was not touched.

Eliminated along the way, on top of what the brief already eliminated:

- `FIELD_WALL_PROBES` / `FIELD_ACTOR_PROBES` values (byte-exact vs the disc).
- `field_tile_is_wall`'s index math (instruction-exact vs `FUN_801cfe4c`).
- A separate overworld probe or reach (the world-map dump is identical).
- The planner lattice at every pitch down to the stepper's own.
- Footprint-vs-point and biased-vs-floor indexing as topology causes.
- A script paint or a wrong `.MAP` (live grid diffed over the full `0x4000`).
- A wrong fallback trigger table (live `+0x12000` == disc entry `0084`).

## Instruments left in the tree

`crates/engine-shell/tests/critical_path_replay.rs` gains
`probe_rung4_lattice`, gated on `LEGAIA_CPR_LATTICE=1` next to the existing
`LEGAIA_CPR_ABLATE` hook: it re-floods the overworld at 32 / 16 / 8 / 4 / 2
units of pitch and then lists every overworld portal with whether the flood
reaches it. The ablation eliminates the flood's *inputs*; this eliminates its
*lattice* and then says what the surviving component actually contains - which
is the step that named `suimon`.

## What is still open

`map01` P2[18]'s conditional is now honoured, but the port has no way to
*clear* `0x27B` by playing `suimon`'s puzzle - the ladder seeds it. Whether
`suimon`'s `P1[0]` / `P1[4]` scripts run far enough under the engine to set the
flag on their own is untested, and is the natural follow-on: it would turn the
seeded flag into an earned one and make the crossing a scored rung of its own.
