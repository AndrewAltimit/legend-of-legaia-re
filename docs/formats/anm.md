# ANM animation container

The keyframe animation container for **player characters and field actors** - the walk cycles, idle loops, and scripted motion the field engine plays back. Asset type `0x06` from the [asset-type dispatcher](asset-type.md).

It is not the only animation format on the disc, and picking the wrong one is the usual mistake. Battle monsters use a separate per-object keyframe stream inside the monster archive; see [monster animation](monster-animation.md). The party's *battle* poses come from the player battle files' own streams, not from here - see [character mesh](character-mesh.md).

ANM data reaches you from two distinct bundles: the [party locomotion bank](#disc-source---the-party-locomotion-bundle-prot-0874-1) and the [per-scene NPC/scene-actor bank](#disc-source---per-scene-anm-bundle).

Implementation: `crates/anm`; parser `legaia_asset::player_anm`.

## Layout

```
u32 count
u32 byte_offsets[count]    // each is a byte offset into the buffer
records[]                  // per-record bodies; offsets[i+1] - offsets[i] = record size
```

Each record begins with an 8-byte header:

```
u16 a              // varies (3..14 observed) - likely record kind / opcode
u16 b              // varies (0..40 observed) - likely frame count
u16 marker_1       // = 0x080C in every record observed
u16 marker_2       // = 0x0002 (78%) or 0x0004 (22%)
```

## Per-record body - animation opcode 6

For records consumed via animation opcode `0x06` (the bulk of retail ANM
data), the body after the header is a per-bone **keyframe table**, not
opcode bytecode. The per-frame interpreter is the canonical actor tick
`FUN_80021DF4` in `SCUS_942.54` (block at `0x80022ec4..0x80023040`),
which walks the table indexed by a bone count sourced from the actor's
mesh context. Layout:

```
+0..+8                      header (a, b, marker_1, marker_2)
+8..+(8 + 8*N)              per-bone OUTPUT slots - written by the tick
                             (8 bytes per bone: packed pos+rot deltas)
+(8 + 8*N)..+(8 + 32*N)     per-bone KEYFRAME data - read by the tick
                             (24 bytes per bone = 12 little-endian i16
                              shorts: src_pos.xyz, dst_pos.xyz,
                              src_rot.xyz, dst_rot.xyz)
```

Total record size for opcode-6 records is `8 + 32*N` bytes for `N` bones.
The tick reads the 12 shorts, multiplies the `(dst - src)` deltas by
`actor[+0x22]` (the per-actor interpolation factor - driven from the
field-VM frame counter), and writes the resulting 8 packed bytes back
into the OUTPUT slots.

`crates/anm` exposes the typed accessor `KeyframeReader` for this layout.
The bone count is supplied by the caller (the actor's mesh context owns
it at runtime); offline tooling can use `KeyframeReader::infer_bone_count`
to recover it from the record size when it fits the equation exactly.

## Public entry point - `play_anm_by_id`

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
| `0x01` | none - the ladder never tests `1` | Common stages only |
| `0x02` | shares with `0x06` at `0x80021E90..0x80021FA4` | Per-bone keyframe-style |
| `0x03` | `0x800226E8..0x800228A0` | CLUT-cell HSV integrate (`+0x90..+0x94` += `+0x96..+0x9A`), then `FUN_80019D50` |
| `0x04` | `0x80022CC8..0x80022EE4` | VRAM rect wrap-scroll |
| `0x05` | `0x80021FB4..0x800226D8` | Positional SFX emitter |
| `0x06` | `0x80021E90..0x80021FA4`, `0x80022F0C..0x80023040` | Keyframe interpolation - fully traced + ported |
| `0x07` | `0x80022C30..0x80022CB8` | Spline / curve-driven variant |

The ladder is read from the compares, not the decompiled C: `beq`/`bne`
against `+0x5A` at `0x80021E7C`/`0x80021E88` (`2`/`6`), `0x80021FAC` (`5`),
`0x800226E0` (`3`), `0x80022C28` (`7`), `0x80022CC0` (`4`) and `0x80022F04`
(`6`). The span `0x800228B8..0x80022B80` that an earlier table listed as the
`0x05` handler is the **default motion block**, the one arm `3` and `5`
*skip* (`beq` to `0x80022B80` at `0x800228A8` / `0x800228B0`): the rotation
banks `+0x24..+0x28 += +0x80..+0x84`, the position step, and the `+0x72` /
`+0x7A` / `+0x78` channels stepped by the rates at `+0x92` / `+0x94` /
`+0x90`. See `ghidra/scripts/funcs/80021df4.txt`.

The `crates/engine-vm` `DispatchByte` enum exposes those values as a typed
dispatch - `DispatchByte::from_byte(actor[+0x5A])` and
`DispatchByte::handled_natively()` for the cases the keyframe pose decoder
can drive on its own (currently only `Keyframe`).

The per-arm physics tick (the part that *isn't* per-record bytecode - i.e.
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
2. Calls `FUN_800204f8` (`a0 = actor`) - actor advance / move tick.
3. Loads `s6 = actor[+0x4C]` - the ANM record pointer.
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

## Per-actor anim state offsets

A pre-action / mid-action save pair (a quiet battle frame vs an
in-flight somersault strike) pins the per-actor anim state to a small
named region inside the `0x2D4`-byte battle actor record. Slot-0 actor
record base = `0x800EC9E8`; the slots continue at `+ 0x2D4` for each
subsequent slot.

| Offset | Length | Purpose |
|---|---|---|
| `+0x1D8` | 16 B | Per-actor anim-PC. Pre-anim is mostly zero with a sentinel `01 77` at `+0x1D7..+0x1D8`; mid-anim holds incrementing per-bone counters (e.g. `00 11 00 27 00 03 03 0F 0E 19 27 00`). |
| `+0x1F4` | 18 B | Per-frame anim flag accumulator. Pre-anim values are zero; mid-anim transitions to a stamped run of `0x11` bytes once the action engages. |
| `+0x234` | 16 B | 4-pointer anim dispatch table (4 × u32, all the same value). Pre-anim = `0x8015CC30`; mid-anim = `0x801621D0`. The pointer is bumped by `+0x55A0` bytes between the two captures - the loader has paged a different ANM record into a different position in the heap for the strike. |

The anim-record header at the post-anim dispatch pointer reads as a
24-byte control block:

```
+0x00  u32  len = 0x18
+0x04  u32  reserved
+0x08  u32  reserved
+0x0C  u32  field_C  (= 4 in the captured record - frame count?)
+0x10  u32  field_10 (= 5)
+0x14  u32  field_14 (= 0x00299307 - dispatch flags / first opcode word?)
+0x18  u32  field_18 (= 0x0140017E - first opcode block)
```

These offsets are codified as
[`engine_core::capture_observations::battle_action_animation`](../../crates/engine-core/src/capture_observations.rs)
and exercised by the disc-gated test
`battle_action_anim_pair_pins_dispatch_pointer_table_and_anim_pc_window`
in `crates/mednafen/tests/real_saves.rs`.

### Per-record consumer struct - SCUS-resident, kind-byte dispatch

The pointer stored at `actor[+0x234]` (and shadowed into the render
context at `+0x4C` via `FUN_80049348` line 213 -
`*(undefined4 *)(param_1 + 0x4c) = *(undefined4 *)(iVar6 + uVar4 * 4 + 0x234)`)
points at a **runtime per-record control struct**, not a bytecode
program. The staged-anim commit `FUN_8004AD80` (SCUS_942.54, see
`ghidra/scripts/funcs/8004ad80.txt`) tests its byte `+0x00` - the entry's
action **tag** - once per commit, not per frame (`0x8004BE30..0x8004BF4C`):

| tag | Behaviour |
|---|---|
| `0x02` | When the battle's first monster id `0x8007BD0C` (`gp[+0x9F4]`) is `0xB3` or `0xB5` (the Songi fights) and the committing actor's HP `+0x14C` is `0`, the tag is rewritten to `0x04` in place (and for `0xB3` the entry's `+0x56` is incremented), so the tag-`0x04` row runs next. |
| `0x04` | HP not `0`: `actor[+0x1DA] = actor[+0x1F2]` (the get-up) and `+0x1DC = 0`. HP `0` on a party seat: `+0x1DA = 7` and `+0x1DC = 0`. A dead monster stages nothing here; its death arm runs at the next commit. |
| `0x05` | `actor[+0x1DC] \|= 4` (return to idle at the natural end). |
| `0x07` | `actor[+0x1DA] = 8`. |
| `0x08` | `actor[+0x1DC] \|= 8` (the root-motion latch). |
| other | No tag-specific branch. |

The ladder is the chain a downed party member plays - knockdown, entry
`7`, entry `8` - described in
[battle.md](../subsystems/battle.md#the-commits-clip-tag-ladder). An
earlier reading of this table, taken from the decompiled C, called it a
per-frame consumer, read `gp[+0x9F4]` as a "global character action
byte" and `+0x14C` as an anim flag, and missed that only a party seat
takes entry `7` and that both tag-`0x04` arms clear `+0x1DC`; the
disassembly above is the correction.

So the "per-record dispatch jump table" the actor record's `+0x234`
slot points at is a **flat struct** consumed via field-offset reads,
not an instruction-pointer entry into a switch. The 24-byte header
section observed at the captured pointer (`u32 0x18, ..., u32 0x0140017E`
for the somersault strike) is part of this same struct - its leading
byte reads as kind `0x18` (`= 24`) which falls into the "other"
arm and means the somersault is a raw-playback record (no scripted
sub-state mutation in `FUN_8004AD80`; it's driven entirely by the
field reads below).

### Per-record struct fields

The consumers (`FUN_80047430`, `FUN_80049348`, `FUN_8004AD80`,
`FUN_80048310`, `FUN_80048A08`, `FUN_8004998C`, `FUN_800495C8`,
`FUN_80049858`) read these offsets within the struct pointed at by
`actor[+0x234]`:

| Offset | Type | Purpose |
|---|---|---|
| `+0x00` | `u8` kind | Kind byte (see table above). The captured `u32 0x18` header word is `kind=0x18` plus three padding bytes. |
| `+0x0E` | `i16` | Movement-scaling factor. `FUN_80047430` integrates per-frame translation as `(angle_lookup * +0x0E * frame_index) / frame_count` (where `frame_count` is the byte at `*(+0x88) + 1`). |
| `+0x14..+0x53` | 8 x 8 B | The action's **effect script**: per-frame visual-effect placement records walked by `FUN_801DEA50` (called from `FUN_80047430` with this struct as its block argument). Layout + spawn routing: [`monster-animation.md` § Effect-script records](monster-animation.md#effect-script-records-entry-0x140x53). |
| `+0x34` / `+0x38` | vec | Position vec A (copied into render-ctx `+0x14` / `+0x18`). |
| `+0x44` / `+0x48` | vec | Position vec B (copied into render-ctx `+0x24` / `+0x28`). |
| `+0x56` | `u16` | Sub-state counter; ticked during `0x02 -> 0x04` transition. |
| `+0x76` | `u8` | Flag byte. |
| `+0x77` | `u8` | Adjustment byte (added to a per-arm constant). |
| `+0x78` | `u8` | Per-frame multiplier. |
| `+0x84` | `u8` | Max-frame byte; the consumer stamps `actor[+0x21B] = +0x84` (previous-action sentinel) and `actor[+0x176] = +0x84 << 4` (frame-counter cap). |
| `+0x85` | `u8` | Loop-target frame index. When `frame_index == +0x86 - 1` and `actor[+0x21B] != 0`, the per-bone interpolator (`FUN_8004998C` line ~1077) sources the "next" frame from `*(+0x88) + 2 + +0x85 * bones * 9` instead of the linear `frame_index + 1` slot. |
| `+0x86` | `u8` | Loop-trigger frame index (the frame at which the loop-target lookup kicks in). |
| `+0x87` | `u8` | Special-effect ID; non-zero values are passed to `FUN_8004E13C`. |
| `+0x88` | `ptr` | Pointer to nested per-frame data array. See ["Nested per-frame data"](#nested-per-frame-data) below. |
| `+0x172` | `u16` | Counter slot. |
| `+0x176` | `u16` | Animation-frame counter. |
| `+0x1BA` | `u16` | Per-actor render flag set (copied to render-ctx `+0x7A`). |

`actor[+0x21D]` (NOT a consumer-struct field - it's a byte on the
actor record itself) is a per-actor LOD-step byte. `FUN_80049348`
reads it as `lod_step = 8 / max(actor[+0x21D], 1)` and uses the result
to skip child actors during the render pass. Observed values are
`0` / `2` / `4` / `8`, mapping to `lod_step = 8` / `4` / `2` / `1`.
The `crates/engine-vm` view onto this is `anim_vm::ActorAnimState`.

The `+0x234..+0x244` slot in the actor record is a 4-deep
**history queue** of dispatch pointers (so the renderer can blend
between the previous and current ANM record across a transition).
`FUN_80047430` lines 1080-1088 implement the back-shift: when a new
record activates, `actor[+0x234]` receives the new pointer and the
previous values shift down through `+0x238..+0x244`.

### Nested per-frame data

The buffer pointed at by `consumer[+0x88]` carries the per-frame bone
keyframes the renderer interpolates each tick. Layout (validated against
`FUN_8004AD80`, `FUN_80048A08`, `FUN_8004998C`, `FUN_80047430`):

```
+0x00  u8   bones_per_frame  (B)
+0x01  u8   frame_count      (N)
+0x02..+0x02 + N*B*9         N frames × B bones × 9-byte keyframe
```

- Header byte `+0` is the per-frame loop count: `FUN_80048A08` line 749
  reads `**(byte **)(consumer + 0x88)` as the inner render loop bound.
- Header byte `+1` is the frame count: `FUN_8004AD80` line 1367 reads
  `*(byte *)(*(+0x88) + 1)` and stamps `(byte+1 - 1) * 16` into the
  actor's frame-counter cap. `FUN_80047430` line 990 divides per-frame
  velocity by this byte to get the per-frame translation step, so
  `frame_count == 0` produces a runtime divide-by-zero.
- Frame stride is `B * 9` and the body starts at offset `+2`:
  `FUN_8004998C` line 1040 reads `pbVar15 = pbVar20 + frame_index * B *
  9 + 2`.

Each per-bone 9-byte block encodes six packed sign-extended 12-bit
signed values, laid out as two `[i16; 3]` vectors:

```text
byte[0] | (byte[2] & 0x0F) << 8   → vec_a.x
byte[1] | (byte[2] & 0xF0) << 4   → vec_a.y
byte[3] | (byte[5] & 0x0F) << 8   → vec_a.z
byte[4] | (byte[5] & 0xF0) << 4   → vec_b.x
byte[6] | (byte[8] & 0x0F) << 8   → vec_b.y
byte[7] | (byte[8] & 0xF0) << 4   → vec_b.z
```

The packing pairs adjacent low bytes (`[0]`/`[1]`, `[3]`/`[4]`, `[6]`/`[7]`)
with shared high-nibble bytes (`[2]`, `[5]`, `[8]`). For each unpacked
12-bit value, if bit 11 (`0x800`) is set, the consumer ORs `0xF000` to
sign-extend (`FUN_8004998C` lines 1055..1062). The two vectors are
treated as runtime angle / pose deltas; their renderer-side semantic
(rotation triplet, position delta, etc.) is lost in compilation but
the byte layout is exact.

#### Frame counter / sub-frame interpolation

The actor's `actor[+0x68]` field is a `u16` frame counter:

- bits `[4..15]` (high 12 bits): frame index (used to seek into the
  per-frame data above).
- bits `[0..3]` (low 4 bits): sub-frame interpolation factor `0..=15`.

When the sub-frame factor is non-zero, `FUN_8004998C` lerps each bone
component-wise toward the next frame using the formula
`dst = a + (b - a) * frac >> 4`. The "next" frame is one of:

- Frame `+0x85` (the loop target) if `frame_index == +0x86 - 1` and
  `actor[+0x21B] != 0`.
- Frame `frame_count - 1` if `frame_index == frame_count - 1` (terminal
  frame uses the buffer's own end-frame for clamping).
- `frame_index + 1` otherwise.

#### Engine-side accessors

`crates/engine-vm::anim_vm` exposes the layout as typed views:

- `OpaqueAnimRecord` wraps the consumer struct at `actor[+0x234]`.
- `NestedFrameData` wraps the buffer pointed at by `+0x88` and exposes
  `bones_per_frame` / `frame_count` / `frame(i)` / `bone(f, b)` /
  `interpolate(f, next, frac)`.
- `BoneFrame` carries the unpacked `vec_a` / `vec_b` triplets and
  round-trips through `from_9_bytes` / `to_9_bytes`.
- `ActorAnimState` exposes the actor-side `+0x21D` LOD step
  (`lod_step_factor`), the previous-action sentinel at `+0x21B`, the
  frame-counter cap at `+0x176`, and the frame counter at `+0x68`
  (with `frame_index` / `sub_frame_factor` extractors).

### What the engine port still needs

`crates/engine-vm/src/anim_vm.rs::Host::on_opaque_record` covers the
field-read interpretation. Engines pin a typed `OpaqueAnimRecord`
view onto the buffer at `actor[+0x234]`, walk the per-frame data via
`OpaqueAnimRecord::nested_data_ptr_raw` + `NestedFrameData::from_bytes`,
and lerp via `NestedFrameData::interpolate`.

The pre-action capture's dispatch pointer (`0x8015CC30`) and the
in-flight strike capture's pointer (`0x801621D0`) point at distinct
records that share this struct shape - the loader pages a different
ANM record into the heap when the action ID changes.

## Disc source - the party locomotion bundle (PROT 0874 §1)

The **party field-locomotion clip set** - the walk / run / idle animations
every field scene poses the resident party meshes with - ships as section 1
of the PROT `0874_befect_data` container (`parse_player_lzs(buf, 3)` →
descriptor 1 → LZS). The decoded 16 864-byte container is **byte-identical**
to the live runtime copy every party actor's `+0x4C` anim-record pointer
resolves into (pinned against the `v0_1_pre_battle_tetsu` town01 field
save). Layout: 23 records = three 7-record character banks (Vahn `0..=6`,
Noa `7..=13`, Gala `14..=20`, all 10-bone - matching the runtime-capped
`nobj = 10` party meshes) plus record 21 (3-bone × 30-frame savepoint
loop, pack slot 3) and record 22 (2-bone aux clip, pack slot 4).

Bank slot 1 is the standing **idle** clip (10 bones × 15 frames): in the
town01 field anchor all three party actors' live record pointers sit at
bank offset +1 (Vahn = record 1, Noa = 8, Gala = 15) and the savepoint's
at record 21. Frame 0 of the idle clip is the character's field rest-pose
assembly transform. Bank slot 0 is the **walk** clip: a live pad-driven
capture (`scripts/pcsx-redux/autorun_locomotion_clip_pin.lua` - hold a
D-pad direction from town01 free-roam and sample `player+0x4C` per field
tick) shows the pointer switch record 1 → record 0 while moving and back
on stop, in both walk directions (no separate turn clip fires). The
remaining bank slots (10 / 8 / 8 / 4 / 3-frame clips) are the
run / interaction family; their per-slot roles are not yet
capture-pinned. A longer sibling capture
(`scripts/pcsx-redux/autorun_locomotion_run_pin.lua` - 15 s of
continuous held-pad walking in town01 free-roam, plus direction +
Square / Circle chords) never moves the pointer off the walk record, so
none of them is reachable as a held-pad "run" there; the select byte at
`player+0x5C` reads only `1` (walking, record 0) / `2` (idle, record 1)
across the capture - one higher than the record index, the same
`id = record + 1` shape as the battle bank's display ids. Whatever
triggers the remaining clips, it is not plain field locomotion, so they
keep neutral labels.

The consuming actor's object-pointer table at `actor[+0x44]`
(`[u32 count][ptr × count]`, built by `FUN_80024D78` straight off the pool
TMD: `count = *(tmd+8)`, object `i` = `tmd + 0xC + i*0x1C`) must have
`count` equal to the record's bone count (`FUN_8001B964` skips the draw
otherwise), so bone `i` drives TMD object `i` one-to-one. For the party
meshes that count is the **runtime-capped 10** (the disc pack ships
`nobj = 12`; groups 10/11 are the equipment-swap templates and are never
rendered - see [`character-mesh.md`](character-mesh.md)).

The bank's walk clip is the **town** walk only. On a kingdom world map the
pad step stores the scene-sentinel base `99`, so the overworld walk binds
body `leader` of the kingdom's own ANM bank instead (three 10-part clips,
byte-identical across the kingdoms - see
[`world-map-overlay.md`](world-map-overlay.md#per-kingdom-clip-inventory));
the standing idle there is still this bank's slot 1.

Parser: `legaia_asset::character_pack::field_locomotion_anm` (+ the
`LOCOMOTION_*` bank constants). Display labels follow the same pinning
discipline: `locomotion_slot_label` names only the two capture-pinned
slots ("Walk" / "Idle") and keeps the unpinned family neutral
("Locomotion N"); `locomotion_record_label` adds the character banks and
the savepoint / aux records.

## Disc source - per-scene ANM bundle

The scene-actor ANM pool - the clip set the town's **NPC actors** (and
scripted scene animations) play - ships **inside each scene's first asset
bundle** (not as a dedicated PROT entry). In the town01 field anchor,
every NPC actor's `+0x4C` points into this bundle's runtime copy at
`DAT_8007B7C8` (villager idles at records 14 / 17, children at 10 / 12,
etc.), while the party actors point into the PROT 0874 §1 locomotion
container above. **Which record an NPC plays comes from its MAN placement
header**: the record's `anim_id` byte = bundle record index + 1 (`0` = no
clip), installed into the actor `+0x5C` halfword at spawn - see
[`subsystems/script-vm.md`](../subsystems/script-vm.md#placement-header-model--animation-resolution).
The header only seeds the word: the record's spawn prologue runs before the
first drawn frame and can rewrite it. Every save crystal (`model 0xF3`) ships
header anim `0` and its prologue sets `22` - the locomotion bundle's savepoint
clip, record 21 - which a retail `conc_field_card_boot` capture shows at the
crystal actor's `+0x5C` with draw kind `1`. Both play hosts therefore pose a
placement from the live word (`World::field_npc_live_anim`) and fall back to
the header byte only when no channel owns the slot. A placement whose live
word is still `0` when the host binds it takes its first clip from a later
ANIMATE cue (`rikuroa`'s party Noa, posed by a cutscene's `A2 10 18`), so
both hosts keep every drawn placement as a clip target and cut its mesh to
the cue's clip bone count then; a host that dropped the cue drew the raw
TMD, every part at the actor origin. The bank a clip id names - at the
spawn binding as at every cue - is the actor's live party-bank bit
(`World::npc_clip_party_bank`), not its spawn class. The crystal's mesh is the
player bank's slot 3 (`0xF3 - 0xF0`), the three-object PROT 0874 §0 member,
which the engine keeps in `World::field_head_pool` apart from the shared
global pool whose slots `3..=32` it fills with the battle effect-model
library; the same capture shows the crystal actor at `+0x64 = 3` and the four
torch-like aux props at `+0x64 = 4`, `+0x5C = 23`. The bundle is a
[`parse_player_lzs`](../../crates/asset/src/lib.rs)-shaped container; section
2 (the third descriptor) is tagged **type byte `0x05`** in the dispatcher
table (labeled "MOVE" in `AssetType`, see [`docs/formats/asset-type.md`
](asset-type.md)) but the actual content LZS-decodes to a canonical ANM
container with `marker_1 = 0x080C` records.

A scene whose MAN arrives in a DATA_FIELD stream rather than a bundle can
ship its clip bank the same way: as the stream's type-`0x05` chunk, stored
raw (the stream walker hands every chunk to the dispatcher with
`copy_only = 1`). `rikuroa` is the case - its 72-record bank is the
type-`0x05` chunk of PROT 157, between the MAN (type `0x03`) and the
type-`0x07` chunk, and no entry of its block holds a container with an ANM
section. A retail `rikuroa` capture holds the same table at the scene-bank
pointer `FUN_800204F8` reads for a clip id below `0x400` with the party-bank
bit clear (`_DAT_8007B888`; the party bank is `_DAT_8007B75C`, ids
`>= 0x400` `_DAT_8007B840`). One resolver serves every host
(`legaia_asset::player_anm::find_scene_bundle`, through
`engine-core::npc_catalog::scene_anm_bundle`): the container sections
first, entry-major, then a stream chunk. Without the stream form every
`rikuroa` clip id went unresolved and its multi-object actors were
withheld from the catalog.

The mismatch between the asset type byte (`0x05` = "MOVE") and the
`ghidra/scripts/funcs/8001f05c.txt` (which
allocates `_DAT_8007B7C8` with the `anm_malloc_err` string and labeled
**ANM** dispatch) is a documented quirk; the runtime case selector indexes
asset bytes differently than the [`AssetType`] enum's display label
suggests.

Confirmed corpus (byte-equality against live `DAT_8007B7C8` in the
[`v0_1_pre_battle_tetsu`](../../scripts/scenarios.toml) field-mode save
state, mc7):

| PROT entry | CDNAME      | Section | Records | Decoded bytes  |
|------------|-------------|---------|---------|----------------|
| `0004`     | town01      | 2       | 69      | 96 448         |
| `0013`     | town0b      | 2       | 69      | 91 784         |
| `0183`     | balden      | 2       | 72      | 71 604         |
| `0408`     | bubu1       | 2       | 70      | 87 844         |
| `1203`     | other5      | 2       | 30      | 87 684 (battle form) |

The field-form bundles all have 69-72 records (the full player-locomotion
+ interaction anim set). PROT `1203_other5` is the battle-form player
animation set **for the PROT 1204 pack's own object order** (its banks'
bone counts match the player skeletons, bone `i` driving 1204 object `i` -
the Baka Fighter / viewer configuration). It is **not** what a real battle
poses the [assembled battle meshes](character-mesh.md#battle-form---assembled-from-the-player-files)
with: 1204's object order differs from the assembled blob's sorted bone-tag
order per character, and the in-battle pose source is the character's own
TRS streams in `record[0]` of their player file
([`battle-data-pack.md` § Battle animations](battle-data-pack.md#battle-animations-record0);
no 1203 record is resident in a mid-battle capture). Other scenes either share
an ANM blob with one of these via runtime caching, or have a smaller per-scene
player-ANM section.

### PROT 1203 bank layout - record roles

The 30 records lay out as three 9-record character banks (Vahn `0..=8`
15-bone, Noa `9..=17` 16-bone, Gala `18..=26` 15-bone - bone counts
matching the PROT 1204 `nobj`s) plus three 10-bone-rig records `27..=29`
(actor untriaged). The per-slot roles are pinned through the Baka Fighter
duel's display-anim ids: the combat tick stores `base + k` at
`actor + 0x5c` (`base = fighter_id * 9`) and the anim player resolves the
id **through the ANM container header** - `container + id*4` reads word
`id`, where word 0 is the record *count* and word `d` is `offsets[d-1]`,
so display id `base + k` lands on bank **record `k - 1`**. That fixes the
record space as: `0` idle (display `base+1`), `1..=3` the three attacks,
`4` the special, `5..=7` the hit / knockdown family, `8` the win flourish
(display `base+9`, the match-result / tally pose). Full derivation +
disasm cites: `legaia_asset::baka_opponents::action_slot_label`
(`party_bank_record_label` for the absolute-record view) and
[`minigame-baka-fighter.md` § Player input + actions](../subsystems/minigame-baka-fighter.md#player-input--actions).

Parser: `legaia_asset::player_anm` (CLI sweep + per-entry detector).

### Per-record layout (the disc form)

The offsets in the offset table are **absolute byte offsets** into the
LZS-decoded buffer (matches the standard `legaia_anm::parse` convention).
Each record's first 8 bytes are the canonical `(a, b, marker_1, flag)`
header from `legaia_anm::RecordHeader`. The per-record body size obeys
exactly:

```text
    record_size = 16 + 8 * (a & 0xFF) * b
```

verified byte-exact across all **310 records** in the 5 pinned scenes (and
across every other scene's bundle the corpus sweep finds; `f(a,b) == size`
falls out 100%). The runtime layout (traced through
`ghidra/scripts/funcs/8001b964.txt` - the per-actor
animated character renderer):

```text
+0x00..+0x08    header (a, b, marker_1=0x080C, flag)
+0x08..+end-8  b frames; per frame:
                   (a & 0xFF) bones × 8 bytes
                 each 8-byte entry is one bone's TR for that frame
+end-8..+end    8 zero bytes (record-boundary padding to 16)
```

The body sits **immediately after the 8-byte header**, frame-major. The 8
extra bytes in the size formula are a zero-padding trailer (every record
in the corpus has the last 8 bytes set to zero). The runtime pointer
`actor[+0x4C]` points at the record's byte 0; the sanity check
`*pbVar6 == nobj` matches header byte 0 (`a & 0xFF`) against the TMD's
animated-object count.

- `a & 0xFF` = **bone count** (number of animated TMD objects in this
  clip). The high byte of `a` is a **step-scaling selector**: with bit 0
  set, the playback advancer scales its per-tick step to
  `(rate * 2 + div - 1) / div` instead of using `rate` verbatim. Clear for
  records 0..8 of every field-form bundle, set to `0x01` for records 9+ and
  for every record in the Baka Fighter bundle; scene bundles set it per clip
  with no index split (see [the frame blender](#the-frame-blender-two-entries-one-gate)).
  The same bit gates the sub-frame blend.
- `b` = **frame count** of this animation clip (3..60 across the corpus;
  longer clips like Vahn's run-loop have higher counts).
- `flag` = the scaled step's **divisor** in its low byte (`0x02` / `0x04` in
  the field corpus; `0x0201` / `0x0401` / `0x0402` in the Baka Fighter
  bundle).

Those three header fields are exactly what the playback advancer
`ghidra/scripts/funcs/800204f8.txt` reads: it takes the
bone count at clip `+0`, the scaling flag at clip `+1`, the **frame count** at
clip `+2` and the divisor at clip `+6`.

### Playback: the frame cursor

`FUN_800204F8` is the per-frame clip driver, called from the actor tick
`FUN_80021DF4`. It binds the clip (`actor+0x4C = pack_base + offsets[id]`,
rebinding whenever the requested id `actor+0x5C` differs from the bound id
`actor+0x5E`, which also resets the cursor and selects draw kind `1`), then
advances the **frame cursor** `actor+0x68`.

The cursor is in **1/16-frame units**: `FUN_8001B964` poses from
`frame = (i16)(actor+0x68) >> 4`, and the clip's last position is
`frame_count * 16 - 1`. The per-tick step is `actor+0x6A` (through the header
scaling above), and the mode word `actor+0x62` carries hold (`0x0002`), clamp
(`0x0008`, clear = loop), reverse (`0x0080`), an end latch (`0x0100`) the tick
sets on reaching either end, and a restart request (`0x0200`) the next tick
consumes. Scripts drive all of it through the field-VM ops `0x2B` / `0x2C` /
`0x2D` (set / clear / spin on a bit of `actor+0x62`), `0x4C` nibble-4 sub-1 (the
rate) and `0x4C` nibble-3 sub-5 / sub-6 (the two pose snaps); `0x22 <id>` is
SET_ANIM. This is what swings a town's house doors open -
see [`field-locomotion.md`](../subsystems/field-locomotion.md#the-door-swing-how-a-bind-script-drives-the-clip).
Engine port: `legaia_engine_core::field_env::PropAnim`.

The detector at `legaia_asset::player_anm::find_in_entry` validates the
size invariant on every record before declaring a bundle parsed; the
disc-gated regression `crates/asset/tests/player_anm_real.rs` pins this
byte-exact across the corpus.

### Per-(bone, frame) 8-byte encoding

Each entry decodes to a `(T, R)` transform via
`ghidra/scripts/funcs/8001be80.txt`:

```text
  byte 0   = low8(T0)
  byte 1   = low8(T1)
  byte 2   = (high4(T1) << 4) | high4(T0)     ; nibble-packed sign bits
  byte 3   = low8(T2)
  byte 4   = (unused)          | high4(T2)    ; T2's high nibble is the LOW
                                              ; nibble of byte 4 (`andi 0xf`
                                              ; at 0x8001BF38); byte 4's high
                                              ; nibble is never read
  byte 5   = u8 rot-X (left-shifted by 4 to make a 12-bit PSX angle)
  byte 6   = u8 rot-Y
  byte 7   = u8 rot-Z
```

- `T0..T2` are **signed 12-bit translation values** (sign-extend to i32
  via `if (v & 0x800) v |= 0xFFFFF000`). These hold the joint offset in
  actor-local space; the runtime pushes them through the GTE's `MVMVA`
  with the actor's rotation matrix, then loads the result into the GTE
  `TR` registers as the per-object world-space translation.
- The three u8 rotations build into the GTE rotation matrix in the order
  Z, Y, X (post-multiplication) via the PsyQ-shape rotation builders at
  `ghidra/scripts/funcs/8004638c.txt` /
  `ghidra/scripts/funcs/8004629c.txt` /
  `ghidra/scripts/funcs/800461a4.txt`. Each function
  reads from the global sin / cos tables at `DAT_80070A2C` /
  `DAT_8007122C` and composes a single-axis rotation into the current
  matrix.

Frame 0 of an idle animation is the rest-pose assembly transform - it
places each TMD object at its joint position with its rest-pose
orientation. For Vahn's field form (nobj=12, bone_count=10) the rest
pose decodes to a bilaterally symmetric humanoid with joint centroids
distributed where you'd expect torso / head / arms / legs.

**Which nibble carries `high4(T2)` is a place prose has already got wrong.**
The instruction is `lbu v0,0x4(t3); andi v0,v0,0xf; sll v0,v0,0x8` at
`0x8001BF30..0x8001BF3C` - unambiguously the **low** nibble. A "byte 4 high
nibble" phrasing has appeared elsewhere in this repo's docs; the code was
never wrong, because the disc-gated unit test
`bone_transform_decode_signed_12bit` (`crates/asset/src/player_anm.rs`, town01
record 17) pins the byte-exact decode and would have failed the moment anyone
"corrected" `bytes[4] & 0x0F` to match the prose. That is the test doing its
real job: containing a documentation error so it cannot reach the port.

### The frame blender: two entries, one gate

`FUN_8001BE80` is not a pure per-entry decoder. Called once per part from
`FUN_8001B964`'s loop (`a0` = actor, `a1` = the part's entry at the current
frame, `a2` = the bound clip `actor+0x4C`, `a3` = the part index), it reads
**two** entries and blends them (see `ghidra/scripts/funcs/8001be80.txt`):

- **The gate** is clip byte `+1` bit 0 - the high byte of `a` - tested at
  `0x8001BF70` for the translations and again at `0x8001C0EC` for the angles.
  Clear, the current entry is emitted as decoded. It is the **same bit** the
  clip tick `FUN_800204F8` tests to select its scaled step (`lbu v0,0x1(a1)` at
  `0x800205B4`, `a1` = `actor+0x4C` there too), so a clip either both scales
  its step and blends, or does neither.
- **The fraction** is the cursor's low nibble, `actor+0x68 & 0xF`
  (`0x8001BFFC`).
- **The next entry** (`0x8001BEAC..0x8001BF00`): while
  `(i16)actor+0x68 >> 4 < frame_count - 1` it is `entry + a2[0]*8`, the same
  part one frame later. On the last frame it is the entry itself when the
  clamp bit `actor+0x62 & 8` is set, and otherwise `*(actor+0x4C) + part*8 + 8`,
  the part's frame-0 entry past the 8-byte header, i.e. the loop wrap. The
  blend always runs toward frame + 1, including while the cursor counts down
  in reverse.
- **Translations** lerp as `cur + (((next - cur) * frac) >> 4)` on the
  sign-extended 16-bit values (`0x8001BFF0..0x8001C06C`; `sra`, so a negative
  delta floors), stored with `sh`.
- **Angles** go through the angle interpolator `FUN_8001D088` with `from` =
  the next entry's angle and `to` = the current one (`0x8001C100..0x8001C154`).
  The interpolator returns `(to + ((from - to) * frac >> 4)) & 0xFFF` after
  bringing the pair onto the short arc by two **sequential** guards
  (`0x8001D090..0x8001D0B8`): add a turn to `to` when `from - to >= 0x800`,
  then add a turn to `from` when the *updated* `to - from >= 0x800`. Because
  the second guard re-reads the bumped `to`, an input exactly half a turn apart
  fires both and they cancel, so that one case resolves forward.
- **The Euler-flip retry.** Before the three angle calls the blender zeroes
  `_DAT_8007BD28` (`0x8001C11C`); each call adds its unwrapped `|from - to|`
  to it and journals its unwrapped `(from, to)` pair into the 8-byte-stride
  slot table at `0x800891A8` (slot = axis). If the sum exceeds `0xC00`
  (`slti v1,v1,0xc01` at `0x8001C164`) the three angles are blended **again**
  from the journal, against the next frame's equivalent Euler triple
  `(x + 0x800, -(y + 0x800), z + 0x800)` (`0x8001C170..0x8001C1CC`) - the
  same Z-Y-X orientation, reached by a shorter total path when the three
  short arcs together run long. The counter and the journal are therefore
  part of the transform, not bookkeeping.

Results are written to scratchpad `0x1F8002C0` (`T` at `+0x0..0x6`, angles at
`+0x8..0xE`) before the GTE load. At `frac == 0` every lane returns the current
entry exactly - the flip retry included, since the interpolator's `to` term
survives it - so a whole-frame cursor poses exactly as the single-entry decode.

On the disc the gate is the common case: 2243 of the 3634 records the ANM
detector finds across every PROT entry carry it, spread over 78 entries, and
1560 of those use step divisor `4`. It is set per clip, not by record index -
327 of the 755 records at index `0..=8` carry it and 963 of those at `9+` do
not. `crates/asset/tests/player_anm_sampler_real.rs` prints the census and
checks every keyframe of every record against the single-entry decode.

**Port.** `legaia_asset::player_anm::BoneTransform::decode` is the entry-decode
half. `PlayerAnmBundle::sample_bone` / `sample_pose` is the two-frame sampler
(next-entry rule, gate, fraction), `blend_bone_transform` the blend including
the flip retry, and `lerp_angle_12` / `lerp_angle_12_journaled` the angle
interpolator, the latter returning the journal and counter outputs as values.
`PlayerAnmRecord::blends` reads the gate. Both hosts pose a placed prop through
one engine kernel, `legaia_engine_core::field_env::prop_bone_offsets`, keyed on
`PropAnim::pose_key` (the live cursor plus the clamp bit), and the NPC / player
clip player `FieldClipPlayer` blends a gated clip on the ticks that fall inside
a frame; both hosts memoise a posed mesh on `FieldClipPlayer::pose_key` rather
than on the frame index.

**Cadence: the gate also slows the clip.** The clip tick's step for a gated
clip is `(rate * 2 + div - 1) / div` (`0x800205C8..0x800205E0`), against the
plain `rate` for every other clip, and the result is multiplied by the frame
step `DAT_1F800393` (`0x80020654..0x80020690`). At the field rate `8` that is
`16 / 8 / 6 / 4 / 3` sixteenths for divisors `1 / 2 / 3 / 4 / 6`, so the
divisor-4 clips - the common gated shape - run at half the ungated speed, and
the blender fills the ticks in between. A live capture on the Rim Elm
free-roam state (`scripts/pcsx-redux/autorun_gated_clip_step.lua`, 600
vsyncs, 62 actors) matches the rule on every call it could check: 7796 of
7796 ungated calls, 1690 of 1690 gated divisor-2 calls and 3969 of 3969
gated divisor-4 calls, all at frame step `2` (calls that rebound the clip,
held, restarted or ran reversed are left out).

The port's clip player `FieldClipPlayer` walks the same cursor:
`field_anim::clip_step` picks the step from the record's gate and divisor,
the cursor wraps to `0` on the tick that reaches `frames * 16 - 1`, and
`field_anim::clip_end_ticks` is the bind-to-latch length the cutscene
timeline times a player clip's end-latch spin with (`FieldLocomotion::scene_clip_ticks`).
The prop path (`PropAnim::tick`) takes its step from the same function.
An ungated clip keeps the two ticks a frame it always had.

The decoder helper `legaia_asset::player_anm::BoneTransform::decode`
returns `(t_x, t_y, t_z, r_x, r_y, r_z)` directly; the WASM
[`LegaiaViewer::player_anm_record_pose_frames`](../../crates/web-viewer/src/lib.rs)
emits the per-frame absolute transforms in that shape for the site's
character viewer.

## Allocator preamble

When the dispatcher (`FUN_8001f05c` case 6) loads ANM data, the malloc'd
buffer at `_DAT_8007B7C8` carries a 16-byte allocator preamble before
the payload:

```
+0x00  back_ptr        (RAM ptr - usually base - 0xC or similar)
+0x04  forward_ptr     (RAM ptr to next allocation)
+0x08  forward_ptr_2   (RAM ptr - sometimes 0)
+0x0C  expanded_size   (u32 - payload byte length)
+0x10  -- payload starts here --
```

`crates/anm::peel_preamble` strips it; the on-disc form has no preamble.

## See also

- [Legaia TMD](tmd.md) - the mesh format these animations transform.
- [Monster animation](monster-animation.md) - the enemy-side battle keyframe stream.
- [`subsystems/actor-vm.md`](../subsystems/actor-vm.md) - the actor/sprite VM that plays these clips.
- [`subsystems/renderer.md`](../subsystems/renderer.md) - the TMD renderer that consumes the posed vertices.
