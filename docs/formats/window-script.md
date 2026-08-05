# Window Widget Scripts

The bytecode programs the menu overlay's window-script VM (`FUN_801D6628`,
[docs/subsystems/actor-vm.md](../subsystems/actor-vm.md)) interprets: small
fixed-width command lists that open, close and slide the pause-menu / shop /
save-UI windows. They are **data resident in the menu overlay itself**
(PROT entry 899, slot-A base `0x801CE818`) - a program table in the
overlay's data segment, not a scene MAN section, effect bundle, or actor
spawn record.

Parser + `jal`-site scanner: `legaia_asset::widget_script`. Disc pins in
`crates/asset/tests/widget_script_real.rs`. Interpreter port:
`legaia_engine_vm::run`; engine host + trigger wiring:
`legaia_engine_core::menu_widget` (see
[actor-vm.md](../subsystems/actor-vm.md#where-the-programs-live)).

Confidence: **Confirmed** (disassembly of the interpreter and callers,
byte-verified program contents on the disc image).

## Instruction encoding

Each instruction is exactly 4 bytes; a zero opcode byte terminates the
program (the VM re-reads byte 0 of the next slot after every dispatch,
`lbu v0,0x0(s4)` at `0x801D6854`):

| Offset | Type | Field |
|---|---|---|
| `+0` | u8 | opcode - `0x01..=0x0D` dispatch via the jump table at `0x801CED70` (`sltiu v0, opcode-1, 0xd` at `0x801D6680`); `0x00` terminates |
| `+1` | u8 | window id - index into the 52-record window descriptor table at `0x801E4738` ([`menu-windows`](../subsystems/field-menu.md); the VM computes `0x801E4738 + id*0x10` and reads the record's `x`/`y` as the instruction's default coordinates) |
| `+2` | u16 LE | operand - packed position for opcodes `0x02`/`0x09` (`x = (w >> 7) & 0x1FE`, `y = w & 0xFF`), style byte for `0x03`, zero elsewhere on disc |

Opcode semantics (create / snap / slide / close / global tick) are the
interpreter's, documented with its port in `crates/engine-vm/src/lib.rs` and
[actor-vm.md](../subsystems/actor-vm.md). Opcodes observed in disc programs:
`0x01`, `0x02`, `0x04`, `0x05`, `0x06`, `0x0A`.

## Where the programs live

The menu overlay's callers materialise a program pointer with a
`lui`/`addiu` pair (or forward one through a saved register) and call
`FUN_801D6628(&program)`. The referenced programs cluster in one data
region of the image (file `0x16260..0x16740`, VA `0x801E4A78..0x801E4F58`).
`legaia_asset::widget_script::scan` recovers them structurally: decode every
`jal 0x801D6628` word in the image, resolve each site's `a0`
materialisation, and keep the targets that parse as terminated programs
with in-range opcodes and window ids. Register-forwarded call sites (the
shop pair among them) are not resolvable by that pass; those programs are
pinned by caller disassembly instead.

Because the program table is overlay data at fixed VAs, the resolver is a
**per-boot lookup, not a per-scene one**: the same programs are resident
whenever the menu overlay is (pause menu, shop, save UI - every scene).

## Pinned programs

Byte-verified on the disc image; the shop pair is additionally pinned by
the randomizer's seru-trading vendor, which reuses exactly these scripts
(`legaia_patcher::seru_overlay::consts`, [shop.md](../subsystems/shop.md)):

| VA | File offset | Program | Caller |
|---|---|---|---|
| `0x801E4E38` | `0x16620` | `[05][01 21][01 2A][01 20][01 28][01 22][00]` - open vendor plate, picker, gold, `0x28`, `0x22` | shop picker open, `FUN_801DAFD4` |
| `0x801E4E54` | `0x1663C` | `[04 28][04 2A][04 22][00]` - close the picker windows, keep gold + vendor plate | shop Sell transition, `FUN_801DAFD4` |
| `0x801E4A78` | `0x16260` | `[05][00]` - global tick only | menu-open staging (multiple callers) |
| `0x801E4D50` / `0x801E4D78` | `0x16538` / `0x16560` | `[01 07][00]` - open window 7 | spell level-up notice, `FUN_801D9280` / `FUN_801D9594` |
| `0x801E4EA8` / `0x801E4EDC` | `0x16690` / `0x166C4` | `[01 1F][00]` - open window 31 | Point Card toast, `FUN_801DB7F4` / `FUN_801DB380` |

## See also

- [actor-vm.md](../subsystems/actor-vm.md) - the interpreter and its
  engine wiring.
- [field-menu.md](../subsystems/field-menu.md) - the window descriptor
  table the window ids index.
- [shop.md](../subsystems/shop.md) - the shop choreography these programs
  drive.
