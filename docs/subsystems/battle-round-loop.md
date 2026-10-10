# Battle round loop, encounters and rewards

## SFX bank + scheduler

Maps battle / field cue IDs (the `kind` byte the art-record `HitCue` / overlay scripts emit) to per-cue `SfxEntry` descriptors that describe how to fire a one-shot through the SPU. Engines populate the catalog at startup, then forward `ScheduledCue`-like requests through `SfxScheduler` which queues each request with its retail timing offset and dispatches when the per-frame tick reaches the firing frame.

| Cue ID | Meaning |
|---|---|
| `0x1A` | Generic SFX trigger ("play sound" hit cue). |
| `0x4C` | Hit-effect visual (no sound on its own). |
| `0x80..=0xFE` | Reserved per-character / per-art SFX IDs. |

`SfxBank::play_one_shot` delegates to the existing `VabBank::play_note` for tone lookup, pitch math, and ADSR setup; the scheduler is a frame-driven queue that returns an `SfxFireBatch` per `tick_frame` call.

The bank is decoded from the user's `SCUS_942.54` `DAT_8006F198` descriptor table at boot (`SfxTable::from_scus` → `SfxBank::from_descriptors`, see [`sfx-table.md`](../formats/sfx-table.md)) and plays through the per-scene music VAB. The live battle loop drives it: each `BattleSfxCue` drained from `World::drain_battle_sfx_cues` is enqueued into the director's scheduler at its `timing_frames` delay, and one `tick_sfx_frame` per simulation tick advances the queue and keys matured cues on through the SPU. Cues touch only the SPU (no RNG), so battle determinism is unaffected; a missing bank / VAB / free voice silently drops the cue.

Implementation: [`crates/engine-audio::sfx`](../../crates/engine-audio/src/sfx.rs); the host-side bank decode + per-tick drive live in `crates/engine-session` (`AudioBgmDirector::{set_sfx_bank,enqueue_sfx,tick_sfx_frame}`).

## Inventory item-use session

State machine that drives the "open inventory → pick item → pick target → use it" flow shared between the field menu and the battle command menu. Engines own a single `InventoryUseSession` for the lifetime of the inventory screen; per-frame they push input events and drain `InventoryUseEvent`s.

Filters items by `InventoryContext` (battle vs field - `usable_in_battle` / `usable_in_field` from the catalog), validates target compatibility (Revive needs a dead target; everything else needs a live one), and folds the resolved `ItemOutcome` into the engine's world state via `World::use_item`.

Implementation: [`crates/engine-core::inventory_use`](../../crates/engine-menus/src/inventory_use.rs).


## Encounter system

Per-scene random-encounter trigger. Engines own one `EncounterSession` per active field scene; the field-step path calls `on_step(rng_word)` each step the player moves. The session brackets the transition with five phases:

| Phase | Drives |
|---|---|
| `Idle` | Steady state. Steps roll against the table; safe zones suppress. |
| `Transition` | Roll succeeded; `transition_frames` (default 32) of camera-shake / fade-out. |
| `Triggered` | Engine drains the resolved `EncounterRoll` and loads the battle scene. |
| `Battling` | Battle is running; tracker is suspended. |
| `Grace` | Post-battle "no immediate re-encounter" window (`grace_frames`, default 30). |

