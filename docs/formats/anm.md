# ANM animation container

Asset type `0x06` from the [asset-type dispatcher](asset-type.md). Implementation: `crates/anm`.

## Layout

```
u32 count
u32 byte_offsets[count]    // each is a byte offset into the buffer
records[]                  // per-record bodies; offsets[i+1] - offsets[i] = record size
```

Each record begins with an 8-byte header:

```
u16 a              // varies (3..14 observed) — likely record kind / opcode
u16 b              // varies (0..40 observed) — likely frame count
u16 marker_1       // = 0x080C in every record observed
u16 marker_2       // = 0x0002 (78%) or 0x0004 (22%)
```

## Per-record body — animation opcode 6

For records consumed via animation opcode `0x06` (the bulk of retail ANM
data), the body after the header is a per-bone **keyframe table**, not
opcode bytecode. The per-frame interpreter is the canonical actor tick
`FUN_80021DF4` in `SCUS_942.54` (block at `0x80022ec4..0x80023040`),
which walks the table indexed by a bone count sourced from the actor's
mesh context. Layout:

```
+0..+8                      header (a, b, marker_1, marker_2)
+8..+(8 + 8*N)              per-bone OUTPUT slots — written by the tick
                             (8 bytes per bone: packed pos+rot deltas)
+(8 + 8*N)..+(8 + 32*N)     per-bone KEYFRAME data — read by the tick
                             (24 bytes per bone = 12 little-endian i16
                              shorts: src_pos.xyz, dst_pos.xyz,
                              src_rot.xyz, dst_rot.xyz)
```

Total record size for opcode-6 records is `8 + 32*N` bytes for `N` bones.
The tick reads the 12 shorts, multiplies the `(dst - src)` deltas by
`actor[+0x22]` (the per-actor interpolation factor — driven from the
field-VM frame counter), and writes the resulting 8 packed bytes back
into the OUTPUT slots.

`crates/anm` exposes the typed accessor `KeyframeReader` for this layout.
The bone count is supplied by the caller (the actor's mesh context owns
it at runtime); offline tooling can use `KeyframeReader::infer_bone_count`
to recover it from the record size when it fits the equation exactly.

## Public entry point — `play_anm_by_id`

`FUN_80024CFC` (`play_anm_by_id(id, actor, ?)` in SCUS) is the writer
that primes an actor for animation playback:

1. Calls `FUN_80020DE0` (actor allocator).
2. Reads the per-record offset from the ANM payload at `_DAT_8007B7C8 + (id*4) + 4`.
3. Stores `(anm_base + record_offset)` in `actor[+0x4C]` (the per-actor anim record pointer).
4. Writes `0xB` to `actor[+0x56]` (animation state byte) and `100` to `actor[+0x68]` (frame counter).

The actor tick `FUN_80021DF4` then reads `actor[+0x4C]` whenever
`actor[+0x5A]` is `2` or `6` and runs the keyframe interpolation pass
described above. Other animation opcodes (set in `actor[+0x5A]`) gate
different per-record body layouts; only opcode `6` is fully traced today.

## Connection to other systems

The [field/event script VM](../subsystems/script-vm.md) opcode `0x34`
sub-op 3 plays a 3D animation by indexing into an ANM container and
handing the entry to `func_0x800252EC`. That sibling path lands the same
`actor[+0x4C]` slot the actor tick consumes.

## Non-keyframe records

Records whose body size is not a multiple of 32 don't fit the opcode-6
keyframe layout. Two structural sub-classes:

| Body | Sub-class | Notes |
|---|---|---|
| 0 bytes | Empty / stub | Placeholder slot; the actor tick skips it |
| Not a multiple of 32 | Irregular body | Opcode-specific layout; interpreter unknown |

Use `anm scan-non-keyframe 'extracted/PROT/*.BIN' --histogram` to surface
these across the corpus. The subcommand silently skips non-ANM files (safe
to glob), and `--histogram` prints the top-8 byte distribution per record
to help fingerprint the layout.

