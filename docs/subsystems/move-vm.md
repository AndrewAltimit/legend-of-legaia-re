# Move-table opcode VM

A bytecode VM dedicated to per-actor animation, motion, and state. Distinct from the [field/event script VM](script-vm.md) and the [actor / sprite VM](actor-vm.md): the move VM lives in `SCUS_942.54`, runs on the per-actor "move buffer" set up by `FUN_800204F8`, and is invoked every frame from the actor tick. The opcode space is **71 instructions** (`0x00..0x46`); opcode `0x2F` escapes to a per-overlay extension dispatcher in the town overlay (`FUN_801D362C`, **61 sub-opcodes** `0x00..0x3C`).

## Three-VM picture

| VM | Driver fn | Where | Opcode count | Operand width |
|---|---|---|---|---|
| Actor / sprite VM | `FUN_801D6628` | Title-screen overlay (0971) | 13 (`docs/subsystems/actor-vm.md`) | byte stream |
| **Move VM** | `FUN_80023070` | `SCUS_942.54` | 71 (`0x00..0x46`) | u16 stream |
| Field / event VM | `FUN_801DE840` | Town overlay (0897) | 43 (`0x21..0x4F`, with gaps) + 0x5x/6x/7x default-route | byte stream |

The three are wired:
- Field VM op `0x22` `EXEC_MOVE` calls `FUN_800204F8`, which finds the move record for `move_id` and stages it into the actor at `actor[+0x48]` (buffer base) / `actor[+0x70]` (PC).
- Actor tick (`FUN_80021DF4`, per frame) and actor spawn (`FUN_80021B04`, one-shot) both call `FUN_80023070(actor)` to step the move buffer.
- Move-VM opcode `0x2F` calls `FUN_801D362C(actor, opcode_ptr)` for **scene-specific extension opcodes** that live in the town overlay.

## Function signature

```c
int FUN_80023070(int actor);
```

No PC argument: the PC is stored on the actor at `+0x70`. The function loops, executing opcodes until one of them clears the loop flag (e.g. opcode `0x08` "halt"). Each handler updates `actor[+0x70]` by writing `param_3` (the local "size in u16 units") into a register that the loop epilogue commits.

## Top-level dispatch

```c
short* op = (short*)(actor[+0x48] + actor[+0x70] * 2);   // u16-aligned PC
short  v1 = op[0];
if (v1 >= 0x47) goto epilogue;                            // out-of-range → end loop
v0 = jt[v1];                                              // JT at 0x80010778
goto v0;                                                  // computed jump
```

- Buffer base: `actor[+0x48]`. The pointer `_DAT_8007B888` (MOVE) or `_DAT_8007B840` (MOVE2) is the runtime root, but per-actor offsets are baked in at setup time by `FUN_800204F8`.
- PC: `actor[+0x70]` (i16, in **u16 units**). Each handler advances by its case-specific size.
- Jump table: `0x80010778` in `SCUS_942.54`, **71 entries × 4 bytes**.

## Actor-side state surfaced by opcodes

The move VM rewrites a wide swath of the actor struct:

| Offset | Type | Use |
|---|---|---|
| `+0x10` | u32 | Actor flag word. Bit `0x8` set by op `0x08`; `0x2`/`0x1000`/`0x10000`/`0x40000000` toggled by various opcodes. |
| `+0x14`/`+0x16`/`+0x18` | u16 | World X / Y / Z (op `0x07` absolute set, op `0x01` add, op `0x03` rotate-add). |
| `+0x22` | u16 | Y-rotation (per-frame ramp by op `0x2D`/`0x35`/`0x37`). |
| `+0x24`/`+0x26`/`+0x28` | u16 | Camera/render slots (op `0x05` add, op `0x06` write `+0x26`, op `0x39` write all three). |
| `+0x2A` | u16 | World Y mirror (kept in sync with `+0x16` for collision lookup). |
| `+0x3C`/`+0x3E`/`+0x40` | u16 | Animation bank (op `0x00`: `[v << 3]`). |
| `+0x44` | int* | Pointer to a per-actor scratch struct (used by `0x3C` and the `0x44+` family). |
| `+0x54` | u16 | Wait/timer (op `0x09` set; ticked down by `FUN_80021DF4`). |
| `+0x62` | u16 | Local flag bank — 16 bits. AND/OR by ops `0x31`/`0x32`. |
| `+0x70` | i16 | **The move-VM PC** (in u16 units). |
| `+0x74` | u32 | Composite control word (op `0x33` clears bit `0x40000000`). |
| `+0x80`/`+0x82`/`+0x84` | u16 | Animation slots, `[v << 3]` (op `0x04`). |
| `+0x90`/`+0x92`/`+0x94` | u16 | Tween source (op `0x35`/`0x37` absolute / increment). |
| `+0x96`/`+0x98`/`+0x9A` | u16 | Tween scale (op `0x2E`, `[v << 3]`). |
| `+0x9C`/`+0xA0`/`+0xA4`/`+0xA8` | i32 | Tween block (op `0x34`, 9-word setup). |
| `+0xAC..+0xCA` | mixed | Per-frame anim slots (key/curve data; op `0x2C` configures, `+0xC0` is the duration). |
| `+0xB0..` | u8 | Per-keyframe descriptor (op `0x0A` writes `count` 3-byte slots). |
| `+0xB2` | i16 | Misc (op `0x38` add). |