`EncounterTable` holds the per-scene rows + 1/256 trigger rate + safe-zone rectangles. The accessory / status modifiers scale the effective rate multiplicatively via `EncounterTracker::set_rate_modifiers` - the statically pinned `FUN_801D9E1C` shifts (High Encounter passive `0x3B` = `<<2`, Low Encounter `0x3C` = `>>1`, system flags `0x1D`/`0x1E` = `<<1`/`>>1`; see [encounter.md](../formats/encounter.md#random-encounter-trigger-path)), refreshed from the party ability mask + flag bank each step. (An earlier additive `add_rate_bias` knob modeled accessories that don't exist in retail; it is removed.)

Implementation: [`crates/engine-battle::encounter`](../../crates/engine-battle/src/encounter.rs).

### The session is a bracket, not the roll

On a scene whose MAN carries encounter *regions* - which is every field area
that fights - the roll does not come from the session at all. It comes from
`RegionEncounterTracker` (the faithful `FUN_801D9E1C` model: per-region rate
counter, formation-range pick, one-step anti-repeat), and the session supplies
only the `Transition -> Triggered -> Battling -> Grace` bracketing around it.

That asymmetry has a failure mode worth naming, because it does not look like
one from either side. The region tracker's trigger branch is **destructive**:
it draws RNG, latches the anti-repeat formation and re-seeds its counter before
returning the pick. A host that dropped `World::encounters.session` after scene entry -
`World::begin_new_game` clears it, and `play-window --seed-party` runs that
*after* `enter_field_live` - therefore left the tracker rolling into a null
sink, and each roll was a fight that happened and was then thrown away, with no
transition drawn and nothing logged. `World::on_field_step` now re-installs a
bare bracket (`World::install_encounter_bracket`) rather than dropping the
pick, and every remaining way a roll can fail to become a battle logs at error:
an unregistered formation in `begin_encounter_battle`, a scripted arm with no
session, and a table/def id mismatch caught at `install_man_encounter` time.

The two id spaces the roll crosses - the MAN formation-row index the roll
produces and the `World::tables.formation_table` key the battle load resolves - are
pinned equal across the whole scene corpus by
[`crates/engine-core/tests/scene_encounter_formations_disc.rs`](../../crates/engine-core/tests/scene_encounter_formations_disc.rs),
which also carries the New-Game-reset regression.

`World::force_encounter(row)` arms a named row through that same bracket. It is
the engine side of `play-window --battle` ([playing-and-viewing.md](../guides/playing-and-viewing.md#getting-into-a-battle-on-purpose)),
and it deliberately does not shortcut into `enter_battle_from_formation` - a
harness that skips the path it verifies proves nothing about it.

### Scripted-battle entry (`3E FF <row>`)

The scripted boss fights enter through field-VM op `0x3E` with `op0 = 0xFF` -
or any `op0 < 100`, which runs the same body (the arm reads `op0` only to fork
off the `>= 100` door-warp; see
[`script-vm.md`](script-vm.md#0x3e-scripted-battle-op0--100)): the case-0x3E
arm (`FUN_801DE840`, field overlay) sets
the SYSTEM entity's 5-state SM to Activating (`sys_ctx[+0x8A] = 1`), points its
encounter-record slot at the per-scene MAN formation-table row `op1`
(`sys_ctx[+0x94] = *(ctrl+0x20) + op1 * *(ctrl+0x5D) + 1`), and requests the
battle mode switch (`FUN_8003CE08(0xE)`); the entity tick `FUN_801DA51C`'s
confirm state then copies the row into the battle formation cell `0x8007BD0C`.
The boss rows sit **outside** every region's rollable
`[base, base + count)` slice, so they can only enter through this op, and they
carry a non-zero first header byte - the predicate the confirm state ORs bit
`0x80` of the per-battle flags byte `DAT_8007BD60` on (see
[encounter.md](../formats/encounter.md#the-per-battle-flags-byte-dat_8007bd60)).
That bit is what gives a scripted fight the `SpinUpParticles` battle intro and
the transition's second audio cue instead of the random-encounter default; the
port carries it per formation row as `FormationDef::header_flags` /
`per_battle_flags()`, so it survives from the MAN parse to the intro. `rikuroa`
rows 16/17 read `01 00 00` where all sixteen of its random rows read `00 00 00`:

| Scene | Beat record | Op | Formation row | Contents |
|---|---|---|---|---|
| `garmel` | `P2[12]` (C1 gate `[0x198]`, self-latching) | `3E FF 09` | 9 | lone **Zeto** (`0x4B`) |
| `garmel` | `P2[11]` (C1 gate `[0x195]`) | `3E FF 08` | 8 | lone **Songi** (`0x4C`) |
| `rikuroa` | `P1[3]` (the Caruban stager, after its `52 89` marker SET) | `3E FF 11` | 17 | lone **Caruban** (`0x49`) |

This dissolves the "boss battle-id global" hypothesis for these fights: the
formation is the scene's own MAN encounter-section row, selected by index from
script bytes. Live-capture pinned twice over: the Zeto capture pins the
*writer* (the formation-store `ra` sits in `FUN_801DA51C`'s record-copy body
while `0x8007B7FC` stays silent), and poll-tier playthrough captures pin the
*values* - at battle entry the formation cell `0x8007BD0C` reads exactly the
lone id for all three rows (`0x49` in `rikuroa`, `0x4C` then `0x4B` in
`garmel`), with `0x8007B7FC` never observed non-zero across whole-chapter
sessions spanning a dozen scripted boss entries.

#### `DAT_8007b7fc` is a writer-less debug forced-battle id

No retail code writes `DAT_8007b7fc`. A capstone sweep of `SCUS_942.54` plus
every extracted static overlay (`crates/asset/data/static-overlays.toml` set)
covering absolute lui/addiu/ori-tracked stores, gp-relative stores against the
SCUS `gp = 0x8007B318` (`0x4e4($gp)`), and constant address-materialisation
into any register finds **no store and no materialised address** - only
readers. The same sweep pointed at the game-mode word reproduces its known
static stores, so the null result is not a tool artifact.

The readers give the global its role. Battle init `FUN_80055b6c` reads it
after clearing the per-battle state block: non-zero routes through
`FUN_80055b20` + `FUN_8005567c`, which seed the battle formation cells
`DAT_8007BD0C..0F` (and the sibling `DAT_8007BD10` array) **from the id
itself** - bypassing the encounter record entirely, with special-case
formations for ids `0xA2..0xA4` and a canned default when the id reads zero
at the final check. And the battle-exit mode selector `FUN_80046A20` reads it
(at `0x80046ddc`) before its three-way mode store: non-zero routes to the
`game_mode = 0` store - the **debug menu** - instead of the field/arena
returns. A set id would enter a forced formation and exit to the debug menu;
retail never sets it, so it reads `0` everywhere and both arms are
dev-harness residue (the same harness the mode-18/19 game-over rows belong
to; see [Party wipe + the game-over overlay](#party-wipe--the-game-over-overlay)).

The carrier differs per boss. The garmel fights ride **partition-2 beat
records** (spawned by the gated record dispatch). The Caruban op instead lives
in a **partition-1 boss-stager placement**: `P1[3]` of the rikuroa streaming
carrier is a parked special-model placement (SJIS locals ノア/Noa) whose own
record opens on a `SysFlag.Test 0x142` park gate, stations its actor at the
nest tile via its own `0x4C 0x51` leg, self-suspends on a `4C 85` halt-acquire,
and carries the beat body (`52 89` staged-marker SET -> `3E FF 11`). No
script-side un-halt poke to the stager channel (`B2 10 0A`) exists anywhere in
the MAN, so the resume is the engine-side approach dispatch: the locomotion
touch (`FUN_801d5b5c`) / interaction probe (`FUN_801cf9f4`) runs the placed
actor's record.

Engine port: `World::trigger_scripted_battle(row)`
([`crates/engine-core::world::encounters`](../../crates/engine-core/src/world/encounters.rs)),
reached from the field-VM host's `scripted_battle` arm for `op0 == 0xFF` and every `op0 < 100`. The
formation resolves against the rows `install_man_encounter` registered at scene
entry (with the PROT 867 archive stats merged; the v12 dungeons resolve their
encounter section from the streaming variant MAN, their only carrier), and the
battle enters through the same immediate latch the field-carrier SM uses - no
field step, no synthetic boss formation id. Boss-stager placements are derived
from the MAN at scene entry (`man_field_scripts::boss_stager_placements` ->
`World::install_boss_stagers_from_man`: the `3E FF` site, the park-gate flag
and the station tile all decode from the record's own bytes) and run on
approach/interact via `World::run_boss_stager_record` - the whole rikuroa
chain, staged marker included, lands from script bytes. Oracles:
[`crates/engine-core/tests/organic_zeto_encounter_disc.rs`](../../crates/engine-core/tests/organic_zeto_encounter_disc.rs),
[`crates/engine-core/tests/organic_beat_records_disc.rs`](../../crates/engine-core/tests/organic_beat_records_disc.rs).

## Encounter trigger - runtime memory layout

A pre/post encounter save pair (one frame walking the `map01` field scene; the next frame with battle just initiated, same `map01` scene) pins the runtime memory layout of an encounter trigger. The `mednafen-state diff` over `0x801C0000..0x80200000` surfaces:

| Range | Bytes changed | What it is |
|---|---:|---|
| `0x801CE808..0x801F3818` | ~133 KB | Battle overlay loaded into RAM (single contiguous region) |
| `0x801C9370..0x801C9900` | ~200-500 B | 8-slot battle actor pointer **table**, stride **4** (eight pointers = 32 bytes); every consumer indexes it `<< 2`. The `0x590` span is the region that changes across the diff, not the table's size. |
| `0x80083000..0x80084000` | ~600 B | Scene-bundle / sound-pool: encounter formation + BGM resolution |

The active scene-name table at `0x80084540` (CDNAME label + scene index) is **identical** between the pre-encounter and post-encounter saves - the battle is layered on top of the field scene rather than swapping it out. Engines that drive the field-to-battle transition therefore preserve the active-scene state and only resolve the formation + battle overlay.

Codified as constants in [`crates/engine-core::capture_observations::encounter_trigger`](../../crates/engine-system/src/capture_observations.rs); a disc-gated test in [`crates/mednafen/tests/real_saves.rs`](../../crates/mednafen/tests/real_saves.rs) (`encounter_trigger_diff_loads_battle_overlay`) exercises the real save bytes.

## Inventory (`crates/asset` page-banked layout)

Battle reads inventory through the same page-banked structure the field VM's op `0x3B` `SET_ITEM_COUNT` writes: 16 entries × 16-bit per page × 0x414-byte stride. The page index is the high nibble of the slot byte; the entry index is the low nibble.

The page-banked inventory state lives in the 512-byte region at `[0x80085718 .. 0x80085918)` - adjacent to the fourth-flag-bank bitfield at `DAT_80085758` (see [field VM](script-vm.md) → "fourth flag bank"). The field VM's op `0x4C` sub-3 sub-2 zeros the entire region.

## Status effects

Per-actor status conditions inflicted by enemy attacks or art `enemy_effect` bytes. The retail engine stores per-status timers and tick-damage values in the battle-actor struct around `+0x130`; the layout is per-flag and not captured in any single overlay dump.

Conditions are named with the game's in-game ailment terms (the `enemy_effect` byte is the on-disc art-record value). The `Retail effect` column is the published behaviour from the Legaia wiki status pages. The poison **tick formulas are pinned** from the per-round DoT ticker `FUN_801E752C` (see [battle-formulas](battle-formulas.md) § "Per-round status DoT ticker"); the `Default duration` values remain engine-side approximations (no retail per-status duration table is in any single overlay dump). The `Engine` column flags where this port diverges from retail.

| Status | byte | Default duration (from-scratch) | Retail effect (wiki) | Engine |
|---|---|---|---|---|
| Toxic | `1` | 4 turns | "Deadly Poison": HP drains faster than Venom AND attack/defense drop | `min(max_hp/16, 256)` tick, never kills (bottoms at 1 HP), suppresses Venom's tick while active (`FUN_801E752C`); combat rolls ×7/10 (`FUN_801DD864` bit 2), mirrored as ATK & DEF ×0.7 |
| Numb | `2` | 3 turns | Paralysis: cannot act; clears on being hit or after some turns | full block + clear-on-hit (enforced, same shape as Sleep) |
| Venom | `3` (Other) | 6 turns | "Poison": HP drains (lesser than Toxic) | `min(max_hp/32, 128)` tick, never kills (`FUN_801E752C`); combat rolls ×9/10 (`FUN_801DD864` bit 1), mirrored as ATK & DEF ×0.9 |
| Sleep | `4` | 3 turns | Asleep; wakes when hit | block + clear-on-hit (matches) |
| Confuse | `5` | 3 turns | Acts uncontrollably / random target | a confused action (monster *or* party physical, plus monster casts) retargets to a random living member of the opposite side (`FUN_801E7320`); a confused party member auto-acts a physical strike with no command menu - an engine stand-in (retail's party-side delegated action pick is unpinned; see [battle-action](battle-action.md) § AI-delegated party members) |
| Curse | `6` | 4 turns | Blocks Magic | blocks Magic (matches) |
| Stone | `7` | whole battle (255) | Petrification: cannot act, cannot be damaged, counts as defeated; lasts the whole battle (no in-battle cure; escape restores) | block + whole-battle duration + invulnerability at every damage entry point + counts-as-defeated in the wipe checks; escape restores (see below) |
| Faint | `8` | until cured | KO at 0 HP: collapse, no actions; revived only by Phoenix / revive Magic | block + `until cured` (matches) |

The **stat debuffs** a player's Seru magic inflicts (DEF / AGL / ATK / SPD / INT / MP down, 5-20% per hit by magic level) are a separate mechanism with no `+0x16E` bit - the element-keyed [side-effect](battle-formulas.md#seru-magic-side-effects---the-element-debuffs-fun_801f3d3c--the-finisher-switch), whose "immunities" are the scripted-fight boost profile, not a monster field.

Implementation: [`crates/engine-vm::status_effects`](../../crates/engine-vm/src/status_effects.rs). The per-tick `StatusEvent` stream feeds back into the engine's HUD pipeline; engines call `World::tick_status_effects` once per round and consume `StatusEffectTracker::drain_events()` for log lines. The live battle loop - the only battle driver - ticks it once per round, at the initiative round boundary (when no living actor still holds an initiative key, just before the keys reseed).

The tick folds the Venom / Toxic DoT into `BattleActor::hp` with the retail never-kill clamp - a tick that would reach 0 leaves the actor at 1 HP instead (`FUN_801E752C` subtracts `current − 1` before applying the per-status cap), so poison alone never downs an actor. It draws no RNG, so it never perturbs the reseed RNG stream.

**Stone escape-restore.** The retail run band (`FUN_801E295C` case `0x64`, successful-escape branch) walks the party slots and floors any 0-HP actor at 1 - the concrete mechanism behind "a petrified member returns to normal when the party escapes". The engine models it as a tracker-level Stone clear when the battle ends with `BattleEndCause::Escaped` (Stone's runtime bit representation is not pinned in the dumped corpus - see `status_effects.rs`).

**Turn-level enforcement (live loop).** The action-blocking columns above are
enforced at the turn grant, not just modelled. When the live battle loop
(`World::live_battle_tick`) hands a combatant its turn, an actor carrying a
`blocks_actions` status (Numb / Sleep / Stone / Faint) **loses the turn** - its
initiative key is already consumed, so play passes on and the SM stays at
`EndOfAction` with no action armed (the status duration ticks once per round at
the initiative boundary, so the affliction wears off). A caster carrying a
`blocks_magic` status (Curse /
Faint) that the monster AI picks a cast for **falls back to a physical
strike** (`World::take_monster_turn`, mirroring the MP-affordability fallback).
The gate reads `StatusKind::blocks_actions`/`blocks_magic` via
`World::actor_blocked_from_acting`/`actor_blocked_from_magic`. The party side
mirrors this: a silenced/petrified player who picks **Magic** can't open the
submenu - `World::build_battle_spell_session` returns `None` for a `blocks_magic`
caster, so the caller bounces back to the command menu (the same graceful
fallback it uses when there's no caster record).

### The `+0x16E` status halfword - retail writer inventory

The per-actor status halfword `actor[+0x16E]` has a fully-enumerated writer set in the static
images (`SCUS_942.54` plus every overlay in `crates/asset/data/static-overlays.toml`, swept for
every `sh`/`sb`/`sw`/`swl`/`swr` whose offset window covers `+0x16C..+0x171`, every pointer
precompute `addiu r,r,0x16E`, and every `ori`/`sllv`-shaped bit-set within reach of a `+0x16E`
access). Lifecycle writers:

- **Battle-start seed** - `0x80051720` copies the persistent per-character status word (char
  record `+0x6F6` off `0x80084140`) into `+0x16E`. The mirror runs the other way per frame
  (`sh v0,0x6f6` sites paired with each cure in `FUN_8004CE2C`, and the conditional persist
  `0x80047680` in `FUN_80047430`, gated on bits `0x404`); `+0x6F6` itself is only ever written
  as a copy of `+0x16E` or by those same cure masks, so it originates nothing.
- **Battle-exit / KO clears** - `sh zero,0x16e` at `0x80046EB0` (`FUN_80046A20` per-party exit
  clear), `0x80040EB8`/`0x80040FDC` (death cleanup).

**Infliction appliers.** Two overlay-resident legs share one kind→bit map, keyed by a
status-kind byte (`see ghidra/scripts/funcs/overlay_battle_action_801ec3e4.txt` /
`overlay_battle_action_801e09f8.txt`):

- the on-hit leg inside `FUN_801EC3E4` reads the **art record**'s kind byte
  (`lbu v0,0x7a(t4)` at `0x801EE3D4`, `t4` reloaded from the `param_2` spill at `0x54(sp)`)
  and dispatches at `0x801EE448`. That is the party-caster direction;
- the special-attack leg inside `FUN_801E09F8` reads `+0x0A` off `ctx[+0x1014]`
  (`0x801E1584`) and dispatches at `0x801E1600`. `ctx[+0x1014]` is not a spell descriptor:
  `FUN_801DEA50` writes it (`sw v0,0x1014(a0)` at `0x801DF284`) with the **move-power record**
  address for the acting actor's queued move id - `0x801F4F5C + map[actor[+0x1DF]] * 26`,
  the `x26` built as `13a << 1` at `0x801DF264..0x801DF274`. So the kind byte is the
  move-power record's `+0x0A` [impact-effect selector](../formats/move-power.md#record-layout-26-bytes),
  and the arm fires when that strike arm's phase byte reaches the impact value
  (`lbu a2,0x24e(v0)` / `li v0,0x3` / `bne` at `0x801E156C..0x801E1574`).

| kind | bit written | writer PCs (hit leg / special leg) | gate |
|---|---|---|---|
| `1`, `2` | none directly - only the `+0x21F` latch (below) | consumed by `FUN_80047430`: `ori 0x380` + `sh` at `0x80047F88`/`0x80047F90`, then `+0x21F` cleared | `+0x21F != 0` |
| `3` | `ori v0,v0,0x1` | `0x801EE4C4` / `0x801E1654` | `rng & 7 == 0` |
| `4` | `ori v0,v0,0x2` | `0x801EE508` / `0x801E1684` | `rng & 7 == 0` |
| `5` | one random bit of `0x38` - `1 << ((rng % 3) + 3)` via `sllv`/`or` | `0x801EE618`/`0x801EE61C` / `0x801E1738`/`0x801E173C` | target slot `< 3` (`sltiu`), then accessory-passive immunity bits `0x01000000`/`0x10000000` of char `+0x6BC` skip - the read precedes the roll, so a guarded target draws no RNG |
| `6` | `ori v0,v0,0x1000` | `0x801EE6C8` / **absent** | `rng & 3 == 0` (hit leg only) |
| `>= 7` | nothing - falls through with no bit write | - | - |

**The two legs' ladders are not the same length.** The hit leg tests `4`, `< 5`, `3`, `5`, then
`6` (`li v0,0x6` / `beq` at `0x801EE478`..`0x801EE47C`). The special leg's ladder stops at `5`:
`0x801E1620` compares against `5` and otherwise jumps straight to the join at `0x801E178C`,
with no `6` arm anywhere in the routine. **An enemy special attack therefore cannot inflict
Curse** - only the physical/arts leg can. (The special leg's `3` comparison reuses register
`a2`, which still holds the impact-phase byte `3` the `bne` at `0x801E1574` just proved equal
to `3` - a register-economy trick, not a second constant.)

**Engine.** The special leg's ladder is ported as
`engine-core::world::battle::monster_ai::enemy_impact_status_proc`, driven by
`World::apply_enemy_move_status` off the installed `MovePowerCatalog` at the end of a monster
cast. Because the id→index map is special-attack-only, a monster's *basic* attack resolves to
the all-zero record 0 and inflicts nothing without a separate guard.

Kinds `1..5` additionally latch `actor[+0x21F] = kind` and stage the effect word `actor[+0x4]`
from the table `0x801F53D4[kind-1]` (hit leg `0x801EE3E8..0x801EE430`, guard `sltiu v0,v0,6`
at `0x801EE3E0`; cast leg `0x801E15A4..0x801E15EC`).

Other setters: `ori 0x4` at `0x80041CF4`/`0x80041DE4` and `ori 0x1000` at
`0x80041EE8`/`0x80041F84` (SCUS band `0x80041...`), the each-frame delegation `ori 0x380` at
`0x8004D118` (`FUN_8004CE2C`) and `0x80047F88` (`FUN_80047430`), plus `ori 0x380` / `ori 0x1`
copies of the same shapes in the slot-B battle-support images (PROT 0902/0903/0905/0907, e.g.
`0x801F7F50` in 0907's image).

**Bit `0x400` has no retail setter.** The sweep above finds *no* instruction in any static
image that sets bit `0x400` (or `0x800`, or `0x40`) of `+0x16E` - not by immediate, not through
the `sllv` appliers (whose shift ranges are `(rng%3)+3` → bits 3..5 only), not via the kind
switch (kinds `>= 7` write nothing), not through `+0x6F6`, and not by any unaligned store
(zero `swl`/`swr` hits near the offset). Every `0x400`-touching write is a **clear**:

- the accessory-passive cure `andi 0xFBFF` at `0x8004CFCC` (`FUN_8004CE2C`, keyed on char
  passive word `+0x6C0` bit `0x08000000`);
- a dedicated per-round waker: `FUN_801F45A4` loops the 7 actor slots and clears exactly bit
  `0x400` behind a `rng & 7 == 0` roll (`andi v0,v0,0xfbff` at `0x801F4610`, `sh` `0x801F4614`;
  the instruction PCs sit inside `FUN_801F45A4` - the neighbouring `FUN_801F452C` this clear
  was once attributed to is the 30-instruction magic-level-increased banner composer that
  ends at `0x801F45A0`. `see ghidra/scripts/funcs/overlay_0898_static_801f45a4.txt`);
- item/spell cure masks `andi 0xFB84` / `0xFF84` / `0xFFFC` in the slot-B battle-support
  images (e.g. `0x801FC6AC` in 0902's image);
- the on-hit strip `andi 0xF07F` at `0x801EDA5C` (`FUN_801EC3E4`) and its bit-`0x4`-gated
  sibling at `0x801DE2E8..0x801DE2FC` (`FUN_801DDB30`);
- the battle-exit and KO clears above.

So bit `0x400` is **latent content**: it has a complete consumer/curer lifecycle (hit-strip
class membership, a dedicated RNG waker, an accessory immunity, item cures, a battle-exit
clear) but no infliction path in the shipped static images - it can only enter play through
the persistent `+0x6F6` mirror, which nothing in the images seeds with it. The "which function
sets `0x400`" question dissolves into this negative.

## AP / Spirit gauge

Each character has a per-turn AP budget that limits how many art commands they can chain. The retail engine reads this from the character record's `+0xC9` (`current_ap`) and `+0xCA` (`bonus_ap`) bytes. The engine's `ApGauge` adds `+5` of its own command units for a Spirit press; that constant is not a retail value - what a retail Spirit turn does to the gauges is the subsection [below](#what-a-spirit-turn-does-to-the-gauge-and-what-it-draws).

The base AP grows by 1 each 10-level milestone (level 1..9 → 4 AP, 10..19 → 5 AP, …, 60+ → 10 AP capped; `ap_base_for_level`). The engine seeds each party member's `ApGauge::base_ap` from that formula at battle entry - `seed_party_battle_stats` reads the live character level alongside the attack / defense fold, so a higher-level character chains more arts per turn. The round-start `reset_party_ap` then refills `current_ap` to that base, and Fury Boost extends from / reverts to it.

| Action constant range | AP cost | Notes |
|---|---|---|
| `0x00` Nothing | 0 | placeholder |
| `0x01..=0x05` | 0 | system actions (Item / Magic / Attack / Spirit / Escape) |
| `0x0C..=0x0F` | 0 | direction bytes (free) |
| `0x19` Regular Art Starter | 1 | |
| `0x1A` Special Art Starter | 1 | |
| `0x1B..=0x32` | 1 | per-character art body |

Implementation: [`crates/engine-battle::ap_gauge`](../../crates/engine-battle/src/ap_gauge.rs). The `World` carries a `[ApGauge; 3]` (one per party slot); engines call `World::reset_party_ap` at turn start.

### What a Spirit turn does to the gauge, and what it draws

The gauge the arts entry actually spends is the battle actor's action gauge `+0x154` / `+0x156` ([`arts-command-gauge.md`](arts-command-gauge.md#where-the-gauge-pool-comes-from)), seeded at battle setup from the character's live AGL. A Spirit turn extends it three ways, all read off `FUN_801E295C`:

- **The seed arm** (`0x801E2F54..0x801E3024`) sends category `4` straight to `0x46` - never through the `0x3C` item pre-arm, so it raises no readout bar (record 7). It sizes placement record `0x0F` (the AP bar) to `+0x154 - 6` and raises it with the AP plate `0x52`, parking the plate's handle at `0x801F6968`.
- **The band** (`0x46..0x48`): `0x46` writes the camera depth `ctx[+0x6D0] = 0x800` (the Spirit close-up), stages the extended gauge `min(+0x156 * 7 / 5 + 8, 0x120)` and the Spirit target `+0x170 + 0x20` (`+0x28` / `+0x23` under the `+0xF8` passives `0x200` / `0x100`); `0x47` grows the bar one frame step at a time to the extended gauge less 6 and climbs the plate; `0x48` finishes the plate. The `0x51` teardown unloads both.
- **The Done band** pays the per-action accumulator `+0x224` into Spirit: `8` for every action, `0x20` for a Spirit turn, plus the two passives, capped at 100. The round boundary then restores a Spirit-charged actor's `+0x154` to the extended gauge, which is the pool the next arts entry opens on.

Retail captures of the band agree (`ctx[+0x6D0] = 0x800`, a 188-wide bar under a 194 AGL, `+0x154` already extended on the following turn). The engine draws the pair through the arts-entry chrome builders on both hosts (`World::spirit_gauge_view`), so the bar is the arts bar: one pixel per AP between its end pieces. The aura the clip's effect script spawns (prototypes `0x07` / `0x08`) is a VDF-morphed mesh on `vdf.dat` entry 12 - see [`effect-vm.md`](effect-vm.md#battle-effect-parts-morph-through-vdfdat).

## Battle stat aggregator

From-scratch port of `FUN_80042558`. Walks the 8 equipment slots, sums modifiers into the actor's resolved attack / UDF / LDF / accuracy / evasion, ORs equipment ability bits into the global 4×u32 mask, then folds in status-effect modifiers (Toxic reduces ATK + both defenses by ~12.5%, Confuse halves accuracy, Numb / Sleep / Stone / Faint zero evasion and block actions, Curse / Faint block Magic).

Implementation: [`crates/engine-battle::battle_stats`](../../crates/engine-battle/src/battle_stats.rs). The pure function `compute_battle_stats(record, table, statuses, modifiers) -> BattleStats` is deterministic and side-effect-free - engines call it once per turn-start.

## Item catalog

Typed catalogue of inventory items the battle / field menu consults. Each entry has an `ItemEffect` describing the side-effect (Heal / Cure / Revive / Stat-up / Spirit-up / Capture / Escape / Damage / KeyItem). The vanilla catalog ships 19 entries covering every category.

`apply_effect(effect, &TargetSnapshot) -> ItemOutcome` is the pure resolver - engines fold each `ItemOutcome` into world state through whatever runtime path they have for HP / status / AP / inventory.

`World::use_item(item_id, target_slot)` is the shared apply kernel (battle item
command + field menu both route through it): it builds the `TargetSnapshot` from
the live actor, resolves the outcome, and writes it back. `StatRaised` (the
permanent stat-up consumables - Power Tonic, Vital Tonic) is applied via
`apply_stat_raise`: an HP/MP-max raise bumps the persistent character record
**and** the live actor's caps (refilling the gained amount); a combat-stat raise
lands in the record's `+0x110` live-stat block that `seed_party_battle_stats`
re-derives from, so the gain shows immediately and survives a save. Combat stats
cap at the record's per-stat cap constant; HP/MP max at 9999. (These items are
field-only and absent from the captured battle traces, so the exact retail cap /
refill rule is not byte-pinned - the engine uses self-consistent rules.)

Implementation: [`crates/engine-core::items`](../../crates/engine-menus/src/items.rs).


## Battle round lifecycle

The live round step is `BattleRound::boundary(&mut world)`, the port of `FUN_801D88CC`, which the live loop runs at each round boundary (`world/battle/loop_driver.rs`): it re-arms the once-per-pass monster flee checkpoint, restores every slot's gauge and clears its action stream, and re-picks each target through `FUN_801DB8B4`'s first-living-monster scan. `BattleRound::begin` (reset the AP gauges, recompute per-slot `BattleStats`, write attack / UDF / LDF back into `World::battle`) and `BattleRound::end` (tick statuses, fold Toxic / Venom damage, count the deaths) survive from the removed battle runner and are called only from tests.

The returned `BattleRound` carries per-slot `action_blocked` / `magic_blocked` arrays the action validator filters command input against (Numb / Sleep / Stone / Faint actors lose action; Curse / Faint actors lose Magic).

Implementation: [`crates/engine-core::battle_round`](../../crates/engine-core/src/battle_round.rs).

### Monster AI (`FUN_801E9FD4` action picker + `FUN_801E7320` target resolver)

Retail monster AI is two routines in the battle overlay:

- **`FUN_801E9FD4` - action picker.** Called per monster from `FUN_801DABA4`
  (`recompute_battle_order`). Its **generic decision core** counts the live
  global magic ids in the monster record's `+0x21..=+0x23` array, rolls
  `rand % (1 + live_count)`; a `0` selects a physical strike (target
  `rand % party_count`), otherwise it picks magic id `magic[roll-1]`, gates on
  affordability (`actor[+0x150] MP < spell_table[id*0xC + 3]` cost), and resolves
  the target by the spell's shape byte `spell_table[id*0xC + 2] & 0x60`
  (`0x40` = one enemy → random party member; `0x60` = all enemies → class `8`;
  `0x20` = all allies → class `9`; `0x00` = one ally → most-weakened-ally HP
  scan). After the core, a large `switch` on `DAT_8007BD0C[slot]` can
  **override** the choice with bespoke scripted casts (hard-coded ids
  `0x50/0x51/0x52/0x53/0x6f/0x40`, cooldowns in `DAT_801C8FE0`).
  `DAT_8007BD0C[slot]` is the **per-slot monster id** - `FUN_801DA51C` fills it
  from the encounter record's `[+4 + slot]` ids (the `[3 reserved][count][ids]`
  format) - so each `switch` case is bespoke AI for a specific monster id, not
  an abstract AI-type.

  One hard data constraint hides in the cast path: after a magic choice the
  picker counts the block's **rollable castable entries** (record `+0x4C`
  entries with id `0x0C..=0x1F` and AGL cost `!= 0xFF`) into `sp+0x10` and
  rolls `rand % count` (`div` at `0x801EA30C`). A count of **zero** executes
  the compiler's divide-by-zero guard - `break 0x1C00` at `0x801EA318` - and
  the BIOS parks the machine forever (vsync alive, pads dead). Retail data
  never has that shape (every caster's block carries rollable entries), so
  this is a constraint on *rebuilt* blocks: a modded block whose `+0x21`
  magic array is live must keep at least one rollable castable entry, which
  is exactly what `legaia_asset::monster_archive::slim_castables` enforces
  (see [randomizer.md](../tooling/randomizer.md)).
- **`FUN_801E7320` - target resolver.** Called from the action SM
  (`FUN_801E295C`) at `ActionSeed` as the `monster_setup` hook, but only for
  monster actors with `actor[+0x16e] & 0x380 != 0`. It reads the targeting class
  the picker left in `actor[+0x1DD]` and expands it: class `0..2` → a living
  monster slot (`rand % monster_count + 3`, `addiu a0,v1,0x3` at `0x801E73B8` -
  pool slot `3` is the first monster whatever the party size); class `3..6` → a living
  party slot (`rand % party_count`); class `8`/other → a `rand % 3` gate
  selecting all-target codes `8`/`9` or self. ctx fields: `ctx[+0]` = party
  count, `ctx[+1]` = monster count, `ctx[+0x13]` = active slot. Dumps:
  `ghidra/scripts/funcs/overlay_battle_action_801e9fd4.txt`,
  `overlay_battle_action_801e7320.txt`.

The from-scratch engine ports it across `engine-core`:

- `World::pick_monster_action` is the action picker's **generic core** (real
  RNG, real `magic_attacks`, spell-shape targeting through the catalog's
  `SpellTarget`).
- `monster_ai::decide` is the **per-monster-id `switch`** - keyed by monster id,
  it overrides the generic choice with the bespoke scripted casts (low-HP
  self-heal, MP-gated nukes, multi-phase boss scripts), reading/writing the
  battle-scoped `MonsterAiState` (per-monster cooldowns `DAT_801C8FE0` - armed
  once per battle, with no per-round re-arm: retail clears the latch array only at
  battle init in `FUN_80055b6c`, so a boss self-heals at most once per fight; the
  `DAT_801C8FE4` phase counter; the recent-target ring).
- `monster_ai::apply_recent_target_ring` is the post-switch anti-repeat ring.
- `World::resolve_monster_target` is the exact `FUN_801E7320` port, wired as the
  `monster_setup` hook.
- `World::advance_battle_mode` is the `ctx+0x28a` writer - the battle-action SM's
  `case 0xFF` (`_DAT_8007BD24[0x28A] += 1`), the boss phase-transition
  pseudo-action. Advancing the mode walks a multi-phase boss to its next
  scripted cast on the following turn (`World::battle_mode` reads the counter).

The picker drives the live loop's monster turns, folding a chosen cast through
`cast_spell_on_slots` (the shared player/monster cast path) and parking the SM at
`EndOfAction`. Scripted casts emit retail spell ids; they fold when the active
catalog knows the id (the disc spell table; a capture-class special resolves
off its disc record through `World::monster_cast_def`) and otherwise degrade to
a physical strike. `SpellCatalog::vanilla` is a disc-free test fixture; no boot
catalog carries it.

**Faithful default = uniform-random single target.** Retail's `OneEnemy` /
physical target is a uniform random living party member (`rand % party_count`,
re-rolled past downed slots). An **opt-in, non-faithful** QoL toggle
(`World::toggles.smarter_monster_targeting`, off by default; `legaia-engine play-window`
reads `LEGAIA_SMART_MONSTERS=1`) instead redirects a single-target attack to the
lowest-HP living member. It is RNG-neutral by construction: the faithful random
pick is still rolled in full (magic roll, target roll + re-roll loop, scripted
override, anti-repeat ring), and only the resolved single party slot is replaced
afterwards - so the RNG stream and call count are byte-identical to the faithful
path, all-party / monster-band / self targets are never touched, and a run stays
deterministic. The default path is bit-for-bit unchanged.

**The two AI gates.** The `ctx+0x28a` battle-mode counter and the `actor+0x16e &
0x380` flag are distinct, and only the first is a monster behaviour the AI flips:

- **`ctx+0x28a` (battle mode)** gates the multi-phase boss cases. Its writer is
  the SM's `case 0xFF` (`_DAT_8007BD24[0x28A] += 1`), a scripted phase-transition
  action a boss issues at an HP/script boundary - **ported as
  `World::advance_battle_mode`**, so those cases activate once a boss script
  drives a transition (proven by the `0xB6` phase-walk test). `0` until then.
- **`actor+0x16e & 0x380`** is **not** a monster flag. `FUN_80047430` sets it
  only on **party** slots (`slot < 3`) whose status word `+0x00` has bit `0x2000`
  (Confuse/Charm), delegating that party member to the AI target resolver
  `FUN_801E7320`; the resolver runs only when it is set. A normal monster keeps
  `0x380` **clear**, so its `!ai380` scripted-cast cases fire and `monster_setup`
  stays dormant - exactly what the engine does (monster actors carry
  `field_flags == 0`). The set-`0x380` path (AI-driven party members) is a
  separate status-effect feature, not a flag the monster AI sets.

**Remaining gaps** (documented in `monster_ai`): a couple of cases touch actor
fields the engine doesn't fully consume yet. The `actor+0x170` **spirit-art
gauge** is modelled (`BattleActor::spirit_gauge`) and filled on every damaging
hit by the finisher's spirit stage (`spirit_gauge_fill`, see
[`battle-formulas.md`](battle-formulas.md)); monster `0x8A`'s AI now reads that
gauge as a charge gate - once it passes `0x31` the monster fires its `0x4E`
all-enemies cast and the gauge is clamped back to `0x32`
(`MonsterAiCtx::spirit_gauge` + `AiCast::spirit_gauge_writeback`, drawing no
RNG). Still unwired: the `'O'` (`0x4F`) boss that rewrites another actor slot,
and the capture-archive preload for spell ids `0x2E/0x2F`.

### Enemy-ally charm at the end-of-action gate (the charm battle softlock)

The randomizer's enemy-ally ("charm") feature rides the stock `0x380`
delegation flag plus one overlay word: the monster-wipe scan's down-mask at
`0x801E6638` widens from `andi v0,v0,0x4` to `andi v0,v0,0x384`, so a living
charmed monster counts as "down" and the player does not have to kill their
own ally to win (`legaia_patcher::enemy_ally`). That widen interacts with a
retail invariant inside the end-of-action gate (state `0x5A` of
`FUN_801E295C`), and the interaction is the pinned cause of the charm battle
hard-freeze.

**The retail invariant.** The state-`0x5A` wipe scans count a combatant as
standing while `+0x14C != 0 && (+0x16E & 0x4) == 0` (party loop
`0x801E6538..0x801E6570`, monster loop `0x801E6614..0x801E664C` with the
mask test at `0x801E6638`), and the initiative scheduler `FUN_801DABA4`
gates on the same predicate (dead-key zeroing `0x801DABD8..0x801DABF8`;
living-side scans `0x801DAD94..0x801DADC8` / `0x801DAE18..0x801DAE54` with
the identical `andi 0x4`). So under the retail mask an **alive** acting
actor at monster-wipe victory is always a party member: an alive, acting
monster would have been counted as standing by the very scan that fired the
wipe (`0x4` retail-marks a captured monster, an actor staged out of the
fight - never one mid-action).

**The victory arm leans on that invariant.** After the monster-wipe branch
sets the end signal (`0x801E6670..0x801E6680`: `DAT_8007BD71 = 0xFE`,
`_DAT_8007BD2C = 0`), it stages the win pose:

- `0x801E6688/0x801E6690` - `lhu a0,0x14C(s3)` / `bne a0,zero,0x801E6728`:
  a **living** acting actor keeps the acting slot unconditionally;
- `0x801E66A4..0x801E6724` - only a dead acting actor re-rolls
  `rand % ctx[+0]` (party count) until a slot with `+0x14C != 0` and
  `(+0x16E & 0x404) == 0` comes up (back-edges `0x801E670C`/`0x801E6720`);
- `0x801E6728..0x801E676C` - formation override: first monster id
  (`DAT_8007BD0C[0]`) `0xB3` forces the pose slot to `2`, `0xB4` to `1`
  (the Songi fights);
- `0x801E6770..0x801E6790` - reads the pose slot's character id from the
  **3-byte party roster** `DAT_8007BD10[slot]` and arms the win-pose "ME"
  archive side-band request `FUN_80055B4C(char_id*3 - 1)`
  (see [`summon-readef.md`](../formats/summon-readef.md#streaming-state-machine)).

**What the widen breaks.** With the `0x384` mask the two predicates
disagree: the scheduler still picks the living charmed ally, but the wipe
scan no longer counts it. When the ally's own action kills the last real
enemy, victory fires with a living **monster** (slot `3..6`) as the acting
actor - the alive-skip keeps the slot, and the roster read indexes past
`DAT_8007BD10[0..2]` into the adjacent globals (`0x8007BD13` pad byte,
`0x8007BD14..` the damage-popup accumulator). The stream request arm then
receives a garbage slot: char byte `0` arms request `0` (no transfer ever
starts for the win-pose staging), any other byte seeks
`((req-1) & 0x7F) * 0x10800` into `readef.DAT`/`summon.dat` - far past
either file for roster-adjacent values. Either way the battle wedges at the
victory hand-off. This state is unreachable in retail; it is a
randomizer-interaction defect, not a retail bug.

**Not the only battle freeze class.** A second, structurally unrelated one
lives in the done/cleanup band: state `0x51` refuses to decrement its exit
countdown while a party actor's displayed HP `+0x172` disagrees with its live
HP `+0x14C`, and that disagreement is permanent once the pending-bar-delta
accumulator `+0x10` reaches zero. The symptom is an endless battle-camera
orbit rather than a hard freeze, and the trigger is an HP write that skips the
bar bookkeeping - not a roster or targeting invariant. See
[battle-action.md](battle-action-exit-gates.md#the-0x51-exit-gate-and-the-hp-bar-settle-invariant).

**What the softlock is *not*.** The long-standing "unbounded reroll in
`FUN_801E7320`" theory is falsified as the cause. Both reroll loops
(`0x801E7370..0x801E73D8` over the monster band, `0x801E7418..0x801E747C`
over the party band) are structurally unbounded, but the scheduler's
living-actor predicate guarantees the acting `0x380` actor is alive - and
for the monster-band loop the acting charmed monster is itself an in-band
exit (a self-pick clears `+0x1DE`, turning the action into a no-op), while
the party band always holds a living member or the previous action's `0x5A`
would already have fired the party wipe. The resolver terminates with
probability 1 in every reachable state.

**Engine port.** `engine-vm::battle_action` `end_of_action` carries the
full gate: both wipe scans mask `0x4` (a captured, non-targetable monster
counts as down), `BattleActionCtx::charm_widen` models the `0x384` widen,
and `victory_pose_fixup` ports the victory arm with the corrected
invariant - the re-pick triggers whenever the acting slot is not a living
party slot (dead **or** a monster slot, the state the widen makes
reachable) and picks uniformly among eligible slots instead of
rejection-sampling, so it cannot spin. The win-pose staging surfaces as
`BattleActionHost::victory_stage(party_slot)` with the slot guaranteed
valid, and the Songi override as `BattleActionHost::first_monster_id`.
Dump: `ghidra/scripts/funcs/overlay_battle_action_801e295c.txt`.

## Captured stat-growth observations

The `mednafen-state diff` toolkit ([`docs/tooling/mednafen-automation.md`](../tooling/mednafen-automation.md)) over a magic-rank-up + character-level-up save triplet pins the per-byte footprint for Vahn (party slot 0). The observed deltas inside Vahn's character record at `0x80084708` (stride `0x414`):

| Event | Offset | Before → After | Interpretation |
|---|---|---|---|
| Magic-rank up (pre → post) | `+0x08` | `0x30 → 0x3C` | `spell_counter[0]` (+12), the u32 array entry - not a flag word |
| Magic-rank up | `+0x9C` | `0x09 → 0x0A` | magic-rank counter (+1) |
| Magic-rank up | `+0x10A` | `0x1B → 0x11` | low byte of `mp_cur` (cast cost spent) |
| Magic-rank up | `+0x161` | `0x02 → 0x03` | spell-level array (`spell_levels[0]` +1) |
| Level-up, 4-level jump (pre → post) | `+0x00` | `0x4F → 0x73` | unconfirmed (jump +0x24 doesn't match a single-level granularity) |
| Level-up | `+0x04..+0x06` | `0x016D → 0x02DA` | u16 LE XP delta (+365) |
| Level-up | `+0x10E` | `0x3A → 0x42` | low byte of `ap_cur` (AP / arts gauge refill, +8) |
| Level-up | `+0x11C..+0x12C` | six per-byte +1..+4 | per-stat increments at byte stride 2 |
| Level-up | `+0x130` | `0x02 → 0x03` | displayed character level (+1) |

The retail per-level growth source **is** in `SCUS_942.54`: the per-stat
98-entry curves at `DAT_800769CC` (stride `0x62`) + the parameter block at
`DAT_80076918` that selects each stat's curve row, read and applied by the
overlay level-up function `FUN_801E9504` (see
[`subsystems/level-up.md`](level-up.md#stat-gains)). The earlier writer-search
came up empty because it scanned the `magic_level_up` *display* overlay, not the
victory-path applier; the "Seru struct +0x74" hypothesis stays falsified (those
`+0x74` reads are the actor's **colour word**, which `FUN_800480D8` stamps with
the 24-bit mid-grey `0x00808080` under the mask `0x00FFFFFF`, not a stat grant -
see [`functions/renderer.md`](../reference/functions/renderer.md#800480d8)).
`legaia_asset::level_up_tables::growth_tables_from_scus` parses the curves +
param block, and the engine applies them: `LevelUpTracker::with_growth_tables`
installs per-character `StatGrowthCurve::PerLevel` (all 8 stats) at boot,
byte-validated against the captured Noa L2->L3 single-level deltas
(see [`level-up.md`](level-up.md#stat-gains)).

Engines populate one captured observation at a time via:

```rust
let obs = legaia_engine_core::levelup::LevelUpObservation::vahn_4_level_jump();
let tracker = LevelUpTracker::new().with_observed_curve(0, &obs);
```

`LevelUpObservation::to_curve` produces a `StatGrowthCurve::PerLevel` vector that emits the per-level *average* inside the observed range and falls back to `StatGain::default` outside it. Implementation: [`crates/engine-battle::levelup`](../../crates/engine-battle/src/levelup.rs).

## CDNAME → MV STR cutscene routing

`engine_core::scene::cutscene_str_for(scene_label) -> Option<&'static str>` resolves an `op*` / `edteien` CDNAME label to its paired `MOV/MVn.STR` filename. The disc carries 6 STR files (`MV1.STR..MV6.STR`); the heuristic mapping is:

| CDNAME | STR file | Scene context |
|---|---|---|
| `opdeene` | `MOV/MV1.STR` | Drake Castle opening |
| `opstati` | `MOV/MV2.STR` | Statue scene |
| `opkorout` | `MOV/MV3.STR` | Korout opening |
| `opurud` | `MOV/MV4.STR` | Urud opening |
| `opmap01` | `MOV/MV5.STR` | World map opening |
| `edteien` | `MOV/MV6.STR` | Garden ending FMV |

`cutscene_label_for_str(filename)` is the inverse (case-insensitive on the basename so `mv1.str` and `MOV/MV1.STR` both round-trip). The remaining `ed*` scenes (`edbylon`, `edbalden`, `edlast`, `edretoin`, `edkorout`, `edbubu`, `eddoman`, `edson`, `edstati3`) are dialogue-actor-overlay driven and have no FMV. The exact retail mapping table lives in the cutscene overlay (not yet captured) - when it lands, the lookup function should be updated to consult the captured map. The `legaia-engine play` and `play-window` subcommands auto-resolve the STR file when the user passes `--scene <op*|edteien>` and the extracted root contains the matching MV file.

## Equipment catalog

Vanilla equipment table covering the early-game roster. Each entry is an `EquipmentEntry` carrying id + name + slot + character restriction + `ItemModifier` + buy/sell prices. `to_modifier_table()` resolves to the `EquipmentTable` the battle stat aggregator (`compute_battle_stats`) reads.

Slots match the retail `equip[8]` byte array at character record `+0x196`:

| Slot | Index | Examples |
|---|---|---|
| Weapon | 0 | Vahn-only swords, Noa-only knuckles, Gala-only quarterstaves |
| Helmet | 1 | Cloth Cap → Mythril Helm |
| Body Armor | 2 | Cloth Robe → Plate Mail |
| Hand Guard | 3 | Cloth Wrap → Iron Gauntlets |
| Boots | 4 | Cloth Shoes → Wind Boots (ability bit 12) |
| Ring 1/2 | 5/6 | Power / Defense / Speed / Hit Rings |
| Accessory | 7 | Goblin Foot (encounter rate down) / Wisdom Ring (MP cost) / Lucky Charm (bonus EXP) |

Implementation: [`crates/engine-core::equipment`](../../crates/engine-menus/src/equipment.rs).

## Seru capture + spell learning

Per-character per-Seru capture-point accumulator. Each captured Seru contributes points toward a per-character spell-learn threshold (default 100); once crossed, the spell is added to the character's learned list.

`SeruDef::learnable_mask` is a 3-bit per-character mask (bit 0 = Vahn, bit 1 = Noa, bit 2 = Gala) so single-character Seru can teach only their bearer. `record_capture` is the pure resolver; `SeruCaptureSession` drives the post-capture banner sequence (`Capturing → Announcing[i] → Done`) for engines to render.

Implementation: [`crates/engine-battle::seru_learning`](../../crates/engine-battle/src/seru_learning.rs).

### The retail capture roll (`FUN_801ec3e4`)

Retail decides a capture inside the arms execution resolver `FUN_801EC3E4`
(overlay 0898, base `0x801CE818`; dump `overlay_0898_801ec3e4.txt`, block
`0x801ee1c0..0x801ee2e8`), at the moment a physical hit resolves:

1. **Killing blow only.** The block is entered from the damage-vs-HP compare
   at `0x801ee1cc` (`sltu` of damage against the target's current HP at
   `+0x14C`): a hit that leaves the monster alive branches past the whole
   capture path. The attacker must also be a party slot (`< 3`).
2. **Capturable gate.** The target's record (per-enemy record-pointer table
   `0x801C9348[slot-3]`) is read record-direct: `+0x3E` (Seru id) zero → no
   roll.
3. **The roll** (`0x801ee268..0x801ee2a8`): base chance = record `+0x3F`
   (percent). If the attacker's character record carries ability-word `+0xF8`
   bit `0x4000` - passive index `0x2E`, **Magic Boost** (Ivory Book) - a flat
   `+30` percentage points is added first (`0x801ee238`). Then
   `rand() % 100 < chance` (rand at `jal 0x80056798`, the `%100` folded
   through the `0x51EB851F` reciprocal multiply).
4. **Success**: `FUN_801E91E8` (`jal` at `0x801EE2C0`) asks whether the
   acting character already knows the Seru - it scans the learned-spell list
   at `0x80084140 + char*0x414 + 0x704`, whose ids are full `0x8x` spell
   ids - and answers "known" outright for a slot without its Ra-Seru
   (`ctx[+0x25F + slot]`) or in a no-reward battle (`_DAT_8007BAC0`). Only an
   unknown Seru is stored into the battle context at `+0x269` (`sb
   v0,0x269(a0)` at `0x801ee2e8` - the byte the shiny-Seru patch hooks). The
   roll's `rand()` is drawn on every Seru kill, before that check.
5. **The grant is in the same action**, not after the battle: the action
   SM's Done band reads `ctx[+0x269]` (`0x801E6224`), calls `FUN_801E92DC`
   with it (`0x801E6234`) - which prepends spell `seru_id + 0x80` to the
   character's list - raises the learn banner `0x59`, and holds its `0x52`
   arm `0xB4` frames so the banner can be read. An earlier revision of this
   section said success "routes the action SM into the capture cinematic
   (states `0x68..0x6B`)"; those states belong to the capture *spells*, and
   the Done band's own disassembly is where this byte goes.

**Which hits reach it.** The kill compare runs on one hit per landing, not on
every hit. The resolver's per-hit gates (`0x801EE128..0x801EE1A4`) are its apply
gate: the apply mode `s2` (`0x801EE060..0x801EE128`) of `0xFF` skips the check,
a non-zero mode on a monster target takes it at once, and otherwise it needs
the parked strike cursor (`ctx[+0x15] == 0xFF`, `0x801EE15C`) on the clip's
last beat (`entry[0x11 + idx] == 0` or `idx == 3`,
`0x801EE180..0x801EE19C`) - the same pair that lands the combo total at
`0x801EE984`. The compare is then the accumulated total `+0x0` against live HP
(`sltu v0,a0,a2` at `0x801EE1CC`). So a combo that crosses the target's HP on
its second hit rolls once, on the hit that lands the total, after every
damage draw of the chain.

The engine runs this path: `World::roll_seru_absorb`
(`world/battle/seru_absorb.rs`) sits on the melee hit fold's kill check and
reads the record's `+0x3E` / `+0x3F` off the monster catalog, and the Done
band hands the staged byte to `World::learn_absorbed_seru`. The hit fold's
callers pass the apply gate as the kill check, the way retail shares it. The engine's capture-spell
path (`World::resolve_capture`, a missing-HP-fraction roll feeding the Seru
registry) is a separate mechanism for the capture spells. The catch-rate byte
is the `--seru-catch-rate` randomizer target
([randomizer.md](../tooling/randomizer.md#seru-catch-rate)).

## Arts command input

The Arts command opens a **per-press directional entry**, not a list. Each
d-pad press appends its command to the acting actor's `+0x1DF` queue and
debits that command's `+0x74` AP cost from the turn pool; the entry ends by
itself the moment nothing is affordable, and the entered sequence is then
matched against the character's learned arts. Retail's flow, the AP
arithmetic and the port's divergences are on
[`arts-command-gauge.md`](arts-command-gauge.md#the-ports-input-session);
the screen's packet-pinned presentation is on
[`minigame-muscle-dome.md`](minigame-muscle-dome.md#arts-command-input-packet-pinned),
which is where it was captured (the dome runs the same screen verbatim).

Port: session `engine_core::arts_command_input`, opened from the command
menu's Arts arm and driven by the live loop while the action SM is parked.
Chrome: `legaia_engine_ui::arts_input`, drawn by both hosts off the shared baked
system-UI atlas. `World::arts_input_active()` / `arts_input_actor()` tell a
host's party surface that an actor owns the pad - retail parks the status
plate off-screen for the whole session. The older saved-chain list stays
reachable behind `LEGAIA_ARTS_SAVED_LIST=1`.

## Tactical Arts chain editor

Menu-side state machine for composing + saving Tactical Arts command chains. `ChainLibrary` holds up to 8 saved chains per character (3..=7-byte length range, matching retail). `ChainEditor` runs a 4-phase SM: `Browsing { cursor } → Editing { working } → Naming { working, name } → Done`. Engines feed picks into the battle command queue at battle start.

Implementation: [`crates/engine-battle::tactical_arts_editor`](../../crates/engine-battle/src/tactical_arts_editor.rs).

## Battle rewards composite

`World::apply_battle_loot(formation, catalog) -> BattleRewards` is the post-victory composite that turns a defeated formation into the runtime side-effects:

- Sums each `MonsterDef::exp` and distributes the total via `World::apply_battle_xp`, which splits the pool equally among the surviving party members (integer divide, remainder dropped; dead members get zero) and runs per-character level-up checks against `LevelUpTracker::xp_table`.
- Sums each `MonsterDef::gold` and adds it to `World::party.money` (saturating).
- Rolls the one drop retail offers through `battle_formulas::victory_drop_roll` - one `rand() % 100` per enemy seat against its percent chance, the last winning seat's item, then a 1-in-4 gate (see [battle-formulas.md](battle-formulas.md#victory-spoils-rewards)). The item is appended to `BattleRewards::drops` and added to `World::party.inventory` unless 99 are already held.
- Returns `BattleRewards { xp, gold, level_ups, drops }` for the engine to surface as the post-battle banner ("got N XP, M gold, level up, found Healing Leaf!").

Monster ids missing from the catalog contribute zero (silently skipped) so a partially-populated catalog still drives a battle-end transition. Implementation: [`crates/engine-core::world::World::apply_battle_loot`](../../crates/engine-core/src/world.rs).

## Live gameplay loop - Field ↔ Battle in `tick`

`World::tick` drives the full Field → Battle → Field round trip itself when `World::toggles.live_gameplay_loop` is set. The flag is an opt-in: with it clear (the default), the `Field` branch runs the field VM + locomotion but never rolls encounters, and the `Battle` branch runs a single `step_battle` without applying damage or re-arming - preserving every existing caller and test that drives those externally.

With the flag set, the per-frame flow is:

- **Field tick** (`World::live_field_tick`): a *step* is the player actor
crossing into a new 128-unit collision tile (`pos >> 7`). Each step drives one
`World::on_field_step` encounter roll; `World::tick_encounter` advances the
session's `Transition` / `Grace` countdowns every frame. When the
`EncounterSession` reaches `Triggered`, `World::begin_encounter_battle` resolves
the rolled `formation_id` against `World::tables.formation_table`, snapshots the field
actor table into `World::field_return`, seeds the battle actor table from the
formation + `MonsterCatalog` (`enter_battle_from_formation`), and flips `mode`
to `Battle`. If a battle track is configured (`World::audio.battle_bgm`, set via
`World::set_battle_bgm`), `enter_battle_from_formation` also calls
`World::swap_to_battle_bgm`: it stashes the current field track and queues a
`FieldEvent::Bgm{sub_op: 1}` for the battle id, which the host's BGM director
cross-fades to exactly like a field op-`0x35` start.
- **Battle tick** (`World::live_battle_tick`): wraps `step_battle` with the host-side glue the retail engine performs through its render + animation systems, so the battle resolves from `tick` alone. It folds this frame's `BattleEvent::ApplyArtStrike` damage into target HP; applies a generic physical strike (`apply_basic_attack`, through the retail melee roll pair `battle_formulas::physical_predamage` - see [battle-formulas](battle-formulas.md#the-melee-roll-pair-and-the-underdog-rewrite)) on the `AttackChain → AttackRecovery` edge when no art strike did; marks zero-HP combatants dead so the SM's wipe scan resolves; clears `ADVANCE_DONE` at `AttackRecovery`; and re-arms the next party attacker at `EndOfAction`. On `StepOutcome::BattleComplete` it calls `World::finish_battle`.
- **Return** (`World::finish_battle`): on `BattleEndCause::MonsterWipe` it credits loot via `World::apply_battle_loot` (recorded in `World::battle.last_rewards`); on `PartyWipe` it raises `World::game_over`. Either way it ends the encounter session's battle (post-battle grace + suppression), restores the `field_return` actor snapshot, and flips `mode` back to `Field`. When a battle-BGM swap was active it also calls `World::restore_field_bgm`, which queues a `FieldEvent::Bgm{sub_op: 1}` for the stashed field track (or a stop, sub-op 4, if no field track was playing at encounter start) so the director cross-fades back.
- **Post-battle script re-entry** (`SceneHost::tick`): retail reloads the field scene after every battle, re-running the scene-entry system script `P1[0]` (`FUN_8003ab2c`).
The host mirrors that on the `Battle -> Field` mode edge by reloading the entry script (`Scene::field_man_entry_script` -> `World::load_field_script_at`).
This re-run is what dispatches post-battle beat records: rikuroa's `P1[0]` tests the transient staged marker `0x289` (SET by the stager `P1[3]`'s own `52 89` script bytes when the approach dispatch ran the record pre-battle)
and issues the op-`0x44` spawn of the post-victory record `P2[50]` through the C1-gated dispatch - whose own script bytes SET the progression gate `0x142`.
No engine code writes the gate flag or the marker (there is no victory latch and no battle-entry stamp); both land from record execution. Disc-gated oracle: `engine-core/tests/organic_beat_records_disc.rs`.

### Auto-resolve vs player-driven

The battle tick has two modes.

- By **default** it auto-resolves: every turn commits a generic physical strike against the first living combatant on the opposing side, with no player choice. The whole actor table takes turns, so **monsters take turns too** - a monster turn strikes a living party member, and a party wipe ends the battle (`game_over`) the same way a monster wipe does. The strike side is chosen by the attacker's slot (`World::first_living_opponent_of`).
- When `World::battle.player_driven` is set (requires the live loop), each *party* turn instead pauses the action SM and opens a `battle_input::BattleCommandSession` (monster turns still auto-resolve) - the player picks a command from the battle command menu and a target before the strike commits. While a session is open `live_battle_tick` skips the SM advance and drives the picker from `World::input`; on confirm `World::tick_battle_command` arms `battle_ctx.{active_actor, queued_action, action_state}` plus the acting actor's `active_target` and resumes the SM. An abort (no valid target) falls back to a default strike so the loop can't deadlock. Target selection reuses the [battle target picker](battle-command-flow.md#battle-target-picker).

**The round has two bands, and nothing acts in the first.** Retail's two state
machines hand a round back and forth (see [the round loop](battle-command-flow.md#the-round-loop---what-re-arms-0x1e)):
the flow SM's **command band** (`0x14 -> 0x1E -> 0x28 ...`) walks every living
party member through a ring while the action SM idles, and only `0x6E`'s
begin arm stores `0xFE` (`0x801D31AC`), the one state that hands the round to
the action SM (`ctx[+0x07] = 0` at `0x801D3224`). From there `FUN_801E295C`
dispatches **every** combatant, party and monster alike, by the max-key pick
`FUN_801DABA4`, and consumes each key at its own `0x0C` dispatch
(`sh zero,0x16c(s3)` at `0x801E2CDC`). So a monster that won initiative
still waits for the last party commit; what initiative decides is the order
inside the **execution band**, never whether anyone acts before the prompt.
The only way a round skips its command band is a rolled **back attack**:
`0x0B`'s `ctx[+0x290] == 1` arm stores `0xFE` outright (`0x801D0E78`), so the
party enters no command and, with its keys zeroed by the side lockout, only
the monsters dispatch.

The engine runs the same two bands (`battle_round::RoundFlow`,
`RoundPhase::{Command, Execute}`; `World::begin_battle_round` /
`begin_round_execution` / `end_battle_round` in `world/battle/loop_driver/round.rs`):

- **Command band.** `begin_battle_round` is retail's `0x14`: the actor sweep
  (`BattleRound::boundary`), the initiative re-seed when no key is live, the
  per-round DoT ticker (round index `!= 0`), then `Begin | Run` for the first
  member that owes a command (`World::next_member_owing_command`, the port of
  `FUN_801DB81C` / `FUN_801DBA04`: skips a committed member, one with no HP, and
  one whose status word carries `+0x16E & 0xF84`). Each commit
  (`World::commit_party_command`, retail's ten-site idiom at `0x801D16AC`)
  parks the typed command in `RoundFlow::pending` and walks the ring on to the
  next member, or begins the round. `Run` is the exception retail makes at
  `0x32`: it stamps category `5` on every party actor and begins the round at
  once.
- **Execution band.** `begin_round_execution` is `0x6E -> 0xFE`. Every idle of
  the action SM at `EndOfAction` is one pick by
  `World::next_combatant_by_initiative` (`FUN_801DABA4`): the living actor with
  the highest unspent key acts - a monster through its AI pick, a party member
  through `World::dispatch_pending_party_action`, which is where the swing
  stream is seeded (`FUN_801EED1C` from state `0x0C`), the art profile staged,
  the spell cast, the item effect landed, the Spirit AP charged and the escape
  rolled. The key is consumed by the pick, the engine's counterpart of the
  `0x0C` consumption. When no living actor holds a key the round ends
  (`end_battle_round`: the `ctx[+0x28A]` bump + the `0x400` waker, retail's
  `0xFF` arm at `0x801E67E8`) and the next `begin_battle_round` opens.
- The initiative **key** (`BattleActor::init_key`, retail `+0x16C`) is seeded by
  `FUN_801DA780` from SPD (`+0x164`): `speed + rand()%(speed/2 + 1) + 1`, plus
  the wounded bonus - party `(max-hp) >> 4` below a quarter, `>> 5` below half,
  `>> 6` above; monsters `>> 10` - then halved under Slow
  (`battle_formulas::seed_initiative`; see [battle-formulas](battle-formulas.md)).
  Battle entry seeds the keys **ahead of** the formation latch, because the
  seeder is the one reader of the unlatched `ctx+0x290` and the side lockout
  would otherwise be lost; round 1 therefore finds live keys and does not
  re-roll. Dead actors' keys are zeroed on every pick (the function's first
  loop) so they can't be picked.
- Party SPD is the **resolved** stat - base plus the equipment table's footwear bonus - written by `World::seed_party_battle_stats` at battle entry, over the raw record value `World::load_party` seeds at boot. Monster SPD comes from `MonsterDef::speed` (record `stats[5]`, unboosted) at battle setup.
- When **no** living actor carries SPD - the disc-free / synthetic case where
  speed data hasn't been loaded - there is nothing to roll: every living slot
  gets one flat turn token and the pick walks them in slot order after the
  last acting actor, which keeps the synthetic loop deterministic while still
  giving it retail's round boundary.

All six commands - **Attack**, **Arts**, **Magic**, **Item**, **Spirit**, **Run** - are wired into the live loop. Attack opens a target cursor and commits a physical strike through the action SM. Arts / Magic / Item resolve to `Resolution::OpenArtsMenu` / `OpenSpellMenu` / `OpenItemMenu` - the command session can't run those pickers itself (they need the caster's saved chains / learned spells / live MP / inventory + party stats), so it hands off to a host-owned submenu. Spirit and Run resolve immediately (no target):

- **Spirit** raises the guard stance at the **commit** (`World::battle.guarding`, the engine model of the retail pending-action byte `+0x1DE == 4`, which the melee kernel's guard roll reads) - so it protects against every monster that dispatches ahead of the member - and lasts until the next round's sweep clears the category. The AP charge (`ApGauge::charge_spirit`, the retail Square-press +5) is the Spirit band's own, at the member's dispatch.
- **Run** stamps category `5` on every party actor at the commit and begins the round at once (retail `0x32`, `0x801D1174..0x801D1184`); each member's dispatch then rolls the escape and arms the ported run band (`RunBegin`/`RunWait`/`RunEscape`): success tears the battle down `Escaped` (no loot, no game over, downed members floored alive at 1 HP), failure consumes the turn. The roll is the decoded `FUN_801E791C` formula - party `(SPD*3)>>1 + missingHP>>4` vs enemy `SPD + missingHP>>5`, two rand draws, Chicken Heart / Chicken King passives honoured (`battle_formulas::escape_roll`; see [battle-action.md](battle-action-queue.md#spirit--run-in-the-live-command-menu)).
  A scripted no-escape fight (`ctx[+0x287]`) is no exception at the prompt: `0x1E` and `0x32` never read the byte, so Run commits there as anywhere, and the roll - which tests `ctx[+0x287]` after its compare (`0x801E7B14`) - fails it, so the run band plays its failure arm and the turn is spent.

The submenu hand-offs:

- **Item** opens a battle-context `inventory_use::InventoryUseSession` on
`World::battle.item_menu` (built by `World::build_battle_item_session` from the
live inventory, with one ally row per party slot plus one enemy row per live
monster slot, the enemy rows tagged `TargetRow::is_enemy` - the roster carries
both sides for the engine's synthetic offensive items). The **side rule is
structural**, as in retail: state `0x64`'s cursor walk wraps strictly inside
the seated party band `[0, ctx[+0x00])` (`0x801D2BE8`/`0x801D2C78`) and the
enemy-side classes go to the monster-ring states `0x5B`/`0x5D` instead, so the
target panel lists **only the selected item's side**
(`inventory_use::target_on_effect_side`; the cursor steps within it and the
projection filters the rows both hosts draw). On entering target-select the
cursor auto-positions on the first benefiting target. On a completed use the
item applies via
`World::use_item`, one copy is removed (`World::consume_item`), and a popup is
surfaced - heal-coloured for heals/revives, damage-coloured for offensive items.
`World::use_item` folds the offensive outcomes too: `DamageDealt` subtracts
enemy HP and downs it at zero, `CaptureRolled` reuses `World::resolve_capture`
(down + log id into `battle_captures`), and `EscapeRequested` sets
`World::battle.escaped` so the item tick returns to the field via
`finish_battle` (no loot).
- **Magic** opens a `battle_magic::BattleSpellSession` on `World::battle.spell_menu` (built by `World::build_battle_spell_session` from the caster's learned spells off their roster record + live MP, MP-gated). The picker kind matches the spell's `SpellTarget` shape. On confirm the session commits a `PendingPartyAction::Spell` through `World::commit_party_command`, and the cast runs as the caster's action-SM dispatch: `World::cast_spell_on_slots` deducts MP once, resolves each affected slot through `spells::cast_spell` (caster magic from `World::battle.magic`, target magic-defense reusing `World::battle.defense`), and folds the outcome into the live actor table via `World::fold_spell_outcome`. All `SpellOutcome` shapes apply:
    - damage / heal / cure / revive;
    - **buffs** (`World::apply_battle_buff` writes the delta straight into the per-slot `battle.attack` / `battle.defense` / `battle.magic` scalar with refresh semantics + a per-turn timer aged in the re-arm path, reverted exactly on expiry);
    - **capture** (`World::resolve_capture` rolls vs the monster's missing-HP fraction - reliable only on a weakened Seru - downing it and logging the id into `World::seru.battle_captures` on success);
    - and **escape** (sets `World::battle.escaped`, and the spell tick returns to the field via `finish_battle` with no loot).
    - Accuracy / Evasion / Speed buffs are tracked but have no live-loop scalar to move yet.
- **Arts** opens the per-press [Arts command input](#arts-command-input) on
`World::battle.arts_input` - the player *types* the chain, one d-pad press per
command, and the entry ends itself when the AP pool can no longer afford a
press. `World::build_arts_action_queue` then builds retail's action queue
from the entered buffer (`legaia_art::tokenize` + the learn-on-use verdict +
the Miracle / MSB-clear / Super finish) and `arm_battle_art_action` hands it to
the action SM's attack band verbatim: each swing, starter and art constant is
its own staged clip, and the clip's hit events resolve the damage
(`World::tick_battle_hit_events`; see
[battle-action.md](battle-action-queue.md#what-the-port-does)). Art records come from
`World::tables.art_records`, keyed by `(Character, ActionConstant)` and installed at
battle entry from the character's art-animation bank
(`World::install_art_bank_records`, both hosts) - the same records retail's
queue-builder walks; the hit-event driver reads them for the status effect
and per-hit cue only - the power bytes are the clip entry's own.
  Because an entry runs until the pool is spent, performing **several** arts in
one turn is the ordinary case, and the performed-art list is what the shout cue
and the learn-on-use check are keyed on - once per art, not once per turn (see
[audio.md](audio.md#battle-arts-voice-shout-path-engine)). A Miracle / Super
replacement answers a single constant, its finisher.
  The legacy saved-chain list (`battle_arts::BattleArtsSession` on
`World::battle.arts_menu`, built by `World::build_battle_arts_rows` from
`World::party.saved_chains`) stays reachable behind `LEGAIA_ARTS_SAVED_LIST=1`. A row
there collapses to the one art whose command string the chain ends with
(`chain_matches_record`), or to a synthetic per-direction profile
(`battle_arts::synthetic_power` - Down → LDF, else UDF, tier-0 ×12, clamped to
`MAX_ART_HITS`) when no record matches. Both paths share the one
`apply_art_strike` kernel.

While any submenu is open both the SM and the command session are parked;
`World::tick_battle_arts_input` / `tick_battle_{arts,spell,item}_menu` drives it
from `World::input`. On a completed action the result is applied, the relevant
popup is surfaced (`World::drain_battle_hit_fx`), and the action SM is **parked at
`EndOfAction`** so the re-arm block cycles to the next combatant - a cast / art
/ item use is the actor's whole turn, no Attack-SM strike fires. Backing out
reopens the command menu for the same actor. Implementation:
[`crates/engine-core::battle_input`](../../crates/engine-menus/src/battle_input.rs)
+ [`arts_command_input`](../../crates/engine-battle/src/arts_command_input.rs) /
[`battle_arts`](../../crates/engine-battle/src/battle_arts.rs) /
[`battle_magic`](../../crates/engine-battle/src/battle_magic.rs).

Coverage: `crates/engine-core/tests/battle_player_driven.rs` walks into a
battle, asserts no strike lands until the player confirms a command, then
drives the picker to a monster wipe + loot.
`battle_command_arms_reachable.rs` is the hand-off guard - each of Arts / Magic
/ Item must open exactly its own surface, consume the command session, arm
nothing, and (for Arts) actually consume a directional press. It exists because
re-pointing an arm is invisible to `--lib`: the surface's own unit tests keep
passing while every integration driver that walked the old arm stops reaching
an executed action.

### Post-battle Seru learning

Capturing a monster (magic capture roll or a capture item) downs it and logs its **monster id** into `World::seru.battle_captures`.

- `World::finish_battle` resolves these through `World::resolve_captures`: each captured monster id maps to a **Seru id** via `MonsterCatalog`'s `MonsterDef::seru_id`, and `seru_learning::record_capture` banks that Seru's capture points against `World::seru.log` for every active party slot eligible by the Seru's `learnable_mask`.
- When a slot's accumulated points cross the Seru's `learn_threshold` the taught spell id joins that character's learned list, and `World::build_battle_spell_session` unions the roster's saved spells with `World::seru.log.learned_spells(slot)` so a freshly-learned spell is immediately castable - no save/load round-trip needed.
- The accepted `CaptureOutcome`s are stashed in `World::seru.last_capture_outcomes` (`drain_last_capture_outcomes`); `resolve_captures` also builds the first accepted capture into `World::party.current_capture_banner` (a `seru_learning::SeruCaptureSession`), the sibling of `World::party.current_level_up_banner`.
- `World::tick` advances the banner one frame per call and clears it when the session reaches `Done`, so it plays out over the field after the battle ends. The session's `current_banner()` yields the active line (`"Captured: <Seru>!"` then per-learn `"<char> learned <spell>!"`); the play-window renders it via `legaia_engine_render::capture_banner_draws_for`.
- `resolve_captures` always drains `battle_captures`; with an empty `World::seru.registry` (the default) it banks nothing - the monster is still downed, but no Seru is learned.
- Capture-point progress (including sub-threshold totals) persists through `World::save_full` / `load_full` as `(seru_id, points)` pairs in each `CharSaveExt::seru_captures`; reload restores the points and, with the registry installed, re-marks any over-threshold Seru as learned.
- This registry path serves the capture *spells* only. Its `MonsterDef::seru_id` mapping + `learn_threshold` / `capture_points` values are engine-side approximations (the live loop installs `SeruRegistry::retail`, which pins only the taught spell ids); the killing-blow Seru absorb does not use them - it reads the record's own `+0x3E` / `+0x3F` ([above](#the-retail-capture-roll-fun_801ec3e4)). Pinning the capture spells' per-monster attachments is gated on the still-uncaptured stat-grant table loader (see [`crate::capture_observations::battle_init_overlay`]).

### What the loop flag does and does not gate

`World::toggles.live_gameplay_loop` gates the **field side only** - the step-driven random-encounter roll. Once the world is in `SceneMode::Battle`, `World::tick` always drives the full `World::live_battle_tick`, regardless of the flag, because a battle that cannot resolve is a soft-lock. Retail has no "loop enabled" concept either: `FUN_801E295C` drives the battle it is in.

That asymmetry is not cosmetic. Battle **entry** was never gated - a field carrier's scripted `3E FF` fight and a world-map region encounter both flip the mode on their own - so gating battle **driving** left the ungated entry paths able to strand a session in `SceneMode::Battle` with no damage applied, no turn armed and no `finish_battle`. Regression: `crates/engine-core/tests/battle_always_resolves.rs`.

### Host-simulated animation edges

Two action-SM gates are driven in retail by the render / animation systems and by nothing in the port, so `World::live_battle_tick` retires each on the frame its state is reached:

- `ADVANCE_DONE` at `AttackRecovery` - retail clears it when the recovery animation finishes.
- The caster's `spell_iter` (`actor+0x1FA`) at `MagicSustain` (`0x2B`). The SM only ever *sets* this byte; retail's cast-animation system counts it down. Without the edge, `magic_sustain`'s `stay` held forever, so **any battle in which a monster or party member cast a spell stopped dead** - which is most real encounters, and is a large part of what "battles don't work" looked like from the outside. Regression: `a_monster_cast_does_not_park_the_action_sm` in `crates/engine-core/tests/battle_always_resolves.rs`; the real-data version is `crates/engine-shell/tests/scene_encounter_rollable.rs`, which drives a `map03` encounter from the disc's own region table through to a resolved battle.

#### The strike-pacing gate must always be able to retire

`attack_chain` (retail `0x1E`) stages one strike-script byte per clip: it writes
`queued_anim`, sets `ADVANCE_DONE`, and holds until the animation system retires
the flag. The engine's anim commit `World::commit_staged_battle_anim` does retire
it for a clip-less swing - but only in the branch it reaches *past* its
`queued_anim == current_anim` early-out. A staged byte equal to the actor's
current anim id therefore never reached the clear, and the SM parked at `0x1E`
for the rest of the session.

Two things had to line up, and an ordinary disc encounter lines them up on its
own. The monster-AI picker writes the chosen spell id into the actor's
action-parameter stream (`params[0]`, retail `+0x1DF`) *before*
`take_monster_turn` discovers the cast cannot fold; the fallback physical strike
then walked that spell id as a swing byte, and the swing committed it into
`current_anim`. The monster's **next** physical turn re-staged the same stale
byte into the converged pair and hung. Both halves are closed:
`World::clear_action_stream` zeroes the stream when a physical action is armed
(the per-action sibling of `FUN_801D88CC`'s round-boundary clear), and
`live_battle_tick` retires `ADVANCE_DONE` whenever the id pair has converged with
no clip in flight. Regressions:
`crates/engine-core/tests/battle_attack_chain_stall.rs`, plus the real-data
`a_starting_party_can_fell_a_real_early_enemy` in
`crates/engine-core/tests/battle_physical_damage.rs` (the Green Slime row of
which parked before the fix).

Both hosts arm the loop through one shared kernel, `World::arm_live_loop` (`crates/engine-core/src/live_loop.rs`): scene label, the synthetic encounter fallback for scenes whose MAN carries no table, the loop / player-battle flags, the Seru registry and the battle-BGM swap. The native `BootSession::enter_field_live` and the browser's `LegaiaRuntime::arm_live_battles` are callers of it, not copies of it.

### Host flags

The `legaia-engine play-window` host ships the loop **on**, matching the browser play page and the project's enhancement-forward default; retail-shaped inspection is one flag away:

- `--no-live-loop` turns the encounter roll off (field VM + locomotion only - the scene-inspection mode). A battle the engine is already in still resolves.
- `--no-player-battle` turns off the command menu, auto-attacking each party turn instead. By default battles are player-driven and the HUD renders party/monster HP plus the command menu / target cursor / arts + spell + item submenus (the host installs the boot spell catalog (the disc table) and the vanilla item catalog; with `LEGAIA_DEMO_BATTLE_SEED=1` it also seeds demo items - Healing Leaf + Bomb - saved chains and a demo `Art1B` record into an empty save, so the ally-heal and offensive item paths are exercisable without a real save. Without the variable an empty save stays empty, as on retail).
- `--battle-bgm <id>` overrides the Battle↔Field music swap track: the live loop cross-fades to it on encounter and resumes the field track on battle end. The swap is on by default (retail's standard battle theme, global BGM `2026` = `music_labels::BATTLE_THEME_1_BGM_ID`, installed by `LiveLoopOpts::playable()`); `0` disables it. Ids route through the same director as field op-`0x35` starts - scene-local ids via the scene's BGM table, `>= 2000` via the global `music_01` pool. The browser twin is `LegaiaRuntime::set_battle_bgm`.

### Battle end, both hosts

`World::finish_battle` is what a resolved battle runs, and three of its results are now read:

- **Party HP / MP persists.** The battle mutates the `BattleActor` mirrors; `finish_battle` writes them into the roster records (via `World::save_party`) *before* restoring the field actor snapshot, then pushes them back onto the restored party actors (`World::resync_party_actors_from_roster`). Without that step every fight ended at the HP it started with, and losing was indistinguishable from winning.
- **A wipe raises `World::game_over`**, which both hosts read and route to the **title screen** - retail's destination, pinned to the `game_mode = 0x16` / `_DAT_8007BB00 = 1` store pair (see [§ party wipe](#party-wipe--the-game-over-overlay)). Native pushes `BootUiState::GameOver`, the browser arms the same `GameOverSession`; neither draws anything and neither reads a button, because retail asks the player nothing here.
- **A victory raises the result screen in battle** (`World::battle_spoils_banner`, up from the results frame of the sequence below through the exit) - retail's two framed windows, described by `engine-ui::battle_spoils_windows` and filled by `battle_spoils_draws_for` on both hosts. Rects and columns are measured off a retail framebuffer; see [level-up](level-up.md#what-the-port-draws-between-the-last-enemy-dying-and-the-field-returning). A `finish_battle` that applies the loot itself (no victory sequence ran) still arms the aging `World::SPOILS_BANNER_FRAMES` window instead.
- **A wipe raises the loss window** (`World::battle_defeat_banner`, same span) - the win window's twin, drawn on the report frame by `engine-ui::battle_defeat_windows` on both hosts; see [below](#the-loss-window-is-the-result-windows-twin). The spoils panel answers only a win: `last_rewards` outlives its battle, and a wipe after a win used to re-show that win's spoils.
- **The exit's party loop runs on every exit** (`battle_formulas::battle_exit_party_reset`): statuses clear unless the special-battle word carries the arena bit, and a member at 0 HP stands up at 1 - see [battle-formulas.md](battle-formulas.md#the-flow-readers).

### The loss window is the result window's twin

The results frame opens one framed window per outcome through the battle HUD's element spawner `FUN_801D8DE8`: element `0x41` on a win (`0x8004F65C`), `0x42` on a wipe (`0x8004F900`), both skipped while the special-battle word is set. An element id is an index into the SCUS **screen-element placement table** (`0x80076C10 + id * 0x18`, `legaia_asset::screen_elements`), not into the pause menu's window descriptor table. Neither id has a labelled arm in the spawner's jump table (`0x801CEB68`, indexed by `id - 0xA`); both take the default post-switch tail (`0x801D91D4..0x801D93DC`), which registers the record's box through `FUN_8003541C` and slides it with `FUN_801DB7B0`.

Records `0x41` and `0x42` are byte-identical on the disc: widget pair `(3, 3)` (the corner-framed window), content box `288 x 42`, node kind `0x0D` (a kind the layout dispatcher `FUN_80030628` fills with nothing), sliding from `(16, 236)` to `(16, 160)`. Outset by the frame's six pixels that box is the band the report window was measured at off a retail framebuffer, so the two sources agree. What differs is the string word `FUN_801D84C0` publishes into each at battle start (`sw` at `0x801D8500` / `0x801D84F0`):

| Element | Buffer | Solo party | Party of two or more |
|---|---|---|---|
| `0x41` | `ctx+0xA9` | lead's name + the victory tail (`0x801F4C38`) | team string (`0x801F4C2C`) + the victory tail |
| `0x42` | `ctx+0x129` | lead's name + the defeat suffix (`0x801F4C94`) | the defeat team string (`0x801F4C78`) |

A team string opens with the text engine's name escape `0xC1`, whose operand `FUN_801D84C0` patches to the lead's index. On a win the results frame re-patches the victory buffer's operand (`ctx+0xAA`, `0x8004F658`) to the pose actor's index when the party has two or more members; the pose actor is the lead, so the store names the same character. The loss arm stores nothing there.

The port reads the two defeat pieces off PROT 0898 (`battle_party_panel::DefeatText`, installed with the move-power table) and composes the line in `World::battle_defeat_banner`; without the disc pool the window opens empty. The win window's sentence is still built from typed state.

### Battle end, retail's way - the results sequencer

`finish_battle` no longer runs on the frame the `0x5A` gate raises the signal. Retail's battle tick `FUN_80046A20` stops stepping the action SM once `DAT_8007BD71 == 0xFE` (`0x80047040`) and runs the results sequencer `FUN_8004E568` every frame instead (`0x800470D0..0x800470E8`), and the battle exits only when the sequencer's phase halfword `ctx[+0x6CE]` reaches `0x43` (`0x80046DAC`). The port's mirror is `World::battle.victory` (`world::battle::victory`), walked by `World::tick_battle_end_sequence` in place of the SM while the scene stays in `SceneMode::Battle`.

`_DAT_8007BD2C` is both the wipe cause and the sequencer's phase word: a victory (`0`) walks the jump table at `0x800152FC` as `0 -> 2 -> 4 -> 5` while the hero's `monster.snd` voice clip (slot 7) and PROT 0889 (the level-up jingle bank, slot 11) stream in, with the pose actor framed at `FUN_801D5854(seat, 8)`; a party wipe (`5`) lands on phase 5 at once with `DAT_8007BD60 & 0x80` clear, which selects the annihilated arm. The timeline, measured once on `rim_elm_gimard_victory` under PCSX-Redux (`scripts/pcsx-redux/autorun_victory_timeline.lua`):

| Frame (vsyncs from the signal) | Retail | Port |
|---|---|---|
| `+0` | `0x5A` gate: `DAT_8007BD71 = 0xFE`, cause `0` | `BattleComplete` arms the sequence |
| `+0..+80` | CD loads, pose-8 framing on the pose actor | `VICTORY_LOAD_FRAMES` hold, same framing |
| `+80` | results frame: flag `0x35`, round bump, pose clip staged, HP floor at 1 for downed members, XP / gold / drop / level-ups, result window `0x41`, level-up window `0x44+mask` + cue `0x50` | same, through `apply_battle_loot` |
| `+80..+336` | hold (`gp+0xA54` to `0x100`), framing 6 | `VICTORY_RESULTS_HOLD_FRAMES` |
| `+336` | exit-fade template (kind 2, `0x40` frames, black → white), phase halfword from 2 | `screen_fade` = the escape template, drawn by both hosts |
| `+402` | `ctx[+0x6CE] >= 0x43`: `game_mode = 2` | `finish_battle`, windows come down |

The pose actor is `ctx[+0x13]`, and the party **leader** poses: no store in the battle overlay
writes a seat there (every store is a round-boundary zero or the magic menu's MP-cost scratch),
and the three-member `noa_levelup_banner` capture reads `ctx[+0x13] == 0` with seat 0 carrying
the staged pose while Noa is the one who levelled. The pose id comes from the SCUS table at
`0x800788A0` through the HP-quarter tier, aged by the round count and forced weak by the
`0x107B` status mask (`victory_pose_tier` / `victory_pose_column`); the clip is one of the eight
base-archive records the art-bank ladder already resolves for ids `0x11..=0x18`. The port
commits that record the way it commits every art-bank record - as a one-shot that hands back to
the idle loop when it ends - so the pose is struck once on the results frame rather than held
for the `0x100` hold; holding it needs the record's own loop window (`+0x85..+0x86`), which
`MonsterAnimation` does not model yet. The hero's voice line (`monster.snd` tail clips) is not
staged - no engine bank carries `monster.snd`.

An **escape** runs the sequencer's `0x67` arm: no results, the phase halfword counts up from the fade the SM's `0x66` teardown spawned, same `0x43` gate. A **party wipe** runs the annihilated arm: the same `0x100` hold and fade, every seat below the party count floored at 1 HP on the fade frame, unconditionally (`0x8004FB94..0x8004FBA4`, with the roster record's HP / MP written beside it - a scripted loss returns to the field standing), then the MAIN INIT game-over gate `finish_battle` folds. The win arm's floor (`0x8004F390`) touches only a seat at 0 HP. After an unscripted wipe retail is in CARD INIT, so the port runs no further battle frame while it holds the frozen scene for the game-over hand-off.

#### The victory camera

The sequencer frames its pose actor `ctx[+0x13]` on every frame it runs, in two ways:

- **The load window** (the side-band hold at its head, `0x8004E5C0..0x8004E624`, and phases `0..=4`, `0x8004EE10..0x8004EE98`): it stores `ctx[+0xD] = 1`, forces a party seat's target `actor[+0x1DD]` into the monster band `3..=6` (`3` when it is not), turns that target to the pose actor's heading `+ 0x800`, and calls `FUN_801D5854(seat, 8)`. Every monster is down, and a dead monster's node is gone (`noa_levelup_banner`: each dead seat's `+4` reads zero), so case 8 takes its **stand-off arm** (`0x801D6B9C`): `TR (0, 0x400, radius * 5 / 2)` with the radius `actor[+0x22C][+0x58]` (`0x280` for every party member in that state), pitch `0`, focus the pose actor's display X / Z, yaw `-target[+0x46] - ((ctx[+0x26D] << 9) - 0x100) + ctx[+0x6DA]`.
- **The results frame onward** (`0x8004FC80..0x8004FC90`): it stores `ctx[+0xD] = 0` and calls `FUN_801D5854(seat, 6)`. With the signal up and a party seat, case 6 takes the battle-over arm: the close-up from behind the posing character, moved by the per-character win-pose script (`battle_cam_script::battle_over_script`), which reads the close-up accumulator `ctx[+0x87C]` - zeroed by the pose clip's commit (`FUN_8004AD80`, `0x8004BF68..0x8004BF78`) and advanced `8` a frame by every framing call - so the shot keeps moving through the hold.

The escape arm returns before either call (`0x8004E720`). `noa_levelup_banner` reads the results framing directly: Vahn posing `0x14` with `ctx[+0x87C] = 616`, pitch `-0x20` and yaw `0x800 - actor[+0x46]` exactly, TR one tween step short of the script's `(0, 928, prescale(1126))` and walking down toward it from the stand-off pose. The port folds both framings over the camera inputs while `World::battle.victory` is armed (`battle_cam_inputs::battle_end_cam_inputs`); before it, the camera stayed on the far framing with the idle orbit through the whole sequence. Disc-free regression: `engine-core/tests/battle_end_camera.rs`.

The focus both arms take is the pose actor's **body pair** `+0x3C` / `+0x40`, and the store that keeps it current is the battle draw callback's (`FUN_80048A08` -> `FUN_8004998C`, [battle-action.md](battle-action.md#where-an-action-leaves-its-combatants)), which runs for every drawn actor whether or not the action SM does. So the pair follows the win pose through the hold: `noa_levelup_banner`'s Vahn stands at a live `(2, -3)` with his pair at `(78, -15)`, 38 frames into pose `0x14`. The port refreshes the pairs on every sequence tick (`World::refresh_battle_body_pairs`); the root-motion half of the locomotion pass stays with the SM.

The exit fade is a fade **to black**, not a white-out. The template's kind word (`2`) is also
the quad's blend: the fade actor's tick `FUN_80025000` hands it to the quad emitter
`FUN_80024EE4` as the second argument, which folds it into the draw-mode packet's ABR bits (`sll
a3,a1,0x5; ori a3,a3,0xe` at `0x80024FB0`) - the same law the battle-intro styles obey (`abr ==
1` brightens to a white-out, `abr == 2` darkens). Kind 2 is `B - F`, so the black → white ramp
subtracts more each frame, over the scene and the result windows alike (the template's trailing id word, `0`, is the quad's OT bucket - the nearest one). Both hosts
draw `World::presentation.fade` through `engine-ui::screen_prim::screen_fade_prim` in their
screen-overlay pass (the native window's redraw overlay, the play page's intro/FX prim pass); a
host that hand-rolls the quad is how the blend gets lost.
The template's hold word is `-1`, so once the ramp lands the black holds - the world tick never
drops it - until `finish_battle` tears the battle down with its fade actor.

### Scenes that cannot roll

`World::scene_can_roll_encounters` (cached as `World::encounters.scene_rollable`) answers whether the installed scene can produce a random encounter at all. Region lookup stops at the **first** containing region (`RegionEncounterTable::region_at_tile`, matching retail's walk), so a rollable region whose every tile is covered by an earlier rate-0 row is unreachable - which is the case for `town01`, the scene the binary boots into. That is retail scene data and the port keeps it; both hosts say so instead, so a town's designed silence does not read as a broken engine.

The two hosts say it through different channels, and the difference is load-bearing. The native window draws a bounded HUD line (`World::show_encounter_hint`). The browser prints its notice from the page's status bar off `LegaiaRuntime::scene_rolls_encounters` - **not** through the overlay draw list, because the page treats a non-empty overlay as owning the frame (it clears the canvas and returns before the dialog layer), so a passive hint routed there would suppress every NPC dialogue for the first seconds of a town.

The spine began as physical-attack-only, single-formation; the Arts / Magic / Item submenus (above) and monster AI turns layer on top of it. The player-driven Arts submenu routes art-driven strikes through the `apply_art_strike` kernel. Implementation: [`crates/engine-core::world`](../../crates/engine-core/src/world.rs); integration test `crates/engine-core/tests/live_loop_tick.rs` drives boot → walk → encounter → victory → return-to-field through `tick` alone with no test-side battle glue.

## End-to-end gameplay loop integration test

`crates/engine-core/tests/end_to_end_gameplay_loop.rs` stitches every gameplay-side subsystem into one cycle:

1. **Boot** - load an `LGSF` `SaveFile` (party + story flags + money + inventory) into a fresh `World` via `load_full`. `load_full` hydrates the `LevelUpTracker` per-slot level from each record's `+0x130` level byte so reloads don't roll the tracker back to L1.
2. **Field walk** - switch to `SceneMode::Field`, install an `EncounterSession` keyed to `vanilla_formation_table` at saturated trigger rate, step until `EncounterPhase::Triggered`.
3. **Encounter** - drain the formation roll, populate monster slots 3..N from the `MonsterCatalog`, flip mode to `SceneMode::Battle`.
4. **Battle SM** - drive `World::tick` while applying from-scratch formula damage on every `AttackChain → AttackRecovery` transition until the action SM resolves to `BattleEndCause::MonsterWipe`.
5. **Rewards** - call `World::apply_battle_loot` to credit the per-character XP / gold split, fire drop rolls, and trigger per-character level-ups; assert at least one party slot crossed a threshold.
6. **Save round-trip** - `world.save_full().write() → SaveFile::parse() → load_full()` into a fresh `World`; assert HP/MP, level, money, story flags, and inventory survived intact.

The crate ships these test variants:

| Test | Purpose |
|---|---|
| `synthetic_party_completes_full_gameplay_loop` | The default CI cycle; hand-spins the action SM with `apply_strike`. |
| `real_battle_data_encounter_drives_loop` | Disc-gated: scans an early `PROT.DAT` entry for a valid `EncounterRecord` byte pattern, installs it via `World::install_encounter_from_record`, and runs the battle through to `MonsterWipe`. Closes the synthetic-formation leak in the field → battle handoff. |
| `real_psx_memory_card_save_drives_full_loop` | Disc-gated: boots the same loop from a real Legaia memory-card save block via `Party::from_retail_sc_block` when `~/.mednafen/sav/` holds a Legaia card. |

Disc-gated variants skip silently when `extracted/PROT.DAT` / the mednafen card is missing.

## Party wipe + the game-over overlay

Both halves are pinned: the wipe **detection** in the action SM, and
the retail **destination** - the CARD (menu / memory-card) continue
screen, reached through a gate in MAIN INIT, not through the mode-18
"GAME OVER" overlay.

Detection is the `0x5A` end-of-action gate of the action SM (see
[battle-action.md](battle-action.md)). It walks the actor pointer table
counting party actors that are alive (`+0x14C != 0`) and not
counts-as-defeated (`+0x16E & 4`, e.g. Stone). With no survivor it sets
the battle-end signal `DAT_8007BD71 = 0xFE` and the wipe cause
`_DAT_8007BD2C = 5`; the mirror-image monster scan sets cause `0`.

### An unseeded party reads as a dead one

The port carries that scan faithfully, and it can represent a state retail
cannot: retail never enters a battle without a seated party - the seated
count at `*(0x8007BD24)` is established at battle load, and the `beq` at
`0x801E6524` shows a zero count would fall straight into the wipe compare,
so retail is saved by the count, not by a guard. The port's
`BattleActor::liveness` (the `+0x14C` mirror) **defaults to `0`, and `0`
means dead**; it is raised only by the roster projection in `load_party` /
`set_active_party`, which reads `hp_cur > 0` off a `CharacterRecord`. A
world built straight from `SceneHost::open_extracted` has never run that
projection, so its party slots are hollow (`max_hp == 0`, liveness `0`).

The port therefore asks the question retail's load answers structurally:
`BattleActionHost::slot_seated` gates the end-of-action `PartyWipe` arm on
`party_seated > 0` (`engine-core`'s implementation seats a slot when the
roster projects a record onto it or `max_hp > 0`), so an unseeded battle is
never a party wipe - while `MonsterWipe` still resolves, so the port-only
state can terminate. A seated party with nobody standing still wipes.
Disc-free pin: `engine-core/tests/unseeded_battle_wipe_guard.rs`.

A harness that never seats a party is still not a playable party: the pad
ladders seed the retail New Game roster (the `0x80078C4C` template, the way
`BootSession::begin_new_game` does) before scoring a fight, and a wipe is
scored **as** a wipe - the game-over hold below means a wiped battle no
longer leaves `SceneMode::Battle` on its own.

The battle-exit mode selector is `FUN_80046A20` (SCUS, `0x80046A20`).
Its three `game_mode` stores pick between `0` (debug-battle id set),
`0x18` / mode 24 OTHER (arena / Muscle Dome, `_DAT_8007BAC0 & 0x100`)
and `2` / mode 2 MAIN INIT, i.e. back to the field. It **never reads
`_DAT_8007BD2C`** - the wipe cause is consumed only by
`FUN_801D5854` (battle-camera framing) and `FUN_8004E568`. So the
battle itself always exits the same way; the wipe fork lives one mode
later, in MAIN INIT.

### The retail wipe destination is the CARD continue screen

What actually happens after the wipe cause is set is pinned by a
write-watch on the game-mode word across live party wipes (probe
`scripts/pcsx-redux/autorun_gameover_mode_writer.lua`; one scripted-loss
wipe and one plain-formation wipe on the `map01` overworld):

1. The battle tears down through `FUN_80046A20`'s ordinary store
   (`0x80046E0C`): `game_mode = 2` (MAIN INIT), wipe or no wipe. The
   selector also leaves the battle-return marker `_DAT_8007B8B8 = 2`
   - but that store (`0x80046E28`) is **conditional on the marker already
   being non-zero** (`lw` at `0x80046E14`, `beqz` at `0x80046E1C`), and it
   renormalises the `1` the field left there on the way in
   (`FUN_80016230`, `0x80016414`; see
   [`field-locomotion.md`](field-locomotion.md#who-writes-the-word)).
   On the `== 0` arm a second `game_mode` store overrides the first -
   `0x18` at `0x80046E50` with the arena bit set, `0` at `0x80046E60`
   otherwise - so a battle entered without a field departure exits to
   the debug menu rather than to the field.
2. MAIN INIT's scene-setup flow `FUN_8003AEB0` carries the game-over
   gate, in its `_DAT_8007B8B8 == 2` back-from-battle arm: when
   `DAT_8007BD60 & 0x80` is clear **and** story-flag index 0
   (`0x80085758` bit `0x80`) is clear, the store at `0x8003B5D4`
   writes `game_mode = 0x16` (22, CARD INIT) and sets the CARD
   entry-context word `_DAT_8007BB00 = 1`. Mode 22 loads the menu
   overlay 0899 and self-advances to mode 23 (`0x80025974`). With that
   entry context the CARD surface presents the **title screen with the
   cursor on CONTINUE** (framebuffer captured live at the wipe
   destination) - retail's game over is a silent return to the title /
   Continue flow, no GAME OVER art, no menu of its own.
3. `DAT_8007BD60` bit `0x80` is a **party-survived latch** by the time
   the battle ends. Before the fight the same bit is the scripted-fight
   input (seeded by `FUN_8001822C`, `0x80018670` / `0x8001869C`, and by
   the encounter reader for a non-zero `record[+0]`); battle init
   `FUN_800513F0` folds it into `ctx[+0x287]` and clears it (`andi 0x7f`
   at `0x80051A14`), which is why the sparring capture reads `0x01`
   mid-fight. It is cleared again by the `0x5A` end-of-action
   wipe scans (0898 `0x801E65F0` / `0x801E6694`, beside their
   `_DAT_8007BD2C` cause writes), then re-set on the surviving exits: the
   results sequencer's victory arm (`FUN_8004E568`, `ori 0x80` into
   `0xa48(gp)` at `0x8004EDD8..0x8004EDE0`), the successful-escape arm of
   the escape roll `FUN_801E791C` (`0x801E802C`), the sparring fight's
   exit arm in PROT 0967 (`0x801F735C`) and the minigame exit
   `FUN_80026018` (`0x800260AC`). A wipe is the only battle end that
   leaves it clear.
   Captured live on both sides: a victory walks the byte to `0x80`
   before the mode-2 exit and returns to field even with a stale wipe
   cause `5` in `_DAT_8007BD2C` (the gate never reads the cause); the
   plain wipe carries `0` into the CARD handoff.
4. Story-flag index 0 is the **scripted-loss latch**: in the scripted
   Rim Elm ambush loss the scene script raises it at battle start, the
   gate reads it set, the wipe returns to field mode 3 like any battle
   end, and MAIN INIT consumes the latch (both captured live - the flag
   byte walks `0x41 -> 0xC1` at battle entry and back to `0x01` on
   return). Flag index 1 (bit `0x40`) is managed by the same block.
   The consumption is **unconditional**: both the survived exit and the
   loss-return exit join at `0x8003B5F4..0x8003B60C`, whose `andi 0x7f`
   clears flag index 0 on every back-from-battle pass - so the latch can
   never linger into a later battle, and it cannot be read back as an
   outcome signal (`ghidra/scripts/funcs/8003aeb0.txt`).
5. Story-flag index 1 doubles as a script-readable **battle-outcome
   flag**: the same gate sets it on the survived path (`ori 0x40` at
   `0x8003B58C`) and clears it on the wipe path (`andi 0xbf` at
   `0x8003B5A0`), before either path reaches the shared flag-0 clear. A
   scene script that runs on the post-battle reload can therefore test
   flag `1` to distinguish a won battle from a wiped one - the general
   mechanism for scoring a scripted battle from the scene script. The same
   block also clears story flag 14 on every return and, when flag 28 is
   set, flags 29 and 30 (`0x8003B530..0x8003B568`). The Tetsu sparring
   capture pair shows the whole block at once: the flag bank's first four
   bytes walk `81 02 80 00` in the fight (`v0_1_battle_start_tetsu`) to
   `41 00 80 00` back in town01 (`v0_1_post_battle_tetsu_town`) - flag 1
   up, flag 0 consumed, flag 14 cleared - with `DAT_8007BD60 = 0x81`.
   That `0x80` is the sparring overlay's close arm (`0x801F7358`), not the
   formation's: in the fight itself the byte reads `0x01` and
   `ctx+0x287 = 0`, because town01 row 4 carries header byte `0`.
   Engine port: `engine-core::battle_return_flags`, run by
   `World::finish_battle` for every ending, with the survived bit keyed on
   the end cause not being a party wipe.
6. Scripts can invoke the same handoff directly: `FUN_8003C7EC` is a
   helper twin of the inline gate body (same three stores), and the
   field-VM op `4C EA` (MENU_CTRL nibble-E sub-A, see
   [script-vm-menuctrl.md](script-vm-menuctrl.md#0x4c-nibble-0xe00xef---misc-scene-writes--emitter-helpers))
   calls it and halts - the scripted game-over trigger.

### The mode-18/19 overlay is a dev harness

A game-over *artwork* screen nevertheless exists as real disc content.
Mode-table rows 18 / 19 (table at `0x8007078C`, 0x18 stride) hand off
to `FUN_80025B30`, which loads **PROT 0902** at base `0x801CE818` with
its entry at `0x801CE844`. The overlay carries the source path
`h:\prot\field\gameover\gameover.pak`, 29 TIMs (the artwork), a
self-advance to mode 19 and a **single, unconditional** exit that writes
`game_mode = 0`.

That pair is unreachable in retail. The mode-18 entry has no static
writer anywhere on the disc: a scan of every `sb`/`sh`/`sw` to
`game_mode` across `SCUS_942.54` and every PROT entry finds the value
`0x12` written nowhere, no mode-table `next` field chains into 18, and
the only `jal 0x80025B30` is inside `FUN_80025B30` itself. The live
wipe captures close the register-indirect remainder: a real party wipe
routes through the CARD gate above and mode 18 never fires. That 0902
exits to mode 0 - the **debug menu** - fits the same reading: the 18/19
pair is a dev harness around dev art. Relatedly, retail's game over is
**not a menu** and **not a screen**: 0902's only readable string is
`GAME OVER`, and nothing on the reachable path draws it.

### The port's hand-off

`engine-core::game_over::GameOverSession` is the port of that store pair,
not of a panel. It holds for `TITLE_HANDOFF_FRAMES` - the window retail
spends streaming the menu overlay, sized from the title's own `0x11` fade
(the screen-fade level `_DAT_8007BAB4` is clamped to `0xFF` where it is
consumed and drains `8` per frame at `0x801DDAEC`, so `0xFF / 8` = 32) -
draws nothing, reads no button, and resolves to its single outcome
`ReturnToTitle`. Both hosts route it into the same title session their
boot path uses.

The MAIN INIT gate itself folds into `World::finish_battle`
(`engine-core::world::battle::teardown`). Its party-wipe arm mirrors the
`FUN_8003AEB0` block leg for leg: it reads the scripted-loss latch
(story-flag index 0 = system flag 0) and, when set, consumes it
(`andi 0x7f`, `0x8003B608`) and returns to the field like any battle end -
a real wipe inside a scripted-loss battle is not a game over. With the
latch clear it clears the survived-flag bit (`andi 0xbf`, `0x8003B5A0`),
raises `World::game_over`, and queues the BGM **pause**
(`jal 0x800266E0(0x8007052C)` at `0x8003B5EC`, the primitive BGM sub-op 2
wraps) in place of the field-BGM cross-fade - the CARD / title flow owns
audio from the wipe store on. The field restore (actor table, scene mode)
is deferred behind `World::game_over_hold`, so the scene stays parked on
the final battle frame through the hold - retail's frozen wipe frame while
mode 22 streams - and `World::resolve_game_over_hold` completes the
restore when the host's session resolves into the title.

The three-row Continue / Retry / Quit panel that stood here while the
destination was unpinned is **deleted**, builder and all. It was a real
improvement over its own predecessor - a `World::game_over` flag nothing
read, i.e. losing a fight returned the player to the field as if they had
won - but it was still a menu the game does not have, and once one exit
store is pinned, three rows cannot be reconstructed from it.

Mode numbers are decimal in these docs and hex in the dumps, which is a
standing trap here: `_DAT_8007B83C = 0x18` is mode **24** (OTHER /
minigame), not game over. Game over is `0x12`. Relatedly,
`extracted/PROT/0002_gameover_data.BIN` is *not* game-over art - the +2
CDNAME filename shift makes it town01's table.
