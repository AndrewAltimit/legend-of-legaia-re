# Move VM - `0x2F` overlay-extension dispatcher

This page details the move VM's `0x2F` `OVERLAY_EXT` opcode and the 61 sub-opcodes
of its overlay-resident extension dispatcher. It is split out of
[`move-vm.md`](move-vm.md) for length; the opcode-reference entry there links here.

## `0x2F` - `OVERLAY_EXT` (size = handler return)

```c
param_3 = func_0x801d362c(actor, op);
```

**Escape to overlay-defined extension opcodes.** `FUN_801D362C` reads `op[1]` as a 16-bit sub-opcode (range `0x00..0x3C`) and dispatches via its own JT at `0x801CE868` (61 entries × 4 bytes).

### Bounds check (no out-of-bounds-jump path)

The sub-opcode is bounds-checked before the indirect jump:

- `lh v1, 0x2(s3)` loads it sign-extended, then `sltiu v1, 0x3D` gates the `jr` - out-of-range values branch to the dispatcher's plain return (`size = 1`).
- Because the compare is *unsigned*, the sign-extended `lh` also rejects negative sub-opcodes (they read as huge unsigned values).

So this overlay-escape, despite being an indirect-jump-table dispatch on a bytecode-supplied operand, has **no out-of-bounds-jump path** - a relevant property given the move buffer is partly attacker-influenceable (the self-modifying sub-ops `0x04`/`0x1B`/`0x1E` below write into it). The from-scratch port mirrors the guarded return with a `_ => default_arm()` catch-all for any sub-opcode `>= 0x3D`.

### Overlay residency - one copy, in the field overlay only

The dispatcher and its JT live in **PROT 0897 alone**. The earlier "many overlays, each with its own JT contents" reading is **falsified**:

