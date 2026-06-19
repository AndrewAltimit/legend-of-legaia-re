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

So this overlay-escape, despite being an indirect-jump-table dispatch on a bytecode-supplied operand, has **no out-of-bounds-jump path** - a relevant property given the move buffer is partly attacker-influenceable (the self-modifying sub-ops `0x04`/`0x1B`/`0x1E` below write into it). The clean-room port mirrors the guarded return with a `_ => default_arm()` catch-all for any sub-opcode `>= 0x3D`.

### Overlay residency

The same dispatcher resides in many overlays (town, world-map and its variants, dialog, cutscene) at the same RAM address; each overlay supplies its own JT contents. Each sub-handler returns the size in u16 units. Sub-handlers at `0x801D31B0` (per-scanline POLY_FT4 strip emitter; reused across dialog / cutscene / world-map / 0897 overlays), `0x801D32F8`, `0x801D3444`, `0x801D3748`, `0x801D52D0`, etc. are members of this table.

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

Sub-ops `0x06` / `0x07` are the bbox-vs-player gate variants:

- `0x06` skips a 7-u16 follow-up payload when the player is **outside** the canonicalised box `[xa..xb]×[za..zb]` (each scaled by `0x80` with a `0x40` half-cell margin);
- `0x07` skips when the player is **inside**.

### Midpoint-to-actor (`0x0E` / `0x12`) + player-relative predicates

Sub-ops `0x0E` / `0x12` share a "midpoint to actor world" idiom backed by `FUN_801E45BC`:

- `0x0E` is the all-operand form (size 11): `actor.world = op[5..7] + ((op[2..4] + op[8..10]) >> 1)` then the helper applies `actor[+0x50]` blend mode.
- `0x12` (size 8) is the slot-indexed variant: the `a` triple comes from `slot_table[actor[+0x86] & 0xFF]` instead of operand u16s, and only `op[2..4]` (offset) and `op[5..7]` (b) live in bytecode.

Other player-relative predicates:

- Sub-ops `0x36`/`0x37` are axis predicates against `0x8E - DAT_8007C348`: pass → continue (size 1), fail → skip 3-u16 follow-up (size 4).
- Sub-ops `0x38`/`0x39` are squared-distance gates between the move actor and the player (`_DAT_8007C364`); `0x38` continues when *outside* radius `op[2]`, `0x39` continues when *inside*.
- Sub-op `0x23` is the anim-bank lerp toward operand world coords using the scratchpad ramp ratio at `_DAT_1F800393` over `op[5]`, with the divide guarded against `op[5] == 0`.
- Sub-ops `0x13`/`0x14` query the fourth flag bank (`DAT_80085758`) and gate on the result with the same size-1-or-4 shape; `0x14` inverts the predicate.

### Self-modifying bytecode ops (`0x04` / `0x1B` / `0x1E`)

Three sub-ops mutate the move bytecode buffer in place - these are "self-modifying" with respect to the operand stream that follows:

- `0x04` writes `actor[+0x14..+0x18]` (world XYZ) into `buffer[state.pc + op[2] + 3..+6]` (3 u16 stores); subsequent ops that read those slots see the captured world snapshot.
- `0x1E` is read-modify-write on a single u16 - `buffer[state.pc + op[2] + 4] += op[3]`.
- `0x1B` is an in-bytecode copy loop - for `i in 0..op[4]`, `buffer[state.pc + op[3] + i + 5] = buffer[state.pc + op[2] + i + 5]`.

The base offset of 5 (versus 3 for `0x04`, 4 for `0x1E`) targets the operand region past the count word, so the bytes following `0x1B`'s instruction header are effectively an inline scratch buffer indexed by op[2]/op[3]. The `MoveHost::move_bytecode_{read,write}_u16` callbacks expose the actor's move buffer to these ops; the engine layer wires them to `actor[+0x48][word_off]`.

### HSV color ramps (`0x1F` / `0x20`)

