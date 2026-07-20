# Battle subsystem

The battle overlay (`0898_xxx_dat`) carries the battle scene loader, the per-actor state machine, and the effect VM cluster. Loaded at RAM `0x801CE818` (same load slot as the town overlay; battle and town never coexist).

This is a large page covering both the retail reverse-engineering and the
clean-room engine systems. Use the contents below to jump to a section.

## Contents

**Retail scene + render**
- [Battle scene loader (`FUN_800520F0`)](#battle-scene-loader-fun_800520f0) - [stage-overlay dispatch](#stage-overlay-dispatch-the-0x47-loader-band) · [sparring-tutorial prompts](#the-sparring-tutorial-prompt-machine-overlay-967) · [command-flow byte](#the-command-flow-byte-ctx0x06---what-the-hook-table-indexes)
- [Battle background](#battle-background) - [ground grid](#backdrop-ground---a-procedural-flat-grid-func_0x801d02c0) · [stage stream per scene](#which-stage-stream-a-scene-fights-in) · [dome](#backdrop-dome---sky--distant-mountains-prot-88-for-map01) · [camera](#battle-camera-exact) · [party meshes](#battle-party-meshes-assembled)

**Retail battle logic + data**
- [Battle action state machine (`FUN_801E295C`)](#battle-action-state-machine-fun_801e295c)
- [Party wipe + the game-over overlay](#party-wipe--the-game-over-overlay)
- [Battle context struct](#battle-context-struct)
- [Stage seats (`FUN_800513F0` placement tables)](#stage-seats-fun_800513f0-placement-tables)
- [Range / line-of-sight (`FUN_8004E2F0`)](#range--line-of-sight-fun_8004e2f0)
- [Monster init (`FUN_80054CB0`)](#monster-init-fun_80054cb0) - [record layout](#monster-record-source-layout) · [archive (PROT 867)](#monster-archive-prot-entry-867) · [mesh](#monster-mesh-record-0x04) · [native bridge](#native-renderer-bridge-clean-room-engine) · [AI](#monster-ai-fun_801e9fd4-action-picker--fun_801e7320-target-resolver)
- [Stat aggregator (`FUN_80042558`)](#stat-aggregator-fun_80042558)
- [Battle archive (`FUN_80052FA0` / `FUN_800542C8`)](#battle-archive-fun_80052fa0--fun_800542c8)
- [Character record layout](#character-record-layout) - [why the pair order is `(max, cur)`](#why-the-pair-order-is-max-cur)
- [Battle main dispatcher (`FUN_801D0748`)](#battle-main-dispatcher-fun_801d0748) · [hottest utility (`FUN_801D8DE8`)](#hottest-battle-utility-fun_801d8de8) · [weapon trail builder](#weapon--effect-trail-builder-fun_80048310--fun_800485bc)
- [Per-frame actor maintenance (`FUN_8004CE2C`)](#per-frame-actor-maintenance-fun_8004ce2c)

**Clean-room engine systems**
- [Inventory (page-banked)](#inventory-cratesasset-page-banked-layout) · [Status effects](#status-effects) · [AP / Spirit gauge](#ap--spirit-gauge) · [Battle stat aggregator](#battle-stat-aggregator) · [Item catalog](#item-catalog)
- [Battle round lifecycle](#battle-round-lifecycle) · [command runner](#battle-command-runner) · [BattleSession Resolve driver](#battlesession-resolve-driver) · [HUD model](#battle-hud-model) · [SFX bank](#sfx-bank--scheduler)
- [Inventory item-use session](#inventory-item-use-session) · [Encounter system](#encounter-system) · [target picker](#battle-target-picker)
- [Equipment catalog](#equipment-catalog) · [Seru capture + spell learning](#seru-capture--spell-learning) · [Tactical Arts chain editor](#tactical-arts-chain-editor) · [rewards composite](#battle-rewards-composite)
- [Live gameplay loop - Field ↔ Battle](#live-gameplay-loop---field--battle-in-tick) - [auto vs player-driven](#auto-resolve-vs-player-driven) · [post-battle Seru learning](#post-battle-seru-learning)

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
  party vertex pools - [character-mesh.md § Battle form](../formats/character-mesh.md#battle-form---assembled-from-the-player-files)).
  The field pack 0874 §0 is field-only; PROT 1204 is the Baka Fighter
  default-equipment sibling pack.
- **State `0xE`** - initialises the runtime [effect 2-pack wrapper](../formats/effect.md) via `FUN_801DE914`. Also fires for the field-VM op `0x3E` warp/interact path on the system context.
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
968), where `_DAT_8007BD0C` is the formation's monster id.

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

### The sparring-tutorial prompt machine (overlay 967)

What overlay 967 *does* is emit the in-battle "how to fight" boxes of the Tetsu
sparring fight. The hook table and every prompt string address are resident in
967, and neither the battle-scene script, MES text, nor the battle overlay
`0898` carries them - which is why porting the battle SM alone never produces
the boxes. **Exclusivity itself is a corpus claim, not an instruction claim:**
what the disassembly shows is where these prompts *are*, not that no other
overlay could emit a prompt. Read it as consistent with 967-only, not as proof.

Its tick `FUN_801F6B70` is a jump-table hook on the battle **flow-state byte**
`ctx[+0x06]` (`ctx = _DAT_8007BD24`), not a linear script:

```
ctx[0x6B0] = 0                           // sh zero,0x6b0(v1) @ 0x801F6BB8
if ctx[0x6B2] != 0  -> suppressed        // bnez @ 0x801F6BB4 - a box is up
if ctx[0x6AE] != 0  -> already emitted   // bnez @ 0x801F6BC4 - one-shot latch
idx = ctx[0x06] - 0x1E                   // 91-entry table at 0x801F69D8
if idx >= 0x5B      -> no-op             // sltiu 0x5b @ 0x801F6BD8
goto table[idx]                          // jr v0 @ 0x801F6BF8
```

The `ctx[0x6B0]` clear is written first here on purpose: it lives in the
**branch delay slot** of the suppression test, so it executes on both paths -
including the suppressed one. Ghidra's C prints it after the guard, which is the
reordered-store artifact.

Only **nine** of the 91 slots are live - flow states `30, 40, 50, 60, 80, 90,
100, 110, 120`; the other 82 point at the shared no-op tail `0x801F718C`. The
table decodes straight out of the disc image: it begins at overlay file offset
`0`, since its base `0x801F69D8` *is* the overlay load base.

Each live handler then switches on `ctx[+0x28A]`, the same byte the
battle-action SM's `case 0xFF` increments (ported as
`World::advance_battle_mode`), which the tutorial reads as the **lesson index**:
`0` attacks, `1` items, `2` spirit, `3` hyper arts, `4` → done. The script is
therefore a `(flow state × lesson)` cross-product, with a "you're learning about
X now! Try again!" rewind (`FUN_801F7628`) whenever the player picks the action
the current lesson is not teaching.

| Flow state | Handler | What it prompts |
|---|---|---|
| `30` | `0x801F6C00` | Turn start - the per-lesson intro, plus a first-visit vs repeat-visit input explainer selected by `_DAT_801D46C8`. |
| `40` | `0x801F6CB8` | `[Begin]` chosen - name the category to pick. Lesson 3 has no prompt here. |
| `50` | `0x801F6CAC` | Run selected - always rejected, always rewinds. |
| `60` | `0x801F6DCC` | Item window opened - the item lesson explains the two windows; every other lesson rewinds. |
| `80` | `0x801F6E4C` | Arts command-entry screen - combo hint (lesson 0) or the drill instruction (lesson 3). |
| `90` | `0x801F6EE4` | Target select; for lesson 3 it first validates the entered command buffer. |
| `100` | `0x801F7060` | Target confirm - unconditional, lesson-independent. |
| `110` | `0x801F7088` | Validates the committed `actor[+0x1DE]` category against the lesson (`3` attack, `1` item, `4` spirit; hyper arts expects `3`, since it is reached through Attack). |
| `120` | `0x801F6D30` | The Auto / Command attack-mode prompt - free choice for lesson 0, forced `[Command]` for lesson 3. |

The hyper-arts drill at flow state `90` asks for `[High] [Low] [High]`
(`0x0F, 0x0E, 0x0F`) and accepts it at three alignments of the command buffer
`actor[+0x1DF..=+0x1E3]`, each a differently-masked load at `0x801F6FD8`. When
`_DAT_801D46C4 == 1` the buffer is auto-filled for the player at `0x801F6FB0`.

The completion tail `0x801F7380` fires once `ctx[0x28A]` reaches `4`: it bumps
the lesson to `5`, writes `ctx[0x06] = 0xC8` (`0x801F73DC`) and `ctx[0x07] =
0xFF` (`0x801F73E8`) to close the fight, and emits the sign-off box.

The tail opens on an idempotence guard the C flattens away. At
`0x801F7390..0x801F73B4` an `sltiu ctx[0x28A], 5` skips ahead when the lesson is
still below `5`; a lesson **already** at or past `5` re-pins it to `5` and
re-issues the same `0xC8`/`0xFF` close writes before reaching the `== 4` arm. So
the close is safe to re-enter, and `5` is a terminal value rather than a
one-frame transient.

**Box placement.** The emitter `FUN_801F747C(text, style)` takes a style index
`0..=9` into a jump table at `0x801F6B48`. `x` is either the fixed left margin
`0x10` or centred at `0xA0 − width/2`; `y` is either the fixed top `0x0E` or
bottom-anchored at `base − (lines × 14 − 4)` for `base` in `{0x9A, 0xB0, 0xCC}`.
Styles `0, 1, 8, 9` do not wait for acknowledgement; `2..=7` do.

Engine port: [`engine-core::battle_tutorial`](../../crates/engine-core/src/battle_tutorial.rs).
The prompt **text is Sony data living in the overlay**, so the port commits only
the string *addresses* and reads the text off the user's own disc at runtime
(`BattleTutorialScript::from_overlay` / `::from_prot`) - the same rule the item /
spell / dialog parsers follow. Disc-gated oracle
`crates/engine-core/tests/battle_tutorial_disc.rs`.

### The command-flow byte `ctx[+0x06]` - what the hook table indexes

The hook key is **not** the action SM's `ctx[+0x07]`. It is `ctx[+0x06]`, the
cursor of the *other* battle state machine: the menu half, `FUN_801D0748`. Both
are byte cursors over the same context struct, and their value spaces collide -
`ctx[7] == 0x64` is `RunBegin`, `ctx[6] == 0x64` is target confirm.

**They do not share a dispatch shape, and it is worth not carrying the opposite
forward.** `FUN_801D0748` has no jump table at all: it dispatches `ctx[+0x06]`
through a binary-search `beq`/`slti` comparison tree at
`0x801D0C84..0x801D0DC8`, and the only `jr` in its 2781 instructions is the
`jr ra` at `0x801D32B4`. The `jr`-table shape belongs to the tutorial hook
`FUN_801F6B70` (`jr v0` at `0x801F6BF8`) and to the action SM `FUN_801E295C`,
not to the menu SM. Reading the menu half as table-driven invents a dense index
space it does not have - its live cases are exactly the 22 constants below,
everything else falling to the default at `0x801D3290`.

Below `0x1E` the command flow is battle entry and turn setup: `0x00` init,
`0x0A`/`0x0B` the intro timer at `ctx[+0x6D6]`, `0x14` turn start (which opens
the top menu and falls into `0x1E`). From `0x1E` up it is the player's command
selection, and the states are regular decimal multiples of ten:

| `ctx[+0x06]` | Handler | On screen | Leaves to |
|---|---|---|---|
| `0x1E` = 30 | `0x801D102C` | `[Begin]` / `[Escape]` turn prompt | `0x28`, `0x32`, `0x6E` |
| `0x28` = 40 | `0x801D1188` | Action-category menu | `0x1E`, `0x3C`, `0x46`, `0x5A`, `0x6E`, `0x78` |
| `0x32` = 50 | `0x801D10F8` | Flee confirm | `0x1E`, `0xFE` |
| `0x3C` = 60 | `0x801D17DC` | Item window | `0x28`, `0x5B`, `0x5D`, `0x64` |
| `0x46` = 70 | `0x801D19F8` | Magic window | `0x28`, `0x5C`, `0x65`, `0x67` |
| `0x50` = 80 | `0x801D1D84` | Arts command-entry screen | `0x28`, `0x5A`, `0x78` |
| `0x5A` = 90 | `0x801D21CC` | Target cursor | `0x28`, `0x50`, `0x6E`, `0x78` |
| `0x64` = 100 | `0x801D2A00` | Target confirm (item window's own) | `0x28`, `0x3C`, `0x6E` |
| `0x6E` = 110 | `0x801D3024` | All members committed - begin | `0x1E`, `0x28`, `0xFE` |
| `0x78` = 120 | `0x801D16E8` | Auto / Command attack-mode prompt | `0x28`, `0x50`, `0x5A` |

**How to read the "Leaves to" column.** It is the exhaustive set of
`sb <reg>,0x0(s3)` stores inside each handler's address range (`s3 = ctx+6`,
loaded at `0x801D0780`), resolved by constant propagation over the `li` / `move`
/ `clear` that feed the stored register - not a per-branch narration. Every
handler can also fall through without storing, which is the implicit "stay put".
Two earlier readings do not survive that sweep: state `0x28` never stores `0x50`
(Attack reaches the arts screen via the `0x78` attack-mode prompt), and state
`0x46` never stores `0x6E`. Both were nested-`if` renderings, not stores.

Above the selection band sit the per-window target sub-cursors. They are two
disjoint runs, `0x5B..0x5E` and `0x64..0x67` - there is no case for
`0x5F..0x63`, and treating the sub-cursors as one contiguous `0x5B..=0x67` range
invents five states. `0xFE` is a real dispatched case ("round armed - run the
action SM"). `0xFF` (idle) is **not**: no comparison tests for it, so it reaches
the default at `0x801D3290` like every other unlisted value - idle by falling
through rather than by being handled.

That band is what pins the tutorial's table. Its nine live slots are exactly
these ten states **minus the magic window** - the sparring fight teaches attacks,
items, spirit and hyper arts, and never magic. Engine mirror
[`engine-core::battle_flow`](../../crates/engine-core/src/battle_flow.rs), which
carries that cross-check as a test.

### How the engine raises the flow state

The engine splits what `FUN_801D0748` does in one machine across a
[`battle_input::BattleCommandSession`](../../crates/engine-core/src/battle_input.rs)
plus host-owned Item / Magic / Arts submenus, so the flow byte is *recomposed*
each frame by `battle_flow::flow_state_for` (an open submenu wins over the
command phase). Three points differ from retail and are deliberate:

- **Turn prompt.** The engine has no separate `[Begin]` screen, so
  `World::open_battle_command` raises state `30` directly for the frame a turn
  opens - the same instant retail enters `0x1E`.
- **Target confirm.** `CommandPhase::Confirmed` is the Attack path, which retail
  routes `0x5A → 0x6E`; state `100` is the item window's own target step and has
  no engine hook point yet.
- **Lesson counter.** Retail shares `ctx[+0x28A]` with the action SM, where the
  sparring fight's scripted `case 0xFF` bumps it. The engine has no script driver
  for that fight, so `BattleTutorial::pending_advance` bumps the lesson when the
  commit hook *accepts* the taught category - one lesson per successful player
  turn, which is the same observable cadence.

A queued box parks the whole battle tick (`World::live_battle_tick` returns
early), which is the port of retail returning before it reads the flow state
while `FUN_801D9BBC` reports a box up (`ctx[+0x6B2]`). A hook that takes the
rewind exit discards the action and reopens the command menu. Hosts arm the
machine with `World::prime_battle_tutorial`, the stand-in for retail's stage-id
dispatch; `legaia-engine play-window --player-battle` primes it in `town01`
(`LEGAIA_BATTLE_TUTORIAL=1`/`0` forces it either way for hand-testing).

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
- **Texture address (constant in the overlay, content per scene).** The grid
  quads sample a **4bpp texture page at framebuffer `(832, 0)`** (tpage attr
  `0x000D`) with **CLUT `(0, 479)`** (CBA `0x77C0`), UV window
  **`(192..255)²`** - scratch literals in `func_0x801d02c0`, confirmed
  against the GT4 packets in the live prim pool of the Tetsu battle states.
  The 64² window holds **four 32×32 sub-tiles** (two distinct variants, each
  duplicated across the row); each cell samples one sub-tile with a
  per-cell random corner mirror. The *address* is scene-independent - the
  scene's battle VRAM build is what places that scene's own ground tile
  there (`town01` = warm sandy pebbles; an earlier engine heuristic that
  borrowed the dome's nearest "grass vertex" sampled a blue texel region in
  `town01` and painted the floor sky-blue). Engine mirror:
  `build_battle_ground_grid` in `play-window`.
  The historical overlay capture filed under the `0896` label (a mislabeled
  slot-A window image; PROT 0896 itself is neither the battle background nor
  an overlay that loads here) shows the same grid renderer + `_DAT_8007b814`
  buffer - it is battle-overlay code seen through that capture.

> **Correction.** An earlier reading called the backdrop the *world-map continent
> heightfield* per a `prim-trace` "3715 hits in `0x80190000`". That was a **false
> positive** (3 degenerate `clut=0` `POLY_FT4` prims stride-1 flooding that
> window). The ground is this **flat procedural grid**, not a per-tile continent
> descriptor table read from RAM, and not a 3D heightfield (cell `Y ≈ 0`).

### Which stage stream a scene fights in

A scene bundle is a fixed slot array - `.MAP`, v12 table, event scripts, asset
table, texture pack, then **one `scene_tmd_stream` per sub-area**. The battle
backdrop is whichever of those streams the type-`0x01` chunk walker
`FUN_8001FE70` last recorded in `_DAT_8007B864` (its sole writer, at
`0x8001FEC0`), so the choice is scene data, not a code table - and it is **not
uniformly the block's first stream**:

| Scene | Bundle slot | Extraction entry | Dome shape | Pinned from |
|---|---|---|---|---|
| `map01` (overworld) | 5 | 88 | 4 objects, 340 verts | the four camera-orbit angle saves |
| `town01` (Rim Elm) | 6 | 7 | 2 objects, 341 verts | the three Tetsu tutorial anchors |

Rim Elm's bundle carries four sub-area backdrops (entries 6..9); the Tetsu
sparring match is fought in the **second**. Each row is pinned by reading
`_DAT_8007B864` in a battle save state, taking object 0's live vertex pool, and
byte-matching it back to a PROT entry.

> **Over-read trap.** PROT extraction over-reads into the following entries, so
> the Rim Elm dome's bytes also appear inside entry **6**'s file - at offset
> `0x16038`, past entry 6's own `(next_lba - lba) * 0x800 = 0x14000`. Any "scan
> the block for the resident dome" sweep must reject hits beyond an entry's
> unique length or it will attribute the backdrop one entry too low. Entries 7
> and 8 additionally share a vertex *count*, so shape alone cannot separate them
> either - only the bytes can.

Engine mirror: `ProtIndex::battle_stage_entry_for_scene`, consumed by
`play-window`'s `build_battle_stage`. Tests
`crates/engine-core/tests/battle_stage_entries_real.rs` (disc) and
`crates/engine-shell/tests/battle_stage_live.rs` (save library).

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

The half-shape holds per scene, along different axes: `town01`'s stage TMD
(extraction 7) is a **half arena authored entirely at `X ≥ 0`** (obj0
`X ∈ [0, 10751]`, `Z ± 10751`), open side facing `-X` - the sea horizon in
the retail Tetsu close-up. Only **object 0** (the arena shell) is on screen
in the retail captures; object 1 (a small ground-level ribbon of near
props / ground mist) never is.

**Engine status.** `legaia-engine play-window` renders stage battles as a
faithful scene: the phase-scripted camera (below), the stage TMD's object 0
drawn **once** at raw coords (an earlier build added a `Ry(180°)` mirror
copy to "complete the circle"; for `town01` that planted a duplicate
village wall across the open sea side, so the mirror is removed - one
instance, like retail), the flat tiled ground grid under the actors (the
`func_0x801d02c0` grid + constant texture address above), a sky-blue clear
so the open horizon reads as sky, the real **assembled** battle party (see
below), and animated monsters. Monster actors compose a half-turn so they
face the party (`-Z` from the `+Z` seats - the retail Tetsu dialogue
close-up shows the monster's face while the archive meshes rest facing
`+Z`). The actors draw through the exact `tr.z = 7680` camera with the
retail **4× actor world scale** composed under the rotation (see below) -
the battle meshes are small (party 134–284 units, monsters 77–368), and the
4× base is what makes them read at retail size against the deep
translation.

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
decrements it by `DAT_1f800393 * 2` ≈ 4 units per camera step while idle -
one step per 2 vsyncs, i.e. -120 units/s), `roll = 0`,
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

**Phase-scripted framings + glides.** The projection above is the fixed part;
the *pose* (pitch / yaw / TR) is **phase-scripted with glides**, not a single
orbit. Pinned per-frame from a PCSX-Redux camera trace on the
`s5_tetsu_battle` anchor (logging the rotation trio `0x8007B790` + the
translation trio `0x800840B8` every vsync), cross-checked against the
catalogued mednafen Tetsu battle states; one camera step spans **2 vsyncs**:

| Phase | pitch | yaw | TR | motion |
|---|---|---|---|---|
| tutorial dialogue up | 0 | 0 | `(0, 1280, 1638)` | held static |
| dialogue dismiss | 0→32, `+6`/step | orbit resumes | z 1638→7680, `+864`/step | rate-clamped glide |
| Begin/Run menu | 32 | free | `(0, 1280, z)` | idle orbit `-4` yaw/step |
| command submenu | 32 | **2288** | `(-512, 1152, 2457)` | 6-step glide in, then held |
| submenu exit | swings 32→256→32 | eases to 0 | via `(0, 1536, 3276)`, back to menu TR | 6-step swing + 7-step return |

`H = 256` and the identity·16384 base hold through every phase. The traced
numbers above are one fight's *instance* of two formulas, not constants: the
submenu yaw `2288` is `0x8F0 - actor_facing` and the menu depth `z` is the
formation-sized `max(span * 3, 0x800)`, which lands on `7680` for the solo
Tetsu seats. Per-seat variation lives in the **focus trio**, which a solo
trace cannot distinguish from a constant. Both framing laws, the per-character
height table `0x801F4D2C`, and the focus trio are covered under
[`battle-action.md`](battle-action.md#case-0---the-submenu-close-up-framing).
Engine mirror: `window/battle_cam.rs` in `play-window` (phase derived from the
live dialogue / command-session state, stepped on the retail display-frame
clock), with the glide-table kernel port at `legaia_engine_vm::battle_camera`
(`FUN_801D829C`).

**Actor pass: the 4× world-scale base matrix.** The battle base matrix
`DAT_8007BF10` holds `16384 * I` (GTE `4096` = 1.0 → a **4.0× uniform
scale**), in RAM across every catalogued battle savestate and at every orbit
angle (a pure diagonal at all four yaws, so it is a *base*, not the composed
rotation - the composed view matrix lives in GTE scratch `0x1F8003C8`). The
actor render `FUN_80048A08` multiplies that camera matrix per actor
(`FUN_8005B3A8(&DAT_1f8003c8, ...)` with the actor's `+0x24` rotation trio,
GTE TR from the actor's `+0x2C` view-translation trio), so the actors - and
their stage translations - draw at 4× under the same `Rx(32)·Ry(yaw)` /
`TR=(0,1280,7680)` / `H=256` camera the backdrop uses at 1×. The 4× is what
makes the small battle meshes read at retail size against the deep
translation (`256 * 4*370 / 7680` ≈ 49 px for a 370-unit monster). The
engine mirrors the split in `play-window`: the dome + grid draw through the
exact 1× `retail_battle_mvp` projection, and the actor + battle-FX draws
compose `BATTLE_WORLD_SCALE = 4.0` under it.

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

## Party wipe + the game-over overlay

The wipe **detection** is pinned; the retail **destination** is not, and
the two should not be conflated.

Detection is the `0x5A` end-of-action gate of the action SM (see
[battle-action.md](battle-action.md)). It walks the actor pointer table
counting party actors that are alive (`+0x14C != 0`) and not
counts-as-defeated (`+0x16E & 4`, e.g. Stone). With no survivor it sets
the battle-end signal `DAT_8007BD71 = 0xFE` and the wipe cause
`_DAT_8007BD2C = 5`; the mirror-image monster scan sets cause `0`.

The battle-exit mode selector is `FUN_80046A20` (SCUS, `0x80046A20`).
Its three `game_mode` stores pick between `0` (debug-battle id set),
`0x18` / mode 24 OTHER (arena / Muscle Dome, `_DAT_8007BAC0 & 0x100`)
and `2` / mode 2 MAIN INIT, i.e. back to the field. It **never reads
`_DAT_8007BD2C`** - the wipe cause is consumed only by
`FUN_801D5854` (battle-camera framing) and `FUN_8004E568`. So on the
statically-reachable exit path, a party wipe returns to the field like
any other battle end.

A game-over screen nevertheless exists as real disc content. Mode-table
rows 18 / 19 (table at `0x8007078C`, 0x18 stride) hand off to
`FUN_80025B30`, which loads **PROT 0902** at base `0x801CE818` with its
entry at `0x801CE844`. The overlay carries the source path
`h:\prot\field\gameover\gameover.pak`, 29 TIMs (the artwork), a
self-advance to mode 19 and a **single, unconditional** exit that writes
`game_mode = 0`.

Two things follow. First, retail's game over is **not a menu**: 0902's
only readable string is `GAME OVER`, it has no Continue / Retry / Quit
vocabulary, and one exit store cannot express three outcomes. The port's
three-row `engine-core::game_over::GameOverSession` is therefore an
**engine invention**, not a port of retail behaviour, and it is
deliberately left unreachable rather than wired to a trigger nobody has
pinned. Second, the mode-18 entry has no static writer anywhere on the
disc: a scan of every `sb`/`sh`/`sw` to `game_mode` across
`SCUS_942.54` and every PROT entry finds the value `0x12` written
nowhere, no mode-table `next` field chains into 18, and the only
`jal 0x80025B30` is inside `FUN_80025B30` itself. That 0902 exits to
mode 0 - the **debug menu** - is further circumstantial evidence that
the 18/19 pair is a dev harness.

What that scan cannot rule out is the nine register-indirect
`game_mode` stores, which remain the search space. Resolving them is a
runtime question rather than a static one; see
[open-rev-eng-threads.md](../reference/open-rev-eng-threads.md).

Mode numbers are decimal in these docs and hex in the dumps, which is a
standing trap here: `_DAT_8007B83C = 0x18` is mode **24** (OTHER /
minigame), not game over. Game over is `0x12`. Relatedly,
`extracted/PROT/0002_gameover_data.BIN` is *not* game-over art - the +2
CDNAME filename shift makes it town01's table.

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
| `+0x34` / `+0x38` | i16 | Current world X / Z (Y in the adjacent halfwords `+0x36`/`+0x3A`; `0` on the flat stage). |
| `+0x3C` / `+0x40` | i16 | Stamped with the authored stage seat at setup (`FUN_800513F0` copies the seat here, then into `+0x34`/`+0x38`); read as the b-actor position by `FUN_8004E2F0`. Live captures show it diverging from the seat mid-battle, so its steady-state role (approach target / delta anchor) is not fully pinned. |
| `+0x4A` | u8 | Magic-slot count. |
| `+0x4C` | int* | Spell-entry pointer array (each entry: `[u8 spell/action id, …, u8 AGL (action) cost @ +0x74]`). |
| `+0x14C..+0x152` / `+0x172..+0x174` / `+0x150..+0x158` | u16 | HP / MP / current / max - three-way mirror layout. |
| `+0x1BC..+0x1BE` | u8 | "Show damage" overlay byte triplet. |
| `+0x1DF` | u8 | Monster size byte (read from a monster record at `+0x1F` and stored here at init). |
| `+0x1EF..+0x1F3` | u8 | Per-element spell-slot index (from the spell ids `2,3,4,5,0xB`). |
| `+0x230` | u32 | Attack-effect / animation data pointer (set from record `+0x04`; **not** XP/drop). |

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

Save-state validation: seven battle library captures (the four camera-orbit angle saves, the three Tetsu tutorial anchors) read the count-1 seats byte-exactly at actor `+0x34`/`+0x38` (`(0, -800)` vs `(0, +800)`); the full-party capture reads the count-3 rows under a uniform `+13` Z scene offset (mid-battle drift on both sides equally, leaving the authored values unambiguous).

Engine mirror: [`engine-core::battle_seats`](../../crates/engine-core/src/battle_seats.rs) (consumed by `World::enter_battle`).

## Range / line-of-sight (`FUN_8004E2F0`)

`FUN_8004E2F0(actor_a_id, actor_b_id) -> i16 distance` is the canonical battle range check, called 5+ times from the per-actor state machine. Reads `[DAT_801C9370 + id*4]` for both actors, computes a euclidean distance from `+0x34/+0x38` (or `+0x3C/+0x40` for the b-actor), then sums the two `+0x1F` size bytes (party-member size table at `0x80078878`, monster size byte read from the live actor) to get the hit radius. Final value is clamped to a per-actor cap and `0xF` per `param_2 < 3` party tier.

## Monster init (`FUN_80054CB0`)

Called from `FUN_800542C8` (secondary battle archive loader). Populates a battle-actor at `[DAT_801C9370 + (slot+3)*4]` from a monster record:

- HP / MP / AGL triplets at `+0x14C..0x158` and `+0x172..0x174` (AGL = the agility / action gauge at `+0x154/+0x156`).
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
| `+0x0E` | u16 | **AGL** → actor `+0x154/+0x156` (agility / action gauge, cur+base; spent per action, reset each round; "Power Up" raises it - *"agility increased!"*). |
| `+0x10` | u16 | **MP** → actor `+0x150/+0x152/+0x174`. |
| `+0x12` | u16 | **ATK** → actor `+0x158/+0x15A` (attacker offense in the damage routine). |
| `+0x14` | u16 | **UDF** (upper defense) → actor `+0x15C/+0x15E` (defender defense, high facet). |
| `+0x16` | u16 | **LDF** (lower defense) → actor `+0x160/+0x162` (defender defense, low facet). |
| `+0x18` | u16 | **INT** → actor `+0x168/+0x16A` (magical damage / magic defense in the summon/arts kernel + the accuracy/evasion seed; the bestiary INT column. Meth962: INT "affects your magical damage and defense against other magical spells"). |
| `+0x1A` | u16 | **SPD** → actor `+0x164/+0x166` (turn-order initiative seed; buffable). |
| `+0x1F` | u8 | **Size class** - body bulk. Read **record-direct** through the same `0x801C9348` pointer table, never copied to the actor: the battle camera's per-action framing `FUN_801F0348` computes `ctx+0x6D0 = clamp(size << 7, 0x0C00, 0x1400)` and the enemy stager `FUN_800513F0` writes `actor+0x58 = size << 5`. Spans `14..=48` across the roster with no zero and no outlier, and it tracks model bulk rather than any stat - Lapis is 64800 HP at size class `20` against Koru's `48`, so a byte tracking HP could not produce the column. Parser: `MonsterRecord::size_class`. |
| `+0x21` | u8[3] | **Magic-attack ids** (`+0x21..+0x23`): up to three **global** spell ids the enemy casts. A slot is live when its value is `> 1`. The AI spell picker `FUN_801E9FD4` (`overlay_0898`) reads `record[0x21 + slot]`, writes it into the live actor at `+0x1DF`, and the battle-action SM names it via `&DAT_800754D0 + id*0xC` (`0x27` → `Tail Fire`). These global ids are **distinct** from the local `+0x4C` entry ids (which only gate the AGL cost); they are the names that appear on screen. Parser: `MonsterRecord::magic_attacks` + `legaia_asset::spell_names`. |
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

Both profiles boost; only the magnitude differs, so the raw record always understates the fight. Profile **B** (the gate-set branch) is what a live international-retail capture reproduces byte-for-byte (Gaza Sim-Seru id 166: raw `[AGL 128, ATK 288, UDF 222, LDF 200, INT 220, SPD 146]` → in-battle `ATK 360, UDF 444, LDF 400, INT 247`), and is what the curated `enemies.toml` bestiary holds. `MonsterRecord::battle_stats()` returns profile B. This cross-region difficulty difference (international retail hitting harder than the raw record / the Japanese release) was first surfaced by **Zetopheonix**.

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

1. Clamps each character's stat fields to a per-field ceiling. It is a **ladder, not one blanket `0x3E7`**: at `0x80042C0C..0x80042CE0` the caps are `+0x104` → `9999`, `+0x108` → `999`, `+0x10C` → `100`, `+0x110` → `280`, then `999` each for `+0x112/+0x114/+0x116/+0x118/+0x11A`. Only the maxima are capped; the paired currents are handled by the clamp triple that follows ([pair order ↓](#why-the-pair-order-is-max-cur)).
2. ORs the character's "active abilities" 16-byte block at `+0xF4..0x100` into a global 4×u32 bitmask at `0x80074358..0x80074368`. This is the "currently-active accessory effects" register read by every other game system.
3. For each character, calls `FUN_800432BC` / `FUN_80042DBC` to add/remove temporary spells per the active spell-slot layout at `+0x2B0`.

The 4-u32 global ability bitmask is what tells the renderer to draw "auto-counter" / "regen" / "magic up" indicators and what tells the battle dispatcher to apply post-hit effects. The read-side primitive is `FUN_800431D0(bit_id) -> bool` - `(&DAT_80074358)[bit_id >> 5] & (1 << (bit_id & 0x1F))`. It's a 6-instruction hot helper cited from most damage / status code paths (the action validator `FUN_8003FB10` does **not** call it - see [battle-action.md](battle-action.md#action-validator-fun_8003fb10)), so a clean-room port models it as `BattleState::ability_active(u8) -> bool`.

`FUN_800349EC` and `FUN_80035EA8` are the HP / MP threshold UI classifiers - given a character index they compare current vs max and return one of `2` (dead/zero) / `6` (low) / `7` (warn) / `9` (healthy). The dialog renderer keys text colour on the result.

`FUN_8003FB10` is the **per-slot target-validity walker** that decides which slots a queued action may target. It dispatches the arm byte through an 18-arm jump table (bound `0x84`); each arm tests per-slot HP/MP quads (battle-actor table `DAT_801C9370` in battle, char records `0x80084708 + n*0x414` in field), record stats, party-slot indirection, system flags (`FUN_8003CE64`), or the inventory-count leaf `FUN_80046898`, writing per-slot validity bits. It does **not** consult the ability bitmask (`FUN_800431D0`) - see [battle-action.md](battle-action.md#action-validator-fun_8003fb10) for the full arm map and the engine port (`engine-vm::battle_action::validate_action`).

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

11 KB / 182 calls. The top of the per-frame battle loop. Routes through every active battle subsystem (rendering, AI, animation, hit detection).

## Hottest battle utility (`FUN_801D8DE8`)

3 KB / 77 incoming refs. The single most-cited battle helper - likely a per-actor utility that every state arm bottoms out into.

## Weapon / effect trail builder (`FUN_80048310` + `FUN_800485BC`)

Visual-only helpers that build the swept geometry behind a moving battle actor (sword trails, dash plumes, particle ribbons). `FUN_80048310` iterates the 16-slot per-actor frame buffer at `actor[+0x68]`, copies vertex triplets from the per-actor pose pool at `gp[0xa0c] + 0x6f4` (stride `0xC`), and calls `FUN_800485BC` twice - once for the outline, once for the base - blending two endpoint colours over N steps via a `0..N` gradient loop.

`FUN_800485BC` is a 275-instruction quad-strip emitter. It looks up the actor pose from `*(int*)(0x801C9370 + actor[+0x5A]*4) + 0x34/+0x38` (re-confirms the battle actor pointer table), reads sin/cos LUTs at `_DAT_8007B81C` / `_DAT_8007B7F8` keyed on `actor[+0x26] & 0xFFF` (a 12-bit angle **mask**, not a multiply - the LUTs are 4096-entry, `s16` 1.12 fixed point), runs each vertex through `FUN_800195A8` for GTE projection, and drops `0x3B808080` packets into the OT.

That code word is a **`POLY_G4`** - four-point gouraud, semi-transparent, *untextured*: the texture bit `0x04` is clear, and `0x808080` is a neutral placeholder colour the fill immediately overwrites. Vertex products carry a `+0xFFF` bias when negative before the `>> 12`, emulating round-toward-zero, and the OT slot is the average of the four corner depths with the same fixup.

These are pure rendering helpers - no gameplay state changes. Engine reimpl can defer them until visuals matter.

## Per-frame actor maintenance (`FUN_8004CE2C`)

The SCUS-resident per-frame sweep over the battle actor table - one of the
largest SCUS functions with no static caller (it is reached from the battle
tick). Three sequential passes over `DAT_801C9370`, bounded by the actor count
byte `*(_DAT_8007BD24)[0]`:

1. **Status-flag reconcile.** For each actor, walks the element/condition word
   in the `0x80084140`-region record and clears matching condition bits in the
   actor's status halfword at `+0x16E` (masks `0x0001`/`0x0003`/`0x0078`/
   `0x1000`/`0x0004`/`0x0400`), i.e. "expire conditional status effects".
2. **Action reaction.** Resolves the active actor's current action id via
   `actor+0x22C → +0x4C → +0x77` (the shared spell/move id space of
   [`move-power.md`](../formats/move-power.md)) and per action band seeds
   animation timers and effect pointers from the overlay globals
   `_DAT_801F53D4` / `_DAT_801F53D8` into `actor+0x4`, `+0x21B..+0x21F`;
   the `action == 24` low-speed arm calls overlay `FUN_801E1D98`.
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
   affliction, not a per-frame damage flash.

Calls the actor-spawn/move-VM invoker `FUN_80021B04` and helpers
`FUN_8004FE5C` / `FUN_800583C8` / `FUN_80031D00` / RNG `FUN_80056798`.
Despite its size and shape it is **not a mode dispatcher**: the master mode
word `_DAT_8007B83C` never appears; every global it touches is battle-domain.
`see ghidra/scripts/funcs/8004ce2c.txt` (`0x8004CE30` is the function's second instruction, not its entry).

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

`EncounterTable` holds the per-scene rows + 1/256 trigger rate + safe-zone rectangles. The accessory / status modifiers scale the effective rate multiplicatively via `EncounterTracker::set_rate_modifiers` - the statically pinned `FUN_801D9E1C` shifts (High Encounter passive `0x3B` = `<<2`, Low Encounter `0x3C` = `>>1`, system flags `0x1D`/`0x1E` = `<<1`/`>>1`; see [encounter.md](../formats/encounter.md#random-encounter-trigger-path)), refreshed from the party ability mask + flag bank each step. (An earlier additive `add_rate_bias` knob modeled accessories that don't exist in retail; it is removed.)

Implementation: [`crates/engine-core::encounter`](../../crates/engine-core/src/encounter.rs).

### Scripted-battle entry (`3E FF <row>`)

The scripted boss fights enter through the field-VM interact op `0x3E` with
`op0 = 0xFF`: the case-0x3E interact arm (`FUN_801DE840`, field overlay) sets
the SYSTEM entity's 5-state SM to Activating (`sys_ctx[+0x8A] = 1`), points its
encounter-record slot at the per-scene MAN formation-table row `op1`
(`sys_ctx[+0x94] = *(ctrl+0x20) + op1 * *(ctrl+0x5D) + 1`), and requests the
battle mode switch (`FUN_8003CE08(0xE)`); the entity tick `FUN_801DA51C`'s
confirm state then copies the row into the battle formation cell `0x8007BD0C`.
The boss rows sit **outside** every region's rollable
`[base, base + count)` slice, so they can only enter through this op, and they
carry a non-zero first header byte (the reader ORs `0x80` into a battle-setup
flag for them):

| Scene | Beat record | Op | Formation row | Contents |
|---|---|---|---|---|
| `garmel` | `P2[12]` (C1 gate `[0x198]`, self-latching) | `3E FF 09` | 9 | lone **Zeto** (`0x4B`) |
| `garmel` | `P2[11]` (C1 gate `[0x195]`) | `3E FF 08` | 8 | lone **Songi** (`0x4C`) |
| `rikuroa` | `P1[3]` (the Caruban stager, after its `52 89` marker SET) | `3E FF 11` | 17 | lone **Caruban** (`0x49`) |

This dissolves the "boss battle-id global" hypothesis for these fights: the
formation is the scene's own MAN encounter-section row, selected by index from
script bytes (live-capture pinned for Zeto - the formation writer `ra` sits in
`FUN_801DA51C`'s record-copy body while `0x8007B7FC` stays silent).

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
reached from the field-VM host's `field_interact` arm when `op0 == 0xFF`. The
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
`+0x74` reads are a `0x80808080` battle-state flag the SCUS handler
`FUN_800480D8` writes, not a stat grant).
`legaia_asset::level_up_tables::growth_tables_from_scus` parses the curves +
param block, and the engine applies them: `LevelUpTracker::with_growth_tables`
installs per-character `StatGrowthCurve::PerLevel` (all 8 stats) at boot,
byte-validated against the captured Noa L2->L3 single-level deltas
(see [`level-up.md`](level-up.md#stat-gains)).

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
- **Post-battle script re-entry** (`SceneHost::tick`): retail reloads the field scene after every battle, re-running the scene-entry system script `P1[0]` (`FUN_8003ab2c`).
The host mirrors that on the `Battle -> Field` mode edge by reloading the entry script (`Scene::field_man_entry_script` -> `World::load_field_script_at`).
This re-run is what dispatches post-battle beat records: rikuroa's `P1[0]` tests the transient staged marker `0x289` (SET by the stager `P1[3]`'s own `52 89` script bytes when the approach dispatch ran the record pre-battle)
and issues the op-`0x44` spawn of the post-victory record `P2[50]` through the C1-gated dispatch - whose own script bytes SET the progression gate `0x142`.
No engine code writes the gate flag or the marker (there is no victory latch and no battle-entry stamp); both land from record execution. Disc-gated oracle: `engine-core/tests/organic_beat_records_disc.rs`.

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

All six commands - **Attack**, **Arts**, **Magic**, **Item**, **Spirit**, **Run** - are wired into the live loop. Attack opens a target cursor and commits a physical strike through the action SM. Arts / Magic / Item resolve to `Resolution::OpenArtsMenu` / `OpenSpellMenu` / `OpenItemMenu` - the command session can't run those pickers itself (they need the caster's saved chains / learned spells / live MP / inventory + party stats), so it hands off to a host-owned submenu. Spirit and Run resolve immediately (no target):

- **Spirit** charges the caster's AP gauge (`ApGauge::charge_spirit`, the retail Square-press +5) and raises a per-slot guard stance (`World::battle_guarding`, the engine model of the retail pending-action byte `+0x1DE == 4`) that halves incoming damage through the finisher's guard stage until the actor's next turn starts; the turn is consumed (SM parked at `EndOfAction`).
- **Run** rolls the escape and arms the ported run band (category 5 → `RunBegin`/`RunWait`/`RunEscape`): success tears the battle down `Escaped` (no loot, no game over, downed members floored alive at 1 HP), failure consumes the turn. The roll is the decoded `FUN_801E791C` formula - party `(SPD*3)>>1 + missingHP>>4` vs enemy `SPD + missingHP>>5`, two rand draws, Chicken Heart / Chicken King passives honoured (`battle_formulas::escape_roll`; see [battle-action.md](battle-action.md#spirit--run-in-the-live-command-menu)).

The submenu hand-offs:

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
