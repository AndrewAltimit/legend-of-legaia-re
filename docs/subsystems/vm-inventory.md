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
| [Actor / sprite VM](actor-vm.md) | `FUN_801D6628` | 13 opcodes, JT `0x801CED70` | resolved | yes - `legaia_engine_vm` root | **inert** |
| [Move VM](move-vm.md) | `FUN_80023070` | 71 opcodes `0x00..0x46`, JT `0x80010778` | resolved | yes - `move_vm` | yes |
| [Move-VM `0x2F` extension](move-vm-overlay-ext.md) | `FUN_801D362C` | 61 sub-opcodes `0x00..0x3C`, JT `0x801CE868` | resolved | yes - `world_map_draw_vm` | **inert** |
| [Motion VM - pursue / patrol](motion-vm.md) | `FUN_8003774C` | 22-slot JT `0x80010EE0`, index `(op & 0x7F) - 0x37` | resolved | yes - `motion_vm` | yes |
| [Motion VM - scripted](motion-vm.md#the-second-motion-vm---fun_80038158) | `FUN_80038158` | 32-slot JT `0x80010FE8`, ops `0x01..=0x20` | partial | split - see [below](#the-scripted-motion-vm-is-ported-in-three-pieces) | yes |
| [Field / event VM](script-vm.md) | `FUN_801DE840` | 43 opcodes `0x21..0x4F` with gaps | resolved | yes - `field` | yes |
| [Field VM `0x4C` MENU_CTRL](script-vm-menuctrl.md) | inline in `FUN_801DE840` | 16 outer nibbles, nibble `B` undefined in retail | resolved | yes - `field::step::menu_ctrl` | yes |
| [Effect VM](effect-vm.md) | `FUN_801E0088` | **none** - see [No opcode space](#the-effect-vm-has-no-opcode-space) | resolved | yes - `effect_vm` | yes |
| [Battle-action SM](battle-action.md) | `FUN_801E295C` | 256-slot JT `0x801CED44`, sparse handled bands, no default arm | partial | yes - `battle_action` | yes |
| [World-map entity SM](world-map.md) | `FUN_801DA51C` | 5 states | resolved | yes - `world_map` | yes |
| [Tile-board walk SM](tile-board.md) | `overlay_0897_801EF2B0` | 15 states, JT at `0x801EF308` | resolved | yes - `legaia_engine_core::tile_board` | yes |
| Per-actor anim dispatch | `FUN_80021DF4` | 7 dispatch bytes `0x01..=0x07` at `actor[+0x5A]` | resolved | yes - `anim_vm` / `actor_tick` | yes |
| Ambient facing channel | `FUN_80038158` ops `0x04` / `0x0D` | 2 of the 32-slot table | resolved | yes - `ambient_motion` | yes |
| Sub-mode dispatcher | `FUN_801DD35C` | 25-slot JT `0x801CF244` | contested - see [below](#one-function-two-ports) | **twice** - `menu` and `title_overlay` | `menu` yes, `title_overlay` **inert** |
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

`crates/engine-vm` ports it twice, under two incompatible descriptions:
`menu.rs` calls it the menu overlay's top-level dispatcher, `title_overlay.rs`
calls it the title-overlay per-frame tick. Both cite dumps that resolve to the
same entry. The outer dispatch both must be describing is the 25-slot jump
table at `0x801CF244`, guarded by the `sltiu` bound at `0x801DD7F8`; the nested
switches deeper in the body are what the two descriptions disagree about.

Which overlay *owns* the function is the open part. The residency evidence
points at one shared slot-A overlay generation rather than separate title and
menu copies - the [actor VM](actor-vm.md) driver shows the same identical-
across-labels pattern in the same dump set - but that is an inference from the
dumps, not a capture. Settling it needs the residency check the `0x2F` thread
used: read the fixed VA out of each candidate overlay's disc image.

## Ported but inert

These ports are faithful and tested, and nothing outside `crates/engine-vm`
calls them. Inert is a reachability statement, not a correctness one.

- **Actor / sprite VM** (`legaia_engine_vm::run`) - the first VM ported, and
  the `Host`-trait shape every later VM port follows. Only its `Position` type
  is imported elsewhere; the interpreter itself has no caller.
- **Move-VM `0x2F` extension** (`world_map_draw_vm`) - the engine's move VM
  never dispatches op `0x2F`.
- **`title_overlay`** - superseded in practice by `menu`, which ports the same
  function and does have callers.
- **`title_prim`**, **`vram_rect_copy`**, **`cutscene_trigger`** - supporting
  primitive and catalogue modules on the same footing.

## Where the status claims drifted

Two corrections that a reader arriving from the code will hit first:

- The `world_map_draw_vm` module header still carries the **falsified**
  per-overlay-copies reading of `FUN_801D362C` - "the same function exists in
  many overlays … each overlay supplies its own contents in the 61-entry JT" -
  and claims to port the `overlay_world_map` flavour specifically. There is one
  copy, in field overlay `0897`. The six capture-derived dumps are identical to
  each other and the `0897` static dump is a strict subset of them (coverage
  gaps where Ghidra could not follow the JT flow), which is what the
  "byte-identical" shorthand in the open-threads register is compressing.
- The module name `world_map_draw_vm` is itself misleading: the file ports the
  move-VM extension dispatcher, which is neither world-map-specific nor a draw
  VM.
