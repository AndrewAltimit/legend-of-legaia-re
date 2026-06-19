# Battle subsystem

The battle overlay (`0898_xxx_dat`) carries the battle scene loader, the per-actor state machine, and the effect VM cluster. Loaded at RAM `0x801CE818` (same load slot as the town overlay; battle and town never coexist).

This is a large page covering both the retail reverse-engineering and the
clean-room engine systems. Use the contents below to jump to a section.

## Contents

**Retail scene + render**
- [Battle scene loader (`FUN_800520F0`)](#battle-scene-loader-fun_800520f0)
- [Battle background](#battle-background) - [ground grid](#backdrop-ground--a-procedural-flat-grid-func_0x801d02c0) · [dome](#backdrop-dome--sky--distant-mountains-prot-88-for-map01) · [camera](#battle-camera-exact) · [party meshes](#battle-party-meshes-assembled)

**Retail battle logic + data**
- [Battle action state machine (`FUN_801E295C`)](#battle-action-state-machine-fun_801e295c)
- [Battle context struct](#battle-context-struct)
- [Range / line-of-sight (`FUN_8004E2F0`)](#range--line-of-sight-fun_8004e2f0)
- [Monster init (`FUN_80054CB0`)](#monster-init-fun_80054cb0) - [record layout](#monster-record-source-layout) · [archive (PROT 867)](#monster-archive-prot-entry-867) · [mesh](#monster-mesh-record-0x04) · [native bridge](#native-renderer-bridge-clean-room-engine) · [AI](#monster-ai-fun_801e9fd4-action-picker--fun_801e7320-target-resolver)
- [Stat aggregator (`FUN_80042558`)](#stat-aggregator-fun_80042558)
- [Battle archive (`FUN_80052FA0` / `FUN_800542C8`)](#battle-archive-fun_80052fa0--fun_800542c8)
- [Character record layout](#character-record-layout)
- [Battle main dispatcher (`FUN_801D0748`)](#battle-main-dispatcher-fun_801d0748) · [hottest utility (`FUN_801D8DE8`)](#hottest-battle-utility-fun_801d8de8) · [weapon trail builder](#weapon--effect-trail-builder-fun_80048310--fun_800485bc)

**Clean-room engine systems**
- [Inventory (page-banked)](#inventory-cratesasset-page-banked-layout) · [Status effects](#status-effects) · [AP / Spirit gauge](#ap--spirit-gauge) · [Battle stat aggregator](#battle-stat-aggregator) · [Item catalog](#item-catalog)
- [Battle round lifecycle](#battle-round-lifecycle) · [command runner](#battle-command-runner) · [BattleSession Resolve driver](#battlesession-resolve-driver) · [HUD model](#battle-hud-model) · [SFX bank](#sfx-bank--scheduler)
- [Inventory item-use session](#inventory-item-use-session) · [Encounter system](#encounter-system) · [target picker](#battle-target-picker)
- [Equipment catalog](#equipment-catalog) · [Seru capture + spell learning](#seru-capture--spell-learning) · [Tactical Arts chain editor](#tactical-arts-chain-editor) · [rewards composite](#battle-rewards-composite)
- [Live gameplay loop - Field ↔ Battle](#live-gameplay-loop--field--battle-in-tick) - [auto vs player-driven](#auto-resolve-vs-player-driven) · [post-battle Seru learning](#post-battle-seru-learning)

**Runtime-memory captures + tests**
- [Encounter trigger memory layout](#encounter-trigger---runtime-memory-layout) · [scene-init residency](#battle-scene-init-residency-window) · [item-use residency](#item-use-battle-event-residency) · [stat-growth observations](#captured-stat-growth-observations)
- [CDNAME → MV STR cutscene routing](#cdname--mv-str-cutscene-routing) · [end-to-end gameplay loop test](#end-to-end-gameplay-loop-integration-test)

## Battle scene loader (`FUN_800520F0`)

Multi-step async state machine; sub-state byte at `gp+0xa59`. The dual-mode
loader (`_DAT_8007b8c2`) chooses between PROT-TOC indices (dev) and
`h:\prot\battle\*.dat` ISO9660 files (retail) for the same data. Notable steps:

- **State `0x8`** - loads the battle texture pack: PROT `0x368` (872) / `etim.dat`.
- **State `0xb`** - loads the battle **model** pack: PROT `0x36a` (**874**) / `etmd.dat`
  (`FUN_8003e68c(0x36a)` + `async_lba_loader`), with PROT `0x369` (873) as its index.
- **State `0xc`** - walks the loaded 874 pack and calls `tmd_register` on every
  entry (`jal 0x80026b4c` = `FUN_80026B4C`, the sole `DAT_8007C018` installer),
  then loads `efect.dat` / PROT `0x36b` (875). **This registration fills the
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
  party vertex pools - [character-mesh.md § Battle form](../formats/character-mesh.md#battle-form--assembled-from-the-player-files)).
  The field pack 0874 §0 is field-only; PROT 1204 is the Baka Fighter
  default-equipment sibling pack.
- **State `0xE`** - initialises the runtime [effect 2-pack wrapper](../formats/effect.md) via `FUN_801DE914`. Also fires for the field-VM op `0x3E` warp/interact path on the system context.
- **State `0xFF`** - dispatches the side-band streaming-effect handler `0x801F17F8` for `summon.dat` / `readef.DAT` (extraction PROT 893 / 894; format + verification in [`formats/summon-readef.md`](../formats/summon-readef.md)).

A paired stage pack loads at PROT `0x367`/`0x36d` (871/877) in states 2/4/6.
The asset-viewer's `--bundle battle` mode mirrors this loader's PROT 865–890 set so character meshes have the right CLUT bindings.

The `asset-viewer battle-scene` subcommand drives the engine-side composite end-to-end: loads the same battle bundle TMDs, builds an `engine-core::World` in `SceneMode::Battle`, spawns 3 party + 5 monster actor slots, and ticks the [battle-action state machine](battle-action.md) per frame. HUD shows the current `ActionState` (decoded into the named variant), queued action, per-slot liveness, transition counts, and any `BattleEndCause` the SM emits. Triangle cycles `queued_action`; Cross re-seeds at `ActionState::Begin`.

## Battle background

A battle is fought **on the environment where the encounter triggered, kept
resident and rendered as a full 3D backdrop** - the battle does not load a
separate flat arena. The battle-action SM only swaps the **camera** (from the
field/world walk camera to a slow orbit around the party↔enemy midpoint) and
overlays the actors + HUD; the surrounding terrain keeps drawing through its
normal renderer.

For an **overworld (world-map) encounter** the backdrop is **two layers** -
a flat tiled **ground grid** + the map's `scene_tmd_stream` **dome** (sky +
distant mountains) - pinned from a 4-angle capture set
(`overworld_battle_bg_angle_a..d`, the same Vahn-vs-Gobu-Gobu battle paused on
the Begin/Run menu while the camera idly orbits).

### Backdrop ground - a procedural flat grid (`func_0x801d02c0`)

The grass underfoot is **not** geometry from a file; it is a procedural flat
tiled grid emitted by `func_0x801d02c0` (battle-overlay variant), the **sole
draw call** the mode-`0x15` render `FUN_80026f50` makes
(`ghidra/scripts/dump_battle_backdrop_draw.py`). It is a GTE rasteriser, not a
TMD walk:

- A `_DAT_1f8003f8 × _DAT_1f8003fa` cell grid (cell pitch `0x200`, sub-step
  `0x100`), centred at the world origin on a **`Y ≈ 0` flat plane**.
- **Pass 1** - RTPS each grid point and write a per-cell visibility byte
  (`-1`/`0`/`1`) into the `0x1000`-byte buffer `_DAT_8007b814` (so the grid can
  be up to ~64×64). **Pass 2** - for each visible cell, RTPT its corners and
  emit one `POLY_GT4` (GP0 `0x0C000000`) into the ordering table.
- These tiles are the **619 `POLY_GT4`** in the live pool. Because the grid is a
  *full* flat plane centred on the actors, it fills the foreground/ground at
  **every** orbit angle - there is no half-dome gap for the ground.
  The historical overlay capture filed under the `0896` label (a mislabeled
  slot-A window image; PROT 0896 itself is neither the battle background nor
  an overlay that loads here) shows the same grid renderer + `_DAT_8007b814`
  buffer - it is battle-overlay code seen through that capture.

> **Correction.** An earlier reading called the backdrop the *world-map continent
> heightfield* per a `prim-trace` "3715 hits in `0x80190000`". That was a **false
> positive** (3 degenerate `clut=0` `POLY_FT4` prims stride-1 flooding that
> window). The ground is this **flat procedural grid**, not a per-tile continent
> descriptor table read from RAM, and not a 3D heightfield (cell `Y ≈ 0`).

### Backdrop dome - sky + distant mountains (PROT 88 for `map01`)

The sky hemisphere + distant mountain ring come from the map's `scene_tmd_stream`
dome (PROT `88` for `map01`) - the `POLY_GT3` prims (116 in angle-a):

- PROT 88 loads contiguously into battle RAM at base `0x800A8B34` (byte-matched
  across all four saves; leading TMD magic `0x80000002` at file `+4`,
  uncompressed). Loaded by the type-`0x01` chunk walker `FUN_8001FE70` into
  `_DAT_8007b864`. It is a 4-object, 968-vertex TMD + two `TimList` texture
  chunks (obj0 = sky `Y` to `-10522`, obj2 = mountains `Y` to `-2257`,
  obj3 = flat ground `Y = 0`, obj1 = near detail); PROT 88/89/90 share identical
  geometry and differ only in texture payload.
- **The dome is drawn as a background ACTOR.** `FUN_800513F0` does
  `tmd_register(_DAT_8007b864)` → installs the TMD pointer into the mesh table
  `DAT_8007C018[idx]` (returning the slot `idx`, stashed at the dome descriptor
  `0x8007680c + 4` = `DAT_80076810`), then `FUN_80020de0` (`actor_alloc`)
  allocates a battle actor whose mesh index is that slot and `FUN_80020f88`
  links it into the actor list. So the dome is rendered by the **normal battle
  actor path** (`FUN_80048A08`) - same as monsters/party - with no special
  dome-draw function (which is why `DAT_80076810` has no resolved reader: the
  actor list is walked pointer-indirect).
- **No full surround.** The dome geometry is a **front half** (`X ∈ [-12155,
  12155]`, `Z ∈ [-1260, +12155]` open toward `-Z`, all 4 objects `Z ≥ 0`), drawn
  **once**, world-fixed. As the orbit camera sweeps, different portions of the
  front arc come into view and the rest of the horizon is open sky/grass. The
  retail captures confirm this: mountains cover only **44–81 % of the horizon
  columns** depending on angle (peak when the camera looks into the arc, trough
  along its edge) - *not* a ring. The dome's own ground ring (inner radius
  `2889`) is the far grass behind the flat grid.

**Engine status.** `legaia-engine play-window --scene map01 --live-loop` renders
the overworld battle as a faithful scene: the exact orbit camera (below), the
PROT 88 dome at raw coords plus a `Ry(180°)` mirror so the mountain ring + sky
read as a full circle, a flat tiled grass grid under the actors (the
`func_0x801d02c0` grid, `0x200` cell pitch, sampling the dome's grass tile), a
sky-blue clear so the open horizon reads as sky, the real **assembled** battle
party (see below), and animated monsters. One caveat: the live camera uses a
*closer* unified depth than the exact `tr.z = 7680` - the battle meshes are small
(party 134–284 units, monsters 77–368), so at the true depth they are a few
pixels (retail draws actors off the rotation-only `DAT_8007bf10` + a per-actor
position, not the backdrop's deep matrix). Cosmetic gaps remain (the ground-mist
`obj1` is more prominent than retail; the mountain CLUT skews tan vs grey).

### Battle camera (exact)

The orbit camera (game mode `_DAT_8007b83c == 0x15`) is pinned exactly from the
four saves + Ghidra. Per-frame `FUN_80026ce4` → `FUN_80026f50` builds the view
matrix via the Euler kernel `FUN_80026988` (cos table `DAT_8007b7f8`, sin table
`_DAT_8007b81c`), composed with the identity base matrix `DAT_80010b84` and
stored at `DAT_8007bf10`; the backdrop + actors then draw through
`func_0x801d02c0`. For a PSX (Y-down) world vertex `v`:

```
screen = H * (R*v + TR) / Ze          R = Rx(pitch) * Ry(yaw)
```

with `pitch = _DAT_8007b790 = 32` (12-bit angle, `4096` = 360°, ≈2.8° down-tilt),
`yaw = _DAT_8007b792` (the orbit azimuth; the battle tick `FUN_801D0748`
decrements it by `DAT_1f800393 * 2` ≈ 4 units/frame while idle), `roll = 0`,
`TR = (_DAT_800840b8, _DAT_800840bc, _DAT_800840c0) = (0, 1280, 7680)` (eye-space
depth 7680 / height 1280), `H = _DAT_8007b6f4 = 256` (written to the GTE
projection register by `FUN_8003d254`), and the look-at target at the world
origin. The engine mirrors this in `legaia-engine`'s `retail_battle_mvp` as
`Proj_H * T(TR) * R * F` (`F` = the renderer's Y-flip), verified to 0.0002 px
against the hand-rolled projection and against the savestate framebuffer.

These values are **live-confirmed byte-exact** by
[`scripts/pcsx-redux/autorun_battle_render_capture.lua`](../../scripts/pcsx-redux/autorun_battle_render_capture.lua):
run on a real `map01` battle save (reading at the `func_0x801d02c0` grid-render
breakpoint, since at frame 0 the globals hold stale field state) it reports
`mode=0x15 pitch=32 roll=0 TR=(0,1280,7680) H=256`, the grid as **28×28** cells,
the battle actors at scale `+0x72 = 0x1000` (1.0, *not* scaled up - the
on-screen size comes from the mesh, not a scale), and the dome registered at
`DAT_8007C018[2]`.

### Battle party meshes (assembled)

The party renders the real **battle-form meshes**, assembled per character the
way the retail loader builds the blobs it installs into `DAT_8007C018[0..=2]`:
each member's mesh is spliced from their player battle file's equipment-id
sections (`legaia_asset::battle_char_assembly`, extraction PROT 863..865,
equipped ids from the roster record's `+0x196..+0x19A` bytes) and relocated
into the slot's runtime VRAM band by
`battle_char_assembly::relocate_tsb_cba` (the registration-time TSB/CBA pass,
`FUN_80053a28` - texpages `x ∈ [512, 896), y = 256`, CLUT row `481 + slot`;
see [`character-mesh.md` § Battle render](../formats/character-mesh.md#battle-render-load-time-tsbcba-relocation)).
PROT 1204 (the Baka Fighter / default-equipment sibling pack) is the
per-member fallback when assembly fails, and supplies the atlas pixel pages -
uploaded at their authoring rects and, when an assembled mesh is bound, also
written into the runtime band the relocated meshes sample.

The battle char TMD is a set of object-local pieces (head/torso/limbs),
**not** a single pre-assembled mesh, so the engine sockets them with the
**character's own idle keyframe stream from `record[0]` of the same player
file** (`battle_char_assembly::idle_battle_animation` - the monster-format
`[parts][frames][9-byte TRS]` stream at action entry `+0xAC`, `parts` =
skeleton bones; see
[`battle-data-pack.md` § Battle animations](../formats/battle-data-pack.md#battle-animations-record0)).
Frame 0 is the combat-stance rest pose, applied `R*v + T` per object
(`tmd_to_vram_mesh_posed_rot`); the clip then loops through the same
`MonsterAnimPlayer` the enemies use. Channel `i` drives object `i` directly
(post-sort object index == bone tag); the `expand_animation_for_objects`
pass duplicates each `200+` equipment extra's **attach-bone** channel onto
it (the assembler's `anm_bones` map), which is what makes the duplicate
weapon/Ra-Seru pieces coincide with their attach piece instead of floating
apart. The **PROT 1203 ANM (`other5`) is NOT this pose source** - its banks
(Vahn @ 0 / Noa @ 9 / Gala @ 18) are authored against PROT 1204's own
object order, which differs from the assembled tag order per character, so
it stays the rest-pose source for the **1204 fallback mesh only** (identity
object→bone). Pinned live + cross-pipeline in
`crates/engine-shell/tests/battle_party_pose_live.rs`. Palette: each
character's decoded battle palette (Vahn `parse_record` PROT 0863; Noa/Gala
`collect_palette` 0864/0865 - the `PLAYER1..3` files) overlays the CLUT rows
its mesh samples (`481 + slot` after relocation), so the party reads in its
real colours (blue Vahn / pink Noa / Gala). A stage battle draws **only
active actors** - the scene-init actors are bound but inactive and parked at
the world origin, so without that gate they pile their meshes at `(0,0,0)`.
A 4th party slot is not rendered: the runtime texture band + CLUT rows cover
party slots 0..=2 only, so Terra (player file 866, idle stream 17 parts)
has no relocation target.

## Battle action state machine (`FUN_801E295C`)

16 KB / 4099 instructions / 155 outgoing calls. The action-execution dispatcher: it takes the player's selected action and runs it to completion across multiple frames.

`_DAT_8007BD24` is a **pointer** to the active battle context struct (typed `int*` in the decompile output). The pointer itself is resolved at battle entry; `*_DAT_8007BD24` = `0x800EB654` for the captured battle. The action state machine accesses fields as `(*_DAT_8007BD24)[N]` - i.e. byte N of the pointed-to struct.

The outer dispatch is `switch((*_DAT_8007BD24)[7])` - byte +0x07 of the ctx struct, which holds the **active action ID** for the currently-resolving party action slot. (Byte +0x06 holds the parallel ID for the monster action slot; only one is non-`0xFF` at a time.) The inner dispatch is `switch(actor[+0x1DE])` - the per-actor **action sub-state** (windup → execute → recover-style staging within each action).

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
| `+0x06` | u8 | Monster-slot active action ID (or `0xFF` if no monster action queued). |
| `+0x07` | u8 | Party-slot active action ID (or `0xFF`). The outer `switch((*_DAT_8007BD24)[7])` in `FUN_801E295C` keys on this. |
| `+0x09` | u8 | Turn / phase counter. |
| `+0x13` | u8 | Active-actor slot index - used to look up the actor pointer via `(&DAT_801C9370)[ctx[0x13]]`. |
| `+0x14..+0x17` | u8 × 4 | Per-action parameter bytes (target slot, sub-action, etc. - varies by action ID at +0x07). |
| `+0x18..+0x1B` | u8 × 4 | More action params (dir/elem byte at +0x18, second target at +0x1A, etc.). |
| `+0x1D` | u8 | Action context flag - `0x03` for summon and capture; `0x00` otherwise. |
| `+0x29..+0x2D` | string | Active spell/move icon glyph (`0xCE 0x14 0x20 'G' 'i' 'm' 'a' 'r' 'd' …`). |
| `+0xA9..+0xEC` | text | Battle dialog buffer (`"Vahn won the battle!|Gained …Experience and …G."`). |
| `+0x6D6..` | u8 × N | The action state machine's "PC offset" / sub-state cursor (read by `*(byte*)(ctx + 0x6D6)`). |

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
| `+0x34` / `+0x38` | i16 | Current world X / Z. |
| `+0x3C` / `+0x40` | i16 | Previous-frame X / Z (for delta tracking). |
| `+0x4A` | u8 | Magic-slot count. |
| `+0x4C` | int* | Spell-entry pointer array (each entry: `[u8 spell/action id, …, u8 SP cost @ +0x74]`). |
| `+0x14C..+0x152` / `+0x172..+0x174` / `+0x150..+0x158` | u16 | HP / MP / current / max - three-way mirror layout. |
| `+0x1BC..+0x1BE` | u8 | "Show damage" overlay byte triplet. |
| `+0x1DF` | u8 | Monster size byte (read from a monster record at `+0x1F` and stored here at init). |
| `+0x1EF..+0x1F3` | u8 | Per-element spell-slot index (from the spell ids `2,3,4,5,0xB`). |
| `+0x230` | u32 | Attack-effect / animation data pointer (set from record `+0x04`; **not** XP/drop). |

## Range / line-of-sight (`FUN_8004E2F0`)

`FUN_8004E2F0(actor_a_id, actor_b_id) -> i16 distance` is the canonical battle range check, called 5+ times from the per-actor state machine. Reads `[DAT_801C9370 + id*4]` for both actors, computes a euclidean distance from `+0x34/+0x38` (or `+0x3C/+0x40` for the b-actor), then sums the two `+0x1F` size bytes (party-member size table at `0x80078878`, monster size byte read from the live actor) to get the hit radius. Final value is clamped to a per-actor cap and `0xF` per `param_2 < 3` party tier.

## Monster init (`FUN_80054CB0`)

Called from `FUN_800542C8` (secondary battle archive loader). Populates a battle-actor at `[DAT_801C9370 + (slot+3)*4]` from a monster record:

- HP / MP / SP triplets at `+0x14C..0x158` and `+0x172..0x174`.
- Magic-resistance bytes at `+0x1EF..+0x1F3` (5 elements; one nibble per element).
- Walks the spell list at `+0x4C` (count at `+0x4A`): for the elemental ids (`2,3,4,5,0xB`) it records the matching spell's slot index into the per-element table at `+0x1EF..+0x1F3`.
- Attack-effect / animation data pointer (record `+0x04`) into `+0x230`.

This is the canonical "monster spawn" path. Engine port reads the record once, populates the actor struct, and lets `FUN_801E295C` take over.

### Monster-record source layout

`param_1` is the in-RAM monster record (after the loader's offset→pointer fixups). Field map traced from `FUN_80054CB0`:

| Offset | Type | Use |
|---|---|---|
| `+0x00` | u32 | Name string pointer (disc offset → pointer; `strlen` copied into actor `+0x1BC`). |
| `+0x04` | u32 | Block-relative offset of the monster's **battle-model TMD** → actor `+0x230` (walked as `0x1C`-stride geometry records - a TMD object-table entry is `0x1C` bytes - by `FUN_80049858` / `FUN_800495C8`). **Not** XP/drop. See [Monster mesh](#monster-mesh-record-0x04). |
| `+0x08` | u32 | Shared-resource pointer (fixed up at load). |
| `+0x0C` | u16 | **HP** → actor `+0x14C/+0x14E/+0x172`. |
| `+0x0E` | u16 | **SP** → actor `+0x154/+0x156` (spirit/action gauge - AI spell-selection budget; spirit-charge source). |
| `+0x10` | u16 | **MP** → actor `+0x150/+0x152/+0x174`. |
| `+0x12` | u16 | **ATK** → actor `+0x158/+0x15A` (attacker offense in the damage routine). |
| `+0x14` | u16 | **DEF↑** → actor `+0x15C/+0x15E` (defender defense, high facet). |
| `+0x16` | u16 | **DEF↓** → actor `+0x160/+0x162` (defender defense, low facet). |
| `+0x18` | u16 | **AGL** → actor `+0x168/+0x16A` (rescaled into the accuracy/evasion seed). |
| `+0x1A` | u16 | **SPD** → actor `+0x164/+0x166` (turn-order initiative seed; buffable). |
| `+0x21` | u8[3] | **Magic-attack ids** (`+0x21..+0x23`): up to three **global** spell ids the enemy casts. A slot is live when its value is `> 1`. The AI spell picker `FUN_801E9FD4` (`overlay_0898`) reads `record[0x21 + slot]`, writes it into the live actor at `+0x1DF`, and the battle-action SM names it via `&DAT_800754D0 + id*0xC` (`0x27` → `Tail Fire`). These global ids are **distinct** from the local `+0x4C` entry ids (which only gate SP); they are the names that appear on screen. Parser: `MonsterRecord::magic_attacks` + `legaia_asset::spell_names`. |
| `+0x44` | u16 | **gold** (base victory-spoils gold). |
| `+0x46` | u16 | **EXP** (base victory-spoils experience). |
| `+0x48` | u8 | **drop item id** (`0` = no drop). |
| `+0x49` | u8 | **drop chance** in percent (`rand() % 100 < pct`). |
| `+0x4A` | u8 | Magic-slot count. |
| `+0x4C` | u32[] | Spell-entry offsets (count at `+0x4A`; block-relative, fixed to pointers at load). Each entry's first byte is a **spell/action id**: ids `2,3,4,5,0x0B` are elemental resist/affinity markers (`FUN_80054CB0` writes the slot index into actor `+0x1EF..+0x1F3`); ids `0x0C..0x1F` are offensive castable spells; `0x23` is special. Entry `+0x74` is the **SP cost**. See [battle-formulas.md → spell list](battle-formulas.md#spell-list-record-0x4c). |

All six stat names are pinned by the consumers of those actor slots - see [battle-formulas.md](battle-formulas.md#actor-stat-block--monster-record-mapping). The parser exposes them via `legaia_asset::monster_archive::MonsterRecord::{attack, defense_high, defense_low, agility, speed, spirit}`.

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

### Monster mesh (record `+0x04`)

Each decoded monster block carries the monster's **battle model**: a
[Legaia TMD](../formats/tmd.md) embedded at the block-relative offset held in
the stat record's `+0x04` field (immediately after the name string). This is
the same pointer the loader installs at battle-actor `+0x230` and that
`FUN_80049858` / `FUN_800495C8` walk as `0x1C`-stride records - a TMD
object-table entry is exactly `0x1C` bytes, so that walk is iterating the
mesh's per-object table. Verified across the archive: **186 of the 194 slots
carry a Legaia TMD at `+0x04` that the parser walks cleanly** (the other 8 are
empty / filler ids); e.g. Gimard (id 10) = 200 vertices / 269 textured prims
at block `+0x7c`.

Decoded-block layout (after the stat-record head at `+0x00`):

```
+0x00  stat record head (name_offset, +0x04 mesh offset, +0x08 pool offset, stats, rewards, spells)
name   NUL-terminated name string (at name_offset, typically just before the mesh)
+0x04→ Legaia TMD              ; the monster's battle model (magic 0x80000002)
spells spell-entry blobs       ; each carries its own attack-effect geometry
+0x08→ texture / CLUT pool     ; per-monster palettes + 4bpp texture pages
```

The mesh's primitives are textured: they reference a CLUT + a 4bpp texture page
via per-prim CBA/TSB. The matching palette + pixel bytes live in the **texture
pool at record `+0x08`**, whose layout is pinned from the battle loader
`FUN_80055468` (the streaming archive loader `FUN_800542C8` calls it with the
pool pointer, the embedded TMD, and the battle-slot index):

```
+0x000  15 x [16 BGR555 colours]   ; CLUT region (0x1E0 bytes; zero-padded for
                                   ;   monsters that use fewer than 15)
+0x1E0  4bpp indices               ; texture page, width x 256 texels, row-major
```

The loader uploads the CLUT region to VRAM `(0, 484 + slot)` (256 colours wide,
STP bit set on non-zero entries) and the page to `(slot*64 + 320, 256)`. The
page is **always 256 rows tall**; its width is **128 texels** (32 fb-units) for
most monsters or **256 texels** (64 fb-units) when the per-monster wide flag is
set - so `width_texels = (pool_len - 0x1E0) / 256 * 2`. A primitive selects its
palette by `cba & 0x3F` and samples the page at its per-vertex `(u, v)`; PSX
index 0 (colour `0x0000`) is transparent. The byte arithmetic is exact: Gimard
`0x1E0 + 128*256/2 = 0x41E0`, Tetsu `0x1E0 + 256*256/2 = 0x81E0`, both equal to
their pool sizes. (The on-disc CBA/TSB are nominal defaults the loader relocates
per slot, so the raw pool bytes do not appear verbatim in a battle VRAM dump -
the `FUN_80055468` layout is the ground truth; see
`ghidra/scripts/funcs/80055468.txt`.)

Parser: `legaia_asset::monster_archive::mesh(entry, id) -> Option<MonsterMesh>`
(returns the decoded block + the TMD/pool offsets); `MonsterMesh::texture()`
decodes the pool into `MonsterTexture { palettes, indices, width, height }`. CLI
`asset monster-archive --id N --obj <out>` exports the mesh as Wavefront OBJ and
`--texture-png <out>` bakes the texture page. WASM: the
`LegaiaViewer::monster_mesh_{positions,normals,indices,bounds,uvs,palette_index}`
and `monster_texture_{indices,palette_rgba,dims}` accessors feed the in-browser
WebGL viewer on the enemy-table site page, which textures the model with the
index→palette lookup the PSX GPU does in VRAM.

### Native renderer bridge (clean-room engine)

The clean-room engine renders the decoded monster directly through its standard
PSX-VRAM texture path rather than the site's index→palette shortcut.
`MonsterMesh::battle_render_mesh(slot, &mut vram)` reproduces the loader's
per-slot relocation: it writes the CLUT region to VRAM row `484 + slot` and the
4bpp page to `((5 + slot) * 64, 256)`, then rewrites every prim's CBA/TSB to
point at those regions (`relocate_cba` / `relocate_tsb`), keeping the
page-local UVs untouched. Because the on-disc CBA/TSB are nominal defaults the
loader relocates, this is what makes the textures resolve against the injected
VRAM. The CLUT region (`x < 240`) and the texture pages (`x >= 320`) never
overlap, so up to five monster slots coexist in one VRAM.

`World::battle_monster_slots()` reports the active enemies as
`(actor_index, monster_id, battle_slot)`; the engine itself never loads the
archive, so the host resolves each id to a `MonsterMesh`, injects it, and binds
the relocated mesh to the actor. `play-window --live-loop` / `--player-battle`
does this on each `Field → Battle` transition (against a throwaway clone of the
field VRAM, restored on the way back) so the enemy is drawn, not a stand-in.

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
- **`FUN_801E7320` - target resolver.** Called from the action SM
  (`FUN_801E295C`) at `ActionSeed` as the `monster_setup` hook, but only for
  monster actors with `actor[+0x16e] & 0x380 != 0`. It reads the targeting class
  the picker left in `actor[+0x1DD]` and expands it: class `0..2` → a living
  monster slot (`rand % monster_count + party_count`); class `3..6` → a living
  party slot (`rand % party_count`); class `8`/other → a `rand % 3` gate
  selecting all-target codes `8`/`9` or self. ctx fields: `ctx[+0]` = party
  count, `ctx[+1]` = monster count, `ctx[+0x13]` = active slot. Dumps:
  `ghidra/scripts/funcs/overlay_battle_action_801e9fd4.txt`,
  `overlay_battle_action_801e7320.txt`.

The clean-room engine ports it across `engine-core`:

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
catalog knows the id (the disc spell table, or the clean-room monster block in
`SpellCatalog::vanilla`) and otherwise degrade to a physical strike.

**Faithful default = uniform-random single target.** Retail's `OneEnemy` /
physical target is a uniform random living party member (`rand % party_count`,
re-rolled past downed slots). An **opt-in, non-faithful** QoL toggle
(`World::smarter_monster_targeting`, off by default; `legaia-engine play-window`
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

## Stat aggregator (`FUN_80042558`)

Per-frame helper that walks the 3 active party members (stride `0x414` - see [character record layout](#character-record-layout)) and:

1. Caps each character's stats at `0x3E7` (999, the in-game stat ceiling).
2. ORs the character's "active abilities" 16-byte block at `+0xF4..0x100` into a global 4×u32 bitmask at `0x80074358..0x80074368`. This is the "currently-active accessory effects" register read by every other game system.
3. For each character, calls `FUN_800432BC` / `FUN_80042DBC` to add/remove temporary spells per the active spell-slot layout at `+0x2B0`.

The 4-u32 global ability bitmask is what tells the renderer to draw "auto-counter" / "regen" / "magic up" indicators and what tells the battle dispatcher to apply post-hit effects. The read-side primitive is `FUN_800431D0(bit_id) -> bool` - `(&DAT_80074358)[bit_id >> 5] & (1 << (bit_id & 0x1F))`. It's a 6-instruction hot helper cited from the action validator (`FUN_8003FB10`) and most damage / status code paths, so a clean-room port models it as `BattleState::ability_active(u8) -> bool`.

`FUN_800349EC` and `FUN_80035EA8` are the HP / MP threshold UI classifiers - given a character index they compare current vs max and return one of `2` (dead/zero) / `6` (low) / `7` (warn) / `9` (healthy). The dialog renderer keys text colour on the result.

`FUN_8003FB10` is the **action validator** that decides whether a queued action can proceed for the active actor. It sub-dispatches on `actor[+0x9A8]` (the queued-action byte) into 16+ handler arms; each arm consults a mix of per-actor state, the current target's record at `0x80084708 + tgt*0x414`, the global ability bitmask via `FUN_800431D0`, and the `0x8007BD10` actor-type table to gate the action with a 16-bit return code (action-OK, blocked, requires-target-flag, etc.). Engine reimpl wires this between the move VM and the per-actor state machine `FUN_801E295C`.

## Battle archive (`FUN_80052FA0` / `FUN_800542C8`)

Two SCUS-side archive loaders feed the battle state. Their record-walk helpers:

- `FUN_800536BC` - copies records of stride `0x1C` from the archive into runtime layout, applying delta fixups to 6 of the 7 u32 fields (offset → absolute pointer pattern: `record[+0x18..0x30]`).
- `FUN_80053898` - bubble-sort over the 7-u32-stride records keyed on parallel byte arrays.
- `FUN_80053B9C` - copies short-array records into the per-slot UI buffer at `iVar1 + 0x894 + slot*0x1E0`, OR-ing `0x8000` into each entry (the "active" flag).

Both archive loaders interact with the battle character / monster slots via the 8-actor table at `0x801C9370`.

## Character record layout

Stride `0x414` bytes per character, base `0x80084708` (so character `n` lives at `0x80084708 + n*0x414`). Surfaced by the inventory/spell helpers (`FUN_80042558`, `FUN_80042DBC`, `FUN_800432BC`, `FUN_800431FC`, `FUN_80043264`):

| Offset | Use |
|---|---|
| `+0x13C` | u8 spell-list count. |
| `+0x13D..+0x160` | u8 spell IDs (variable-length; up to 36). |
| `+0x161..+0x184` | u8 parallel spell-level / experience array. |
| `+0x196..+0x19D` | u8 equipment slot bytes (8 slots; weapon, armour, accessories). |
| `+0x2A7..+0x2B0` | NUL-padded ASCII display name (`Vahn`/`Noa`/`Gala`/`Terra`/player-entered lead), 9 bytes bounded by the active-spell table at `+0x2B0`. Pinned across six in-game RAM captures for all four roster slots. In the retail SC save block this lands at `game+0x66F + n*0x414` (SC `+0x86F` for slot 0); see [`save-screen.md`](save-screen.md). Accessor `legaia_save::CharacterRecord::name` (`NAME_OFFSET`). |
| `+0x2B0..+0x37F` | Active spell-slot array (stride `0x14`, up to N entries). Populated by `FUN_80042DBC` from the spell list. |
| `+0xF4..0x100` | "Active abilities" 16-byte block - OR'd into the global 4×u32 bitmask at `0x80074358..0x80074368` by `FUN_80042558`. |
| `+0x104..0x110` | HP / MP / SP triplets (cur, max stored as separate u16s). |
| `+0x10E` | u8 - written on level-up (delta `+8` for Vahn slot in the captured pre→post pair). Likely max-HP byte component or stat-derived rank. |
| `+0x11A` | Stat-cap field (clamped to `0x3E7`). |
| `+0x11C..+0x122` | Six adjacent stat bytes (paired) - incremented by small deltas (`+1..+4`) on level-up. Likely the per-stat rank table consumed by the level-up apply path. |
| `+0x130` | u8 - the **displayed character level** (the byte the status screen reads as "LV"; the `Level 99` cheat target), incremented `+1` per level-up event. See [save-record.md](../formats/save-record.md#0x130-is-the-displayed-character-level). |
| `+0x161..+0x184` | u8 spell-level array (one byte per spell id; stride matches spell list). Magic-rank up writes here (delta `+1` per learned spell). |

**Level-up captured deltas (Vahn, pre/post a single character-level event).** Diff captured via `mednafen-state` shows the per-character side-effects:

| Offset | Width | Pre → Post | Interpretation |
|---|---|---|---|
| `+0x00` | u8 | `0x4F` → `0x73` (79 → 115) | Possibly raw level byte / per-character XP-derived counter. |
| `+0x04..+0x06` | u16 LE | `0x016D` → `0x02DA` (365 → 730) | XP word delta (+365). Matches the published level-up XP curves. |
| `+0x10E` | u8 | `0x3A` → `0x42` (+8) | Max-HP / vitality byte. |
| `+0x11C..+0x122` | 6× u8 | `67/1C/13/10/16/0B` → `6B/20/15/12/1A/0F` | Per-stat increments (`+4 +4 +2 +2 +4 +4`). |
| `+0x130` | u8 | `0x02` → `0x03` | Displayed character level (+1 - the level 2 → 3 event). |

Noa and Gala records are byte-identical across the same pair - the level-up event in this capture pair is for Vahn alone.

**Magic-rank up captured deltas (Vahn, pre/post a single magic-rank-up event).** Diff over the same record range surfaces a strict subset of the level-up footprint, focused on the spell-level table:

| Offset | Width | Pre → Post | Interpretation |
|---|---|---|---|
| `+0x08` | u8 | `0x30` → `0x3C` (+12) | Flag word - specific bit TBD. |
| `+0x9C` | u8 | `0x09` → `0x0A` (+1) | Magic-rank mirror. |
| `+0x10A` | u8 | `0x1B` → `0x11` (-10) | TBD (transient battle state, possibly post-strike). |
| `+0x161` | u8 | `0x02` → `0x03` (+1) | Spell-level byte (`+0x161..+0x184` array). Confirms magic-rank up writes here. |

## Battle main dispatcher (`FUN_801D0748`)

11 KB / 182 calls. The top of the per-frame battle loop. Routes through every active battle subsystem (rendering, AI, animation, hit detection).

## Hottest battle utility (`FUN_801D8DE8`)

3 KB / 77 incoming refs. The single most-cited battle helper - likely a per-actor utility that every state arm bottoms out into.

## Weapon / effect trail builder (`FUN_80048310` + `FUN_800485BC`)

Visual-only helpers that build the swept geometry behind a moving battle actor (sword trails, dash plumes, particle ribbons). `FUN_80048310` iterates the 16-slot per-actor frame buffer at `actor[+0x68]`, copies vertex triplets from the per-actor pose pool at `gp[0xa0c] + 0x6f4` (stride `0xC`), and calls `FUN_800485BC` twice - once for the outline, once for the base - blending two endpoint colours over N steps via a `0..N` gradient loop.

`FUN_800485BC` is a 275-instruction quad-strip emitter. It looks up the actor pose from `*(int*)(0x801C9370 + actor[+0x5A]*4) + 0x34/+0x38` (re-confirms the battle actor pointer table), reads sin/cos LUTs at `_DAT_8007B81C` / `_DAT_8007B7F8` keyed on `actor[+0x26] * 0xFFF`, runs each vertex through `FUN_800195A8` for GTE projection, and drops `0x3B808080` (GP0 G3 textured-quad) packets into the OT.

These are pure rendering helpers - no gameplay state changes. Engine reimpl can defer them until visuals matter.

## Inventory (`crates/asset` page-banked layout)

Battle reads inventory through the same page-banked structure the field VM's op `0x3B` `SET_ITEM_COUNT` writes: 16 entries × 16-bit per page × 0x414-byte stride. The page index is the high nibble of the slot byte; the entry index is the low nibble.

The page-banked inventory state lives in the 512-byte region at `[0x80085718 .. 0x80085918)` - adjacent to the fourth-flag-bank bitfield at `DAT_80085758` (see [field VM](script-vm.md) → "fourth flag bank"). The field VM's op `0x4C` sub-3 sub-2 zeros the entire region.

## Status effects

Per-actor status conditions inflicted by enemy attacks or art `enemy_effect` bytes. The retail engine stores per-status timers and tick-damage values in the battle-actor struct around `+0x130`; the layout is per-flag and not captured in any single overlay dump.

Conditions are named with the game's in-game ailment terms (the `enemy_effect` byte is the on-disc art-record value). The `Retail effect` column is the published behaviour from the Legaia wiki status pages. The poison **tick formulas are pinned** from the per-round DoT ticker `FUN_801E752C` (see [battle-formulas](battle-formulas.md) § "Per-round status DoT ticker"); the `Default duration` values remain clean-room approximations (no retail per-status duration table is in any single overlay dump). The `Engine` column flags where this port diverges from retail.

| Status | byte | Default duration (clean-room) | Retail effect (wiki) | Engine |
|---|---|---|---|---|
| Toxic | `1` | 4 turns | "Deadly Poison": HP drains faster than Venom AND attack/defense drop | `min(max_hp/16, 256)` tick, never kills (bottoms at 1 HP), suppresses Venom's tick while active (`FUN_801E752C`); combat rolls ×7/10 (`FUN_801DD864` bit 2), mirrored as ATK & DEF ×0.7 |
| Numb | `2` | 3 turns | Paralysis: cannot act; clears on being hit or after some turns | full block + clear-on-hit (enforced, same shape as Sleep) |
| Venom | `3` (Other) | 6 turns | "Poison": HP drains (lesser than Toxic) | `min(max_hp/32, 128)` tick, never kills (`FUN_801E752C`); combat rolls ×9/10 (`FUN_801DD864` bit 1), mirrored as ATK & DEF ×0.9 |
| Sleep | `4` | 3 turns | Asleep; wakes when hit | block + clear-on-hit (matches) |
| Confuse | `5` | 3 turns | Acts uncontrollably / random target | a confused action (monster *or* party physical, plus monster casts) retargets to a random living member of the opposite side (`FUN_801E7320`); a confused party member auto-acts a physical strike with no command menu - an engine stand-in (retail's party-side delegated action pick is unpinned; see [battle-action](battle-action.md) § AI-delegated party members) |
| Curse | `6` | 4 turns | Blocks Magic | blocks Magic (matches) |
| Stone | `7` | whole battle (255) | Petrification: cannot act, cannot be damaged, counts as defeated; lasts the whole battle (no in-battle cure; escape restores) | block + whole-battle duration + invulnerability at every damage entry point + counts-as-defeated in the wipe checks; escape restores (see below) |
| Faint | `8` | until cured | KO at 0 HP: collapse, no actions; revived only by Phoenix / revive Magic | block + `until cured` (matches) |

Implementation: [`crates/engine-vm::status_effects`](../../crates/engine-vm/src/status_effects.rs). The per-tick `StatusEvent` stream feeds back into the engine's HUD pipeline; engines call `World::tick_status_effects` once per round and consume `StatusEffectTracker::drain_events()` for log lines. Both battle drivers tick it once per round: the runner path at `BattleRound::end`, and the live loop at the initiative round boundary (when no living actor still holds an initiative key, just before the keys reseed).

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

## AP / Spirit gauge

Each character has a per-turn AP budget that limits how many art commands they can chain. The retail engine reads this from the character record's `+0xC9` (`current_ap`) and `+0xCA` (`bonus_ap`) bytes. Pressing the Spirit button during command input adds `+5` AP exactly once per turn.

The base AP grows by 1 each 10-level milestone (level 1..9 → 4 AP, 10..19 → 5 AP, …, 60+ → 10 AP capped; `ap_base_for_level`). The engine seeds each party member's `ApGauge::base_ap` from that formula at battle entry - `seed_party_battle_stats` reads the live character level alongside the attack / defense fold, so a higher-level character chains more arts per turn. The round-start `reset_party_ap` then refills `current_ap` to that base, and Fury Boost extends from / reverts to it.

| Action constant range | AP cost | Notes |
|---|---|---|
| `0x00` Nothing | 0 | placeholder |
| `0x01..=0x05` | 0 | system actions (Item / Magic / Attack / Spirit / Escape) |
| `0x0C..=0x0F` | 0 | direction bytes (free) |
| `0x19` Regular Art Starter | 1 | |
| `0x1A` Special Art Starter | 1 | |
| `0x1B..=0x32` | 1 | per-character art body |

Implementation: [`crates/engine-core::ap_gauge`](../../crates/engine-core/src/ap_gauge.rs). The `World` carries a `[ApGauge; 3]` (one per party slot); engines call `World::reset_party_ap` at turn start.

## Battle stat aggregator

Clean-room port of `FUN_80042558`. Walks the 8 equipment slots, sums modifiers into the actor's resolved attack / UDF / LDF / accuracy / evasion, ORs equipment ability bits into the global 4×u32 mask, then folds in status-effect modifiers (Toxic reduces ATK + both defenses by ~12.5%, Confuse halves accuracy, Numb / Sleep / Stone / Faint zero evasion and block actions, Curse / Faint block Magic).

Implementation: [`crates/engine-core::battle_stats`](../../crates/engine-core/src/battle_stats.rs). The pure function `compute_battle_stats(record, table, statuses, modifiers) -> BattleStats` is deterministic and side-effect-free - engines call it once per turn-start.

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

Implementation: [`crates/engine-core::items`](../../crates/engine-core/src/items.rs).


## Battle round lifecycle

`BattleRound::begin(&mut world, &[Option<StatRecord>; 8], &EquipmentTable, &StatusModifiers)` resets every party AP gauge, recomputes per-slot `BattleStats` through `compute_battle_stats`, and writes the resolved attack / UDF / LDF back into `World::battle_attack` / `battle_defense_split` so the strike resolver picks them up. `BattleRound::end(&mut world)` ticks every actor's status, folds Toxic / Venom tick damage into `BattleActor::hp`, and returns the count of actors that died from tick damage this round.

The returned `BattleRound` carries per-slot `action_blocked` / `magic_blocked` arrays the action validator filters command input against (Numb / Sleep / Stone / Faint actors lose action; Curse / Faint actors lose Magic).

Implementation: [`crates/engine-core::battle_round`](../../crates/engine-core/src/battle_round.rs).

## Battle command runner

Sits between the player-input layer and the action state machine. One `BattleRunner` per battle session; engines feed it raw player commands per turn and call `tick_action` to drive the per-frame action SM.

`begin_round` delegates to `BattleRound::begin` for AP refresh + stat recompute, `push_command` / `push_chained_art` gate input against `ApGauge` and surface a typed `OutOfAp` error, `pop_command` / `pop_chained_art` refund the cost cleanly, `commit_turn` runs the queue through `resolve_action_queue` (Miracle / Super expansion) and stashes the resolved per-slot `ActionQueue`s. `end_round` drives `BattleRound::end` for tick-damage drainage.

Per-slot buffers + chained-art lists let the player switch between party members mid-turn without losing state. The runner is the **input → queue** half of the battle pipeline; the SM tick itself runs through the existing `step_battle` loop.

Implementation: [`crates/engine-core::battle_runner`](../../crates/engine-core/src/battle_runner.rs).

## BattleSession Resolve driver

`BattleSession` owns the action SM during the `Resolve` phase. After
`commit_turn` succeeds, the session builds a `ResolveDriver` queue
containing one entry per party slot whose resolved action queue is
non-empty, in slot order (`0 → 1 → 2`). Slot routing:

| Resolved queue contains | Action category byte |
|---|---|
| At least one `ActionConstant::RegularStarter` | `TacticalArts (0)` |
| Otherwise (directional commands only) | `Attack (3)` |

Each `BattleSession::tick` during `Resolve`:

1. Drains `World::pending_battle_events` into HUD popups + session events.
2. If the head-of-queue attacker hasn't been armed yet, sets
   `world.battle_ctx.{active_actor, queued_action, action_state}` and
   the attacker's `BattleActor::{action_category, active_target}` to
   point at the first alive monster slot.
3. Calls `world.tick()` exactly once.
4. Clears `ActorFlags::ADVANCE_DONE` on `AttackRecovery` (the render-side
   "recovery anim finished" edge the session simulates inline since it
   doesn't render).
5. On `Transition { from: AttackChain, to: AttackRecovery }`, applies a
   clean-room formula strike against the attacker's `active_target`:
   reads `atk` + `udf` + `acc` + `eva` off `BattleRound::stats`, rolls
   accuracy via `accuracy_roll`, folds variance via `psyq_rand_step`,
   writes the result back through `BattleActor::hp` and emits
   `SessionEvent::HpChanged`.
6. On `EndOfAction`, pops the head of the queue and re-arms next frame.

When the queue drains (no more attackers) or `StepOutcome::BattleComplete`
fires, the session drops the driver and transitions to `RoundOutro`
(queue-drained path) or relies on the routed `BattleEnd` event to land
the terminal phase (`Victory` / `Defeat`). Engines that prefer to drive
`world.tick()` themselves can skip `commit_turn` from the session and
fall through the legacy "observe events only" Resolve path.

The deterministic RNG seed used for the accuracy + variance rolls is
exposed as `BattleSession::rng_seed` (configurable via
`with_rng_seed(seed)` before `begin_round`).

End-to-end coverage:
[`crates/engine-core/tests/end_to_end_gameplay_loop.rs::battle_session_drives_action_sm_to_monster_wipe`](../../crates/engine-core/tests/end_to_end_gameplay_loop.rs)
exercises the full pipeline - encounter trigger → BattleSession setup →
`push_command` per slot → commit via `SessionInput { start: true, .. }` →
Resolve → `BattlePhase::Victory`.

## Battle HUD model

Renderer-agnostic UI state for the in-battle screen. Holds per-slot HP / MP / AP / status-icon state plus a queue of damage popups and battle-event log lines. `engine-render::battle_hud_draws_for` turns one of these into a `Vec<TextDraw>` for the GPU pipeline; engines that render via a different path (web / terminal) read the same struct directly.

The HUD is fed by `World` events:

- `BattleEvent::ApplyArtStrike` → `push_damage` / `push_heal` (per-strike popup with a fade timer).
- `StatusEvent::TickDamage` / `Cleared` → `sync_status` (replaces the slot's icon list from the `StatusEffectTracker`).
- `BattleRound::begin` / `end` → `sync_slot` (refreshes HP / MP / AP per round).

Damage popups carry a 60-frame default lifetime and an `alpha()` helper for fade-out renders. The log column rings the most recent N entries (default 6, matching the retail scrolling-log column).

Implementation: [`crates/engine-core::battle_hud`](../../crates/engine-core/src/battle_hud.rs).

## SFX bank + scheduler

Maps battle / field cue IDs (the `kind` byte the art-record `HitCue` / overlay scripts emit) to per-cue `SfxEntry` descriptors that describe how to fire a one-shot through the SPU. Engines populate the catalog at startup, then forward `ScheduledCue`-like requests through `SfxScheduler` which queues each request with its retail timing offset and dispatches when the per-frame tick reaches the firing frame.

| Cue ID | Meaning |
|---|---|
| `0x1A` | Generic SFX trigger ("play sound" hit cue). |
| `0x4C` | Hit-effect visual (no sound on its own). |
| `0x80..=0xFE` | Reserved per-character / per-art SFX IDs. |

`SfxBank::play_one_shot` delegates to the existing `VabBank::play_note` for tone lookup, pitch math, and ADSR setup; the scheduler is a frame-driven queue that returns an `SfxFireBatch` per `tick_frame` call.

The bank is decoded from the user's `SCUS_942.54` `DAT_8006F198` descriptor table at boot (`SfxTable::from_scus` → `SfxBank::from_descriptors`, see [`sfx-table.md`](../formats/sfx-table.md)) and plays through the per-scene music VAB. The live battle loop drives it: each `BattleSfxCue` drained from `World::drain_battle_sfx_cues` is enqueued into the director's scheduler at its `timing_frames` delay, and one `tick_sfx_frame` per simulation tick advances the queue and keys matured cues on through the SPU. Cues touch only the SPU (no RNG), so battle determinism is unaffected; a missing bank / VAB / free voice silently drops the cue.

Implementation: [`crates/engine-audio::sfx`](../../crates/engine-audio/src/sfx.rs); the host-side bank decode + per-tick drive live in `crates/engine-shell` (`AudioBgmDirector::{set_sfx_bank,enqueue_sfx,tick_sfx_frame}`).

## Inventory item-use session

State machine that drives the "open inventory → pick item → pick target → use it" flow shared between the field menu and the battle command menu. Engines own a single `InventoryUseSession` for the lifetime of the inventory screen; per-frame they push input events and drain `InventoryUseEvent`s.

Filters items by `InventoryContext` (battle vs field - `usable_in_battle` / `usable_in_field` from the catalog), validates target compatibility (Revive needs a dead target; everything else needs a live one), and folds the resolved `ItemOutcome` into the engine's world state via `World::use_item`.

Implementation: [`crates/engine-core::inventory_use`](../../crates/engine-core/src/inventory_use.rs).


## Encounter system

Per-scene random-encounter trigger. Engines own one `EncounterSession` per active field scene; the field-step path calls `on_step(rng_word)` each step the player moves. The session brackets the transition with five phases:

| Phase | Drives |
|---|---|
| `Idle` | Steady state. Steps roll against the table; safe zones suppress. |
| `Transition` | Roll succeeded; `transition_frames` (default 32) of camera-shake / fade-out. |
| `Triggered` | Engine drains the resolved `EncounterRoll` and loads the battle scene. |
| `Battling` | Battle is running; tracker is suspended. |
| `Grace` | Post-battle "no immediate re-encounter" window (`grace_frames`, default 30). |

`EncounterTable` holds the per-scene rows + 1/256 trigger rate + safe-zone rectangles. `EncounterTracker::add_rate_bias` lets accessory effects (Goblin Foot = -32, Encounter Up = +32) tune the effective rate per-roll.

Implementation: [`crates/engine-core::encounter`](../../crates/engine-core/src/encounter.rs).

## Battle target picker

Drives the post-action target cursor. Parameterised on a `TargetKind` enum constraining valid targets:

| TargetKind | Allowed targets |
|---|---|
| `SingleEnemy` | One alive monster slot. |
| `SingleAlly` | One alive party slot, **excluding** the actor. |
| `SingleAllyOrSelf` | Any alive party slot, including the actor. |
| `DeadAlly` | One fallen party slot (Revive / Resurrection). |
| `AnyAlly` | Any party slot, alive or dead. |
| `AllEnemies` / `AllAllies` | Sweep target - auto-confirm. |
| `Self_` | The actor itself - auto-confirm. |

Sweep kinds resolve in `init_cursor`; single-target picks walk valid candidates with cursor-wrap and auto-skip-dead. Implementation: [`crates/engine-core::target_picker`](../../crates/engine-core/src/target_picker.rs).

`BattleSession::push_command_with_target(world, cmd, kind, actor_slot)` is the
wiring API engines drive when a command needs a target. The session charges AP
up-front, opens the picker, and stashes the command in `pending_target_command`.
When the picker resolves, `maybe_close_picker_with_world` writes the resolved
slot to `BattleActor::active_target` (the field the action SM reads at strike
time via `host.actor(actor_slot).active_target`) and admits the buffered command
into the runner queue without re-charging AP. Sweep targets write a `0xFF`
sentinel; cancellation drops the command without admitting it. Engines that
already have a `&World` borrow at picker-open time use [`open_target_picker`];
engines that need the same active-target write at open-time (sweep / self) call
[`open_target_picker_mut`].

## Encounter trigger - runtime memory layout

A pre/post encounter save pair (one frame walking the `map01` field scene; the next frame with battle just initiated, same `map01` scene) pins the runtime memory layout of an encounter trigger. The `mednafen-state diff` over `0x801C0000..0x80200000` surfaces:

| Range | Bytes changed | What it is |
|---|---:|---|
| `0x801CE808..0x801F3818` | ~133 KB | Battle overlay loaded into RAM (single contiguous region) |
| `0x801C9370..0x801C9900` | ~200-500 B | 8-slot battle actor pointer table; stride `0x60` per slot |
| `0x80083000..0x80084000` | ~600 B | Scene-bundle / sound-pool: encounter formation + BGM resolution |

The active scene-name table at `0x80084540` (CDNAME label + scene index) is **identical** between the pre-encounter and post-encounter saves - the battle is layered on top of the field scene rather than swapping it out. Engines that drive the field-to-battle transition therefore preserve the active-scene state and only resolve the formation + battle overlay.

Codified as constants in [`crates/engine-core::capture_observations::encounter_trigger`](../../crates/engine-core/src/capture_observations.rs); a disc-gated test in [`crates/mednafen/tests/real_saves.rs`](../../crates/mednafen/tests/real_saves.rs) (`encounter_trigger_diff_loads_battle_overlay`) exercises the real save bytes.

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
[`engine_core::capture_observations::battle_init_overlay`](../../crates/engine-core/src/capture_observations.rs);
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
[`engine_core::capture_observations::item_use_battle_event`](../../crates/engine-core/src/capture_observations.rs);
disc-gated test
`item_use_pair_pins_field_pack_base_flip_and_script_vm_ctx_shift`
in `crates/mednafen/tests/real_saves.rs`.

## Captured stat-growth observations

The `mednafen-state diff` toolkit ([`docs/tooling/mednafen-automation.md`](../tooling/mednafen-automation.md)) over a magic-rank-up + character-level-up save triplet pins the per-byte footprint for Vahn (party slot 0). The observed deltas inside Vahn's character record at `0x80084708` (stride `0x414`):

| Event | Offset | Before → After | Interpretation |
|---|---|---|---|
| Magic-rank up (pre → post) | `+0x08` | `0x30 → 0x3C` | flag word low byte (+12) |
| Magic-rank up | `+0x9C` | `0x09 → 0x0A` | magic-rank counter (+1) |
| Magic-rank up | `+0x10A` | `0x1B → 0x11` | low byte of `mp_max` (cast cost spent) |
| Magic-rank up | `+0x161` | `0x02 → 0x03` | spell-level array (`spell_levels[0]` +1) |
| Level-up, 4-level jump (pre → post) | `+0x00` | `0x4F → 0x73` | unconfirmed (jump +0x24 doesn't match a single-level granularity) |
| Level-up | `+0x04..+0x06` | `0x016D → 0x02DA` | u16 LE XP delta (+365) |
| Level-up | `+0x10E` | `0x3A → 0x42` | low byte of `sp_max` (Spirit, +8) |
| Level-up | `+0x11C..+0x12C` | six per-byte +1..+4 | per-stat increments at byte stride 2 |
| Level-up | `+0x130` | `0x02 → 0x03` | displayed character level (+1) |

The retail per-level growth source **is** in `SCUS_942.54`: the per-stat
98-entry curves at `DAT_800769CC` (stride `0x62`) + the parameter block at
`DAT_80076918` that selects each stat's curve row, read and applied by the
overlay level-up function `FUN_801E9504` (see
[`subsystems/level-up.md`](level-up.md#stat-gains)). The earlier writer-search
came up empty because it scanned the `magic_level_up` *display* overlay, not the
victory-path applier; the "Seru struct +0x74" hypothesis stays falsified (those
`+0x74` reads are a `0x80808080` battle-state flag the SCUS handler
`FUN_800480D8` writes, not a stat grant).
`legaia_asset::level_up_tables::growth_tables_from_scus` parses the curves +
param block; turning their bytes into a per-character
`StatGrowthCurve::PerLevel` vector is the remaining step (it needs a pre/post
level-up capture to validate the byte->gain math before wiring).

Engines populate one captured observation at a time via:

```rust
let obs = legaia_engine_core::levelup::observations::vahn_mc8_to_mc9();
let tracker = LevelUpTracker::new().with_observed_curve(0, &obs);
```

`LevelUpObservation::to_curve` produces a `StatGrowthCurve::PerLevel` vector that emits the per-level *average* inside the observed range and falls back to `StatGain::default` outside it. Implementation: [`crates/engine-core::levelup`](../../crates/engine-core/src/levelup.rs).

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

Implementation: [`crates/engine-core::equipment`](../../crates/engine-core/src/equipment.rs).

## Seru capture + spell learning

Per-character per-Seru capture-point accumulator. Each captured Seru contributes points toward a per-character spell-learn threshold (default 100); once crossed, the spell is added to the character's learned list.

`SeruDef::learnable_mask` is a 3-bit per-character mask (bit 0 = Vahn, bit 1 = Noa, bit 2 = Gala) so single-character Seru can teach only their bearer. `record_capture` is the pure resolver; `SeruCaptureSession` drives the post-capture banner sequence (`Capturing → Announcing[i] → Done`) for engines to render.

Implementation: [`crates/engine-core::seru_learning`](../../crates/engine-core/src/seru_learning.rs).

## Tactical Arts chain editor

Menu-side state machine for composing + saving Tactical Arts command chains. `ChainLibrary` holds up to 8 saved chains per character (3..=7-byte length range, matching retail). `ChainEditor` runs a 4-phase SM: `Browsing { cursor } → Editing { working } → Naming { working, name } → Done`. Engines feed picks back to `BattleRunner::push_chained_art` at battle start.

Implementation: [`crates/engine-core::tactical_arts_editor`](../../crates/engine-core/src/tactical_arts_editor.rs).

## Battle rewards composite

`World::apply_battle_loot(formation, catalog) -> BattleRewards` is the post-victory composite that turns a defeated formation into the runtime side-effects:

- Sums each `MonsterDef::exp` and distributes the total via `World::apply_battle_xp`, which splits the pool equally among the surviving party members (integer divide, remainder dropped; dead members get zero) and runs per-character level-up checks against `LevelUpTracker::xp_table`.
- Sums each `MonsterDef::gold` and adds it to `World::money` (saturating).
- For each defeated monster with a non-`None` `drop_item` and `drop_rate_q8 > 0`, pulls one byte from `World::next_rng` and compares against `drop_rate_q8 / 256`. On hit, the item id is appended to `BattleRewards::drops` and incremented in `World::inventory`.
- Returns `BattleRewards { xp, gold, level_ups, drops }` for the engine to surface as the post-battle banner ("got N XP, M gold, level up, found Healing Leaf!").

Monster ids missing from the catalog contribute zero (silently skipped) so a partially-populated catalog still drives a battle-end transition. Implementation: [`crates/engine-core::world::World::apply_battle_loot`](../../crates/engine-core/src/world.rs).

## Live gameplay loop - Field ↔ Battle in `tick`

`World::tick` drives the full Field → Battle → Field round trip itself when `World::live_gameplay_loop` is set. The flag is an opt-in: with it clear (the default), the `Field` branch runs the field VM + locomotion but never rolls encounters, and the `Battle` branch runs a single `step_battle` without applying damage or re-arming - preserving every existing caller and test that drives those externally.

With the flag set, the per-frame flow is:

- **Field tick** (`World::live_field_tick`): a *step* is the player actor
crossing into a new 128-unit collision tile (`pos >> 7`). Each step drives one
`World::on_field_step` encounter roll; `World::tick_encounter` advances the
session's `Transition` / `Grace` countdowns every frame. When the
`EncounterSession` reaches `Triggered`, `World::begin_encounter_battle` resolves
the rolled `formation_id` against `World::formation_table`, snapshots the field
actor table into `World::field_return`, seeds the battle actor table from the
formation + `MonsterCatalog` (`enter_battle_from_formation`), and flips `mode`
to `Battle`. If a battle track is configured (`World::battle_bgm`, set via
`World::set_battle_bgm`), `enter_battle_from_formation` also calls
`World::swap_to_battle_bgm`: it stashes the current field track and queues a
`FieldEvent::Bgm{sub_op: 1}` for the battle id, which the host's BGM director
cross-fades to exactly like a field op-`0x35` start.
- **Battle tick** (`World::live_battle_tick`): wraps `step_battle` with the host-side glue the retail engine performs through its render + animation systems, so the battle resolves from `tick` alone. It folds this frame's `BattleEvent::ApplyArtStrike` damage into target HP; applies a generic physical strike (`apply_basic_attack`, `damage = art_strike_damage_default(attack, defense, 16)`) on the `AttackChain → AttackRecovery` edge when no art strike did; marks zero-HP combatants dead so the SM's wipe scan resolves; clears `ADVANCE_DONE` at `AttackRecovery`; and re-arms the next party attacker at `EndOfAction`. On `StepOutcome::BattleComplete` it calls `World::finish_battle`.
- **Return** (`World::finish_battle`): on `BattleEndCause::MonsterWipe` it credits loot via `World::apply_battle_loot` (recorded in `World::last_battle_rewards`); on `PartyWipe` it raises `World::game_over`. Either way it ends the encounter session's battle (post-battle grace + suppression), restores the `field_return` actor snapshot, and flips `mode` back to `Field`. When a battle-BGM swap was active it also calls `World::restore_field_bgm`, which queues a `FieldEvent::Bgm{sub_op: 1}` for the stashed field track (or a stop, sub-op 4, if no field track was playing at encounter start) so the director cross-fades back.

### Auto-resolve vs player-driven

The battle tick has two modes.

- By **default** it auto-resolves: every turn commits a generic physical strike against the first living combatant on the opposing side, with no player choice. The whole actor table takes turns, so **monsters take turns too** - a monster turn strikes a living party member, and a party wipe ends the battle (`game_over`) the same way a monster wipe does. The strike side is chosen by the attacker's slot (`World::first_living_opponent_of`).
- When `World::battle_player_driven` is set (requires the live loop), each *party* turn instead pauses the action SM and opens a `battle_input::BattleCommandSession` (monster turns still auto-resolve) - the player picks a command from the battle command menu and a target before the strike commits. While a session is open `live_battle_tick` skips the SM advance and drives the picker from `World::input`; on confirm `World::tick_battle_command` arms `battle_ctx.{active_actor, queued_action, action_state}` plus the acting actor's `active_target` and resumes the SM. An abort (no valid target) falls back to a default strike so the loop can't deadlock. Target selection reuses the [battle target picker](#battle-target-picker).

**Turn order.** Who acts next is chosen by `World::next_combatant_by_initiative`, the port of `recompute_battle_order` (`FUN_801daba4`).

- Each living actor carries a per-turn **initiative key** (`BattleActor::init_key`, retail `+0x16c`) seeded from its SPD (`World::battle_speed`, retail `+0x164`): `init_key = speed + rand()%(speed/2 + 1) + 1` (`overlay_0897_801e23ec`; see [battle-formulas](battle-formulas.md)).
- The selector picks the living actor with the highest key (random tiebreak via `rand % tie_count`), then consumes that actor's key so the next turn picks another; once every living actor's key is spent, a new round is seeded.
- Dead actors' keys are zeroed each call (the function's first loop) so they can't be picked.
- Party SPD is seeded from each character record's live stats in `World::load_party`; monster SPD from `MonsterDef::speed` (record `stats[5]`) at battle setup.
- When **no** living actor carries SPD - the disc-free / synthetic case where speed data hasn't been loaded - the selector falls back to round-robin slot order (`World::next_living_combatant`), which keeps the synthetic loop deterministic.

All four commands - **Attack**, **Arts**, **Magic**, **Item** - are wired into the live loop. Attack opens a target cursor and commits a physical strike through the action SM. Arts / Magic / Item resolve to `Resolution::OpenArtsMenu` / `OpenSpellMenu` / `OpenItemMenu` - the command session can't run those pickers itself (they need the caster's saved chains / learned spells / live MP / inventory + party stats), so it hands off to a host-owned submenu:

- **Item** opens a battle-context `inventory_use::InventoryUseSession` on
`World::battle_item_menu` (built by `World::build_battle_item_session` from the
live inventory, with one ally row per party slot **plus one enemy row per live
monster slot**, the enemy rows tagged `TargetRow::is_enemy`). The session routes
by item: heals / cures / revives validate against ally rows, offensive items
(Bomb / capture / escape) against enemy rows, and on entering target-select the
cursor auto-positions on the first valid-side target
(`target_valid_for_effect`). On a completed use the item applies via
`World::use_item`, one copy is removed (`World::consume_item`), and a popup is
surfaced - heal-coloured for heals/revives, damage-coloured for offensive items.
`World::use_item` folds the offensive outcomes too: `DamageDealt` subtracts
enemy HP and downs it at zero, `CaptureRolled` reuses `World::resolve_capture`
(down + log id into `battle_captures`), and `EscapeRequested` sets
`World::battle_escaped` so the item tick returns to the field via
`finish_battle` (no loot).
- **Magic** opens a `battle_magic::BattleSpellSession` on `World::battle_spell_menu` (built by `World::build_battle_spell_session` from the caster's learned spells off their roster record + live MP, MP-gated). The picker kind matches the spell's `SpellTarget` shape. On confirm `World::apply_battle_spell` deducts MP once, resolves each affected slot through `spells::cast_spell` (caster magic from `World::battle_magic`, target magic-defense reusing `World::battle_defense`), and folds the outcome into the live actor table via `World::fold_spell_outcome`. All `SpellOutcome` shapes apply:
    - damage / heal / cure / revive;
    - **buffs** (`World::apply_battle_buff` writes the delta straight into the per-slot `battle_attack` / `battle_defense` / `battle_magic` scalar with refresh semantics + a per-turn timer aged in the re-arm path, reverted exactly on expiry);
    - **capture** (`World::resolve_capture` rolls vs the monster's missing-HP fraction - reliable only on a weakened Seru - downing it and logging the id into `World::battle_captures` on success);
    - and **escape** (sets `World::battle_escaped`, and the spell tick returns to the field via `finish_battle` with no loot).
    - Accuracy / Evasion / Speed buffs are tracked but have no live-loop scalar to move yet.
- **Arts** opens a `battle_arts::BattleArtsSession` on `World::battle_arts_menu`
(built by `World::build_battle_arts_rows` from `World::saved_chains` filtered to
the caster). An Art is a saved command chain; each menu row carries a per-strike
**power profile** (`Vec<PowerByte>` + `EnemyEffect`) and runs through the real
art-power path: `World::apply_battle_art` drives each power byte through
`crate::art_strike::apply_art_strike`, so the byte's multiplier tier + UDF/LDF
target decode, `resolve_battle_defense` picks the matching defense half, and the
art's status effect lands on a hit. The profile comes from a staged `ArtRecord`
(`World::art_records`, keyed by `(Character, ActionConstant)`, populated from
disc PROT entry `0x05C4` via `World::set_art_record`) when a record's command
string the saved chain ends with (`chain_matches_record`); with no matching
record it falls back to a synthetic per-direction profile
(`battle_arts::synthetic_power` - Down → LDF, else UDF, tier-0 ×12, clamped to
`MAX_ART_HITS`). Both paths share the one `apply_art_strike` kernel; the
synthetic fallback keeps a saved chain playable when the disc art tables aren't
loaded.

While any submenu is open both the SM and the command session are parked;
`World::tick_battle_{arts,spell,item}_menu` drives it from `World::input`. On a
completed action the result is applied, the relevant popup is surfaced
(`battle_hit_fx`), and the action SM is **parked at `EndOfAction`** so the
re-arm block cycles to the next combatant - a cast / art / item use is the
actor's whole turn, no Attack-SM strike fires. Backing out reopens the command
menu for the same actor. Implementation:
[`crates/engine-core::battle_input`](../../crates/engine-core/src/battle_input.rs)
+ [`battle_arts`](../../crates/engine-core/src/battle_arts.rs) /
[`battle_magic`](../../crates/engine-core/src/battle_magic.rs); coverage
`crates/engine-core/tests/battle_player_driven.rs` walks into a battle, asserts
no strike lands until the player confirms a command, then drives the picker to a
monster wipe + loot.

### Post-battle Seru learning

Capturing a monster (magic capture roll or a capture item) downs it and logs its **monster id** into `World::battle_captures`.

- `World::finish_battle` resolves these through `World::resolve_captures`: each captured monster id maps to a **Seru id** via `MonsterCatalog`'s `MonsterDef::seru_id`, and `seru_learning::record_capture` banks that Seru's capture points against `World::seru_log` for every active party slot eligible by the Seru's `learnable_mask`.
- When a slot's accumulated points cross the Seru's `learn_threshold` the taught spell id joins that character's learned list, and `World::build_battle_spell_session` unions the roster's saved spells with `seru_log.learned_spells(slot)` so a freshly-learned spell is immediately castable - no save/load round-trip needed.
- The accepted `CaptureOutcome`s are stashed in `World::last_capture_outcomes` (`drain_last_capture_outcomes`); `resolve_captures` also builds the first accepted capture into `World::current_capture_banner` (a `seru_learning::SeruCaptureSession`), the sibling of `World::current_level_up_banner`.
- `World::tick` advances the banner one frame per call and clears it when the session reaches `Done`, so it plays out over the field after the battle ends. The session's `current_banner()` yields the active line (`"Captured: <Seru>!"` then per-learn `"<char> learned <spell>!"`); the play-window renders it via `legaia_engine_render::capture_banner_draws_for`.
- `resolve_captures` always drains `battle_captures`; with an empty `World::seru_registry` (the default) it banks nothing - the monster is still downed, but no Seru is learned.
- Capture-point progress (including sub-threshold totals) persists through `World::save_full` / `load_full` as `(seru_id, points)` pairs in each `CharSaveExt::seru_captures`; reload restores the points and, with the registry installed, re-marks any over-threshold Seru as learned.
- The `MonsterDef::seru_id` mapping + `learn_threshold` / `capture_points` values are clean-room approximations (`SeruRegistry::vanilla`); pinning the real per-monster Seru attachments and capture arithmetic is gated on the still-uncaptured stat-grant table loader (see [`crate::capture_observations::battle_init_overlay`]).

The `legaia-engine play-window` host exposes both as flags:

- `--live-loop` walks-and-fights through the round trip.
- `--player-battle` (which implies `--live-loop`) makes battles player-driven and renders the party/monster HP plus the live command menu / target cursor / arts + spell + item submenus in the HUD (it installs the vanilla spell + item catalogs and, when the boot save has none, seeds a couple of demo saved chains plus a few demo items - Healing Leaf + Bomb - so the ally-heal and offensive item paths are both exercisable).
- `--battle-bgm <id>` enables the Battle↔Field music swap: the live loop cross-fades to `<id>` on encounter and resumes the field track on battle end (the id is routed through the same director as field op-`0x35` starts, so it must resolve in the current scene's BGM table - the live loop doesn't load a separate battle audio bundle).
- Without a flag, play-window keeps the legacy "explore but never fight" behaviour.

The spine began as physical-attack-only, single-formation; the Arts / Magic / Item submenus (above) and monster AI turns layer on top of it. The damage path for art-driven strikes flows through `apply_art_strike` → `fold_battle_event` in the SM-driven `battle_session` runner, and the player-driven Arts submenu reuses the same `apply_art_strike` kernel directly. Implementation: [`crates/engine-core::world`](../../crates/engine-core/src/world.rs); integration test `crates/engine-core/tests/live_loop_tick.rs` drives boot → walk → encounter → victory → return-to-field through `tick` alone with no test-side battle glue.

## End-to-end gameplay loop integration test

`crates/engine-core/tests/end_to_end_gameplay_loop.rs` stitches every gameplay-side subsystem into one cycle:

1. **Boot** - load an `LGSF` `SaveFile` (party + story flags + money + inventory) into a fresh `World` via `load_full`. `load_full` hydrates the `LevelUpTracker` per-slot level from each record's `+0x100` byte so reloads don't roll the tracker back to L1.
2. **Field walk** - switch to `SceneMode::Field`, install an `EncounterSession` keyed to `vanilla_formation_table` at saturated trigger rate, step until `EncounterPhase::Triggered`.
3. **Encounter** - drain the formation roll, populate monster slots 3..N from the `MonsterCatalog`, flip mode to `SceneMode::Battle`.
4. **Battle SM** - drive `World::tick` while applying clean-room formula damage on every `AttackChain → AttackRecovery` transition until the action SM resolves to `BattleEndCause::MonsterWipe`.
5. **Rewards** - call `World::apply_battle_loot` to credit the per-character XP / gold split, fire drop rolls, and trigger per-character level-ups; assert at least one party slot crossed a threshold.
6. **Save round-trip** - `world.save_full().write() → SaveFile::parse() → load_full()` into a fresh `World`; assert HP/MP, level, money, story flags, and inventory survived intact.

The crate ships four test variants:

| Test | Purpose |
|---|---|
| `synthetic_party_completes_full_gameplay_loop` | The default CI cycle; hand-spins the action SM with `apply_strike`. |
| `battle_session_phase_transitions_during_loop` | Smoke around the BattleSession side; verifies the session reaches `CommandInput`. |
| `battle_session_drives_action_sm_to_monster_wipe` | Drives the same loop through `BattleSession::tick` instead of `world.tick` - `push_command` → `SessionInput { start: true }` → Resolve → `BattlePhase::Victory`. The session owns the action SM during `Resolve`. |
| `real_battle_data_encounter_drives_loop` | Disc-gated: scans an early `PROT.DAT` entry for a valid `EncounterRecord` byte pattern, installs it via `World::install_encounter_from_record`, and runs the battle through to `MonsterWipe`. Closes the synthetic-formation leak in the field → battle handoff. |
| `real_psx_memory_card_save_drives_full_loop` | Disc-gated: boots the same loop from a real Legaia memory-card save block via `Party::from_retail_sc_block` when `~/.mednafen/sav/` holds a Legaia card. |

Disc-gated variants skip silently when `extracted/PROT.DAT` / the mednafen card is missing.

## See also

**Reference** -
[Battle action SM](battle-action.md) ·
[Damage / accuracy formulas](battle-formulas.md) ·
[Encounter record](../formats/encounter.md) ·
[Player battle files](../formats/battle-data-pack.md)
