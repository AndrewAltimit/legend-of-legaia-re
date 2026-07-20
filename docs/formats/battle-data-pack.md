# Player battle files (`data\battle\PLAYER1..4`)

The per-character battle asset files for Vahn / Noa / Gala / Terra - the retail
`battle_data` CDNAME block (defines `865..868`, extraction entries
**0863..0866**; the extraction filename labels `0863/0864_edstati3` are the
[+2 label shift](cdname.md#numbering-space)). Each file is a self-contained
container: a header + LZS `record[0]` (the battle-palette chain), a 12-byte
descriptor table, and a region of per-slot LZS streams that decompress to
`[32-byte header + Legaia TMD + texture pool]`.

> **Identity note (supersedes the earlier "battle_data pack" reading).** This
> page previously described a "custom 16 MB container at PROT 0865". The 16 MB
> figure was extraction 0865's TOC-*indexed* window (7811 sectors), which
> over-reads across 0866 into the [monster archive](#not-the-monster-archive)'s
> sectors; every structure documented here actually sits inside each player
> file's own footprint (0865 = Gala, 222 sectors). The monster archive is a
> **different container** at extraction 0867.

This format is **distinct from**:

- the [monster stat archive](#not-the-monster-archive) (extraction 0867, retail `monster_data`),
- the standalone [TIM-pack](tim-pack.md) used by some other PROT entries,
- the [DATA_FIELD streaming format](data-field.md) used by scene bundles,
- the [field-pack](field-pack.md) and [effect-bundle](effect.md) containers.

Implementations:
[`crates/asset/src/battle_char_palette.rs`](../../crates/asset/src/battle_char_palette.rs)
(the runtime-pinned `record[0]` + CLUT chain) and
[`crates/asset/src/battle_data_pack.rs`](../../crates/asset/src/battle_data_pack.rs)
(the TMD-slot walker over the `[id, offset, size]` descriptor table).

## Contents

- [Load chain + index space](#load-chain--index-space)
- [TOC geometry (the 16 MB misreading)](#toc-geometry-the-16-mb-misreading)
- [Not the monster archive](#not-the-monster-archive)
- [File layout](#file-layout)
- [Descriptor table](#descriptor-table)
- [Slot region](#slot-region)
- [Decompressed slot layout](#decompressed-slot-layout)
- [Battle animations (record[0])](#battle-animations-record0)
  - [Swing records](#swing-records-equipment-sections--slots-0xc0xf)
  - [Art-animation bank](#art-animation-bank-record0-0x58)
  - ["ME" stream archives](#me-stream-archives-readefdat)
  - [Facial animation tracks](#facial-animation-tracks-entry-0x8c--0x98)
  - [Equipment-variant track](#equipment-variant-track-entry-0xa4--fun_8004ccd4)
- [Texture-pool VRAM placement](#texture-pool-vram-placement)
- [Parser status](#parser-status)
- [VRAM byte-match corpus](#vram-byte-match-corpus)
- [CLI](#cli)
- [Open questions](#open-questions)
- [See also](#see-also)

## Load chain + index space

`FUN_80052770` points each party character's asset-table entry at the dev path
`data\battle\PLAYER<n>` (string installs at `0x80052E64..`, decomp
`ghidra/scripts/funcs/80052770.txt`) and opens it through the dual-mode wrapper
`FUN_800558FC(path, …, char_id + 0x360)`. The retail ISO9660 branch is a trap
stub on this build, so the load always resolves through `FUN_8003E8A8` with the
**raw in-RAM TOC index** `char_id + 0x360` - extraction entry
`char_id + 0x360 − 2` (see [`prot.md` § In-RAM TOC](prot.md#in-ram-toc)):

| Player | Raw TOC index | PROT.DAT offset | Footprint | Extraction entry |
|---|---|---|---|---|
| Vahn  | `0x361` | `0x36E8000` | 338 sectors (`0xA9000`) | 0863 (`edstati3` label) |
| Noa   | `0x362` | `0x3791000` | 303 sectors (`0x97800`) | 0864 (`edstati3` label) |
| Gala  | `0x363` | `0x3828800` | 222 sectors (`0x6F000`) | 0865 |
| Terra | `0x364` | `0x3897800` |  47 sectors (`0x17800`) | 0866 |

The offsets are the live-traced `FUN_800558FC` reads (see
[`character-mesh.md` § Battle form](character-mesh.md#battle-form---assembled-from-the-player-files))
and equal the TOC `start_lba × 0x800` of extraction 863..866 exactly. The
historical "Vahn = PROT 0861" attribution matched the same bytes through the
1-sector stub entries 0859..0862 that precede the true file - entry 0861's
*extended* window reaches Vahn's file at window offset `0x1000`.

`FUN_80052FA0` is the per-character **assembler**: it LZS-decodes
`record[0]` + its sub-records into the battle party palette (rows 481..483)
*and* decodes the five equipment-selected sections, then builds each
character's merged battle TMD from them (`FUN_800536BC` splice ×5 +
`FUN_80053898` post-pass; `FUN_800513F0` registers the result). Full chain:
[`character-mesh.md` § Battle form](character-mesh.md#battle-form---assembled-from-the-player-files);
palette half: [`character-mesh.md` § Battle render](character-mesh.md#battle-render-load-time-tsbcba-relocation).

## TOC geometry (the 16 MB misreading)

The TOC declares extraction 0865 with `indexed_size = 7811` sectors
(`0xF41800` ≈ 16.0 MB) against a 222-sector footprint. That extended window
covers Gala's own file (`0x0..0x6F000`), all of Terra's (`0x6F000..0x86800`),
and 7542 of the monster archive's 7760 sectors (`0x86800..`). The extractor's
`0865_battle_data.BIN` is therefore a 16 MB file whose first 222 sectors are
the actual player file - the earlier "16 MB battle_data container" reading
analyzed that window without noticing the boundary. The format structures
below all live inside the footprint, and the slot region **tiles each file's
footprint exactly** (`data_base + last_offset + last_size = footprint` in all
four retail files), confirming the footprint is the true file size.

## Not the monster archive

The monster stat archive (`legaia_asset::monster_archive`, retail-space
`monster_data` = define 869 → extraction **0867**) shares the
`[u32 dec_size][LZS] → mesh + texture pool` general shape but is a different
container with no shared structures:

- Archive slots are **fixed-stride** `0x14000` bytes keyed by 1-based monster
  id (`slot = (id−1) × 0x14000`), with no descriptor table; player-file slots
  are variable-size, reached through the 12-byte descriptor table.
- The archive's decoded head is the monster **stat record**
  (`+0x00 name_offset`, `+0x0C` HP, `+0x4C` action-offset array - see
  [`monster-animation.md`](monster-animation.md)); the player-file slot head
  is the 32-byte texture-layout header below, with the TMD at `+0x20`.
- Within extraction 0865's extended window the archive begins at byte
  `0x86800`; the player-file descriptor table (`0x6C68`) and slot region
  (`0x8000..0x6F000`) sit entirely before it.

The old conflation ("battle_data 0865 vs monster archive 0867") came from the
overlapping extraction windows; the [CDNAME −2 correction](cdname.md#numbering-space)
resolves it - the dev names say exactly what each entry is.

## File layout

All offsets file-relative; values measured from the retail disc.

```
+0x00  u32 desc_off     ; descriptor-table offset. Also reads as a type-0
                        ; streaming chunk header ((0x00<<24)|size), which is
                        ; how streaming-format walkers skip the head cleanly.
+0x04  u32 clut_a_off   ; CLUT A offset within record[0]'s DECODED output
+0x08  u32 clut_b_off   ; CLUT B offset within record[0]'s DECODED output
+0x0C  u32 budget       ; record[0] decoded size (LZS output-byte budget)
+0x10  record[0] LZS stream
+desc_off               ; descriptor table (12-byte entries, see below)
+0x8000 (data_base)     ; slot region: per-slot [u32 dec_size][LZS stream]
```

Measured per file:

| File | `desc_off` | `clut_a` | `clut_b` | `budget` | entries | footprint |
|---|---|---|---|---|---|---|
| 0863 Vahn  | `0x55F4` | `0x5E00` | `0x7E04` | `0x9E48` | 54 | `0xA9000` |
| 0864 Noa   | `0x75C4` | `0x76A8` | `0x970C` | `0xB750` | 50 | `0x97800` |
| 0865 Gala  | `0x6C68` | `0x7464` | `0x9488` | `0xB4AC` | 43 | `0x6F000` |
| 0866 Terra | `0x6CAC` | `0x83E0` | `0xA5C4` | `0xC7A8` |  5 | `0x17800` |

`data_base = 0x8000` in all four retail files (the gap between the table end
and `0x8000` is zero-padded). It is **not** header-derived: `FUN_80052770`
opens each file with a fixed 16-sector prologue read - `FUN_800559EC(…, 0x8000)`
→ `FUN_8003E800(dst, 0x10, 1)` loads the first `0x10` sectors (`0x8000` bytes,
the header + `record[0]` + descriptor table) before the slot region is streamed,
so `0x8000` is a hardcoded loader constant. `legaia_asset::battle_data_pack`
reaches the same base by probing sector boundaries until every slot's `dec_size`
prefix reads sane.

## Descriptor table

At `desc_off`, a chained array of 12-byte entries:

```
u32 id       ; slot id; 0 marks a section boundary / default-variant slot
u32 offset   ; byte offset of the slot from data_base
u32 size     ; slot allocation in bytes (sector-aligned)
```

The chain invariant `offset[i+1] == offset[i] + size[i]` holds across every
entry; an all-zero entry terminates the table. Entries group into **sections
of descending ids separated by `id = 0` entries** - e.g. Gala (0865):

```
57 56 55 54 53 | 00 | 42 41 40 3f | 00 | 21 20 27 26 25 24 23 22
2b 2a 29 28 33 32 31 30 2f 2e | 00 | 19 18 17 16 15 14 13 | 00 |
69 68 67 66 | 00
```

Terra (0866) carries only five `id = 0` entries - no variant slots.

**The slot ids are equippable item ids** (the
[item-name table](item-table.md) id space - the same ids the
[equipment stat table](equipment-table.md) indexes). The five sections are
the character's five equipment slots, in the order of the character record's
equipped-item bytes at `+0x196..+0x19A` (live record base `0x80084708`,
stride `0x414`). `FUN_80052770` case 4 walks the table sequentially with a
section counter: each entry's `id` is compared against the current slot's
equipped-item byte (`(offset, size)` captured on match), an `id = 0` entry
supplies the section's **default** when nothing matched and advances the
section counter (decomp `ghidra/scripts/funcs/80052770.txt`, the
`*0x414 + -0x7ff7b762` read = record `+0x196`). Vahn's file (0863), for
example, carries a body section (`0x43` Hunter Clothes … `0x4A`), a head
section, a weapon section (`0x22` Survival Knife … plus `0xBA`), a Ra-Seru
weapon section (`0x01..0x09` Meta tiers), and a footwear section - each with
its `id = 0` fallback. Live proof: in a full-party battle save with Vahn
wearing Hunter Clothes / Survival Knife / Ra-Seru Meta, the assembled battle
mesh's vertex pools byte-match exactly the `id = 0x43`, `0x22`, and `0x01`
sections (and the defaults for the unequipped slots) - see
[`character-mesh.md` § Battle form](character-mesh.md#battle-form---assembled-from-the-player-files).

## Slot region

At `data_base + entry.offset`:

```
u32 decompressed_size       ; LZS output-byte budget
LZS stream                  ; standard Legaia LZS (see lzs.md)
```

The decoder stops on the output count, not the input length - hand it a
generous source slice rather than truncating to `entry.size`.

## Decompressed slot layout

```
+0x00  u32 frame_off         ; self-relative offset of the loader frame the
                             ; assembler reads (below): 0x14 + 4*attach_objs
+0x04  u32 swing_rec_a       ; self-relative offset of the section's SWING
                             ; ACTION RECORD (sections 2..4; 0 in sections 0/1)
+0x08  u32 swing_rec_b       ; second swing record - consumed only for
                             ; section 4 (0 everywhere else)
+0x0C  u32 tmd_body_end      ; section footprint: where the embedded Legaia
                             ; TMD ends = where the texture-pool upload block
                             ; starts = the decode-buffer advance to the next
                             ; section
+0x10  s16 attach_obj_count  ; attach-object records (0 / 1 / 2 observed)
+0x12  u16 upload_flag       ; non-zero = the post-TMD pool is uploaded to
                             ; VRAM at battle init (the `lh 0x12(s2)` gate in
                             ; FUN_80052FA0); zero = the pool bytes are dead
                             ; (overwritten by the next section's decode)
+0x14  u32 attach_obj_off[]  ; attach_obj_count self-relative offsets, each
                             ; -> an attach-object record whose +0x07 byte is
                             ; its ATTACH KEY (matched against action-entry
                             ; +0x77 bytes - see Battle animations below)
+frame_off  loader frame     ; consumed by FUN_800536BC:
       +0x00 u8  attach_count    ; objects that bind to skeleton bones
       +0x01 u8  bone_ids[]      ; one bone id per attached object (padded)
       +0x08 u32 data_size       ; section data extent (word-copied span)
       +0x0C Legaia TMD          ; magic 0x80000002
       +0x14 u32 nobj            ; (= the TMD's own object count)
       +0x18 object table        ; 7-word TMD object entries
+tmd_body_end                ; texture / CLUT pool
```

The earlier readings of `+0x04`/`+0x08` as "nested-section end offsets"
(`sub_obj0_end`/`sub_obj1_end`) and of `+0x10` as "low half of the flag
word" are superseded: `FUN_80052FA0`'s section loop rebases `+0x04` (and,
for section index 4 only, `+0x08` - the `if (1 < iVar3)` / `iVar3 == 4`
guards) and splices the records into the runtime action table, and walks
`+0x10`/`+0x14` as the attach-object list (decomp
`ghidra/scripts/funcs/80052fa0.txt`). `frame_off` = `0x14 +
4 * attach_obj_count` across the whole retail corpus (`0x14`/`0x18`/`0x1C`).

The assembler `FUN_800536BC` reads the section through the **loader frame**
at `decoded + frame_off`: one bone-id byte per object while
`obj_index < attach_count`, then `0xFF` / `0xFE` tags for the surplus
objects (the equipment's visual meshes - see
[`character-mesh.md` § Battle form](character-mesh.md#battle-form---assembled-from-the-player-files);
`0xFF` = the per-clip swap variant, `0xFE` = an extra animated part - see
[Equipment-variant track](#equipment-variant-track-entry-0xa4--fun_8004ccd4)).
The byte runs the earlier byte-match corpus read as "texture format tags" at
`u32[5..6]` (e.g. `0x0b0a0906`, `0x000e0d0c`) are these attach-count +
bone-id bytes (`06 09 0a 0b | 0c 0d 0e 00` = 6 attached objects on bones
9..14 - a footwear section).

The post-TMD pool has no PSX TIM image-block headers: it is one upload block
in the `FUN_80053B9C` frame -
`[u16 clut_x][u16 clut_n][clut_n × u16 BGR555][w*h halfwords 4bpp pixels]`
(the CLUT struct is the same `[base][count][colours]` shape the palette
chain STP-copies to VRAM rows 481..483; that RAM-side path is decoded in
[`character-mesh.md`](character-mesh.md#battle-render-load-time-tsbcba-relocation)
and ported as `legaia_asset::battle_char_palette`). The pool's VRAM
placement is pinned - see
[Texture-pool VRAM placement](#texture-pool-vram-placement).

## Battle animations (record[0])

`record[0]` (the LZS stream at file `+0x10`, decoded to `budget` bytes) is
not just the battle-palette chain: its head is a **u32 action-offset table**
(12 populated disc slots at `+0x00..+0x2C`; the loader widens it at runtime -
see below) whose entries are the character's **battle action-animation
records** - the same per-action entry family as the monster archive's
(`docs/formats/monster-animation.md`), with the packed
`[u8 part_count][u8 frame_count][9-byte TRS records]` keyframe stream at
**entry `+0xAC`** (the monster entries keep theirs at `+0x8C`), the playback
rate byte at entry `+0x78`, and the entry's first byte its **action tag**
(identity with the slot index in these files). Slot 0 is the neutral
**idle** loop; its frame 0 is the combat-stance rest pose that sockets the
assembled battle mesh.

The **runtime action table** (rebased copies at `0x801C9360 + slot*4`,
built by `FUN_80052FA0`) is wider than the 12 disc words:

- slots `0xC`/`0xD`/`0xE` are filled with **swing records spliced from the
  equipped-item sections** 2/3/4 (each section payload carries a per-item
  action record; section 4 contributes a second record into slot `0xF`) -
  the four direction-command swings (`0x0C` L / `0x0D` R / `0x0E` D /
  `0x0F` U, the same byte values the Tactical-Arts command queue stages as
  anim ids) are therefore **per-equipment animations** (see
  [Swing records](#swing-records-equipment-sections--slots-0xc0xf));
- slots `0x10`/`0x11` are **dynamic**: the anim commit `FUN_8004AD80`
  materializes a record there for any staged id `>= 0x10` from the
  per-character **art-animation bank** (the `+0x58` word; see
  [Art-animation bank](#art-animation-bank-record0-0x58)), loading its
  keyframe stream into a scratch buffer and rewriting the queued id to the
  slot number;
- the `+0x5C` word is a rebased sibling pointer, **paired with the `+0x58`
  art-bank pointer**: `FUN_80052FA0` self-relative→absolute rebases both at
  load, back to back - `*(base+0x58) += base; *(base+0x5c) += base;`
  (`80052fa0.txt:558-561`, with the record[0] base stored at
  `0x801C9360 + char*4`). Its target is pinned: in all four retail files
  `+0x5C == clut_a_off − 4`, the **zero word immediately before record[0]'s
  first image block**. But the paired field is **vestigial**: `+0x58` has a
  reader (`FUN_8004AD80`, `8004ad80.txt:1492`, `base+0x58+4` = the art-bank
  skip count), whereas **no reader of a fixed `+0x5C` exists in the code
  searched** - see [the sweep and its coverage](#the-0x5c-no-reader-sweep)
  for exactly what that covers. Every action-table consumer indexes
  `base + index*4` for its slots or reads `+0x58`/`+0xAC`, and the word
  `+0x5C` (= slot `0x17`) falls outside every consumer's range. The CLUT
  upload uses the file-header fields `file+0x04`/`+0x08`, not `+0x5C`. So it
  is a rebased-at-load paired-relocation field with no traced reader. The
  earlier "it points at the art `"ME"` stream archive" hypothesis is
  separately **disc-refuted** - those archives live in `readef.DAT`
  ([below](#me-stream-archives-readefdat)), and no `"ME"` archive exists
  anywhere in a player file's footprint or its decoded record[0].

### The 0x5C no-reader sweep

The relocation itself is instruction-pinned, in `80052fa0.txt`:

```text
800532b4  lw   a0,0x0(v1)         ; v1 = 0x801C9360 + sel*4  -> record[0] base
800532bc  lw   v0,0x58(a0)
800532c4  addu v0,v0,a0
800532c8  sw   v0,0x58(a0)        ; self-relative -> absolute
800532d4  lw   v0,0x5c(a0)
800532e0  addu v0,v0,a0
800532e4  sw   v0,0x5c(a0)        ; ... and the paired word, same shape
```

The **negative** is the part that needs its coverage stated, because a
"no reader anywhere" claim is only as exhaustive as the thing it swept. This
one was re-derived word-wise from bytes with capstone, not from the Ghidra dump
corpus:

- **`SCUS_942.54`: exhaustive.** All 110,080 words of the `t_size = 0x6B800`
  text decoded individually (102,684 decode as instructions; the remainder are
  data / COP2 words, and no `lw` encoding is undecodable, so the sweep is sound
  for loads). The whole executable contains **31** loads at offset `0x5c`, of
  which exactly one has a non-`sp` base: `800532d4`, the relocation above.
  Every other non-`sp` `0x5c` access is an `lh`/`lhu`, which cannot read a
  32-bit relocated pointer.
- **Overlays: 15 of 26 images.** The 15 binaries in `extracted/overlays/`
  (278,267 words) yield two non-`sp` word loads at `0x5c`, both in
  `overlay_summon_render_0900.bin` (`801f7984`, `801f9af4`). Both sit inside a
  contiguous `+0x44..+0x60` eight-word block copy whose base register is set by
  `801f7090 lui s1,0x1f80` - the **scratchpad**, not a `0x801C9360` record.
  Not readers.

**What is not covered:** `crates/asset/data/static-overlays.toml` carries 26
overlay identities; 11 of them - mostly summon stagers sharing the
`0x801F69D8` window - are not extracted locally and were seen only through
Ghidra dumps. To close the negative completely, extract those with
`asset overlay` and re-run the same word-wise sweep. Until then the honest
statement is "no reader in SCUS or the 15 extracted overlays", **not**
"no reader anywhere".

### Swing records (equipment sections → slots 0xC..0xF)

Each selected section's decoded payload carries self-relative offsets to
its swing record(s) at `+0x04` (and `+0x08` for section 4 - see
[Decompressed slot layout](#decompressed-slot-layout)). A swing record is a
standard **action entry**: action-tag byte at `+0x00`, rate byte at
`+0x78`, packed `[u8 parts][u8 frames][parts*frames × 9-byte TRS]` keyframe
stream at `+0xAC`. The splice helper `FUN_800557B8` pins the shape exactly:
it copies `0x2B` words (`0xAC` bytes) of header plus
`(parts*frames*9 + 5) >> 2` words of stream into the persistent buffer, and
`FUN_80052FA0` installs the copy at action-table word `0x28 + section*4`
(slot `0xC + section − 2`; section 4's `+0x08` record at word `0x3C` =
slot `0xF`), pointing the entry's `+0x88` stream pointer at `entry+0xAC`
(decomps `80052fa0.txt` / `800557b8.txt`).

Disc census (every equippable id in every file, disc-gated
`swing_anim_real` test): sections 0/1 carry `0` in both words; every
section-2/3/4 slot carries a valid record at `+0x04` (and section 4 at
`+0x08`), `parts` = the character's skeleton bone count (up to +2 channels
on slots with attach objects), stream end inside the section footprint.
The record's `+0x00` tag is a presentation-class id (`0x0E..0x1F`
observed), **not** the runtime slot. Sections with `attach_obj_count > 0`
additionally carry attach-object records; `FUN_80052FA0` matches each
attach record's `+0x07` **attach key** against the action entries'
`+0x77` bytes (then the art bank's `+0x9B` keys) and links the attach copy
into the matching entry's `+0x04`/`+0x08` pointer pair (copy helper
`FUN_80055854`).

The copy helper pins the attach-object record's shape, because its length is
discovered by walking rather than read from a header:

```text
[word][word][u32 outer_count]
  outer_count × ( [word][word][u32 pair_count] + pair_count × 2 words )
```

Everything is word-granular and both counts are treated as **signed** (a
negative count copies nothing rather than wrapping), source and destination
strides are identical, and no pointer inside the copied bytes is relocated -
it is a plain `memcpy` whose extent the two nested counts define. The helper
returns the advanced destination pointer, which the caller uses as the bump
allocator's new high-water mark. Note `pair_count` is doubled before the copy,
so it counts **pairs**, not words.

Parser: `legaia_asset::battle_char_assembly::swing_battle_animations`
(slots `0xC..=0xF` for a given equipped-id set, sharing the monster-archive
stream decoder).

### Art-animation bank (record[0] +0x58)

The self-relative word at record[0] `+0x58` locates the bank:
`[u32 count]` then `count` `0xD0`-stride records. Each record is a
`0x24`-byte arts-matcher head + a standard `0xAC`-byte action entry
(`0x24 + 0xAC = 0xD0` exactly):

```
+0x00  u8 combo[..0x0A]   ; arts-matcher direction commands (1..4),
                          ; zero-terminated; empty on the base records
+0x0A  u8 stream_source   ; entry index into the character's "ME" stream
                          ; archive (the FUN_8002B28C third argument)
+0x10  char name[20]      ; inline art-name string (NUL-terminated ASCII;
                          ; empty on the base / un-named records)
+0x24  action entry       ; 0xAC bytes - the standard entry header:
       +0x00 u8  tag          ; presentation-class id (0x16..0x1F on named
                              ; arts, 0 on base records)
       +0x04/+0x08 u32        ; attach pointers - 0 on disc, written at
                              ; runtime by FUN_80052FA0's attach-key scan
       +0x77 u8  attach_key   ; matched against equipment attach records
                              ; (record-relative +0x9B)
       +0x78 u8  rate         ; playback rate (FUN_80047430 cursor)
       +0x84 u8  rate_alt     ; secondary anim-rate field (-> actor +0x21B);
                              ; 0xFF marks the eight base-archive records
       +0x88 u32 stream_ptr   ; 0 on disc - FUN_8004AD80 points it at the
                              ; decoded scratch buffer at commit
       +0x8C u8  eyes[4][3]   ; facial eye track (= record +0xB0) - the
                              ; standard entry tracks, read while the
                              ; materialized art clip plays (see Facial
                              ; animation tracks)
       +0x98 u8  mouth[4][3]  ; facial mouth track (= record +0xBC)
```

The "record 0's first byte coincides with the bank count" wrinkle
dissolves byte-exactly: the bank head is a **u32 count** and records start
at `bank + 4` - `FUN_8004AD80`'s install arithmetic
`q*0xD0 + bank + 4 − 0xCDC` = `bank + 4 + (q−0x10)*0xD0 + 0x24` (entry),
name read at `−0xCF0` (= record `+0x10`), stream-source byte at `−0xCF6`
(= record `+0x0A`); `FUN_80052FA0`'s attach scan reads the keys at
`bank + 4 + k*0xD0 + 0x9B` (decomps `8004ad80.txt` / `80052fa0.txt`). A
staged anim id `q >= 0x10` selects record `q − 0x10`; ids `0x10` and
`0x1A` install at slot `0x11`, every other id at `0x10`; ids `> 0x1A`
drive the HUD art-name display from `+0x10` and
`FUN_8004C650(char, id − 0x1B)`. Retail banks: Vahn 33 / Noa 35 / Gala 32 /
Terra 9 records; the named band (records 11+) carries the Hyper/Miracle
Art names (`Vahn Rondo`, `Fiery Miyawaki`, `Mirage Lancer`, …).

Parser: `legaia_asset::battle_char_assembly::art_animation_bank` (+
`art_animation` to resolve a record's keyframe stream through its archive).

### "ME" stream archives (readef.DAT)

An art record's keyframe stream is **not inline** in the player file:
`FUN_8004AD80` calls `FUN_8002B28C(_DAT_8007BD74, scratch, stream_source)`,
and `_DAT_8007BD74` is the battle side-band **streaming buffer** -
`FUN_801F17F8` fills it with one `0x10800`-byte slot of
`data\battle\summon.dat` / `readef.DAT`
([`summon-readef.md`](summon-readef.md)). The player art archives live at
the head of the **`readef.DAT`** (extraction PROT 894) slots

| character | main archive (named arts) | base archive (`rate_alt = 0xFF`) |
|---|---|---|
| Vahn  | slot 1 (17 entries) | slot 2 (8) |
| Noa   | slot 4 (18) | slot 5 (8) |
| Gala  | slot 7 (19) | slot 8 (8) |
| Terra | slot 10 (1) | slot 11 (8) |

i.e. slots `3*char + 1` / `3*char + 2` (slot `3*char` is the group's
non-ME texture slot). The staging is traced end to end, and the
main-vs-base pick is **per battle phase, not per record** - the resident
`*0x8007BD74` buffer holds one `0x10800` slot at a time, and which archive
is resident when `FUN_8004AD80` commits decides the read:

- **Main archive (`3*char+1`) - resident during turns.** Every scheduled
  turn the initiative scheduler `FUN_801DABA4` writes the acting entity's
  group base into `ctx+0x277` (party actor: `3*(char−1)` from
  `DAT_8007BD10`; enemy actor: `3 * monster_record[+0x1C]`, the monster's
  readef group byte; an AI-delegated party attacker substitutes the
  delegate's group) and kicks the applier SM `FUN_801F12D0`
  (`ctx+0x276 = 1`), which requests slot `base+0` (the texture page),
  uploads it, requests slot `base+1`, and **stops there for readef
  groups** (the stage-4 tail resets the SM unless bit 7 is set or
  `base == 0x36`) - so any art committed mid-turn decodes from the main
  archive.
- **Base archive (`3*char+2`) - resident at battle end.** The battle-end
  arms request it directly through `FUN_80055B4C` (one slot, bypassing
  the group SM): the scheduler's no-living-enemy branch and the
  battle-action SM's victory arm (`FUN_801E295C`, which first re-rolls
  `ctx+0x13` onto a living party member). The victory sequencer then
  stages the win poses - ids `0x11..=0x18` = bank records 1..8, exactly
  the eight `rate_alt == 0xFF` records with `stream_source` `0..=7` in
  all four retail files - and `FUN_8004AD80` decodes them from the
  resident base archive.

The exact cover observable on disc (the eight base records' sources =
`0..=7` = the base archive's entry range; the remaining records' max
source = the main archive's `count − 1`) is the consequence of this
phase split, not a per-record selector. Decomps
`overlay_battle_action_801daba4.txt`, `overlay_battle_action_801e295c.txt`,
`overlay_muscle_dome_801f12d0.txt`, `80055b4c.txt`.

Archive layout (reader `FUN_8002B28C`, decomp `8002b28c.txt`):

```
+0x00  'M' 'E'                 ; magic
+0x02  u8  count
+0x03  u16 entry_sizes[count]  ; bit 15 = compressed, low 15 bits = size
+0x03 + 2*count                ; concatenated bodies, in entry order
```

A clear bit 15 means the body is the packed keyframe stream verbatim; a
set bit 15 routes through the **channel-delta codec** `FUN_8002A9CC`
(decomp `8002a9cc.txt`): header byte `(b0 & 0xC0) == 0x40`, u16 offsets at
`+1`/`+3` to a 4-bit operand stream and a byte stream (`[parts][frames]` +
literal low bytes), selector bits at `+5`. Per 12-bit channel value the
selectors choose a literal, a previous-part delta ± nibble, or a literal
nibble; frame 0 accumulates spatially down the parts, later frames
temporally per channel; each frame row re-packs into the standard 9-byte
TRS records. **Every** art entry on the retail disc has bit 15 set, so the
codec is the exercised path. Decoded output validated across the full
corpus: every stream is length-exact (`2 + parts*frames*9`) with
`parts` == the character's skeleton bone count.

Parsers: `legaia_asset::me_archive` (`parse` + `decode_channel_delta`) and
`legaia_asset::battle_char_assembly::art_me_archive` (the readef slot
slicing); `legaia_asset::summon_readef` classifies these slots as
`SlotKind::MeArchive`.

`part_count` equals the character's **skeleton bone count** (15 Vahn /
16 Noa / 15 Gala / 17 Terra - the assembled mesh's `nobj` minus its
equipment extras): channel `i` drives assembled object `i` (post-sort,
object index == bone tag), and the extras ride their attach bone's channel
via the assembled blob's side tables. The retail consumers are the battle
render node's update `FUN_80047430` → `FUN_8004AD80` (the node's `+0x4C`
anim context is one of these entries; the loader rewrites the in-RAM action
table to absolute pointers and points the entry's `+0x88` stream pointer at
`entry +0xAC`). Live-verified: in a full-party capture every party slot's
anim context sits at `record0_image + action_table[0]` and the whole idle
stream byte-matches the disc decode
(`crates/engine-shell/tests/battle_party_pose_live.rs`). The **PROT 1203
ANM bundle is not the battle pose source** - no 1203 record is resident in
battle RAM, and its banks are authored against PROT 1204's own object
order (see
[`character-mesh.md` § Assembly](character-mesh.md#assembly---object-local-pieces-posed-by-the-characters-own-battle-streams)).

**Populated slots** (disc census, asserted by the disc-gated
`player_action_table_real` test): all four characters carry entries `0..0xB`
with `action_tag == slot`; Vahn / Noa / Gala have decodable streams at
`{0,1,2,3,4,5,7,8,9,11}`, Terra at `{0,1,2,3,4,5,9,11}` (her 7/8 entries
exist but hold empty streams - she barely fights). **Entry 6's stream is
empty in all four files**, and that's expected: retail's idle anim id is
`0` (the SM stages `+0x1DA = 0`), and the `FUN_801D5854(actor, 6..9)` calls
are a separate camera/presentation program space - id 6 never reaches the
anim system. The slot semantics are the **action-tag space** (see
[`monster-animation.md` § Action tags](monster-animation.md#action-tags-and-the-0x1ef-reaction-map)):
`1` walk/approach (staged by the attack band's party arm), `2`/`3` light
flinches, `4` knockdown, `5` get-up, `7`/`8`/`9` ready/recover/defeat
(staged by the SM and the `FUN_8004AD80` end-of-clip chains), `0x0B` block.
The historical "strike family awaiting per-state attribution" reading is
resolved: the attack swings do **not** come from these entries at all -
they come from the equipment-spliced slots `0xC..0xF` and the dynamic art
slots `0x10`/`0x11` above. The engine plays the hit-reaction family via
`engine-core::World::queue_battle_reaction` (the `FUN_800402F4` staging
rule) and keeps the SM pose ids on their same-numbered entries
(`apply_battle_pose`; idle maps to entry 0, matching the frames retail
shows).

Parsers: `legaia_asset::battle_char_assembly::{decode_record0,
battle_animations, idle_battle_animation, expand_animation_for_objects,
swing_battle_animations, art_animation_bank, art_me_archive,
art_animation}` (the stream decode is shared with
`legaia_asset::monster_archive`; the `"ME"` archive + codec live in
`legaia_asset::me_archive`).

### Facial animation tracks (entry `+0x8C` / `+0x98`)

Two fields of the `0xAC` action-entry header are per-clip **facial
keyframe tracks**, consumed by the per-frame facial animator
`FUN_8004C7B4` (called from the render-node update `FUN_80047430` with
the node's `+0x68` anim cursor - in integer keyframes - as the frame
counter, for every party member except Terra - char index 3 is skipped):

- entry `+0x8C`: **eye** track - four 3-byte records
  `[frame_id, start, end]`;
- entry `+0x98`: **mouth** track - same shape.

(The eye/mouth identity is pinned visually from the catalogued battle
captures: the `+0x8C` table's frames are the wide two-eye band - frame 1
a narrowed blink - and the `+0x98` frames the closed / open mouth
shapes.)

The mid-battle **art clips** read the same two offsets through a
different entry: the anim commit `FUN_8004AD80` installs the art-bank
record's **embedded entry** (bank record `+0x24`) as the action-table
slot `0x10`/`0x11` pointer, so while the materialized art plays the
render node's `+0x4C` anim context is that entry and the animator's
track reads land at bank record `+0xB0` (eyes) / `+0xBC` (mouth) - see
[Art-animation bank](#art-animation-bank-record0-0x58). The art clips
are face-rich on disc: nearly every Vahn / Noa / Gala bank record
carries live records (32 of 33 / 33 of 35 / 30 of 32 records with
non-empty tracks); Terra's nine are all empty, matching her empty
record[0] tracks.

A record is active while `start <= clip_frame <= end` (`end != 0`,
counter clamped at `0xFE`); its `frame_id` selects a face frame from the
static per-character SCUS tables - eye-frame source x/y at
`DAT_80076824/26` (stride 4, eight frames per character, char stride
`0x20`), mouth frames at `DAT_80076884/86` (six per character, char
stride `0x18`), rect sizes + per-character destination offsets at
`DAT_800768CC` (eyes) / `DAT_800768E4` (mouth), all banded by the
per-slot origin deltas at `DAT_800768FC/FE` (3 slots - the member band
origins `(0x200 + p*0x80, 0x100)`). No active record selects frame 0
(the neutral face): when no record is active the neutral frame is
re-stamped instead, which is the steady state - the **idle entries'
tracks are empty in all four retail files** (resting faces are neutral;
the eye/mouth records live on the flinch / knockdown / recover / defeat
and equipment-swing entries). Character-record word `+0xF8` flag
`0x2000` - ability-bitfield (`+0xF4`) bit 45, the Rage passive (Evil
Medallion) - forces the neutral mouth frame. Each stamp is a libgpu
`MoveImage`
(`FUN_80058490`) from the frame strip (parked in the character's
texture band by the normal pool uploads) onto the live face rows of
section 1's rect - e.g. Vahn's eyes `(544,384) 15x17 → (512,272)` +
mouth `(544,452) 7x16 → (516,298)` in band slot 0, re-stamped every
frame (live-traced across a battle entry with
`autorun_battle_moveimage_trace.lua`).

During the battle-end **victory celebration** the mouth source switches.
`FUN_8004C7B4`'s override branch gates on four conditions: the battle-end
signal `DAT_8007BD71 == 0xFE` (the SM's `0x5A` monster-wipe arm / `0x66`
escape teardown), the victory sequencer `FUN_8004E568` running (its phase
halfword `ctx+0x6CE != 0`), the celebration flag `DAT_8007BD60` bit
`0x80` (set by the sequencer's asset-load step; explicitly cleared on a
party wipe, never set on an escape), and the actor's last-staged anim id
`actor[+0x1DB]` in `0x11..=0x18` - at victory time the staged **win
pose**, an HP-tier pick from the per-character id tables at
`DAT_800788A0/A2/A4` (a held debug pad combo on `_DAT_8007B850`
substitutes any of `0x11..0x18` directly). Inside the window the mouth
pass walks **sixteen** 3-byte records from the static table at
`0x80077E80`, indexed `char*0x180 + staged_id*0x30 + i*3` with the *raw*
band byte (the addressed rows start at `+0x330`; char stride `0x180` =
exactly 8 bands × `0x30`, so the 24 rows tile contiguously), and the
animator's frame counter - mouth **and** eye pass, which still reads the
entry's `+0x8C` records - becomes the global victory counter
`gp[+0x9EA] >> 1` (reset to 0 when the sequencer stages the win pose; its
per-frame incrementer is not in the dumped corpus), still clamped at
`0xFE`. The record shape and mouth-frame indexing are unchanged; the
retail rows only ever select non-neutral in-range mouth frames, some held
to end `0xFF` - the win-quote mouth flap. The sibling pass
`FUN_8004CCD4` (called right after) is **not a stamp** - it is the
per-frame equipment mesh-variant swap, decoded below
([Equipment-variant track](#equipment-variant-track-entry-0xa4--fun_8004ccd4)).
This resolves the historical "~220-byte facial-texel
overwrite" residue in the texture-placement validation: the overwrite is
the facial animator's current frame, and a character whose stamped frame
equals the pool default (Noa in the catalogued captures, Terra always)
shows no residue at all.

Parser + engine consumer: `legaia_asset::face_anim` carries the track
parser (`FaceTracks` / `battle_face_tracks`; the swing entries' tracks
ride on `battle_char_assembly::SwingAnimation::face` and the art-bank
embedded entries' on `battle_char_assembly::ArtAnimRecord::face`), the
SCUS table parsers (`FaceFrameTables::from_scus`, the override table as
`ArtMouthTables::from_scus` with an `ArtMouthTables::track` lookup keyed
by the staged id) and the retail stamp selection
(`FaceFrameTables::stamps` / `stamps_with_art_window`, which takes the
override track + the raw victory counter and applies the `>> 1` and the
clamp). The engine play-window battle path
registers each assembled member's tracks and re-stamps the current
eye/mouth frame per tick through `legaia_tim::Vram::move_image` (the
`MoveImage` port), keyed by the playing clip's `action_id` + keyframe
cursor - a staged id `>= 0x10` selects the art-bank record's embedded
tracks (the `FUN_8004AD80` entry-pointer rule above), every other id its
action slot's - so party members blink and mouth through their
reaction, swing and dynamic-art clips exactly like retail. When the battle ends in a monster wipe
while a member still plays a dynamic-art-slot clip (the engine carries
the staged id as the art-bank clip's `action_id`; the world's
`battle_end` latch mirrors the `0xFE` signal), the stamp tick opens the
override window and clocks a per-member `gp+0x9EA` mirror from 0 - the
sequencer-progress gates (`ctx+0x6CE`, `DAT_8007BD60` bit `0x80`) have
no engine counterpart yet, so "the won battle is still on screen" stands
in for them. Disc-gated validation:
`crates/asset/tests/face_anim_real.rs` (table anchors + track census -
record[0] entries, swing entries and the art-bank embedded entries -
plus the override-table census: 40 live records, in-range non-neutral
frames, the empty rows where retail has no flap, in-band stamps for
every reachable counter) and
`crates/engine-shell/tests/battle_face_stamp_live.rs` (live battle
VRAM holds a byte-exact stamped frame at the documented rects). The
`FUN_8004CCD4` equipment-variant swap (next section) is not modelled
in the engine.

### Equipment-variant track (entry `+0xA4`) + `FUN_8004CCD4`

The last 8 bytes of the `0xAC` action-entry header are a third per-clip
track: two 2-byte `[start_frame, end_frame]` windows per **variant pair**
(pair 0 at `+0xA4`/`+0xA6`, pair 1 at `+0xA8`/`+0xAA`; a window is active
while `start <= clip_frame <= end` with `end != 0` - the facial tracks'
activity rule, without the `frame_id` byte). The consumer is
`FUN_8004CCD4`, called back to back with the facial animator from the
render-node tick `FUN_80047430` under the same guards (party render
slots, Terra skipped, animator not paused) - but it is a **mesh swap,
not a texture stamp**: it writes Legaia-TMD object pointers into the
render node's per-channel model table (`*(node+0x44) + 4 + channel*4`,
the array the draw pass `FUN_80048A08` hands to `tmd_render` once per
animation channel).

The swappable objects are the sections' **surplus objects** (`nobj`
larger than the loader frame's attach count). The splice `FUN_800536BC`
tags each section's surplus `0xFF` (the first) / `0xFE` (the rest); the
post-pass `FUN_80053898` then:

- retags every `0xFE` into the `0x64` band, so the selection sort seats
  it **directly after the skeleton bones** - these are *extra animated
  parts*, driven in place by the extra stream channels that exist only
  in their own section's swing streams (the swing census's "up to +2
  channels"); it also records the pair ordinal (the count of `0xFF`
  objects seen so far) at `ctx+0x240+slot`;
- retags every `0xFF` into the `0xC8` band (post-sort table indices
  `nobj−2` / `nobj−1`, past every drawn channel - **never rendered
  directly**) and appends the preceding object's bone tag to the side
  table at `blob+nobj` - the variant's attach-bone channel.

At registration `FUN_800513F0` snapshots, per party slot, the two
attach-bone channel indices (`ctx+0x23A`/`+0x23B`) and the
default-vs-variant object-pointer pairs (`ctx+0x1030..0x103C`: default =
the bone's own object, variant = the `0xFF` object at `nobj−2`/`nobj−1`).
Per frame `FUN_8004CCD4` then picks which pointer sits in each attach
channel:

- **extra-channel escape**: if the playing stream's part count (first
  byte at `*(entry+0x88)`) differs from the idle stream's, the clip is
  one of the extra-channel swings - the pass force-installs the variant
  of the pair recorded at `ctx+0x240` and returns;
- otherwise the `ctx+0x240` pair is pinned to its default and every
  other pair follows the entry's `+0xA4` windows - variant inside a
  window, default outside.

The arts motion-trail renderer `FUN_80049348` re-runs the pass per
after-image ghost (history entries whose staged-anim-id byte, the actor
`+0x1FB` ring, is `> 0x10` - dynamic-art clips) with the ghost's
historical cursor (`+0x17A` ring) and entry (`+0x234` ring), so each
translucent trail copy shows the variant state of its own frame.

Retail census (all four player files): live `+0xA4` windows exist **only
in Noa's file** - her reaction entries 1..5/7..9, 28 of her 35 art-bank
records (every named art, e.g. `Vulture Blade` frames 3..47), and her
swing records; Vahn / Gala / Terra carry all-zero windows everywhere, so
for them the pass re-asserts the defaults every frame. The extra-channel
escape fires exactly where the disc has `0xFE` surplus: Noa's `0x1E`
weapon band (17-part swings vs 16 bones) and Gala's Ra-Seru Ozma high
tiers (16/17-part swings vs 15 bones). Decomps `8004ccd4.txt`,
`80047430.txt`, `80049348.txt`, `800536bc.txt`, `80053898.txt`,
`800513f0.txt`, `80048a08.txt`.

## Texture-pool VRAM placement

The battle-init texture upload is fully pinned. `FUN_80052FA0` runs once per
**present** party member; `p` below is the member's 0-based ordinal among the
present battle party (the band selector - *not* the character id). The
ordinal rule is live-verified for **all four playable characters**: a
Noa + Terra party capture (`terra_party_battle`) byte-matches both bands at
100% with Terra (char id 4, player file 0866) banding at her ordinal like
any other member - there is no special "4th band"
(`crates/engine-shell/tests/battle_char_texture_live.rs`). Per
member it issues up to seven upload blocks through `FUN_80053B9C`
(decomp `ghidra/scripts/funcs/80052fa0.txt` / `80053b9c.txt`):

1. **Two `record[0]` image blocks**, at the file header's `clut_a_off` /
   `clut_b_off` inside record[0]'s decoded output, with inline rects
   `(x0, y0, w, h)` = `(0x20, 0x80, 0x20, 0x80)` and `(0x60, 0x00, 0x20,
   0x80)` (both carry `clut_n = 0` in retail).
2. **One block per flagged equipment section** (the decoded slot's
   `upload_flag` at `+0x12`), at `decoded + tmd_body_end`, with the rect
   taken from the static `SCUS_942.54` table at **`0x800775B8`**
   (4 × u16 per section, indexed by equip-section 0..4):

   | section | `x0` | `y0` | `w` | `h` |
   |---|---|---|---|---|
   | 0 | `0x00` | `0x80` | `0x20` | `0x80` |
   | 1 | `0x00` | `0x00` | `0x40` | `0x80` |
   | 2 | `0x40` | `0x00` | `0x20` | `0x80` |
   | 3 | `0x40` | `0x80` | `0x20` | `0x80` |
   | 4 | `0x60` | `0x80` | `0x20` | `0x80` |

`FUN_80053B9C` reads the block's `[u16 clut_x][u16 clut_n]` prefix and issues
two `LoadImage`s (wrapper `FUN_800583C8`, literal `"LoadImage"` debug string):

- **CLUT**: rect `(clut_x, 0x1E1 + p, clut_n, 1)` from the `clut_n` entries,
  with the STP bit OR'd onto every non-zero colour (the same pass that fills
  the RAM palette block at `ctx + p*0x1E0 + 0x894`).
- **Pixels**: rect `(x0 + 0x200 + p*0x80, y0 + 0x100, w, h)` from the bytes
  after the CLUT run (`w` in VRAM halfwords).

The seven rects **tile the member's band exactly** - 128 halfwords × 256
rows at `x ∈ [0x200 + p*0x80, +0x80)`, `y ∈ [0x100, 0x200)`, i.e. texpages
`0x18 + 2p` / `0x19 + 2p` - precisely the pages + CLUT row the
registration-time mesh relocation `FUN_80053A28` retargets
([`character-mesh.md` § Battle render](character-mesh.md#battle-render-load-time-tsbcba-relocation)).
Unflagged sections upload nothing; their band area keeps whatever the other
blocks wrote (their pool bytes are overwritten in RAM by the next section's
decode without ever reaching VRAM).

**Validation** (disc + save-library gated
`engine-shell/tests/battle_char_texture_live.rs`): decoding the player files
with the live party ids (`DAT_8007BD10`) + equipped item ids (char record
`+0x196`) and comparing every block against captured battle VRAM reproduces
the bands at **99.7–100 %** per member across the `party_battle_gobu_gobu`
and `noa_levelup_fight_pre` captures (most blocks byte-exact). The residual
is a single ~220-byte cluster in section 1's rect (face rows), identical
across captures - the facial animator's current frame, stamped over the
pool default every frame (see
[Facial animation tracks](#facial-animation-tracks-entry-0x8c--0x98)),
not a placement error. (A first-battle-frame capture, before the upload
pass runs, still shows field texels in the band.)

Typed port: `legaia_asset::battle_char_assembly` -
`SECTION_TEXTURE_RECTS` / `RECORD0_TEXTURE_RECTS` /
`parse_upload_block` / `section_texture_upload` /
`record0_texture_uploads` / `character_texture_uploads`. The engine
play-window battle path uploads these blocks for each assembled member
(PROT 1204's atlases remain the fallback approximation).

## Parser status

Two parsers read these files:

- [`legaia_asset::battle_char_palette`](../../crates/asset/src/battle_char_palette.rs)
  implements the runtime-pinned framing above (header words, descriptor
  chain, `record[0]` + sub-record palette assembly; byte-exact vs live battle
  VRAM).
- [`legaia_asset::battle_char_assembly`](../../crates/asset/src/battle_char_assembly.rs)
  ports the battle-init consumer chain: equipment-section selection
  (`select_sections`), mesh splice (`assemble_character`), the TSB/CBA
  relocation (`relocate_tsb_cba`), and the texture-pool uploads at the
  pinned placement (`character_texture_uploads` and friends - see
  [Texture-pool VRAM placement](#texture-pool-vram-placement)).
- [`legaia_asset::battle_data_pack`](../../crates/asset/src/battle_data_pack.rs)
  (the TMD-slot walker) reads the same descriptor table in the
  `[id, offset, size]` frame above. Detection validates the chain invariant
  (entry 0 at offset 0, `offset[i+1] == offset[i] + size[i]`, sector-aligned
  sizes, all-zero terminator) plus the header-word ordering
  (`clut_a < clut_b < budget`), which accepts all four retail player files -
  including Terra's 0866, whose table is all-default (`id = 0`) entries -
  and rejects every other PROT entry. An earlier revision of this walker
  read the table through a 4-byte-shifted frame (entry 0's `id` as a "record
  count", sizes paired off by one slot); its observations "the table is
  sized to a maximum and zero-padded", "0866 has a zero count in the
  canonical position" and "the last 0865 slot over-runs the footprint" were
  all artifacts of that shifted frame. Under the correct frame 0866 parses
  like its siblings and all four files tile their footprints exactly.

## VRAM byte-match corpus

The principled tool for pinning the texture-pool descriptor is byte-matching:
slide a 32-byte halfword-aligned window over each decoded slot's post-TMD
bytes and search a mednafen-captured VRAM blob for exact matches; each hit
yields `(slot, slot_offset, fb_x, fb_y)`. Driver: `mednafen-state clut-trace`
(see [CLI](#cli)); analysis API `battle_data_pack::find_clut_in_vram`.

Findings from a four-save corpus over Gala's file (0865; saves: Rim Elm town,
Izumi town, pre-battle, active battle):

| Slot (table entry) | Header signature | VRAM placement (fb_x, fb_y range) |
| ------ | ---------------- | --------------------------------- |
| id 0x66 | `..., 0x010000, 0x0b0a0906, 0x000e0d0c, ...` | (864, 426..433) - town only |
| id 0x00 (last section default) | `..., 0x010000, 0x0b0a0906, 0x000e0d0c, ...` | (864, 388..507) - town only |
| id 0x54 | `..., 0x010000, 0x010002, 0x000000, ...` | (768, 441) - battle only |
| id 0x53 | `..., 0x010000, 0x010002, 0x000000, ...` | (768, 393..441) - battle |
| id 0x00 (first section default) | `..., 0x010000, 0x010002, 0x000000, ...` | (768, 385..496) - battle |
| ids 0x42..0x3f | `..., 0x010000, 0x000201, 0x000000, ...` | (768, 272..310) - battle |
| id 0x00 (second section default) | `..., 0x010000, 0x000201, 0x000000, ...` | (768, 272..331) - battle |

Consecutive slot offsets step by `0x40` per `+1` in `fb_y`: the post-TMD pool
uploads as a 32-halfword-wide (128 px @ 4bpp) contiguous block. The corpus
could not recover per-slot `(fb_x, fb_y)` from the on-disc bytes because the
placement is **per-section, not per-slot** - every slot of a section shares
the section's static rect, banded by the party ordinal (resolved in
[Texture-pool VRAM placement](#texture-pool-vram-placement); the corpus rows
above are exactly those rects for Gala in band `p = 2`, with overlapping
hits where equipment variants share texels).

**Not in these files: the row-479 NPC palettes.** The town NPC CLUTs at
row 479 byte-match no decoded slot of any player file (nor any raw PROT entry
or `SCUS_942.54` as an 8-byte prefix). They are plain PSX TIMs in each
scene's own `scene_tmd_stream` entries, uploaded by `FUN_8001FE70` at battle
init - see [`npc-palette.md`](npc-palette.md). The engine consequence (field
scene-loads exclude these packs from VRAM entirely) is wired through
[`SceneResources::SceneLoadKind`](../../crates/engine-core/src/scene_resources.rs).

## CLI

```bash
# Inspect one player file's TMD-slot table.
asset battle-data-pack extracted/PROT/0865_battle_data.BIN

# Dump every decoded slot to a directory.
asset battle-data-pack extracted/PROT/0865_battle_data.BIN --out /tmp/0865_records

# Bulk-scan a directory of PROT entries for this shape.
asset battle-data-pack-scan extracted/PROT --cdname extracted/CDNAME.TXT

# Byte-match decoded slots against PSX VRAM in mednafen save states.
mednafen-state clut-trace \
  --pack extracted/PROT/0865_battle_data.BIN \
  --json /tmp/clut_corpus.json \
  ~/.mednafen/mcs/Legend\ of\ Legaia*.mc2 \
  ~/.mednafen/mcs/Legend\ of\ Legaia*.mc6
```

(The CLI names keep the historical "battle-data-pack" spelling; they operate
on the player files.)

## Open questions

- ~~**Per-texture descriptor / placement**~~ **resolved**: the placement is
  per-*section*, from the static rect table at `0x800775B8` + the
  party-ordinal band - see
  [Texture-pool VRAM placement](#texture-pool-vram-placement). The residual
  facial-texel overwrite is ~~narrowed~~ **resolved**: it is the per-frame
  facial animator `FUN_8004C7B4` stamping the current eye + mouth frames
  over section 1's face rows via `MoveImage`, driven by the action-entry
  facial tracks at `+0x8C`/`+0x98` - see
  [Facial animation tracks](#facial-animation-tracks-entry-0x8c--0x98).
  (The earlier "one-shot at init" reading came from tracing a summon
  mid-cast window, where the animator is paused; a battle-entry trace
  from `karisto_sol_pre_encounter` shows it re-stamping every frame.)
  The sibling-pass residue is **resolved**: `FUN_8004CCD4` is not a
  stamp - it is the per-frame equipment mesh-variant swap driven by the
  entry's third track at `+0xA4`, running under the same caller guards
  plus a per-ghost re-run in the arts trail renderer - see
  [Equipment-variant track](#equipment-variant-track-entry-0xa4--fun_8004ccd4).
- ~~**Slot id ↔ equipment id mapping**~~ **resolved**: the section ids ARE
  item-table ids and the `FUN_80052770` case-4 picker matches them against
  the character record's equipped-item bytes (see
  [Descriptor table](#descriptor-table)). The battle `nobj +2` weapon
  objects source from these sections too - byte-verified, see
  [`character-mesh.md` § Battle form](character-mesh.md#battle-form---assembled-from-the-player-files).
- ~~**`data_base` derivation**~~ **resolved**: `0x8000` is not header-derived -
  `FUN_80052770`'s fixed 16-sector prologue read (`FUN_800559EC(…, 0x8000)` →
  `FUN_8003E800(dst, 0x10, 1)`) hardcodes it. See
  [File layout](#file-layout).
- ~~**Sub-object end offsets** (`u32[1]`, `u32[2]`)~~ **resolved**: they are
  the section's **swing action records** (the earlier "multi-mesh slot"
  reading of a Gala slot with `u32[1] = 0x3310` was this swing record -
  sec-2 id `0x21`'s entry at `0x3310` parses as a 15-part/17-frame stream).
  See [Swing records](#swing-records-equipment-sections--slots-0xc0xf).
- ~~**record[0] `+0x5C` consumer**~~ **reframed - vestigial**: the word is
  rebased self-relative→absolute at load by `FUN_80052FA0` (`:561`)
  **alongside** the `+0x58` art-bank pointer (`:558`), but unlike `+0x58` (read
  by `FUN_8004AD80`) it has **no reader in the code searched** - word-wise
  exhaustive over `SCUS_942.54` plus the 15 extracted overlay binaries, with 11
  overlay images still dump-only ([coverage](#the-0x5c-no-reader-sweep)) - and
  the word (slot `0x17`) sits outside every action-table
  consumer's range. Target is `clut_a_off − 4` (zero on disc), and the CLUT
  upload uses `file+0x04`/`+0x08`, not this field. So it is a rebased-at-load
  paired-relocation field, not untraced-dead; a read-watchpoint would only
  confirm the deadness (low-yield). The art `"ME"`-archive hypothesis is
  separately refuted (the archives are in `readef.DAT`).
- ~~**Art-archive slot staging**~~ **resolved**: the main-vs-base pick is
  per battle phase, not per record. The initiative scheduler `FUN_801DABA4`
  stages the acting character's group each turn (the applier SM stops after
  slot `base+1`, leaving the **main** archive resident); the battle-end arms
  (`FUN_801DABA4`'s no-living-enemy branch + `FUN_801E295C`'s victory arm)
  directly request slot `3*char+2`, so the win poses (ids `0x11..=0x18` =
  the eight `rate_alt == 0xFF` bank records 1..8) decode from the **base**
  archive. The exact cover is the consequence - see
  ["ME" stream archives](#me-stream-archives-readefdat).

## See also

- [`character-mesh.md`](character-mesh.md) - the battle-form meshes + the fully decoded palette chain these files feed.
- [`monster-animation.md`](monster-animation.md) - the monster archive (extraction 0867) this page is *not* about.
- [Legaia TMD](tmd.md) - the mesh embedded in each slot.
- [LZS compression](lzs.md) - the per-slot decompression stage.
- [`subsystems/battle.md`](../subsystems/battle.md) - the battle scene loaders.
- [`cdname.md` § numbering space](cdname.md#numbering-space) - the index-space correction this page applies.