- The `0897` **static** dump plus the six **capture-derived** ones (`world_map` / `world_map_walk` / `dialog_mc4` / `dialog_typing` / `cutscene_dialogue` / `cutscene_mapview` `_801d362c.txt`) all disassemble **byte-identically**, 1293 instructions each - one 0897-resident function observed under different scenario labels (world-map, dialog and mapview-cutscene play are all 0897-hosted modes). The static dump has no coverage gaps: Ghidra followed the JT flow.
- In the disc images of every other mapped slot-A overlay the VA `0x801D362C` holds unrelated bytes: menu 0899 / fishing 0972 / slot-machine 0973 / baka 0976 / dance 0980 carry mid-function code of their own, cutscene_str 0970 is zero-fill there, battle-action 0898 has a different battle function (a save-block `0x80084140` walker) at that address, and the title overlay holds data tables. None has a 61-pointer JT at `0x801CE868` (0897's is at file `+0x50`; the others carry path strings or unrelated data there).

Since the SCUS opcode arm calls the **fixed VA** `0x801D362C`, op `0x2F` is only executable while the field overlay is resident - in any other overlay generation it would jump into unrelated code. Battle-side move records (monster archive, summon stagers) therefore cannot use op `0x2F`; the extension sub-ops are a field/world-map/dialog-mode vocabulary.

Each sub-handler returns the size in u16 units. Sub-handlers at `0x801D31B0` (per-scanline POLY_FT4 strip emitter), `0x801D32F8`, `0x801D3444`, `0x801D3748`, `0x801D52D0`, etc. are members of the 0897 table.

## Instruction widths

Every arm leaves its instruction's width, in u16 halfwords, in `s2` before joining the shared epilogue at `0x801D4A3C` (`sll v0, s2, 0x10`, then the return shifts back, so the count is a sign-extended 16-bit value).

**There is no size-1 arm.** The only `li s2, 0x1` in `FUN_801D362C` is at `0x801D365C`, in the branch delay slot of the `sltiu v1, 0x3D` bounds check - the resync width for a sub-opcode the jump table does not cover. Every in-range sub-opcode has a wider arm of its own.

The width is invisible in the decompiled C: Ghidra renders each arm's `j 0x801D4A3C` exit as a `func_0x801d4a3c()` label-call and drops the `li s2, N` sitting in its delay slot, so a C-sourced reading of any arm reports the bounds-check default. This is the label-call artifact catalogued in [`ghidra.md`](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims), and it is why the widths have to be read off the disassembly.

Returning 1 from an in-range arm leaves the PC on the sub-opcode word, which the **outer** move-VM opcode space then decodes as an instruction - `0x25` there is `CHILD_SPAWN`, `0x1F`/`0x20` are anim-block ops, and so on. On a looping record that mis-decode re-fires every tick.

### Fall-through width by sub-opcode

| Width | Sub-opcodes |
|---|---|
| 2 | `0x01` `0x02` `0x03` `0x08` `0x09` `0x0F` `0x10` `0x11` `0x15` `0x16` |
| 3 | `0x04` `0x0A` `0x0B` `0x0C` `0x0D` `0x1C` `0x1D` `0x25` `0x26` `0x27` `0x2F` `0x31` `0x32` `0x34` `0x35` `0x3A` |
| 4 | `0x13` `0x14` `0x1E` `0x36` `0x37` `0x38` `0x39` `0x3B` |
| 5 | `0x05` `0x18` `0x1B` `0x1F` `0x20` `0x21` `0x22` `0x28` `0x29` `0x30` |
| 6 | `0x23` `0x2B` `0x2D` `0x33` `0x3C` |
| 7 | `0x06` `0x07` `0x2C` |
| 8 | `0x12` `0x17` `0x19` `0x1A` `0x24` `0x2A` |
| 11 | `0x0E` |
| 13 | `0x2E` |
| 16 | `0x00` |

For the ten branch arms listed below, the width above is the fall-through side; the taken side adds a displacement read from the instruction's last operand word.

The mirror is `move_vm_overlay_ext::canonical_size`; `crates/engine-vm`'s unit tests dispatch every sub-opcode against it.

Two members are easy to lump with a neighbour and are not the same width. `0x18` is 5 where `0x17` / `0x19` / `0x1A` are 8: it zeroes the world-struct record's first three u16s and seeds the last two from `actor.world_y + op[3]` and `+ op[4]`, so it reads two operand words, not five. `0x11` is 2 where `0x25` is 3: `0x11` takes its slot index from the cycle counter, `0x25` from the bytecode.

### The ten conditional-branch arms

Ten arms have a second, data-dependent exit. They are **branches**: the last operand word is a signed halfword displacement added to the fall-through width. All ten reach the same tail at `0x801D4830`, which returns the preset `s2` when the predicate is false and `lhu v0,0x6(s3); addiu s2,v0,0x4` when it is true. The `lhu` is zero-extending but the epilogue truncates to 16 bits and sign-extends, so a negative displacement walks the PC backwards - the spin-wait-until-condition idiom.

| Sub-ops | Encoding | Branch taken when |
|---|---|---|
| `0x06` / `0x07` | `[2F][op][xa][za][xb][zb][delta]`, base 7 | player outside / inside the box |
| `0x0A` / `0x0B` | `[2F][op][delta]`, base 3 | `DAT_801F22F4` set / clear |
| `0x13` / `0x14` | `[2F][op][flag][delta]`, base 4 | flag set / clear |
| `0x36`..`0x39` | `[2F][op][arg][delta]`, base 4 | the arm's own predicate holds |

`0x06` / `0x07` add their delta through `0x801D3868` (`addu s2,s2,v0`) rather than the shared `addiu`, and take it from `op[6]`; `0x0A` / `0x0B` take it from `op[2]` through `0x801D38C8`. The rest read `op[3]`.

Three arms look conditional and are not. `0x28`'s clamp cascade, `0x3B`'s party-lookup miss and `0x23`'s divide-by-zero guard each choose what the instruction *does*, not how wide it is - `li s2` is set before the branch in all three.

## Sub-op clusters

### Shared scratch table `&DAT_801F3498`

The 16-slot, 8-byte-stride scratch table at `&DAT_801F3498` is shared across actors:

- `0x25`/`0x26` round-trip world coords (8 B).
- `0x27`/`0x28` round-trip the tween-source triple at `+0x90` (with `>> 12` fixed-point scaling and `[-0xFF, 0xFF]` clamping on read).
- `0x31`/`0x32` round-trip the render-bank section at `+0x24..+0x2C`.
- `0x34`/`0x35` round-trip `actor[+0x72]`.

Sub-op `0x0C` sets `actor[+0x50]` (the midpoint blend / sub-state byte consumed by the `FUN_801E45BC` mid-point helper from sub-ops `0x0E` / `0x12`); sub-op `0x0D` is the additive variant.

### Move-VM globals + cycle counter

Two move-VM globals live alongside the slot table:

- `DAT_801F22F4` - a u32 predicate set/cleared by sub-ops `0x08`/`0x09` and tested by `0x0A`/`0x0B`.
- `DAT_801F22F6` - a u16 counter wrapped mod 16.

Sub-op `0x0F` clears the counter; `0x10` reads it (wrapping when `>= 16`), captures the low byte into `actor.field_86`, and increments. Sub-op `0x11` then saves world coords to `slot_table[field_86 & 0xFF]` - i.e. the cycle counter feeds the slot-save index, distinct from `0x25` which takes the index from the operand stream.

### World-position lerp (`0x24` / `0x2A`) + bbox gates (`0x06` / `0x07`)

World-position lerp lives in sub-ops `0x24` / `0x2A`. Both share the per-axis form `actor[axis] = base + ((target - base) * t) >> 12`. The Y axis always lerps toward `_DAT_8007C364 + 0x16` (player Y). For X / Z:

- sub-op `0x24` uses the fixed map origin `(_DAT_80089118, _DAT_80089120)` (target = `-(base + origin)`);
- sub-op `0x2A` uses the player position (target = player X / Z).

Sub-ops `0x06` / `0x07` are the bbox-vs-player branch variants. Both are 7 halfwords wide and branch by `op[6]`: `0x06` when the player is **outside** the canonicalised box `[xa..xb]×[za..zb]` (each scaled by `0x80` with a `0x40` half-cell margin), `0x07` when **inside**.

The canonicalisation is a bytecode write, not a local: `sh v1,0x4(s3)` / `sh a0,0x4(s0)` at `0x801D3784` swap `op[2]` with `op[4]` when `op[4] < op[2]`, and the sibling pair swaps `op[3]` with `op[5]`. That puts these two in the self-modifying family with `0x04` / `0x1B` / `0x1E`.

### Midpoint-to-actor (`0x0E` / `0x12`) + player-relative predicates

Sub-ops `0x0E` / `0x12` share a "midpoint to actor world" idiom backed by `FUN_801E45BC`:

- `0x0E` is the all-operand form (size 11): `actor.world = op[5..7] + ((op[2..4] + op[8..10]) >> 1)` then the helper applies `actor[+0x50]` blend mode.
- `0x12` (size 8) is the slot-indexed variant: the `a` triple comes from `slot_table[actor[+0x86] & 0xFF]` instead of operand u16s, and only `op[2..4]` (offset) and `op[5..7]` (b) live in bytecode.

Other player-relative predicates:

- Sub-ops `0x36`/`0x37` are axis predicates against `0x8E - DAT_8007C348`, and `0x38`/`0x39` are squared-distance gates between the move actor and the player (`_DAT_8007C364`); `0x38` fires when *outside* radius `op[2]`, `0x39` when *inside*. All four are 4-halfword branches on the shared `0x801D4830` tail - pass → `4 + op[3]`, fail → 4. The "pass → size 1" reading is falsified: each arm presets `li s2, 0x4` before jumping to that tail.
- Sub-op `0x23` is the anim-bank lerp toward operand world coords using the scratchpad ramp ratio at `_DAT_1F800393` over `op[5]`, with the divide guarded against `op[5] == 0`.
- Sub-ops `0x13`/`0x14` are **conditional branches** on the fourth flag bank (`DAT_80085758`): encoding `[2F][13|14][flag][delta]`, where the taken side returns size `4 + delta` (`lhu v0,0x6(s3); addiu s2,v0,4` at `0x801D4838`) and the untaken side returns 4. `0x13` branches when the flag is set, `0x14` when it is clear; `delta` is signed, and a negative delta onto a preceding `0x09` wait forms the spin-wait-until-flag idiom jou's ambient lightning cyclers idle on (`2F 14 0364 FFFA`).

### Self-modifying bytecode ops (`0x04` / `0x1B` / `0x1E`)

Three sub-ops mutate the move bytecode buffer in place - these are "self-modifying" with respect to the operand stream that follows:

- `0x04` writes `actor[+0x14..+0x18]` (world XYZ) into `buffer[state.pc + op[2] + 3..+6]` (3 u16 stores); subsequent ops that read those slots see the captured world snapshot.
- `0x1E` is read-modify-write on a single u16 - `buffer[state.pc + op[2] + 4] += op[3]`.
  Its size is **4** (it skips its own operand words): the raw arm at
  `overlay_0897_801d362c` `0x801D3E18..` ends `li s2, 0x4` before the shared
  `j 0x801D4A3C` size-return, which the decompiled C renders as a
  `func_0x801d4a3c()` label-call with the size dropped. The default-arm
  reading made a `0x2F 0x1E` instruction re-execute its own operands as
  opcodes - jou's ambient CLUT-cycler record (which patches its *following*
  op-`0x2C` operand, then falls through to execute that `0x2C`) is the disc
  witness for the size-4 decode. See
  [`field-ambient-fx.md`](field-ambient-fx.md#the-self-modifying-spawn-stepper).
- `0x1B` is an in-bytecode copy loop - for `i in 0..op[4]`, `buffer[state.pc + op[3] + i + 5] = buffer[state.pc + op[2] + i + 5]`.

The base offset of 5 (versus 3 for `0x04`, 4 for `0x1E`) targets the operand region past the count word, so the bytes following `0x1B`'s instruction header are effectively an inline scratch buffer indexed by op[2]/op[3]. The `MoveHost::move_bytecode_{read,write}_u16` callbacks expose the actor's move buffer to these ops; the engine layer wires them to `actor[+0x48][word_off]`.

### HSV color ramps (`0x1F` / `0x20`)

Sub-ops `0x1F` / `0x20` are HSV-space ramps on a packed 24-bit RGB color stored in `actor[+0xa0..+0xa3]` (`0x1F`) or `actor[+0xa4..+0xa7]` (`0x20`). The packed `(R, G, B)` is decomposed (R = byte 0, G = byte 1, B = byte 2), converted RGB→HSV via the SCUS helper at `FUN_8001a78c` (H ∈ 0..0x167, S ∈ 0..255, V ∈ 0..255), then `op[2..4]` are added per channel (H wraps mod `0x168`, S/V clamp to 0..255), then HSV→RGB via `FUN_8001a8dc` (clamped to 0..0xF8 by `FUN_8001a6c8`) and re-packed.

Both are ordinary **5-halfword** instructions: `[2F][1F|20][dH][dS][dV]`. The single arm serving them sets `li s2, 0x5` at `0x801D3F60`, after the HSV→RGB call and before the shared `j 0x801D4A3C` at `0x801D3F80`, and no other path through it writes `s2`.

The earlier reading - that the size-1 return is intentional, and `op[2..]` is deliberately re-read as outer opcode `0x1F` / `0x20` to seed an anim-block update in the same instruction - is **falsified**. It came from the decompiled C, where the exit renders as a label-call and the delay-slot `li` disappears; the same artifact had already produced a wrong size for `0x1E`. There is no density trick here, and the three operand words are the H/S/V deltas rather than a second instruction.

The re-pack is a full 32-bit `sw` (`sw v1,0x0(s0)` at `0x801D3F84`), so the packed word's top byte is cleared rather than preserved. `crates/engine-vm` ships the from-scratch `rgb_to_hsv` / `hsv_to_rgb` pair that mirrors the SCUS algorithms exactly.

### Fourth flag bank (shared with the field VM)

The fourth flag bank at `DAT_80085758` is shared between the move VM (sub-ops `0x13` / `0x14` predicate, `0x1C` / `0x1D` set / clear) and the field VM (high-byte default routes `0x5x` set / `0x6x` clear / `0x7x` test). `engine-core::World` exposes it as a single lazily-grown `system_flags: Vec<u8>` with MSB-first bit ordering (mirroring `FUN_8003CE08`'s `0x80 >> (idx & 7)`). 

The field VM's `idx` encoding is `((opcode_byte & 0x8F) << 8) | operand_byte`, ranging over `0..=0x8FFF` in retail: the route select at `0x801E3570` tests the **raw** opcode byte (`andi v1,v0,0x70`), so `0xF0..0xFF` reach the same routes and `0xFF & 0x8F = 0x8F` tops the index out. That is why the bank cannot be a fixed-size 256-bit array. The port's match arm is narrower - `0x50..=0x77`, which caps the index at `0x87FF` - and does not cover masked opcodes `0x78..=0x7F`.

### Player-relative cluster close-out (`0x3A` / `0x3B` / `0x3C`)

Sub-ops `0x3A`, `0x3B`, `0x3C` close out the player-relative cluster:

- `0x3A` writes the angle from the actor to the player (computed as `atan2(dz, dx)` quantised to PSX 12-bit angle units, 4096 = full circle) into `bytecode[state.pc + op[2] + 3]`. Engines wire `MoveHost::ext_compute_angle` to surface the player position; the world-side default reads `world.player_actor_slot`.
- `0x3B` looks up the position of party-member `op[2]` and writes the world-XYZ triple into `bytecode[state.pc + op[3] + 4..+6]`. Pre-clears the dst slots before the lookup so a no-table host still gets the zero-sentinel guarantee. When the lookup returns `None`, the size is `4` (skip the follow-up payload). Engines populate `world.party_actor_slots: Vec<Option<u8>>` with the live party-to-actor-slot map.
- `0x3C` writes the immediate fade colour to scratchpad globals (`ticks == 0`) or schedules a per-frame ramp (`ticks > 0`). The world records the request in `world.pending_fade: Option<FadeRequest>` so engines can drain it each frame to drive the screen overlay.

## Sub-op coverage in `crates/engine-vm`

**61/61 dispatched** (every entry of the `FUN_801D362C` JT at `0x801CE868`). Some sub-ops have host-trait stubs that fall through to no-ops on the default `MoveHost` impl:

- The world wires the ones with natural state - `ext_compute_angle`, `ext_party_member_lookup`, `ext_fade_color`, `ext_query_flag_bank`, `ext_set_flag_bank`, `ext_clear_flag_bank`, `ext_scratchpad_*`, `ext_set_8007b9d8`.
- The remaining stubs (`ext_debug_world`, `ext_func56798`, `ext_midpoint_set`, `ext_func801d31b0`, `ext_emit_ot_packet`, `ext_world_struct_*`, `ext_17`, `ext_20`) carry pure rendering / opaque-PsyQ side-effects and are best overridden per engine.
