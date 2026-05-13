# legaia-engine-vm

Clean-room Rust ports of Legaia's runtime VMs.

Three VMs are bundled as separate modules. Each is written from the
decompiled source in `ghidra/scripts/funcs/<addr>.txt` plus the format
notes in `docs/subsystems/`, with no static-recompiled bytes from the
original executable.

## `actor_vm` - `FUN_801D6628`

Sprite / actor script VM. The first script VM identified in retail
Legaia. Lives in the title-screen / field overlay loaded into the
`0x801C0000+` window at runtime. Small (612 bytes, 13 opcodes) and
well-bounded - the smallest target we have for a runtime-faithful port.

### Bytecode layout (4 bytes per instruction)

```text
byte 0:    opcode
byte 1:    operand_b - typically an actor id
bytes 2-3: operand_w - little-endian u16, typically packed (x, y)
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

## `field_vm` - `FUN_801DE840` (the field/event script VM)

Per-scene event script VM (traced from `FUN_801DE840`). Switch dispatch at
`0x801E00F4`; ~17.5 KB, the largest function in the corpus. All 43
opcodes ported. Default-route opcodes (`0x5x` / `0x6x` / `0x7x`) are
SET / CLEAR / TEST against a 256-bit bitfield at `DAT_80086D70` and
exposed via `FieldHost::system_flag_{set,clear,test}`. Distinct from
the actor VM above.

## `effect_vm` - `FUN_801DE914` / `FUN_801DFDF8` / `FUN_801E0088`

Effect VM with a 32-master + 128-child slot pool.
`Pool::init` / `Pool::spawn` / `Pool::tick` are the three API entries;
`EffectHost::advance_state` is the extension hook for per-effect state
machines that aren't pure data-driven.

## `move_vm` - `FUN_80023070`

71-opcode move-table VM (jump table at `0x80010778`); `actor_tick` and
`decrement_wait_timer` mirror the `FUN_80021DF4` + `FUN_80022B94` gate
(skip when wait_timer ≥ 0, run VM, check HALT flag). Op `0x2F` escapes
into the overlay-resident `FUN_801D362C` extension VM (61 sub-opcodes);
the dispatch table is ported in `world_map_draw_vm.rs`.

## `actor_tick` - `FUN_80021DF4`

Per-actor physics tick - the `FUN_8002519C`-driven per-frame loop calls
this on every active actor. The dispatch byte at `actor[+0x5A]` selects
which subset of side-effects fires:

| Stage | Runs for | Behaviour |
|---|---|---|
| Common pre-update | every byte | Drain timer at `+0x54`, advance rotation accumulator at `+0x22`. |
| Keyframe accel | `0x02` / `0x06` | Fold `+0xC0..+0xCA` into shake envelopes at `+0xB4..+0xC8`. |
| Positional SFX emitter | `0x05` | Distance-based pan / volume engine; ramp interpolation between target / source pairs over `+0xBC` frames; `key-on` / `vol-update` / `release` SsAPI calls surface as `TickEvent::Sfx*`. |
| Path interpolation | `0x03` | Three-axis velocity into `+0x90..+0x94`, zoom envelope advance, path state machine at `+0x9C`. |
| Default movement | every byte except `0x05` | Velocity / accel into `motion_x..motion_z`, trig-LUT-driven world rotation, shake / focal envelopes. |
| Common late-update | every byte | Cap envelopes, optional move-VM kick, render submissions for `0x04` / `0x07`, keyframe pose write for `0x06`. |

`ActorPhysics` mirrors the retail actor record's tick-relevant fields
(`+0x10` through `+0xD0`, with offset annotations on every field).
Cross-cutting effects surface as `TickEvent` entries; engines drain
them into their own audio mixer / scene graph / move-VM driver.

## `status_effects`

Per-actor status-effect tracker. `StatusKind` covers the eight retail
condition kinds (Burned / Shocked / Poisoned / Asleep / Confused /
Silenced / Stunned / Petrified). The tracker maintains per-instance
turn counters, drains queued `StatusEvent`s into the engine's HUD
pipeline, and bridges from art-record `EnemyEffect` bytes through
`StatusKind::from_enemy_effect`. Damage-over-time formulas (Burned =
`max_hp / 16`, Poisoned = `current_hp / 8`) live alongside.

## `battle_formulas`

Damage / MP-cost / accuracy / RNG arithmetic kernels.
`art_strike_damage(attack, defense, multiplier, divisor, floor)`
applies the per-strike Tactical Art damage formula; `accuracy_roll`
and `mp_cost_after_ability_bits` mirror the retail bit-test selectors
in `FUN_800402F4`.

## `action_validator`

16-arm action validator (`FUN_8003fb10`) - clean-room port of the
per-slot "target valid" predicate the menu / UI consults before
committing a player choice.

## See also

- [`docs/subsystems/script-vm.md`](../../docs/subsystems/script-vm.md)
- [`docs/subsystems/actor-vm.md`](../../docs/subsystems/actor-vm.md)
- [`docs/subsystems/effect-vm.md`](../../docs/subsystems/effect-vm.md)
- [`docs/subsystems/move-vm.md`](../../docs/subsystems/move-vm.md)
