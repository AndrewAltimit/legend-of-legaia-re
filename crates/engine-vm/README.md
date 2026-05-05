# legaia-engine-vm

Clean-room Rust ports of Legaia's runtime VMs.

Three VMs are bundled as separate modules. Each is written from the
decompiled source in `ghidra/scripts/funcs/<addr>.txt` plus the format
notes in `docs/subsystems/`, with no static-recompiled bytes from the
original executable.

## `actor_vm` — `FUN_801D6628`

Sprite / actor script VM. The first script VM identified in retail
Legaia. Lives in the title-screen / field overlay loaded into the
`0x801C0000+` window at runtime. Small (612 bytes, 13 opcodes) and
well-bounded — the smallest target we have for a runtime-faithful port.

### Bytecode layout (4 bytes per instruction)

```text
byte 0:    opcode
byte 1:    operand_b — typically an actor id
bytes 2-3: operand_w — little-endian u16, typically packed (x, y)
```

Execution stops on opcode `0x00`. Opcodes outside `1..=0xD` are no-ops.

### Opcodes

| op | name | semantics |
|----|------|-----------|
| `0x00` | `End` | Terminate the program. |
| `0x01` | `SpawnDefault` | Ensure actor exists, snap to default position, conditional clear of `field20`. |
| `0x02` | `SpawnAt` | Ensure actor exists, snap to packed `operand_w`. |
| `0x03` | `SetField1d` | Write low byte of `operand_w` to actor `field1d`. |
| `0x04` | `DeleteSprite` | Delete the sprite for `operand_b`. |
| `0x05` | `GlobalUpdate` | Tick the global sprite system. |
| `0x06` | `ClearField20` | Clear actor `field20` if actor exists. |
| `0x07`–`0x0D` | `Nop` / reserved | Fall through to default. |
| `0x08` | `Effect` | Trigger actor effect. |
| `0x09` | `MotionAt` | Motion to packed `operand_w`. |
| `0x0A` | `EffectMotion` | Capture target, trigger effect, respawn, motion. |

### Packed-position encoding

```text
x = (operand_w >> 7) & 0x1FE
y =  operand_w       & 0xFF
```

## `field_vm` — `FUN_801DE840` (the field/event script VM)

Per-scene event script VM (long-sought "Epic 4.3"). Switch dispatch at
`0x801E00F4`; ~17.5 KB, the largest function in the corpus. All 43
opcodes ported. Default-route opcodes (`0x5x` / `0x6x` / `0x7x`) are
SET / CLEAR / TEST against a 256-bit bitfield at `DAT_80086D70` and
exposed via `FieldHost::system_flag_{set,clear,test}`. Distinct from
the actor VM above.

## `effect_vm` — `FUN_801DE914` / `FUN_801DFDF8` / `FUN_801E0088`

Effect VM with a 32-master + 128-child slot pool.
`Pool::init` / `Pool::spawn` / `Pool::tick` are the three API entries;
`EffectHost::advance_state` is the extension hook for per-effect state
machines that aren't pure data-driven.

## `move_vm` — `FUN_80023070`

71-opcode move-table VM (jump table at `0x80010778`); `actor_tick` and
`decrement_wait_timer` mirror the `FUN_80021DF4` + `FUN_80022B94` gate
(skip when wait_timer ≥ 0, run VM, check HALT flag). Op `0x2F` escapes
into the overlay-resident `FUN_801D362C` extension VM, not yet ported.

## See also

- [`docs/subsystems/script-vm.md`](../../docs/subsystems/script-vm.md)
- [`docs/subsystems/actor-vm.md`](../../docs/subsystems/actor-vm.md)
- [`docs/subsystems/effect-vm.md`](../../docs/subsystems/effect-vm.md)
- [`docs/subsystems/move-vm.md`](../../docs/subsystems/move-vm.md)