## Opcode reference

The cases below are paraphrased from `FUN_80023070` (`ghidra/scripts/funcs/80023070.txt`). Sizes are in **u16 units** (each opcode + operands consumes `param_3` halfwords).

### 0x00 — `ANIM_BANK_SET` (size 4)

`[u16 op, u16 v1, u16 v2, u16 v3]`. `actor[+0x3C..+0x40] = v1..v3 << 3`.

### 0x01 — `WORLD_ADD` (size 4)

`actor[+0x14] += v1; actor[+0x16] += v2; actor[+0x2A] += v2; actor[+0x18] += v3` (Y mirror in `+0x2A`).

### 0x02 — `BANK_SET_98` (size 2)

`actor[+0x98] = v1 << 3`.

### 0x03 — `WORLD_ROTATE_ADD` (size 2)

Adds rotated `v1` into world X / Z using sin/cos table at `_DAT_8007B81C` / `DAT_8007B7F8` indexed by `actor[+0x96] & 0xFFF`.

### 0x04 — `ANIM_BANK_2` (size 4)

`actor[+0x80..+0x84] = v1..v3 << 3`.

### 0x05 — `RENDER_BANK_ADD` (size 4)

`actor[+0x24..+0x28] += v1..v3`.

### 0x06 — `WRITE_26` (size 2)

`actor[+0x26] = v1`.

### 0x07 — `WORLD_SET` (size 4)

Absolute `actor[+0x14..+0x18] = v1..v3` (Y mirror in `+0x2A` matches `v2`).

### 0x08 — `HALT` (size 0, ends loop)

`actor[+0x10] |= 0x8`. Sets `bVar3 = false` so the dispatcher exits without advancing PC.

### 0x09 — `WAIT_SET` (size 2, ends loop)

`actor[+0x54] = v1 << 3` (the wait timer; `FUN_80021DF4` ticks it down each frame).

### 0x0A — `KEYFRAME_LOAD` (variable, size = `3 + count*3`)

`actor[+0x10] |= 0x1000`. `actor[+0x6C] = byte(op[2])`. Then loops `count = op[2]` writing `actor[+0xB0+i] = byte(op[3+3*i])` and two scaled curves for `+0xB8` / `+0xC8` per slot.

### 0x16 — `STUB` (size 2)

Calls `FUN_80024C80(actor, op[1])`. The body is a pure `jr ra` / nop — the opcode is a no-op. A clean-room port can implement it as PC-advance only; see `ghidra/scripts/funcs/80024c80.txt`.

### 0x2C — `KEY_BUFFER_ALLOC` (size 5)

`[op, count_x, count_y, w, h]` configures `actor[+0xA0..+0xA6] = ops[1..4]`. If `w >= 0x11` allocates `w * h * 2` bytes via `FUN_80017888` and stores at `actor[+0xA8]`; else uses the inline buffer at `actor[+0xAC]`. Then `FUN_8005842C` initialises the descriptor at `actor[+0xA0]`.

### 0x2D — `WORLD_INC_VARIANT` (size 4)

`actor[+0x90] += v1; actor[+0x92] += v2; actor[+0x94] += v3`.

### 0x2E — `TWEEN_SCALE_SET` (size 4)

`actor[+0x96] = v1 << 3; actor[+0x98] = v2 << 3; actor[+0x9A] = v3 << 3`.

### 0x2F — `OVERLAY_EXT` (size = handler return)

```c
param_3 = func_0x801d362c(actor, op);
```

