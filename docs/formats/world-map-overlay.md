# Slot-4 records (the world-map scene's animation bank)

> Slot 4 of each kingdom bundle is the world-map scene's **actor
> animation bank** - an ordinary asset-type-`0x05` ("MOVE") ANM
> container, the same shape every field scene carries, not a
> world-map-specific format. Each body is one animation clip:
> `frame_count x part_count` 8-byte entries, each entry a **rigid
> transform** (three packed 12-bit signed translation components plus
> three 8-bit rotation angles) applied to one object of an actor's mesh.
> Container byte-verified against live RAM; consumer chain pinned end to
> end from the disassembly and confirmed on a live `map01` state - see
> [Per-entry semantic](#per-entry-semantic---one-rigid-transform-decoded)
> and [Consumer call sites](#consumer-call-sites).
>
> Two earlier readings of these bytes are **falsified**. The
> "world-map wireframe / coastline" one, and the "each 8-byte record is
> a GTE vertex `(i16 x, y, z, attr)` whose triangle topology lives in an
> unpinned cluster-A command stream" one: there is no slot-4 command
> stream, no slot-4 topology and no slot-4 geometry. The `attr` i16 that
> reading called render-unused is bytes 6-7 of the entry - the Y and Z
> **rotation angles**, read every frame.
>
> The geometry an animated actor poses is a `DAT_8007C018` pool TMD
> selected by the actor, and the bulk continent terrain comes from the
> kingdom slot-1 TMD pack routed through `FUN_80043390`'s overlay-mode
> dispatch table at `0x801F8968`. Neither is in slot 4. See
> [`subsystems/world-map.md`](../subsystems/world-map.md#top-view-bulk-terrain-render-path-overlay-replaced-per-prim-renderers).

## Contents

- [Container layout (confirmed)](#container-layout-confirmed)
  - [Per-entry semantic](#per-entry-semantic---one-rigid-transform-decoded)
- [Per-kingdom clip inventory](#per-kingdom-clip-inventory)
- [RAM layout (confirmed)](#ram-layout-confirmed)
- [Consumer call sites](#consumer-call-sites)
  - [Live `map01` actor list](#live-map01-actor-list-slot-4-in-use)
  - [Where `FUN_80043390` fits](#where-fun_80043390-fits)
  - [Cluster A internals](#cluster-a-internals)
  - [How slot-4 bytes reach cluster A](#how-slot-4-bytes-reach-cluster-a)
  - [Cross-kingdom hit-count comparison](#cross-kingdom-hit-count-comparison)
  - [Reproducing the capture](#reproducing-the-capture)
- [Falsified hypotheses](#falsified-hypotheses)
- [Current working hypothesis](#current-working-hypothesis)
- [Tooling](#tooling)
- [`DAT_8007C018` - global TMD pointer table](#dat_8007c018---global-tmd-pointer-table-the-actual-cluster-a-source)
  - [Live snapshot (settled field scene)](#live-snapshot-settled-field-scene)
  - [Disc-side source of `[0..4]`](#disc-side-source-of-04)
  - [Loader chain - resolved](#loader-chain---resolved)
  - [Live snapshot (Sebucus mid-warp)](#live-snapshot-sebucus-mid-warp)
  - [Implication for slot 4 - resolved](#implication-for-slot-4---resolved)
- [Open work](#open-work)
- [See also](#see-also)

Slot 4 of each world-map (kingdom) bundle decompresses to a fixed-size
buffer that the runtime loads verbatim into RAM. Three carriers:

| Bundle | PROT index | CDNAME label | Decoded size |
|---|---|---|---:|
| Drake | 0086 | `map01` | 32304 |
| Sebucus | 0245 | `map02` | 26964 |
| Karisto | 0392 | `map03` | 24444 |

Those are the **bundle** entries, not the `0085` / `0244` / `0391` this page
used to name. Each kingdom block runs `[.MAP 36 sectors] [v12 header 1]
[prescript 1..3] [bundle N]`, so the bundle is one entry past the prescript;
the older numbers came from scanning the prescript entry's pre-correction
134-sector window and finding the bundle's table at `0x1800` - the next
entry's offset 0. Constants: `legaia_asset::kingdom_bundle::BUNDLE_ENTRIES`.

The 7-asset bundle is the standard
[`scene_asset_table`](scene-bundles.md#scene_asset_table---count-prefixed-asset-bundle)
shape with type sequence `(1, 2, 3, 4, 5, 6, 7)`. Slot 4's type byte
is `0x05` per [asset-type](asset-type.md) - "MOVE" - and the
kingdom-bundle consumer reads it exactly the way every other scene's
type-`0x05` section is read: as the per-scene actor
[**ANM**](anm.md) bank. `asset player-anm extracted/PROT/0086_map01.BIN
--desc-count 7` reports the Drake bundle as one player-ANM bundle
(`count=15`, `record0 marker_1=0x080C`), and `town01`'s type-`0x05`
section has the same record shape, the same `0x080C` marker and the same
size law. There is no world-map-specific slot-4 format.

## Container layout (confirmed)

### Outer pack

```text
+0x00   u32  count                ; number of sub-bodies
+0x04   u32  byte_offsets[count]  ; absolute byte offset into the
                                  ; decoded payload (NOT word offsets,
                                  ; unlike the slot-1 TMD pack)
+offset bodies[count]             ; contiguous sub-bodies
```

Drake decodes to 32304 bytes with `count = 15`. First entry is
`0x40 = 4 + 4*15` (right after the header).

### Sub-body header (8 bytes) - the ANM clip header

```text
+0x00   u8   part_count       ; objects posed per frame   (was "count_a")
+0x01   u8   flags            ; bit 0 = sub-frame interpolation on
                              ;                            (was "flag_a")
+0x02   u16  frame_count      ; clip length in frames   (was "count_b" +
                              ;                          "flag_b" = its
                              ;                          always-zero high byte)
+0x04   u16  marker           ; 0x080C - the ANM record marker
+0x06   u16  rate             ; low byte = sub-frame divisor, 1 / 2 / 4
                              ;                            (was "kind")
```

Every field is read by name in the disassembly: `part_count` at
`0x8001BACC` (`lbu a0,0(fp)`) and `0x8001BEF4`, `flags` bit 0 at
`0x800205B4` and `0x8001BF70`, `frame_count` at `0x8001BEB0`
(`lhu v1,2(a2)`) and `0x800206E4`, `rate`'s low byte at `0x800205CC`
(`lbu v1,6(a1)`). `marker` is never read by the runtime - it is a
format tag the offline detectors key on.

### Body payload

```text
+0x08   entry[frame_count * part_count]   ; 8 bytes each, FRAME-major:
                                          ; entry(f, p) at
                                          ; +8 + (f*part_count + p)*8
+...    trailer (8 bytes)                 ; always 8 zero bytes
```

Total body size is always `8 + part_count * frame_count * 8 + 8`. The
math fits every body in all three kingdoms exactly, and every trailer is
eight zero bytes (47/47 bodies). Frame-major order is the indexing the
pose reader computes at `0x8001BAC0..0x8001BAEC`:
`frame = (i16)actor[+0x68] >> 4`, `entry0 = rec + 8 + frame*part_count*8`,
then `+8` per part.

### Per-entry semantic - one rigid transform (decoded)

Each 8-byte entry is the pose of **one object of the actor's mesh in one
frame**: a translation and a rotation, decoded by `FUN_8001BE80`
(`0x8001BE80..0x8001C200`, called from the animated-actor renderer
`FUN_8001B964` at `0x8001BB20`).

```text
byte 0   translation X, low 8 bits
byte 1   translation Y, low 8 bits
byte 2   bits 0-3 = X high nibble, bits 4-7 = Y high nibble
byte 3   translation Z, low 8 bits
byte 4   bits 0-3 = Z high nibble; bits 4-7 RESERVED (zero in every entry)
byte 5   rotation X   (PSX angle = byte << 4; 4096 = one turn)
byte 6   rotation Y
byte 7   rotation Z
```

The three translation components are **12-bit signed** (`-2048..2047`),
sign-extended by the `andi 0x800` / `ori 0xF000` pairs at
`0x8001BF44..0x8001BF6C`:

```text
8001BF04  lbu v1,2(t3)          ; byte 2
8001BF08  lbu a0,0(t3)          ; byte 0
8001BF0C  andi v0,v1,0xf        ; X high nibble
8001BF10  sll  v0,v0,8
8001BF14  or   a0,a0,v0         ; X = byte0 | (byte2 & 0x0F) << 8
8001BF1C  andi v1,v1,0xf0       ; Y high nibble
8001BF20  lbu  v0,1(t3)         ; byte 1
8001BF24  sll  v1,v1,4
8001BF28  or   a3,v0,v1         ; Y = byte1 | (byte2 & 0xF0) << 4
8001BF30  lbu  v0,4(t3)         ; byte 4
8001BF34  lbu  v1,3(t3)         ; byte 3
8001BF38  andi v0,v0,0xf        ; Z high nibble  (byte 4's HIGH nibble unread)
8001BF3C  sll  v0,v0,8
8001BF40  or   v1,v1,v0         ; Z = byte3 | (byte4 & 0x0F) << 8
```

The decoded `(X, Y, Z)` is written to the scratchpad vector at
`0x1F8002C0` and pushed through the GTE as `MVMVA`
(`0x4A480012` at `0x8001C0E0`) against the rotation matrix at
`0x1F8002D4` - i.e. it is an object-local **translation**, not a vertex.
The three angles are written to `0x1F8002C8/CA/CC` as `byte << 4`
(`0x8001C1D4..0x8001C1DC` on the non-interpolated path).

The same packing is what the *other* ANM family writes: the
`actor[+0x5A] == 6` keyframe interpolator inside the actor tick
`FUN_80021DF4` emits exactly these 8-byte entries at
`0x80022FC4..0x80023030` (`sb` of the low byte, then
`sra 8; andi 0xf` and `sra 4; andi 0xf0` recombined into the shared
nibble byte). Two producers, one entry layout.

**Sub-frame interpolation.** When header `flags & 1` is set, the
decoder also decodes the *next* frame's entry for the same part and
lerps: the next-entry pointer is `entry + part_count*8` for any frame
but the last (`0x8001BEF4`), and wraps to frame 0's entry for that part
(`0x8001BEE0`) unless `actor[+0x62] & 8` (hold-last), in which case it
repeats the current entry. The blend weight is the cursor's low nibble
(`actor[+0x68] & 0xF`), so `out = cur + ((next - cur) * frac) >> 4`
(`0x8001BFF0..0x8001C06C`). Angles interpolate through
`FUN_8001D088(next<<4, cur<<4, frac, axis)`, which handles wrap-around
and accumulates a total-delta guard in `_DAT_8007BD28`.

**The clip cursor.** `FUN_800204F8` advances `actor[+0x68]`, a
1/16-frame fixed-point cursor, by `_DAT_1F800393` (the per-tick frame
delta) times a step: `actor[+0x6A]` normally, or
`ceil(actor[+0x6A] * 2 / rate)` when `flags & 1`
(`0x800205C8..0x800205E0`). It wraps at `frame_count << 4`
(`0x800206E4..0x8002072C`), or clamps when `actor[+0x62] & 8`, and sets
`actor[+0x62] |= 0x100` on each wrap. `actor[+0x62] & 0x80` runs the
clip backwards; `& 2` freezes it; `& 0x200` requests a restart.

**Structural invariant.** The renderer refuses to draw when the clip's
`part_count` does not equal the mesh's object count:
`0x8001BAF0` (`bne v1,a0,0x8001BDCC`) compares `chain[0]` - the `nobj`
of the actor's `DAT_8007C018` pool TMD - against the header's
`part_count` and skips the entire draw on a mismatch. A live `map01`
state satisfies it for every animated actor (see
[Live map01 actor list](#live-map01-actor-list-slot-4-in-use)).

### `rate` (1/2/4) - the sub-frame divisor (decoded)

The header's `+0x06` field, previously called `kind` and read as a body
class tag, is the animation's **sub-frame divisor**. Its low byte is the
`rate` in `FUN_800204F8`'s step formula above, and it is only consulted
when `flags & 1` is set. That is why the two fields co-vary: in the
corpus `rate = 4` bodies always carry `flags = 1`, and the single
`rate = 2` body with `flags = 1` is the "exception" an earlier pass
recorded. `rate` values `1 / 2 / 4` are the only ones on the disc.

The body-level observations that reading produced still hold as data,
they just do not mean what they were labelled:

- the three leading `rate = 1` bodies (0, 1, 2) are **byte-identical
  across all three kingdoms** - three shared animation clips shipped in
  every kingdom bundle;
- a trailing cluster (Drake bodies 9-11, Sebucus/Karisto 12-14) is
  likewise byte-identical across all three, and other bodies are shared
  between adjacent kingdom *pairs* - the same clip reused by the same
  actor type on more than one map;
- degenerate bodies (`part_count = 1`, all-zero entries) are empty
  placeholder clips.

### The one genuinely unread field - byte 4's high nibble

The entry's only field the runtime never reads is the **high nibble of
byte 4**. It is `0` in every entry of every body in all three kingdom
slot-4 payloads and in `town01`'s type-`0x05` bundle (22 228 entries
checked), so it is structural padding, not dropped data.

The earlier "`attr` is a per-vertex non-coordinate the render path
cannot see" analysis was measuring the wrong thing: `attr` was bytes
6-7 read as one `i16`, i.e. the Y and Z **rotation angles**. They are
read every frame at `0x8001C0E4`/`0x8001C0E8`. The register-width
argument that made `attr` "unreachable by construction" was about
`FUN_80044c14`'s GTE `VZn` loads - a function that never sees these
bytes. The `corr(attr, x/y/z) ~ 0.1`, "135 distinct values", and
"varies smoothly across groups" observations are all consistent with a
rotation channel sampled per frame, which is what they were measuring.

## Per-kingdom clip inventory

Drake = 15 clips; Sebucus = 16; Karisto = 16. The clip id an actor plays
is 1-based into this table (`actor[+0x5C] = k + 1` selects body `k` -
see [Consumer call sites](#consumer-call-sites)). The leading three
clips (`rate = 1`, `part_count = 10`) are byte-identical across all three
kingdoms.

The X / Y / Z "span" columns this table used to carry are deleted, not
recomputed: they measured bytes 0-1 / 2-3 / 4-5 as three `i16`, a
partitioning that straddles the packed 12-bit translation fields (see
[Per-entry semantic](#per-entry-semantic---one-rigid-transform-decoded)),
so every figure in them - including the "body 13 reaches the full +-32K
world bounds" reading - was an artifact of the split. The real
translation range across all three kingdoms is `-541..384` units.

### Drake (`map01`, PROT 0086)

| Body | parts | frames | rate | flags | entries |
|---|---:|---:|---:|---:|---:|
| 0 | 10 | 20 | 1 | 0 | 200 |
| 1 | 10 | 20 | 1 | 0 | 200 |
| 2 | 10 | 30 | 1 | 0 | 300 |
| 3 | 2 | 30 | 2 | 0 | 60 |
| 4 | 2 | 20 | 2 | 0 | 40 |
| 5 | 10 | 30 | 2 | 0 | 300 |
| 6 | 10 | 26 | 2 | 0 | 260 |
| 7 | 10 | 30 | 2 | 0 | 300 |
| 8 | 10 | 3 | 2 | 0 | 30 |
| 9 | 12 | 30 | 2 | 0 | 360 |
| 10 | 12 | 30 | 2 | 0 | 360 |
| 11 | 12 | 10 | 2 | 0 | 120 |
| 12 | 10 | 120 | 2 | 0 | 1200 |
| 13 | 14 | 15 | 4 | 1 | 210 |
| 14 | 2 | 30 | 2 | 0 | 60 |

### Sebucus (`map02`, PROT 0245)

| Body | parts | frames | rate | flags |
|---|---|---|---|---|
| 0-3 | 10/10/10/2 | 20/20/30/30 | 1/1/1/2 | 0 |
| 4-7 | 10/10/10/10 | 30/26/30/3 | 2/2/2/2 | 0 |
| 8-11 | 11/11/1/1 | 30/15/30/15 | 4/4/4/4 | 1 |
| 12-15 | 12/12/12/10 | 30/30/10/30 | 2/2/2/2 | 0 |

### Karisto (`map03`, PROT 0392)

| Body | parts | frames | rate | flags |
|---|---|---|---|---|
| 0-3 | 10/10/10/1 | 20/20/30/15 | 1/1/1/2 | 0 |
| 4-7 | 14/14/11/11 | 15/15/30/15 | 4/4/4/4 | 1 |
| 8-11 | 1/1/10/10 | 15/30/5/15 | 4/4/2/4 | 1 |
| 12-15 | 12/12/12/10 | 30/30/10/30 | 2/2/2/2 | 0 |

## RAM layout (confirmed)

Slot 4 is loaded **verbatim into RAM** with zero per-byte diffs vs disc,
and its resident address is **`_DAT_8007B888`** - the asset dispatcher's
type-`0x05` buffer pointer, `malloc`ed per scene load, which is why it
varies per kingdom. The dispatcher's case-5 arm is a five-instruction
allocate-and-store:

```text
8001F38C  ori   s4,s4,0x10          ; asset-present bit for type 5
8001F394  addiu a1,s3,3
8001F398  srl   a1,a1,2
8001F39C  jal   FUN_80017888        ; malloc(0, round_up4(size))
8001F3A0  sll   a1,a1,2
8001F3A8  sw    v0,-0x4778(at)      ; _DAT_8007B888 = buffer
```

Bases pinned by byte-matching the disc-decoded payload against a
post-warp full-RAM dump (`scripts/pcsx-redux/locate_slot4_base.py`, all
bodies agreeing unanimously):

| Kingdom | bundle | `_DAT_8007B888` | end (excl.) | bytes | bodies matched |
|---|---|---|---|---|---|
| Drake   | `map01` / 0086 | `0x8011A624` | `0x80122454` | 32304 | 15/15 |
| Sebucus | `map02` / 0245 | `0x80119CE4` | `0x80120638` | 26964 | 16/16 |
| Karisto | `map03` / 0392 | `0x80108D84` | `0x8010ED00` | 24444 | 16/16 |

Body 0's entries start `0x40` past the base (after the 4-byte count and
15-16 x 4-byte offsets). No runtime fixup is applied. Because the base is
not constant, a probe that arms breakpoints on the slot-4 RAM window
**must locate the base for that kingdom first** - or simply read
`_DAT_8007B888`, which is the base by definition. In a `map01` field-run
PCSX-Redux state (`pcsxr-state extract`) `_DAT_8007B888` reads
`0x8011A624` and the 32304 bytes there are byte-identical to the
disc-decoded slot-4 payload, which is what settles the identity of that
address: it is the type-`0x05` buffer, not a coincidence of allocation
order.

The Drake load was originally verified by
`scripts/pcsx-redux/diff_slot4_ram_vs_disc.py` (every byte of all 15
bodies matches the LZS-decoded payload); the per-kingdom base table above
extends that with `autorun_dump_full_ram_hold.lua` (drives the warp, then
dumps post-warp RAM) + `locate_slot4_base.py` (the unanimous body-vote
search).

## Consumer call sites

The consumer is one SCUS chain, and it is an **animation** chain, not a
render one. Each link is a plain `jal` and each reads a field of the
record it was handed:

| Step | Function | What it does |
|---|---|---|
| install | `FUN_8001F05C` case 5 (`0x8001F38C`) | `_DAT_8007B888` = the LZS-decoded slot-4 buffer |
| select + clock | `FUN_800204F8` | resolves `actor[+0x5C]` to a clip and advances the cursor `actor[+0x68]` |
| draw | `FUN_8001B964` (`0x8001B964..0x8001BE7C`) | render mode `actor[+0x56] == 1`: computes the frame, walks the parts, calls `FUN_80043390` per object |
| pose | `FUN_8001BE80` | decodes one 8-byte entry into the GTE translation + rotation |

**Clip selection** (`0x80020534..0x80020598`). `FUN_800204F8` picks one
of three banks and indexes it 1-based:

```text
if      (actor[+0x10] & 0x01000000)  bank = _DAT_8007B75C   ; party bank
else if ((i16)actor[+0x5C] <  0x400) bank = _DAT_8007B888   ; type 0x05 - SLOT 4
else                                 bank = _DAT_8007B840   ; type 0x0B "MOVE2"
rec = bank + *(u32*)(bank + (actor[+0x5C] & 0x3FF) * 4);
actor[+0x4C] = rec;  actor[+0x68] = 0;  actor[+0x56] = 1;
```

The missing `+4` in `bank + id*4` is what makes the id 1-based: id `1`
reads `offsets[0]`. Verified on the live `map01` state below.

`_DAT_8007B888` has exactly **six** references in the whole image corpus
(SCUS + every extracted overlay): the case-5 store above, a reset in
`FUN_8002541C` (`0x800254E8`), the `FUN_800204F8` read (`0x8002055C`),
and three reads in the Baka Fighter overlay 0976
(`scripts/ghidra-analysis/find-gp-relative-refs.py --va 0x8007b888`).
**No world-map overlay reads it, and `FUN_80043390` never sees it.**

### Live `map01` actor list (slot 4 in use)

Walking the live actor list (`*_DAT_8007C354`, chained through `+0x00`)
in a `map01` field-run state gives 14 actors, and every one that plays an
animation resolves into the type-5 buffer through the rule above:

| `+0x5C` id | `+0x4C` | offset in slot 4 | body | header `parts` | `chain[0]` = pool `nobj` |
|---:|---|---:|---:|---:|---:|
| 5 | `0x8011BE64` | `0x1840` | 4 | 2 | 2 |
| 11 | `0x8011E714` | `0x40F0` | 10 | 12 | 12 |
| 14 | `0x80121BC4` | `0x75A0` | 13 | 14 | 14 |
| 15 | `0x80122264` | `0x7C40` | 14 | 2 | 2 |

Each `+0x4C` equals `base + offsets[id - 1]` exactly, and each clip's
`part_count` equals the `nobj` of the actor's `DAT_8007C018` pool TMD -
the invariant `0x8001BAF0` enforces. The other five drawn actors are the
placed landmarks (render mode `+0x56 == 5`), which carry no clip at all
(`+0x5C = 0`, `+0x4C = 0`). Three more are the party, animating out of
the `_DAT_8007B75C` bank (`0x801589E4`, 23 clips) under the
`actor[+0x10] & 0x01000000` arm.

### Where `FUN_80043390` fits

`FUN_80043390` **is** in the loop - `FUN_8001B964` calls it once per
posed object at `0x8001BC84` - but it receives a `DAT_8007C018` pool TMD
group pointer, never a slot-4 address. The sections below document that
dispatcher because it is the world map's prim renderer and because the
capture that first found slot-4 reads recorded return addresses inside
`FUN_8001B964`; they are **not** a description of how slot 4 is read.

> **Cluster-B provenance caveat.** The dump file `ghidra/scripts/funcs/80059de4.txt`
> is **mislabeled**: its function entry is `FUN_80059BD4`, which is a **generic
> VRAM `LoadImage` DMA**, not a slot-4-specific reader. The Exec-bp hits at
> `0x80059DE4` fall inside that DMA routine, so "cluster B" is the generic
> image-upload path incidentally touching the slot-4 RAM window, not a dedicated
> slot-4 consumer.

`FUN_80043390`'s code window contains GTE opcodes (`4A280030` = MVMVA,
`4B400006` = NCLIP, `4812C000` = SWC2/load) interleaved with `LW` reads
of its vertex pool - it is the GTE-driven 3D primitive emitter that
turns a TMD group into GP0 packets. The post-load RAM window for `map01`
holds a 75 KB **GP0-primitive pool** (records at `0x20`-byte stride with
command bytes like `0x7D` / `0x7F` for textured triangles) at
`_DAT_8007B8D0 - 0x12800` while the overlay is paged in. That pool is
where the *posed* world-map objects land; its inputs are pool TMDs, not
slot-4 bytes.

### Cluster A internals

> **Scope.** Everything from here to
> [Reproducing the capture](#reproducing-the-capture) documents
> `FUN_80043390`, the world map's **TMD primitive dispatcher**. It is
> the function the posed objects are drawn through, and it is worth
> having decoded, but it never receives a slot-4 pointer: its inputs are
> `DAT_8007C018` pool TMDs. Read "slot-4 records" in the paragraphs below
> as "TMD vertex-pool records"; where a capture's numbers are about the
> prim dispatcher rather than about slot 4, the surrounding text says so.

`FUN_80043390` (712 bytes / 178 instructions; see
`ghidra/scripts/funcs/80043390.txt`) is the world-map primitive
renderer. It takes three arguments:

```c
void FUN_80043390(struct *display_state, u32 cmd_flags, u32 fade_flags);
//   display_state[0]   -> vertex pool base  (a2 in handlers = param_3)
//   display_state[3]   -> non-zero gates the color/light-modulation path
//   display_state[4]   -> command-stream pointer (a TMD group's prim section)
```

The function reads one command word from the stream
(`*display_state[4]`), extracts a 15-bit `kind` from bits 17-31 and a
16-bit `count` from bits 0-15, optionally re-arms the GTE colour
registers, and **tail-calls a per-kind handler** through a jump table.
Each handler consumes its own command's primitive batch (count items of
a kind-specific stride), emits GP0 packets into the active primitive
pool at `_DAT_8007BB04`, then **chain-calls the next kind handler at
the same dispatch point** - the renderer is a TMD-style display-list
walker, not a fixed-size record loop.

#### Jump tables

Two parallel handler tables drive the dispatch:

| Table | Address | When used |
|---|---|---|
| SCUS handlers | `0x8007657C` | always - the default world-map / overlay-resident render |
| Overlay handlers | `0x801F8968` | when `_DAT_1F800394 & 1` is set - the alternate route for the bulk-terrain pipeline (see `world-map.md`) |

Within the SCUS table the dispatcher adds a **bank offset** to the
`kind*4` index based on the caller's `cmd_flags` (`param_2`) and
`fade_flags` (`param_3`) arguments. The selection is the literal
disassembly from
`ghidra/scripts/funcs/80043390.txt`
(lines 230-244):

```c
_DAT_1f800028 = 0;
if (fade_flags != 0) {
    _DAT_1f800028 = 0x50;
    if ((cmd_flags & 0x04000000) != 0) _DAT_1f800028 = 0xA0;
    if ((cmd_flags & 0x20000000) != 0) _DAT_1f800028 = 0xF0;
}
```

So there are **four banks**, not three; the two `if`s are sequential
(not else-if), so the `0x20000000` branch wins when both flags are
set. And bank 0 / bank 1 are gated by `fade_flags`, not by
`cmd_flags` bits.

| `fade_flags` | `cmd_flags` bits | Bank offset | Effect |
|---|---|---:|---|
| `== 0` | (ignored) | `0x00` | bank 0 - `kind ∈ [12..19]` use the small `0x80043658..0x80043F10` handler set |
| `!= 0` | neither `0x04000000` nor `0x20000000` | `0x50` | bank 1 - `kind 12..19` swap to the `0x800448B0..0x80045584` set |
| `!= 0` | `0x04000000` set, `0x20000000` clear | `0xA0` | bank 2 - `kind 12..17` same as bank 1; `kind 18` / `19` swap to `0x800457C4` / `0x80045988` |
| `!= 0` | `0x20000000` set | `0xF0` | bank 3 - subtractive blend; `kind 19` swaps to `0x80045BB4`. Never observed in retail world-map render |

`kind ∈ [0..7]` and `kind ≥ 20` are NULL slots in every bank -
encountering them ends the primitive stream. `kind ∈ [8..11]` is
shared across all banks; only `kind ∈ [12..19]` swaps handler per bank.

#### Banks exercised in retail world-map play

Empirically (Drake post-warp settled, 19,935 dispatcher-entry
hits captured via
[`autorun_slot4_dispatcher_args.lua`](../../scripts/pcsx-redux/autorun_slot4_dispatcher_args.lua)):

| Bank | Drake hits | % | cmd_flags values seen |
|---|---:|---:|---|
| `0x00` (no fade) | 15,257 | 77% | `0x00D0D0D0`, `0xC9000000`, `0x00808080`, …  |
| `0x50` (fade) | 4,678 | 23% | `0x40D0D0D0`, `0x40808080`, `0x50808080`, … |
| `0xA0` | 0 | 0% | (`0x04000000` mask never set) |
| `0xF0` | 0 | 0% | (`0x20000000` mask never set) |

The high cmd_flags bits `0x04000000` and `0x20000000` are **never set**
during retail Drake world-map gameplay; banks 2 and 3 are reachable in
the dispatcher but no caller passes the flags that select them. The
sole bank distinction is `fade_flags != 0` (bank 0 ↔ bank 1).

"Never selected in this capture" is not "not a real render mode": the four
banks are the PSX semi-transparency states, and bank 3 is **subtractive**
blend. See [`subsystems/renderer.md`](../subsystems/renderer.md) for the
per-bank alpha semantics, which is the authority on what a bank *means*; this
page only records which ones the world-map capture exercised.

#### Per-kind primitive types

Every handler has the same shape: read N command-stream words, transform
3-or-4 vertices through the GTE, write an `M`-byte GP0 packet at the
primitive-pool pointer (`_DAT_8007BB04`-shaped global, advanced by `M`
each emit). The strides give away the PSX primitive type:

| Kind | Bank 0 entry | Banks 1,2,3 entry | cmd stride | GP0 stride | Likely primitive |
|---:|---|---|---:|---:|---|
| 8 | `0x8004409c` (shared) | (shared) | 0x14 (20B) | 0x20 (32B) | `POLY_G4` (gouraud quad) |
| 9 | `0x8004423c` (shared) | (shared) | 0x18 (24B) | 0x28 (40B) | `POLY_GT4` (gouraud-textured quad) |
| 10 | `0x80044434` (shared) | (shared) | 0x18 (24B) | 0x28 (40B) | `POLY_GT4` variant |
| 11 | `0x800445b0` (shared) | (shared) | 0x1c (28B) | 0x34 (52B) | extended quad (extra per-vert data) |
| 12 | `0x80043658` | `0x800448b0` | 0x0c (12B) | 0x14 (20B) | `POLY_F3` (flat triangle) |
| 13 | `0x80043768` | `0x80044a3c` | 0x0c (12B) | 0x18 (24B) | `POLY_G3` / `POLY_FT3` (gouraud or textured tri) |
| 14 | `0x80043b58` | `0x80044fdc` | 0x14 (20B) | 0x1c (28B) | `POLY_FT3` (flat textured triangle) |
| 15 | `0x80043c6c` | `0x80045194` | 0x18 (24B) | 0x24 (36B) | `POLY_GT3` (gouraud-textured triangle) |
| 16 | `0x800438b8` | `0x80044c14` | 0x14 (20B) | 0x20 (32B) | `POLY_G4` |
| 17 | `0x800439e4` | `0x80044dc8` | 0x18 (24B) | 0x28 (40B) | `POLY_GT4` |
| 18 | `0x80043dd4` | `0x800453bc` (b1, b3) / `0x800457c4` (b2) | 0x1c (28B) | 0x28 (40B) (b1) / 0x20 (b2) | `POLY_GT4` extended (per-vertex tag word) |
| 19 | `0x80043f10` | `0x80045584` (b1) / `0x80045988` (b2) / `0x80045bb4` (b3) | 0x24 (36B) | 0x34 (52B) (b1) / 0x28 (b2) | `POLY_GT4` extended-plus (sub-poly) |

Decomp dumps for each handler live at
`ghidra/scripts/funcs/slot4_<kind>_<bank>_<addr>.txt`; the SCUS table is
at `ghidra/scripts/funcs/slot4_handler_table_scus_0x8007657C.txt`.
Each handler decodes the per-command words as two packed vertex indices
per `u32` (low-16 `& 0x7FF8`, high-16 also `& 0x7FF8` - a `>>3` divisor
plus 8-byte vertex stride from `param_3` = the vertex pool base).

##### Sweeping the render path - what the range must cover

Decoding all four banks of `0x8007657C` yields **23 distinct handler entries**,
and a sweep scoped to the contiguous span `0x80043658..0x80045988` silently
misses two disjoint pieces of the render path:

- **`0x80045BB4`** - bank-3 `kind 19`, whose body runs past the end of that
  span to roughly `0x80046488`. It is the only handler reachable solely
  through bank 3.
- **The eight overlay-resident replacements** in PROT 0901 at
  `0x801F7644..0x801F8690`, reached through the second dispatch table
  `0x801F8968` when `_DAT_1F800394 & 1` is set. These **replace** kinds 12..19
  while the world-map overlay is paged in, so they are the bulk-terrain render
  path - not an optional extra. They live in a different image at a different
  base and cannot be reached by extending any SCUS address range.

A sweep that means "the whole world-map render path" therefore has to cover
`SCUS 0x80043390..0x80046498` **and** the PROT 0901 image
(`0x801F69D8..0x801FA1D8`, base from
[`static-overlays.toml`](../../crates/asset/data/static-overlays.toml)). Note
also that ~6% of words in this family are COP2/GTE ops that a stock MIPS32
disassembler declines to decode; confirm such words are opcode `0x12` rather
than assuming a failed decode is a data word.

#### Mapping captured LW PCs to kinds

Each of the eight cluster-A LW PCs captured by
[`autorun_slot4_consumer_pcs.lua`](../../scripts/pcsx-redux/autorun_slot4_consumer_pcs.lua)
falls inside one of the bank-1 kind handlers (`0x800448B0..0x80045584`).
The probe doesn't have PCs inside bank 0 (`0x80043658..0x80043F10`) or
bank 2 (`0x800457C4..0x80045988`):

| LW PC | Handler | Kind | Bank |
|---|---|---:|---|
| `0x80044B00` | `0x80044A3C..0x80044C13` | 13 | bank 1 (= bank 2 for this kind) |
| `0x80044C70` | `0x80044C14..0x80044DC7` | 16 | bank 1 (= bank 2 for this kind) |
| `0x80044E08` | `0x80044DC8..0x80044FDB` | 17 | bank 1 (= bank 2 for this kind) |
| `0x80045418` | `0x80045194..0x800453BB` | 15 | bank 1 (= bank 2 for this kind) |
| `0x800455E4` | `0x800453BC..0x80045583` | 18 | bank 1 only (bank 2 uses `0x800457C4`) |
| `0x800455E8` | `0x800453BC..0x80045583` | 18 | bank 1 only |
| `0x8004561C` | `0x800453BC..0x80045583` | 18 | bank 1 only |
| `0x80045658` | `0x800453BC..0x80045583` | 18 | bank 1 only |

The Drake-tuned probe's per-PC labels (`A_lw_count_word`,
`A_lw_body0_offset`, etc.) describe **what RAM region the LW happened
to touch** at the moment of the first hit, not the role of the field
in the underlying handler. Those reads are the handler's normal
load-vertex-from-pool operation over a TMD; the labels' "body 0 offset"
framing is a leftover of the falsified slot-4-vertex-pool reading and
names nothing in slot 4.

[`autorun_slot4_dispatcher_args.lua`](../../scripts/pcsx-redux/autorun_slot4_dispatcher_args.lua)
captures the *dispatcher prologue* (`0x80043390`) directly - `a0`,
`a1` (cmd_flags), `a2` (fade_flags), and the first command word's
kind / count fields *before* the handlers clobber the registers. Use
that probe to characterise per-call dispatch behaviour; use
`autorun_slot4_consumer_pcs.lua` to count handler-level work (number
of primitives emitted per kind).

### How slot-4 bytes reach cluster A

**They don't.** No slot-4 address ever reaches `FUN_80043390`. The
type-`0x05` buffer pointer `_DAT_8007B888` has six references across
SCUS and every extracted overlay and none of them is in a render path
(see [Consumer call sites](#consumer-call-sites)); what
`FUN_80043390` receives is always a `DAT_8007C018` TMD group pointer.
The section title is kept because other pages link to this anchor.

The cluster-A input pointer originates from `DAT_8007C018` (the global
asset-pointer table - see [reference/memory-map](../reference/memory-map.md)).
Two parallel call paths funnel into the same dispatcher:

1. **Top-view dispatcher (`FUN_801F69D8`)**: reads
   `DAT_8007C018[(visible_object_kind8 + DAT_8007B6F8) * 4]` per tile
   and passes `entry + 0xC` (the TMD's group-descriptor array start)
   to `FUN_80043390`. This is the warp-into-world-map render path the
   Read-bp probe captured.
2. **Per-actor renderer (`FUN_8001ada4`, caller RA `0x8001B47C`)**:
   walks `actor+0x44 = [u32 count, u32 mesh_ptr[count]]` and passes
   each `mesh_ptr` to `FUN_80043390`. The mesh pointers came from
   `actor+0x44`, which is populated by `FUN_80021B04`/`FUN_80024D78`
   from `DAT_8007C018[actor[+0x64].i16]` - same table, different
   actor-allocator path.

This `DAT_8007C018` table serves the *character-mesh* and per-tile
world-map TMD objects, and it is the **only** thing the dispatcher is
handed. The kingdom slot-4 payload is read in place at `_DAT_8007B888`
by the animation chain instead - see
[Slot-4 is read in place](#slot-4-is-read-in-place---there-is-no-transcode-drake-capture).

The two paths meet only at the actor: `FUN_8001B964` reads the clip out
of `_DAT_8007B888` to build the object's matrix, then hands that
object's `DAT_8007C018` TMD group to `FUN_80043390` to draw.

The clip header's `rate` field (the old `kind ∈ {1, 2, 4}`) has **no
link** to the cluster-A bank selector (see
[Banks exercised in retail world-map play](#banks-exercised-in-retail-world-map-play));
the bank is chosen by the per-call `fade_flags` / `cmd_flags` args,
not by any slot-4 field.

See [reference/memory-map](../reference/memory-map.md#world-map-tmd-and-actor-tables)
for the `DAT_8007C018` snapshot breakdown and the kingdom-TMD prefix
counter `DAT_8007B6F8`.

### Cross-kingdom hit-count comparison

These counts measure **prim-dispatcher work per kingdom**, not slot-4
reads: the PCs are inside `FUN_80043390`'s kind handlers, whose vertex
pools are TMDs. They are kept because they characterise the world map's
per-kingdom render load, which is the question they actually answer.

Exec-breakpoint hit counts at the eight cluster-A LW PCs + the
cluster-B LW PC during a single warp-tile transition. All three
kingdoms captured with `LEGAIA_PC_CAP=50000` over 1800 vsyncs; no PC
saturates the cap, so the per-kingdom totals are exact:

| Kingdom | Capture state | Cluster A total | Cluster B | Cluster A RAs observed |
|---|---|---:|---:|---|
| Drake | already on map01, held UP | 71,331 | 178 | 0x8001B47C, 0x8001BC8C, 0x801F78D4 |
| Sebucus | town → map02, held DOWN | 90,096 | 67 | 0x8001B47C, 0x801F78D4 |
| Karisto | town → map03, held DOWN | 13,593 | 115 | 0x8001B47C, 0x801F78D4 |

Sebucus's cluster-A total is *higher* than Drake's despite Sebucus's
slot-4 being smaller - confirming hit-count tracks scene-render
volume, not slot-4 record count. Cluster B's variance is the inverse:
Drake walks the most slot-4 bodies, then Karisto, then Sebucus. The
per-kind breakdown ([Per-kind delta](#per-kind-delta) below) makes the
per-handler differences visible.

#### Per-kind delta

With the cluster-A LW PCs mapped to specific kind handlers (see
[Cluster A internals](#cluster-a-internals) above), the per-PC × per-
kingdom hit counts surface a clean signal. All three kingdoms
captured uncapped (`LEGAIA_PC_CAP=50000` over 1800 vsyncs):

| Kind handler | Primitive (likely) | Drake hits | Sebucus hits | Karisto hits |
|---:|---|---:|---:|---:|
| 13 banks 1,2 (`0x80044A3C`, LW `0x80044B00`) | `POLY_G3`/`POLY_FT3` triangle | **9,465** | **2,040** | 49 |
| 17 banks 1,2 (`0x80044DC8`, LW `0x80044E08`) | `POLY_GT4` textured quad | **762** | **240** | 147 |
| 18 bank 1 (`0x800453BC`, 4 LW PCs `0x800455E4..0x80045658`) | `POLY_GT4` extended quad | **13,561** (×4) | **20,601** (×4) | **1,820** (×4) |
| 16 banks 1,2 (`0x80044C14`, LW `0x80044C70`) | `POLY_G4` quad | **7,688** | **878** | **2,058** |
| 15 banks 1,2 (`0x80045194`, LW `0x80045418`) | `POLY_GT3` textured triangle | **6,860** | **5,412** | **2,059** |
| cluster B (`0x80059DE4`) | mid-body reader | **178** | **67** | **115** |

Cross-kingdom picture (now with all entries uncapped):

- **Kind 13** scales sharply: Drake (9,465) ≫ Sebucus (2,040) ≫
  Karisto (49). Drake / Sebucus have continental geometry with many
  small triangle primitives; Karisto barely uses them.
- **Kind 17** scales with overall scene weight: Drake (762) >
  Sebucus (240) > Karisto (147). Ratio Drake / Karisto ≈ 5.2.
- **Kind 16** is the inverse of kind 13: Karisto-heavy (2,058) /
  Drake-heavy (7,688) but Sebucus uses it least (878). Drake's quad
  count dwarfs the others.
- **Kind 18 (extended quad)** is the absolute workhorse - Sebucus
  dispatches **20,601 instances** of it (~80% of the cluster-A
  primitive count), Drake 13,561, Karisto 1,820. This is the dominant
  per-frame primitive across every kingdom.
- **Cluster B** (the mid-body reader): Drake (178) > Karisto (115) >
  Sebucus (67) - Drake's larger slot 4 visits more of the secondary
  reader's body subset.

The captured CSVs land under
`captures/slot4_uncapped/` (per-row
flushed; safe to inspect mid-run). The dispatcher-entry probe CSV at
`captures/slot4_dispatcher/` gives
the first-kind / `cmd_flags` / `fade_flags` per call - see the bank-
breakdown table above.

### Reproducing the capture

The kingdom-agnostic consumer probe is
[`autorun_slot4_consumer_pcs.lua`](../../scripts/pcsx-redux/autorun_slot4_consumer_pcs.lua),
which arms Exec breakpoints at the cluster-A + cluster-B PCs and fires
identically across all three kingdoms (the Drake-tuned Read-breakpoint
`autorun_slot4_readers.lua` is archived under
`archive/pcsx-redux-probes/` - its offsets are Drake-specific and don't
generalise):

```bash
LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/<your-sebucus-warp-save>.sstate \
LEGAIA_HOLD_BUTTON=6 LEGAIA_HOLD=60 \
LEGAIA_FRAMES=1800 \
LEGAIA_PC_CAP=50000 \
LEGAIA_OUT=captures/slot4_uncapped/sebucus.csv \
LEGAIA_LUA=scripts/pcsx-redux/autorun_slot4_consumer_pcs.lua \
    timeout --kill-after=30s 1500s bash scripts/pcsx-redux/run_probe.sh
```

Each CSV row records `probe_idx, cluster, pc, name, ra, a0..a3, s8`
at the moment the Exec breakpoint fires - enough to cross-reference
caller RA + register state per hit when comparing kingdoms. A
`.detail.txt` sidecar carries the first-hit call-context for each PC
(32 GPRs, 16-word code window around PC, 32-word stack window at sp).

`pcsx-redux` in `-interpreter -debugger` mode does not reliably
self-terminate within a tractable wall-clock window even though
`probe.lua` calls `PCSX.quit(0)` after the capture window - the
PSX vsync timer is game-time, not wall-time, and interpreter overhead
with active breakpoints stretches the 1830-vsync wall-clock by an
order of magnitude. The `timeout --kill-after=30s 900s` wrapper above
forces a clean shutdown after 15 minutes; the CSV is flushed per row,
so the partial capture remains usable even after an explicit kill.

## Falsified hypotheses

Both readings below treated the 8 bytes as **geometry**. They are a
transform, so every projection of them is meaningless by construction -
which is why no projection ever matched anything.

1. **Top-down dev-menu wireframe / continent coastline.** That body 12
   traces a continent coastline, body 13 the world boundary frame, and
   the rest inner contours visible in the developer top-view. Projecting
   all bodies onto `xz` produces no recognizable map silhouette in any
   kingdom; PNG renders matched against the dev-menu top-view captured
   from PCSX-Redux save states found no agreement. Sub-variants tested
   and equally falsified: a `count_a x count_b` heightfield grid, and a
   heterogeneous set readable in some single non-`xz` projection (the
   "vertical pillars" that appear in `xy` for Drake bodies 9 and 11 are
   an artifact of plotting packed translation nibbles as an `i16`).

2. **A vertex pool indexed by an unpinned cluster-A command stream.**
   That each entry is a GTE vertex `(i16 x, y, z, attr)` loaded straight
   into `VXYn`/`VZn` by `FUN_80044c14`, with the triangle topology and
   per-object placement living in a separate command stream nobody had
   found. This one came from reading a prim handler that *does* work
   that way and assuming slot 4 was its pool. The stream was never found
   because it does not exist: `_DAT_8007B888` reaches no renderer
   (six references image-wide, none in a render path), the entry's real
   field boundaries fall on nibbles rather than `i16`s, and the byte
   pairs that reading called `attr` are the Y/Z rotation angles.

Both are recorded in
[`reference/re-do-not-re-walk.md`](../reference/re-do-not-re-walk.md).

## Current working hypothesis

None - the format is decoded, so this section records only what remains
*unlabelled* rather than unknown.

Every field of the container and of the 8-byte entry is pinned to an
instruction that reads it, the clip-id lookup is verified against a live
`map01` state, and the invariant tying a clip's `part_count` to its
mesh's `nobj` is enforced by the renderer. What is not pinned is
**which actor plays which clip**: the ids come from the scene's MAN
script (`actor[+0x5C]`), so naming Drake clip 11 "the windmill's spin"
rather than "the clip actor `+0x64 = 39` plays" needs a scene-script
pass, not more format work. The three leading clips being byte-identical
across all three kingdoms says they belong to an actor type every
kingdom has; it does not say which.

## Tooling

The wireframe-era tools still parse the container correctly; only their
*interpretation* of the entries was wrong, so they are kept for byte
inspection and round-trip work:

| Tool | Role |
|---|---|
| `cargo run -p legaia-asset --bin asset -- kingdom-slot <PROT>.BIN --slot 4` | Per-body inventory dump (parts / frames / rate / flags). `--out` writes the decoded payload. |
| `cargo run -p legaia-asset --bin asset -- player-anm <PROT>.BIN --desc-count 7` | The generic ANM detector - reports a kingdom bundle exactly the way it reports any scene's type-`0x05` section. |
| `legaia_asset::world_map_overlay::{parse, Slot4Body, Slot4Transform, translation_path_segments}` | Rust API. `Slot4Body::{part_count, frame_count, interpolates, subframe_divisor, entry}` expose the header; `Slot4Record::transform()` decodes one entry; `translation_path_segments` emits each part's translation path across the clip. |
| `legaia_asset::world_map_overlay::{top_down_lines, wireframe_segments_3d, record_points, body_axis_range}` | The wireframe-era projections. They plot raw `i16` field pairs that straddle the entry's real boundaries - byte-inspection aids only, never geometry. |
| `cargo run -p legaia-asset --bin asset -- slot4-png --input <PROT>.BIN --out <png>` | PNG renderer over those projections. `--style row\|col\|pairs\|grid\|points`, `--axes xz\|xy\|zy`, `--only-body N`, `--from-raw <bin>`. |
| `scripts/pcsx-redux/run_probe.sh --lua scripts/pcsx-redux/autorun_dump_slot4.lua` | PCSX-Redux closed-loop dumper: loads a save state, dumps the live slot-4 RAM region, quits. |
| `scripts/pcsx-redux/autorun_dump_full_ram.lua` | Full 2 MiB main RAM dump. Use when the load base is unknown for a new build / state - or just read `_DAT_8007B888`. |
| `scripts/pcsx-redux/diff_slot4_ram_vs_disc.py` | Byte-compare a RAM dump against the disc-decoded payload. |

The world-overview web viewer does not expose slot 4. The WASM exports
(`slot4_wireframe_lines` / `slot4_wireframe_points` /
`slot4_wireframe_bounds`) sit on the falsified projections and should be
retired or re-pointed at `translation_path_segments` before any page
draws them.

**Live-engine inspection overlay.** The from-scratch engine decodes slot 4
for every world-map scene onto `SceneResources::world_map_slot4` (resolved
only for `SceneLoadKind::WorldMap`; `None` everywhere else). When
`LEGAIA_WORLDMAP_SLOT4=1` is set, `legaia-engine play-window` builds a
`LineList` from `wireframe_segments_3d` and merges it into the world-map
overlay-lines buffer. **That draw is meaningless**: it plots the entries'
raw `i16` pairs, which straddle the packed nibble boundaries, so what
appears in the view is neither geometry nor motion. It is off by default
and should be re-pointed at `translation_path_segments` (each part's
object-local translation path over the clip), which is a real curve.

### Slot-4 loader (loader-hunt probe)

Running `autorun_slot4_loader_hunt.lua` (now archived under
`archive/pcsx-redux-probes/` - investigation resolved) against Drake
(held UP for 60 vsyncs into the warp) with Write bps tiled across
slot-4 RAM (`0x8011A624 + offset[0..7000]`) surfaced **the LZS
decoder** as the sole writer:

| Caller chain | PC of write | Notes |
|---|---|---|
| `FUN_8001A55C` (LZS decoder, body at `0x8001A55C..0x8001A6XX`) | `0x8001A604` (`sb v1, 0(s1)` - literal-byte write) | Dominant: 5-byte bursts at every probed offset |
| same | `0x8001A664` / `0x8001A668` / `0x8001A610` / `0x8001A5AC` | Back-reference copy / literal-run / dictionary-byte paths inside the LZS loop |

Every captured first-write shows:

```text
pc = 0x8001A604, ra = 0x8001A58C  (LZS decoder calling itself)
stack:
  +0x20  0x8001F194    <- next caller up (inside asset dispatcher region)
  +0x24  0x8001F0A0    <- asset dispatcher FUN_8001F05C-area
  +0x30  0x801E3DC0
```

The chain is the **standard asset-load path**: scene loader →
`FUN_8001F05C` (asset dispatcher) → LZS decoder → writes slot 4 at its
allocated RAM destination (`0x8011A624` for Drake). No special slot-4
transcoder; the asset is just LZS-decoded verbatim into RAM, matching
the byte-verified `disc → RAM` finding documented in
[RAM layout](#ram-layout-confirmed) above.

### Working-buffer writers (transcoder-hunt probe)

Running `autorun_slot4_transcoder_hunt.lua` (now archived under
`archive/pcsx-redux-probes/` - investigation resolved) against Drake
(held UP for 60 vsyncs into the warp transition) with Write bps tiled
across the `0x801BA000` working buffer surfaced **two distinct
writers**, not a single transcoder:

| Working-buffer offset | First-write PC | RA | Writer function | Role |
|---|---|---|---|---|
| `+0x7F8` (`0x801BA7F8`, cluster A's `vertex_base`) | `0x80028710` / `0x8002871C` (paired `sh` instructions) | `0x8001B160` | `FUN_80028158` (5580 B / 1395 instructions) | **per-frame procedural mesh builder**, called from `FUN_8001ada4` case 4 |
| `+0x8E4` (`0x801BA8E4`, cluster A's `command_stream`) | `0x800293C8` / `0x800296A0` (paired `sw` instructions) | `0x8001B160` | same `FUN_80028158` | per-frame procedural primitive-batch writer (same call) |
| `+0x6000` (`0x801C0000`, deeper region) | `0x8001A8C8` (memcpy inner loop) | `0x8001E758` | `FUN_8001E54C` (836 B), the streaming chunk processor | **scene-load chunk loader** - copies streaming-format chunks (`[type, size, data]`) to the buffer |

**`FUN_80028158`** decompiles as a switch on `(param_2 >> 3) & 0xf`
with per-case mesh layouts; it reads only the actor's `+0x9C` params
struct (offsets `+0x10..+0x22`) and writes the working buffer
directly. **No slot-4 RAM pointers appear in its arguments** - it is a
procedural mesh generator (probably waves / sky / particle-emitter
sheets), not a slot-4 transcoder.

**`FUN_8001E54C`** is the `[type, size, data]` streaming chunk
dispatcher: it switches on `*(char*)(chunk + 3)` (the chunk type byte)
and routes each chunk to one of memcpy (case 0/2), LZS decode (case
1/3), or another decoder (case 12). Its 4 captured writes at
`0x801C0000` are scene-load chunk copies that land deeper into the
buffer than cluster A's per-frame inputs at `+0x7F8` / `+0x8E4`.

**Neither writer is a slot-4 transcoder.** `FUN_80028158` populates
the working buffer procedurally and `FUN_8001E54C` copies *other*
scene-load chunk streams; neither ever carries slot-4 pointers. The
slot-4 records themselves are read in place by the renderer - see the
next section. (An intermediate "distribute-then-read-working-buffer"
model built on these writers is superseded by the in-place capture.)

### Slot-4 is read IN PLACE - there is no transcode (Drake capture)

The heading's claim holds: the bytes are read where they were decoded,
every frame, with no intermediate buffer. The **reader** in the original
write-up was misattributed, and the corrected identification is what
finally explained the capture's own return addresses.

`scripts/pcsx-redux/autorun_slot4_source_map.lua` arms Read bps tiled
across the Drake slot-4 RAM window (`0x8011A624 + k*0x800`) plus an Exec
bp on the `FUN_8001E54C` streaming-chunk dispatcher, and drives the
held-Up warp from the `drake_castle_to_worldmap` save. Result (365 rows):

- **363 of the captured accesses are reads, none a copy.** The faulting
  addresses span almost the whole window (`0x8011A624`..`0x80121E24`,
  14 distinct tiled offsets, 8 of 16 read bps hitting their per-bp cap),
  so the buffer is read throughout, every frame.
- The `FUN_8001E54C` dispatcher fired twice and both times its data
  pointer was `0x80184BD0` - **not** in the slot-4 window. Nothing
  copies the records.

The reads are the animation chain. The Sebucus run of the same probe
records `ra 0x8001BB28` for its in-window reads, and `0x8001BB28` is the
return address of `jal FUN_8001BE80` at `0x8001BB20` inside the
animated-actor renderer `FUN_8001B964` - i.e. the entry decoder reading
the clip. The Drake run's `0x8001BC8C` is the return of
`jal FUN_80043390` at `0x8001BC84`, six instructions later in the *same*
function. Both RAs place the caller in `FUN_8001B964`, which is exactly
where an animation reader belongs and is not a top-view terrain pass.

Two of that write-up's inferences do **not** survive:

- "The read PCs are the cluster-A prim dispatcher's GTE mesh path
  (`0x80044C70`)" - `0x80044C70` is a prim handler's vertex load, and a
  prim handler is only ever handed a `DAT_8007C018` TMD. A read bp fires
  on an address, not on a provenance; a TMD allocated into the same heap
  region as the animation bank will trip a window bp all the same.
- "`ra = 0x801F78D4` (the world-map top-view overlay renderer)" - under
  the recovered base `0x801F69D8` for PROT 0901, `0x801F78D0` decodes as
  `swc2 $1,4($t6)`, not a `jal`, and the same VA in the sibling image
  0900 is `addu $t6,$zero,$zero`. No `jal` at `0x801F78D0` exists in
  either slot-B image, so no call in either can produce that return
  address. What the value was is unresolved; it is not evidence that a
  world-map overlay routine reads slot 4.

The "762 of 2153 `FUN_80043390` calls take `a0` from inside the slot-4
window" figure is the same address-vs-identity confusion: `a0` there is
a TMD group pointer, and during the warp the heap put TMDs in that
address range.

**Cross-kingdom.** The resident base is byte-pinned for all three
kingdoms (Drake `0x8011A624`, Sebucus `0x80119CE4`, Karisto
`0x80108D84`) and each equals that kingdom's `_DAT_8007B888`. The
Sebucus re-read against the correct base shows 171 of 177 reads inside
the byte-verified window, from `ra 0x8001BB28` as above.

### Cluster-A caller (`FUN_8001ada4`)

`FUN_8001ada4` (2456 bytes / 614 instructions; see
`ghidra/scripts/funcs/8001b47c.txt`) is the per-actor renderer that
walks a linked list of actor records. For each record at
`piVar2 = head_ptr`, then chained via `piVar2 = piVar2[0]`, it:

1. Pre-transforms the actor's local origin through the GTE
   (`copFunction(2, 0x480012)`) and writes the transformed coordinates
   back into `piVar2[+0x2C..+0x34]`.
2. Switches on `piVar2[+0x56]` (a u16 actor type, values 1..6) to do
   type-specific drawing.

The cluster-A call at PC `0x8001B474` is one of those drawing paths.
The relevant disasm slice (lines 415-442 of `8001b47c.txt`):

```text
8001b40c  lw v1,0x44(s0)        ; v1 = actor+0x44 = mesh-table base
8001b414  lw v0,0x0(v1)         ; v0 = *v1 = mesh count (terminator if 0)
8001b41c  beq v0,zero,...       ; if no meshes, skip render
8001b430  addu v0,v1,s2<<2      ; v0 = mesh-table[index]
8001b438  lw s3,0x4(v0)         ; s3 = (actor+0x44 + index*4 + 4) = mesh_ptr
...
8001b46c  lw a1,0x74(s0)        ; a1 = actor+0x74 (FUN_80043390's cmd_flags arg)
8001b470  lhu a2,0x78(s0)       ; a2 = actor+0x78 (FUN_80043390's fade_flags arg)
8001b474  jal 0x80043390        ; call cluster A with s3 = mesh struct
8001b478  _move a0,s3
```

The mesh-table at `actor+0x44` is a contiguous `[u32 count, u32
mesh_ptr[count]]` array. Each `mesh_ptr` is the pointer FUN_80043390
receives as `param_1` (= the struct exposing `vertex_base` at +0,
`flag_word` at +0xC, `command_stream` at +0x10). Case 3 inside the
type switch contains the same pattern explicitly:

```c
puVar5 = (uint *)piVar2[0x11];  // = actor+0x44
if (*puVar5 != 0) {
  do {
    uVar11 = puVar5[uVar10 + 1];     // mesh_ptr
    FUN_80043390(uVar11, piVar2[0x1d] | uVar8, *(undefined2 *)(piVar2 + 0x1e));
    ...
  } while (uVar10 < *puVar5);
}
```

**The animated sibling.** `FUN_8001ADA4` switches on `actor[+0x56]`
through an 11-entry jump table at `0x8001042C` (`0x8001AE60..0x8001AE8C`).
Case 5 is the *static* mesh-chain draw quoted above - each object drawn
at the actor's own matrix. Case 1 runs an optional screen-space cull
(`FUN_8001B73C`, a bounding-box RTPT leaf, when `actor[+0x10] & 4`) and
then calls **`FUN_8001B964`** at `0x8001B198` - the *animated* chain
draw, which walks the same `actor+0x44` chain but re-poses each object
from the clip at `actor[+0x4C]` first. That is the routine slot 4 feeds;
see
[Consumer call sites](#consumer-call-sites). (Case 11 is a third,
unrelated user of an `actor[+0x4C]` record: the type-`0x06` CLUT-walk
`MoveImage` stepper at `0x8001B664`, whose record layout is a `[u8
count]` header plus 8-byte `(x, y)` source cells.)

The Exec-bp register snapshots from `autorun_slot4_consumer_pcs.lua`
captured `a1 = 0x801BA8E4` and `a2 = 0x801BA7F8` at the cluster-A LW
PCs - both in the **`0x801BA000`-ish working buffer**, not in the
type-5 buffer (`0x8011A624..0x80122454` for Drake). Those hits belong to
the *per-actor* renderer path (`FUN_8001ada4` walking `actor+0x44` mesh
tables over the procedurally-built working buffer) - a **separate,
non-slot-4 stream**.

## DAT_8007C018 - global TMD pointer table (the *actual* cluster-A source)

`FUN_80043390`'s `display_state` arg points at a TMD's group-descriptor
array (offset `+0xC` into a TMD blob whose `+0x00` carries the Legaia
magic `0x80000002`). Those TMD pointers live in a global runtime table:

```
DAT_8007C018 : array of u32 TMD pointers; entry stride = 4
DAT_8007B774 : install counter (next free index)
DAT_8007BB38 : walker counter (last installed index, used by the table walker)
DAT_8007B824 : per-pack count (set by case 2 to `*pack_header[0]`)
```

The installer is **`FUN_80026B4C` @ PC `0x80026BA8`** (called per-TMD
from the asset dispatcher's case 2 TMD-pack handler):

```
80026b90  lui   v1, 0x8008
80026b94  lw    v1, -0x488c(v1)     ; v1 = *DAT_8007B774 (next free idx)
80026b98  addiu v0, v0, -0x3fe8     ; v0 = 0x8007C018
80026b9c  sll   v1, v1, 0x2
80026ba0  addu  v1, v1, v0          ; v1 = &DAT_8007C018[idx]
80026ba4  jal   FUN_800268dc        ; build per-group descriptor array at tmd+0xC
80026ba8  _sw   a0, 0x0(v1)         ; install: DAT_8007C018[idx] = tmd_ptr
                                     ;          and, via gp+0x820: DAT_8007BB38 = idx
                                     ; (gp[+0x820] aliases DAT_8007BB38 in SCUS)
```

Ghidra's static reference-database doesn't surface either store because
the `addu` between the `lui+addiu` and the `sw` defeats its constant
propagation. The materialisation scan
[`ghidra/scripts/find_addr_materializer_dat_8007c018.py`](../../ghidra/scripts/find_addr_materializer_dat_8007c018.py)
walks every `lui+addiu` pair that produces `0x8007C018` across SCUS +
every world-map overlay; that's how the installer was pinned. A scan
across `SCUS_942.54`, `overlay_world_map.bin`, `overlay_world_map_top.bin`,
`overlay_world_map_walk.bin`, and `overlay_world_map_top_ext.bin`
returns **only one store site** (`FUN_80026B4C @ 0x80026BA8`); every
other materialisation in those programs is a read.

After installation, each pointed-to TMD has the runtime shape:

```
[+0x00] u32 magic = 0x80000002
[+0x04] u32 flags     (= 1 post-fixup; FUN_800268DC's idempotency guard)
[+0x08] u32 group_count
[+0x0C] array of group_count × 0x1C-byte group descriptors
        each starts with `vertex_base_ptr (u32) + vertex_count (u32)`
        followed by 0x14 bytes of per-group state
```

### Readers (all access `DAT_8007C018[*]` read-only)

| Function | Site | Role |
|---|---|---|
| `FUN_80021B04` (SCUS actor allocator) | reads `DAT_8007C018[actor[+0x64].i16]` | populates `actor[+0x44] = [count, mesh_ptr[count]]` from TMD groups |
| `FUN_80024D78` (SCUS actor allocator - variant) | reads `DAT_8007C018[actor[+0x64].i16]` | same shape as `FUN_80021B04` but also OR-sets `actor[+0x10] \|= 0x08000000` (a per-actor enable flag) |
| `FUN_801D77F4` (overlay alt allocator) | reads `DAT_8007C018[(i16)param_2]` | copies vertex pool from sub-records into `actor[+0x90]` |
| `FUN_801D8280` (overlay table walker) | iterates `DAT_8007C018[0..DAT_8007BB38]` | hands each sub-record to `FUN_801D5E20` |
| `FUN_801F69D8` (world-map top-view dispatcher in `world_map_top_ext`) | reads `DAT_8007C018[(visible_object_kind8 + DAT_8007B6F8) * 4]` | walks per-tile visibility scratchpad and calls `FUN_80043390(tmd+0xC, color, fog)` |
| `FUN_8001E890` | sets `entry[+0x8] = 10` for three consecutive table indices at `DAT_8007B824 + 0..2` | per-pack count override (overwrites the installed TMD's `group_count` field) |
| `FUN_8001EBEC` | reads `DAT_8007C018[DAT_8007B824 + 0..2]` (3 consecutive party-character TMDs) | per-party-member group-descriptor patch - for each of 3 chars, picks one of two pre-built 0x1C-byte descriptors (`TMD+0x124` vs `TMD+0x140`) based on a per-character byte at `0x80084xxx + char_stride*N + offset`, then overwrites the indexed group descriptor in the TMD. Drives equipment-conditional mesh swaps |

The world-map top-view dispatcher `FUN_801F69D8` (2572 B / 643 instr at
prologue `0x801F69D8`, dumped in
`ghidra/scripts/funcs/overlay_world_map_top_ext_wm_ext_dispatcher_caller_801f69d8.txt`)
is the route the warp-into-world-map Read-bp probe captured. Its body
copies a 0x20-byte camera struct from `0x8007BF10` into scratchpad,
nested-loops over Y/X tile indices (padded by ±10), dereferences each
visible tile's 0x20-byte object record from
`_DAT_1F8003EC + 0x8000 + Y*0x100 + X*2`, applies frustum + GTE RTPT,
then routes the TMD via `DAT_8007C018` and calls `FUN_80043390`. The
`color` arg is `0xD0D0D0` default, switched to `0x40D0D0D0` if the
object record's `[+0x1E]` flag is set, and OR'd with `0x10000000` if
`record[+0x12] & 0x800`. The `fog` arg is
`clamp((GTE_screen_z - 0x5000) >> 3, 0, 0x1000)`.

### Live snapshot (settled field scene)

> **Capture provenance correction.** The local dump file is named
> `drake_world.bin`, but its `0x80084540` scene id is `0x3c` and the scene name
> at `0x80084548` is `dolk` with `game_mode 0x03` - it is the **`dolk` field
> scene**, *not* the Drake world map. The `DAT_8007C018` table is filled
> identically by every field-scene load (the single descriptor-walk
> `FUN_80020224`), so the layout/counter observations below are valid as a
> generic field-scene example; just don't read them as world-map-specific. The
> `[5..142]` entries are this scene's field-file TMD pack (one contiguous
> 138-entry pack), not a "kingdom bundle".

RAM dump after the scene load has settled
(`captures/ram_dumps/drake_world.bin`, local-only - the `dolk`
field scene, see correction above):

| Field | Value |
|---|---:|
| `DAT_8007B774` (install counter) | `143` |
| `DAT_8007BB38` (walker counter) | `142` |
| `DAT_8007B6F8` (kingdom-TMD prefix) | `5` |
| `DAT_8007B828` (error bits) | `0x00000000` (no magic mismatches during install) |

Entry contents (per
[`scripts/asset-investigation/classify_dat_8007c018.py`](../../scripts/asset-investigation/classify_dat_8007c018.py)):

| Index range | Count | Content |
|---|---:|---|
| `[0..4]` | 5 | Character-mesh TMDs at `0x8014D554..0x801585C0`, group_count 10/10/10/3/2. Disc source: [§ Disc-side source of `[0..4]`](#disc-side-source-of-04) below. |
| `[5..142]` | 138 | Kingdom-derived TMDs at `0x800F7908..0x80138D44` (group_count 1..10, mixed sizes) |
| `[143..255]` | 113 | Either zero (uninstalled - never written) or stale junk past the walker counter - **never read by code** because every reader gates on `DAT_8007BB38` or an explicit index ≤ install counter |

**Every populated entry is a valid Legaia TMD** (magic
`0x80000002`, flags = 1, group_count > 0). The table is homogeneous in
the steady state. This is a *field-scene* (`dolk`) snapshot, not a
world-map one: its `0x8011Axxx`-range TMDs (e.g. `[94..113]` at
`0x8011A7B0..0x8012202C`) are this scene's field-file TMD pack, not
the kingdom slot-4 buffer. An earlier reading treated those
body-aligned entries as "slot-4 bytes overwritten by TMD blobs" -
that conflated this field-scene table with the world-map slot-4
render; on a world-map warp the slot-4 records are read in place at
their resident base, not overwritten.

### Disc-side source of `[0..4]`

The five character-mesh TMDs at `DAT_8007C018[0..4]` originate from
**PROT entry 0874 (`befect_data`)**, not from the dev-tree path
`data\field\player.lzs` (whose runtime name maps to PROT 876 -
`player_data` - which actually carries a VAB + TIM_LIST + SEQ
streaming-format payload with **zero TMDs**; see [data-field.md](data-field.md)
for the chunk shape).

PROT 0874 is a [`parse_player_lzs(buf, 3)`](asset-descriptor.md)-shaped
container with three LZS-compressed sections:

| Section | Type byte | Compressed size | File offset | Content |
|---:|:---:|---:|---:|---|
| 0 | `0x01` | `0xB49C` (46 236 B) | `0x20` | 5-TMD pack (LZS decodes to 65 536 B) |
| 1 | `0x02` | `0x41E0` (16 864 B) | `0x5037` | Secondary TMD payload |
| 2 | `0x03` | `0x1D524` (120 100 B) | `0x7055` | MAN-shape data |

Decoding section 0 (LZS-decompress from file offset `0x20`) yields a
canonical TMD pack - `[u32 count][u32 word_offsets[count]][TMD bodies]`
with word offsets in 4-byte units (same convention as
[`tim-pack`](tim-pack.md) / kingdom slot 1):

| Pack slot | Body offset | nobj (disc) | Body bytes (to next slot) |
|---:|---:|---:|---:|
| 0 | `0x0018` | 12 | 13 220 |
| 1 | `0x33BC` | 12 | 13 800 |
| 2 | `0x69A4` | 12 | 11 656 |
| 3 | `0x972C` | 3  | 6 488 |
| 4 | `0xB084` | 2  | 20 348 (trailing padding to pack end) |

Byte-equality check against a settled field-scene RAM snapshot
(`captures/ram_dumps/drake_world.bin`,
local-only - the `dolk` field scene, not the Drake world map; see the
provenance correction in [§ Live snapshot](#live-snapshot-settled-field-scene)).
The character meshes `[0..4]` are the shared party pack every field scene loads,
so the equality holds scene-independently:

- **Pack slot 3 vs RAM `DAT_8007C018[3]`** (un-fixup the runtime's
  absolute-pointer group descriptors back to disc-form offsets using
  `disc_off = abs_ptr - (tmd_base + 0xC)`): the full 6488-byte body
  matches byte-for-byte (0 differences over 0x1958 bytes compared).
- **Pack slot 4 vs RAM `DAT_8007C018[4]`**: the first 1048 bytes match
  byte-for-byte. The runtime allocates only the in-use prefix; the
  trailing ~19 KB of disc padding is not copied.
- **Pack slots 0/1/2 vs RAM `DAT_8007C018[0..2]`**: the first three
  group descriptors and groups 4..9 match byte-for-byte. RAM's `nobj=10`
  vs disc's `nobj=12` is a deliberate runtime override (see "10-group
  cap" below); RAM's slot-3 group descriptor is sourced from disc's
  group 11 (see the FUN_8001EBEC patch below).

### 10-group cap + equipment-conditional group patch

The five disc TMDs ship with `nobj=12` (for the three active-party
slots) and `nobj=3 / 2` (for the trailing two - confirmed `nobj` from
the disc pack matches RAM exactly for those). The active-party
post-install loop in `FUN_8001E890` overwrites
`DAT_8007C018[DAT_8007B824 + 0..2]`'s `entry[+0x08]` (TMD `group_count`)
to **10**, capping each of the first three TMDs at 10 active groups.
The last two disc groups (10 and 11) are *equipment-conditional*
descriptors: `FUN_8001EBEC` reads two per-character bytes (from
`0x80084xxx`, equipment slots) and for each of the three active party
slots picks either `TMD+0x124` (= group 10) or `TMD+0x140` (= group
11) and overwrites the indexed live group descriptor with that
pre-built 0x1C-byte template. This is the equipment-conditional mesh
swap (weapon variant, etc.) - see the
[`dat-8007c018-global-tmd-pointer-table`](#dat_8007c018---global-tmd-pointer-table-the-actual-cluster-a-source)
section's `FUN_8001EBEC` row in the readers table for the matching
asm trace.

### Loader chain - resolved

The retail loader is the overlay-resident scene loader **`FUN_801D6704`**
(`ghidra/scripts/funcs/overlay_world_map_801d6704.txt`): it calls
`FUN_80020118` to install PROT 0874 section 0 into `DAT_8007C018[0..4]`, then
`FUN_80020224(0)` (line ~1022 of the dump) to install the kingdom-derived
`[5..N]` via the generic `FUN_8001F05C case 2` → `FUN_80026B4C` TMD-pack chain
- the **same** descriptor walk every field scene runs; there is no
kingdom-specific installer. The static-SCUS dead ends below are retained for
provenance:

`FUN_8001E890`'s retail-PROT branch (`DAT_8007B8C2 != 0`) calls
`FUN_8003eb98(0x36C, piVar2, 1)`, which loads PROT 876's raw bytes
into `piVar2`. The downstream LZS calls then interpret
`piVar2[2..7]` as three `(size, offset)` pairs - but PROT 876's
bytes there are streaming-format chunk data (the start of a VABp
header inside chunk 0), not LZS descriptors. That branch is
therefore **incompatible with PROT 876's actual layout** in retail.
The escape hatch once offered here - that the branch might be gated
off by `DAT_8007B8C2 == 0` in retail - is **falsified**: retail boots
that flag at `1`, so the `!= 0` branch is precisely the one retail
takes. Either the branch is genuinely unreached for another reason,
or the shape analysis above is wrong; it is not resolved by the flag.
The `data\field\player.lzs` string and PROT-876 fast path both fall
over the same shape mismatch.

Among the **static SCUS** sites, `FUN_800520F0` (the battle scene
loader) loads PROT 873+874 contiguously, but its two install loops
walk both buffers as flat `[count, offsets[], data]` packs and
process PROT 874's `count = 3` entries via `FUN_8001fbcc` (VDF
install). PROT 874 section 0 is gated on the type byte (`0x01`), so
no static SCUS site funnels section 0 through the TMD-pack handler -
that dispatch lives in the overlay loader `FUN_801D6704` above
(`FUN_80020118` → `FUN_8001F05C case 2` → `FUN_80026B4C`). The one
residual is the exact CDNAME indirection handing `FUN_80020118` its
PROT-0874 bytes; see [Open work](#open-work) below.

### Live snapshot (Sebucus mid-warp)

The Sebucus dump captures the warp transition partway through the
asset install:

| Field | Value |
|---|---:|
| `DAT_8007B774` (install counter) | `92` |
| `DAT_8007BB38` (walker counter) | `91` |
| `DAT_8007B6F8` (kingdom-TMD prefix) | `5` |
| `DAT_8007B828` (error bits) | `0x00000000` |

Entries `[0..91]` are valid TMDs; the install is in flight, so the
TMD-pack handler has not yet completed pushing every member. Entries
`[92..]` carry leftover pointers from a previous game-state's table
fill, but `DAT_8007BB38 = 91` means **no consumer ever reads past index 91**.

The mid-load Sebucus state is what historical "non-TMD entry
classification" passes appear to have sampled. Those mid-load reads
went *past* the walker counter and treated stale leftover pointers as
table content - producing the previously-reported "[45..53] FFFAFFFA",
"[114..193] mixed text/vertex/texture" classifications. With
`DAT_8007BB38` as the authoritative bound, those characterisations are
**out-of-bounds reads, not table contents**.

### Implication for slot 4 - resolved

The `DAT_8007C018` table holds the *geometry*; slot 4 holds the
*motion*. They are separate buffers reached through separate globals
(`DAT_8007C018` vs `_DAT_8007B888`), and the only place they meet is
`FUN_8001B964`, which poses an object from a clip and then draws that
object's TMD. Slot 4 is never routed through `DAT_8007C018`, never
overwritten by TMD-pack installs, and never handed to `FUN_80043390`.
A live `map01` state shows the two occupying disjoint address ranges -
the type-5 buffer at `0x8011A624..0x80122454`, the pool's kingdom TMDs
`[5..44]` from `0x80125150` upward. See
[Slot-4 is read in place](#slot-4-is-read-in-place---there-is-no-transcode-drake-capture).

## Open work

1. **~~Slot-4 → TMD converter~~ - dissolved; ~~kingdom `[5..N]` populate~~ - resolved.**
   There is no converter: the slot-4 records are animation clips read in
   place by `FUN_8001B964`, and the kingdom-derived `DAT_8007C018` entries come
   from the scene's TMD packs, not from slot 4. **The `[5..N]` populate is the
   same generic descriptor walk every town uses - there is no kingdom-specific
   installer.** The world-map loader `FUN_801D6704`
   (`ghidra/scripts/funcs/overlay_world_map_801d6704.txt`) calls
   `FUN_80020118` (party / character meshes → `DAT_8007C018[0..4]`) and then a
   single `FUN_80020224(0)` (the call at line ~1022 in the dump) that fills
   `[5..N]`. `FUN_80020224` walks the descriptor pack into `FUN_8001F05C case 2`
   (the LZS TMD-pack installer), which calls `FUN_80026B4C` per TMD, storing at
   `DAT_8007C018[DAT_8007B774]`. `FUN_80026B4C` mirrors the install cursor to
   `gp+0x820` (= `DAT_8007BB38`) **before** incrementing `DAT_8007B774`, so
   `DAT_8007BB38 = DAT_8007B774 - 1` is the **inclusive** upper bound the table
   walkers loop on (`i < DAT_8007BB38 + 1`). Same chain, same counters as any
   field-scene load.

   **Static-side evidence narrowing the earlier MOVE-buffer hunt** (sweep via
   [`scripts/ghidra-analysis/scan_funcs_for_addr_range.py`](../../scripts/ghidra-analysis/scan_funcs_for_addr_range.py)
   across SCUS + every captured overlay dump under
   `ghidra/scripts/funcs/`):

   - **`_DAT_8007B888` (type-`0x05` buffer pointer set by `FUN_8001F05C`
     case 5):** six sites across SCUS and every extracted overlay image
     (`find-gp-relative-refs.py --va 0x8007b888`). SCUS: the case-5 store
     `0x8001F3A8`, the `FUN_8002541C` reset `0x800254E8`, and the single
     reader `0x8002055C` inside `FUN_800204F8`. Overlays: three reads,
     **all in PROT 0976 (Baka Fighter)**. **Zero in any world-map
     overlay.** That measurement was right, and its conclusion - "so the
     converter must run somewhere else" - was the mistake: there is no
     converter to find, because `FUN_800204F8` is not a "Tactical Arts
     move-table parser" but the **animation clip selector**, and the
     world-map controller has no reason to read the pointer. The buffer
     is reached by every animated actor, on every map, through the actor
     tick. See [Consumer call sites](#consumer-call-sites).
   - **`DAT_8007C018[94..113]` (the index range whose live snapshot
     once held slot-4-body-aligned pointers):** zero specialized
     readers - no function statically materializes any address in
     `0x8007C190..0x8007C1E0` via `lui+addiu`, `lui+lw_with_offset`,
     or positive-offset `lw` from the table base. Consistent with the
     [Live snapshot](#live-snapshot-settled-field-scene) finding
     that those entries are real TMDs in steady state and are reached
     only through the generic table walkers that iterate
     `[0..DAT_8007BB38]`.

2. **~~Per-record 4th `i16` (`attr`)~~ - dissolved.** There is no 4th
   `i16`. Bytes 6 and 7 are the Y and Z rotation angles and are read
   every frame at `0x8001C0E4` / `0x8001C0E8`. The sweep that found "no
   reader of the pool word's high half" was sweeping the prim-handler
   family, which never sees these bytes, so it could not have found one;
   the "body-12 values cluster at `+-1280, +-1792, ...` like packed
   tags" observation was `rotY | rotZ << 8` read as one word. The one
   field nothing reads is byte 4's **high nibble**, zero in all 22 228
   entries on the disc - see
   [The one genuinely unread field](#the-one-genuinely-unread-field---byte-4s-high-nibble).
   (Separately, there is **no** clip `rate` ↔ `cmd_flags` bank link:
   Drake's dispatcher probe never sets `0x04000000` / `0x20000000`, so
   only banks `0x00` and `0x50` are exercised - see
   [Banks exercised in retail world-map play](#banks-exercised-in-retail-world-map-play).
   `rate ∈ {1, 2, 4}` is a clip field, unrelated to bank dispatch.)

3. **Banks 2 (`0xA0`) and 3 (`0xF0`).** Banks reachable in the
   dispatcher but never observed during retail world-map play.
   Candidates: dev/debug menu render modes, battle-overlay re-use of
   the dispatcher, or cutscene render paths. Setting up a wide-
   coverage `cmd_flags`-capture probe across multiple non-world-map
   game modes would pin which (if any) caller passes those flags.

4. **PROT 0874 section-0 loader site.** The byte-equality match
   between PROT 0874 section 0 (LZS-decoded from file offset `0x20`)
   and the 5 TMDs at `DAT_8007C018[0..4]` is conclusive (see
   [§ Disc-side source of `[0..4]`](#disc-side-source-of-04)
   above). The **inner dispatch** is fully pinned:

   - `FUN_80020224(asset_type)` walks `_DAT_8007B85C` as an
     [`asset-descriptor`](asset-descriptor.md) pack, calling
     `FUN_8001F05C(buf + offset, size, type, 0)` for each record.
   - `FUN_8001F05C case 2` is the TMD-pack installer: it
     LZS-decodes the section, walks the `[u32 count][u32
     word_offsets[count]][TMD bodies]` pack, and calls
     `FUN_80026B4C(pack + word_offsets[i] * 4, 0)` for each TMD.
   - The retail callers of `FUN_80026B4C` (from a corpus grep over
     `ghidra/scripts/funcs/`) are `FUN_8001E890`, `FUN_8001E928`,
     `FUN_800520F0`, `FUN_8001F05C` itself (recursive), `FUN_800513F0`,
     `FUN_800542C8`, and the muscle-dome minigame loader at
     `overlay_muscle_dome_801f19ec.txt`.

   The **outer producer** that feeds PROT 0874's bytes into this
   dispatch chain is not pinned in the static `SCUS_942.54` dumps:

   - `FUN_8001E890`'s retail-PROT branch (`_DAT_8007B8C2 != 0`)
     loads PROT **876** (`0x36c`), not 874, via
     `FUN_8003eb98(0x36C, piVar2, 1)`. PROT 876 is a streaming-format
     file (VAB + TIM_LIST + SEQ) whose first bytes are a `VABp`
     header, not a 3-section `parse_player_lzs(buf, 3)` container.
     The branch's downstream `FUN_8001a55c(piVar2[2] & 0xffffff,
     ...)` calls read those VAB-header bytes as `(size, offset)`
     pairs - shape-incompatible with PROT 876's actual layout.
     Either the branch is dead code in retail, or
     `_DAT_8007B85C` is populated by another caller first and
     `FUN_8001E890` only fires the dispatch.
   - `FUN_800520F0` (battle scene loader) is the only static SCUS
     caller that issues `FUN_8003e68c(0x36a)` / `FUN_8003eb98` with
     PROT 0x369+0x36A, but it loads them as a contiguous block via
     the debug `_DAT_8007B8C2 != 0` branch and processes the result
     through `FUN_8001fbcc` (VDF install) rather than as a
     3-section `parse_player_lzs` container.

   **Conclusion**: the dispatch goes through the generic
   `FUN_80020224` → `FUN_8001F05C case 2` → `FUN_80026B4C` chain
   from the **overlay-resident scene loader `FUN_801D6704`**
   (`ghidra/scripts/funcs/overlay_world_map_801d6704.txt`), not from
   any static `SCUS_942.54` site. `FUN_801D6704` calls `FUN_80020118`
   to fill the party / character meshes `DAT_8007C018[0..4]` (PROT
   0874 section 0), then `FUN_80020224(0)` to fill the kingdom-derived
   `[5..N]` - the same generic descriptor walk every field scene runs,
   with **no** kingdom-specific installer. So the outer producer is
   pinned; what remains is only the exact CDNAME indirection that hands
   `FUN_80020118` its PROT-0874 bytes (a write-bp probe on
   `DAT_8007C018[0]` would settle it, per
   [`docs/tooling/pcsx-redux-automation.md`](../tooling/pcsx-redux-automation.md)).

   A further static narrowing: the `FUN_8001F05C` case-2
   "freeze" sub-path (`if (param_3 == 1) { _DAT_8007B704 =
   size; _DAT_8007B824 = pack_count; }`) is the sole SCUS
   `sw` writer of `_DAT_8007B824` (at PC `0x8001F2F8`). The
   freeze sets the persistent-base index that
   `FUN_8001E1B4` later reads to reset the install cursor
   (`DAT_8007B774 = _DAT_8007B824`), so a non-zero
   `_DAT_8007B824` would mark slots `[0..pack_count-1]` as
   carried across mode transitions. A corpus grep over every
   call site shows zero static SCUS callers of
   `FUN_8001F05C` pass `param_3 == 1` (the three direct
   callers - `FUN_80020224`, `FUN_8002541C`, and
   `overlay_baka_fighter_801d4c50` - pass `s6`, `0`, and
   `0` respectively), and zero dumped overlay callers of
   `FUN_80020224` pass `param_1 == 1`. So either the freeze
   path is in an overlay not yet captured, or
   `_DAT_8007B824` stays at its BSS-init value of zero
   throughout retail play and every mode rebuilds the TMD
   pool from index 0 (in which case the "persistent slots"
   semantic is vestigial, not load-bearing). The dynamic
   probe should also break on `_DAT_8007B824` writes to
   settle which case holds.

## See also

- [`subsystems/world-map.md`](../subsystems/world-map.md) - the world-map controller and render pipeline.
- [`subsystems/world-overview-viewer.md`](../subsystems/world-overview-viewer.md) - the static-site WebGL viewer.
- [ANM animation container](anm.md) - the same container, in its per-scene form.
- [Legaia TMD](tmd.md) - the mesh format the posed objects are drawn from.
