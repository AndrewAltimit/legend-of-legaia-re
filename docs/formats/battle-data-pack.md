# Player battle files (`data\battle\PLAYER1..4`)

The per-character battle asset files for Vahn / Noa / Gala / Terra — the retail
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
**raw in-RAM TOC index** `char_id + 0x360` — extraction entry
`char_id + 0x360 − 2` (see [`prot.md` § In-RAM TOC](prot.md#in-ram-toc)):

| Player | Raw TOC index | PROT.DAT offset | Footprint | Extraction entry |
|---|---|---|---|---|
| Vahn  | `0x361` | `0x36E8000` | 338 sectors (`0xA9000`) | 0863 (`edstati3` label) |
| Noa   | `0x362` | `0x3791000` | 303 sectors (`0x97800`) | 0864 (`edstati3` label) |
| Gala  | `0x363` | `0x3828800` | 222 sectors (`0x6F000`) | 0865 |
| Terra | `0x364` | `0x3897800` |  47 sectors (`0x17800`) | 0866 |

The offsets are the live-traced `FUN_800558FC` reads (see
[`character-mesh.md` § Battle form](character-mesh.md#battle-form--prot-1204))
and equal the TOC `start_lba × 0x800` of extraction 863..866 exactly. The
historical "Vahn = PROT 0861" attribution matched the same bytes through the
1-sector stub entries 0859..0862 that precede the true file — entry 0861's
*extended* window reaches Vahn's file at window offset `0x1000`.

`FUN_80052FA0` then decodes `record[0]` + its sub-records into the battle
party palette (rows 481..483); the TMD slots install through the battle
loaders. Full palette chain: [`character-mesh.md` § Battle render](character-mesh.md#battle-render-load-time-tsbcba-relocation).

## TOC geometry (the 16 MB misreading)

The TOC declares extraction 0865 with `indexed_size = 7811` sectors
(`0xF41800` ≈ 16.0 MB) against a 222-sector footprint. That extended window
covers Gala's own file (`0x0..0x6F000`), all of Terra's (`0x6F000..0x86800`),
and 7542 of the monster archive's 7760 sectors (`0x86800..`). The extractor's
`0865_battle_data.BIN` is therefore a 16 MB file whose first 222 sectors are
the actual player file — the earlier "16 MB battle_data container" reading
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
  (`+0x00 name_offset`, `+0x0C` HP, `+0x4C` action-offset array — see
  [`monster-animation.md`](monster-animation.md)); the player-file slot head
  is the 32-byte texture-layout header below, with the TMD at `+0x20`.
- Within extraction 0865's extended window the archive begins at byte
  `0x86800`; the player-file descriptor table (`0x6C68`) and slot region
  (`0x8000..0x6F000`) sit entirely before it.

The old conflation ("battle_data 0865 vs monster archive 0867") came from the
overlapping extraction windows; the [CDNAME −2 correction](cdname.md#numbering-space)
resolves it — the dev names say exactly what each entry is.

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
and `0x8000` is zero-padded). The exact derivation of `data_base` from the
header is not pinned; `legaia_asset::battle_data_pack` self-corrects it by
probing sector boundaries until every slot's `dec_size` prefix reads sane.

## Descriptor table

At `desc_off`, a chained array of 12-byte entries:

```
u32 id       ; slot id; 0 marks a section boundary / default-variant slot
u32 offset   ; byte offset of the slot from data_base
u32 size     ; slot allocation in bytes (sector-aligned)
```

The chain invariant `offset[i+1] == offset[i] + size[i]` holds across every
entry; an all-zero entry terminates the table. Entries group into **sections
of descending ids separated by `id = 0` entries** — e.g. Gala (0865):

```
57 56 55 54 53 | 00 | 42 41 40 3f | 00 | 21 20 27 26 25 24 23 22
2b 2a 29 28 33 32 31 30 2f 2e | 00 | 19 18 17 16 15 14 13 | 00 |
69 68 67 66 | 00
```

Terra (0866) carries only five `id = 0` entries — no variant slots. Per the
runtime palette consumer (`FUN_80052770` case 4, see
[`character-mesh.md`](character-mesh.md#battle-form--prot-1204)), each section
ships one record per **equipment id** plus the `id = 0` default, and the
loader picks the equipment-id-matched entry or the default per section. The
mapping from these ids to item-table ids is not yet pinned (open question).

## Slot region

At `data_base + entry.offset`:

```
u32 decompressed_size       ; LZS output-byte budget
LZS stream                  ; standard Legaia LZS (see lzs.md)
```

The decoder stops on the output count, not the input length — hand it a
generous source slice rather than truncating to `entry.size`.

## Decompressed slot layout

```
+0x00  u32 magic_or_count    ; 0x14 (= 20) in every observed slot
+0x04  u32 sub_obj0_end      ; nested-section end offset; often 0
+0x08  u32 sub_obj1_end      ; nested-section end; non-zero in multi-mesh slots
+0x0C  u32 tmd_body_end      ; offset where the embedded Legaia TMD ends
+0x10  u32                   ; per-texture flag (typically 0x010000 / 0x010002)
+0x14  u32                   ; texture format tag
+0x18  u32                   ; sometimes 0; sometimes a packed (slot, bpp) tag
+0x1C  u32                   ; offset to start of CLUT/texture pool (~= tmd_body_end - 0x20)
+0x20  Legaia TMD            ; magic 0x80000002 (see tmd.md)
+tmd_body_end                ; texture / CLUT pool
```

The 32-byte header is a layout descriptor for the post-TMD texture pool. The
pool has no PSX TIM image-block headers: it is raw 4bpp pixel pages
interleaved with 32-byte CLUT rows. Flagged slots additionally carry a
trailing CLUT struct (`[u16 base][u16 count][count × BGR555]`) that the
palette chain STP-copies to VRAM rows 481..483 — that path is fully decoded in
[`character-mesh.md`](character-mesh.md#battle-render-load-time-tsbcba-relocation)
and ported as `legaia_asset::battle_char_palette`. The header's
`u32[4..7]` texture-placement encoding is **not** pinned (see
[VRAM byte-match corpus](#vram-byte-match-corpus)).

## Parser status

Two parsers read these files:

- [`legaia_asset::battle_char_palette`](../../crates/asset/src/battle_char_palette.rs)
  implements the runtime-pinned framing above (header words, descriptor
  chain, `record[0]` + sub-record palette assembly; byte-exact vs live battle
  VRAM).
- [`legaia_asset::battle_data_pack`](../../crates/asset/src/battle_data_pack.rs)
  (the TMD-slot walker) reads the same descriptor table in the
  `[id, offset, size]` frame above. Detection validates the chain invariant
  (entry 0 at offset 0, `offset[i+1] == offset[i] + size[i]`, sector-aligned
  sizes, all-zero terminator) plus the header-word ordering
  (`clut_a < clut_b < budget`), which accepts all four retail player files —
  including Terra's 0866, whose table is all-default (`id = 0`) entries —
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
| id 0x66 | `..., 0x010000, 0x0b0a0906, 0x000e0d0c, ...` | (864, 426..433) — town only |
| id 0x00 (last section default) | `..., 0x010000, 0x0b0a0906, 0x000e0d0c, ...` | (864, 388..507) — town only |
| id 0x54 | `..., 0x010000, 0x010002, 0x000000, ...` | (768, 441) — battle only |
| id 0x53 | `..., 0x010000, 0x010002, 0x000000, ...` | (768, 393..441) — battle |
| id 0x00 (first section default) | `..., 0x010000, 0x010002, 0x000000, ...` | (768, 385..496) — battle |
| ids 0x42..0x3f | `..., 0x010000, 0x000201, 0x000000, ...` | (768, 272..310) — battle |
| id 0x00 (second section default) | `..., 0x010000, 0x000201, 0x000000, ...` | (768, 272..331) — battle |

Consecutive slot offsets step by `0x40` per `+1` in `fb_y`: the post-TMD pool
uploads as a 32-halfword-wide (128 px @ 4bpp) contiguous block. Within a
header-signature cluster the per-slot `(fb_x, fb_y)` is *not* recoverable
from the on-disc bytes — the placement is runtime-resolved; tracing that
resolver is the open step.

**Not in these files: the row-479 NPC palettes.** The town NPC CLUTs at
row 479 byte-match no decoded slot of any player file (nor any raw PROT entry
or `SCUS_942.54` as an 8-byte prefix). They are plain PSX TIMs in each
scene's own `scene_tmd_stream` entries, uploaded by `FUN_8001FE70` at battle
init — see [`npc-palette.md`](npc-palette.md). The engine consequence (field
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

- **Per-texture descriptor** (`u32[4]..u32[7]`): slots sharing an identical
  signature land at different VRAM coords, so the placement is
  runtime-resolved — trace the battle-init upload path that consumes the
  header.
- **Slot id ↔ equipment id mapping**: the section ids plausibly index the
  equipment tables (each section = one equip slot's variants, per the
  `FUN_80052770` case-4 picker); pinning id → item-table id needs a capture
  with a known equipment change. Related open thread: the battle `nobj +2`
  weapon objects (**D-WEAP**, [`character-mesh.md`](character-mesh.md#equipment-groups-battle-only))
  plausibly source from these slots — unverified.
- **`data_base` derivation**: observed `0x8000` in all four files; the
  header/table → `0x8000` rule is unconfirmed.
- **Sub-object end offsets** (`u32[1]`, `u32[2]`): multi-mesh slots (e.g. a
  Gala slot with `u32[1] = 0x3310`) hold several TMDs back-to-back; the
  stride isn't validated across every variant.

## See also

- [`character-mesh.md`](character-mesh.md) — the battle-form meshes + the fully decoded palette chain these files feed.
- [`monster-animation.md`](monster-animation.md) — the monster archive (extraction 0867) this page is *not* about.
- [Legaia TMD](tmd.md) — the mesh embedded in each slot.
- [LZS compression](lzs.md) — the per-slot decompression stage.
- [`subsystems/battle.md`](../subsystems/battle.md) — the battle scene loaders.
- [`cdname.md` § numbering space](cdname.md#numbering-space) — the index-space correction this page applies.
