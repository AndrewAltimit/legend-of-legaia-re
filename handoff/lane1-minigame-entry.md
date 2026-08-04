# Lane 1 - minigame entry: the port had the door and never opened it

## Headline

Retail enters every minigame through **one** mechanism, and the port already
decoded it, documented it, and then routed it into the wrong subsystem.

Field-VM op `0x3E` with `op0 >= 100` is the **mode-24 minigame door-warp**.
`sub_id = op0 - 100` selects a *code overlay*, not a scene. The port's op-`0x3E`
arm called `host.scene_transition(map_id)`, the world host recorded it as
`pending_scene_transition`, and `SceneHost::tick` resolved it through
`DefaultMapIdResolver` - a **CDNAME-ordinal** table - into an unrelated scene
name and warped the player there. So this was never "the minigames are
untested". A player who walked into the casino and used a cabinet was teleported
to whatever scene happened to sit at that ordinal.

The resolver's own doc comment conceded the problem while shipping the bug: it
recorded that the id "maps to a code overlay at PROT `0x4d + map_id`" and called
its CDNAME ordering "an approximation" for a retail table "in an uncaptured
overlay". There is no such table. The only scene name in the whole chain is the
*departure* scene, saved so the minigame can warp back.

## The brief's premise, corrected

The brief said "no field-VM opcode enters a minigame". **Half right, and the
wrong half was load-bearing.**

- Op `0x49` is not a minigame launcher - **confirmed**. Its 14 sub-ops all arm
  the same in-field submode-driver actor (template `0x8007065C`, tick
  `FUN_801F159C`); no overlay load, no mode change. *(disassembly-grounded)*
- Op `0x4C` `MENU_CTRL` is not one either - **confirmed, negative**. Inside the
  whole field VM there are exactly two stores to the game-mode word
  `0x8007B83C`: `0x801E07A0` (op `0x3E`, value `0x18`) and `0x801E3104`
  (`0x1A`, the STR/FMV mode). No `0x4C` arm contributes, and no `0x4C` arm
  calls the overlay loader. *(disassembly-grounded)*
- Op `0x3E` **is** the launcher. *(disassembly-grounded)*

## The retail chain

```
801e078c  lui   v0,0x8008
801e0794  sw    zero,-0x4540(v0)   ; _DAT_8007BAC0 = 0
801e0798  lbu   v1,0x0(s6)         ; v1 = op0
801e079c  li    v0,0x18
801e07a0  sh    v0,-0x47c4(a1)     ; _DAT_8007B83C = 0x18   -> game mode 24
801e07a8  sw    zero,0x4440(v0)    ; _DAT_80084440 = 0      -> winnings acc
801e07b0  addiu v1,v1,-0x64        ; sub_id = op0 - 100
801e07b8  _sh   v1,-0x45cc(v0)     ; _DAT_8007BA34 = sub_id
801e07cc  addiu s8,s8,0x6          ; PC += 6
801e07d0  and   v0,v0,a0           ; player[+0x10] &= ~0x80000
```

`see ghidra/scripts/funcs/overlay_0897_801de840.txt`. **No scene-change packet
call** (`func_0x8001FD44`) appears in the arm - which is what op `0x3F`, the
*named* scene change, does call. That absence is the proof the id is not a map
id. *(disassembly-grounded)*

Then `FUN_80025980` (mode-24 OTHER INIT, static SCUS) backs up the departure
scene name, loads the overlay with `FUN_8003EBE4(sub_id + 0x4D)` (`+2` first
when `sub_id >= 6`), calls the sub-id's init, and hands to mode 25.
`FUN_80026018` is the return warp. Both were **already ported** as
`World::arm_minigame_warp` / `minigame_return_warp` - and nothing in the field
path called them.

### Why none of the five state machines has a caller

They are not called. Each is the `+0x08` **tick word of a static 24-byte actor
template**; the sub-id init spawns an actor from that template via
`FUN_80020DE0`, and the per-frame pool walk reaches it through
`jalr actor[+0x0C]` in `FUN_8002519C`. A `jal` search for these addresses
returns zero **by construction** - that is the correct answer, not a corpus gap.
*(disassembly-grounded)*

