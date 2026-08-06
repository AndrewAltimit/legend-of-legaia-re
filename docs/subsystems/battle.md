# Battle subsystem

The battle overlay (`0898_xxx_dat`) carries the battle scene loader, the per-actor state machine, and the effect VM cluster. Loaded at RAM `0x801CE818` (same load slot as the town overlay; battle and town never coexist).

This is a large page covering both the retail reverse-engineering and the
clean-room engine systems. Use the contents below to jump to a section.

## Contents

**Retail scene + render**
- [Battle scene loader (`FUN_800520F0`)](#battle-scene-loader-fun_800520f0) - [stage-overlay dispatch](#stage-overlay-dispatch-the-0x47-loader-band) · [sparring-tutorial prompts](#the-sparring-tutorial-prompt-machine-overlay-967) · [command-flow byte](#the-command-flow-byte-ctx0x06---what-the-hook-table-indexes) · [the round loop](#the-round-loop---what-re-arms-0x1e) · [`s2` + commit](#s2-is-not-the-pad-and-how-a-command-commits)
- [Battle background](#battle-background) - [ground grid](#backdrop-ground---a-procedural-flat-grid-func_0x801d02c0) · [stage stream per scene](#which-stage-stream-a-scene-fights-in) · [backdrop shell](#backdrop-shell---two-copies-of-one-mesh) · [camera](#battle-camera-exact) · [post-strike two-shot](#the-post-strike-two-shot-fun_801d5854-cases-7-and-8) · [menu vs input framing](#the-command-chooser-is-the-far-framing-the-arts-input-is-the-close-up) · [resting yaw](#the-resting-yaw-is-the-orbit-and-a-battle-inherits-it) · [party meshes](#battle-party-meshes-assembled) · [display list](#the-battle-display-list-is-the-registration-set-not-active) · [staged-anim channel](#one-staged-anim-channel-actor0x1da)

**Retail battle logic + data**
- [Battle action state machine (`FUN_801E295C`)](#battle-action-state-machine-fun_801e295c)
- [Party wipe + the game-over overlay](#party-wipe--the-game-over-overlay) - [the port's hand-off](#the-ports-hand-off)
- [Battle context struct](#battle-context-struct)
- [Stage seats (`FUN_800513F0` placement tables)](#stage-seats-fun_800513f0-placement-tables)
- [Range / line-of-sight (`FUN_8004E2F0`)](#range--line-of-sight-fun_8004e2f0)
- [Monster init (`FUN_80054CB0`)](#monster-init-fun_80054cb0) - [record layout](#monster-record-source-layout) · [archive (PROT 867)](#monster-archive-prot-entry-867) · [mesh](#monster-mesh-record-0x04) · [native bridge](#native-renderer-bridge-clean-room-engine) · [browser battle render](#browser-play-page-battle-render) · [AI](#monster-ai-fun_801e9fd4-action-picker--fun_801e7320-target-resolver) · [charm at the end-of-action gate](#enemy-ally-charm-at-the-end-of-action-gate-the-charm-battle-softlock)
- [Stat aggregator (`FUN_80042558`)](#stat-aggregator-fun_80042558)
- [Battle archive (`FUN_80052FA0` / `FUN_800542C8`)](#battle-archive-fun_80052fa0--fun_800542c8)
- [Character record layout](#character-record-layout) - [why the pair order is `(max, cur)`](#why-the-pair-order-is-max-cur)
- [Battle main dispatcher (`FUN_801D0748`)](#battle-main-dispatcher-fun_801d0748) · [hottest utility (`FUN_801D8DE8`)](#hottest-battle-utility-fun_801d8de8) · [weapon trail builder](#weapon-trail-builder-fun_8005112c--fun_80048310--fun_800485bc) · [move-FX streak ribbon](#move-fx-streak-ribbon-fun_801e1d98)
- [Per-frame actor maintenance (`FUN_8004CE2C`)](#per-frame-actor-maintenance-fun_8004ce2c)
- [Additional SCUS battle-band helpers](#additional-scus-battle-band-helpers)

**Clean-room engine systems**
- [Inventory (page-banked)](#inventory-cratesasset-page-banked-layout) · [Status effects](#status-effects) · [AP / Spirit gauge](#ap--spirit-gauge) · [Battle stat aggregator](#battle-stat-aggregator) · [Item catalog](#item-catalog)
- [Battle round lifecycle](#battle-round-lifecycle) · [command runner](#battle-command-runner) · [BattleSession Resolve driver](#battlesession-resolve-driver) · [HUD model](#battle-hud-model) · [screen chrome](#battle-screen-chrome-packet-pinned) · [widget-class table](#the-widget-class-table---where-every-chrome-sprite-comes-from) · [SFX bank](#sfx-bank--scheduler)
- [Inventory item-use session](#inventory-item-use-session) · [Encounter system](#encounter-system) · [target picker](#battle-target-picker)
- [Equipment catalog](#equipment-catalog) · [Seru capture + spell learning](#seru-capture--spell-learning) · [Tactical Arts chain editor](#tactical-arts-chain-editor) · [rewards composite](#battle-rewards-composite)
- [Live gameplay loop - Field ↔ Battle](#live-gameplay-loop---field--battle-in-tick) - [auto vs player-driven](#auto-resolve-vs-player-driven) · [post-battle Seru learning](#post-battle-seru-learning)

**Runtime-memory captures + tests**
- [Encounter trigger memory layout](#encounter-trigger---runtime-memory-layout) · [scene-init residency](#battle-scene-init-residency-window) · [item-use residency](#item-use-battle-event-residency) · [stat-growth observations](#captured-stat-growth-observations)
- [CDNAME → MV STR cutscene routing](#cdname--mv-str-cutscene-routing) · [end-to-end gameplay loop test](#end-to-end-gameplay-loop-integration-test)
- [Field-to-battle intro presentation](#field-to-battle-intro-presentation)

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
968, `0x80055D2C..0x80055D44`), where `_DAT_8007BD0C` is the formation's
monster id. A **fourth writer lives outside the SCUS census**, in the battle
band's overlay code (the `0897` program): `FUN_801FD150`'s epilogue arm
(`0x801FD4D4..0x801FD548`, the `sb v0,-0x49b6(a0)` at `0x801FD514`) writes
stage id **3** (entry 969) mid-fight when both hold - the same formation cell
still reads `0xB5`, **and** the first monster seat (`actor_table[3]`) has HP
`+0x14C == 0`. The arm issues the loader-B page-in itself (`jal 0x8003EC70`
with `a0 = 0x4A = 3 + 0x47` - same-frame, not deferred to the dispatch
reader), bumps the battle ctx phase counter `ctx[+0x26]`, forces the
flow-state byte `ctx[+0x7] = 0xFD`, and zeroes the dead seat's `+0x21C` /
`+0x225`. So the `0xB5` boss fight walks two stage overlays: 968 from setup
(phase 1 alive), 969 once phase 1 dies - the guard separating the arms is the
seat's liveness, not a different id. Engine mirror:
`engine-core::overlay_loader::battle_init_stage_override` /
`boss_transition_stage_id`, resolved live by `World::battle_stage_id`
(`world/battle/stage.rs`); the stage overlays are MIPS code the engine does
not execute, so the resolver pins the selection, not a 968/969 behaviour
port.

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

**The prompt is a sized window, not loose text.** The emitter
`FUN_801F747C(text, style)` measures its prompt before it places it -
`FUN_8003CBA8(str)` returns the rendered line count, `FUN_80035F04(str)` the
pixel width - and the shared tail at `0x801F75B8` passes both on to the SCUS
text-actor registrar as a full rect:

```
FUN_8003541C(1 + waits, 0xD, str, x, y, width, lines*14 - 4, 0x44 - waits)
             a0         a1   a2   a3 +0x10 +0x14  +0x18       +0x1C
```

`FUN_8003541C` links the node into a list sorted on its `+0x08` key and stores
the rect at `+0x0A..+0x10`, the kind byte at `+0x1C` and the priority at
`+0x1D`, then draws it (`FUN_80030628`). So the box's *size* is measured, and
only its *corner* comes from the style table.

**Box placement.** The style index `0..=9` selects a jump table at
`0x801F6B48`. `x` is either the fixed left margin `0x10` or centred at
`0xA0 − width/2`; `y` is either the fixed top `0x0E` or bottom-anchored at
`base − (lines × 14 − 4)` for `base` in `{0x9A, 0xB0, 0xCC}` - the same height
expression the rect carries. Styles `0, 1, 8, 9` do not wait for
acknowledgement; `2..=7` do.

The wait is not a flag on one actor. The emitter initialises `s4 = 1` and only
the `0 / 1 / 8 / 9` arms clear it, because table slots `8` and `9` are the `2`
and `3` arms entered one instruction later - `0x801F7528` / `0x801F7538`, past
the `move s4, zero`. `s4` then picks the registered actor's sort key (`1 + s4`)
and priority (`0x44 − s4`), so a waiting prompt is a *different* text actor
from a self-dismissing one.

**What the frame looks like.** A retail capture of the drill prompt (style `0`,
two lines) shows the same gold double-line 9-slice frame and blue gradient
interior the dialog reading box wears, sized to the text. The measured
footprint agrees with the rect on every axis: centre rect `(0x10, 0x0E, w, 24)`
inflated 8 px on each side gives an outer left edge of `8` and an outer height
of `40`, and the text rows sit at the rect origin on the 14-px pitch. The port
therefore frames the prompt with the reading box's own chrome builder at
`BoxStyle::box_rect` - see
[`engine-ui::battle_tutorial_box`](../../crates/engine-ui/src/battle_tutorial_box.rs),
drawn by both hosts through the 320x240 stage transform (the rect is in retail
framebuffer pixels, not surface pixels). The confirm hand on a waiting box is
a port affordance borrowed from the dialog pager: what retail's slot-`2` actor
draws to signal the wait is not decoded.

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

### The round loop - what re-arms `0x1E`

`0x14` is the round-start arm and the **only** writer of `0x1E`:

```text
801d0ec4  lw    v1,-0x42dc(s0)      ; ctx
801d0ecc  sw    v0,0x880(v1)        ; highlight cursor = 0x8000 (the Left arm)
801d0ed0  jal   0x801d88cc          ; per-round actor sweep
801d0ed4  _sb   s5,0x0(s3)          ; ctx[+0x06] = 0x1E   (s5 = 0x1E at 0x801D0C98)
801d0ee4  jal   0x801d388c          ; open the prompt window (a0 = a1 = 0)
801d0ef4  lbu   v0,0x28a(v0)        ; round index
801d0efc  beq   v0,zero,0x801d0f0c  ; round 0 only: the tutorial arm below
```

The store is unconditional - no arm of `0x14` skips it - so **every round the
player is given starts on `Begin` / `Run`**, and the ring `0x28` is only ever
entered from `0x1E`'s confirm at `0x801D108C`. A port that opens its command
surface on the ring is not one frame early, it is a different machine.

`0x14` is reached from two different state machines:

- **Battle open.** The intro timer `0x0B` runs down and branches on the
  back-attack byte: `ctx[+0x290] == 1` stores `0xFE` (the party loses its
  first round outright), anything else stores `0x14`
  (`0x801D0E68..0x801D0EB8`).
- **Every later round.** The *action* SM's `ctx[+0x07] == 0xFF` arm, jump-table
  slot `0xFF` of `0x801CED44`. Its whole body is two writes:

```text
801e67e8  lui   a0,0x8008
801e67ec  lw    v1,-0x42dc(a0)
801e67f0  li    v0,0x14
801e67f4  sb    v0,0x6(v1)          ; ctx[+0x06] = 0x14  -> next round's prompt
801e6800  lbu   v0,0x28a(v1)
801e6808  addiu v0,v0,0x1
801e680c  jal   0x801f45a4
801e6810  _sb   v0,0x28a(v1)        ; ctx[+0x28A] += 1   (the round index)
```

`ctx[+0x07] = 0xFF` is stored at `0x801E67E4`, on the arm where the per-round
action cursor has passed every living actor. So the two bytes hand the round
back and forth: the flow SM ends a round by arming `0xFE` -> `0xFF`, and the
action SM ends it by arming `0x14`.

**Read this one off the jump table, not off the decompiler's flow analysis.**
Nothing inside `FUN_801E295C` branches to `0x801E67E8` - it is reached only
through the `jr v0` at `0x801E2AAC` - so a pass that does not resolve the table
reports `Removing unreachable block (ram,0x801E67E8)` and drops the round bump
from the C entirely. That `+0x28A` is the round index rather than some other
counter is corroborated by `0x14`'s own second reader: under
`_DAT_8007BD0C == 0xB6` (the Muscle Dome match) `0x801D0F94..0x801D0FA4` draws
`4 - ctx[+0x28A]`, the rounds remaining.

### `s2` is not the pad, and how a command commits

Every handler in `FUN_801D0748` tests `s2`, and `s2` is built two different ways
before the state switch runs.

The masks are **packed** throughout - byte-swapped against the raw BIOS word
(`engine-core::world_map_panel_host::packed_pad`), so the four directions are
Left `0x8000`, Right `0x2000`, Down `0x4000`, Up `0x1000`. Read raw they look
like face buttons, which in turn makes the confirm and cancel masks look
unreachable; the same trap is catalogued in
[`arts-command-gauge.md`](arts-command-gauge.md).

**With a selection widget up** (`_DAT_800846C8 != 0` and `ctx[+0x275] != 0`), the
pre-dispatch block at `0x801D07FC..0x801D0AC0` walks a highlight rather than
handing the press down. A pressed direction stores that mask in `ctx[+0x880]`
and stamps `+0x1D = 2` on the matching widget actor - `ctx[+0x1114]` Left,
`+0x1118` Right, `+0x111C` Up, `+0x1120` Down - with every other actor set to
`1`; `ctx[+0x275]` is how many arms exist, and the Up and Down arms are skipped
below `3` and `4` (`sltiu` guards at `0x801D0A24` and `0x801D096C`). Then
`0x801D0AC4..0x801D0B08` **rewrites `s2` outright**: the confirm mask
`_DAT_800846D0` replaces it with the stored `ctx[+0x880]`, the cancel mask
`_DAT_800846D4` replaces it with itself, and anything else leaves zero. So a
handler below sees a direction bit only on the frame confirm is pressed, which
is what turns its direction tests into "take the highlighted chip".

**Without one**, `0x801D0B0C` builds `s2 = _DAT_8007B874 | _DAT_8007B938`, the
plain packed pad, and the direction tests are direct presses.

`0x14` seeds `ctx[+0x880] = 0x8000` (`0x801D0ECC`), so a freshly armed prompt is
highlighted on its Left arm.

Which `s2` bit routes where, in the three prompt states:

| State | Left `0x8000` | Right `0x2000` | confirm `_DAT_800846D0` | cancel `_DAT_800846D4` |
|---|---|---|---|---|
| `0x1E` | Begin | Run -> `0x32` | Begin | - |
| `0x32` | run confirmed -> `0xFE` | back to `0x1E` | - | back to `0x1E` |
| `0x6E` | begin the round -> `0xFE` | step back | begin the round | step back |

`0x32`'s confirm arm stamps `+0x1DE = 5` (the Run action category) on all three
party actors at `0x801D1174..0x801D1184` before storing `0xFE`.

The ring's four arms sit on the same four masks, and every one of them commits
through the same idiom - **advance to the next member that still owes a
command, or begin the round**:

```text
801d16ac  jal   0x801db81c          ; next member after ctx[+0x13] awaiting a command
801d16b4  lw    v1,-0x42dc(s6)
801d16bc  lbu   v1,0x0(v1)          ; ctx[+0x00] = seated party count
801d16c4  bne   v0,v1,0x801d16d8    ; someone still owes one -> stay in 0x28
801d16cc  li    v0,0x6e
801d16d0  sb    v0,0x0(s3)          ; nobody does -> 0x6E
```

Ten sites in the handler share it, one per commit path: Spirit at `0x801D16AC`,
the target-cursor confirm at `0x801D22C4`, and the per-window target
sub-cursors at `0x801D24B4`, `0x801D2698`, `0x801D2830`, `0x801D29C0`,
`0x801D2AAC`, `0x801D2D74`, `0x801D2E64` and `0x801D2FE4`. `FUN_801DB81C` scans
forward from `ctx[+0x13] + 1`; its sibling `FUN_801DBA04` scans from zero and is
what `0x1E`'s confirm and `0x6E`'s cancel call. Both skip a member whose
per-member state byte `_DAT_8007BD10[i]` is already `4` (committed), whose live
HP `+0x14C` is zero, or whose status word `+0x16E & 0xF84` is set, and both
return `ctx[+0x00]` when none is left.

**No command path leaves the flow parked.** All ten end in `0x28` or `0x6E`,
which is the invariant a port has to keep: a command that resolves without
arming the next surface is a soft-lock, and it does not have to be the command
itself that breaks - see the readout desync in
[`battle-action.md`](battle-action.md#the-0x51-exit-gate-and-the-hp-bar-settle-invariant).

### How the engine raises the flow state

The engine splits what `FUN_801D0748` does in one machine across a
[`battle_input::BattleCommandSession`](../../crates/engine-core/src/battle_input.rs)
plus host-owned Item / Magic / Arts submenus, so the flow byte is *recomposed*
each frame by `battle_flow::flow_state_for` (an open submenu wins over the
command phase). Three points differ from retail and are deliberate:

- **Round prompt.** `World::open_battle_command` builds the session **already
  on** `CommandPhase::RoundPrompt` whenever the flow byte says the round is
  opening (battle entry leaves it `Idle`; the round boundary parks it on
  `TurnPrompt`), matching retail's unconditional `0x14 -> 0x1E` store. It has
  to be the phase the session is constructed in rather than one applied on a
  later tick: `battle_command.is_some()` is the only edge a host or a test
  has, so a prompt that lands one frame behind it is a prompt nothing sees -
  and `Run` lives on that prompt and on no other surface. A session reopened
  mid-round (a submenu backed out of) finds the flow on a window state and
  opens on the ring, which is where retail's own cancel arms land.
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
rewind exit discards the action and reopens the command menu.

**No host arms it.** `World::enter_battle` consumes the disc's own one-shot
system-flag arm (above), so the native window and the browser play page both
show the boxes in the fight retail shows them in, with no scene name, flag or
environment variable in the condition. `World::prime_battle_tutorial` is a
debug force, and `LEGAIA_BATTLE_TUTORIAL` (`0` suppress / `1` force / `now`
force and enter a fight) is `play-window`'s hand-testing knob on top of it -
neither is the port. Browser oracle:
`crates/web-viewer/tests/battle_tutorial_page.rs`.

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
  The 64² window is stretched across one whole `0x200` cell as **four quads**:
  the emit loop runs 2×2 times per visible cell and advances the sub-tile row
  pointer by `0x10` each time, so the sub-tile is `sub_row * 2 + sub_col` and
  there is **no RNG anywhere in the routine**. An earlier reading here - "each
  cell samples one sub-tile with a per-cell random corner mirror", over "two
  distinct variants duplicated across the row" - was wrong on both counts: the
  tiling is deterministic, and the variant count is a claim about the texture's
  content rather than about the renderer. The random corner mirror is real but
  belongs to the *particle* scatter `FUN_801E0080` (`rand() % 4` → two mirror
  bits). See [`functions/battle.md`](../reference/functions/battle.md#801d02c0).
  The *address* is scene-independent - the
  scene's battle VRAM build is what places that scene's own ground tile
  there (`town01` = warm sandy pebbles; an earlier engine heuristic that
  borrowed the dome's nearest "grass vertex" sampled a blue texel region in
  `town01` and painted the floor sky-blue). Engine mirror:
  `build_battle_ground_grid` in `play-window`, over the kernels in
  `engine-core::battle_backdrop`.
  The historical overlay capture filed under the `0896` label (a mislabeled
  slot-A window image; PROT 0896 itself is neither the battle background nor
  an overlay that loads here) shows the same grid renderer + `_DAT_8007b814`
  buffer - it is battle-overlay code seen through that capture.

#### The grid's own constants, read off the emitter

The sub-tile UVs are not derived - they are sixteen literal words the prologue
builds into scratchpad `0x1f800034` (`0x801d0304..0x801d03a0`) and the emit
loop reads back one group per quad, advancing `0x10` each time
(`0x801d0660` / `0x801d06c8`). Decoding them as `POLY_GT4` UV words gives four
fixed 32×32 blocks of the `(192..=255)²` window:

| Quad | Words | `u` | `v` |
|---:|---|---|---|
| 0 | `77c0c0c0 000dc0df 0000dfc0 0000dfdf` | `0xC0..=0xDF` | `0xC0..=0xDF` |
| 1 | `77c0c0e0 000dc0ff 0000dfe0 0000dfff` | `0xE0..=0xFF` | `0xC0..=0xDF` |
| 2 | `77c0e0c0 000de0df 0000ffc0 0000ffdf` | `0xC0..=0xDF` | `0xE0..=0xFF` |
| 3 | `77c0e0e0 000de0ff 0000ffe0 0000ffff` | `0xE0..=0xFF` | `0xE0..=0xFF` |

The `clut` half of word 0 and the `tpage` half of word 1 are where the `0x77C0`
/ `0x000D` address above comes from. No corner is ever mirrored: the four UVs
are copied into the packet verbatim.

**Grid origin.** `0x801d03b4..0x801d03d8` computes `x0 = -((w >> 1) << 9)` and
`z0 = -((h >> 1) << 9) - 0x200`. The `z` axis carries an extra cell of bias, so
the grid is not symmetric about the origin - at the live 28×28 it spans
`x ∈ [-7168, +7168]` but `z ∈ [-7680, +6656]`.

**The two culls.** Pass 1 transforms each cell *centre* by the view matrix
(`cop2 0x0480012` = `MVMVA` rotation/`V0`/`+TR`/`sf=1`), reads `IR3` back, and
writes `-1` / `0` / `1` per cell: `-1` when `z + 0x200 <= 0`, `0` when
`z > 0x6500`, else `1`. Only `1` emits - pass 2 skips on both the `bltz` and
the `beq zero` (`0x801d04b0` / `0x801d04b8`). There is **no screen-space test
in pass 1**; the screen-rect reject is separate, in pass 2
(`0x801d052c..0x801d05e8`), and drops a cell only when all four outer corners
fall past the same edge of the `0x140 × 0xF0` display.

Both are ported and tested (`battle_backdrop::classify_cell` /
`cell_offscreen`) and neither is applied by the port's builder: they remove
only geometry that is off-screen or behind the camera, which a depth-buffered
projection discards anyway, and the port uploads the grid once while the
camera orbits over it.

**Where the tile comes from.** The two addresses are constant for the whole
game, but the pixels behind them are not: each `scene_tmd_stream` entry carries
its own TIM at framebuffer `(832, 0)` with a palette at `(0, 479)`, so the
floor changes per stage while the emitter never does. 178 of the 182 backdrop
entries carry that pair, and **no** entry fills that page under a different
palette - which is what pins the constants against the corpus rather than
against one stage. The four that carry neither must draw no floor at all: an
untextured grid is a flat slab across the whole stage, which is a worse artifact
than an absent one. `battle_backdrop::ground_grid_drawable` is that decision,
shared so the two viewers cannot answer it differently, and the sweep is
`the_ground_tile_is_addressed_by_the_emitters_own_constants`.

The asset-viewer PROT browser and the browser entry viewer both draw the grid
under a backdrop. Two traps sit on that path. The grid is appended **after** the
shell's second copy, because it is world-fixed rather than part of the shell and
handing it to the transform would draw it twice, once flipped in `Z`. And the
browser viewer's VRAM upload is *targeted* - it uploads only the blocks the
TMD's own primitives sample - so the grid's page has to be added to that request
by name (`ground_page_rect` / `ground_clut_rect`); left out, the mesh builds
fine and the floor draws untextured, a failure visible only on screen.

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

### Backdrop shell - two copies of one mesh

The sky hemisphere, distant mountain ring and far ground ring come from the
scene's `scene_tmd_stream` entry (PROT `88` for `map01`) - `POLY_GT3` prims,
116 of them on screen in the angle-a capture. The entry is loaded by the
type-`0x01` chunk walker `FUN_8001FE70` into `_DAT_8007b864` and lands
contiguously in battle RAM (base `0x800A8B34` for PROT 88, byte-matched across
the four angle saves; leading TMD magic `0x80000002` at file `+4`,
uncompressed). PROT 88/89/90 share identical geometry and differ only in
texture payload.

#### One primitive list, two texture classes

A shell is not all texture. About a fifth of it by primitive count is
`F*`/`G*` flat / gouraud panels that carry a baked colour word and no UVs -
the sky band, the painted wall faces, the flat water. `town01`'s Tetsu arena
is 325 textured triangles and 79 untextured; `map01`'s dome is 336 and 78.
Retail draws them together: `FUN_8001ADA4` case 3 walks the whole group chain
and the GPU takes `POLY_F*` packets as readily as `POLY_*T*` ones.

The port has to reassemble that from two builders, because
`tmd_to_vram_mesh` drops any prim with no UVs - such a prim samples nothing.
The native window pairs it with `tmd_to_color_mesh` on the untextured
pipeline; the browser page uses the single `tmd_to_vram_mesh_field_hybrid`
mesh with a per-vertex textured flag. Both halves take the same second-copy
transform (`ColorMesh::append_scaled` mirrors the textured builder's, winding
reversal included), and both hosts must end up with the same triangle set -
pinned by `the_backdrop_shells_untextured_half_is_a_double_digit_share` in
`crates/engine-core/tests/battle_stage_entries_real.rs`. Rendering only the
textured half punches holes in the arena wherever a sky panel belongs.

#### The stage streams of one bundle share their VRAM

A scene bundle carries one `scene_tmd_stream` per sub-area, and those streams
are **not** allocated disjoint VRAM. Rim Elm's four (extraction entries
6..=9) each declare the same two 4bpp pages, `(768, 0)` and `(832, 0)`, under
the same two CLUT rows, `473` and `479`; the field texture pack puts a page
at `(768, 0)` as well. Retail never has to arbitrate, because the chunk
walker records one stream in `_DAT_8007B864` and only that one is resident.

A port that DMAs every TIM in the bundle - which the battle resource build
does, `BuildOptions::upload_all_tims` - leaves whichever sibling was written
last holding the address, and the shell then draws through a neighbouring
sub-area's texels and palette. `town01`'s semi-transparent cloud band
(`(768, 0)` at `v` 191..254, palette `1` of row 473, a greyscale + STP ramp)
came out as flat green rectangles standing on the arena wall, because the
palette that won the row was one of the rainbow CLUT-cycling ramps a sibling
parks at that index.

`engine-core::scene::upload_battle_stage_tims_into_vram` re-uploads the
selected entry's own TIMs last, restoring retail residency without touching
the rest of the build. Both hosts call it from their `build_battle_stage`.
Sweeps: `rim_elms_four_stage_streams_all_claim_the_same_vram` and
`the_selected_stage_entry_owns_its_vram_after_the_reupload`.

The shell is authored as **half** a bowl. That is the real shape, not a
truncated parse: across all 182 entries object 0 puts at most 8 % of its X or
Z extent past `X = 0` / `Z = 0`, and every object satisfies
`vert_top + n_vert * 8 == normal_top` exactly. What closes the circle is a
second draw of the same mesh.

**What the second copy is worth, measured.** Project `map01`'s drawn objects
through the exact camera each of the four angle captures was taken at (yaw
`_DAT_8007B792`, pitch `32`, `TR = (0, 1280, 7680)`, `H = 256`, all read from
the save state) and count the 320 screen columns the mountain ring covers:

| Capture | Camera yaw | One copy | Two copies | Retail pixels |
|---|---|---|---|---|
| a | 19.7° | 100.0 % | 100.0 % | 98.1 % |
| b | 334.7° | **71.9 %** | 100.0 % | **100.0 %** |
| c | 275.6° | 100.0 % | 100.0 % | 100.0 % |
| d | 231.3° | 99.7 % | 100.0 % | 100.0 % |

Three of the four yaws cannot tell the models apart - one copy already fills
the frame. Capture **b** can: a single copy leaves columns `0..89` with no
mountain geometry at all, and the retail framebuffer has a mountain band in
**90 of those 90 columns** (mean thickness 15.3 px). The second copy is not an
embellishment the captures merely tolerate; without it those pixels have no
source.

#### Two actors, one registered mesh

`FUN_800513F0` registers the TMD **once** - `80051a60 jal 0x80026b4c`, slot
stashed at the descriptor `0x8007680c + 4` = `DAT_80076810` - and then calls
`actor_alloc` (`FUN_80020DE0`) **twice** from that same descriptor
(`80051a7c`, `80051aa8`), parking the two actor pointers at
`battle_ctx + 0x106C` (copy A) and `+0x1070` (copy B). Both are ordinary
battle actors on the normal draw path, which is why `DAT_80076810` has no
resolved reader: the actor list is walked pointer-indirect.

They are two genuine draw entries, not one entry visited twice: each actor
gets its **own** `0x9C`-byte part table at `+0x44`, zeroed in `actor_alloc`
(`80020f04`) and allocated in the link pass (`80021184`). Live battle states
read two distinct table pointers, and the object-count edit below is applied
to each separately.

`FUN_80050120` drives the pair in lockstep - the depth-cue ramp at `+0x78` and
the draw-mode selector at `+0x56` are written to both on the same path
(`80050848..80050880`). `+0x56 = 3` selects case 3 of `FUN_8001ADA4`'s jump
table (`8001ae60 lhu v0,0x56(s0)`, table at `0x8001042C`) - **not**
`FUN_80048A08`.

Copy A draws at raw coordinates. Copy B gets one of two transforms:

| Selector | Written by | Effect | Determinant |
|---|---|---|---|
| `+0x26 = 0x800` (default) | `80051bc0`/`80051bc4` | half turn about world Y | `+1` |
| `+0x5A = 2` (exception) | `80051cc4`..`80051ce4` | X scale `-1` - reflection in the YZ plane | `-1` |

`+0x26` is the second of the three half-words `FUN_80026988` reads at
`actor + 0x24`; that kernel writes `sin` of it bare into matrix element
`[0][2]` and `cos` into `[2][2]`, which only a Y rotation does. `0x800` of the
`0x1000` full turn is exactly 180 degrees. The exception path routes through
`FUN_8001ADA4` case 3, which turns `+0x5A & 2` into `_DAT_1F800348 = -0x1000`
(`8001af28`..`8001af34`) and calls `FUN_8005B4E8` (`ScaleMatrix`, column
scaling - so the reflection is in model space, under the rotation). The same
predicate `+0x5A & 0xE` (`8001afd8`) negates the per-object rotation argument
and swaps the draw-call mode word from `0x40000000` to `0x48000000` - the
winding compensation a negative-determinant transform needs.

#### The per-stage table

Which transform a stage gets comes from the zero-terminated `u16` table at
`DAT_80078B50` (`SCUS_942.54` file `0x69350`, 99 slots naming 98 distinct
stages), walked at `80051bc8`..`80051c18` against the backdrop id
`word[0x80084540] + byte[0x8007BD60] & 0x7F`. A hit takes the mirror; a miss
takes the half turn. Stage id + 3 is the PROT extraction index, and every one
of the 98 distinct ids resolves to a `scene_tmd_stream` entry under that
offset.

**The table respects one geometric constraint.** A shell whose open side faces
`-Z` is symmetric about `X = 0`, so reflecting it in the YZ plane reproduces it
in place and fills nothing - only a half turn closes it. Of the 49 `-Z`-open
shells in the corpus, **zero** are on the mirror list; of the 133 X-open
shells, 98 are. Parser `legaia_asset::battle_backdrop`; the disjointness sweep
is `no_z_open_shell_takes_the_mirror_transform` in
`crates/asset/tests/battle_backdrop_real.rs`.

`0007_town01` (stage id 4) is on the list - the Tetsu arena is completed by a
reflection, not a half turn. Applying the half turn there instead plants a
second village wall across the open sea side, which is the artifact that once
read as "no completion exists" (see
[`re-do-not-re-walk.md`](../reference/re-do-not-re-walk.md#the-backdrop-shell-is-drawn-once-so-no-completion-exists)).

#### The choice is authorial, not derivable

Beyond that one constraint the table is hand-maintained per-stage data, and a
viewer that tries to infer it from the mesh will be wrong. 39 backdrop meshes
are carried by more than one PROT entry, byte for byte, and retail's table
splits **12** of those groups across the two transforms.

The clearest case is the Conkram family. `0730_concend` and `0736_conc3` are
identical files - `concend` carries `conc3`'s three stage meshes in reverse
slot order - and the table names `conc3`'s variants while naming none of
`concend`'s. So the same mesh is half-turned in one scene and mirrored in the
other, and the two renders differ visibly in where the colonnade and the
stairs sit around the ring. Neither is a port defect. `conc` and `conc2` take
the mirror alongside `conc3`; `urudre2` takes the half turn alongside
`concend`.

One group is split **inside a single scene**: `0321_balden2` is mirrored and
`0322_balden2` is half-turned on identical bytes. That rules out any per-scene
rule as well as any per-mesh one. It also shows what the choice costs where it
does not matter - that shell's cut section is exactly symmetric in `z`, and a
`z`-symmetric half is carried to the same point set by both transforms, so the
two draws are indistinguishable. Retail can differ freely wherever that holds.

Sweep: `the_second_copy_transform_is_not_a_function_of_the_mesh`.

#### The sibling table at `DAT_80078C1C` - a depth-cue selector, not geometry

`80051c1c`..`80051c6c` scans a **second** zero-terminated `u16` table the same
way and against the same backdrop id, setting a byte flag at `0x8007BDA8`
(`gp + 0xA90`, `gp = 0x8007B318`) instead of touching either actor.

Its 13 ids are the outdoor stages: the three variants of each kingdom
overworld (`map01` / `map02` / `map03`) plus `retona`, `deene`, `kor5` and
`rikuroa`. Note the 7 four-object shells are all inside the overworld nine.

The flag is read twice, both times in `FUN_80050120` and both times on the
**depth-cue** value the two backdrop actors share:

- `800505b8`..`800505c8` picks the ramp ceiling clamped into both actors'
  `+0x78` - `0x800` when clear, `0xC00` when set (either way forced to
  `0x1000` when `ctx+0x278 > 1` or `ctx+0x243 > 1`).
- `800507fc`..`80050834` picks how the far colour at `0x8007BB48` is derived
  from `ctx+0x890` - `>> 1` when clear, `(c - 0x010101) * 2` when set.

So it brightens the far-fog ramp on wide-open stages. It adds no third
geometric behaviour, and the completion is unaffected. Live-confirmed across
15 battle save states: the flag is `1` in exactly the captures whose stage id
is in the table and `0` in every other.

#### Object 1 is dropped

Immediately after allocating the pair, `80051ad4`..`80051bac` decrements each
actor's object count at `**(actor + 0x44)` and left-shifts the pointer array
by one **from index 1** (`A[i] = B[i+1]`, `B[i] = B[i+1]`, `i >= 1`). The
surviving draw list is objects `0, 2, 3, ...`; object 1 stays resident in the
relocated object table, unreferenced. So the 175 two-object stages draw object
0 alone, and the 7 four-object overworld shells draw 0, 2 and 3. For `map01`
that is obj0 = sky (`Y` to `-10522`), obj2 = mountains (`Y` to `-2257`),
obj3 = flat far ground (`Y = 0`, inner radius `2889`); obj1 is a near-detail
prop that never appears on screen.

The whole block is gated on `DAT_8007B64B == 0` (`80051abc` / `80051acc`).
That byte is bit 5 of byte `+8` of the field scene's encounter-region record
(`801DA09C`..`801DA0AC` in the field battle-intro overlay) - the same byte
whose low 5 bits pick which of a scene's stage variants to use. Set, it keeps
object 1.

#### Port

`legaia_asset::battle_backdrop` is the shared kernel: `MirrorXTable::from_scus`
parses the table, `drawn_objects_tmd` applies the object-1 drop, and
`SecondCopy::scale` / `flips_winding` give the second copy's transform (both
are exact integer diagonals, so no trigonometry is involved). Mesh side,
`Mesh::append_scaled` / `VramMesh::append_scaled` in `legaia_tmd::mesh` append
the transformed copy and reverse triangle winding when the determinant is
negative - the mesh-level equivalent of retail's mode-word swap. The
asset-viewer PROT browser and the browser entry viewer both place backdrops
this way and label them from the resolved transform.

The rest of the stage scene in `legaia-engine play-window`: the phase-scripted
camera (below), the flat tiled ground grid under the actors (the
`func_0x801d02c0` grid + constant texture address above), a sky-blue clear so
open horizon reads as sky, the real **assembled** battle party (see below),
and animated monsters. Monster actors compose a half-turn so they face the
party (`-Z` from the `+Z` seats - the retail Tetsu dialogue close-up shows the
monster's face while the archive meshes rest facing `+Z`). The actors draw
through the exact `tr.z = 7680` camera with the retail **4× actor world
scale** composed under the rotation (see below) - the battle meshes are small
(party 134–284 units, monsters 77–368), and the 4× base is what makes them
read at retail size against the deep translation.

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
| action executing | 0 (or floor-tilted) | `0x800 − facing`, or the drifting `ctx[+0x6DA] − facing` | `(0, height, 0x500)` party / `(0, 0x500, ctx[+0x6D0])` monster | 6-step glide in, 7-step out, then held |

The **step counts are retail's own** `FUN_801D829C` durations, not just trace
readings: the framing cases pass `a3` in *display frames* and a camera step is
two frames, so cases `0`/`1`/`2`/`3`/`6` (`a3 = 0xC`) glide over 6 steps and
case `9` (`a3 = 0xE`, `0x801D712C`) over 7. The `-4`/step idle orbit is likewise
in the disassembly: the action SM subtracts `DAT_1F800393 * 2` from
`_DAT_8007B792` per tick and gates that on `ctx[7]` being `0x00` or `0x0B`
(`0x801E2A3C..0x801E2A6C`), which is what makes "no action executing" the
phase-script condition for the orbit rather than an inference.

**The action framing (`FUN_801D5854` case 6)** is the one the action SM arms at
almost every state, and it forks on `_DAT_8007BD71 == 0xFE && slot < 3`. The
party arm frames from behind the actor (`yaw = 0x800 − actor[+0x46]`) at a
height of `−5 × actor[+0x3E]`, floored at `0x280` with a quarter of the
shortfall added to the pitch so the camera tilts down instead of sinking
(`0x801D6494`). Between the base pose and that floor it runs a per-character
script dispatched at `0x801D5D50` (`0x801D5DAC` / `0x801D5FC0` / `0x801D61E8` /
`0x801D6440`, rejoining at `0x801D645C`) which reads `actor[+0x1DB]` over the
band `0x11..=0x18` (bias `-0x11`, bound `8`).
**Which states hand it the camera is a band, not a byte list.** `FUN_801E295C`
arms per band: the setup band (`0x00`, `0x0B`) arms nothing and runs the
prologue orbit, the seed (`0x0C`) and action (`0x14..=0x48`) bands arm case
`6`, the Run band (`0x64..=0x67`) arms case `9` plus the orbit itself, and the
Done band (`0x50..=0x5A`) arms case `6`/`8` under a **bounded** tail - retail
seeds `ctx[+0x6D8] = 0x3C` in the `0x50` arm and leaves for `0x5A` when the
frame step drives it negative, so the per-action framing survives at most ~60
display frames past the strike.

That bound is the one thing the port cannot copy: its `DoneFadeDown` waits on
the HP-bar display cursor settling, which is unbounded, and a measured
auto-resolved fight rests there for half its frames. `battle_cam_script::
action_state_frames_the_action` therefore treats the whole Done band as idle,
which is the port's stand-in for retail's timer; classifying it as an action
in flight leaves both hosts in the per-action close-up for the entire fight,
with one actor filling the frame and no idle orbit. Guards:
`the_done_band_does_not_own_the_action_framing` and
`a_real_turn_spends_most_of_its_frames_in_the_far_framing`.

**Case 6 is re-armed every pass, so the framing chases the actor.** Each of
the action states calls `FUN_801D5854(actor, 6)` before it does anything else
- `0x0C`, `0x14`, `0x1F`, `0x20`, `0x32`, `0x37`, `0x3C`..`0x40`, `0x46`,
`0x47` - so the three tween-target vectors are rebuilt out of the live actor
record each display frame and `FUN_801D829C` re-emits the step table. The
target is not frozen at the state change, and the difference is not cosmetic:
`0x14` stages the approach walk and `0x19` runs it, so a party member crosses
most of the gap to its target before the swing. A focus pinned to the vacated
seat frames bare ground - at the close-up depth `prescale(0x500)` = 2048
against 4x-scaled stage coordinates the whole formation leaves the frustum,
several combatants behind the eye. `BattleCamera::retarget_action_glide` is
the port's re-arm; it carries the armed segment's remaining step count over,
so a framing whose actor stands still still arrives on target at
`ACTION_STEPS`. One visible consequence: the fallback arm's yaw now follows
the live `ctx[+0x6DA]` drift instead of freezing on its value at the phase
change.

The fallback arm frames on the seat position at `ctx[+0x6D0]` - the depth
`FUN_801F0348` derives from the framed monster's size class - with a style byte
`ctx[+0xD]` selecting three tweaks and character id `4` overriding the whole
translation. `ctx[+0x6DA]` is not a constant: the SM's prologue advances it
about one unit per display frame (`0x801E29E4..0x801E2A24`), so successive enemy
actions frame from a slowly drifting angle.

**The framing-case table.** `FUN_801D5854`'s mode argument indexes a
ten-entry jump table at `0x801CEA00` (PROT 0898 file `0x1E8`), and modes `4`
and `5` are the same no-op tail slot:

| Mode | Entry | Framing | Focus |
|---|---|---|---|
| `0` | `0x801D59E0` | arts / spell / item **input** close-up | acting actor |
| `1` | `0x801D5A6C` | submenu-exit swing | acting actor |
| `2` / `3` | `0x801D5BB0` / `0x801D5BD4` | menu-driver transitions | acting actor |
| `4` / `5` | `0x801D7138` | nothing - straight to the shared tail | - |
| `6` | `0x801D5CE8` | per-action framing (two arms) | acting actor |
| `7` | `0x801D65DC` | post-strike **two-shot** | attacker-target **midpoint** |
| `8` | `0x801D67D0` | end-of-action | the target |
| `9` | `0x801D6EF4` | far Begin/Run framing | formation centre |

### The post-strike two-shot (`FUN_801D5854` cases 7 and 8)

Case `7` is the only framing in the set that orbits **both** combatants. Its
base (`0x801D65DC..0x801D6694`) is pitch `0`, yaw `ctx[+0x6DA] - actor[+0x46]`,
`TR = (0, 0x500, ctx[+0x6D0])` and a focus at the midpoint of the acting actor
and its target (`actor[+0x1DD]` through the actor table `0x801C9370`), each
component `(a + b) >> 1` and negated. Then the shared `ctx[+0xD]` style fork
(`1`/`3` add half a turn, `2`/`3` drop `TR.y` to `0x400` and tilt the pitch by
`0x80`), a **one-way** yaw unwrap at `0x801D6700` - `yaw = (yaw - 0x700) &
0xFFF`, plus a full turn when that lands below the live `_DAT_8007B792`, so the
swing never takes the short arc back - and a "pull in" tweak at `0x801D6780`
(pitch levelled, `TR.y += 0x40`, `TR.z = 3z/5`) gated on `_DAT_800846C0 == 0`
and the acting actor's anim state.

Case `8` is the same shape aimed at the target alone: an extra `-0x100` on the
yaw base, `focus.y` forced to the stage floor, a `-0x600` unwrap, and a focus
fork that falls back to the acting actor when `actor[+0x1DD] >= 8` or the
target's node is dead (`0x801D6870`). Its long per-liveness tail from
`0x801D69A8` - the death-clip re-frame, the counter-attack flags, the
`ctx[+0x270]` ramp - is decoded but not ported; every branch reads a channel
the engine's battle actor does not carry.

Which states arm them is `FUN_801E295C`'s own fork, not an inference. The
attack chain's recovery-wait and return (`0x1F`, `0x20`) share one arm at
`0x801E5660..0x801E56C0` whose **default** is `li a1,0x7`; it takes mode `8`
only when the target's live anim id matches its counter-trigger bytes
(`s8[+0x1F1]` / `+0x1F2`) or a party slot faces a target already in a death
clip. The Done cleanup (`0x50`) forks on the action category at
`0x801E5FC0..0x801E6018` - `actor[+0x1DE] == 3` (Attack) and "party slot whose
target's live-HP halfword reached zero" branch to `li a1,0x8`, everything else
to `li a1,0x6` - and `0x52` / `0xFD` arm `8` unconditionally (`0x801E5F74`).

Engine side: `battle_cam_script::recover_framing` / `action_end_framing`, armed
by the `Recover` / `ActionEnd` phases. `0x51` is deliberately left idle - see
the Done-band note above; the port's residency there is unbounded where
retail's is `ctx[+0x6D8] = 0x3C` frames.

### The command chooser is the far framing, the arts input is the close-up

"A battle menu is open" does not select the close-up. The battle menu driver
`FUN_801D388C` arms **both** cases: `0x801D475C` / `0x801D53B8` pass `a1 = 0`
and `0x801D4908` / `0x801D5688` pass `a1 = 9`, and the battle tick
`FUN_801D0748` arms case `9` itself at `0x801D0E98`. Two retail save states
separate them, framebuffer and RAM together: with the **Begin / Run** chooser
up the rotation/translation trios read `pitch 32`, `TR (0, 1280, 7680)`, focus
at the origin - case 9's `max(span * 3, 0x800)` over the `+-800` seats,
prescaled, exactly - and the framebuffer shows both fighters; with the **arts
input** panel up they read `TR (-512, 1152, 2457)` and `yaw = 0x8F0 -
actor[+0x46]` (`2119` against a facing of `169`), which projects the enemy off
the left edge behind the panel. So the close-up belongs to the input pickers,
and a host that folds the command chooser into it puts the opponent behind the
eye for the whole command phase.

### Case 9 is re-derived every pass, so the depth follows the formation

The far framing is not armed once and left. `FUN_801D0748` re-arms it per tick
and the menu driver re-arms it on its own transitions, so `max(span * 3,
0x800)` and the bbox centre are rebuilt out of the live actor table - exactly
like case 6. This matters because the formation *moves*: an attacker walks most
of the way to its target during the approach, collapsing the span onto the
`0x800` floor. A depth frozen at the moment the far framing was armed survives
the actor walking back to its seat, leaving the eye at `prescale(0x800)`
against a full-width formation with one combatant filling the frame and the
other behind it. Engine side: `BattleCamera::retarget_menu_glide`, which skips
only the two segments that are not "walk to the far framing" (the rate-clamped
dialogue dismiss and the scripted submenu-exit swing).

### The resting yaw is the orbit, and a battle inherits it

`_DAT_8007B790/92/94` is **one** rotation trio, shared by the field and battle
cameras, and nothing on the battle-entry path zeroes it: case 9 passes
`_DAT_8007B792` straight through and the action SM only decrements it. A fight
therefore inherits whatever azimuth the field camera left. Five battle save
states caught at the identical framing (`ctx[7] == 0x00`, pitch `32`,
`TR (0, 1280, 7680)`, focus at the origin, `+-800` seats) read five different
yaws - `224`, `2632`, `3136`, `3808`, `3882` - so no captured value is *the*
resting yaw. What must not survive is `0`: at yaw `0` the eye looks straight
down the seat axis and the two rows project to the same screen X, each
occluding the other. `BattleCamInputs::entry_yaw` carries the inherited
azimuth; both hosts feed it `World::field_camera_azimuth`.

**The per-art attack camera is an override, not a fold.** `FUN_801D71B8` is
*not* part of case 6. Its only call site is `FUN_801D5854`'s shared tail
(`0x801D7180`), which runs after whichever framing case has already handed its
pose to the tween builder, and is gated on `_DAT_800846C0 != 2` and the acting
actor's `+0x1DD < 8` (`0x801D7138..0x801D7178`). The routine then seeds a
**fresh** pose from the actor - pitch `0`, yaw `−actor[+0x46]`,
`TR = (0, 0x400, 0x400)` (`0x600` height for character `3`), look-at the negated
actor position - runs a per-character / per-art arm over the second band
`0x1A..=0x2D`, and calls the *same* tween builder again with its own much
shorter duration (`1`, `3` or `6` display frames against case 6's `0xC`).
Whichever call ran last owns the step table that frame, and this one runs last;
an art id with no arm returns without arming anything and case 6's framing
stands. Both the seed depth (`0x400`) and the arms' folds make the swing
close-up **tighter** than case 6's `0x500`, ramping as `ctx[+0x26E]` climbs.

The arm's offsets come from the disc table
[`battle-attack-camera-table.md`](../formats/battle-attack-camera-table.md),
whose two columns are a per-action coin flip rather than two swing phases; that
page carries the row map, the ramp counters and the `actor[+0x1DB]` id space.
Engine side: `legaia_engine_vm::battle_attack_camera` runs the thirteen arm
bodies and owns the ramp quartet; `battle_cam_script`'s Action phase steps the
live pose toward whatever the arms produce, each frame, on the arm's own
duration - which is what retail's per-frame rebuild of the step table amounts
to. Both hosts feed it the same three per-actor channels (`actor[+0x1DB]` as
`BattleActor::latched_anim`, `actor[+0x21B]` as `hit_count_bound`, and
`actor[+0x22C][+0x68]` from the battle animation player's cursor, `<< 4` into
retail's sixteenths).

`H = 256` and the identity·16384 base hold through every phase. The traced
numbers above are one fight's *instance* of two formulas, not constants: the
submenu yaw `2288` is `0x8F0 - actor_facing` and the menu depth `z` is the
formation-sized `max(span * 3, 0x800)`, which lands on `7680` for the solo
Tetsu seats. Per-seat variation lives in the **focus trio**, which a solo
trace cannot distinguish from a constant. Both framing laws, the per-character
height table `0x801F4D2C`, and the focus trio are covered under
[`battle-action.md`](battle-action.md#case-0---the-submenu-close-up-framing).
Engine mirror: the phase script lives ONCE, in
`legaia_engine_vm::battle_cam_script` (phases, poses, glides, plus
`battle_vp` - the retail GTE view-projection as one matrix), and both hosts
drive it: the native `play-window` (`window/battle_cam.rs` adapter;
`battle_cam_inputs` derives phase / acting actor / formation from the live
dialogue / command-session state) and the browser play page
(`web-viewer::play_battle_render`, same derivation, handing the page a ready
view-projection via `play_battle_camera_vp`), each stepping on the retail
display-frame clock. The glide-table kernel port stays at
`legaia_engine_vm::battle_camera` (`FUN_801D829C`); a cross-host recipe test
in each host pins both derivations to the same literal pose.

**Screen shake.** `FUN_801D9D30` jitters the same translation pair
(`0x800840B8/BC`) by two LCG samples masked to `0xFFFFFF >> (0x15 − amplitude)`,
where the amplitude is `_DAT_8007B630`. That global has exactly one retail
writer - the field-VM opcode `0x4C` outer-nibble `8` sub-`4`
(`[4C, 84, amplitude]`, arm `0x801E2134`, jump-table slot `0x801CEF58`) - and
`FUN_801D9D30`'s only callers are the field-family overlay's per-frame camera
updaters (`0x801D1344` and siblings), so in retail the shake is a *field*
effect and no caller is resident during a fight. The port models the opcode
(`FieldHost::op4c_n8_sub4_set_b630` → `World::camera_shake_amplitude`) and
steps the kernel from the shared battle camera, which owns the same
translation pair. The offset is held beside the framing pose rather than
inside it, so a live shake cannot stall a rate-clamped glide.

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
translation (`256 * 4*370 / 7680` ≈ 49 px for a 370-unit monster).

**Every battle draw class rides that scale in the port**, not just the
combatants - the backdrop is registered as an ordinary background actor
(`FUN_800513F0` → `FUN_80020de0` alloc → the normal actor path), so it goes
through the same `FUN_80048A08` composition. The port therefore lifts the
arena and the ground grid with the same `BATTLE_WORLD_SCALE = 4.0`
(`PlayWindowApp::battle_stage_model` natively, `BattleMesh::stage_positions`
in the browser upload) and scales the grid's DPCS ramp window with them,
because that window is a view depth.

The camera's translation trio is authored in this scaled space: the traced
far framing's `TR.z = 7680` is the eye distance to a formation whose seats
are `±800` **before** the scale. Leaving a draw class at raw 1× under that
trio has two consequences, and the port shipped both. The eye orbits that
class at four times the intended radius - clear of the arena on one side and
straight through its shell on the other, so the frame fills with a single
magnified wall - and every actor draws `3 × seat` away from the ground cell
it stands on. Neither is visible at the far framing on a centred formation,
because a focus at the origin makes the two classes coincide: that is the one
configuration the pose tests and `retail_battle_mvp` sample. Guard:
`the_ground_under_an_actor_projects_under_the_actor` projects each retail
seat through both classes at every framing and requires the same pixel.

The function that camera comes from is **`battle_dome_camera_mvp`**, not
`retail_battle_mvp`. The two are not interchangeable and only one is live:

| | `retail_battle_mvp` | `battle_dome_camera_mvp` |
|---|---|---|
| Pose | the fixed `TR = (0, 1280, 7680)` | the live phase-scripted pose |
| Role | camera-RE reference + regression target | every battle draw |
| Reached by | nothing (`#[allow(dead_code)]`) | the play-window battle path |

`retail_battle_mvp` pins the *static* composition to 0.0002 px against the
savestate framebuffer, which is what makes it the regression target; it holds
the backdrop's own translation fixed, so it cannot express the phase glides the
[camera section](#battle-camera-exact) traces. `battle_dome_camera_mvp` takes pitch /
yaw / TR / focus from the live `battle_cam` pose each frame and falls back to
the far framing at its minimum depth on the first frame. Both build on the
shared `battle_mvp_with_tr`, which is why the pinned projection stays a valid
oracle for the live one.

Note also that the 4× is sourced from `DAT_8007BF10 = 16384 * I` - the actor
pass's base matrix - and not from the actor field `+0x78`, which
`FUN_8001ADA4` passes as `FUN_80043390`'s IR0 depth-cue argument
([`renderer.md`](renderer.md)). Two different quantities; reading `+0x78` as
the world scale would be a different claim with different evidence.

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
real colours (blue Vahn / pink Noa / Gala).
A 4th party slot is not rendered: the runtime texture band + CLUT rows cover
party slots 0..=2 only, so Terra (player file 866, idle stream 17 parts)
has no relocation target.

#### The battle display list is the registration set, not `active`

Retail's loader gives the fight its own actor set: `FUN_800513F0` registers
the backdrop, the party blobs and the monster meshes into `DAT_8007C018[]`
and links **those** actors into the render OT. The field scene's actor list
does not survive the transition.

The port keeps one actor array across the transition (the world clones it
into `field_return` and restores it at battle end), so every field slot
arrives in the battle still holding its scene-mesh binding and draws at
whatever battle-world coordinates its `move_state` carries - which for a
scene actor that never moved is the **origin**, dead centre of the arena
between the two rows.

"Draw only `active` actors" is **not** a sufficient gate, and the earlier
reading that the leftover slots are all inactive is false: rikuroa hands the
battle two live field actors, which drew a scene prop over the party member
and made the fight look like it had no party in it at all. The registration
set is the gate - `unregister_non_battle_meshes` (native host
`window/battle.rs`) drops the `tmd_binding` of every slot the battle loader
did not just register, so the display list is exactly what the loader built.
Nothing is stashed for the restore: the bindings return with the field actor
table. Regression: `battle_display_list_tests` in the same module.

`LEGAIA_DIAG_BATDRAW=1` prints that display list at battle entry - one row
per bound slot with its role (party ordinal / monster id / `STRAY`), seat,
mesh vertex count and projected seat. It is the "which meshes is this battle
actually drawing" instrument; `LEGAIA_DIAG_BATCAM` answers the per-frame
framing question and `LEGAIA_DIAG_POSE` the per-frame mesh one.

### One staged-anim channel: `actor[+0x1DA]`

Which clip an actor plays is a **single byte**, `actor[+0x1DA]`, with
`+0x1DB` as its committed mirror. Every producer writes that same byte, and
the last writer wins:

| Producer | Site | What it writes |
|---|---|---|
| Action SM, party approach | attack band state `0x14` | literal `1` (the walk entry) |
| Action SM, strike loop | attack chain | the strike-script byte (`0x0C..0x0F` swings, art ids) |
| Damage arm, flinch | `FUN_800402F4` `0x80042124` | `actor[+0x1EF]` (tag-2 entry) |
| Damage arm, knockdown | `FUN_800402F4` `0x80042118` | `actor[+0x1F1]` (tag-4 entry) |
| Knockdown → get-up chain | `FUN_8004AD80` `0x8004B690` | `actor[+0x1F2]` (tag-5 entry) |

The commit `FUN_8004AD80` copies `+0x1DA` into `+0x1DB` unconditionally
(`0x8004AEB0..0x8004AEB8`); there is no reaction guard anywhere on that path.
So a hit reaction is not a mode an actor is *in* - it is just the current
value of the staged byte, and the next thing the SM stages replaces it.

Which arm the damage takes is decided at `0x800420F4..0x80042124`: flinch
when `actor[+0x1F2] == 0` (no get-up entry) **and** the damage is survivable,
knockdown otherwise. The `+0x1EF..+0x1F3` map is filled by `FUN_80054CB0`
(`0x80055360..0x800553F0`), one slot per action tag `2/3/4/5/0xB`, with the
tag-4 → tag-2 fallback at `0x80055428`. Every player battle file carries a
tag-5 entry, so a party member takes the **knockdown** arm on any hit.

The port models the reaction with its own `Actor::battle_reaction` latch
(`engine-core::world::actors`) because its `Pose` hook - the per-frame
`pose(Idle)` the attack band issues - is an engine-local channel with no
retail counterpart and would otherwise cancel a reaction on the frame after
it starts. That latch must **not** outrank the staged channel:
`commit_staged_battle_anim` clears it whenever it installs a staged clip.
Giving the latch priority instead is what made a hit party member spend its
whole attack turn face-down - it walked to the target and back playing the
knockdown / get-up pair, and the approach clip plus every weapon swing were
dropped on the floor. Regression:
`crates/engine-core/tests/battle_reaction_stage_precedence.rs`, plus the
GPU-free pose oracle in
`crates/asset/tests/battle_pose_orientation_real.rs` which pins that the
upright family really is upright and the reaction family really is prone.

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

The port carries that scan faithfully, and it inherits a hazard retail does
not have: retail cannot enter a battle without a seated party, the port can.
`BattleActor::liveness` (the `+0x14C` mirror) **defaults to `0`, and `0` means
dead**. It is raised only by the roster projection in `load_party` /
`set_active_party`, which reads `hp_cur > 0` off a `CharacterRecord`. A world
built straight from `SceneHost::open_extracted` has never run that projection,
while `World::party_count` already defaults to `3` - so the party-side scan
finds three actors, all reading dead.

The party arm is tested **before** the monster arm, so such a battle reports a
**party wipe on its first end-of-action**, at full nominal HP, before anything
is struck - and a deliberate *monster* wipe is reported as a party wipe too.
Since the port defers the field restore behind the game-over hold (below), the
scene then parks in `SceneMode::Battle` and never returns to the field.

The failure is silent in the direction that matters: a harness that never seats
a party sees "battle ended" and walks on, so a run can score whole legs after
its party is gone. Any fixture that enters a battle must seat one the way
`BootSession::begin_new_game` does; a bare `open_extracted` host is not a
playable party.

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
   selector also leaves the battle-return marker `_DAT_8007B8B8 = 2`.
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
3. `DAT_8007BD60` bit `0x80` is a **party-survived latch**: seeded at
   battle load (`FUN_8001822C` body, `0x80018670` / `0x8001869C`, next
   to its `game_mode = 0x14` store), cleared by the `0x5A` end-of-action
   wipe scans (0898 `0x801E65F0` / `0x801E6694`, beside their
   `_DAT_8007BD2C` cause writes), then re-set on the surviving exits:
   the victory reward path `FUN_80026018` (`ori 0x80` at `0x800260AC`)
   and the successful-escape arm of the escape roll `FUN_801E791C`
   (`0x801E802C`). A wipe is the only battle end that leaves it clear.
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
5. Scripts can invoke the same handoff directly: `FUN_8003C7EC` is a
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

The **engine port applies it**: `engine_core::monster_catalog::monster_def_from_record` seeds ATK / UDF / LDF / INT from `battle_stats()` and AGL / SPD / HP / MP from the plain record fields, matching which stores the boost block does and does not touch. The accuracy / evasion bytes clamp the *boosted* INT, because the actor halfword the interrupt roll reads (`+0x168`) is the one the boost block's last store writes. Seeding from the raw accessors instead - which the port did - makes every enemy in the game materially weaker than retail.

Battle entry also seeds **both defence facets** into `World::battle_defense_split`, not one collapsed `max(UDF, LDF)` scalar. The melee kernel picks UDF or LDF by the swing's command parity (`FUN_801EC3E4` at `0x801ECE14`), so a single scalar leaves that branch dead for the whole monster band and makes every enemy defend with its better half against every swing. A Defense buff moves both halves together, as retail's "Defense Up" does.

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

The name string carries a two-byte **element-icon escape**: a `^` + letter
prefix (`^A Gimard`, `^F Aluru`) the battle UI renders as the element badge,
in the fixed order `^A`=Fire, `^B`=Thunder, `^C`=Wind, `^D`=Water, `^E`=Earth,
`^F`=Light, `^G`=Dark, `^H`=Evil (the icon-glyph row `0x1D..0x24` in the same
order - **not** the element-id order of the [`+0x1D` element byte](#monster-record-source-layout)).
Across the roster every carrying monster's caret letter agrees with its
element byte, with one deliberate exception: `^H Cort` (the final boss) wears
the Evil icon over element byte `7` - the no-affinity id whose matrix row and
column are all-100. Boss-tier `$2`/`$3` name suffixes are literal ASCII, not
markup.

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
the relocated mesh to the actor. `play-window` does this on each
`Field → Battle` transition (against a throwaway clone of the
field VRAM, restored on the way back) so the enemy is drawn, not a stand-in.

### Browser play-page battle render

The browser play page runs the same `Field → Battle` edge through
`legaia_web_viewer::play_battle_render` (`LegaiaRuntime::enter_battle_render`
/ `exit_battle_render`), reusing the shared kernels above rather than a
second implementation: the scene's battle-kind resource build
(`SceneLoadKind::Battle`) makes the stage dome + its textures resident, the
dome takes `drawn_objects_tmd` + the `MirrorXTable` second copy
(pre-appended via `VramMesh::append_scaled` so the page uploads one mesh),
the ground grid comes from `build_ground_grid` with the `DAT_80078C1C` far
colour, the flame atlas + per-slot monster injection + assembled party bands
land in a throwaway battle VRAM the page swaps in for the fight, and each
actor's idle / action / swing / art-bank clips are installed on the world so
the shared battle SM poses them (`pose_frame`, read back per frame through
`play_battle_actor_pose`). Actor draws compose the same enemy half-turn and
retail 4× world scale as the native window.

Host differences, disclosed rather than approximated silently: the camera is
the retail far "menu" framing (`FUN_801D5854` case 9, formation-sized depth,
idle orbit) mapped onto the page's orbit projection - the native
phase-scripted dialogue / submenu close-ups and measured glides are not
ported; the ground grid draws without its per-draw GTE depth cue (the page
renderer's cue uniform is global); and the per-tick facial-animation VRAM
re-stamps, mid-battle summon-creature spawn and battle-intro screen-prim
emitter remain native-only.

### Weapon-trail afterimage streak

The trail a swinging weapon leaves is one semi-transparent `POLY_FT4` per emitter call (`FUN_801E1AB0`), and its two projection inputs are context words the action effect script's terminator writes: the billboard centre from `ctx[+0x1144]` and the half-width as `ctx[+0x6C6] - 0x200`. The terminator stages both in one block - `sw` of the move-power record pointer to `+0x1014`, `sh` of that record's `+0x04` to `+0x6C6`, then a four-slot loop writing phase `1` to `+0x24E + i` and the launch position to `+0x1144 + i*8` (`FUN_801DEA50`, `0x801DF284..0x801DF2E0`).

The launch position is **not** the bare actor position: retail re-seeds its stack pair from `actor[+0x34..+0x3B]` at the top of every record iteration and runs the scale + facing rotation on it before the terminator test, so the quad the seed loop copies out carries the terminator record's own placement.

Port: `engine-core::action_effect_script::MoveFxStreak` is the block (record id rather than pointer, one shared launch point rather than four identical copies), installed by the live per-frame walk in `World::step_actor_effect_script` and read back through `World::move_fx_streak`. `engine-render::streak_pass` projects it once per frame and hands the corners to the ported packet builder `afterimage::build_afterimage_quad`, whose jitter law, brightness band, UVs, CLUT (`0x7700 + trail id`) and texpage (`0x0027`) are unchanged. The native window appends the quads to its screen-space textured batch.

Two disclosed departures. The **projection** is the engine camera's, not the GTE's: `project_streak_corners_mvp` takes the screen-space gradient of the battle MVP and fans the corners out along the screen axes, which is the same operation `FUN_800195A8` performs in view space - but the engine's battle camera carries no GTE rotation/translation pair to feed the exact port (`billboard::project_billboard`). And retail links each packet at the projected billboard's own OT bucket, inside the scene; the engine's screen-space batch draws them over the actors instead of interleaved with them.

The chained-ribbon sibling `FUN_801E1D98` is wired through the same pass, and the dispatcher choice (`0x801E0CA0` vs `0x801E0CD0`) is decoded: the phase driver `FUN_801E09F8` walks the counter `ctx[+0x6C6]` down `DAT_1F800393 << 2` per frame and selects by value - party afterimage at `>= 0x281`, ribbon below `0x201` (nothing in the dead band), monster ribbon at every value (`0x801E0C64..0x801E0CE8`). Port: `streak_pass::streak_quads_scheduled` + `engine-core::MoveFxStreak::tick_counter`. See [`battle-action.md` § Arts presentation](battle-action.md#arts-presentation-slow-motion-and-after-image-ghosts).

**Reachability today.** The pass is wired into the native window's screen-FX
builder, but a live `--battle` fight emits **zero** quads. It is gated on
`World::active_move_fx_trail_texpage()`, which is set only when
`World::spawn_move_fx` stages a move-power record's Spawn prototypes - i.e.
when a *move* runs. A party basic Attack stages no move: the attack
resolution leaves the actor's `+0x1DF` action stream all-zero, so the attack
chain reads its terminator on the first byte and exits straight to recovery
without staging a swing (`0x0C..0x0F`) at all. Damage still lands - the live
loop applies it through its own strike path, not through the SM's strike
band - but until the action stream has a producer for the party attack, both
the swing clips and the streak that trails them stay unreached.

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
[battle-action.md](battle-action.md#the-0x51-exit-gate-and-the-hp-bar-settle-invariant).

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

## Weapon trail builder (`FUN_8005112C` + `FUN_80048310` + `FUN_800485BC`)

The swept `POLY_G4` streak an ordinary arts swing leaves behind a party
character's blade (distinct from the mesh after-image ghosts, which only the
Super / Miracle starter dash gets).

**Trigger** (`FUN_8005112C`, called per party seat from the per-actor battle
draw tick `FUN_800480D8`): fires only while the committed action record's
`+0x77` clip-identity byte matches a per-character constant - Vahn `0x29`
(base object `0x0C`, tint `0x802040`), Noa `0x1E` (base `0x04`, `0x80FFC0`)
and `0x2A` (base `0x0A`, `0x208040`), Gala `0x64` (base `0x06`, `0x204080`) -
always with **3 control points** (the weapon bone chain `base..base+3`).

**Sweep** (`FUN_80048310`): saves the anim cursor `actor[+0x68]`, and up to 16
times re-decodes the pose at the current cursor (`FUN_8004998C`), copies the
control points' decoded object positions out of the pose pool
(`gp[0xa0c] + 0x6f4`, stride `0xC`) into a 16-step scratch, and rewinds the
cursor by `2 * record[+0x78]` - two display frames per step - stopping at the
clip start. With at least two captured steps it emits gouraud bands: segment 0
white -> `0x808080`, segment 1 `0x7F7F7F` -> black, then every segment `k` of
`n` with the trigger tint faded linearly (`rgb * (n-k)/n -> rgb * (n-k-1)/n`,
truncating division) - all semi-transparent, stacking additively.

**Band emitter** (`FUN_800485BC`, 275 instructions): per band, yaw-rotates the
two steps' local control points by `actor[+0x26]` against the sin/cos LUTs
(`_DAT_8007B81C` / `_DAT_8007B7F8`, a 12-bit angle **mask** into 4096-entry
`s16` 1.12 tables), adds the battle slot's world base
(`*(int*)(0x801C9370 + actor[+0x5A]*4) + 0x34/+0x38`), projects each vertex
through `FUN_800195A8`, and drops `0x3B808080` packets into the OT - a
**`POLY_G4`**: four-point gouraud, semi-transparent, *untextured*
(`0x808080` is a placeholder the per-vertex fill overwrites; vertices
`v0/v2` = the leading step's pair carrying the band's lead colour, `v1/v3`
trailing). Vertex products carry a `+0xFFF` bias when negative before the
`>> 12` (round-toward-zero), and the OT slot is the average of the four corner
depths with the same fixup.

**Port**: trigger table + sweep/band schedule `engine-vm::battle_trail`; the
projected band packets `engine-ui::battle_trail` (a gouraud `FlatQuad` through
the shared screen-prim pass, ABR 1); `World::battle_weapon_trail_draws`
samples the sweep off the pose-history ring (step `k` = the pose `2k` frames
ago, the retail rewind under a constant rate) bounded by the ring's per-frame
clip key. Both hosts project with their own battle camera and composite the
bands over the scene - the OT interleave with scene depth is the same
disclosed simplification as the move-FX streak.

### Move-FX streak ribbon (`FUN_801E1D98`)

The move-FX draw dispatcher has two 2D streak shapes and picks between them by call site: `0x801E0CA0` calls `FUN_801E1AB0`, the single-billboard afterimage; `0x801E0CD0` calls `FUN_801E1D98`, the chained ribbon. Both take the trail-texture id from the move-power record's `+0x0b` byte and both build the same kind of packet - a semi-transparent textured `POLY_FT4` (`0x2e808080`), texpage `0x27`, CLUT `0x7700 + trail_id`.

The ribbon starts from one `FUN_800195A8` billboard projection of the actor point - half-width `0x100`, half-height `0x200`, no in-plane spin, and no `+0x120` Y push (that push is the afterimage's, not shared). From the projected quad it derives two governing numbers:

- **Suppression.** If the projected top edge spans `0x41` px or more (`x1 - x0`, signed), the routine returns without linking anything. The packet it had already carved out of the frame arena is simply abandoned; there is no single-quad fallback.
- **Segment height.** The projected height `y2 - y0` is kept when it is at least `0x40`, otherwise `0x40` is substituted. That is a **floor**, not a cap - a tall billboard produces tall segments and therefore a shorter chain.

Every further segment reuses the previous segment's top edge as its own bottom edge, so the quads form one continuous strip, and the un-jittered baseline steps up by exactly one segment height per iteration. The walk stops when the baseline (sign-extended to 16 bits) is no longer greater than `-height`, i.e. once the strip has left the top of the screen.

The jitter law differs between the first segment and the rest, and the magnitudes are all shifts of the segment height `h`:

| Segment | `rand` draws | What each moves |
|---|---|---|
| Bottom (from the projection) | 7 | one shared `[-h/4, +h/4]` X wobble on the whole top edge, one shared `[-h/8, +h/8]` X wobble on the whole bottom edge, then four independent `[-h/8, +h/8]` Y wobbles in corner order, then the brightness band |
| Each further segment | 4 | one shared `[-h, +h]` X wobble carried across both new top corners, two independent `[-h/4, +h/4]` Y offsets off the stepped baseline, then the brightness band |

Because the X wobble is shared inside an edge, the strip keeps its width and snakes sideways rather than shearing. The brightness band is `(rand & 3) << 5`, selecting one of four `0x20`-wide texture sub-columns; the quad then samples `band ..= band|0x1f` horizontally and `0 ..= 0x3f` vertically, assigned `TL, TR, BL, BR`. That corner assignment is **mirrored relative to `FUN_801E1AB0`**, which puts the `|0x1f` edge on corners 0 and 1 - folding the two UV builders together would flip the texture on one of them.

Retail links every segment at the **same** OT bucket, the depth `FUN_800195A8` returned for the bottom billboard, so the strip is depth-flat.

Ported as `legaia_engine_render::afterimage::build_streak_ribbon` (injected rng, unit-tested); projection is `project_ribbon_corners`, and arena allocation plus OT linking stay on the retail-renderer side that engine-render replaces.


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
   `FUN_801E93C8`); Vahn's clip-`0x18` arm is tint-only (`0x90..=0xA0`). The
   overlay ribbon `FUN_801E1D98` is called by the clip-`0x67` arm, not the
   `0x18` one. Port: `engine-vm::battle_impact_fx` +
   `World::tick_battle_impact_fx`; the tint decays through the
   `FUN_80050F30` per-lane ease (`FUN_80050120` arm 0).
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

   Two parts of the pass stay out of the port. The Rot arm's per-character
   index window (`DAT_80078630`) has no parser in any crate, so only the
   Stone arm is ported; and the recolour cannot yet be *triggered* in play,
   because the port has no monster-side `enemy_effect` source - status flows
   party -> monster only, and these rows are the party's.

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

Slot indices are **absolute actor-table indices** - party ordinals below `party_count`, monsters above - and stay absolute through the draw list. `battle_hud_draws_for` derives monster-row Y and popup anchors from the slice position, so a host that hands it a compacted "active slots only" list shifts every monster row up and anchors damage numbers to the wrong actor. Inactive slots are passed through as empty-name rows, which the builder skips while still consuming their Y.

### The drawn surface

`battle_hud_draws_for` returns two lists (`BattleHudDraws`): `text` samples the
dialog-font atlas (glyphs plus the solid-texel rects), `sprites` samples the
resident system-UI atlas. Both hosts composite `sprites` under `text` in the
same slot they already use for the dialog and menu chrome.

The panel X anchors are pinned from the battle overlay (`FUN_801D84C0`'s
per-party-size anchor table - solo `0x72`, pair `0x3F`/`0xA5`, trio
`0x0C`/`0x72`; canonical port `engine-vm::battle_party_panel`), and the packet
walk confirms them as the panels' **name pen**, five pixels inside a 102x48
panel plate. Retail's own seats, rects and palettes for the whole surface are
in [screen chrome](#battle-screen-chrome-packet-pinned).

**Retail draws no HP or MP gauge at all**, in either the panel or the
full-width active-actor readout, and none for monsters either. Filled bars and
monster rows are engine additions rather than approximations of a retail
surface, and both sit behind `LEGAIA_DIAG_HUD`.

**Two party surfaces, mutually exclusive.** Retail's party readout is not one
widget. At rest each live member gets a **roster panel** - a 102x48 plate at
`y = 164`, seated at `x = 109` solo, `58` / `160` for a pair, `7` / `109` /
`211` for a trio - carrying the name and level on its top cell, then an HP row
and an MP row. The member currently entering a command or acting instead takes
the **active-actor bar**: one plate run at `(8, 188)` with a 288-px interior,
so it spans `x 8..=312`. The bar does not hide the panels by not drawing them;
retail parks the whole cluster at `y = 230`, under its 228-line display window.
Seats, sub-palettes and the 3-slice plate law are pinned in
`engine-vm::battle_chrome`; `engine-ui` mirrors the seats as literals because
it sits below `engine-vm` in the crate graph, and `engine-shell`'s HUD tests
pin the two sets equal.

A single-surface reading of the same screen - "one full-width lozenge per live
member" - is what a solo capture shows when that member happens to be acting,
and it is wrong for a resting party. The display-list walk is what separates
the two cases.

Inside the bar: name pen `(16, 192)`, the `HP` label cell at `(80, 194)`, the
current HP right-aligned to `x = 134`, a `/` sprite at `(136, 188)`, the
maximum right-aligned to `x = 178`, then the `MP` label at `(192, 194)` with
its own pair right-aligned to `238` and `274` around a `/` at `(240, 188)`.
The panel rows carry the same two fields against `CUR_RIGHT = 57` /
`MAX_RIGHT = 97`, and the level against `LV_DIGITS_RIGHT = 96`.

### Numbers are cells, names are glyphs

Every **number** on the battle screen is a run of fixed 8x12 cells off the
menu-glyph strip; only the names use the proportional dialog font. That split
is what the field geometry is built around, and it decides two things at once.

**Both halves of a `cur / max` pair are right-aligned.** Neither runs forward
from a pen. A capture whose values happen to share a digit count cannot tell
the two models apart - it takes a second capture at a different width, and
three of them disagree with the forward-running reading:

| field | 2 digits | 3 digits | 4 digits | right edge |
|---|---|---|---|---|
| bar HP maximum | - | `154` | `146` | `178` |
| bar MP maximum | `258` | `250` | - | `274` |
| panel maximum | `81` | `73` | `65` | `97` |
| panel level | `80` | - | - | `96` |

**A field's width budget is a cell count.** Four cells per HP field and per
panel field, three per bar MP field - the panel's numerals close five pixels
short of its right edge, mirroring the five-pixel inset of its name pen. A
proportional-font `9999` is wider than four cells and overruns the 102-px
plate into the neighbouring member's panel, which is the failure the cell
model removes by construction rather than by moving a column.

Retail draws **no gauge bar of any kind** on either surface - the display-list
walk carries no bar primitive in either readout.

The `LV`, `HP` and `MP` label cells are three texels in **one** sub-palette
(CLUT row 511 sub-palette 1); the gold-vs-green difference is baked into the
texels, not resolved per label, so they draw untinted.

**The plaque.** A carved-gold plate at `(8, 8)` naming the actor the frame
belongs to - the party member on his turn, the monster through its attack -
whose interior is sized to the **measured** name (plus a 20-px element badge
and a 5-px gap when the actor carries one). `name_plaque` lays it out; its
parked seat is `(8, -24)`, so retail slides it in from above. The port draws
the live seat only. This is also where the port's **monster readout** lives:
retail draws no monster gauge at all, so a monster's name is the whole of what
it contributes to the drawn surface. `battle_hud::battle_active_actor` picks
the actor and `battle_plaque_element_badge` picks the badge.

**One seat, two surfaces.** The plaque and the
[message banner](#the-full-width-message-banner) share content pen `(16, 12)`
- they are alternatives, not layers, and drawing both puts two text runs on
the same pixels. `BattleHudFrame::banner` wins when a message is up, and
`plaque_seat_taken` lets a host claim the seat for a box it draws itself
(the sparring-tutorial prompt, whose rect starts on that same pen).

**The surface samples the disc's own cells.** The 102x48 marbled panel plate,
the blue plate 3-slice, the 8x16 `/` separator and the 8x12 numerals are all
baked into the shared sprite atlas and drawn 1:1 from it - the first three off
the resident system-UI sheet (sub-palettes 0 / 4 / 5), the numerals off the
neighbouring menu-glyph atlas through sub-palette 13. The plaque takes the
carved-gold plate row, which is the same art the field menu's tab banner
already bakes. Source rects live in
[`title_pak`](../../crates/asset/src/title_pak.rs) as
`OVERLAY_SYSTEM_UI_BATTLE_*`; atlas seats in
[`save_menu_atlas`](../../crates/engine-core/src/save_menu_atlas.rs) as
`ATLAS_RECT_BATTLE_*`, all at their natural sheet coordinates except the
numeral strip, whose own row the filigree tile holds.

Without an atlas the builder still draws: plates degrade to a solid interior
with a 1-px rim, the labels and the `/` to tinted text, and the numerals to
font glyphs **centred on the same 8-px cells** - the fallback changes
letterforms, never layout.

**The status badge is retail's own cell.** When a slot's ladder selects an
ailment the HUD blits the 48x16 word tag off the sheet
([the badge sheet](#the-status-element-badge-sheet)) rather than a labelled
stand-in, at `panel + (56, 0)` - the ladder caller's `pen + (0x33, -4)` off
the panel's `(+5, +4)` name pen. The engine's tag survives as the per-cell
fallback: a host whose atlas could not reach a badge's sub-palette keeps the
tag for that one badge and blits the other eight.

**Parked, not stacked.** The port emits no panel draws at all while the
active-actor bar or a command-entry session owns the frame, rather than
drawing at retail's parked `y = 230`: the engine stage is 240 lines against
retail's 228-line display window, so `y = 230` would still be visible here.
Drawing both is what "two mutually exclusive surfaces" rules out - the bar
would sit on top of the panel row it replaces.

**Diagnostic surface** (`LEGAIA_DIAG_HUD` set to anything but `0` / empty).
Everything the port used to draw unconditionally and retail does not: monster
rows with HP numerals and thin gauge bars, the K.O. tag, the per-slot LV / AP
tail, and the "ENCOUNTER!" transition banner. Off by default on both hosts;
the toggle is read from the environment so they resolve one answer, and on
wasm the variable never exists.

The filled rects need no dedicated pipeline: `font_solid_src` locates a
solid-white texel in the dialog-font atlas and every rect is a `TextDraw`
stretching that 1x1 source under a colour tint, through the same textured-quad
pass both hosts already run for glyphs. Without either atlas the builder still
draws - the lozenge degrades to a solid interior plus a 1-px rim, the label
sprites to tinted `HP` / `MP` text at the same columns.

Two retail colour laws drive the surface, both fed the **displayed** (ramping)
HP - `BattleActor::hp_display`, retail actor `+0x172`, walked by the
quarter-step ramp `FUN_80047430` so damage drains over frames instead of
snapping:

- **Numerals** take the readout-tint law (`hp_bar_color_index` / `mp_bar_color_index`, ports of `FUN_800349EC` / `FUN_80035EA8`). A dead member's whole row dims.
- **Bar fills** take the whole-gauge law (`engine-vm::battle_gauge::gauge_colors`, port of `FUN_80046A20`): death greys the whole track, an active status forces both fills to the override colour, otherwise each bar bands independently on its floored half/quarter thresholds. Only the diagnostic rows draw bars now, so this law reaches the default surface nowhere. The index-to-RGB map (`gauge_fill_color`) is approximate - retail resolves the index through unpinned font-CLUT rows.

MP has no ceiling on the battle actor: `World::character_max_mp`, keyed by battle ordinal, is the only source, so monster rows carry `mp_max = 0` and the builder draws them no MP field.

### The status element

Retail draws **one** status marker per party slot, never a strip, and which one is a fixed priority ladder in `FUN_8002C2E4` (`ghidra/scripts/funcs/8002c2e4.txt`). Its inputs come from the display record at `0x80084140 + slot * 0x414` - which is the live character record read `0x5C8` bytes early, since `0x80084140 + 0x5C8 == 0x80084708`. So the selector's three fields are character-record fields: `+0x6F6` = `+0x12E` (the packed status word), `+0x6CE` = `+0x106` (current HP) and `+0x6F8` = `+0x130` (the displayed level).

The word itself is battle actor `+0x16E` verbatim: `FUN_80047430` mirrors it with a paired `lhu`/`sh` on both its arms (`0x80047680`, `0x80048040`). Three draws come out:

| condition | draw |
|---|---|
| word `== 0`, HP `!= 0` | base marker sprite `0x0A` at `(pen + 0x3B, pen + 2)`, then the **level** from `+0x6F8` as two digits at `(pen + 0x4B, pen)` |
| HP `== 0` | sprite `0x20`, tested before any bit - the KO marker wins outright |
| HP `!= 0`, bits set | the ladder's first match, at `(pen + 0x33, pen - 4)` |

The ladder tests `0x0004`, `0x0400`, `0x0800`, `0x0380`, `0x0078`, `0x1000`, `0x0002`, `0x0001` in that order, emitting sprites `0x1A`, `0x1D`, `0x1E`, `0x1C`, `0x1B`, `0x1F`, `0x19`, `0x18`. The band `0x18..=0x20` is nine sprites for the nine conditions the status model tracks, KO being the one that is a zero-HP test rather than a bit. Per-bit provenance is in [`accessory-passive-table.md`](../formats/accessory-passive-table.md#status-guard-clear-masks) - the seven accessory guards each clear exactly one ailment's mask, which is what fixes the assignment - and mirrored at `engine-vm::status_effects::display_flags`.

The **art agrees with that assignment**, independently: each of the nine ids is a word tag on the system-UI sheet, and decoding the cells gives `Venom` / `Toxic` / `Stone` / `Rot` / `Rage` / `Numb` / `Sleep` / `Curse` / `Faint` in ladder order. Cells, sub-palettes and the sheet law are in [the badge-sheet section](#the-status-element-badge-sheet).

Port: `BattleSlotHud::status_display_flags` packs the engine's typed status set into the retail word and `status_element` runs the ladder. The no-ailment arm is the level, and retail draws that as a **panel row** - the `LV` label cell at the panel's `(64, 6)` with its digits at `(88, 4)` - not as a floating marker. The ladder is exclusive, so any set bit (or zero HP) **replaces** that level with its own element, and the port draws the selected id as its own labelled tag on the same panel seat rather than blitting retail's cell. Three bits - `0x0040` inside the Rot group, and `0x2000`/`0x4000`/`0x8000`, which survive even Master Guard's clear - have no writer anywhere in the dumped corpus and stay unassigned.

### Enemy target strip

While a target picker's cursor is on the enemy row, both hosts draw retail's deduplicated monster-name strip instead of a debug label: `battle_hud::battle_enemy_target_rows` builds the rows off the live monster slots (identical adjacent monsters collapse into one run whose label takes the dedup-glyph suffix, `FUN_801D9D3C`), and each host runs the retail centre/relax/clamp layout (`target_picker::layout_enemy_menu_rows`) with its font as the measurer. The projected screen X the layout averages (battle actor `+0x34`) is renderer-owned and not plumbed into the HUD layer, so rows centre at `0xA0` and the retail overlap-relaxation pass spreads them - an approximation of retail's over-the-monster placement with the pass structure exact.

Implementation: [`crates/engine-core::battle_hud`](../../crates/engine-core/src/battle_hud.rs). The native window folds the live actor table into it each tick in `engine-shell`'s `window/battle.rs::sync_battle_hud_rows`; the browser play page runs the same fold in `web-viewer`'s `play_battle.rs`.

## Battle screen chrome (packet-pinned)

What retail actually draws around the fight: the actor-name plaque, the
party status readout and the command-chip cluster. Every number below is
read out of retail's own display list. A mednafen battle save state carries
main RAM verbatim, and libgpu leaves its queued primitives there as
ordering-table nodes (`[u32 tag][GP0 words]`, `tag = len<<24 | next`), so
the RAM image **is** the frame's packet stream - each `SPRT` carries its own
`(x, y)`, `(u, v)`, `(w, h)` and CLUT id inline, and the `DR_TPAGE` node
traversed before it fixes the texture page. Cross-checked against a
full-VRAM dump of the same frame (`mednafen-state vram-dump`). Port:
[`engine-vm::battle_chrome`](../../crates/engine-vm/src/battle_chrome.rs).

Anchors used: the Tetsu-tutorial progression `v0_1_battle_command_menu` /
`v0_1_battle_command_submenu`, the three-member `party_battle_gobu_gobu`,
and the solo action frames `battle_gimard_tail_fire_a` /
`battle_melee_hit_spark` / `player_steal_skeleton_pre` (see
[`scripts/scenarios.toml`](../../scripts/scenarios.toml)).

### One sheet, one 3-slice, two palettes

The whole chrome samples the **resident system-UI TIM**
([`title_pak::OVERLAY_SYSTEM_UI_TIM_OFFSET`](../../crates/asset/src/title_pak.rs),
`PROT.DAT` `0x18E0`), whose pixels upload to VRAM page `(896, 256)` and
whose CLUT block packs into VRAM row **511** as side-by-side sub-palettes -
the chrome plates use the first sixteen, and the row runs further (the
[status badges](#the-status-element-badge-sheet) reach sub-palette 18).
Text comes off the neighbouring menu-glyph atlas at page
`(896, 0)` through row **510** sub-palette 13, as 14x15 blits of 16x16 cells
(`cell = ascii - 0x20`, sixteen cells per row, `u = (i%16)*16`,
`v = (i/16)*16`) advanced by each glyph's own width.

The name plaque, the party bar and every command chip are the **same three
tiles** at two sheet rows:

| Row | Left cap / body / right cap | Sub-palette | Drawn by |
|---|---|---|---|
| `v = 0` | `(208,0)` / `(192,0)` / `(216,0)`, 8x20 / 16x20 / 8x20 | 4 (blue) | party status bar, command chips |
| `v = 64` | `(208,64)` / `(192,64)` / `(216,64)` | 12 (carved gold) | actor-name plaque |

The `v = 64` row is the art
[`title_pak::OVERLAY_SYSTEM_UI_TAB_CAP_L`](../../crates/asset/src/title_pak.rs)
already pins as the field menu's tab banner. The battle plaque and the pause
menu's title tab look like the same object because they are one asset;
`battle_chrome::gold_plate_matches_tab_banner` asserts the equality.

A run is composed left-to-right: cap at `x`, 16-wide body tiles from `x + 8`
with the **final tile clipped** to the remainder, cap at `x + 8 + interior`.
A 27-pixel interior emits a 16-wide and an 11-wide tile - the clip is retail
behaviour, not a rounding of it.

### One placement record derives every plate

The plate is not stored anywhere; it is derived from a **content box**, and
the box is a record of the screen-element placement table at `0x80076C10`
([`memory-map.md`](../reference/memory-map.md#0x80076c10---one-table-three-names)).
Reading the live table out of the same save states the packets came from
gives one arithmetic that fits all four surfaces:

```text
glyph pen = (rec.x, rec.y - 2)
plate     = (rec.x - 8, rec.y - 6),  size (rec.w + 16, 20)
```

`rec.h` is `0x0C` in every initialised record, so a plate is always 20 tall,
and `rec.w` **is** the interior width - which is what makes a plate sized to
its content with the last body tile clipped. The `-8` / `-4` content-to-plate
bias is the same one `FUN_801DBC30` applies when it frames a box.

| Surface | Record `(x, y, w)` | Glyph pen | Plate |
|---|---|---|---|
| actor-name plaque | `(16, 14, 63)` | `(16, 12)` | `(8, 8)` 79x20 |
| active-actor bar | `(16, 194, 288)` | `(16, 192)` | `(8, 188)` 304x20 |
| `Item` chip | `(204, 34, 48)` | `(204, 32)` | `(196, 28)` 64x20 |
| `Begin` chip | `(104, 88, 36)` | `(104, 86)` | `(96, 82)` 52x20 |

The plaque is record **68** (`0x80077270`, element id pair `0x2323`, kind
`0x0202`), pinned by width rather than by name: across three states the
record's `w` tracks the measured plaque interior exactly - 27 for `Vahn`, 62
for `CheDelilas`, 63 for `Gimard` behind its badge - while its live seat stays
`(16, 14)` and its parked seat `(16, -24)`. So the plaque slides in from above
the screen, and the record's `+0x14` points at the name scratch buffer the
string was measured out of (a party-name buffer for a member, a monster-name
buffer for an enemy).

The per-member roster panel is the exception that proves the rule: it is a
fixed 102x48 sprite rather than a plate run, so its own record (`w = 88`,
`h = 50`) insets by `(-5, -6)` and widens by 14 instead.

The table is **disc data** - it is initialised rodata in the executable's data
segment, and the runtime writes back only the measured width, the string
pointer and the live seat while an element slides. Parser
`legaia_asset::screen_elements`; the disc-gated oracle
`crates/asset/tests/screen_elements_real.rs` re-decodes it off the user's
`SCUS_942.54` and asserts each seat above.

### The actor-name plaque

Fixed seat `(8, 8)`, 20 px tall, in every battle. It names whichever actor is
currently acting - the party member on their turn, the monster on its - which
is why the same surface reads `Vahn` in one frame and `Gimard` in the next.

The interior is exactly its content, so the plate is sized to the name:

- no badge: `interior = name width`, first glyph at `(16, 12)`;
- with an element badge: `interior = 20 + 5 + name width`, badge at
  `(16, 12)`, first glyph at `(41, 12)`.

Captured widths: `Noa` -> 20 (right cap at x=36), `Carl` -> 23, `Zeto` -> 24,
`Vahn` -> 27 (cap at 43), `CheDelilas` -> 62, `Gimard` behind a badge -> 63
(cap at 79). Total plate width is `interior + 16`.

The **element badge** is a 20x12 sprite off the same sheet: eight badges at a
32-texel pitch from `u = 6`, row `v = 192`. Each takes its own 16-entry
sub-palette out of the CLUT block at VRAM x `896..`, rows 498 / 499 - so the
colour is per-element and the geometry is not. The selector is the badge
record's own palette byte, `0x40 + index`, decoded two-dimensionally; the
winged `v = 208` strip is a separate set of eight 28x12 records on its own
CLUT block. Both are pinned in
[the element-badge section](#the-element-badges-and-their-per-badge-palette).

### The party status readout - and it has no gauge

Two mutually exclusive surfaces, and **neither draws a bar**. There is no HP
or MP meter primitive anywhere in either packet run: a label sprite, numerals
and a separator, nothing else. A filled gauge in a party HUD is an engine
invention.

**The active-actor bar** is a full-width blue run at `(8, 188)`, interior 288,
spanning `8 ..= 312`. It appears while one actor holds the screen - entering
their command, or playing their action out - and shows that actor only:

| Piece | Seat |
|---|---|
| name glyphs | `(16, 192)` |
| HP label sprite `(208,86)` 16x10 | `(80, 194)` |
| HP current, right-aligned | ends x=134, `y = 192` |
| HP `/` separator `(96,64)` 8x16, sub-pal 5 | `(136, 188)` |
| HP maximum, left-aligned | starts x=154 |
| MP label sprite `(224,86)` | `(192, 194)` |
| MP current / separator / maximum | ends 238 / `(240, 188)` / starts 258 |

Numerals are 8x12 cells at `v = 208`, `u = digit * 8`, off the font page
through sub-palette 13. The separator sprite sits four rows **above** the
numerals it separates.

**The roster panels** are the default: one 102x48 marbled plate per member,
texels `(0, 0)` of the same sheet through sub-palette 0, at `y = 164`, seated
at x `109` (solo), `58` / `160` (pair), `7` / `109` / `211` (trio) - a
102-pixel pitch. Content is panel-relative: name `(+5, +4)`, LV label sprite
`(192,86)` at `(+64, +6)` with its digits at `(+88, +4)`, HP label at
`(+4, +21)` and MP label at `(+4, +36)`, each row's current value ending at
`+57`, separator at `(+57, y-4)`, maximum starting at `+73`.

The panel seats are the same layout `FUN_801D84C0` publishes - its
per-party-size anchors (`0x72`; `0x3F`/`0xA5`; `0x0C`/`0x72`, port
[`battle_party_panel::panel_anchors`](../../crates/engine-vm/src/battle_party_panel.rs))
are the **name pen**, five pixels inside the panel background. When the
active-actor bar takes over, the panels do not stop drawing - they move to
`y = 230`, below the 228-line display window, the same park row the arts
input screen uses ([`minigame-muscle-dome.md`](minigame-muscle-dome.md#arts-command-input-packet-pinned)).

### The command chips

Chips are blue plate runs around a D-pad glyph - texels `(0, 112)` 16x16,
sub-palette 7, drawn 15x15 as a textured quad centred on the cluster. Every
chip in one cluster is built at the **same** interior width; a chip is not
sized to its own label, and the label is left-aligned at the interior's left
edge, four rows down.

| Cluster | Centre | Chip interior | Seats |
|---|---|---|---|
| `Begin` / `Run` | `(160, 92)` | 36 | horizontal pair, plates at x=96 and x=172, y=82 |
| per-actor commands | `(228, 70)` | 48 | four-way diamond, `dx = 44`, `dy = 32` |

The command diamond seats `Item` up `(196, 28)`, `Attack` left `(152, 60)`,
the element command right `(240, 60)` and `Spirit` down `(196, 92)`. An
unavailable command still gets its chip - the right seat draws a single `-`
glyph for a character with no magic. The `Begin` / `Run` cluster is seat- and
size-identical in a solo tutorial fight and in a three-member battle.

The dome's element table names the same four seats from a second, unrelated
capture (`(204, 34)` / `(160, 66)` / `(248, 66)` / `(204, 98)` through the
plate law above), which is what says this cluster is the battle command menu's
and not a per-mode variant - see
[`minigame-muscle-dome.md`](minigame-muscle-dome.md#the-command-cluster-is-the-battle-cluster).
Note the two "cannot pick this" marks are different widgets: the `-` glyph is
an unavailable command, while a *forbidden* one wears the red cross-out X
(`FUN_801DBC30`, port `battle_party_panel::cross_out_mark`).

**Port.** The cluster's draw side is
[`engine-ui::battle_command_ui`](../../crates/engine-ui/src/battle_command_ui.rs) -
plate run, both clusters, the shared D-pad glyph cell and the `-` chip - and
both battle hosts seat their command menu through it, so the menu is chips
rather than a text list on either. Every chip sits on a pinned arm: the two
clusters are three **phases**, not two rows of one menu, and the phase a frame
is in ([`ChipPhase`](#the-battle-open-flow---ctx0x06-from-the-intro-timer-to-the-first-swing))
is what names the seats. The `engine-ui` literals are pinned equal to
`battle_chrome` by `engine-shell`'s
`engine_ui_command_chips_mirror_the_packet_pinned_battle_chrome`.

## The widget-class table - where every chrome sprite comes from

Everything the packet walk measured above is **disc data**, and it all comes
out of one array: the widget-class table at `SCUS_942.54` VA `0x800732A4`,
`0x0C` bytes per record, `0x9D` records. The run's end is structural rather
than guessed - `0x800732A4 + 0x9D * 0x0C` is exactly `0x80073A00`, the frame
tile-set pool the class arms read next. Parser `legaia_asset::ui_widgets`;
disc-gated oracle `crates/asset/tests/ui_widgets_real.rs`.

A [screen-element placement record](../reference/memory-map.md#0x80076c10---one-table-three-names)'s
`+0x0E` *kind pair* is two indices into this table - which is what turns the
chrome section's correlation ("`0x0101` is on every blue chip, `0x0202` on the
gold plaque") into a mapping. Kind `0x01` **is** widget record `0x01`, the blue
plate body; kind `0x02` is record `0x02`, the carved-gold one. The join holds
for all 103 initialised placement records: every kind byte, high and low,
names a real widget record, and each named surface resolves to the art the
packets drew.

### Record layout

| Offset | Type | Field |
|---|---|---|
| `+0x00` | u8 | frame **class** - which layout arm draws it (`0..=6`, jump table `0x80010D18`) |
| `+0x01` | u8 | **tile-set** index into the frame pool at `0x80073A00` |
| `+0x02` | i8 | **chain delta** to the next record in this widget; `0` ends the run |
| `+0x03` | u8 | **palette** byte - bit 7 semi-transparent, the rest a packed CLUT address |
| `+0x04`..`+0x07` | u8 x4 | source rect `u`, `v`, `w`, `h` on the system-UI sheet |
| `+0x08` / `+0x0A` | i16 | seat bias `dx` / `dy` |

Two SCUS routines read it. `FUN_8002C488(x, y, id)`
(`ghidra/scripts/funcs/8002c488.txt`) draws exactly one sprite and seats it at
the caller's `(x, y)` **verbatim** - it never applies `+0x08`/`+0x0A`.
`FUN_8002C69C(x, y, w, h)` (`ghidra/scripts/funcs/8002c69c.txt`, the
`POLY_FT4` / `SPRT` emitter) draws a sized widget with the record index in
`gp+0x14C`, applies the bias, and then loops: `lb v1, 0x2(s7)` at `0x8002FF00`,
`addu` it into the index, and re-enter at `0x8002C780` unless it is zero.

The `(x, y)` it is called with is the **glyph pen** - the content box's
`(x, y - 2)` - not the box seat, and the bias converts pen to frame origin.
One law covers both frame families: the plate run's `(-8, -4)` takes the
plaque's pen `(16, 12)` to the documented plate at `(8, 8)`, and the framed
window's `(-8, -8)` takes a banner pen `(16, 12)` to a frame at `(8, 4)`.
Both are packet-confirmed (see the message banner below).

That split is why the same table produces both behaviours the packet walk saw:
the status marker lands at `pen + (0x3B, 2)` because its caller
(`FUN_8002C2E4`) supplies that offset, while the roster panel's `HP` label
lands at `pen + (-1, 17)` because record `0x07` carries it.

### The palette byte is a packed CLUT address

Both routines decode `+0x03` with the same six instructions, and it has two
forms:

```text
bit 6 clear:  CBA  = 0x7FC0 + (b & 0x3F)      -> VRAM row 511, x = (b & 0x3F) * 16
bit 6 set:    fb_y = 498 + ((b & 0x3F) >> 2)
              fb_x = 896 + (b & 3) * 16
```

The first form is the system-UI sheet's own sub-palette strip on VRAM row 511
(the chrome's blue is sub-palette 4, the carved gold 12, the marbled panel 0).
The second addresses a **separate 4-wide block of CLUTs at VRAM
`(896.., 498..501)`**, and it is the whole answer to the element-badge palette
question - see below.

Bit 7 selects the GP0 code: `0x66` (raw sprite) instead of `0x64`.

### Chains: a widget is a run of records

`+0x02` is a signed hop, so one kind draws several sprites. The two the chrome
section describes are both chains, and following them reproduces the captured
seats exactly:

| Kind | Chain | What it lays out |
|---|---|---|
| `0x2B` | `0x2B → 0x2C → 0x2D → 0x2E → 0x2F` | the active-actor bar: `HP` label `(+64, +2)`, `/` `(+120, -4)`, `MP` label `(+176, +2)`, `/` `(+224, -4)`, then the blue plate body |
| `0x07` | `0x07 → 0x08 → 0x09` | a roster panel: `HP` row `(-1, +17)`, `MP` row `(-1, +32)`, then the 102x48 marbled plate at `(-5, -4)` |
| `0x33` / `0x34` / `0x35` | `→ 0x41 → 0x42 → 0x08 → 0x09` | the same panel with its level / status marker, one kind per party slot |

Against the bar's own pen `(16, 192)` those biases give `(80, 194)`,
`(136, 188)`, `(192, 194)`, `(240, 188)` - the four seats the packets carry.

### Classes and the frame pool

The class byte picks the layout arm. Two matter for the battle screen:

- **class 3** - the rounded **plate run**. It reads a `(left cap, right cap)`
  quad pair from `0x80073A60 + tileset * 8`; tile-set 3 gives
  `(208, 0, 8, 20)` / `(216, 0, 8, 20)` (blue) and tile-set 4
  `(208, 64, ...)` / `(216, 64, ...)` (gold). Body tiles come from the
  record's own rect. Tile-set `0` is the sentinel the arm skips, so a
  cap-less run is expressible.
- **class 0** - the rectangular **9-slice window**. It reads eight quads from
  `0x80073A00 + tileset * 0x20` in the order top-left, top-right,
  bottom-left, bottom-right, top, bottom, left, right. Tile-set 0 is the gold
  border: 4x4 corners and 24x4 / 4x24 edges cut from one 32x32 patch at
  texels `(160, 0)`.

The two views overlap by construction - a cap pair *is* the last two quads of
a frame set - which is why `0x80073A60` sits three tile-sets into the pool.

### The full-width message banner

The top-of-screen banner every battle message uses is a class-0 window on the
same seat, and it is packet-pinned mid-fight (the `rim_elm_gimard_seru_capture_after`
and `noa_levelup_banner` states). Content pen `(16, 12)`, frame origin
`(8, 4)`, left / right border columns 4 wide, interior 20 tall - so the frame
is 28 tall and its right column starts at `16 + measured_width`. The top and
bottom edges tile 24 wide from `x = 12` with the final tile clipped, exactly
as a plate run clips its last body tile.

Retail draws **no interior fill** under it: the display list carries the
border sprites and the glyph run and nothing else, so the scene shows
through. The 32x32 blue-marbled patch records `0x03` / `0x04` carry as their
own rect is a fill the framed *menu* windows use, not this banner.

The same frames catch the actor-name plaque parked: a gold plate run at
`(8, -30)` with a 27-pixel interior (cap, one 16-wide body tile, one clipped
to 11) and `Vahn` on the pen at `(16, -26)`. That is placement record 68's
disc-side parked seat `(16, -24)` through the pen and bias law above, and it
is the clip rule and the plate arithmetic confirmed in one packet run.

**Port.** [`engine-ui::battle_hud_chrome`](../../crates/engine-ui/src/battle_hud_chrome.rs)
carries the geometry (`banner_frame` / `banner_interior`, the tiled-edge
emit, no fill) and the HUD builder draws it in place of the plaque. What
feeds it is the port's two battle messages, level-up and Seru-capture - the
`noa_levelup_banner` state is one of the two the geometry came from. The port
raises both a mode-tick **after** the fight has handed the frame back to the
field, where retail raises them on the battle result screen, so the message
takes the banner wherever the port raises it and the widget is not gated on
battle mode. A multi-line message grows the interior by the 14-px text pitch
per extra row and nothing else moves.

### The status-element badge sheet

The nine ids the exclusive status ladder emits, `0x18..=0x20`, are **48x16
cells in a two-column block** on the system-UI sheet, and each takes its own
row-511 sub-palette. The art is a word tag, not an icon, which is what settles
the ladder's per-bit assignment independently of the accessory-guard argument:

| Sprite | Mask tested | Sheet cell | Sub-palette | Reads |
|---|---|---|---|---|
| `0x18` | `0x0001` | `(0, 48)` | 9 | `Venom` |
| `0x19` | `0x0002` | `(48, 48)` | 10 | `Toxic` |
| `0x1A` | `0x0004` | `(48, 80)` | 16 | `Stone` |
| `0x1B` | `0x0078` | `(48, 112)` | 14 | `Rot` |
| `0x1C` | `0x0380` | `(0, 96)` | 17 | `Rage` |
| `0x1D` | `0x0400` | `(0, 64)` | 11 | `Numb` |
| `0x1E` | `0x0800` | `(0, 80)` | 15 | `Sleep` |
| `0x1F` | `0x1000` | `(48, 64)` | 13 | `Curse` |
| `0x20` | HP `== 0` | `(48, 96)` | 18 | `Faint` |

The block's tenth cell (`(0, 112)`) is other art - there is no tenth badge.
The KO badge reading `Faint` is the confirmation that the zero-HP arm and the
bit ladder are one selector over one sheet.

Two corollaries the sheet forces. Row 511's sub-palette strip is **wider than
sixteen**: these badges alone reach index 18, so the strip runs to VRAM x 288,
and the "sixteen side-by-side sub-palettes" reading above describes the block
the chrome plates use, not the row's extent. And the no-ailment arm's marker,
sprite `0x0A`, is a plain 16x10 `LV` label at `(192, 86)` on sub-palette 1 -
the same three-texel label set as `HP` and `MP`.

**Where the strip's continuation lives.** The system-UI sheet's own CLUT
block is `16 x 16` at VRAM `(0, 511)` - sub-palettes 0..15 and no more. Sub-
palettes 16 / 17 / 18 come from a separate **CLUT-only TIM** immediately
before it at `PROT.DAT[0x1858]` (`0x1858 + 0x88 == 0x18E0`), whose block is
`16 x 3` at VRAM `(256, 511)` and whose image block is a four-word stub. So
"the strip runs to VRAM x 288" is a second file, not a wider first one - and
an atlas bake rooted at the sheet cannot see it, which is why the port's
badge accessor answers per cell. Constants + bake:
`engine-core::save_menu_atlas::SYSTEM_UI_CLUT_EXT_TIM_OFFSET`.

### The element badges and their per-badge palette

The badge strip is eight consecutive records, `0x8B..=0x92`: `20 x 12` at a
32-texel pitch from `u = 6`, row `v = 192`, exactly as the packet walk
measured. Their palette bytes are `0x40 + index`, and the bit-6 decode turns
that single walking byte into a 4-wide by 2-tall block of CLUTs:

```text
badge i -> palette 0x40 + i -> CLUT ( 896 + (i % 4) * 16 , 498 + i / 4 )
```

Which reproduces every captured pair - `u = 6` with `(896, 498)`, `38` with
`(912, 498)`, `166` with `(912, 499)`, `230` with `(944, 499)` - from the disc
alone. The pairs looked unrelated to the badge index because the index is
encoded **two-dimensionally**: the low two bits pick the column, the next two
the row. The palette does travel with the badge; it just travels through a
packed address rather than a lookup.

A sibling strip of eight *winged* badges lives at `0x94..=0x9B`, `28 x 12` from
`u = 2` on row `v = 208`, on the second CLUT block (`0x48 + index`, rows
500 / 501 - byte-identical to 498 / 499 in a live frame). Record `0x9B` is the
one asymmetry: it reads `v = 192`, so the eighth wide badge samples the
square-framed art on the plain row while its seven siblings sample the winged
row. The winged eighth badge exists in VRAM and no record selects it.

**Neither strip's texels are on the system-UI sheet.** Rows `v = 192` and
`v = 208` are past that TIM's 192, and belong to the **extension strip** that
continues the page at VRAM `(896, 448)`
(`title_pak::OVERLAY_SYSTEM_UI_EXT_TIM_OFFSET`, strip `v` = sheet `V - 192`).
Each of the four CLUT rows `498..501` is a whole sibling TIM of its own -
`0x10178` / `0x100D0` / `0x10028` / `0xFF80` - so a badge's palette is
`(row TIM, index & 3)`. The port bakes the plain eight from the first two
(`save_menu_atlas::add_element_badge_sprites`); the winged four on row 500
are already baked as the status screen's ATR icons, which is the same art.

**Port + what is inferred.** The plaque wears badge `element` for a monster
whose record `+0x1D` names one (`battle_hud::battle_plaque_element_badge`),
and the plaque widens by `20 + 5` exactly as `name_plaque` lays out. The
geometry, the palette decode and the plaque law are all disc-read; **the
selector is not** - no dumped caller computes the badge id, so "badge index
= element id" is an inference from the two eights lining up, and whether a
neutral (id 7) actor draws a badge at all is unverified.

### Four ids are not on this sheet at all - they are the save-slot portraits

`FUN_8002C488` has a second arm for ids `0x86`, `0x87`, `0x88` and `0x8A`.
They draw through texture page `0x1F` (VRAM `(960, 256)`) instead of `0x1E`
(`(896, 256)`), take their CLUT from the four-word side table at `0x80073DB8`
instead of their palette byte, and are the only ids whose `+0x08`/`+0x0A`
bias appears on the *single-sprite* path - though only `0x8A` carries a
non-zero one, `(-8, -8)`, which centres the 32x32 frame on the same seat a
16x16 face takes.

They are **not an undrawn surface**. The side table reads `(976, 304)`,
`(976, 305)`, `(976, 306)`, `(976, 307)`; the records' rects
(`(64|80|96, 0, 16, 16)` and `(64, 16, 32, 32)`) address VRAM `x = 976 + u/4`
at 4bpp, i.e. `(976..988, 256..272)` and `(976..984, 272..304)`. Those are
exactly the framebuffer coordinates of the four load-screen TIMs at
`PROT.DAT[0x1AC90]` and `[0x1AED0]` - the three party-member face portraits
and the empty-cell frame the save-slot grid already draws
(`title_pak::OVERLAY_LOAD_PORTRAIT_TIM_OFFSET`, port
`engine-ui::ui_title_save::slot_grid`). One asset, two consumers.

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

### The session is a bracket, not the roll

On a scene whose MAN carries encounter *regions* - which is every field area
that fights - the roll does not come from the session at all. It comes from
`RegionEncounterTracker` (the faithful `FUN_801D9E1C` model: per-region rate
counter, formation-range pick, one-step anti-repeat), and the session supplies
only the `Transition -> Triggered -> Battling -> Grace` bracketing around it.

That asymmetry has a failure mode worth naming, because it does not look like
one from either side. The region tracker's trigger branch is **destructive**:
it draws RNG, latches the anti-repeat formation and re-seeds its counter before
returning the pick. A host that dropped `World::encounter` after scene entry -
`World::begin_new_game` clears it, and `play-window --seed-party` runs that
*after* `enter_field_live` - therefore left the tracker rolling into a null
sink, and each roll was a fight that happened and was then thrown away, with no
transition drawn and nothing logged. `World::on_field_step` now re-installs a
bare bracket (`World::install_encounter_bracket`) rather than dropping the
pick, and every remaining way a roll can fail to become a battle logs at error:
an unregistered formation in `begin_encounter_battle`, a scripted arm with no
session, and a table/def id mismatch caught at `install_man_encounter` time.

The two id spaces the roll crosses - the MAN formation-row index the roll
produces and the `World::formation_table` key the battle load resolves - are
pinned equal across the whole scene corpus by
[`crates/engine-core/tests/scene_encounter_formations_disc.rs`](../../crates/engine-core/tests/scene_encounter_formations_disc.rs),
which also carries the New-Game-reset regression.

`World::force_encounter(row)` arms a named row through that same bracket. It is
the engine side of `play-window --battle` ([playing-and-viewing.md](../guides/playing-and-viewing.md#getting-into-a-battle-on-purpose)),
and it deliberately does not shortcut into `enter_battle_from_formation` - a
harness that skips the path it verifies proves nothing about it.

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

The **enemy** row is not a slot-order walk. Each picker row carries the slot's battle-world seat (`actor[+0x34]` / `+0x38`, filled by `World::battle_target_rows` from the actor's `move_state`), and a `SingleEnemy` cursor steps through retail's attack-target ring - `FUN_801D8A88` builds the ring and `FUN_801D8D00` steps it, so Left/Right move to the *angularly* nearest live monster. Retail seats at most four monsters, so a fifth engine slot has no ring entry; that slot, an un-seated host (all seats at the origin), and a ring entry that is not a live monster each fall the cursor back to the plain scan. See [`battle-action.md`](battle-action.md#actor-pool-leaf-helpers) for the two kernels.

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
Chrome: `engine_ui::arts_input`, drawn by both hosts off the shared baked
system-UI atlas. `World::arts_input_active()` / `arts_input_actor()` tell a
host's party surface that an actor owns the pad - retail parks the status
plate off-screen for the whole session. The older saved-chain list stays
reachable behind `LEGAIA_ARTS_SAVED_LIST=1`.

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
- **Battle tick** (`World::live_battle_tick`): wraps `step_battle` with the host-side glue the retail engine performs through its render + animation systems, so the battle resolves from `tick` alone. It folds this frame's `BattleEvent::ApplyArtStrike` damage into target HP; applies a generic physical strike (`apply_basic_attack`, through the retail melee roll pair `battle_formulas::physical_predamage` - see [battle-formulas](battle-formulas.md#the-melee-roll-pair-and-the-underdog-rewrite)) on the `AttackChain → AttackRecovery` edge when no art strike did; marks zero-HP combatants dead so the SM's wipe scan resolves; clears `ADVANCE_DONE` at `AttackRecovery`; and re-arms the next party attacker at `EndOfAction`. On `StepOutcome::BattleComplete` it calls `World::finish_battle`.
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
- **Round 1 is picked the same way.** Battle setup seeds every living actor's key and consumes none, then takes the opening turn from the same max-key selector. Slot 0 is not hand-armed: a fast party member can open ahead of Vahn, a fast monster can open on the party, and a rolled **back attack** cashes in - the side lockout zeroes the party's keys and the monsters therefore lead, which is the advantage's whole effect.
- Party SPD is the **resolved** stat - base plus the equipment table's footwear bonus - written by `World::seed_party_battle_stats` at battle entry, over the raw record value `World::load_party` seeds at boot. Monster SPD comes from `MonsterDef::speed` (record `stats[5]`, unboosted) at battle setup.
- When **no** living actor carries SPD - the disc-free / synthetic case where speed data hasn't been loaded - the selector falls back to round-robin slot order (`World::next_living_combatant`), which keeps the synthetic loop deterministic.

All six commands - **Attack**, **Arts**, **Magic**, **Item**, **Spirit**, **Run** - are wired into the live loop. Attack opens a target cursor and commits a physical strike through the action SM. Arts / Magic / Item resolve to `Resolution::OpenArtsMenu` / `OpenSpellMenu` / `OpenItemMenu` - the command session can't run those pickers itself (they need the caster's saved chains / learned spells / live MP / inventory + party stats), so it hands off to a host-owned submenu. Spirit and Run resolve immediately (no target):

- **Spirit** charges the caster's AP gauge (`ApGauge::charge_spirit`, the retail Square-press +5) and raises a per-slot guard stance (`World::battle_guarding`, the engine model of the retail pending-action byte `+0x1DE == 4`) that halves incoming damage through the finisher's guard stage until the actor's next turn starts; the turn is consumed (SM parked at `EndOfAction`).
- **Run** rolls the escape and arms the ported run band (category 5 → `RunBegin`/`RunWait`/`RunEscape`): success tears the battle down `Escaped` (no loot, no game over, downed members floored alive at 1 HP), failure consumes the turn. The roll is the decoded `FUN_801E791C` formula - party `(SPD*3)>>1 + missingHP>>4` vs enemy `SPD + missingHP>>5`, two rand draws, Chicken Heart / Chicken King passives honoured (`battle_formulas::escape_roll`; see [battle-action.md](battle-action.md#spirit--run-in-the-live-command-menu)).

The submenu hand-offs:

- **Item** opens a battle-context `inventory_use::InventoryUseSession` on
`World::battle_item_menu` (built by `World::build_battle_item_session` from the
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
`World::battle_escaped` so the item tick returns to the field via
`finish_battle` (no loot).
- **Magic** opens a `battle_magic::BattleSpellSession` on `World::battle_spell_menu` (built by `World::build_battle_spell_session` from the caster's learned spells off their roster record + live MP, MP-gated). The picker kind matches the spell's `SpellTarget` shape. On confirm `World::apply_battle_spell` deducts MP once, resolves each affected slot through `spells::cast_spell` (caster magic from `World::battle_magic`, target magic-defense reusing `World::battle_defense`), and folds the outcome into the live actor table via `World::fold_spell_outcome`. All `SpellOutcome` shapes apply:
    - damage / heal / cure / revive;
    - **buffs** (`World::apply_battle_buff` writes the delta straight into the per-slot `battle_attack` / `battle_defense` / `battle_magic` scalar with refresh semantics + a per-turn timer aged in the re-arm path, reverted exactly on expiry);
    - **capture** (`World::resolve_capture` rolls vs the monster's missing-HP fraction - reliable only on a weakened Seru - downing it and logging the id into `World::battle_captures` on success);
    - and **escape** (sets `World::battle_escaped`, and the spell tick returns to the field via `finish_battle` with no loot).
    - Accuracy / Evasion / Speed buffs are tracked but have no live-loop scalar to move yet.
- **Arts** opens the per-press [Arts command input](#arts-command-input) on
`World::battle_arts_input` - the player *types* the chain, one d-pad press per
command, and the entry ends itself when the AP pool can no longer afford a
press. `World::resolve_arts_input_entry` then runs the entered buffer through
the retail matcher order (Miracle string → Super tail → per-art greedy
longest-match, unmatched directions staying plain synthetic swings) to a
per-strike **power profile** (`Vec<PowerByte>` + `EnemyEffect`) plus the list of
named arts the turn performs. `World::apply_battle_art` drives each power byte
through `crate::art_strike::apply_art_strike`, so the byte's multiplier tier +
UDF/LDF target decode, `resolve_battle_defense` picks the matching defense half,
and the art's status effect lands on a hit. Art records come from
`World::art_records`, keyed by `(Character, ActionConstant)` and populated from
disc PROT entry `0x05C4` via `World::set_art_record`.
  Because an entry runs until the pool is spent, performing **several** arts in
one turn is the ordinary case, and the performed-art list is what the shout cue
and the learn-on-use check are keyed on - once per art, not once per turn (see
[audio.md](audio.md#battle-arts-voice-shout-path-engine)). A Miracle / Super
replacement answers a single constant, its finisher.
  The legacy saved-chain list (`battle_arts::BattleArtsSession` on
`World::battle_arts_menu`, built by `World::build_battle_arts_rows` from
`World::saved_chains`) stays reachable behind `LEGAIA_ARTS_SAVED_LIST=1`. A row
there collapses to the one art whose command string the chain ends with
(`chain_matches_record`), or to a synthetic per-direction profile
(`battle_arts::synthetic_power` - Down → LDF, else UDF, tier-0 ×12, clamped to
`MAX_ART_HITS`) when no record matches. Both paths share the one
`apply_art_strike` kernel.

While any submenu is open both the SM and the command session are parked;
`World::tick_battle_arts_input` / `tick_battle_{arts,spell,item}_menu` drives it
from `World::input`. On a completed action the result is applied, the relevant
popup is surfaced (`battle_hit_fx`), and the action SM is **parked at
`EndOfAction`** so the re-arm block cycles to the next combatant - a cast / art
/ item use is the actor's whole turn, no Attack-SM strike fires. Backing out
reopens the command menu for the same actor. Implementation:
[`crates/engine-core::battle_input`](../../crates/engine-core/src/battle_input.rs)
+ [`arts_command_input`](../../crates/engine-core/src/arts_command_input.rs) /
[`battle_arts`](../../crates/engine-core/src/battle_arts.rs) /
[`battle_magic`](../../crates/engine-core/src/battle_magic.rs).

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

Capturing a monster (magic capture roll or a capture item) downs it and logs its **monster id** into `World::battle_captures`.

- `World::finish_battle` resolves these through `World::resolve_captures`: each captured monster id maps to a **Seru id** via `MonsterCatalog`'s `MonsterDef::seru_id`, and `seru_learning::record_capture` banks that Seru's capture points against `World::seru_log` for every active party slot eligible by the Seru's `learnable_mask`.
- When a slot's accumulated points cross the Seru's `learn_threshold` the taught spell id joins that character's learned list, and `World::build_battle_spell_session` unions the roster's saved spells with `seru_log.learned_spells(slot)` so a freshly-learned spell is immediately castable - no save/load round-trip needed.
- The accepted `CaptureOutcome`s are stashed in `World::last_capture_outcomes` (`drain_last_capture_outcomes`); `resolve_captures` also builds the first accepted capture into `World::current_capture_banner` (a `seru_learning::SeruCaptureSession`), the sibling of `World::current_level_up_banner`.
- `World::tick` advances the banner one frame per call and clears it when the session reaches `Done`, so it plays out over the field after the battle ends. The session's `current_banner()` yields the active line (`"Captured: <Seru>!"` then per-learn `"<char> learned <spell>!"`); the play-window renders it via `legaia_engine_render::capture_banner_draws_for`.
- `resolve_captures` always drains `battle_captures`; with an empty `World::seru_registry` (the default) it banks nothing - the monster is still downed, but no Seru is learned.
- Capture-point progress (including sub-threshold totals) persists through `World::save_full` / `load_full` as `(seru_id, points)` pairs in each `CharSaveExt::seru_captures`; reload restores the points and, with the registry installed, re-marks any over-threshold Seru as learned.
- The `MonsterDef::seru_id` mapping + `learn_threshold` / `capture_points` values are clean-room approximations (`SeruRegistry::vanilla`); pinning the real per-monster Seru attachments and capture arithmetic is gated on the still-uncaptured stat-grant table loader (see [`crate::capture_observations::battle_init_overlay`]).

### What the loop flag does and does not gate

`World::live_gameplay_loop` gates the **field side only** - the step-driven random-encounter roll. Once the world is in `SceneMode::Battle`, `World::tick` always drives the full `World::live_battle_tick`, regardless of the flag, because a battle that cannot resolve is a soft-lock. Retail has no "loop enabled" concept either: `FUN_801E295C` drives the battle it is in.

That asymmetry is not cosmetic. Battle **entry** was never gated - a field carrier's scripted `3E FF` fight and a world-map region encounter both flip the mode on their own - so gating battle **driving** left the ungated entry paths able to strand a session in `SceneMode::Battle` with no damage applied, no turn armed and no `finish_battle`. Regression: `crates/engine-core/tests/battle_always_resolves.rs`.

### Host-simulated animation edges

Two action-SM gates are driven in retail by the render / animation systems and by nothing in the port, so `World::live_battle_tick` retires each on the frame its state is reached:

- `ADVANCE_DONE` at `AttackRecovery` - retail clears it when the recovery animation finishes.
- The caster's `spell_iter` (`actor+0x1FA`) at `MagicSustain` (`0x2B`). The SM only ever *sets* this byte; retail's cast-animation system counts it down. Without the edge, `magic_sustain`'s `stay` held forever, so **any battle in which a monster or party member cast a spell stopped dead** - which is most real encounters, and is a large part of what "battles don't work" looked like from the outside. Regression: `a_spell_cast_does_not_park_the_action_sm` in `crates/engine-core/tests/battle_always_resolves.rs`; the real-data version is `crates/engine-shell/tests/scene_encounter_rollable.rs`, which drives a `map03` encounter from the disc's own region table through to a resolved battle.

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
- `--no-player-battle` turns off the command menu, auto-attacking each party turn instead. By default battles are player-driven and the HUD renders party/monster HP plus the command menu / target cursor / arts + spell + item submenus (the host installs the vanilla spell + item catalogs and, when the boot save has none, seeds a couple of demo saved chains plus a few demo items - Healing Leaf + Bomb - so the ally-heal and offensive item paths are both exercisable).
- `--battle-bgm <id>` enables the Battle↔Field music swap: the live loop cross-fades to `<id>` on encounter and resumes the field track on battle end (the id is routed through the same director as field op-`0x35` starts, so it must resolve in the current scene's BGM table - the live loop doesn't load a separate battle audio bundle). The browser twin is `LegaiaRuntime::set_battle_bgm`.

### Battle end, both hosts

`World::finish_battle` is what a resolved battle runs, and three of its results are now read:

- **Party HP / MP persists.** The battle mutates the `BattleActor` mirrors; `finish_battle` writes them into the roster records (via `World::save_party`) *before* restoring the field actor snapshot, then pushes them back onto the restored party actors (`World::resync_party_actors_from_roster`). Without that step every fight ended at the HP it started with, and losing was indistinguishable from winning.
- **A wipe raises `World::game_over`**, which both hosts read and route to the **title screen** - retail's destination, pinned to the `game_mode = 0x16` / `_DAT_8007BB00 = 1` store pair (see [§ party wipe](#party-wipe--the-game-over-overlay)). Native pushes `BootUiState::GameOver`, the browser arms the same `GameOverSession`; neither draws anything and neither reads a button, because retail asks the player nothing here.
- **A victory arms the spoils panel** (`World::battle_spoils_banner`, `World::SPOILS_BANNER_FRAMES`), drawn by the shared `engine-ui` builder `battle_spoils_draws_for` on both hosts. The XP / gold / drops were always applied; nothing showed them.

### Scenes that cannot roll

`World::scene_can_roll_encounters` (cached as `World::scene_encounters_rollable`) answers whether the installed scene can produce a random encounter at all. Region lookup stops at the **first** containing region (`RegionEncounterTable::region_at_tile`, matching retail's walk), so a rollable region whose every tile is covered by an earlier rate-0 row is unreachable - which is the case for `town01`, the scene the binary boots into. That is retail scene data and the port keeps it; both hosts say so instead, so a town's designed silence does not read as a broken engine.

The two hosts say it through different channels, and the difference is load-bearing. The native window draws a bounded HUD line (`World::show_encounter_hint`). The browser prints its notice from the page's status bar off `LegaiaRuntime::scene_rolls_encounters` - **not** through the overlay draw list, because the page treats a non-empty overlay as owning the frame (it clears the canvas and returns before the dialog layer), so a passive hint routed there would suppress every NPC dialogue for the first seconds of a town.

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

## Additional SCUS battle-band helpers

Small `SCUS_942.54` routines the battle tick and scene-init reach through the
actor / mode tables (no static caller). Roles are read off the stores in each
bare-hex dump under `ghidra/scripts/funcs/`; where a purpose is inferred it is
stated by the concrete writes.

| Function | Role |
|---|---|
| `FUN_80055B6C` | Battle scene initializer: clears the actor/effect pools, resolves the party-slot composition (dedup + fill from `DAT_8007BD0C..`), sizes the LZS scratch, allocates the `0x7A34`-word monster-object arena at `_DAT_801C9370`, and programs the disp/draw environment. |
| `FUN_80055B20` | Seeds the fallback party-slot id table `DAT_8007BD10 = {1, 2, 3}` (Vahn/Noa/Gala); `FUN_80055B6C` overwrites it from the live party. Slot bytes index character records as `(id-1)*0x414`. |
| `FUN_80054A6C` | Battle party-file loader: builds the `data\battle\` filename (`s_data_battle_800153B8`), then streams each live party member's player battle file keyed on the party-id table `DAT_8007BD0C` at file stride `(id-1)*0x14000`. Dual-mode on `_DAT_8007B8C2`: retail ISO9660 (`FUN_800608F0`/`FUN_80060920`/`FUN_80060944` async CD reads) vs dev PROT-TOC (`FUN_8003E8A8`/`FUN_8003E964`/`FUN_8003E800`, entry `0x365`); bumps the loaded-count `DAT_8007B649`. CD/loader I/O infra - documented, not ported. |
| `FUN_800480D8` | Per-actor battle tick / teardown: on the scene-clear byte `gp[0xA0C]+0x272` (guarded by `DAT_8007BD71 == -1`) runs the four overlay shutdowns and voids the effect-node table `DAT_801C90F0`, else forwards to the tint pass `FUN_8004A908` and the death / `0x808080` greyscale path. |
| `FUN_8004A908` | Battle-actor tint: writes the colour word `+0x74` and blink halfword `+0x78` from the actor's transformed depth vs the monster-object depth threshold, with hard overrides for the `+0x16E` status bits (`0x01`→red, `0x02`→red-violet, `0x380`→magenta) and a greyscale-invert path gated on `DAT_8007BDA8`. The two arithmetic cores are ported (with tests): the per-channel depth-brightness ramp as `scus_battle_helpers::depth_cue_scale_channel` (min-4 dim floor, clamp-to-base), the negative-colour recolour as `scus_battle_helpers::invert_bgr24`. The GTE transform (`FUN_8003D344`) and colour-word packing stay render-track. |
| `FUN_80046A20` | **Not a small helper** - this is the battle-scene per-frame tick (2576 bytes, 644 instructions), listed here only because the rows below are the routines it drives. It calls the scene loader `FUN_800520F0`, the seat stager `FUN_800513F0`, the party-file loader `FUN_80054A6C`, the main dispatcher `FUN_801D0748`, the action SM `FUN_801E295C`, the separation driver `FUN_80051078` and the actor-presentation tick `FUN_80050120`. Its one self-contained kernel is the HP/MP gauge-fill colour selector keyed on `+0x172`/`+0x174` vs `+0x14E>>1`/`>>2` and the status word `+0x16E`, ported as `battle_gauge::gauge_colors`. Full row in [`functions/battle.md`](../reference/functions/battle.md). |
| `FUN_8004DC68` | Target-highlight pass: OR/clears the actor draw-flag bits `0x83000000` by 2D distance from the acting actor (angle+radius via `FUN_80019B28`), dimming out-of-range targets during command selection; boss/target ids are special-cased. |
| `FUN_8004C650` | Battle name-banner placement: measures a name string width (`FUN_80035F04`) and centres its four banner X coords around `0xA0`, with `0xCF`/`0xC1` leading-byte nudges. |
| `FUN_8004CCD4` | Per-command display resolver (battle-data-pack): for each of the actor's up-to-2 command slots, tests a threshold value against the `+0xA4` range pairs and writes the matching `+0x1034` (hit) or `+0x1030` (fallback) display pointer into the caller's output table. |
| `FUN_80046978` | Screen-flash colour submit: when trigger `gp[0x9D4]` is set, scales stored colour `gp[0x9D0]` by scratch byte `0x1F800393` and submits via `FUN_80024EE4`. The per-channel saturating scale is ported as `scale_rgb24`; the trigger + submit stay caller-side. |
| `FUN_80050120` | Per-actor battle-presentation tick: walks the actor table `DAT_801C9370`, skips actors with no `+0x22C` sub-struct, and dispatches on the actor state byte `+0x21C` (11-entry jump table at `0x8001532C`). Its live arms ease the actor's packed tint/tween word `+0x04` toward a target via `FUN_80050F30`, and treat the packed arrival value `0x20080200` (all three channels at the neutral `0x80` target) as "reached". |
| `FUN_80050F30` | 3×10-bit packed approach-to-target step: eases each 10-bit channel of a packed `u32` toward an 8-bit target (widened `<<2`) by at most `step_scale * DAT_1f800393 * 8` per call, clamping on the target without overshoot; only differing channels are rewritten (the byte-exact masking is why the top two bits survive an unchanged Z channel). A pure closed-form kernel with no table/hardware dependency; **ported** (with tests) as `battle_formulas::packed3_approach_target` / `approach_channel_clamped`. |
| `FUN_80050BB8` | Pairwise battle-actor separation (push-apart): reads two actors' body radii `+0x22C→+0x58` and positions `+0x3C`/`+0x40`, projects the between-actor distance onto the angle from `FUN_80019B28` via the sin/cos LUTs `_DAT_8007B81C`/`DAT_8007B7F8`, and if the projected gap is below `(r1+r2)/6` nudges both actors' **live** position pairs `+0x34`/`+0x38` apart by `sin/cos >> 10` (the `+0x3C`/`+0x40` pair it measures is the seat). Ported as a faithful fixed-point mirror in `engine-vm::battle_separation::push_apart` (trig samples lifted to caller parameters, no Sony table bytes); driven every live battle frame by `World::tick_battle_separation`, on the line after the action-SM step - retail's `FUN_80046A20` call order. |
| `FUN_80051078` | Separation driver: the 7×7 double loop over the actor table that calls `FUN_80050BB8(i, j)` for every ordered pair of living actors (`i != j`, both `+4 != 0`), so every actor is pushed off every other once per pass. Its caller is `FUN_80046A20`, which runs it **every battle frame** immediately after the action SM (`jal 0x801E295C` then `jal 0x80051078`), gated only on "battle live and not tearing down". Not a movement-only pass. |
| `FUN_8005133C` | Per-actor status-marker + display-list primitive spawn: allocates a primitive on the ordered list `_DAT_1F8003A0` (type tag `0x1E1 + slot`, size `0xF0`, priority 1), fills it from `gp[0xA0C] + slot*0x1E0 + 0x894` via `FUN_800583C8`, then sets the four actor status-marker bytes `+0x220..+0x223 = 1` (the lingering-status visual flags near the `+0x21F` marker). Render + status write - documented, not ported. |

The animation pair `FUN_800495C8` / `FUN_80049858` (pose→vertex blend) is
documented in [`monster-animation.md`](../formats/monster-animation.md#vertex-blend-variants-fun_800495c8--fun_80049858).
The tween/separation cluster (`FUN_80050120` and the helpers it drives) is the
battle-overlay actor-**presentation** layer: it moves and tints the on-screen
actor sprites but touches no HP/MP/stat field, so it sits beside - not inside -
the [damage formulas](battle-formulas.md). Only `FUN_80050F30` is a pure kernel;
the rest depend on the actor table, the trig LUTs, or the GPU ordered list.

## Field-to-battle intro presentation

The transition between leaving the field and the battle scene coming up is its
own overlay, PROT 0979 `field_battle_intro`. It does two jobs at once:
sequence the battle handoff, and drive one of five visual styles.

The **handoff** half is live. `FUN_801CF5BC` is ported as
`engine-vm::battle_intro_transition::tick_transition` and driven once per frame
by `World::tick_battle_intro` for as long as the encounter session sits in its
`Transition` phase. Phase 7 is terminal: it raises `ready` bit 1 and stops
advancing, and bit 0 comes from the post-switch spin test, so `ready == 3` is
the completion state.

**Every battle entry rides this phase**, not just the field step roll: a
scripted carrier fight (the op-`0x3E FF` / dialogue-engage path,
`World::begin_field_carrier_battle`) and a world-map contact
(`World::begin_world_map_encounter`) both arm the same session `Transition`
instead of flipping the mode on the spot - retail runs the intro overlay for
all of them. A scene with no session of its own (towns, the overworld) gets a
bare bracket installed on demand (`World::install_encounter_bracket`), a
post-battle `Grace` window never swallows a story fight (it is reset before
arming), and the drain into the actual entry runs in every relevant mode:
`live_field_tick` under the live loop, a live-loop-off arm of the `Field`
tick (`--no-live-loop` gates the roll, never an armed fight), and the
`WorldMap` tick (which drains into `World::enter_world_map_battle`).

The **visual** half is live end to end, on both hosts. The five style kernels
are ported in `engine-vm` and drawn by `engine-ui::battle_intro` (re-exported
at its old `engine-render::battle_intro` path), the per-frame working-set
owner the native play window **and** the browser play page each arm for every
encounter - the hosts differ only in how the captured field frame is read back
(see [`host-drift.md`](../tooling/host-drift.md#screen-space-psx-primitives-across-the-two-hosts)):

| Style | Retail tick | Simulation port | Packet builder port |
|---|---|---|---|
| Scatter particles | `FUN_801CFDA0` | `battle_intro_styles::tick_particle_field` (`PARTICLE_TICK_A`) | `battle_intro::emit_particle_field` |
| Scatter with spin-up | `FUN_801D0370` (+ ring tail `FUN_801D1CFC`) | same, `PARTICLE_TICK_B` | same + `emit_spinup_ring` |
| Tile shatter | `FUN_801D0D24` | `battle_intro_tiles::tick_tile_grid` | `battle_intro::emit_tile` |
| Swirl fan | `FUN_801D1888` / `FUN_801D1A20` | `battle_intro_swirl::tick_swirl` | `battle_intro::emit_swirl_band` |
| Screen-strip curtain | `FUN_801D11D0` | `battle_intro_styles::tick_curtain` | `battle_intro::intro_quad_to_screen` |

The chain: `BattleIntro` holds the style's working set between frames and
synchronises its clock from the live transition entity; the one-shot field
frame capture lands the drawn field in the texture pages each style's packets
name (`Renderer::capture_rgba` → `land_capture_rgba` on the native window,
`gl.readPixels` → `play_intro_land_capture` on the page); and the emitted
`ScreenPrim`s composite over the scene - through
`RenderTarget::SceneWithScreenPrims` natively, through the page's
screen-prim pass in the browser.

### The curtain is a render-to-texture, and only its row pass is on screen

`FUN_801D11D0` draws two passes and it does **not** draw them to the same
place. Between them it links draw-environment packets into the ordering table,
and their OT buckets - a higher index draws first - order them against the
strips:

| OT bucket | packet |
|---|---|
| `0x1F4` | `SetDrawOffset(0, 0)` + `SetDrawArea(320, 0, 320, 240)` |
| `0x1EA` | `FUN_801D1D9C(0x1EA, 2, 0x808080)`, the mid-pass emitter |
| `0x1C2` | the column strips |
| `0x190` | `SetDrawArea(0, y, 320, h)` + `SetDrawOffset(0, y)`, the back buffer |
| `0x12C` | the row strips |

So the column pass runs with the draw area on VRAM `(320, 0)` and its offset at
zero, which makes its primitive coordinates absolute VRAM. `CURTAIN_COL_DRAW_BIAS`
(`0x1E0`) is what makes that fit: a column that passes the visibility test -
which re-centres on `0xA0` - lands at `x` in `320..640`, exactly the installed
area. That area is the rect the row pass' texture pages `0x105` / `0x108`
decode to, so **the row pass samples what the column pass just drew**. The
image is warped horizontally into an intermediate and then sliced vertically
out of it; only the second slice reaches the display.

Two consequences for the port, both now carried. The one-shot field capture
belongs in the *columns* rect only (`capture_rects_for`) - the rows rect is the
intermediate, overwritten every frame - and the column pass has to be
rasterised somewhere, which `engine-render::battle_intro`'s
`compose_curtain_intermediate` does on the CPU because a screen-space quad list
has no render-to-VRAM target. Reading the two rects as "two copies of the same
capture" instead left the curtain stretching in one axis only.

The accumulation the effect rides on is carried, and both of its decays are
pinned from the overlay's own image. Retail never clears. The display side:
`FUN_801D11D0` re-arms the screen wash `FUN_8004695C(0x80808)` unconditionally
at the top of **every** frame (`0x801D1228..0x801D1230`), so a scanline drawn
on one frame decays by 8 per channel behind the ones drawn after it - ~31
frames to black. The intermediate side: the mid-pass emitter `FUN_801D1D9C`
(dumped from the `field_battle_intro` image itself,
`ghidra/scripts/funcs/overlay_field_battle_intro_801d1d9c.txt` - the old
aliased-VA caveat is retired) is `FUN_80024EE4`'s shape pointed one screen
right: a five-word `0x2B` semi-transparent quad over `x 0x140..0x140+W,
y -4..H` (the display halfwords `_DAT_1F80038C` / `_DAT_1F80038E` biased by
`0x140`) behind a `SetDrawMode((abr << 5) | 0xE)` packet at the same layer.
With the curtain's `(0x1EA, 2, 0x808080)` arguments that subtracts `0x80` per
channel from the whole intermediate each frame, between the draw-area install
at `0x1F4` and the column strips at `0x1C2` - a culled column ghosts out over
two frames rather than vanishing.

The port carries both: the intermediate persists across frames and decays by
one mid-pass step instead of being cleared, and a CPU model of the display
buffer - seeded from the same field capture retail's init lands in both
display buffers, decayed one wash step per frame, overdrawn with each frame's
row strips - is uploaded into a spare VRAM rect and drawn as textured backdrop
quads behind the live strips, so the gaps between departing rows show the
fading trail rather than black
(`engine-render::battle_intro::CURTAIN_TRAIL_RECT` + siblings). Two disclosed
approximations: the wash drain (`FUN_80046978`) scales its constant by the
scratchpad brightness byte, taken at full brightness; and retail's display is
double-buffered, so its per-buffer trail may interleave at half this rate -
settling that needs a retail frame capture of one of the three curtain
formations (hypothesis, graded inference).

### The window has no field in it

Retail's transition owns the whole frame - its init writes game mode `9` and
the field renderer does not run again until the completion arm hands over
(details, incl. the capture chain and the per-style fade blend modes, on
[`cutscene.md`](cutscene.md#the-transition-owns-the-whole-frame)). The port has
no such mode: it composites the transition's primitives *over a live scene*,
because that is the only render target that can put a strip over a field.

`battle_intro::backdrop_prim` is what stands in for the absent mode - an opaque
display-rect quad at the farthest OT bucket, emitted on every frame of the
window as `prims[0]`, including the frames a style draws nothing on. Without
it two things went wrong at once, and only the second was obvious: a patch
still at its rest pose drew additively over an identical live copy of itself
and read at double brightness, and once the last particle expired the emitter
returned an empty list - which put the host back on the non-compositing arm and
presented a clean, still-animating field for the rest of the window.

The dry stretch itself is retail's. `FUN_801D0370` decays a moving particle's
colour by `-0x50505` per frame and the tick's top-byte test masks it for good
once that underflows, so the spin-up field expires around a third of the way in
and the fade ramp does not start until `total - 0x18`. Retail spends the gap on
the CD: phase 5 issues the battle-data read and phases 3 and 6 sit in
`FUN_8003DE7C`'s "READ WAIT" poll, and because the completion arm needs
`clock > total` **and** `ready == 3`, the 132 frames are a floor rather than a
length. The port's loads are instant, so the floor is the whole window and the
gap draws as black - the same thing retail draws, for a reason the port does
not have. `every_transition_frame_covers_the_screen` in
`crates/engine-render/src/tests/battle_intro_emitter.rs` pins the invariant.

The session's `Transition` phase length **is** the intro's own
`DAT_801D2458` - 132 display frames, 252 for the swirl
(`battle_intro_styles::intro_duration_frames`,
[`cutscene.md`](cutscene.md#how-long-a-transition-runs-dat_801d2458)) - because
the entity clock counts up to the same number the session counts down from.
Two things depend on it that are easy to read as style bugs when the window is
short: every fade ramp is a lead before it, and the tile shatter's records hold
at their seeded pose until `delay < elapsed * 0x3C` with `delay = rand() % 5000`,
so the grid needs ~84 frames just to finish starting. Per-style packet detail - what each
emitter builds, the dispatcher flag decode, and the two nuances the port
leaves un-carried - is on
[`cutscene.md`](cutscene.md#per-style-emitters-render-track-gtegpu);
`crates/engine-render/src/tests/battle_intro_emitter.rs` pins per-style packet
counts, geometry and OT linkage, and `crates/engine-vm/tests/battle_intro_chain.rs`
the working-set arithmetic.

## The battle open flow - `ctx[+0x06]` from the intro timer to the first swing

The battle command UI is **not one menu**. `FUN_801D0748` walks the flow byte
`ctx[+0x06]` through three separate selection surfaces, each a small cluster of
plate chips around a D-pad glyph, and none of them is a scrolling list.
The whole sequence is readable off the disc: the dispatcher's state chain is a
binary-search `beq` ladder at `0x801D0C84`, and every chip's seat and label
comes from the [screen-element placement table](#the-widget-class-table---where-every-chrome-sprite-comes-from)
plus the two string pools below.

### The state chain

Each row is `ctx[+0x06]`, the arm's entry, and what it does. Addresses are in
PROT entry `0898` at base `0x801CE818`.

| `ctx[+0x06]` | Arm | Behaviour |
|---|---|---|
| `0x00` | `0x801D0DD0` | init (`FUN_801D84C0`), then `0x0A` |
| `0x0A` | `0x801D0DE0` | party plates + formation banner (`FUN_801D9D3C`); intro timer `ctx[+0x6D6] = 0x5A`, or `0x78` when `ctx[+0x290] != 0`; then `0x0B` |
| `0x0B` | `0x801D0E3C` | count the timer down; on expiry `0xFE` if `ctx[+0x290] == 1`, else `0x14` |
| `0x14` | `0x801D0EC4` | one-frame turn setup; sets `ctx[+0x06] = 0x1E` unconditionally |
| `0x1E` | `0x801D102C` | the round-open `Begin` \| `Run` prompt |
| `0x28` | `0x801D1188` | the four-arm command ring |
| `0x32` | `0x801D10F8` | escape |
| `0x78` | `0x801D16E8` | the `Auto` \| `Command` attack-mode prompt |
| `0xFE` | `0x801D31E8` | round armed; hands the frame to the action SM (`ctx[+0x06] = 0xFF`, `ctx[+0x07] = 0`) |

The prompt at `0x1E` is a property of the **round**, not of the turn: `0x14`
is the only way into it, and the action SM's round end (`0x801E67E8`) writes
`ctx[+0x06] = 0x14` and bumps the round counter `ctx[+0x28A]`. So every round
opens with `Begin` / `Run`, and each party member then picks from the ring in
turn.

### Each surface is a D-pad map

There is no face-button map. Every chip is seated on a **D-pad arm** and its
`s2` test is the **packed direction mask** for that arm (packed = byte-swapped
against the raw BIOS word - the trap
[`s2` is not the pad](#s2-is-not-the-pad-and-how-a-command-commits) catalogues;
an earlier revision of this table read the same masks raw and attributed the
arms to Triangle / Square / Circle / Cross). That is why a **D-pad glyph** sits
at the centre of each cluster (`FUN_801DB8F4(x, y)`, the textured-quad emitter,
drawn every frame of all three states). Capture cross-check from the
`cort_evolved_battle_first_menu` state
(`scripts/pcsx-redux/autorun_battle_item_window_capture.lua`): in the ring, a
single Up press opened the item window within three vsyncs while nineteen
Triangle presses changed nothing.

| State | Packed mask | Chip | Next |
|---|---|---|---|
| `0x1E` | Left `0x8000` / confirm mask `0x800846D0` | `Begin` | `0x28` (or `0x6E`) |
| `0x1E` | Right `0x2000` | `Run` | `0x32` |
| `0x28` | Up `0x1000` (`0x801D1364`) | `Item` (up arm) | `0x3C` |
| `0x28` | Left `0x8000` (`0x801D1404`) | `Attack` (left arm) | `0x78` / `0x5A` / `0x50` by option `0x800846C4` |
| `0x28` | Right `0x2000` (`0x801D136C`) | Ra-Seru magic (right arm) | `0x46` |
| `0x28` | Down `0x4000` (`0x801D1544`) | `Spirit` (down arm) | commit; `0x6E` on the last member |
| `0x28` | cancel mask `0x800846D4` | - | back to `0x1E` |
| `0x78` | Left / confirm mask | `Auto` | `0x5A` (target cursor) |
| `0x78` | Right | `Command` | `0x50` (directional arts entry) |
| `0x78` | cancel mask | - | back to `0x28` |

With the selection widget up (`_DAT_800846C8` / `ctx[+0x275]`), the same table
reads as highlight-then-confirm: the pre-dispatch block walks the highlight on
direction presses and rewrites `s2` to the highlighted arm's mask on the
confirm press, so each handler's direction test doubles as "take the
highlighted chip".

`Attack` is therefore **not** the plain strike: it is the door to the
attack-mode prompt, and option `0x800846C4` decides whether that prompt is shown
(`0`), skipped straight to auto-target (`1`), or skipped straight to the
directional entry (`2`).

### Where the words come from

Two pools, and which one a label lives in follows from who writes it into the
placement record's `+0x14` payload pointer. Parser
`legaia_asset::battle_ui_strings`; the coordinates are pinned, never the text.

| Chip | Record | Source |
|---|---|---|
| `Begin` | 0/1/2 | `SCUS_942.54` `0x8007B688`, static on the disc |
| `Run` | 3/4/5 | `SCUS_942.54` `0x8007B684`, static |
| `Item` | 8 | `SCUS_942.54` `0x8007B67C`, static |
| `Attack` | 9 | `SCUS_942.54` `0x8007B674`, static |
| magic | 10 | overlay, written at runtime - see below |
| `Spirit` | 11 | overlay `0x801F4B98`, written by `0x801D8F98` |
| `Auto` | 85 | `SCUS_942.54` `0x8007B658`, static |
| `Command` | 84 | `SCUS_942.54` `0x8007B660`, static |
| `Reselect` | 19/20/21 | `SCUS_942.54` `0x800152D4`, static |

Each disc-static record's seats are the pinned rects the packet walk already
measured: record 1 lives at `(104, 88)` and record 4 at `(180, 88)` with content
width `36`, which is exactly `CLUSTER_TOP_LEVEL`; records 8..=11 sit at
`(204, 34)` / `(160, 66)` / `(248, 66)` / `(204, 98)` with width `48`, which is
`CLUSTER_COMMAND`'s four arms.

**The magic arm is not labelled `Magic`.** `0x801D8F30` reads the acting slot's
character id out of `DAT_8007BD10 + ctx[+0x13]` and indexes a 10-byte-stride run
at `0x801F4B9E`, so the word on the chip is the character's **Ra-Seru**: `Meta`
(Vahn), `Terra` (Noa), `Ozma` (Gala). Index `4` of the same run is a single `-`,
which the `ctx[+0x25F + slot]` gate above it selects for a character with no
Ra-Seru magic - the disc's own instance of the "an unavailable command keeps its
plate and draws a dash" law.

### The formation banner

`FUN_801D9D3C`'s arm at `0x801DA234` reads `ctx[+0x290]` and picks the line it
stores into placement record 67 before the intro timer runs:

| `ctx[+0x290]` | Line | Consequence |
|---|---|---|
| `0` | none - the draw at `0x801DA2E4` is skipped | ordinary round |
| `1` back attack | `0x801F4D10` | `0x0B` jumps to `0xFE`: the party enters **no** command that round |
| `2` pre-emptive | `0x801F4CD8` / `0x801F4CF8` | ordinary round; the monsters sit it out |

The singular / plural pick at `0x801DA274` tests the byte at `DAT_8007BD10 + 1`
(present-party slot 1), so a party with nobody there gets the shorter line. The
name is substituted into the `0xC1` token by `FUN_8003CBF8`, whose operand is
`DAT_8007BD10[0] - 1` - the party **leader**, not the acting member.

### Port

`engine-core::battle_input` carries the three phases (`CommandPhase::RoundPrompt`
/ `Menu` / `AttackMode`) and `engine-ui::battle_command_ui::ChipPhase` the
seating for each; `engine-core::battle_open` composes the banner and
`World::raise_battle_open_banner` queues it onto the shared battle message box
(retail's `ctx[+0x6B2]` surface). The round-scoped prompt is armed from
`World::arm_round_open_prompt`, keyed on the flow byte parking at
`BattleFlowState::TurnPrompt` - which the round boundary and battle entry both
set, and which a mid-round reopen does not. The ambush's lost round is already
the `ctx[+0x290]` side lockout in `World::reseed_initiative`.

The port follows retail's **direct-commit press**: a direction press takes the
chip drawn on that side of the screen in the same frame, no confirm. The map is
spatial, mirroring retail's own per-arm dispatch: on the ring Up commits
`Item`, Left `Attack`, Right the magic arm, Down `Spirit`
(`battle_input::ring_seat`), and on both two-chip prompts Left is always the
left chip and Right the right chip (`battle_input::pair_seat`). Cross
additionally commits whatever the cursor rests on - the route a scripted
harness that cannot aim a direction drives - and Circle keeps its back-out /
outright-`Run` roles. `engine-shell`'s
`direction_presses_land_on_the_chip_drawn_on_that_side` holds the map equal to
the drawn seating.

## The battle item window (`0x3C`) - packet-pinned

What state `0x3C` actually puts on screen, read out of its own display list:
the `battle_item_window` / `battle_item_window_cursor1` captures
(`scripts/pcsx-redux/autorun_battle_item_window_capture.lua`, pad-walked from
`cort_evolved_battle_first_menu`) hold the window open and one Down press
apart, and the OT walk gives:

| Piece | Pin |
|---|---|
| item-list window | system-UI window-skin tile grid (widget page `(896, 256)`, CLUT row 511 sub-palette 2), spanning x `166..=313`, y `28..=164` |
| description window | same skin, x `8..=167`, y `122..=164`; shows the highlighted item's info-window line |
| hand cursor | 16x16 pointing-finger `POLY_FT4` (CLUT row 511 sub-palette 7) at `(167, 45 + 14*row)` - the two captures pin the row pitch at 14 |
| rows | eight per page; `PAGE n/m` header top-right, counts right-aligned at the interior's right edge |
| breadcrumbs | gold tab plates `Begin` \| acting member's name \| `Item` top-left, replacing the actor-name plaque while the window is up |

Content pens (row text, header, description line, breadcrumb seats) are
screenshot-read off the same captures - the glyph packets ride a different
draw pass than the window tiles.

State `0x64` (the item window's own target confirm) is packet-pinned the same
way (`battle_item_target` / `battle_item_target_cursor1` captures,
`scripts/pcsx-redux/autorun_battle_item_target_capture.lua`, one RIGHT press
apart): the item windows **close**, the third breadcrumb becomes the selected
item's name (`Begin | Vahn | Healing Leaf`), and the surface is a single
full-width **target strip** at the screen's foot - window skin caps at x `8`
and `304`, one 20-px row at y `188`; target name glyphs from `(16, 192)`; the
gold `HP` label widget (`#0x07`) at `(80, 194)` with current-HP numerals
ending at x `134` and max-HP from `146`; the `MP` widget (`#0x08`) at
`(192, 194)` with numerals at `214..238` / `250..`. The regular 3-member HUD
parks offscreen (its digit rows sit at y `234..264` in the capture), a name
tag floats beside the targeted actor, and the camera re-frames on the target;
RIGHT steps the target across the party band and the whole strip follows.

**Port.** `engine-ui::battle_item_ui` carries the pins and composes the
windows through the shared 9-slice menu-window chrome + tab-banner 3-slice +
save-select hand cell; the projection (dedup row list with the cursor mapped
into it, disc description, breadcrumb name, same-side target rows) is
`engine-core::World::battle_item_menu_model` /
`InventoryUseSession::menu_view`, consumed by both play hosts. Target select
draws the pinned strip for the pointed-at member. Known divergences, disclosed
in the module doc: breadcrumb tabs are sized per label (the engine font is
wider than retail's tab glyphs), the HP/MP labels are text stand-ins for the
gold HUD label widgets, and the floating world-anchored name tag + the
target-camera re-frame are not drawn.

## See also

**Reference** -
[Battle action SM](battle-action.md) ·
[Damage / accuracy formulas](battle-formulas.md) ·
[Encounter record](../formats/encounter.md) ·
[Player battle files](../formats/battle-data-pack.md)