**Escape to overlay-defined extension opcodes.** The town overlay's `FUN_801D362C` reads `op[1]` as a 16-bit sub-opcode (range `0x00..0x3C`) and dispatches via its own JT at `0x801CE868` (61 entries × 4 bytes). Each sub-handler returns the size in u16 units. Sub-handlers at `0x801D31B0`, `0x801D32F8`, `0x801D3444`, `0x801D3748`, `0x801D52D0`, etc. are members of this table.

The 16-slot, 8-byte-stride scratch table at `&DAT_801F3498` is shared across actors — sub-ops `0x25/0x26` round-trip world coords (8 B), `0x27/0x28` round-trip the tween-source triple at `+0x90` (with `>> 12` fixed-point scaling and `[-0xFF, 0xFF]` clamping on read), `0x31/0x32` round-trip the render-bank section at `+0x24..+0x2C`, and `0x34/0x35` round-trip `actor[+0x72]`. Sub-op `0x0C` sets `actor[+0x50]` (the midpoint blend / sub-state byte consumed by the `FUN_801E45BC` mid-point helper from sub-ops `0x0E` / `0x12`); sub-op `0x0D` is the additive variant.

Two move-VM globals live alongside the slot table: `DAT_801F22F4` (a u32 predicate set/cleared by sub-ops `0x08`/`0x09` and tested by `0x0A`/`0x0B`) and `DAT_801F22F6` (a u16 counter wrapped mod 16). Sub-op `0x0F` clears the counter; `0x10` reads it (wrapping when `>= 16`), captures the low byte into `actor.field_86`, and increments. Sub-op `0x11` then saves world coords to `slot_table[field_86 & 0xFF]` — i.e. the cycle counter feeds the slot-save index, distinct from `0x25` which takes the index from the operand stream.

World-position lerp lives in sub-ops `0x24` / `0x2A`. Both share the per-axis form `actor[axis] = base + ((target - base) * t) >> 12`. The Y axis always lerps toward `_DAT_8007C364 + 0x16` (player Y). For X / Z, sub-op `0x24` uses the fixed map origin `(_DAT_80089118, _DAT_80089120)` (target = `-(base + origin)`); sub-op `0x2A` uses the player position (target = player X / Z). Sub-ops `0x06` / `0x07` are the bbox-vs-player gate variants — `0x06` skips a 7-u16 follow-up payload when the player is **outside** the canonicalised box `[xa..xb]×[za..zb]` (each scaled by `0x80` with a `0x40` half-cell margin); `0x07` skips when the player is **inside**.

Sub-ops `0x0E` / `0x12` share a "midpoint to actor world" idiom backed by `FUN_801E45BC`. `0x0E` is the all-operand form (size 11): `actor.world = op[5..7] + ((op[2..4] + op[8..10]) >> 1)` then the helper applies `actor[+0x50]` blend mode. `0x12` (size 8) is the slot-indexed variant: the `a` triple comes from `slot_table[actor[+0x86] & 0xFF]` instead of operand u16s, and only `op[2..4]` (offset) and `op[5..7]` (b) live in bytecode. Sub-ops `0x36`/`0x37` are axis predicates against `0x8E - DAT_8007C348`: pass → continue (size 1), fail → skip 3-u16 follow-up (size 4). Sub-ops `0x38`/`0x39` are squared-distance gates between the move actor and the player (`_DAT_8007C364`); `0x38` continues when *outside* radius `op[2]`, `0x39` continues when *inside*. Sub-op `0x23` is the anim-bank lerp toward operand world coords using the scratchpad ramp ratio at `_DAT_1F800393` over `op[5]`, with the divide guarded against `op[5] == 0`. Sub-ops `0x13`/`0x14` query the fourth flag bank (`DAT_80086D70`) and gate on the result with the same size-1-or-4 shape; `0x14` inverts the predicate.

Three sub-ops mutate the move bytecode buffer in place — these are "self-modifying" with respect to the operand stream that follows. `0x04` writes `actor[+0x14..+0x18]` (world XYZ) into `buffer[state.pc + op[2] + 3..+6]` (3 u16 stores); subsequent ops that read those slots see the captured world snapshot. `0x1E` is read-modify-write on a single u16 — `buffer[state.pc + op[2] + 4] += op[3]`. `0x1B` is an in-bytecode copy loop — for `i in 0..op[4]`, `buffer[state.pc + op[3] + i + 5] = buffer[state.pc + op[2] + i + 5]`. The base offset of 5 (versus 3 for `0x04`, 4 for `0x1E`) targets the operand region past the count word, so the bytes following `0x1B`'s instruction header are effectively an inline scratch buffer indexed by op[2]/op[3]. The `MoveHost::move_bytecode_{read,write}_u16` callbacks expose the actor's move buffer to these ops; the engine layer wires them to `actor[+0x48][word_off]`.