Sub-ops `0x1F` / `0x20` are HSV-space ramps on a packed 24-bit RGB color stored in `actor[+0xa0..+0xa3]` (`0x1F`) or `actor[+0xa4..+0xa7]` (`0x20`). The packed `(R, G, B)` is decomposed (R = byte 0, G = byte 1, B = byte 2), converted RGB→HSV via the SCUS helper at `FUN_8001a78c` (H ∈ 0..0x167, S ∈ 0..255, V ∈ 0..255), then `op[2..4]` are added per channel (H wraps mod `0x168`, S/V clamp to 0..255), then HSV→RGB via `FUN_8001a8dc` (clamped to 0..0xF8 by `FUN_8001a6c8`) and re-packed.

The size-1 default-arm return is intentional - the operand stream `op[2..]` is also re-interpreted as outer opcode `0x1F` / `0x20` on the next dispatch (a bytecode-density trick: one HSV ramp instruction simultaneously seeds an `actor[+0x9E..+0xAE]` anim-block update). `crates/engine-vm` ships the clean-room `rgb_to_hsv` / `hsv_to_rgb` pair that mirrors the SCUS algorithms exactly.

### Fourth flag bank (shared with the field VM)

The fourth flag bank at `DAT_80085758` is shared between the move VM (sub-ops `0x13` / `0x14` predicate, `0x1C` / `0x1D` set / clear) and the field VM (high-byte default routes `0x5x` set / `0x6x` clear / `0x7x` test). `engine-core::World` exposes it as a single lazily-grown `system_flags: Vec<u8>` with MSB-first bit ordering (mirroring `FUN_8003CE08`'s `0x80 >> (idx & 7)`). The field VM's `idx` encoding `((opcode_byte & 0x8F) << 8) | operand_byte` ranges over `0..=0x87FF`, which is why the bank can't be a fixed-size 256-bit array.

### Player-relative cluster close-out (`0x3A` / `0x3B` / `0x3C`)

Sub-ops `0x3A`, `0x3B`, `0x3C` close out the player-relative cluster:

- `0x3A` writes the angle from the actor to the player (computed as `atan2(dz, dx)` quantised to PSX 12-bit angle units, 4096 = full circle) into `bytecode[state.pc + op[2] + 3]`. Engines wire `MoveHost::ext_compute_angle` to surface the player position; the world-side default reads `world.player_actor_slot`.
- `0x3B` looks up the position of party-member `op[2]` and writes the world-XYZ triple into `bytecode[state.pc + op[3] + 4..+6]`. Pre-clears the dst slots before the lookup so a no-table host still gets the zero-sentinel guarantee. When the lookup returns `None`, the size is `4` (skip the follow-up payload). Engines populate `world.party_actor_slots: Vec<Option<u8>>` with the live party-to-actor-slot map.
- `0x3C` writes the immediate fade colour to scratchpad globals (`ticks == 0`) or schedules a per-frame ramp (`ticks > 0`). The world records the request in `world.pending_fade: Option<FadeRequest>` so engines can drain it each frame to drive the screen overlay.

## Sub-op coverage in `crates/engine-vm`

**61/61 dispatched** (every entry of the `FUN_801D362C` JT at `0x801CE868`). Some sub-ops have host-trait stubs that fall through to no-ops on the default `MoveHost` impl:

- The world wires the ones with natural state - `ext_compute_angle`, `ext_party_member_lookup`, `ext_fade_color`, `ext_query_flag_bank`, `ext_set_flag_bank`, `ext_clear_flag_bank`, `ext_scratchpad_*`, `ext_set_8007b9d8`.
- The remaining stubs (`ext_debug_world`, `ext_func56798`, `ext_midpoint_set`, `ext_func801d31b0`, `ext_emit_ot_packet`, `ext_world_struct_*`, `ext_17`, `ext_20`) carry pure rendering / opaque-PsyQ side-effects and are best overridden per engine.