## Dispatch byte at `actor[+0x5A]`

`FUN_80021DF4` ladders through `actor[+0x5A]` (`u16`) and routes to a
per-opcode handler block. Observed values:

| `actor[+0x5A]` | Handler block in `FUN_80021DF4` | Status |
|---|---|---|
| `0x01` | (TBD) | Snap variant — pose-snap only |
| `0x02` | shares with `0x06` at `0x80021E90..` | Per-bone keyframe-style |
| `0x03` | `0x800226DC..` | Path / state-write variant |
| `0x04` | `0x80022CBC..0x80022EE4` | Damp / spring-decay variant |
| `0x05` | `0x800228B0..0x80022B80` | Path-alt — reads geometry from `actor[+0x80]` |
| `0x06` | `0x80021EA0..0x80021FA4` | Keyframe interpolation — fully traced + ported |
| `0x07` | `0x80022C24..0x80022CC0` | Spline / curve-driven variant |

The `crates/engine-vm` `DispatchByte` enum exposes those values as a typed
dispatch — `DispatchByte::from_byte(actor[+0x5A])` and
`DispatchByte::handled_natively()` for the cases the keyframe pose decoder
can drive on its own (currently only `Keyframe`).

The per-arm physics tick (the part that *isn't* per-record bytecode — i.e.
position / velocity / acceleration math, the SFX emitter at dispatch `0x05`,
and the per-arm render submissions for `0x04` and `0x07`) is fully ported in
[`crates/engine-vm/src/actor_tick.rs`](../../crates/engine-vm/src/actor_tick.rs).
Cross-cutting effects surface via `TickEvent` so engines can fold them into
their own audio mixer / scene graph / move-VM driver. See
[the actor-VM doc](../subsystems/actor-vm.md#per-arm-physics-tick) for the
per-arm breakdown.

The per-frame interpreter for non-opcode-6 records is **partially
overlay-resident**.
`FUN_80024CFC` only primes the actor (`actor[+0x4C]` = record pointer,
`actor[+0x56] = 0xB`); a handler in the town overlay (`FUN_801DE840`,
overlay 0897) reads `actor[+0x4C]` at `801e260c` via a sub-dispatch table
at `0x801CEF88` (routes by `opcode & 0xF`, 16 entries):

1. Guard: reads `actor[+0x5C]`; skips the whole handler if ≤ 0.
2. Calls `FUN_800204f8` (`a0 = actor`) — actor advance / move tick.
3. Loads `s6 = actor[+0x4C]` — the ANM record pointer.
4. Calls `FUN_80056798` (BIOS vector `0xa0/0x2F`) while advancing the
   field VM PC by 2 in the delay slot.
5. The 40-byte body at `801e2630..801e2670` uses `s6` and the return value
   to complete the frame-selection logic; this segment is in the overlay dump
   but not extracted in the current function-coverage pass.

This path is gated on `actor[+0x5C]` and is distinct from the opcode-6
keyframe path in `FUN_80021DF4` (which gates on `actor[+0x5A] == 6`).

See `ghidra/scripts/funcs/overlay_0897_801de840.txt` line ~3389 for the
disassembly. The full handler body requires a targeted dump of
`0x801e2630..0x801e2670` within `overlay_0897.bin`.

## Allocator preamble

When the dispatcher (`FUN_8001f05c` case 6) loads ANM data, the malloc'd
buffer at `_DAT_8007B7C8` carries a 16-byte allocator preamble before
the payload:

```
+0x00  back_ptr        (RAM ptr — usually base - 0xC or similar)
+0x04  forward_ptr     (RAM ptr to next allocation)
+0x08  forward_ptr_2   (RAM ptr — sometimes 0)
+0x0C  expanded_size   (u32 — payload byte length)
+0x10  -- payload starts here --
```

`crates/anm::peel_preamble` strips it; the on-disc form has no preamble.