Sub-ops `0x1F` / `0x20` are HSV-space ramps on a packed 24-bit RGB color stored in `actor[+0xa0..+0xa3]` (`0x1F`) or `actor[+0xa4..+0xa7]` (`0x20`). The packed `(R, G, B)` is decomposed (R = byte 0, G = byte 1, B = byte 2), converted RGB→HSV via the SCUS helper at `FUN_8001a78c` (H ∈ 0..0x167, S ∈ 0..255, V ∈ 0..255), then `op[2..4]` are added per channel (H wraps mod `0x168`, S/V clamp to 0..255), then HSV→RGB via `FUN_8001a8dc` (clamped to 0..0xF8 by `FUN_8001a6c8`) and re-packed. The size-1 default-arm return is intentional — the operand stream `op[2..]` is also re-interpreted as outer opcode `0x1F` / `0x20` on the next dispatch (a bytecode-density trick: one HSV ramp instruction simultaneously seeds an `actor[+0x9E..+0xAE]` anim-block update). `crates/engine-vm` ships the clean-room `rgb_to_hsv` / `hsv_to_rgb` pair that mirrors the SCUS algorithms exactly.

The fourth flag bank at `DAT_80086D70` is shared between the move VM (sub-ops `0x13` / `0x14` predicate, `0x1C` / `0x1D` set / clear) and the field VM (high-byte default routes `0x5x` set / `0x6x` clear / `0x7x` test). `engine-core::World` exposes it as a single lazily-grown `system_flags: Vec<u8>` with MSB-first bit ordering (mirroring `FUN_8003CE08`'s `0x80 >> (idx & 7)`). The field VM's `idx` encoding `((opcode_byte & 0x8F) << 8) | operand_byte` ranges over `0..=0x87FF`, which is why the bank can't be a fixed-size 256-bit array.

Sub-ops `0x3A`, `0x3B`, `0x3C` close out the player-relative cluster:
- `0x3A` writes the angle from the actor to the player (computed as `atan2(dz, dx)` quantised to PSX 12-bit angle units, 4096 = full circle) into `bytecode[state.pc + op[2] + 3]`. Engines wire `MoveHost::ext_compute_angle` to surface the player position; the world-side default reads `world.player_actor_slot`.
- `0x3B` looks up the position of party-member `op[2]` and writes the world-XYZ triple into `bytecode[state.pc + op[3] + 4..+6]`. Pre-clears the dst slots before the lookup so a no-table host still gets the zero-sentinel guarantee. When the lookup returns `None`, the size is `4` (skip the follow-up payload). Engines populate `world.party_actor_slots: Vec<Option<u8>>` with the live party-to-actor-slot map.
- `0x3C` writes the immediate fade colour to scratchpad globals (`ticks == 0`) or schedules a per-frame ramp (`ticks > 0`). The world records the request in `world.pending_fade: Option<FadeRequest>` so engines can drain it each frame to drive the screen overlay.

**Sub-op coverage in `crates/engine-vm`: 61/61 dispatched (every entry of the `FUN_801D362C` JT at `0x801CE868`).** Some sub-ops have host-trait stubs that fall through to no-ops on the default `MoveHost` impl (the world wires the ones with natural state — `ext_compute_angle`, `ext_party_member_lookup`, `ext_fade_color`, `ext_query_flag_bank`, `ext_set_flag_bank`, `ext_clear_flag_bank`, `ext_scratchpad_*`, `ext_set_8007b9d8`). The remaining stubs (`ext_debug_world`, `ext_func56798`, `ext_midpoint_set`, `ext_func801d31b0`, `ext_emit_ot_packet`, `ext_world_struct_*`, `ext_17`, `ext_20`) carry pure rendering / opaque-PsyQ side-effects and are best overridden per engine.

### 0x30 — `KEY_BUFFER_FREE` (size 0, ends loop / falls into 0x22)

If `actor[+0xA8]` was heap-allocated, calls `FUN_800583C8(actor + 0xA0, buf)`; else uses the inline buffer at `actor + 0xAC`. Clears `actor[+0x9C]`. Goto `caseD_22` epilogue.

### 0x31 — `LFLAG_AND` (size 2)

`actor[+0x62] &= v1`. Per-actor local flag bank (16 bits).

### 0x32 — `LFLAG_OR` (size 2)

`actor[+0x62] |= v1`.

### 0x33 — `CLEAR_BIT_40000000` (size 1)

`actor[+0x74] &= ~0x40000000`.

### 0x34 — `TWEEN_SETUP` (size 9)

Loads 8 i16 operands into `actor[+0xAC..+0xC8]` (with `+0xAC`/`+0x9C..0xA8` zero-extended to i32). 9-u16 instruction.

### 0x35 — `WORLD_INC_VARIANT2` (size 3)

`actor[+0x90] += v1; actor[+0x92] += v2`.

### 0x36 — `TWEEN_DURATION_SET` (size 3)

`actor[+0x98] = v1 << 3; actor[+0x9A] = v2 << 3; actor[+0xB8] = 0`.

### 0x37 — `WORLD_SET_VARIANT2` (size 3)

`actor[+0x90] = v1; actor[+0x92] = v2`.

### 0x38 — `B2_ADD` (size 2)

`actor[+0xB2] += v1`.

### 0x39 — `RENDER_BANK_SET` (size 4)

Absolute `actor[+0x24..+0x28] = v1..v3`.

### 0x3A / 0x3B — `FLAG_2_SET` / `FLAG_2_CLEAR` (size 1)

`actor[+0x10] |= 2` / `actor[+0x10] &= ~2`.

### 0x3C — `SCRATCH_WRITE` (size ?)

Writes `(int)(short)v1` into `*actor[+0x44]` (the indirect scratch slot).

(Cases beyond `0x3C` are documented in the function dump but follow the same pattern; consult `ghidra/scripts/funcs/80023070.txt` for the exhaustive list.)

## Control flow

The interpreter loops: read opcode at `actor[+0x70]`, dispatch, write the new PC `actor[+0x70] += param_3`, then either continue or break depending on `bVar3`. Break opcodes:

- `0x08` (HALT): clears the loop flag.
- `0x09` (WAIT_SET): wait timer is set; further opcodes deferred to next frame.
- A few epilogue cases (e.g. `0x30`) jump to the epilogue without advancing.

After the loop exits, the function returns; the caller (`FUN_80021B04` or `FUN_80021DF4`) gets a "tick complete for this actor" signal. The next frame's `FUN_80021DF4` updates physics, then re-enters `FUN_80023070` from the saved PC.

## Connection to other crates

- **`crates/mdt`** — parses the [MDT format](../formats/mdt.md). The per-frame data inside an MDT record is exactly the move-VM bytecode this VM consumes. With the move-VM opcode set documented, `crates/mdt` can grow a disassembler.
- **`crates/engine-vm`** — clean-room Rust port lives in `move_vm.rs`. The dispatcher (`step` / `run_until_break`), every opcode handler, and the `MoveHost` callback trait live there. Per-frame entry is `move_vm::actor_tick`, which mirrors the gate at `FUN_80021DF4 + 0x80022B94..0x80022BBC`: skip when `wait_timer >= 0`, otherwise step the VM and report `Halted` if the post-call `flags & 0x8` bit is set. `decrement_wait_timer` is the matching pre-tick helper.
- **Field VM op `0x22`** (`EXEC_MOVE`) — the gateway from script-VM into move-VM; calls `FUN_800204F8` to set up the per-actor buffer.

## Decompile quirks worth knowing

- **Operand units are u16**, not bytes. `op[1]` is the 16-bit operand at offset 2 from the opcode word.
- **PC is also in u16 units.** `actor[+0x70]` × 2 is the byte offset.
- **`param_3` is the size in u16 units**, not bytes. The dispatcher epilogue does `actor[+0x70] += param_3`.
- **0x47-bound check**: `sltiu v0, v1, 0x47`. Out-of-range opcodes silently fall through to the loop-exit, the same shape as the field-VM `default` arm. Treat any opcode `>= 0x47` as "end of move buffer" rather than "unknown opcode".
- **Cases that "look like NOPs" in the C decompile** still advance the PC via `param_3` — the increment is set in MIPS branch-delay slots and is invisible at the C level (same pattern as the field VM, see `script-vm.md` § "Decompile quirks").