| sub_id | PROT | init | template | driver | notes |
|---|---|---|---|---|---|
| 0 | 0972 | `FUN_801CF070` | `0x801D8FF4` | `FUN_801CF3BC` | fishing |
| 1 | 0973 | `FUN_801CE8A0` | - | - | dev `OTHER2`, **no door on the disc** |
| 2 | 0974 | `FUN_801CEE80` | - | - | dev `OTHER3`, **no door on the disc** |
| 3 | 0975 | `FUN_801CEC94` | `0x801D3618` | `FUN_801CF0D8` | slot machine |
| 4 | 0976 | `FUN_801CF00C` | `0x801D75DC` then `0x801D75F4` | `FUN_801CF388` then `FUN_801D3468` | Baka is two-stage |
| 5 | 0977 | `FUN_801CEA6C` | `0x801D1A20` | `FUN_801CF870` | dome hub; match SM is battle-overlay |
| 6 | 0980 | `FUN_801CEF54` | `0x801D42E4` | `FUN_801CF470` | dance; note the `+2` loader step |

The dome round returns to the arena rather than the field through the **second**
game-mode-24 writer on the disc: `FUN_80046A20` stores `0x18` when
`_DAT_8007BAC0 & 0x100` is set, `0x2` otherwise. *(disassembly-grounded)*

## Disc census - the door sites

