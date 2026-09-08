# VM inventory

The complete set of VM-shaped subsystems in the runtime, with what is decoded,
what is ported, and what a live caller actually reaches.

A subsystem qualifies as "VM-shaped" here on one of two structural tests: it
walks a bytecode or record stream through a dispatcher, or it advances a
per-entity state byte through a `switch` each frame. Both shapes get the same
columns below, because both raise the same question - is the whole space
decoded, and does anything call the port.

**What catches people out: "five VMs" is an orientation, not a census.** The
[runtime VM family](move-vm.md#the-runtime-vm-family) table names the five
*bytecode-and-state* drivers that share the actor model, and it is the right
mental model for that layer. It is not the inventory. Several drivers below -
the `0x2F` extension dispatcher, the `0x4C` sub-dispatcher, the battle-action
and world-map and tile-board state machines - are separate dispatchers with
their own tables, and they are missing from any count that stops at five.

**And "ported" is not "live".** Port status and reachability are independent
facts, tracked in separate columns for a reason: several faithful ports have no
non-test caller. See [Ported but inert](#ported-but-inert).

## The inventory

Op/state spaces are structural invariants read off the dispatcher bound in the
disassembly (`sltiu` immediate before the `jr`), not off the port.

| Subsystem | Driver | Op / state space | RE status | Ported | Live caller |
|---|---|---|---|---|---|
| [Actor / sprite VM](actor-vm.md) | `FUN_801D6628` | 13 opcodes, JT `0x801CED70` | resolved | yes - `legaia_engine_vm` root | yes - shop widget choreography (`engine-core::menu_widget`) |
| [Move VM](move-vm.md) | `FUN_80023070` | 71 opcodes `0x00..0x46`, JT `0x80010778` | resolved | yes - `move_vm` | yes |
| [Move-VM `0x2F` extension](move-vm-overlay-ext.md) | `FUN_801D362C` | 61 sub-opcodes `0x00..0x3C`, JT `0x801CE868` | resolved | yes - `move_vm::ext` (live) + `move_vm_overlay_ext` (replaced) | **live** |
| [Motion VM - pursue / patrol](motion-vm.md) | `FUN_8003774C` | 22-slot JT `0x80010EE0`, index `(op & 0x7F) - 0x37` | resolved | yes - `motion_vm` | yes |
| [Motion VM - scripted](motion-vm.md#the-second-motion-vm---fun_80038158) | `FUN_80038158` | 32-slot JT `0x80010FE8`, ops `0x01..=0x20` | partial | split - see [below](#the-scripted-motion-vm-is-ported-in-three-pieces) | yes |
| [Field / event VM](script-vm.md) | `FUN_801DE840` | 43 opcodes `0x21..0x4F` with gaps | resolved | yes - `field` | yes |
| [Field VM `0x4C` MENU_CTRL](script-vm-menuctrl.md) | inline in `FUN_801DE840` | 16 outer nibbles, nibble `B` undefined in retail | resolved | yes - `field::step::menu_ctrl` | yes |
| [Effect VM](effect-vm.md) | `FUN_801E0088` | **none** - see [No opcode space](#the-effect-vm-has-no-opcode-space) | resolved | yes - `effect_vm` | yes |
| [Battle-action SM](battle-action.md) | `FUN_801E295C` | 256-slot JT `0x801CED44`, sparse handled bands, no default arm | partial | yes - `battle_action` | yes |
| [World-map entity SM](world-map.md) | `FUN_801DA51C` | 5 states | resolved | yes - `world_map` | yes |
| [Tile-board walk SM](tile-board.md) | `overlay_0897_801EF2B0` | 15 states, JT at `0x801CF65C` | resolved | yes - `legaia_engine_core::tile_board` | yes |
| Per-actor anim dispatch | `FUN_80021DF4` | 7 dispatch bytes `0x01..=0x07` at `actor[+0x5A]` | resolved | yes - `anim_vm` / `actor_tick` | yes |
| Ambient facing channel | `FUN_80038158` ops `0x04` / `0x0D` | 2 of the 32-slot table | resolved | yes - `ambient_motion` | yes |
| [Title-screen tick](#one-function-two-ports) | `FUN_801DD35C` | 25-slot JT `0x801CF244`, sub-mode word `+0x204` | resolved | yes - `title_overlay` | menu law only - see [below](#one-function-two-ports) |
| Per-prim render dispatch | `FUN_80043390` | 20 kind slots × 4 alpha banks | resolved | yes - `prim_dispatch` | yes |
| Status-effect ticker | `FUN_801E752C` | per-actor condition set | resolved | yes - `status_effects` | yes |

## The effect VM has no opcode space

A low opcode count is not evidence of incomplete RE, and here the count is
zero by construction. `FUN_801E0088` has no central switch on a per-slot
opcode byte at all: the bytes that look like state tokens are 5.3 fixed-point
**wait counters**, and the walker is a pair of countdown-driven cursor walks.
Searching for its opcode table is the documented dead end.

The thread that tracked this was originally framed as decoding an opcode space,
so it read as open for as long as the opcode space failed to appear. It is
recorded [resolved + ported](../reference/open-rev-eng-threads.md), and the
port runs on the live path - `World::tick_effects` sweeps `Pool::tick_retail`
once per retail frame from the per-frame tick.

## The scripted motion VM is ported in three pieces

`FUN_80038158` is the one entry below whose port does not sit behind a single
module, which is why its status reads differently depending on where a reader
enters. Its static decode - which stream binds to which placement, wander pace,
default-move harvest - is `legaia_engine_core::man_field_scripts::npc_motion`,
because the bytecode arrives as MAN tail-section 1 rather than through the
actor tick's own buffer. Its runtime facing channel is
`legaia_engine_vm::ambient_motion`. The rest of the 32-slot table is decoded
but has no port.

## One function, two ports

`FUN_801DD35C` is a single 3026-instruction dispatcher, and the disassembly is
**identical** across the `overlay_menu`, `overlay_title`, `overlay_save_ui_*`
and `overlay_shop_save` dumps - the same one-resident-function-under-many-
scenario-labels shape that settled the [`0x2F` residency
question](move-vm-overlay-ext.md#overlay-residency---one-copy-in-the-field-overlay-only).

Which overlay *owns* it is settled by a byte search: its 48-byte prologue
occurs exactly once on the disc, inside PROT **0899** at file `+0xEB44`
(`0x801CE818 + 0xEB44` reproduces the VA), and it is absent from
`SCUS_942.54`. The many-labelled dumps are one resident copy under scenario
labels; the short `overlay_801dd35c.txt` that reads differently is a 436-byte
PROT 0897 routine `FUN_801DD310` at an aliased VA.

**What it does is the title-screen tick**, and `crates/engine-vm` used to
describe it two ways - `title_overlay.rs` as the title tick, `menu.rs` as the
menu overlay's top-level dispatcher. Hosting it in the menu overlay's image is
what made the second reading look right; the routine's own operands falsify it:

- Its 56 `sw ..,0x204(..)` sub-mode writes store only `0x02..=0x18`, inside
  the jump table's `sltiu v0,s2,0x19` bound at `0x801DD7F8`. The pause-menu,
  shop and inn screen bytes `menu.rs` enumerates are never written by it, and
  the `0x70` literals in its body are the `y` argument of the centred-text
  helper `FUN_801E1C1C`.
- Its two master-mode stores into `0x8007B83C` are `0x1A` at `0x801DDCF0`
  (attract -> STR) and `2` at `0x801DFC00` (NEW GAME -> field), and its only
  caller is `FUN_801E36A0` (0899 `+0x14E88`), nine instructions that are
  `jal 0x801dd35c` with both arguments zeroed, spawned by master mode 22.

So `title_overlay.rs` carries the `PORT:` and `menu.rs` a `REF:` plus the
correction.

### What of the tick runs on both hosts

`TitleMenuState::step` is the executable half - the `AttractIdle` (`0x10`)
block at `0x801DDB74..0x801DDCF4`, which is where every player-visible law of
the title menu lives:

| Law | Retail | Where it is now |
|---|---|---|
| Cursor step | `Down 0x4000` `+1` / `Up 0x1000` `-1`, cue `0x21` (`0x801DDB9C..0x801DDBE0`) | `TitleMenuState::step` |
| Row space | `andi v1,v1,0x1` at `0x801DDC00` - two rows | `TITLE_MENU_ROWS` |
| Confirm | `pad & 0x844` (Start / L1 / Cross), cue `0x20` (`0x801DDC04`) | `TitleMenuEvent::Confirmed` |
| Input freeze | whole block skipped while countdown `< 0x11` (`0x801DDB84`) | `ATTRACT_INPUT_FREEZE_BELOW` |
| Attract countdown | `0x5DC`, re-armed on any held pad, `-= frame scalar` (`0x801DDC74..0x801DDCC8`) | `TitleMenuState::countdown` |

`engine-core::title::TitleSession` owns one and steps it every frame, so the
native window and the browser play page share it without either host
changing. One thing around it is still the port's own and says so in the
module docs: the `continue_enabled` row skip, which retail does not have.

The whole dispatcher is ported alongside it as
`title_overlay::TitleTickState::step` - one arm per sub-mode plus the shared
epilogue, over the state fields the handlers read. Its graph is the
`STATE_204_WRITES` table, all 56 `state[+0x204]` stores with the handler and
guard each belongs to, and `cold_boot_reachable_modes` walks it.

The attract fire arm is wired on **both** hosts behind
`TitleSession::attract_enabled`, which each host sets for itself because a
host with no movie destination would freeze input for the last sixteen frames
of every idle period and then do nothing. The native window plays retail's
`fmv_id 0` through the same MDEC path the field-VM FMV trigger uses; the
browser play page enters the same `TitlePhase::Attract`, discloses that it has
no STR/MDEC playback, and returns to the menu.

The mode-graph question of which sub-mode a cold boot shows is settled: `0x10`
always. See [`boot.md`](boot.md#a-cold-boot-always-shows-sub-mode-0x10-never-0x02).

`menu.rs` remains the engine's own pause / shop / inn screen graph
- state bytes engine-chosen, per-screen behaviour sourced from
[`shop.md`](shop.md), [`inn.md`](inn.md) and
[`field-menu.md`](field-menu.md) - it is simply not a port of this routine.

## Ported but inert

These ports are faithful and tested, and nothing outside `crates/engine-vm`
calls them. Inert is a reachability statement, not a correctness one.

- **Actor / sprite VM** (`legaia_engine_vm::run`) - **no longer inert.** The
  missing prerequisite was a bytecode source, and it is resolved: the
  programs are data resident in the menu overlay
  ([`window-script.md`](../formats/window-script.md), parser
  `legaia_asset::widget_script`), and `MenuRuntime::tick` runs the shop
  open / Sell slide-away programs through the interpreter over
  `engine-core::menu_widget::MenuWidgetState` on the same transitions
  retail's `FUN_801DAFD4` drives. The `World::run_actor_bytecode` /
  `FieldDemoHandler` edge remains the demo-only field-actor host, still
  constructed nowhere outside a `#[cfg(test)]` module. History + triage:
  [`reach-triage.md`](../tooling/reach-triage.md#the-actor-vm-a-resolved-bytecode-source).
- **Move-VM `0x2F` extension** - **no longer inert, and the "inert" framing
  was measuring the wrong surface.** There are two Rust surfaces over
  `FUN_801D362C`. The one an executing move program reaches is
  `move_vm::ext::ext_default_dispatch`, the default body of
  `MoveHost::ext_dispatch`, which `move_vm::dispatch`'s `0x2F` arm calls and
  which `engine-core::world::vm_hosts` inherits - so every actor the world
  ticks runs its `0x2F` instructions through it. That surface had no `PORT:`
  tag, which is why the address read as inert; it has one now.

  The other surface is `move_vm_overlay_ext`'s standalone `step` / `walk`
  walker, and **no host is owed it**: a disc-wide five-form reference scan
  for `0x801D362C` finds exactly one caller, the SCUS move-VM arm at
  `0x80023AE0`, and the port already hosts that caller live. A second
  interpreter for one opcode is not one more reachable behaviour. Its
  `canonical_size` width table stays live on its own account - it is the
  disassembly-sourced mirror `move_vm::ext` is tested against, and
  `engine-core`'s VDF-pulse scanner reads it to skip `0x2F` instructions.

  Note for anyone reading `--live-audit`: `step` and `walk` show as *live*
  there, and they are not. Both names collide with live free functions in
  the same crate (`motion_vm::step` among them), which is why neither
  carries a `NOT WIRED:` tag - tagging them would put two name-collision
  rows into the stale-tag triage list rather than disclose anything.
- **`title_overlay`** - **no longer wholly inert.** Its menu half
  ([`TitleMenuState`](#what-of-the-tick-runs-on-both-hosts)) runs on both
  hosts. What stays disclosed is the 25-mode dispatcher itself: the sub-mode
  table and the state-struct offsets are a decoded description with no
  interpreter behind them.
- **`title_prim`**, **`vram_rect_copy`**, **`cutscene_trigger`** - supporting
  primitive and catalogue modules on the same footing.

## How many copies of `FUN_801D362C` exist

One, in field overlay `0897`. The reading that each overlay carries its own
flavour with its own 61-entry jump table is falsified: all seven dumps - the
six capture-derived ones and the `0897` static one - carry the same 1293
instructions with a byte-identical disassembly section. There is no subset
relation and no address that appears in one but not another; the whole-file
line-count spread is header and decompiled-section noise. That identity is what
the "byte-identical" shorthand in the open-threads register
compresses, and it is worth stating precisely: a reader diffing the dump sizes
will otherwise think the shorthand is broken.

The consequence is a real constraint on the engine, not just a documentation
detail - op `0x2F` is executable only while `0897` is resident, so battle-side
move records cannot reach the extension dispatcher at all.
