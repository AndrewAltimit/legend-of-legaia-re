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
| `0x01` | (TBD) | Snap variant - pose-snap only |
| `0x02` | shares with `0x06` at `0x80021E90..` | Per-bone keyframe-style |
| `0x03` | `0x800226DC..` | Path / state-write variant |
| `0x04` | `0x80022CBC..0x80022EE4` | Damp / spring-decay variant |
| `0x05` | `0x800228B0..0x80022B80` | Path-alt - reads geometry from `actor[+0x80]` |
| `0x06` | `0x80021EA0..0x80021FA4` | Keyframe interpolation - fully traced + ported |
| `0x07` | `0x80022C24..0x80022CC0` | Spline / curve-driven variant |

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
program. The per-frame consumer is the ladder in `FUN_8004AD80`
(SCUS_942.54, `ghidra/scripts/funcs/8004ad80.txt` lines ~1363..1706),
which dispatches on the byte at offset `+0x00` of the struct:

| `kind` byte | Behaviour |
|---|---|
| `0x02` | Handshake: when the global character action byte `(unaff_gp + 0x9F4)` is `-0x4D` / `-0x4B` AND `actor[+0x14C] == 0`, advance to kind `0x04` and tick the per-record counter at `+0x56`. |
| `0x04` | Action engaged. If actor's anim-flag at `+0x14C` is zero, set `actor[+0x1DA] = 7` (next sub-state). Else copy `actor[+0x1F2]` into `+0x1DA`. |
| `0x05` | OR `actor[+0x1DC] |= 4` (set bit 2 of per-actor flag byte). |
| `0x07` | Set `actor[+0x1DA] = 8`. |
| `0x08` | OR `actor[+0x1DC] |= 8` (set bit 3 of per-actor flag byte). |
| other | No kind-specific branch; the record is consumed by the surrounding fields below. |

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
capture-pinned.

The consuming actor's object-pointer table at `actor[+0x44]`
(`[u32 count][ptr × count]`, built by `FUN_80024D78` straight off the pool
TMD: `count = *(tmd+8)`, object `i` = `tmd + 0xC + i*0x1C`) must have
`count` equal to the record's bone count (`FUN_8001B964` skips the draw
otherwise), so bone `i` drives TMD object `i` one-to-one. For the party
meshes that count is the **runtime-capped 10** (the disc pack ships
`nobj = 12`; groups 10/11 are the equipment-swap templates and are never
rendered - see [`character-mesh.md`](character-mesh.md)).

Parser: `legaia_asset::character_pack::field_locomotion_anm` (+ the
`LOCOMOTION_*` bank constants).

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
[`subsystems/script-vm.md`](../subsystems/script-vm.md#placement-header-model--animation-resolution). The bundle is a
[`parse_player_lzs`](../../crates/asset/src/lib.rs)-shaped container; section
2 (the third descriptor) is tagged **type byte `0x05`** in the dispatcher
table (labeled "MOVE" in `AssetType`, see [`docs/formats/asset-type.md`
](asset-type.md)) but the actual content LZS-decodes to a canonical ANM
container with `marker_1 = 0x080C` records.

The mismatch between the asset type byte (`0x05` = "MOVE") and the
[`FUN_8001f05c` case 6](../../ghidra/scripts/funcs/8001f05c.txt) (which
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

verified byte-exact across all **296 records** in the 5 pinned scenes (and
across every other scene's bundle the corpus sweep finds; `f(a,b) == size`
falls out 100%). The runtime layout (traced through
[`FUN_8001B964`](../../ghidra/scripts/funcs/8001b964.txt) - the per-actor
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
  for every record in the Baka Fighter bundle.
- `b` = **frame count** of this animation clip (3..60 across the corpus;
  longer clips like Vahn's run-loop have higher counts).
- `flag` = the scaled step's **divisor** in its low byte (`0x02` / `0x04` in
  the field corpus; `0x0201` / `0x0401` / `0x0402` in the Baka Fighter
  bundle).

Those three header fields are exactly what the playback advancer
[`FUN_800204F8`](../../ghidra/scripts/funcs/800204f8.txt) reads: it takes the
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
[`FUN_8001BE80`](../../ghidra/scripts/funcs/8001be80.txt):

```text
  byte 0   = low8(T0)
  byte 1   = low8(T1)
  byte 2   = (high4(T1) << 4) | high4(T0)     ; nibble-packed sign bits
  byte 3   = low8(T2)
  byte 4   = ??                | high4(T2)    ; high nibble unused
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
  [`FUN_8004638C`](../../ghidra/scripts/funcs/8004638c.txt) /
  [`FUN_8004629C`](../../ghidra/scripts/funcs/8004629c.txt) /
  [`FUN_800461A4`](../../ghidra/scripts/funcs/800461a4.txt). Each function
  reads from the global sin / cos tables at `DAT_80070A2C` /
  `DAT_8007122C` and composes a single-axis rotation into the current
  matrix.

Frame 0 of an idle animation is the rest-pose assembly transform - it
places each TMD object at its joint position with its rest-pose
orientation. For Vahn's field form (nobj=12, bone_count=10) the rest
pose decodes to a bilaterally symmetric humanoid with joint centroids
distributed where you'd expect torso / head / arms / legs.

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