Independently measured twice (a corpus-wide walk, and this lane's own test).
Test: `crates/engine-core/tests/minigame_entry_census_disc.rs`, N = 124 CDNAME
scenes walked, **17 genuine door sites** decoded at real instruction
boundaries across 5 sub-ids. *(disc-measured)*

| sub_id | minigame | sites |
|---|---|---|
| 0 | fishing | `map02` P1[7], `map03` P1[19] (overworld signboards) |
| 3 | slot machine | `koin1` P1[54..56], `balden` P1[24], `balden2` P1[24] |
| 4 | Baka Fighter | `koin1` P1[51..53], `map03` P2[20] (dev residue) |
| 5 | Muscle Dome | `koin1` P1[9] x3 - one record, three course arms |
| 6 | dance | `koin3` P1[16], P2[6], P2[9] |

**sub_id 1 and 2 have zero sites disc-wide.** The two dev modules have no
reachable door anywhere, which independently corroborates the five-playable /
two-dev split the port now encodes in `MinigameSubId::is_playable`.

Two corrections to existing docs fell out and are **not** yet applied (see
Residue): the fishing venue bundle `other1` carries essentially no field-VM
script, and the dance hall module `other7` carries no genuine warp - both
venues' doors live in other scenes.

## What was wired

1. **`crates/engine-vm/src/field/host.rs`** - new `FieldHost::minigame_door_warp(sub_id)`
   (default no-op), documented with the arm's disassembly.
2. **`crates/engine-vm/src/field/step.rs`** - op `0x3E`'s `op0 >= 100` arm now
   calls `minigame_door_warp` instead of `scene_transition`. `scene_transition`
   survives for the field `.MAP` walk-on door triggers, which do carry map ids.
3. **`crates/engine-core/src/minigame_entry.rs`** *(new)* - `MinigameSubId`:
   the 7-slot id space, retail's PROT arithmetic
   (`sub_id + (2 if >= 6) + 0x4D + 0x37F`), the playable/dev split.
4. **`crates/engine-core/src/world/vm_hosts.rs`** - the host arm: run
   `arm_minigame_warp()` (the winnings zero + departure-scene backup) and
   publish `pending_minigame_warp`.
5. **`crates/engine-core/src/world/state.rs`** - the `pending_minigame_warp`
   channel, with the "not a map id" warning at the field.
6. **`crates/engine-core/src/scene/host/minigame_warp.rs`** *(new)* - the port
   of the mode-24 init: read the sub-id's overlay off the disc, parse its own
   tables, install the session. Every failure arm completes the round trip, so
   a script that armed a warp is never parked in a mode with no exit.
7. **`crates/engine-core/src/scene/host/scene_entry.rs`** - drain the warp
   ahead of the map-id transition; outcome lands in `SceneHost::last_minigame_warp`.

**A deliberate non-change:** the outcome is a host field, not a new
`SceneTickEvent` variant. Adding a variant forces an arm into every exhaustive
match, and two of those live in this wave's off-limits files
(`engine-shell/src/bin/**`, `web-viewer`). If a later pass wants the event
variant, that is the one-line follow-up plus ~2 match arms.

### One `NOT WIRED` disclosure this falsified

`crate::mode::other_warp_init_stage` is the ported mode-24 **staging plan**
(`PORT: FUN_80025980`) and already computed retail's `sub_id -> loader param`
arithmetic exactly - the `+ 0x4D` bias and the `+2` step at `sub_id >= 6`. It
carried a `NOT WIRED:` disclosure saying nothing stages a per-sub-id overlay.

`MinigameSubId::prot_index` / `overlay_init_va` now call it, so it has a live
caller and the seven PROT indices exist in exactly one place instead of two -
the first draft of this lane re-derived them, which would have given the disc
two sources of truth for the same numbers. The disclosure is downgraded to
`PARTIALLY WIRED` and now says precisely what is still missing: the overlay
*residency* model (load an image at a base and `jalr` `overlay_entry` out of
the `0x80010AE4` table). The engine's minigames are resident Rust rules
engines, so that half remains genuinely absent.

## The ladder

`crates/engine-shell/tests/minigame_replay.rs`, ratchet
`scripts/replays/minigame_replay_baseline.toml`. **Score 5 / 5.** Nothing in it
calls `World::enter_*`; every leg locates the disc's real door record and
executes the real bytecode.

| # | rung | result |
|---|---|---|
| 1 | every playable minigame's door record located | clear, 5/5 |
| 2 | each door's bytecode publishes its `sub_id` | clear, 5/5 |
| 3 | host drain enters the scene mode off real overlay tables | clear, 5/5 |
| 4 | each minigame advances under a pad stream | clear, 5/5 |
| 5 | each returns to the field | clear, 5/5 |

Rungs 2 and 3 are split on purpose: a rung-2 failure is a VM defect, a rung-3
failure is a host defect, and collapsed they would report the same number.

### The caveat that matters most

**Every door clears only *past* its own prologue.** Run from the record's start,
all five come to rest inside the attendant's conversation without reaching the
warp - four on `0x1F` (a text-segment byte, not an opcode), the two `koin1`
cabinets on `0xAB` (the `0x80` cross-context prefix on op `0x2B`). The runner
then retries from the warp instruction's own PC, which is still the disc's bytes
through the ported VM, minus the prologue. The test prints which path each door
took and the `(pc, opcode)` it stalled on.

Setting `World::use_vm_dialogue` moves none of them, and that is itself a
measurement: the inline-script runner is reached through
`trigger_field_interact`'s dialogue install, not through a script loaded with
`load_field_script_at`, so it never engages on this path.

So the honest reading of rung 2 is **"the door's bytecode drives the port into
the minigame"**, not "walking up and pressing X does".

## Coverage - with its N

`cargo llvm-cov --release -p legaia-engine-shell --test minigame_replay` joined
through `scripts/ci/replay-port-coverage.py`:

```
ported anchors        : 834
live / entered        : 689 / 146
not-live / entered    : 145 / 0     (observable 145 / 145)
NOT WIRED executed    : 0
live never entered    : 488
```

**Read that 146 as a different measurement, not a delta.** The join is
denominated on **one test binary**; `main`'s 132 is `critical_path_replay`'s
number. Two different tests, two different numbers - subtracting them would be
meaningless.

### The caveat, measured rather than assumed

The wave brief flagged that a row this join calls "never entered" may well be
executed by `crates/engine-core/tests/*_minigame_real.rs`, and that an
unqualified number would mislead. So both were run and joined against the same
catalog. Rows live **and** observable in at least one of the two binaries:

| module | ladder | `*_minigame_real` | union | live |
|---|---|---|---|---|
| `fishing` | 3 | 4 | 4 | 24 |
| `baka_fighter` | 12 | 13 | 13 | 19 |
| `dance` | 8 | 12 | 12 | 18 |
| `baka_hub_actors` | 0 | 0 | 0 | 15 |
| `muscle_dome` | 8 | 8 | 8 | 12 |
| `ui_fishing` | 0 | 0 | 0 | 10 |
| `slot_machine` | 7 | 7 | 7 | 8 |
| `minigame_*` | 0 | 1 | 1 | 7 |
| `baka` (other) | 2 | 2 | 2 | 3 |
| **total** | **40** | **47** | **47** | **116** |

**Union equals `real`. The ladder enters no row the sibling tests did not
already enter** - its 40 are a strict subset of their 47.

That is the honest headline, and it is not a disappointment: **the ladder's
product is reachability, not coverage.** Those 47 rows were always executed;
what no instrument showed was that a player could reach them. Before this lane
the answer was that they could not - the door warped them to an unrelated
scene. The catalog number is the wrong denominator for that question, which is
why the ladder scores rungs rather than rows.

Two corrections fall out. The block is **116-118** live rows, not the ~79 the
brief carried (the spread is which binary a row is observable in) - re-derive
before quoting. And the 69 unentered rows are unentered by **both** binaries,
so they are not "covered elsewhere".

### Rows the ladder reached the surface of and still did not enter

These are the ones worth acting on, and they split into three kinds:

- **`baka_hub_actors` 0/15 - a real wiring gap, and neither binary touches it.**
  This is the op-`0x49` submode dispatcher (the casino hub / coin counter). The
  door warp does not go through it and nothing else arms it, so the hub is
  reached by no path at all, test or player. Distinct from the door work - see
  residue 2.
- **`ui_fishing` 0/10 and most of `minigame_*` - observability, not wiring.**
  Draw-list builders and art / sfx resolvers. Both binaries are headless and
  build no draw lists, so neither could enter them either way. A renderer-side
  oracle is the right instrument, not a deeper pad stream.
- **`fishing` 3-4/24 - both instruments stop too early.** The ladder hooks and
  reels but never *lands* a fish (its leg ends with the phase still
  `Fighting`), so the catch, score, point-bank and prize-exchange paths never
  run; `fishing_minigame_real` adds exactly one row over it. This is the
  cheapest single gain in the table and it is a gain for **both** instruments.

## Residue - for the integration pass

Ordered by how much each blocks a player.

1. **The walk-on door trigger still mis-routes.** `world/field_movement.rs`
   (off-limits to this lane) posts `WalkTouchEvent::Warp { target_map }` into
   `pending_scene_transition`, and `target_map` comes from
   `classify_placement`'s `PlacementKind::Portal { target_map: op0 - 100 }` -
   **the same mode-24 sub-id space**. Both producers of that channel carry
   sub-ids; only the VM one is now routed correctly. Fix is one line: post
   `pending_minigame_warp` instead. Watch `field_walk_touch_disc.rs:125`, which
   asserts the old field, and `world_map_live.rs:367`, which injects
   `pending_scene_transition = Some(0)` with a synthetic resolver and is the
   reason the drain was not simply repointed wholesale.

2. **No placement-interaction dispatch.** `World::trigger_field_interact` opens
   inline dialogue and runs a partition-1 record only for boss stagers. Retail
   resumes the placement's script on every interaction. Until that exists, no
   door clears through its own prologue. `install_cutscene_timeline_record` is
   the general "run a MAN record through the field VM" primitive to build on.

3. **`DefaultMapIdResolver` is a falsified hypothesis still in the tree.** Its
   doc comment describes a retail map-id -> scene-name table in an uncaptured
   overlay. No such table exists. Now that op `0x3E` no longer reaches it, its
   only remaining producer is the walk-on trigger - i.e. residue 1 removes its
   last caller. Candidate for `docs/reference/re-do-not-re-walk.md`.

4. **Dome / dance sessions are staged with stand-ins.** The drain opens the dome
   on flat favoured costs and stand-in HP/budget, and the dance on the qualifier
   short song. Retail stages `(course, round)` and the dance heat from the
   venue's story flags (`0x536`/`0x537`/`0x538` for the three dome courses,
   latched by the course-menu arms). Wiring those makes the entry state correct
   rather than merely live.

5. **Doc corrections found but not applied** (outside this lane's doc scope):
   `docs/subsystems/world-map.md` says "exactly 11 genuine door-warp portals
   survive" - that count comes from a partition-1 placement classifier over
   bundle MANs only; all three partitions plus streaming-variant MANs give 17-18.

## Evidence grades

- **disassembly-grounded**: the op-`0x3E` arm and its five stores; both
  game-mode-24 writers; `FUN_80025980`'s loader call, jump table and mode-25
  handoff; every template `+0x08` word and its single materialisation site; the
  `jalr actor[+0x0C]` invoker; the op-`0x4C` and op-`0x49` negatives.
- **disc-measured**: the 17-site door census and its per-sub_id breakdown; the
  zero-door result for sub_ids 1 and 2; the ladder's 5/5 and every
  `(pc, opcode)` stall.
- **inferred**: that `FUN_801CF388` reaches its stage-2 spawn on the retail path
  (the site is in its body; the guarding branches were not traced). Also the
  dome/dance stand-in staging in the drain - a live entry, not a claim about
  which course or heat retail opens.
