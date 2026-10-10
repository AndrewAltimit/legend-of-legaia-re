# Battle subsystem

The battle overlay (`0898_xxx_dat`) carries the battle scene loader, the per-actor state machine, and the effect VM cluster. Loaded at RAM `0x801CE818` (same load slot as the town overlay; battle and town never coexist).

This is a large page covering both the retail reverse-engineering and the
from-scratch engine systems. Use the contents below to jump to a section.

## Contents

**Retail scene + render**
- [Battle scene loader (`FUN_800520F0`)](#battle-scene-loader-fun_800520f0) - [stage-overlay dispatch](#stage-overlay-dispatch-the-0x47-loader-band) · [sparring-tutorial prompts](battle-command-flow.md#the-sparring-tutorial-prompt-machine-overlay-967) · [the two boss-stage modules](battle-command-flow.md#what-the-two-boss-stage-modules-do-overlays-968--969) · [command-flow byte](battle-command-flow.md#the-command-flow-byte-ctx0x06---what-the-hook-table-indexes) · [the round loop](battle-command-flow.md#the-round-loop---what-re-arms-0x1e) · [`s2` + commit](battle-command-flow.md#s2-is-not-the-pad-and-how-a-command-commits) · [commit confirm](battle-command-flow.md#the-commit-confirm-screen-0x6e)
- [Battle background](battle-stage-camera.md#battle-background) - [ground grid](battle-stage-camera.md#backdrop-ground---a-procedural-flat-grid-func_0x801d02c0) · [stage stream per scene](battle-stage-camera.md#which-stage-stream-a-scene-fights-in) · [backdrop shell](battle-stage-camera.md#backdrop-shell---two-copies-of-one-mesh) · [camera](battle-stage-camera.md#battle-camera-exact) · [post-strike two-shot](battle-stage-camera.md#the-post-strike-two-shot-fun_801d5854-cases-7-and-8) · [menu vs input framing](battle-stage-camera.md#the-round-prompt-is-the-far-framing-a-members-surfaces-are-the-close-up) · [resting yaw](battle-stage-camera.md#the-resting-yaw-is-the-orbit-and-battle-init-zeroes-it) · [entry sweep](battle-stage-camera.md#the-battle-entry-sweep) · [party meshes](battle-actor-rendering.md#battle-party-meshes-assembled) · [display list](battle-actor-rendering.md#the-battle-display-list-is-the-registration-set-not-active) · [staged-anim channel](battle-actor-rendering.md#one-staged-anim-channel-actor0x1da)

**Retail battle logic + data**
- [Battle action state machine (`FUN_801E295C`)](#battle-action-state-machine-fun_801e295c)
- [Party wipe + the game-over overlay](battle-round-loop.md#party-wipe--the-game-over-overlay) - [the port's hand-off](battle-round-loop.md#the-ports-hand-off)
- [Battle context struct](#battle-context-struct)
- [Stage seats (`FUN_800513F0` placement tables)](#stage-seats-fun_800513f0-placement-tables)
- [Range / line-of-sight (`FUN_8004E2F0`)](#range--line-of-sight-fun_8004e2f0)
- [Monster init (`FUN_80054CB0`)](#monster-init-fun_80054cb0) - [record layout](#monster-record-source-layout) · [archive (PROT 867)](#monster-archive-prot-entry-867) · [mesh](battle-actor-rendering.md#monster-mesh-record-0x04) · [native bridge](battle-actor-rendering.md#native-renderer-bridge-from-scratch-engine) · [browser battle render](battle-actor-rendering.md#browser-play-page-battle-render) · [AI](battle-round-loop.md#monster-ai-fun_801e9fd4-action-picker--fun_801e7320-target-resolver) · [charm at the end-of-action gate](battle-round-loop.md#enemy-ally-charm-at-the-end-of-action-gate-the-charm-battle-softlock)
- [Stat aggregator (`FUN_80042558`)](#stat-aggregator-fun_80042558)
- [Battle archive (`FUN_80052FA0` / `FUN_800542C8`)](#battle-archive-fun_80052fa0--fun_800542c8)
- [Character record layout](#character-record-layout) - [why the pair order is `(max, cur)`](#why-the-pair-order-is-max-cur)
- [Battle main dispatcher (`FUN_801D0748`)](#battle-main-dispatcher-fun_801d0748) · [hottest utility (`FUN_801D8DE8`)](#hottest-battle-utility-fun_801d8de8) · [weapon trail builder](battle-actor-rendering.md#weapon-trail-builder-fun_8005112c--fun_80048310--fun_800485bc) · [move-FX streak ribbon](battle-actor-rendering.md#move-fx-streak-ribbon-fun_801e1d98)
- [Per-frame actor maintenance (`FUN_8004CE2C`)](#per-frame-actor-maintenance-fun_8004ce2c)
- [Additional SCUS battle-band helpers](#additional-scus-battle-band-helpers)

**From-scratch engine systems**
- [Inventory (page-banked)](battle-round-loop.md#inventory-cratesasset-page-banked-layout) · [Status effects](battle-round-loop.md#status-effects) · [AP / Spirit gauge](battle-round-loop.md#ap--spirit-gauge) · [Battle stat aggregator](battle-round-loop.md#battle-stat-aggregator) · [Item catalog](battle-round-loop.md#item-catalog)
- [Battle round lifecycle](battle-round-loop.md#battle-round-lifecycle) · [HUD model](battle-hud.md#battle-hud-model) · [screen chrome](battle-hud.md#battle-screen-chrome-packet-pinned) · [widget-class table](battle-hud.md#the-widget-class-table---where-every-chrome-sprite-comes-from) · [SFX bank](battle-round-loop.md#sfx-bank--scheduler)
- [Inventory item-use session](battle-round-loop.md#inventory-item-use-session) · [Encounter system](battle-round-loop.md#encounter-system) · [target picker](battle-command-flow.md#battle-target-picker)
- [Equipment catalog](battle-round-loop.md#equipment-catalog) · [Seru capture + spell learning](battle-round-loop.md#seru-capture--spell-learning) · [Tactical Arts chain editor](battle-round-loop.md#tactical-arts-chain-editor) · [rewards composite](battle-round-loop.md#battle-rewards-composite)
- [Live gameplay loop - Field ↔ Battle](battle-round-loop.md#live-gameplay-loop---field--battle-in-tick) - [auto vs player-driven](battle-round-loop.md#auto-resolve-vs-player-driven) · [post-battle Seru learning](battle-round-loop.md#post-battle-seru-learning)

**Runtime-memory captures + tests**
- [Encounter trigger memory layout](battle-round-loop.md#encounter-trigger---runtime-memory-layout) · [scene-init residency](#battle-scene-init-residency-window) · [item-use residency](#item-use-battle-event-residency) · [stat-growth observations](battle-round-loop.md#captured-stat-growth-observations)
- [CDNAME → MV STR cutscene routing](battle-round-loop.md#cdname--mv-str-cutscene-routing) · [end-to-end gameplay loop test](battle-round-loop.md#end-to-end-gameplay-loop-integration-test)
- [Field-to-battle intro presentation](battle-stage-camera.md#field-to-battle-intro-presentation)

## Battle scene loader (`FUN_800520F0`)

Multi-step async state machine; sub-state byte at `gp+0xa59`. The dual-mode
loader (`_DAT_8007b8c2`) chooses between PROT-TOC indices (dev) and
`h:\prot\battle\*.dat` ISO9660 files (retail) for the same data. Notable steps:

Every index in this section is a **raw TOC** index, the space the loader's
own `li a0,…` constants live in; the extraction entry is two lower
([`cdname.md`](../formats/cdname.md#numbering-space)). All four members
belong to the `befect_data` block - raw 872..875 = extraction 870..873 =
`etim` / `etmd` / `vdf` / `efect` ([`effect.md`](../formats/effect.md)).

- **State `0x8`** - loads the battle texture pack: PROT raw `0x368` (872) =
  extraction 870 / `etim.dat`.
- **State `0xb`** - loads the battle **model** pack: PROT raw `0x369` (873) =
  extraction 871 / `etmd.dat`, together with raw `0x36a` (874) = extraction
  872 / `vdf`. One read covers both: `FUN_8003e8a8(0x369)` leaves 873's LBA
  in `gp+0x8f0` and its sector count in `gp+0xa84` (`0x80052518`), then
  `FUN_8003e68c(0x36a)` adds 874's sector count (`0x8005253c`) so the
  transfer is `size(873) + size(874)` sectors from 873's LBA. The 874 half
  lands at `base + size(873)*2048`, cached at `0x8007B878`.
- **State `0xc`** - two loops over that contiguous 873+874 load. The FIRST
  (`jal 0x8001FBCC` at `0x80052584`) walks the **874 half** - the `vdf`
  pack, header `[u32 count][u32 byte_offsets[count]]` (count `0x20`) - and
  appends each `base + offset` to the VDF pointer table `0x80083E58`
  (`FUN_8001FBCC` is that table's append). It does **not** touch the
  character pack, whose *extraction* label is also 874; that collision is
  the falsified "the battle loader reads PROT 0874's header words as VDF
  pointers" reading - see
  [`character-mesh.md` § Not a dual consumer](../formats/character-mesh.md#not-a-dual-consumer---the-battle-vdf-pack-is-a-different-entry).
  The SECOND walks the **873** (`etmd`) pack and calls `tmd_register` on every entry
  (`jal 0x80026b4c` = `FUN_80026B4C`, the sole `DAT_8007C018` installer),
  then loads `efect.dat` / PROT raw `0x36b` (875) = extraction 873.
  **This registration fills the
  effect/model window `DAT_8007C018[3..]`, NOT the party `[0..=2]`.** The party
  battle meshes come from a **separate** pack - **PROT 1204 (`other5`)**,
  installed into `DAT_8007C018[0..=2]` for Vahn/Noa/Gala by **static SCUS battle
  state-handlers** (NOT an overlay): `FUN_800513F0` registers the active-actor
  meshes (`tmd_register(*(actor+0x50)+0x18)` in a `while<3` loop, alongside the
  `FUN_80052FA0` palette decode) and `FUN_800542C8` registers the additional
  party members (per-member loop, `tmd_register(*(*rec+4))`). Both are dispatched
  indirectly, so a static `DAT_8007C018` cross-reference finds no writer; pinned
  by a write-watchpoint at battle entry ([`autorun_battle_party_mesh_install.lua`](../../scripts/pcsx-redux/autorun_battle_party_mesh_install.lua),
  installed pointers byte-match the battle form - e.g. Vahn at `0x80165f48`). The
  party actors' mesh pointer `actor[+0x230]` resolves
  to those `[0..=2]` entries. The installed meshes are **assembled per
  character from the player battle files** (equipment-id-selected sections,
  spliced by `FUN_80052FA0`/`FUN_800536BC`; byte-verified against the live
  party vertex pools - [character-mesh.md § Battle form](../formats/character-mesh.md#battle-form---assembled-from-the-player-files)).
  The field pack 0874 §0 is field-only; PROT 1204 is the Baka Fighter
  default-equipment sibling pack.
- **State `0xE`** - initialises the runtime [effect 2-pack wrapper](../formats/effect.md) via `FUN_801DE914`. Also fires for the field-VM op `0x3E` scripted-battle / door-warp paths on the system context.
- **State `0xFF`** - dispatches the side-band streaming-effect handler `0x801F17F8` for `summon.dat` / `readef.DAT` (extraction PROT 893 / 894; format + verification in [`formats/summon-readef.md`](../formats/summon-readef.md)).

A paired stage pack loads at raw TOC `0x367`/`0x36d` (= extraction entries 0869/0875) in states 2/4/6.
The asset-viewer's `--bundle battle` mode mirrors this loader's PROT 865–890 set so character meshes have the right CLUT bindings.

### Stage-overlay dispatch (the `+0x47` loader band)

Sub-states `0x0E` and `0x10` read the **battle-stage id** byte `_DAT_8007B64A`
and, only when it is non-zero, page a per-stage code overlay into slot B. Both
arrive at the same block: the loader's sub-state dispatcher routes `0x0E` at
`0x80052198` and `0x10` at `0x800521EC` into `0x8005266C`/`0x80052670`, which
fall through to the id read at `0x80052678`.

```
stage_id = *(u8 *)0x8007B64A;                     // lbu v1,-0x49b6(v1) @ 0x8005267C
if (stage_id == 0) goto no_stage;                 // beq v1, zero  @ 0x80052688
sub_state = 0x11;                                 // sb v0,0xa59(gp) @ 0x80052698
FUN_8003EC70(stage_id + 0x47, 0);                 // addiu a0,a0,0x47 @ 0x800526A0
```

`0x11` is written on the way *out*, as the state entered once the load has been
issued - it is the load-wait state, not the reader. Dispatched at `0x800521D0`,
it joins the shared wait block `0x800526C8` that polls `FUN_8003DE7C`.

Overlay loader B resolves extraction entry `param + 0x37F`, so a stage overlay
lives at **extraction `stage_id + 966`**. This is the `+0x47` computed-parameter
site in the SCUS loader census, and the only call site that can reach entries
**967 / 968** - no constant-parameter site produces them.

`SCUS_942.54` touches the id byte in three places: two clears, and
`FUN_80055B6C`'s per-formation override `*_DAT_8007BD0C == 0xB5 → 2` (entry
968, `0x80055D2C..0x80055D44`), where `_DAT_8007BD0C` is the formation's
monster id. A **fourth writer lives outside the SCUS census**, in the battle
overlay itself: the tail arm of the Lost Grail Final Heal sweep
`FUN_801E6968` (`0x801E6CE4..0x801E6D64`, the `sb v0,-0x49b6(a0)` at
`0x801E6D2C`; `overlay_battle_action_801e6968.txt`), run by cleanup state
`0x50` of the battle SM `FUN_801E295C`. It writes stage id **3** (entry 969)
mid-fight when both hold - the formation cell still reads `0xB5` (**Cort**,
archive id 181; see
[`re-settled-threads.md`](../reference/re-settled-threads.md) for the 0968 /
0969 identifications and the Lapis-Wave id-space collision), **and** the
first monster seat (`actor_table[3]`) has HP `+0x14C == 0`. The arm issues
the loader-B page-in itself (`jal 0x8003EC70` at `0x801E6D14` with
`a0 = 0x4A = 3 + 0x47` - same-frame, not deferred to the dispatch reader),
bumps the battle ctx phase counter `ctx[+0x26]`, forces the flow-state byte
`ctx[+0x7] = 0xFD`, and zeroes the dead seat's `+0x21C` / `+0x225`. So the
Cort fight walks two stage overlays: 968 from setup (phase 1 alive), 969
once the form dies - the guard separating the arms is the seat's liveness,
not a different id. (A print-integrity footnote: this arm was long carried
at the phantom coordinate `0x801FD514` from a base-tag-less `overlay_0897`
dump, `+0x167E8` high; the store's byte pattern occurs in no PROT entry but
0898, at file `0x18510`.) Engine mirror:
`engine-core::battle_stage_module::battle_init_stage_override` /
`boss_transition_stage_id`, written into the stored stage byte by
`World::enter_battle_from_formation` and `World::run_boss_transition_arm`
(`world/battle/stage.rs`); the 968 / 969 behaviour is ported beside them -
[below](battle-command-flow.md#what-the-two-boss-stage-modules-do-overlays-968--969).

**Stage id `0` is the norm, not a fallback.** Across the catalogued battle
save-state library every battle reads `0` - the fight simply draws over the
resident field/world backdrop - except the **Tetsu sparring tutorial**, which
reads `1` and whose loader-B current-id tracker `gp+0x934` (`0x8007BC4C`) holds
`0x48` = extraction **967**, the battle tutorial overlay. `_DAT_8007BD0C` reads
`0x4F` (Tetsu's archive id) in those same states.

The overlay is battle *code*, not stage geometry: the backdrop mesh comes from
the resident scene bundle (below). Engine mirror:
[`engine-core::overlay_loader::battle_stage_overlay_entry`](../../crates/engine-core/src/overlay_loader.rs);
oracle `crates/engine-shell/tests/battle_stage_live.rs`.

#### Who writes stage id `1` - the one-shot arm flag `0x19`

None of the three SCUS sites above ever writes `1`, so the loader census alone
cannot say what turns the tutorial on. The writer lives in the field/world
**entity SM** `FUN_801DA51C`, in the tail that commits an installed encounter
record to a fight - right after it clears `entity[+0x94]` and bumps the
battle counter `entity[+0x8A]`:

```
801da698  jal 0x8003ce64            ; TEST(a0 = 0x19)   - system-flag bank
801da69c  _sb zero,-0x49b6(s0)      ; delay slot: stage id = 0
801da6a0  beq v0,zero,0x801da6b4    ; flag clear -> no stage overlay
801da6a4  _li v0,0x1
801da6a8  sb v0,-0x49b6(s0)         ; stage id = 1  -> extraction 967
801da6ac  jal 0x8003ce34            ; CLEAR(0x19)   - fire once
801da6b0  _li a0,0x19
```

So the id is not a property of the formation, the scene or the monster: it is a
**one-shot system-flag arm** (`0x19` in the `DAT_80085758` bank), consumed by
the first battle entered after it is raised. The `sb zero` sits in the `jal`
delay slot, so the default `0` is written on both paths.

The setter is disc data. A disc-wide field-VM flag census finds exactly one
site writing flag `0x19`: town01's own Tetsu sparring record, where the bytes
`50 19` (op `0x5x` SET) sit two ops before that record's `3E FF` battle-entry
op, between Tetsu's `"Come at me!"` line and his post-fight one. No other scene
sets it and no script tests it - the entity SM is the only reader.

Engine port: [`battle_tutorial::TUTORIAL_ARM_FLAG`](../../crates/engine-core/src/battle_tutorial.rs)
plus `stage_id_at_battle_entry`, consumed by `World::enter_battle` through
`World::take_battle_tutorial_arm`. Because the arm is disc-side, no host
decides anything: the native window and the browser play page each get the
tutorial in the fight retail gives it and in no other.

A **direct entry** into the row (`play-window --battle 4`) runs the battle
entry without the record, so the arm has to be replayed from the record's
own bytes: `man_field_scripts::walk_battle_entry_arms` pairs every system
SET with a `3E FF <row>` battle-entry op that follows it within a few
coherently decoded instructions, and `World::replay_scripted_battle_arm(row)`
raises the flag when the pairing `(0x19, row)` exists in the scene's script.
The pairing is the key, not the flag census's `clean` bit: the SET sits a
few ops past the record's dialogue bytes, where the linear walk is still
resynchronising, so the census reports the one real site as desynced. A
phantom SET inside text is not followed by a decodable `3E FF` and a real
one is.

## Battle action state machine (`FUN_801E295C`)

16 KB / 4099 instructions / 155 outgoing calls. The action-execution dispatcher: it takes the player's selected action and runs it to completion across multiple frames.

`_DAT_8007BD24` is a **pointer** to the active battle context struct (typed `int*` in the decompile output). The pointer itself is resolved at battle entry; `*_DAT_8007BD24` = `0x800EB654` for the captured battle. The action state machine accesses fields as `(*_DAT_8007BD24)[N]` - i.e. byte N of the pointed-to struct.

The outer dispatch is `switch((*_DAT_8007BD24)[7])` - byte +0x07 of the ctx struct, which holds the **active action ID** for the currently-resolving action slot. Byte `+0x06` is not a parallel monster ID: it is the command **menu** SM's flow byte (`FUN_801D0748`), a different state machine on the same struct. The inner dispatch is `switch(actor[+0x1DE])` - the committed **action category** (`1` item, `3` attack, `4` spirit, `5` run), which `FUN_801D0748` stamps on all three party actors at commit (`0x801D1174..0x801D1184`).

Action IDs surfaced from save-state captures:

| ID | Action |
|---|---|
| `0x20` | Special move / capture (different sub-states) |
| `0x28` | Action-menu cursor active (player still selecting) |
| `0x35` | Magic - summon |
| `0x47` | Spirit |
| `0x50` | Martial-arts directional input mode |

The function reads battle actor pointers via `(&DAT_801C9370)[ctx[0x13]]` (resolves the active actor via `ctx[0x13]` = actor slot index, then indexes the 8-slot pointer table). It guards on `_DAT_800846C0 != 2` (game-state check). The global pointer `_DAT_8007BD24` plays the same role as the field-VM context pointer - this is a state machine, not a bytecode VM, but it shares the field VM's "context-pointer-as-VM-state" idiom.

Distinct from:
- The [field/event script VM](script-vm.md) (which doesn't run in battle).
- The [effect VM cluster](effect-vm.md) (which handles per-effect spawn/render but doesn't drive actor decisions).
- The [move-table VM](move-vm.md) (which drives Tactical Arts inputs and per-action keyframe scheduling - a layer below this one).

Found via the `overlay_battle_action.bin` import (a save state captured with the action menu open). Dumped as `ghidra/scripts/funcs/overlay_battle_action_801e295c.txt`. The 78-function inventory of the battle overlay is in `overlay_battle_action_inventory.txt` (top 80 dumped). All 6 captured battle modes (summon / special-move / martial-arts-input / spirit / action / capture) load identical battle overlay code - only data buffers (actor table at `0x801C9370`, ctx struct at `0x800EB654`, GPU OT lists, audio scratch) differ between captures.

## Battle context struct

The active battle context lives at `0x800EB654` (resolved at battle entry; the global pointer at `0x8007BD24` is set to this address). 32-byte fixed prefix followed by a per-battle dialog/text buffer.

| Offset | Type | Use |
|---|---|---|
| `+0x00` | u8 × 6 | Battle phase/state flags (mostly `01 01 01 00 00 00` while a turn is resolving). |
| `+0x06` | u8 | The **command-flow byte** - the menu state machine's cursor, dispatched by `FUN_801D0748`. Value space `0xFD` (SCUS battle init's store, `FUN_80055B6C` at `0x80055FA8`, before the overlay's init), `0x00`, `0x0A`, `0x0B`, `0x0C`, `0x14`, `0x1E`, `0x28`, `0x32`, `0x3C`, `0x46`, `0x50`, `0x5A..0x5E`, `0x64..0x67`, `0x6E`, `0x78`, `0xFE`. See the flow table above. |
| `+0x07` | u8 | Party-slot active action ID (or `0xFF`). The outer `switch((*_DAT_8007BD24)[7])` in `FUN_801E295C` keys on this. |
| `+0x09` | u8 | Turn / phase counter. |
| `+0x13` | u8 | Active-actor slot index - used to look up the actor pointer via `(&DAT_801C9370)[ctx[0x13]]`. |
| `+0x14..+0x17` | u8 × 4 | Per-action parameter bytes (target slot, sub-action, etc. - varies by action ID at +0x07). |
| `+0x18..+0x1B` | u8 × 4 | More action params (dir/elem byte at +0x18, second target at +0x1A, etc.). |
| `+0x1D` | u8 | Action context flag - `0x03` for summon and capture; `0x00` otherwise. |
| `+0x29..+0x2D` | string | Active spell/move icon glyph (`0xCE 0x14 0x20 'G' 'i' 'm' 'a' 'r' 'd' …`). |
| `+0xA9..+0xEC` | text | Battle dialog buffer (`"Vahn won the battle!|Gained …Experience and …G."`). |
| `+0x6D6` | u16 | Battle-open **intro timer**, written as a halfword by the `0x0A` arm (`0x5A`, or `0x78` when `ctx[+0x290] != 0`) and counted down by `0x0B`. Base of the camera/timer trio with `+0x6D8` (Done-band countdown) and `+0x6DA` (drifting yaw). The action SM's own cursor is `ctx[+0x07]`. |

Only the leading 32 bytes vary between captures. Beyond `+0x40` the buffer is a long text-rendering scratch area populated when battle messages are printed. Engine port models this as a 1-of-N enum for the action-ID byte, with side-data fields populated per-action.

| Slot | Role |
|---|---|
| `0..2` | Active party members (ordered by formation). |
| `3..7` | Monster slots (up to 5 enemies per battle). |

Combatant struct fields surfaced by helpers analysed so far:

| Offset | Type | Use |
|---|---|---|
| `+0x07` | u8 | Per-actor state byte. Drives `FUN_801E295C`. |
| `+0x13` | u8 | Active-character index (read from `_DAT_8007BD24+0x13`). |
| `+0x1F` | u8 | Hit-radius / size byte. Used by `FUN_8004E2F0` (range). |
| `+0x34` / `+0x38` | i16 | Current world X / Z (Y in the adjacent halfwords `+0x36`/`+0x3A`; `0` on the flat stage). |
| `+0x3C` / `+0x40` | i16 | The **body pair**: stamped with the authored stage seat at setup (`FUN_800513F0` copies the seat here, then into `+0x34`/`+0x38`), then rewritten every drawn frame by the pose decoder `FUN_8004998C` as the live pair plus the facing-rotated pose centroid ([battle-action.md](battle-action.md#where-an-action-leaves-its-combatants)). Read as the b-actor position by `FUN_8004E2F0` and on both sides of the separation pass. |
| `+0x4A` | u8 | Magic-slot count. |
| `+0x4C` | int* | Spell-entry pointer array (each entry: `[u8 spell/action id, …, u8 AGL (action) cost @ +0x74]`). |
| `+0x14C..+0x152` / `+0x172..+0x174` / `+0x150..+0x158` | u16 | HP / MP / current / max - three-way mirror layout. |
| `+0x1BC..+0x1BE` | u8 | "Show damage" overlay byte triplet. |
| `+0x1DF` | u8 | First byte of the **arts / queued-move command buffer** (`+0x1DF..=+0x1E3`), written by the command commit. The monster size byte is *not* copied here - `FUN_800513F0` stores `size << 5` to `actor+0x58`. |
| `+0x1EF..+0x1F3` | u8 | Hit-reaction staged-anim ids - slot indices of the block entries tagged `2/3/4/5/0xB` (flinch / knockdown / get-up / Block at `+0x1F3`), filled by `FUN_80054CB0`; cast modules stage the victim's reaction from `+0x1F1`. Not element data - those tags double as elemental markers only inside the `+0x4C` spell list. |
| `+0x230` | u32 | Pointer to the monster's **battle-model TMD** (set from record `+0x04`; **not** XP/drop). `FUN_800495C8` walks it as a `0x1C`-stride object table. See [Monster mesh](battle-actor-rendering.md#monster-mesh-record-0x04). |

## Stage seats (`FUN_800513F0` placement tables)

Every combatant's battle position is stamped at setup from two static `SCUS_942.54` tables of 8-byte seat entries `[i16 x, i16 y, i16 z, i16 pad]` (`y` is `0` on every row - the stage is flat). `FUN_800513F0` passes the entry to the spawn-node builder `FUN_80024c88` (which copies it verbatim to node `+0x14/+0x16/+0x18`), then writes node `+0x14`/`+0x18` to the actor seat pair `+0x3C`/`+0x40` and copies that into the live position `+0x34`/`+0x38`. The party faces `+Z`, the monsters `-Z`, and the battle camera orbits the origin between the rows.

**Party table `0x800775C8`** - row = `ctx+0` (the party count), stride `0x18` (3 slots x 8 bytes):

| Count | Slot seats (x, z) |
|---|---|
| 1 | `(0, -800)` |
| 2 | `(300, -800)` `(-300, -800)` |
| 3 | `(0, -825)` `(600, -775)` `(-600, -775)` |

**Monster table `0x80077608`** - row = `ctx+1` (the monster count) `+ 4` for the alternate family, stride `0x20` (4 slots x 8 bytes; the placement loop seats at most 4 monsters):

| Count | Normal family (x, z) | Alternate family |
|---|---|---|
| 1 | `(0, 800)` | same |
| 2 | `(-300, 800)` `(300, 800)` | same |
| 3 | `(-600, 825)` `(0, 750)` `(600, 825)` | `(0, 900)` `(-600, 700)` `(600, 700)` |
| 4 | `(-900, 900)` `(-300, 800)` `(300, 800)` `(900, 900)` | `(0, 1000)` `(-600, 800)` `(600, 800)` `(0, 600)` |

The alternate family is selected by `DAT_8007BD60` bit 7 - the same bit the setup stores to `ctx+0x287`, the no-escape flag the run/escape roll honours - or by formation ids `0x3D..0x3F` in modes `0xC`/`0x15` (the scripted / pincer fights).

Save-state validation: seven battle library captures (the four camera-orbit angle saves, the three Tetsu tutorial anchors) read the count-1 seats byte-exactly at actor `+0x34`/`+0x38` (`(0, -800)` vs `(0, +800)`). Every three-on-one capture reads the party at `z = -812 / -762` and the monster at `813` - the authored rows moved `+13` in Z. That offset is not drift: it is the round-start recentre below, and a balanced formation (three on three, one on one, two on two) reads its authored rows unmoved.

**Pool slots are fixed.** Party member `i` takes actor-table slot `i` and monster `k` takes slot `3 + k` whatever the party size (`0x801C9370 + (k+3)*4`, `addiu s0,s2,0x3` at `0x8005185C`), so a party of one leaves slots `1` and `2` empty - the catalogued solo and duo fights read their monsters in slots `3..` and zeros at the unused party slots. The engine compacts monsters down to `party_count + k`; the seat each combatant takes is the same on both, so the difference is an index space, converted where a routine reads a fixed slot (`World::retail_battle_pool_slot`).

**The formation recentres every round.** The battle flow SM runs `FUN_801DB318` at every round start (`FUN_801D388C(0, 0)` at `0x801D0EE4`, between the initiative seeder and the DoT tick) and when the ring's first member cancels back to the round prompt (case `2`, `0x801D11E0`). It takes the X/Z extents over pool slots `0..3` unconditionally and `3..7` with live HP, squashes an axis whose span exceeds `0x800` back to `0x800`, then subtracts the centroid `((max + min) as u32) >> 1` from every included actor.

Nothing walks a combatant home after an action (`World::tick_battle_locomotion`), so this is what pulls a wandered formation back into frame; on an authored formation it is the recentre alone, which is `-13` for `z = -825 ..= 800`. The focus pair it also shifts (`_DAT_80089118` / `_DAT_80089120`, the negated camera target) is re-derived by the far framing it arms next (`FUN_801D5854(0, 9)`). Engine: `World::normalize_battle_formation`, called from `begin_battle_round` and the ring cancel.

**The alternate family is the scripted flag.** The monster row index is `ctx[+1] + ((DAT_8007BD60 >> 5) & 4) + s4` (`0x80051838..0x8005184C`), so the scripted-fight bit alone moves a fight to rows `5..8`; `s4 = 4` is the map-gated arm (first monster `0x3D..=0x3F` on `_DAT_80084540` `0x0C` / `0x15`).

The engine counts both addends (`World::seat_monster_family`): one selects the alternate family, both select rows `9..12`, which the disc leaves zero-filled.

The Rim Elm ambush reaches row 8 by the map arm alone. Its row (`town0b` / `town0c` formation row 3, `[0x3F, 0x3E, 0x3E, 0x3E]`) carries header byte `0`, and the field VM's `3E FF 03` arm (`0x801E070C..0x801E0788`) writes only the system entity's `+0x8A` / `+0x94`, the step counter and the mode request - so `DAT_8007BD60` bit 7 stays clear and `ctx+0x287` is `0`. A capture of the `rim_elm_queen_bee_battle` state reads exactly that (`DAT_8007BD60 = 0x00100003`), the seat loop fetching row index 8 (`(0,1000) (-600,800) (600,800) (0,600)`), and the formation roll raising `_DAT_8007BAC0` to `0x200`. The ambush is therefore escapable, draws the random-encounter boost profile, and runs the formation roll. No retail fight is known to select rows `9..12`.

Of the disc's `3E FF` sites whose row the bundle MAN carries, two more rows carry header byte `0`: `town01` row 4 (`0x4F`, the Tetsu spar) and `deene` row 11 (`0xA7`). The spar still skips the formation roll, through its other gate: the tutorial arm sets the battle-stage id `DAT_8007B64A` (`0x80051DB8`). The engine derives `ctx+0x287` from the row alone (`World::enter_battle_from_formation`); `World::trigger_scripted_battle` sets no flag. Disc-gated check: `crates/engine-core/tests/rim_elm_ambush_disc.rs`.

The map id is `_DAT_80084540`, the loaded scene's **raw CDNAME define** (`town01` = `3`, `town0b` = `0x0C`, `town0c` = `0x15`, `map01` = `0x55`; every catalogued save state reads the define of the scene named at `0x80084548`), carried as `BattleState::map_id` and also read by the formation roll's scripted-ambush arm and the intro style picker. It is two above the extraction index `Scene::start` holds, which the intro picker had been reading - so its `0x3E` / `0x3F` arm on `3` / `0x0C` / `0x15` could never match.

Engine mirror: [`engine-core::battle_seats`](../../crates/engine-battle/src/battle_seats.rs) (consumed by `World::enter_battle`).

### The Ra-Seru-forbidden bit of the special-battle word

The same two map-gated fights also forbid the Ra-Seru chip, through bit `0x200` of the special-battle word `_DAT_8007BAC0` (the word whose `0x100` bit is the arena's Item restriction). Two routines write it at battle setup:

- **Battle init** (`FUN_800513F0`, `0x800519C0..0x80051A04`) first clears the word when it holds exactly `0x200`, so a lone Ra-Seru bit does not outlive its battle, then raises `0x200` when the formation's first monster (`DAT_8007BD0C`) is `0xAF`.
- **The formation roll** (`FUN_80051D84`, `0x8005200C..0x8005205C`) raises `0x200` for first monster `0x3D..=0x3F` on map `0x0C` / `0x15` - the Rim Elm ambush. The test sits at the tail of the back-attack arm, which the forced ambush always takes, so a roll that runs on such a formation always raises it; a roll the caller skips (`ctx+0x287`, `DAT_8007B64A`) raises nothing. The `0xA7` force reaches the same tail and raises nothing.

The battle round driver `FUN_801D0748` (PROT 0898) reads the bit twice in the command ring's phase-`0x28` arm: `0x801D12DC..0x801D12F4` draws the red cross-out (`FUN_801DBC30(0xF8, 0x42)`) over the Ra-Seru chip, and `0x801D1448..0x801D1454` returns from the chip's arm without committing. A sweep of SCUS, 0897, 0898 and 0899 for `lw` of `0x8007BAC0` followed by `andi 0x200` finds only those two readers.

Engine: `BattleState::special_word` carries the regular battle's word (the Muscle Dome session keeps its own); the two raisers are `battle_formulas::battle_init_special_word` and `formation_roll_special_word`, run from battle setup. `battle_hud::battle_magic_chip` clears the chip's `enabled` flag and the ring refuses the Magic arm (`World::tick_battle_command`). `battle_hud::battle_raseru_cross_out` answers whether the ring draws the cross-out this frame, and both play hosts draw it as a chrome-atlas sprite over the chip (`engine-ui::battle_command_ui::cross_out_mark_sprite`, anchor `(0xF8, 0x42)`), its texels baked from the effect page by `save_menu_atlas::add_cross_out_mark`.

The word's other readers test it whole (`!= 0`), so the Ra-Seru bit also withholds the gold, EXP, drop and steal, the Seru absorb and spell XP, and a monster's flee - the table is in [battle-formulas.md](battle-formulas.md#the-special-battle-words-readers). The engine reads all of them through `World::special_battle_word`, the arena word ORed with this one.

## Range / line-of-sight (`FUN_8004E2F0`)

Its first test is the battle-end byte `0x8007BD71`: anything but `0xFF` - the
wipe and escape teardowns store `0xFE` - returns the out-of-range `1` before
any slot is read (`0x8004E2F4..0x8004E310`); the port reads `battle.end` for
it.

`FUN_8004E2F0(actor_a_id, actor_b_id) -> i16 distance` is the canonical battle range check, called 5+ times from the per-actor state machine. Reads `[DAT_801C9370 + id*4]` for both actors, computes a euclidean distance from `+0x34/+0x38` (or `+0x3C/+0x40` for the b-actor), then sums the two `+0x1F` size bytes (party-member size table at `0x80078878`, monster size byte read from the live actor) to get the hit radius. Final value is clamped to a per-actor cap and `0xF` per `param_2 < 3` party tier.

## Monster init (`FUN_80054CB0`)

Called from `FUN_800542C8` (secondary battle archive loader). Populates a battle-actor at `[DAT_801C9370 + (slot+3)*4]` from a monster record:

- HP / MP / AGL triplets at `+0x14C..0x158` and `+0x172..0x174` (AGL = the agility / action gauge at `+0x154/+0x156`).
- Five per-action-tag **slot indices** at `+0x1EF..+0x1F3`. The tag-match loop (`0x80055340..0x800553F0`) walks the `+0x4C` entry list, compares each entry's first byte against `2 / 3 / 4 / 5 / 0xB`, and stores the **loop index** - so these are staged-anim entry ids (tag-2 flinch, tag-4 knockdown, tag-5 get-up), not packed resistance nibbles.
- Walks the spell list at `+0x4C` (count at `+0x4A`): for the elemental ids (`2,3,4,5,0xB`) it records the matching spell's slot index into the per-element table at `+0x1EF..+0x1F3`.
- Battle-model TMD pointer (record `+0x04`) into `+0x230`.

This is the canonical "monster spawn" path. Engine port reads the record once, populates the actor struct, and lets `FUN_801E295C` take over.

### Monster-record source layout

`param_1` is the in-RAM monster record (after the loader's offset→pointer fixups). Field map traced from `FUN_80054CB0`:

| Offset | Type | Use |
|---|---|---|
| `+0x00` | u32 | Name string pointer (disc offset → pointer; `strlen` copied into actor `+0x1BC`). |
| `+0x04` | u32 | Block-relative offset of the monster's **battle-model TMD** → actor `+0x230` (walked as `0x1C`-stride geometry records - a TMD object-table entry is `0x1C` bytes - by `FUN_80049858` / `FUN_800495C8`). **Not** XP/drop. See [Monster mesh](battle-actor-rendering.md#monster-mesh-record-0x04). |
| `+0x08` | u32 | Shared-resource pointer (fixed up at load). |
| `+0x0C` | u16 | **HP** → actor `+0x14C/+0x14E/+0x172`. |
| `+0x0E` | u16 | **AGL** → actor `+0x154/+0x156` (agility / action gauge, cur+base; spent per action, reset each round; "Power Up" raises it - *"agility increased!"*). |
| `+0x10` | u16 | **MP** → actor `+0x150/+0x152/+0x174`. |
| `+0x12` | u16 | **ATK** → actor `+0x158/+0x15A` (attacker offense in the damage routine). |
| `+0x14` | u16 | **UDF** (upper defense) → actor `+0x15C/+0x15E` (defender defense, high facet). |
| `+0x16` | u16 | **LDF** (lower defense) → actor `+0x160/+0x162` (defender defense, low facet). |
| `+0x18` | u16 | **INT** → actor `+0x168/+0x16A` (magical damage / magic defense in the summon/arts kernel + the accuracy/evasion seed; the bestiary INT column. Meth962: INT "affects your magical damage and defense against other magical spells"). |
| `+0x1A` | u16 | **SPD** → actor `+0x164/+0x166` (turn-order initiative seed; buffable). |
| `+0x1C` | u8 | **readef animation-group index** (`0..=25`). Read **record-direct** through the same `0x801C9348` pointer table, never copied to the actor. The per-turn initiative scheduler `FUN_801DABA4` turns it into the side-band streaming applier's base slot - `base = 3 * group`, then `ctx+0x277 = base` (`overlay_battle_action_801daba4.txt` `0x801db098` / `0x801db0c8`) - so the group names three `readef.DAT` slots. The AI spell picker `FUN_801E9FD4` reads the same byte as a monster-family tag (`0x801ebb90`: `group == 0x17` selects a hardcoded action id). Census + group semantics in [`summon-readef.md`](../formats/summon-readef.md#which-monsters-name-which-readef-group). Parser: `MonsterRecord::readef_group`. |
| `+0x1D` | u8 | **Element id** (`0..=7`: earth / water / fire / wind / thunder / light / dark / neutral). Read record-direct through `0x801C9348` by the affinity scale `FUN_801DD864` (`overlay_battle_action_801dd864.txt` `0x801dd8dc`), never copied to the actor. Parser: `MonsterRecord::element`; matches `legaia_asset::element_affinity::Element`. |
| `+0x1F` | u8 | **Size class** - body bulk. Read **record-direct** through the same `0x801C9348` pointer table, never copied to the actor: the battle camera's per-action framing `FUN_801F0348` computes `ctx+0x6D0 = clamp(size << 7, 0x0C00, 0x1400)` and the enemy stager `FUN_800513F0` writes `actor+0x58 = size << 5`. Spans `14..=48` across the roster with no zero and no outlier, and it tracks model bulk rather than any stat - Lapis is 64800 HP at size class `20` against Koru's `48`, so a byte tracking HP could not produce the column. Parser: `MonsterRecord::size_class`. |
| `+0x20` | u8 | **Double-width texture page** flag, `0` or `1`. Read record-direct through `0x801C9348` twice over. Its primary reader is the monster model upload `0x801F1D0C` -> `FUN_80055468`, where a set byte widens the VRAM rect from `0x20` to `0x40` halfwords (`0x800554E0..0x800554F4`). Three slot-B summon ticks - PROT 0907 (Nighto), 0908 (Zenoir), 0916 (Aluru) - **also** read it, as a resist gate under the scripted-fight flag `ctx[+0x287]`; see [the instant-death gate](#the-instant-death--status-resist-gate-record-0x20) below. Set on 37 of 186 records. Parser: `MonsterRecord::wide_texture_page`; engine mirror `MonsterDef::wide_texture_page`, which feeds the Nighto roll's resist input. |
| `+0x21` | u8[3] | **Magic-attack ids** (`+0x21..+0x23`): up to three **global** spell ids the enemy casts. A slot is live when its value is `> 1`. The AI spell picker `FUN_801E9FD4` (`overlay_0898`) reads `record[0x21 + slot]`, writes it into the live actor at `+0x1DF`, and the battle-action SM names it via `&DAT_800754D0 + id*0xC` (`0x27` → `Tail Fire`). These global ids are **distinct** from the local `+0x4C` entry ids (which only gate the AGL cost); they are the names that appear on screen. Parser: `MonsterRecord::magic_attacks` + `legaia_asset::spell_names`. |
| `+0x3E` | u8 | **Seru id** (`0` = not capturable). Read record-direct through `0x801C9348` by the [killing-blow capture roll](battle-round-loop.md#the-retail-capture-roll-fun_801ec3e4); on success it is written to battle ctx `+0x269`, and the granted spell is global id `seru_id + 0x80` (Gimard's `1` → `0x81`). 63 records carry Seru ids `0x01..=0x15`. Parser: `MonsterRecord::seru_id`. |
| `+0x3F` | u8 | **Seru catch chance** in percent (`rand() % 100 < pct`); rolled only when the blow kills and `+0x3E` is nonzero. Retail spans `1..=80`. Parser: `MonsterRecord::catch_rate_pct`. |
| `+0x44` | u16 | **gold** (base victory-spoils gold). |
| `+0x46` | u16 | **EXP** (base victory-spoils experience). |
| `+0x48` | u8 | **drop item id** (`0` = no drop). |
| `+0x49` | u8 | **drop chance** in percent (`rand() % 100 < pct`). |
| `+0x4A` | u8 | Magic-slot count. |
| `+0x4C` | u32[] | Spell-entry offsets (count at `+0x4A`; block-relative, fixed to pointers at load). Each entry's first byte is a **spell/action id**: ids `2,3,4,5,0x0B` are elemental resist/affinity markers (`FUN_80054CB0` writes the slot index into actor `+0x1EF..+0x1F3`); ids `0x0C..0x1F` are offensive castable spells; `0x23` is special. Entry `+0x74` is the **AGL (action) cost**. See [battle-formulas.md → spell list](battle-formulas.md#spell-list-record-0x4c). |

All six stat names match the game's own labels + the fan bestiaries, cross-checked against the runtime consumer of each actor slot - see [battle-formulas.md](battle-formulas.md#actor-stat-block--monster-record-mapping). The parser exposes them via `legaia_asset::monster_archive::MonsterRecord::{attack, defense_high, defense_low, intelligence, speed, agility}`.

**Battle-load stat boost.** The record bytes are *not* what the player fights. After copying the record into the actor, `FUN_80054CB0` **boosts** four combat stats, choosing one of two profiles by the battle-context flag `_DAT_8007bd24 + 0x287` (= `(*(u8*)0x8007BD60 >> 5) & 4`, bit 7 of a per-battle flags byte set by `FUN_800513F0`):

| stat | gate-set profile (B) | gate-clear profile (A) |
|---|---|---|
| **ATK** (`+0x12`) | `+= ATK>>2` (×5/4) | unchanged |
| **UDF** (`+0x14`) | `× 2` | `+= (UDF>>1)+(UDF>>2)` (×7/4) |
| **LDF** (`+0x16`) | `× 2` | `+= (LDF>>1)+(LDF>>2)` (×7/4) |
| **INT** (`+0x18`) | `+= INT>>3` (×9/8) | `+= INT>>2` (×5/4) |
| HP / MP / AGL / SPD | unchanged | unchanged |

Both profiles boost; only the magnitude differs, so the raw record always understates
the fight - but **which profile runs is the fight class**, not the region. Within the NTSC-U build, that is - the PAL executables carry **no** boost at all ([below](#no-boost-on-the-pal-executables)). `ctx[+0x287]`
is the scripted-fight flag (bit `0x80` of `DAT_8007BD60`, raised for a formation row
with a non-zero header byte -
[`encounter.md`](../formats/encounter.md#the-per-battle-flags-byte-dat_8007bd60)), and
both branches are save-state pinned: every boss capture (Gaza Sim-Seru id 166: raw `[AGL
128, ATK 288, UDF 222, LDF 200, INT 220, SPD 146]` → in-battle `ATK 360, UDF 444, LDF
400, INT 247`; Cort likewise) carries `+0x287 == 4` and profile **B**, and every
random-encounter capture (a world-map Gobu Gobu: raw `ATK 17, UDF 15, LDF 14, INT 10` →
in-battle `17, 25, 24, 12`) carries `0` and profile **A**.
`MonsterRecord::battle_stats()` returns profile B, `battle_stats_random()` profile A,
`battle_stats_for(scripted)` picks. The curated `enemies.toml` bestiary holds profile B
for every enemy - the boss-fight numbers, which overstate a random encounter's UDF/LDF
by 8/7 and its ATK by 5/4. The earlier reading that profile B is *the*
international-retail profile for every fight rested on the Gaza capture alone; the
cross-region difficulty difference itself (international retail hitting harder than the
raw record / the Japanese release) was first surfaced by **Zetopheonix**. The same flag
gates which Seru-magic side-effect debuffs can ever land on the enemy - see
[battle-formulas.md](battle-formulas.md#seru-magic-side-effects---the-element-debuffs-fun_801f3d3c--the-finisher-switch).

The **engine port installs the profile the fight's class selects**: battle entry seeds ATK / UDF / LDF / INT through `MonsterDef::installed_stats(scripted)` (`engine-battle::monster_catalog`) - the boss profile for a scripted fight, the random-encounter profile (`x7/4` defence, unboosted ATK) for every rollable one - and AGL / SPD / HP / MP from the plain record fields, matching which stores the boost block does and does not touch. The accuracy / evasion bytes clamp the *boosted* INT, because the actor halfword the interrupt roll reads (`+0x168`) is the one the boost block's last store writes. Seeding from the raw accessors instead - which the port did - makes every enemy in the game materially weaker than retail.

Battle entry also seeds **both defence facets** into `World::battle.defense_split`, not one collapsed `max(UDF, LDF)` scalar. The melee kernel picks UDF or LDF by the swing's command parity (`FUN_801EC3E4` at `0x801ECE14`), so a single scalar leaves that branch dead for the whole monster band and makes every enemy defend with its better half against every swing. A Defense buff moves both halves together, as retail's "Defense Up" does.


#### No boost on the PAL executables

The boost is specific to `SCUS_942.54`. In the JP original (`SCPS_100.59`) and
all three PAL executables (`SCES_019.44` / `.45` / `.46`) the record copy into the actor (`+0x14C..+0x16A`,
the same store sequence as `0x8005516C..0x8005520C`) is followed **directly** by
the `+0x4A` spell-list loop - no `ctx[+0x287]` test, no shift-add block for
either profile; a byte search for the boss-profile arm (`lhu 0x12(s4); lhu
0x15A(a0); srl 2`) and for the switch load (`lbu 0x287`) finds neither in any
PAL or JP image. `SCES_019.45`: copy at `0x80055FC0..0x80056060`, spell loop
from `0x8005607C`; `SCPS_100.59`: copy at `0x80056EE0..0x80056FF8` (each stat
loaded twice - older codegen), byte clears at `0x80057004`, spell loop from
`0x80057014`. The monster records' stat and reward columns are byte-identical
across the five discs, so a JP or PAL fight uses the raw record: Zeto meets the party at
`ATK 108 / UDF 95 / LDF 76 / INT 117` on PAL and at `135 / 190 / 152 / 131` on
the USA disc. Walkthrough bestiaries that print `108 / 165 / 133 / 146` for the
same boss are showing the NTSC-U **random-encounter** profile (A) applied to
the record - not a profile any Zeto fight installs, since his formation row
carries the boss switch. Note for the JP archive: `MonsterRecord::decode_all`
reports zero populated slots because the name field is not ASCII, while
`--dump-block --id N` decodes the slot and shows the same stat / reward head as
USA (only the mesh offset at `+0x04` moves with the name length). The reward
side of the same regional split is in
[battle-formulas.md](battle-formulas.md#regional-difference---the-pal-executables-pay-more).

### The instant-death / status-resist gate (record `+0x20`)

Three slot-B summon ticks share one gate, byte for byte:

```text
801F6BF0  lbu  v0,0x287(a1)          ; the scripted-fight flag
801F6BF8  beqz v0, <roll>
801F6C00  v1 = 0x801C9348
801F6C04  v0 = victim_seat - 3
801F6C10  v0 = [0x801C9348 + (seat-3)*4]  ; the monster RECORD pointer
801F6C18  lbu  v0,0x20(v0)
801F6C20  bnez v0, <resist>
```

PROT 0907 (Nighto) at `0x801F6BF0` / `0x801F6C18`, PROT 0908 (Zenoir) at
`0x801F81F0` / `0x801F8208`, PROT 0916 (Aluru) at `0x801F6D44` / `0x801F6D70`.
In PROT 0907 the resist arm sets the module word `0x801F853C`, which the
arm-13 fork reads to abandon both the instant-death and the confuse outcome.
PROT 0908 additionally tests the battle-phase byte `_DAT_8007BD0C` against
`0x4D` / `0xAD` / `0xAE`.

The sweep denominator: over 84 images with 113 materialisations of
`0x801C9348`, exactly three loads at `+0x20` follow one (the controls `+0x1F`
and `+0x3E` return 12 and 1 at their known sites), plus the model-upload site
in PROT 0898.

**This is not a dedicated immunity table.** `+0x20` is the texture-page width
flag above, and the summons reuse it as a "big model" proxy. The set is 37 of
186 records: every named boss plus the Evil Fly / Death Wings / Demon Fly
family. The separate negative that
[`battle-formulas.md`](battle-formulas.md#seru-magic-side-effects---the-element-debuffs-fun_801f3d3c--the-finisher-switch)
records - no per-monster immunity for the Seru-magic **stat debuffs** - is
about `+0x24..+0x43`, which is zero across the roster, and stands.

**Rewards (EXP / gold / drop)** are inline in the record head at `+0x44..+0x49` (*not* at `+0x04`, which is the effect/animation data above). The victory-spoils function `FUN_8004E568` reads them from the per-enemy **record-pointer table at `0x801C9348`** (the loader `FUN_800542C8` populates it, so the actor *does* retain its record there - that's why monster-init never needed to copy the reward fields):

- **gold** (`+0x44`, u16): summed `>> 1` across dead enemies, optionally `* 1.25` (a living party member with ability bit `0x10000`), then the total is halved. A lone enemy yields `floor((gold >> 1) / 2)` - Gimard `60` → `15`, confirmed by a runtime write-watchpoint on party gold (`0x8008459C`).
- **EXP** (`+0x46`, u16): summed `* 3/4`, then split evenly among living party members.
- **drop** (`+0x48` item id, `+0x49` chance %): per dead enemy, `rand() % 100 < chance` grants the item (id added to the win banner at actor `+0xA9` and to inventory via `FUN_800421D4`).

(`FUN_80026018` is **not** part of this commit path - it is the mode-24 **minigame exit / return-warp** handler, whose `_DAT_800845A4 += _DAT_80084440` commit is the **casino-coin** bank, not battle XP; no battle-path caller exists in the dump corpus. See [`script-vm.md § 0x3E WARP`](script-vm.md#0x3e-warp-mode-24-minigame-door-warp).) Drop *item names* cross-check against [`legaia-gamedata`](../reference/gamedata.md) (Gimard `+0x48`=119 @ 10% - drops Healing Leaf). The reward formula detail lives in [battle-formulas.md](battle-formulas.md#victory-spoils-rewards).

### Monster archive (PROT entry 867)

`FUN_800542C8` streams the records as **per-monster `0x14000`-byte LZS slots** at archive offset `(id-1)*0x14000` (the monster id is the global monster-table index, ~194 fixed slots). Each slot is `[u32 decompressed_size][Legaia LZS stream]`; the decoded block's head is the stat record above, with the name and spell-entry payloads at the block-relative offsets the loader fixes up.

The archive is **extraction PROT entry `0867_battle_data`** (the EXTENDED footprint - the 15.9 MB archive lives in the entry's trailing-gap sectors, not its small indexed payload). Retail-semantically it **is** the `monster_data` block: the define `monster_data 869` names extraction 867 under the raw-TOC −2 correction ([`cdname.md`](../formats/cdname.md#numbering-space)), and the loader index `0x365` = define-space 869 resolves there directly (the earlier "misleading `monster_data` stub at extraction 869" reading was the filename shift; extraction 869 is a `sound_data` VAB stream).

The shipped retail build takes the debug `FUN_8003E8A8(0x365)` PROT-index path (`_DAT_8007B8C2 != 0`); the alternate `data\battle\<name>` open via the `break 0x103` host trap (`FUN_800608F0`) is a build-time dev-host artifact with no matching ISO9660 file on the disc.

Pinned by a PCSX-Redux watchpoint during the Rim Elm scripted battles (`scripts/pcsx-redux/autorun_monster_record_source.lua`): the loader's relative seek `(id-1)*40` sectors + the `disc_read` CdlLOC resolve to PROT.DAT offset `0x38AF000` = entry 867, and three decoded records match the live actor stats byte-for-byte (Gimard id 10 = HP 99 / MP 20, Killer Bee id 62 = 288 / 288, Queen Bee id 63 = 888 / 888). town01's encounter formations resolve to the Rim Elm Mist-attack set (Gobu Gobu id 4, Green Slime 7, Gimard 10, Hornet 61, Killer Bee 62, Queen Bee 63, Tetsu 79 - Tetsu being the 999/999 tutorial sparring partner).

Parser: [`legaia_asset::monster_archive`](../../crates/asset/README.md) (`record(entry, id)` / `records(entry)`; CLI `asset monster-archive`). Engine bridge: `legaia_engine_core::monster_catalog::catalog_from_monster_archive`, merged into the catalog by `SceneHost::enter_field_scene` for the scene's encounter ids so triggered battles spawn real stats.

## Stat aggregator (`FUN_80042558`)

Per-frame helper that walks the 3 active party members (stride `0x414` - see [character record layout](#character-record-layout)) and:

1. Clamps each character's stat fields to a per-field ceiling. It is a **ladder, not one blanket `0x3E7`**: at `0x80042C0C..0x80042CE0` the caps are `+0x104` → `9999`, `+0x108` → `999`, `+0x10C` → `100`, `+0x110` → `280`, then `999` each for `+0x112/+0x114/+0x116/+0x118/+0x11A`. Only the maxima are capped; the paired currents are handled by the clamp triple that follows ([pair order ↓](#why-the-pair-order-is-max-cur)).
2. ORs the character's "active abilities" 16-byte block at `+0xF4..0x100` into a global 4×u32 bitmask at `0x80074358..0x80074368`. This is the "currently-active accessory effects" register read by every other game system.
3. For each character, calls `FUN_800432BC` / `FUN_80042DBC` to add/remove temporary spells per the active spell-slot layout at `+0x2B0`.

The 4-u32 global ability bitmask is what tells the renderer to draw "auto-counter" / "regen" / "magic up" indicators and what tells the battle dispatcher to apply post-hit effects. The read-side primitive is `FUN_800431D0(bit_id) -> bool` - `(&DAT_80074358)[bit_id >> 5] & (1 << (bit_id & 0x1F))`. It's a 6-instruction hot helper cited from most damage / status code paths (the action validator `FUN_8003FB10` does **not** call it - see [battle-action.md](battle-action-queue.md#action-validator-fun_8003fb10)), ported as `World::party_has_ability(index)` against `World::party.party_ability_mask`.

`FUN_800349EC` and `FUN_80035EA8` are the HP / MP threshold UI classifiers - given a character index they compare current vs max and return one of `2` (dead/zero) / `6` (low) / `7` (warn) / `9` (healthy). The dialog renderer keys text colour on the result.

`FUN_8003FB10` is the **per-slot target-validity walker** that decides which slots a queued action may target. It dispatches the arm byte through an 18-arm jump table (bound `0x84`); each arm tests per-slot HP/MP quads (battle-actor table `DAT_801C9370` in battle, char records `0x80084708 + n*0x414` in field), record stats, party-slot indirection, system flags (`FUN_8003CE64`), or the inventory-count leaf `FUN_80046898`, writing per-slot validity bits. It does **not** consult the ability bitmask (`FUN_800431D0`) - see [battle-action.md](battle-action-queue.md#action-validator-fun_8003fb10) for the full arm map and the engine port (`engine-vm::battle_action::validate_action`).

## Battle archive (`FUN_80052FA0` / `FUN_800542C8`)

Two SCUS-side archive loaders feed the battle state. Their record-walk helpers:

- `FUN_800536BC` - copies records of stride `0x1C` from the archive into runtime layout, applying delta fixups to 6 of the 7 u32 fields (offset → absolute pointer pattern: `record[+0x18..0x30]`).
- `FUN_80053898` - bubble-sort over the 7-u32-stride records keyed on parallel byte arrays.
- `FUN_80053B9C` - copies short-array records into the per-slot UI buffer at `iVar1 + 0x894 + slot*0x1E0`, OR-ing `0x8000` into each entry (the "active" flag).

Both archive loaders interact with the battle character / monster slots via the 8-actor table at `0x801C9370`.

### The battle heap budget - why a formation of large distinct bosses cannot load

Everything the battle loader places in RAM comes from one custom heap, and its arithmetic is what bounds a formation - not VRAM (each battle seat owns its own texture-page column at `(320 + slot*64, 256)` / CLUT row `484 + slot`, so distinct enemies never contend), and not the AI (a load failure freezes the machine before any AI runs; retail's own `[161,161,161]` scripted fight proves multi-instance AI works).

**The heap.** `FUN_8002B3D4(pool_count=2, DAT_8007B414, size)` initialises a
best-fit free-list heap with 12-byte node headers, one shared free ring and a
per-pool allocated ring (so a pool can be mass-freed). The stage-init path
(`FUN_8001E1B4`) sizes it `0x134800` (~1.23 MB), arena
`0x80091800..0x801C6000`. With `gp = 0x8007B318`: descriptor pointer
`gp+0x840 = 0x8007BB58`, alloc counter `gp+0x488 = 0x8007B7A0`, malloc-error
accumulator `gp+0x510 = 0x8007B828`. The malloc wrapper
`FUN_80017888(pool, size)` → `FUN_8002B468` **returns NULL on exhaustion**
(dev-console `malloc err size %d`); the monster streamer `FUN_800542C8` stores
and copies through that pointer **unchecked**, so an over-budget formation
writes the decoded block over low kernel RAM and the machine locks inside the
mode-`0x15` tick (vsync stops - an emulator-side observer sees the mode byte
parked at `0x15` forever).

**The per-monster RAM cost is `block[+0x08]` bytes** (the texture-pool offset): stats + name + TMD + all action entries and animation streams. The texture pool itself never enters the heap - it is decoded into the staging area at `DAT_8007B728 + 0x12800` (inside the GPU packet buffer) and uploaded to VRAM from there (`FUN_80055468`). The loader dedupes by id: only the first occurrence of an id in the formation cells `DAT_8007BD0C[0..3]` streams and allocates; duplicate seats share the record and mesh, which is why instanced trios are nearly free.

**The measured ledger** (allocator breakpoint trace over a forced battle load
from a town scene; every row `FUN_80017888(0, size)`): scene asset buffer
`0x62C00`, GPU primitive-packet double buffer `0x64000` (`FUN_8001E3B8`,
`packet_size 0x32000 << 1`), the enemy stager's fixed working buffer `0x2E390`
(`FUN_800513F0` @`0x80051740`, stored `gp+0xa5c = 0x8007BD74` - **fixed-size,
party-count-independent**; shrinking the party roster `DAT_8007BD10` does not
reclaim it), battle ctx `0x7A34` (`FUN_80055B6C`), a transient `0x19000`
party-mesh decode temp (`FUN_80052FA0`, freed in-loop), sound streaming
`0x1014` chunks, then one allocation per distinct monster, then a post-monster
tail (`0x1800` + small nodes).

What remains at the first monster allocation in that context is ~`0x28230`
(164.4 KB); the measured post-monster tail (`0x1800` + `0x3100` + small
nodes) is ~19.5 KB, so the workable distinct-monster budget is **~145 KB**.
Probe-bracketed: `[162,10]` (123.3 KB of monster blocks) loads and runs with
18.5 KB free; `[162,79]` (152.2 KB) seats both monsters but dies on the
`0x3100` tail alloc; `[162,163]` (165.2 KB) dies on the second monster
itself. Retail's own authoring respects the budget: the largest distinct-id
formation on the disc costs 124.3 KB of heap ([108,3] / [107,2] in the Drake
kingdom bundle), and no retail formation exceeds two distinct ids. The three
Delilas blocks cost `0x15030`/`0x144D0`/`0x147E8` (84.0/81.2/82.0 KB) each -
any two together (163-166 KB) overshoot by ~20-25 KB, which is the entire
reason the Delilas Challenge dome course fields them one per round.
Instrument: `scripts/pcsx-redux/autorun_delilas_battle_load.lua` (formation
install + allocator breakpoints + free-ring walk); offline sibling
`scripts/asset-investigation/battle-heap-walk.py` (free + allocated rings
from a save-state RAM extract).

### The species-order rebuild - why 2 distinct species is an engine invariant

Before any monster streams, the battle setup `FUN_80055B6C` (loop at
`0x80055C80..0x80055D2C`) classifies the formation cells `DAT_8007BD0C[0..3]`
into "the first species" (`cells[0]`, with a copy count in `s1`) and "the
other species" - held in a **single register** (`s3`, with a count in `s0`) -
then, behind a 50% coin flip (`FUN_80056798() & 1`), rebuilds the cell array
as `[other x s0, first x s1]`: the species-order variety shuffle that makes
the same authored formation open with either species in front. For retail's
authoring - never more than two distinct species per formation - the rebuild
is an exact multiset-preserving swap.

With **three** distinct species the single `s3` register is overwritten by
each later species, so `[a, b, c]` rebuilds as `[c, c, a]`: the middle
species silently vanishes and the last is duplicated. On the other half of
the flip the cells load verbatim and all three distinct blocks stream - which
is what turns an over-budget trio into a *probabilistic* battle-load hang.
Pinned live (write watchpoints at `0x80055D14/18` + cell readback at battle
main, `autorun_formation_cell_writers.lua`): forced installs of
`[133,151,94]`, `[94,133,151]`, `[151,133,94]` (map03 context) and
`[32,34,14]` (rikuroa context) each read back `[c2, c2, c0]`, with seat 1's
record pointer sharing seat 0's block; with the flip forced to the verbatim
side, `[133,151,94]` (180.0 KB of blocks) ran the heap to **0 bytes free**
with the record table overwritten by non-pointers, and `[14,150,93]`
(177.2 KB) and `[162,163]` (169.2 KB, two distinct - no rebuild involved)
each crashed the machine with decoded-block bytes over the exception vector
at `0x80000080` (the "Deli[las]" name string was the faulting instruction
word). Passing brackets in the same context: 146.3 KB with 17 KB free,
137.6 KB with 6 KB free.

Both limits together are why the encounter randomizer's unconditional
**battle-load safety pass** (`legaia_patcher::encounter::
SceneEncounters::enforce_species_limits`) caps every random formation at 2
distinct species and at the disc's own authored heap-cost maximum - see
[`randomizer.md`](../tooling/randomizer.md).

## Character record layout

Stride `0x414` bytes per character, base `0x80084708` (so character `n` lives at `0x80084708 + n*0x414`). Surfaced by the inventory/spell helpers (`FUN_80042558`, `FUN_80042DBC`, `FUN_800432BC`, `FUN_800431FC`, `FUN_80043264`):

| Offset | Use |
|---|---|
| `+0x08..+0x98` | u32 per-spell counter array (stride 4), maintained in lockstep with the two byte arrays below. See [the three parallel spell arrays](#the-three-parallel-spell-arrays). |
| `+0x13C` | u8 spell-list count. |
| `+0x13D..+0x160` | u8 spell IDs (variable-length; up to 36). |
| `+0x161..+0x184` | u8 per-spell **level / rank** (one byte per entry, same index as `+0x13D`). Floored to `1` when a spell is learned; magic-rank up writes `+1` here. |
| `+0x196..+0x19D` | u8 equipment slot bytes (8 slots; weapon, armour, accessories). |
| `+0x2A7..+0x2B0` | NUL-padded ASCII display name (`Vahn`/`Noa`/`Gala`/`Terra`/player-entered lead), 9 bytes bounded by the active-spell table at `+0x2B0`. Pinned across six in-game RAM captures for all four roster slots. In the retail SC save block this lands at `game+0x66F + n*0x414` (SC `+0x86F` for slot 0); see [`save-screen.md`](save-screen.md). Accessor `legaia_save::CharacterRecord::name` (`NAME_OFFSET`). |
| `+0x2B0..+0x37F` | Active spell-slot array (stride `0x14`, up to N entries). Populated by `FUN_80042DBC` from the spell list. |
| `+0xF4..0x100` | "Active abilities" 16-byte block - OR'd into the global 4×u32 bitmask at `0x80074358..0x80074368` by `FUN_80042558`. |
| `+0x104..0x110` | HP / MP / AP `(max, cur)` u16 pairs - `+0x104/+0x108/+0x10C` effective maxima, `+0x106/+0x10A/+0x10E` currents ([pair order ↓](#why-the-pair-order-is-max-cur)); AP = the arts / action-point gauge, its max sized by AGL - the AGL stat itself is the adjacent "Max AGL" field at `+0x110`/`+0x122`, see [save-record.md](../formats/save-record.md)). |
| `+0x10E` | u8 - written on level-up (delta `+8` for Vahn slot in the captured pre→post pair): the live AP pair's current cell refilling to the raised max. |
| `+0x11A` | Stat-cap field (clamped to `0x3E7`). |
| `+0x11C..+0x122` | Six adjacent stat bytes (paired) - incremented by small deltas (`+1..+4`) on level-up. Likely the per-stat rank table consumed by the level-up apply path. |
| `+0x130` | u8 - the **displayed character level** (the byte the status screen reads as "LV"; the `Level 99` cheat target), incremented `+1` per level-up event. See [save-record.md](../formats/save-record.md#0x130-is-the-displayed-character-level). |

### The three parallel spell arrays

The character record carries a spell list as **three** arrays at the same index,
not two, and an earlier revision of the table above listed `+0x161..+0x184` twice
- once as a "spell-level / experience" array and once as a "spell-level" array.
Both rows described `+0x161` correctly as far as the *level* goes; the
"experience" half was real data attributed to the wrong offset.

`FUN_800432BC` (learn a spell - insert at the head of the list) settles it. It
shifts all three arrays up by one in the same loop at `0x80043338..0x80043370`,
then writes the new entry at index 0:

| Array | Stride | Shift loop | Insert store |
|---|---|---|---|
| `+0x13D` spell id | 1 | `lbu 0x13d` `0x80043344` → `sb 0x13d` `0x8004334C` | `sb t3,0x13d(t0)` at `0x80043378` |
| `+0x161` level | 1 | `lbu 0x161` `0x80043350` → `sb 0x161` `0x80043358` | `sb t1,0x161(t0)` at `0x8004337C` |
| `+0x08` counter | 4 | `lw 0x8` `0x80043364` → `sw 0x8` `0x80043370` | `sw t2,0x8(t0)` at `0x80043380` |

The count at `+0x13C` is incremented last (`0x80043384` / `0x8004338C`).

Two details separate the byte from the word. The level byte `t1` is read from the
source spell-slot at `+0x2B5` and **floored to a minimum of 1** (`bne t1,zero` at
`0x8004331C`, `addiu t1,t1,0x1` at `0x80043324`) - a rank starts at 1, which is
level semantics and not counter semantics. The u32 `t2` is *assembled* from four
separate bytes of that same slot, `+0x2B1..+0x2B4`
(`0x800432F8..0x8004331C`, shifted `<<24/<<16/<<8` and summed), which is the
shape of an accumulating counter and not of a 1-byte rank.

`FUN_80042DBC` moves the same data the other way, writing `+0x161` back out to
the slot byte `+0x2B5` (`lbu 0x161` at `0x80042E64` → `sb 0x2b5` at
`0x80042E6C`), and runs the mirror-image compaction loop at
`0x80042E84..0x80042E9C` when an entry is removed.

The captured magic-rank-up deltas agree independently: the same event moves
`+0x161` by `+1` (`0x02 → 0x03`, a rank) and `+0x08` by `+12`
(`0x30 → 0x3C`, an accumulation). The extent lines up too - 36 entries at stride
4 from `+0x08` ends at `+0x98`, immediately before the magic-rank counter at
`+0x9C`.

What the `+0x08` counter *counts* is Inferred, not Confirmed: the disassembly
pins its structure, lifetime and stride, and the capture pins one `+12` delta on
a rank-up, but no site was traced that consumes it to decide a threshold. The
"experience" reading is plausible and is the likeliest origin of the old row's
wording - it is recorded here as a lead, not as a decoded field.

### Why the pair order is `(max, cur)`

The decisive sequence is the clamp triple that closes the stat aggregator
`FUN_80042558` at `0x80042CE4..0x80042D34`. For each of the three pairs it loads
the low halfword, loads the high halfword, and writes the **low** one into the
**high** slot when the high slot is larger:

```
80042ce4  lhu  v1,0x104(s0)     ; max
80042ce8  lhu  v0,0x106(s0)     ; cur
80042cf0  sltu v0,v1,v0         ; max < cur ?
80042cfc  sh   v1,0x106(s0)     ; cur := max
```

Repeated verbatim for `0x108`/`0x10A` and `0x10C`/`0x10E`. A value that gets
clamped *down to* its neighbour is the current; the neighbour is the maximum.
Two more instruction-level corroborations sit either side of it: the hard caps
just above (`0x80042C0C..0x80042C50`) apply to `+0x104`, `+0x108`, `+0x10C`
only, at `9999` / `999` / `100` - a `100` ceiling on `+0x10C` is unambiguously
the AP *maximum* - and the walk-regen tick `FUN_801D0B90` (dialog overlay) bumps
`+0x106` by `8` and clamps it at `+0x104` (`0x801D0C00..0x801D0C20`), with the
same shape for MP and AP. Consumers: `legaia_save::HpMpSp`,
`engine-core::walk_regen`.

**Level-up captured deltas (Vahn, pre/post a single character-level event).** Diff captured via `mednafen-state` shows the per-character side-effects:

| Offset | Width | Pre → Post | Interpretation |
|---|---|---|---|
| `+0x00` | u8 | `0x4F` → `0x73` (79 → 115) | Possibly raw level byte / per-character XP-derived counter. |
| `+0x04..+0x06` | u16 LE | `0x016D` → `0x02DA` (365 → 730) | XP word delta (+365). Matches the published level-up XP curves. |
| `+0x10E` | u8 | `0x3A` → `0x42` (+8) | AP current (live pair `(max, cur)`; the +8 AP grant). |
| `+0x11C..+0x122` | 6× u8 | `67/1C/13/10/16/0B` → `6B/20/15/12/1A/0F` | Per-stat increments (`+4 +4 +2 +2 +4 +4`). |
| `+0x130` | u8 | `0x02` → `0x03` | Displayed character level (+1 - the level 2 → 3 event). |

Noa and Gala records are byte-identical across the same pair - the level-up event in this capture pair is for Vahn alone.

**Magic-rank up captured deltas (Vahn, pre/post a single magic-rank-up event).** Diff over the same record range surfaces a strict subset of the level-up footprint, focused on the spell-level table:

| Offset | Width | Pre → Post | Interpretation |
|---|---|---|---|
| `+0x08` | u32 | `0x30` → `0x3C` (+12) | `spell_counter[0]` - entry 0 of the per-spell u32 array, not a flag word ([why](#the-three-parallel-spell-arrays)). |
| `+0x9C` | u8 | `0x09` → `0x0A` (+1) | Magic-rank mirror. |
| `+0x10A` | u16 lo | `0x1B` → `0x11` (-10) | MP **current** (the `+0x108`/`+0x10A` pair) - the cast that earned the rank-up. Not a TBD field. |
| `+0x161` | u8 | `0x02` → `0x03` (+1) | Spell-level byte (`+0x161..+0x184` array). Confirms magic-rank up writes here. |

## Battle main dispatcher (`FUN_801D0748`)

11124 bytes / 2781 instructions. The top of the per-frame battle loop: it opens
by loading the battle context pointer `_DAT_8007BD24` and dispatching on the
**sub-state byte** at `ctx+6`, then routes through every active battle
subsystem (rendering, AI, animation, hit detection).

One body serves four game modes. The dumps taken from the battle-action,
magic-capture, magic-level-up and Muscle Dome captures - and the static
`overlay_0898` print - are **byte-identical across all 2781 instructions**, so
"the capture dispatcher", "the level-up tick" and "the dome match controller"
name the same routine reached in different modes, not three routines at one VA.
The dome's use of it is written up under
[`minigame-muscle-dome.md`](minigame-muscle-dome.md); the sub-states `0x1E` /
`0x32` / `0x6E` / `0xFE` update the camera yaw `_DAT_8007B792`.

## Hottest battle utility (`FUN_801D8DE8`)

3028 bytes / 757 instructions, 77 incoming refs - the single most-cited battle
helper, and it is the **HUD element renderer**: `(elem_id, mode, ...)` bounded
by `sltiu v0,v1,0x50` and dispatched through the 80-entry jump table at
`0x801CEB68`, one case per on-screen element. Not a per-actor utility. The
battle HUD and the Muscle Dome plate share it - per-`elem_id` breakdown in
[`minigame-muscle-dome.md`](minigame-muscle-dome.md#hud-elements-fun_801d8de8)
and [`functions/battle.md`](../reference/functions/battle.md). The tiny 3- and
4-instruction bodies at this VA in the fishing / dance / slot-machine /
debug-menu / Baka Fighter images are a different overlay's occupant.

## Per-frame actor maintenance (`FUN_8004CE2C`)

The SCUS-resident per-frame sweep over the battle actor table - one of the
largest SCUS functions with no static caller (it is reached from the battle
tick). Three sequential passes over `DAT_801C9370`, bounded by the actor count
byte `*(_DAT_8007BD24)[0]`:

1. **Status-flag reconcile.** For each actor, walks the element/condition word
   in the `0x80084140`-region record and clears matching condition bits in the
   actor's status halfword at `+0x16E` (masks `0x0001`/`0x0003`/`0x0078`/
   `0x1000`/`0x0004`/`0x0400`), i.e. "expire conditional status effects".
2. **Per-clip impact arms.** Resolves the acting actor's committed record's
   `+0x77` clip-identity byte (the `attach_key` slot of
   [`battle-data-pack.md`](../formats/battle-data-pack.md)) and its anim
   cursor, dispatches on the roster character id, and on hand-picked
   (clip, cursor-window) pairs writes the impact-config words
   `_DAT_801F53D4` / `_DAT_801F53D8` into the **target's** `+0x04` tint and
   `+0x21F` selector. Gala's clip-`0x18` arm additionally **freezes the
   target's pose** (`+0x21D = 0`, cursor window `0x40..=0x80`; restored by
   `FUN_801E93C8`); Vahn's clip-`0x18` arm is tint-only (`0x90..=0xA0`).
   Every tint arm also stamps `+0x0C = 0x1000`. The rest of the pass, from
   `0x8004D01C` to `0x8004D32C`, is [tabulated below](#the-other-clip-tag-arms).
   Port: `engine-vm::battle_impact_fx` +
   `World::tick_battle_impact_fx`; the tint decays through the per-actor
   presentation SM `FUN_80050120` (arm 0: `FUN_80050F30` ease to neutral,
   then the `+0x0C` blend drains, then the `+0x21F` selector retires - port
   `engine-vm::battle_formulas::tint_sm_step`, driven by the same tick).
   The same triple is what a **landing hit** stamps on the struck actor:
   the melee / arts routine `FUN_801EC3E4` reads the acting record's
   `+0x7A` status / impact selector (`0x801EE3D4..0x801EE43C`, the tint
   gated `0 < sel < 6` by `sltiu v0,v0,0x6` because selector `6` is the
   tint-less Curse arm at `0x801EE690`; every connecting swing reaches it -
   there is no exit ahead of the arm), the monster special-attack tick
   `FUN_801E09F8` reads the move-power record's `+0x0A` at each arm's
   impact phase (`0x801E15AC..0x801E15EC`, unguarded - that ladder ends at
   `5`). Port `World::arm_impact_tint`, called from
   the basic-strike kernel, the `ApplyArtStrike` fold and the enemy
   status-proc arm; the class rides the clip as
   `MonsterAnimation::impact_class`. How the words reach the pixel is in
   [tint pass and draw pass](battle-actor-rendering.md#how-the-tint-words-reach-the-pixel).
   See [the other clip-tag arms](#the-other-clip-tag-arms) for the table.
3. **Per-encounter boss hooks.** Gated on `DAT_8007BD0C` - the **monster /
   formation id**, not a sequence sub-phase byte, and `0x8A`/`0xA7`/`0xAA`/`0xB4`
   (138/167/170/180) are **boss ids**, not phase bands. Each arm applies
   hand-written camera / pose / scale overrides to the first monster actor:
   the `0x51EB851F` magic multiply is a fixed-point **÷50** (the spirit value is
   clamped to 50 first), and `0x1F80 - frame*0x12` is a triangular angle ramp
   written to `+0x1BA`, **not** a gauge bar width and **not** a hardware
   register.
4. **CLUT status recolour.** For actors with status bit `0x04` (Stone, latched
   via `+0x220`) or bits `0x08`/`0x10`/`0x20` (latched via `+0x221..+0x223`),
   it recolours the actor's **240-entry palette row** - not its texels - staging
   through `ctx+0xE34` and uploading a `1`-pixel-tall rect, so each actor owns
   VRAM CLUT row `481 + slot`. Stone averages the three BGR555 channels
   (`l = (r+g+b) >> 2`, clamped to 31) into a grey; the other three build the
   same luminance plus `b = (l*3) >> 1` and set the STP bit, giving a blue
   tint over a per-character index window from the 3-pair table at
   `DAT_80078630` (stride 6). This is status tinting latched once per
   affliction, not a per-frame damage flash. The desaturate step is the
   reusable arithmetic core; it is ported (with tests) as
   `legaia_engine_vm::scus_battle_helpers::bgr555_to_grey`, while the packet
   build (`_DAT_1F8003A0` OT, `FUN_800583C8` submit) stays render-track.

   The `0x894` window is exactly `3 * 0x1E0` bytes wide before the staging
   buffer at `0xE34` begins, so the palette source covers the **three party
   slots** and no monster: rows `481..=483` are the party's (the monster CLUT
   rows start at `484`).

   **Port.** `engine-core::battle_status_clut::StatusClutState` holds the
   engine's equivalents of the three things retail reads here - the per-actor
   palette copy, the `+0x220` latch and the staged row. The latch is armed
   from `BattleHud::sync_status` on the Stone edge; the pass runs against the
   host's battle VRAM, greys the pristine copy through `bgr555_to_grey` and
   rewrites row `481 + slot`. The copy is snapshotted off that same VRAM row
   rather than off the disc palette, which is exact rather than approximate:
   the two forms differ only in bit 15 (the loader's `FUN_80053B9C` STP-set),
   and the desaturate masks bit 15 off. Keeping the copy is what makes a
   second fire re-grey the original instead of compounding, exactly as retail
   does by never writing `ctx[+0x894]`.

   One part of the pass stays out of the port: the Rot arm's per-character
   index window (`DAT_80078630`) has no parser in any crate, so only the
   Stone arm is ported. The recolour **is** reachable in play - the
   monster-side source is `World::apply_enemy_agl_status`, the port of
   `FUN_800402F4`'s class-9 / class-10 arms (see
   [battle-formulas.md](battle-formulas.md#status-application-the-art--move-record-status-byte)),
   which the monster-cast fold calls and which lands the `+0x16E` bit on a
   party seat.

### The other clip-tag arms

Pass 2 keys every arm on the acting actor's committed record `+0x77` and the
anim-player node's cursor `+0x68` (sixteenths of a keyframe), and writes the
target named by the acting actor's `+0x1DD`. The whole pass, from the
disassembly (`see ghidra/scripts/funcs/8004ce2c.txt`):

| Who acts | Tag | Cursor | Writes |
|---|---|---|---|
| Gala | `0x16` | `>= 0x20` | target tint, entry 1, selector `2`, blend `0x1000` |
| Gala | `0x17` | `>= 0x40` | the same tint |
| Gala | `0x18` | `0x40..=0x80` | the same tint plus the pose freeze; acting `+0x21F = 2` on the tag alone |
| Gala | `0x67` | `0xB0..=0xF0` | the same tint plus `FUN_801E1D98(&target[+0x3C], 0xC)` |
| Vahn | `0x18` | `0x90..=0xA0` | target tint, entry 0, selector `1` |
| Vahn | `0x2B` | any | acting `+0x21C = 3` below `0x51`, `0` from there |
| Noa | `0x29` / `0x2D` | any | target `+0x16E` takes bits `0x380`, gated below |
| a monster | `0x3B` | any | acting `+0x21C = 3`, target `4` while acting `+0x21B == 0x13`; both `0` when it reads `0` |

The two Gala tint-only arms are open-ended: `slti v0,v0,0x20` / `0x40` at
`0x8004D14C` / `0x8004D168` gate the start and nothing gates the end. Noa's
arm needs a landed hit (acting `+0x1F4 != 0`), an ordinary fight
(`ctx[+0x287] == 0`), an even `rand()` (`0x8004D0EC`) and a first monster
other than `0xA7` - the byte it reads is `gp+0x9F4`, which with
`gp = 0x8007B318` is the formation cell `0x8007BD0C`. `+0x21C` is the
presentation arm the tint SM `FUN_80050120` dispatches on.

Every row is ported in `engine-vm::battle_impact_fx` and applied by
`World::tick_battle_impact_fx`. The tag-`0x67` ribbon's `FUN_801E1D98`
call is surfaced as `ClipImpactWrite::effect_at_target`, staged on
`World::battle.clip_ribbon` and drawn on both hosts by `engine-ui::streak_pass::clip_ribbon_quads`.

Calls the actor-spawn/move-VM invoker `FUN_80021B04` and helpers
`FUN_8004FE5C` / `FUN_800583C8` / `FUN_80031D00` / RNG `FUN_80056798`.
Despite its size and shape it is **not a mode dispatcher**: the master mode
word `_DAT_8007B83C` never appears; every global it touches is battle-domain.
`see ghidra/scripts/funcs/8004ce2c.txt` (`0x8004CE30` is the function's second instruction, not its entry).

## Battle scene-init residency window

A separate `map01` save pair (one frame with the encounter armed but
battle not yet entered, the next frame with battle just initiated)
pins the **post-load residency window** of the battle scene-init
pipeline. Distinct from the encounter-trigger overlay swap above; this
pair brackets the loader function with concrete RAM-resident artefacts
the loader writes into.

| Range | Bytes changed | What it is |
|---|---:|---|
| `0x80124690..0x801503C4` | ~168 KB | Battle-bundle residency window. Pre-battle holds field-scene payload (sample dialog text strings visible); post-battle holds battle-bundle data (vertex / TIM / actor records). Codified as `BATTLE_BUNDLE_WINDOW`. |
| `0x801CE808..0x801D3018` | ~16 KB | Battle-overlay scratch slice. Wholesale reset on entry; distinct from the broader encounter-trigger overlay residency at `0x801CE800..0x801F4000`. Codified as `OVERLAY_SCRATCH_WINDOW`. |
| `0x800836C8` | 4 B | Per-frame actor-tick fn-pointer slot in the bundle-pool extension. Pre-battle reads `0x80024C50`; post-battle reads `0xF41D0280` = `FUN_80021DF4`. Codified as `ACTOR_TICK_FN_PTR_ADDR` / `ACTOR_TICK_FN_PTR_VALUE`. |
| `0x801FFCA0..0x801FFFFE` | ~600 B | CD I/O state slice. Rewires while the battle bundle is paged in; reliable "battle scene-init in flight" signature. |

The pair is **post-load** by design - both save frames resolve to a
state where the loader function has already returned. The loader
function (which reads PROT entry `0x05C4` + sibling Seru blobs and
populates the battle bundle) lives in an overlay slice that is not
directly visible in either snapshot. Pinning it requires a
mid-execution capture between the field→battle game-mode flip and
this residency state, which the current Mednafen workflow can't
generate without manual frame-stepping (mednafen 1.29 has no headless
mode).

Codified as constants in
[`engine_core::capture_observations::battle_init_overlay`](../../crates/engine-system/src/capture_observations.rs);
disc-gated test
`battle_init_overlay_pair_pins_battle_bundle_window_and_actor_tick_wiring`
in `crates/mednafen/tests/real_saves.rs`.

## Item-use battle-event residency

A mid-battle save pair (battle just initiated; party member about to
use a Healing Leaf) pins the **item-use sub-mode residency**:

| Address | Pre / Post | Notes |
|---|---|---|
| `_DAT_8007B8D0` | `0x8014BD30 → 0x800ABA4C` | Field-pack base pointer flips. The item-use sub-mode reseats the active scene asset buffer. |
| `0x801BA7DC..0x801BADEC` | ~660 B shift | Script-VM context block. The menu / item / target / commit pipeline rewrites the entire ctx region as it runs. |
| Actor pool slots 0..4 | per-frame motion deltas | 3 party + 2 monsters (count-2 formation). Slots 5..7 stay zero across the pair. |

The captured pair uses a **Healing Leaf** (consumable HP-restore) -
not Fire Book I (a spell-learn item). The pair therefore pins the
residency window of the item-use battle-event handler without lifting
the Fire Book-specific writer to the displayed-skills array at
`+0x185`. A second save pair specifically capturing Fire Book I use
is required to lift that writer.

Codified as constants in
[`engine_core::capture_observations::item_use_battle_event`](../../crates/engine-system/src/capture_observations.rs);
disc-gated test
`item_use_pair_pins_field_pack_base_flip_and_script_vm_ctx_shift`
in `crates/mednafen/tests/real_saves.rs`.

## Additional SCUS battle-band helpers

Small `SCUS_942.54` routines the battle tick and scene-init reach through the
actor / mode tables (no static caller). Roles are read off the stores in each
bare-hex dump under `ghidra/scripts/funcs/`; where a purpose is inferred it is
stated by the concrete writes.

| Function | Role |
|---|---|
| `FUN_80055B6C` | Battle scene initializer: clears the actor/effect pools, resolves the party-slot composition (dedup + fill from `DAT_8007BD0C..`), sizes the LZS scratch, allocates the `0x7A34`-word monster-object arena at `_DAT_801C9370`, and programs the disp/draw environment. |
| `FUN_80055B20` | Seeds the fallback party-slot id table `DAT_8007BD10 = {1, 2, 3}` (Vahn/Noa/Gala); `FUN_80055B6C` overwrites it from the live party. Slot bytes index character records as `(id-1)*0x414`. |
| `FUN_80054A6C` | Battle party-file loader: builds the `data\battle\` filename (`s_data_battle_800153B8`), then streams each live party member's player battle file keyed on the party-id table `DAT_8007BD0C` at file stride `(id-1)*0x14000`. Dual-mode on `_DAT_8007B8C2`: retail ISO9660 (`FUN_800608F0`/`FUN_80060920`/`FUN_80060944` async CD reads) vs dev PROT-TOC (`FUN_8003E8A8`/`FUN_8003E964`/`FUN_8003E800`, entry `0x365`); bumps the loaded-count `DAT_8007B649`. CD/loader I/O infra: scope row in `asset_load_plumbing` - the port streams the same four files through `SceneAssets`, from the disc image, with no drive command sequence. |
| `FUN_800480D8` | Per-actor battle draw tick, called by the render dispatcher's mode-2 arm on bodies at view depth `>= 0xA1`. The first body each frame runs the battle's per-frame global passes (effect-VM walker `FUN_801E0080`, cast census, damage popup, effect-node sweep) off the latch `ctx[+0x272]` the frame driver `FUN_80046A20` raises; then the tint pass and the zero-colour / lone-monster grey gate decide whether and how the body draws. Ported `engine-vm::battle_actor_tick`, live - [details](battle-actor-rendering.md#the-distance-fade). |
| `FUN_8004A908` | Battle-actor tint pass: writes the colour word `+0x74` and blend weight `+0x78` from the body's view depth against half its radius (the distance fade), with the `+0x16E` status colours, the outdoor-stage invert on `DAT_8007BDA8` and the cursor-dim arm. Ported whole as `engine-vm::battle_actor_tint`, live on both hosts; capture-matched 258 / 266 - [details](battle-actor-rendering.md#the-distance-fade). |
| `FUN_80046A20` | **Not a small helper** - this is the battle-scene per-frame tick (2576 bytes, 644 instructions), listed here only because the rows below are the routines it drives. It calls the scene loader `FUN_800520F0`, the seat stager `FUN_800513F0`, the party-file loader `FUN_80054A6C`, the main dispatcher `FUN_801D0748`, the action SM `FUN_801E295C`, the separation driver `FUN_80051078` and the actor-presentation tick `FUN_80050120`. Its one self-contained kernel is the HP/MP gauge-fill colour selector keyed on `+0x172`/`+0x174` vs `+0x14E>>1`/`>>2` and the status word `+0x16E`, ported as `battle_gauge::gauge_colors`. Full row in [`functions/battle.md`](../reference/functions/battle.md). |
| `FUN_8004DC68` | Near-camera ghost pass: sets / clears the `+0x8` mode bits `0x83000000` (semi-transparent, blend `3`) on bodies within `dist / 4` of the camera's view point, and on a caster's allies during a magic cast. Ported as `engine-vm::battle_action::camera_ghost_pass` - [details](battle-actor-rendering.md#the-near-camera-ghost-pass-fun_8004dc68). |
| `FUN_8004C650` | **Move-name** banner placement (placement records 76/77 - captured as the art name, e.g. `Poisonous Sting`, at `(117, 148)`; not the enemy-name banner, which `FUN_801D9D3C` composes): measures a name string width (`FUN_80035F04`) and centres its four banner X coords around `0xA0`, with `0xCF`/`0xC1` leading-byte nudges. |
| `FUN_8004CCD4` | Per-command display resolver (battle-data-pack): for each of the actor's up-to-2 command slots, tests a threshold value against the `+0xA4` range pairs and writes the matching `+0x1034` (hit) or `+0x1030` (fallback) display pointer into the caller's output table. |
| `FUN_80046978` | Screen-flash colour submit: when trigger `gp[0x9D4]` is set, scales stored colour `gp[0x9D0]` by scratch byte `0x1F800393` and submits via `FUN_80024EE4`. The per-channel saturating scale is ported as `scale_rgb24`; the trigger + submit stay caller-side. |
| `FUN_80050120` | Per-actor battle-presentation tick: walks the actor table `DAT_801C9370`, skips actors with no `+0x22C` sub-struct, and dispatches on the actor state byte `+0x21C` (11-entry jump table at `0x8001532C`). Arm 0 eases `+0x04` to neutral, then drains `+0x0C`, then clears `+0x21F`; arms `1`/`3`/`4`/`6..=10` ease toward fixed colours (dim / red / blue / magenta / soft red / green / yellow / white) with `+0x0C = 0x1000`; arm 2 is the defeat / capture fade to black. Ported as `engine-vm::battle_formulas::tint_sm_step` (arm table in its module docs), driven per frame by `World::tick_battle_impact_fx`. |
| `FUN_80050F30` | 3×10-bit packed approach-to-target step: eases each 10-bit channel of a packed `u32` toward an 8-bit target (widened `<<2`) by at most `step_scale * DAT_1f800393 * 8` per call, clamping on the target without overshoot; only differing channels are rewritten (the byte-exact masking is why the top two bits survive an unchanged Z channel). A pure closed-form kernel with no table/hardware dependency; **ported** (with tests) as `battle_formulas::packed3_approach_target` / `approach_channel_clamped`. |
| `FUN_80050BB8` | Pairwise battle-actor separation (push-apart): reads two actors' body radii `+0x22C→+0x58` and positions `+0x3C`/`+0x40`, projects the between-actor distance onto the angle from `FUN_80019B28` via the sin/cos LUTs `_DAT_8007B81C`/`DAT_8007B7F8`, and if the projected gap is below `(r1+r2)/6` nudges both actors' **live** position pairs `+0x34`/`+0x38` apart by `sin/cos >> 10` (it measures the per-frame body pair `+0x3C`/`+0x40`, so a nudged overlap clears). Ported as a faithful fixed-point mirror in `engine-vm::battle_separation::push_apart` (trig samples lifted to caller parameters, no Sony table bytes); driven every live battle frame by `World::tick_battle_separation`, on the line after the action-SM step - retail's `FUN_80046A20` call order. |
| `FUN_80051078` | Separation driver: the 7×7 double loop over the actor table that calls `FUN_80050BB8(i, j)` for every ordered pair of living actors (`i != j`, both `+4 != 0`), so every actor is pushed off every other once per pass. Its caller is `FUN_80046A20`, which runs it **every battle frame** immediately after the action SM (`jal 0x801E295C` then `jal 0x80051078`), gated only on "battle live and not tearing down". Not a movement-only pass. |
| `FUN_8005133C` | Per-actor status-marker + display-list primitive spawn: allocates a primitive on the ordered list `_DAT_1F8003A0` (type tag `0x1E1 + slot`, size `0xF0`, priority 1), fills it from `gp[0xA0C] + slot*0x1E0 + 0x894` via `FUN_800583C8`, then sets the four actor status-marker bytes `+0x220..+0x223 = 1` (the lingering-status visual flags near the `+0x21F` marker). Render + status write: scope row in `render_pipeline` - the primitive is a wgpu draw in the port, and the four status-marker bytes it sets ride the actor's status flags. |

The animation pair `FUN_800495C8` / `FUN_80049858` (pose→vertex blend) is
documented in [`monster-animation.md`](../formats/monster-animation.md#vertex-blend-variants-fun_800495c8--fun_80049858).
The tween/separation cluster (`FUN_80050120` and the helpers it drives) is the
battle-overlay actor-**presentation** layer: it moves and tints the on-screen
actor sprites but touches no HP/MP/stat field, so it sits beside - not inside -
the [damage formulas](battle-formulas.md). Only `FUN_80050F30` is a pure kernel;
the rest depend on the actor table, the trig LUTs, or the GPU ordered list.

## See also

**Reference** -
[Battle action SM](battle-action.md) ·
[Damage / accuracy formulas](battle-formulas.md) ·
[Encounter record](../formats/encounter.md) ·
[Player battle files](../formats/battle-data-pack.md)

## Moved sections

These sections live on sibling pages; the anchors remain so existing links resolve.

- <a id="an-unseeded-party-reads-as-a-dead-one"></a>[An unseeded party reads as a dead one](battle-round-loop.md#an-unseeded-party-reads-as-a-dead-one)
- <a id="auto-resolve-vs-player-driven"></a>[Auto-resolve vs player-driven](battle-round-loop.md#auto-resolve-vs-player-driven)
- <a id="backdrop-ground---a-procedural-flat-grid-func_0x801d02c0"></a>[Backdrop ground - a procedural flat grid (`func_0x801d02c0`)](battle-stage-camera.md#backdrop-ground---a-procedural-flat-grid-func_0x801d02c0)
- <a id="backdrop-shell---two-copies-of-one-mesh"></a>[Backdrop shell - two copies of one mesh](battle-stage-camera.md#backdrop-shell---two-copies-of-one-mesh)
- <a id="battle-camera-exact"></a>[Battle camera (exact)](battle-stage-camera.md#battle-camera-exact)
- <a id="battle-end-retails-way---the-results-sequencer"></a>[Battle end, retail's way - the results sequencer](battle-round-loop.md#battle-end-retails-way---the-results-sequencer)
- <a id="battle-party-meshes-assembled"></a>[Battle party meshes (assembled)](battle-actor-rendering.md#battle-party-meshes-assembled)
- <a id="battle-screen-chrome-packet-pinned"></a>[Battle screen chrome (packet-pinned)](battle-hud.md#battle-screen-chrome-packet-pinned)
- <a id="dat_8007b7fc-is-a-writer-less-debug-forced-battle-id"></a>[`DAT_8007b7fc` is a writer-less debug forced-battle id](battle-round-loop.md#dat_8007b7fc-is-a-writer-less-debug-forced-battle-id)
- <a id="enemy-ally-charm-at-the-end-of-action-gate-the-charm-battle-softlock"></a>[Enemy-ally charm at the end-of-action gate (the charm battle softlock)](battle-round-loop.md#enemy-ally-charm-at-the-end-of-action-gate-the-charm-battle-softlock)
- <a id="flow-0x0c-is-the-boss-stage-modules-baton"></a>[Flow `0x0C` is the boss stage module's baton](battle-command-flow.md#flow-0x0c-is-the-boss-stage-modules-baton)
- <a id="how-the-engine-raises-the-flow-state"></a>[How the engine raises the flow state](battle-command-flow.md#how-the-engine-raises-the-flow-state)
- <a id="how-the-tint-words-reach-the-pixel"></a>[How the tint words reach the pixel](battle-actor-rendering.md#how-the-tint-words-reach-the-pixel)
- <a id="live-gameplay-loop---field--battle-in-tick"></a>[Live gameplay loop - Field ↔ Battle in `tick`](battle-round-loop.md#live-gameplay-loop---field--battle-in-tick)
- <a id="monster-ai-fun_801e9fd4-action-picker--fun_801e7320-target-resolver"></a>[Monster AI (`FUN_801E9FD4` action picker + `FUN_801E7320` target resolver)](battle-round-loop.md#monster-ai-fun_801e9fd4-action-picker--fun_801e7320-target-resolver)
- <a id="monster-mesh-record-0x04"></a>[Monster mesh (record `+0x04`)](battle-actor-rendering.md#monster-mesh-record-0x04)
- <a id="move-fx-streak-ribbon-fun_801e1d98"></a>[Move-FX streak ribbon (`FUN_801E1D98`)](battle-actor-rendering.md#move-fx-streak-ribbon-fun_801e1d98)
- <a id="object-1-is-dropped"></a>[Object 1 is dropped](battle-stage-camera.md#object-1-is-dropped)
- <a id="one-placement-record-derives-every-plate"></a>[One placement record derives every plate](battle-hud.md#one-placement-record-derives-every-plate)
- <a id="one-staged-anim-channel-actor0x1da"></a>[One staged-anim channel: `actor+0x1DA`](battle-actor-rendering.md#one-staged-anim-channel-actor0x1da)
- <a id="party-wipe--the-game-over-overlay"></a>[Party wipe + the game-over overlay](battle-round-loop.md#party-wipe--the-game-over-overlay)
- <a id="scripted-battle-entry-3e-ff-row"></a>[Scripted-battle entry (`3E FF <row>`)](battle-round-loop.md#scripted-battle-entry-3e-ff-row)
- <a id="the-0x16e-status-halfword---retail-writer-inventory"></a>[The `+0x16E` status halfword - retail writer inventory](battle-round-loop.md#the-0x16e-status-halfword---retail-writer-inventory)
- <a id="the-battle-entry-sweep"></a>[The battle-entry sweep](battle-stage-camera.md#the-battle-entry-sweep)
- <a id="the-battle-frame-step-is-the-frames-own-cost"></a>[The battle frame step is the frame's own cost](battle-stage-camera.md#the-battle-frame-step-is-the-frames-own-cost)
- <a id="the-battle-intro-enemy-name-banner"></a>[The battle-intro enemy-name banner](battle-hud.md#the-battle-intro-enemy-name-banner)
- <a id="the-battle-open-flow---ctx0x06-from-the-intro-timer-to-the-first-swing"></a>[The battle open flow - `ctx+0x06` from the intro timer to the first swing](battle-command-flow.md#the-battle-open-flow---ctx0x06-from-the-intro-timer-to-the-first-swing)
- <a id="the-command-flow-byte-ctx0x06---what-the-hook-table-indexes"></a>[The command-flow byte `ctx+0x06` - what the hook table indexes](battle-command-flow.md#the-command-flow-byte-ctx0x06---what-the-hook-table-indexes)
- <a id="the-commit-confirm-screen-0x6e"></a>[The commit-confirm screen (`0x6E`)](battle-command-flow.md#the-commit-confirm-screen-0x6e)
- <a id="the-commit-log"></a>[The commit log](battle-command-flow.md#the-commit-log)
- <a id="the-commits-clip-tag-ladder"></a>[The commit's clip-tag ladder](battle-actor-rendering.md#the-commits-clip-tag-ladder)
- <a id="the-curtain-is-a-render-to-texture-and-only-its-row-pass-is-on-screen"></a>[The curtain is a render-to-texture, and only its row pass is on screen](battle-stage-camera.md#the-curtain-is-a-render-to-texture-and-only-its-row-pass-is-on-screen)
- <a id="the-distance-fade"></a>[The distance fade](battle-actor-rendering.md#the-distance-fade)
- <a id="the-drawn-surface"></a>[The drawn surface](battle-hud.md#the-drawn-surface)
- <a id="the-full-width-message-banner"></a>[The full-width message banner](battle-hud.md#the-full-width-message-banner)
- <a id="the-grids-near-colour-and-cue-depth"></a>[The grid's near colour and cue depth](battle-stage-camera.md#the-grids-near-colour-and-cue-depth)
- <a id="the-grids-own-constants-read-off-the-emitter"></a>[The grid's own constants, read off the emitter](battle-stage-camera.md#the-grids-own-constants-read-off-the-emitter)
- <a id="the-near-camera-ghost-pass-fun_8004dc68"></a>[The near-camera ghost pass (`FUN_8004DC68`)](battle-actor-rendering.md#the-near-camera-ghost-pass-fun_8004dc68)
- <a id="the-party-status-readout---and-it-has-no-gauge"></a>[The party status readout - and it has no gauge](battle-hud.md#the-party-status-readout---and-it-has-no-gauge)
- <a id="the-per-phase-rule---what-the-sub-draw-script-builds"></a>[The per-phase rule - what the sub-draw script builds](battle-hud.md#the-per-phase-rule---what-the-sub-draw-script-builds)
- <a id="the-resting-yaw-is-the-orbit-and-battle-init-zeroes-it"></a>[The resting yaw is the orbit, and battle init zeroes it](battle-stage-camera.md#the-resting-yaw-is-the-orbit-and-battle-init-zeroes-it)
- <a id="the-retail-capture-roll-fun_801ec3e4"></a>[The retail capture roll (`FUN_801ec3e4`)](battle-round-loop.md#the-retail-capture-roll-fun_801ec3e4)
- <a id="the-rings-cancel-steps-back-a-member"></a>[The ring's cancel steps back a member](battle-command-flow.md#the-rings-cancel-steps-back-a-member)
- <a id="the-sparring-tutorial-prompt-machine-overlay-967"></a>[The sparring-tutorial prompt machine (overlay 967)](battle-command-flow.md#the-sparring-tutorial-prompt-machine-overlay-967)
- <a id="the-victory-camera"></a>[The victory camera](battle-round-loop.md#the-victory-camera)
- <a id="the-widget-class-table---where-every-chrome-sprite-comes-from"></a>[The widget-class table - where every chrome sprite comes from](battle-hud.md#the-widget-class-table---where-every-chrome-sprite-comes-from)
- <a id="weapon-trail-builder-fun_8005112c--fun_80048310--fun_800485bc"></a>[Weapon trail builder (`FUN_8005112C` + `FUN_80048310` + `FUN_800485BC`)](battle-actor-rendering.md#weapon-trail-builder-fun_8005112c--fun_80048310--fun_800485bc)
- <a id="what-the-two-boss-stage-modules-do-overlays-968--969"></a>[What the two boss-stage modules do (overlays 968 / 969)](battle-command-flow.md#what-the-two-boss-stage-modules-do-overlays-968--969)
- <a id="where-the-words-come-from"></a>[Where the words come from](battle-command-flow.md#where-the-words-come-from)
- <a id="which-stage-stream-a-scene-fights-in"></a>[Which stage stream a scene fights in](battle-stage-camera.md#which-stage-stream-a-scene-fights-in)
