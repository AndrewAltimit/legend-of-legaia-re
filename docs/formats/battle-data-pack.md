# Battle-data pack format

The custom container used by PROT entries in the `battle_data` CDNAME block
(PROT 0865) and a small number of sister blocks (e.g. `edstati3` at PROT 0863).
Each pack holds 30-90 character / monster TMDs with their texture pools, all
wrapped in per-record LZS streams.

This format is **distinct from**:

- the standalone [TIM-pack](tim-pack.md) used by some other PROT entries,
- the [DATA_FIELD streaming format](data-field.md) used by scene bundles,
- the [field-pack](field-pack.md) and [effect-bundle](effect.md) containers.

Implementation: [`crates/asset/src/battle_data_pack.rs`](../../crates/asset/src/battle_data_pack.rs).

## Outer layout

```
+0x0000  u32 chunk0_header     ; (type=0x00 << 24) | first_chunk_size
+0x0004  ...chunk0 payload...  ; opaque streaming data
                                ; (chunk0 payload's last u32 holds the trailer record count)
+chunk0_size + 4   u32         ; streaming-format terminator (low24=0)

+chunk0_size       u32 record_count       ; e.g. 0x57 = 87 slots
+chunk0_size + 4   u32 reserved           ; always 0
+chunk0_size + 8   Record[record_count]   ; 12 bytes each

data_base  ; next 0x800-aligned offset that satisfies the per-record `dec_size` sanity check
data_base + Record[i].data_offset  ; compressed entry for record i
```

The chunk0 header at offset 0 carries the trailer record count in its last u32 -
the count is simultaneously the streaming chunk's final payload word and the
trailer-table count. The chunk-stream terminator at `chunk0_size + 4` lets the
runtime's streaming walker stop cleanly without ever inspecting the trailer.

## Record (12 bytes)

```
u32 on_disc_size  ; allocation footprint of the compressed entry (NOT the LZS stream length)
u32 id            ; slot id (0..0x7F observed); 0 marks an empty/filler slot
u32 data_offset   ; byte offset from `data_base`
```

`on_disc_size` is the slot's reservation, not the LZS stream length. The LZS
decoder stops based on its output count (`dec_size`), so the compressed input
often spills past `on_disc_size` into the next slot's region. Decoders MUST
hand the decompressor a generous source slice (e.g. the entire remainder of
the file from `data_base + data_offset + 4`) rather than truncating to
`on_disc_size - 4`.

The table is sized to the maximum slot count the engine ever loads. Real
files use only the first 30-50 records and zero-pad the rest. The parser
stops at the first zero-`on_disc_size` row.

A slot with `id = 0` and `data_offset = 0` is a **filler** entry. Retail
0865 has one such entry (rec 42) that points back into the table-padding
region. Don't decode filler slots.

## Compressed entry

At file offset `data_base + record.data_offset`:

```
u32 decompressed_size       ; output byte count
LZS stream                  ; legaia LZS; see lzs.md
```

The LZS stream is the standard Legaia LZS as used everywhere else, with
the same 4 KB zero-initialised ring buffer.

## Decompressed entry layout

```
+0x00  u32 magic_or_count    ; 0x14 (= 20) in every observed 0865 record
+0x04  u32 sub_obj0_end      ; nested-section end offset within decoded buffer; often 0
+0x08  u32 sub_obj1_end      ; nested-section end; non-zero in records with multiple sub-meshes
+0x0C  u32 tmd_body_end      ; offset where the embedded Legaia TMD ends
+0x10  u32                   ; per-texture flag (typically 0x010000 / 0x010002)
+0x14  u32                   ; texture format tag (typically 0x010002 / 0x05040303)
+0x18  u32                   ; sometimes 0; sometimes a packed (slot, bpp) tag
+0x1C  u32                   ; offset to start of CLUT/texture pool (~= tmd_body_end - 0x20)
+0x20  Legaia TMD            ; magic 0x80000002, custom Legaia variant (see tmd.md)
+tmd_body_end                ; texture / CLUT pool
```

The 32-byte header acts as a layout descriptor for the post-TMD texture
pool. Fields at `+0x10..0x20` correlate with the slot ids the TMD's
primitives reference, but the exact semantics aren't fully pinned -
descending into the pool to extract individual CLUTs requires more
reverse engineering. See *Open layout questions* below.

### TMD location

The TMD is at `+0x20` for every simple-shape record. Some records have
nested sub-meshes and the TMD shifts later in the buffer; the locator in
`legaia_asset::battle_data_pack` first tries `+0x20` and then falls back
to a word-aligned magic scan.

### Post-TMD texture pool

The bytes after the TMD hold packed texture and palette data. Empirically:

- The first 32 bytes after the TMD form a valid 16-color RGB1555 CLUT row
  (verified by checking the high-transparency bit on every halfword).
- Larger 4bpp pixel regions follow, interspersed with more 32-byte CLUT-shaped
  runs.

The pool layout doesn't use standard PSX TIM image-block headers. The
runtime presumably uses the descriptor at `u32[3..0x20]` of the entry
header to know where to DMA each slot into VRAM. Until that descriptor's
fields are pinned, the
[`legaia_asset::battle_data_pack::probe_first_clut_run`](../../crates/asset/src/battle_data_pack.rs)
helper just locates the first CLUT-shaped run by structural heuristic.

## Why this matters

Town01's four NPC TMDs reference CLUT row y=479 slots x=128..240
(CBA `0x77C8..0x77CF`). Those palettes live inside the post-TMD pool of
one or more `battle_data` records. Without descending into this pack the
raw TIM scanner finds zero TIMs in 0865 (the data is wrapped in this
custom format) and the engine's targeted-upload path leaves the row
unsupplied, dropping ~388 prims as MissingClut.

The pack parser (this format) is the entry point for closing that gap.
Once the post-TMD layout descriptor is pinned, the engine can extract
specific CLUTs and upload them at the right VRAM coordinates.

## CLI

```bash
# Inspect one pack-shaped PROT entry.
asset battle-data-pack extracted/PROT/0865_battle_data.BIN

# Dump every decoded record to a directory.
asset battle-data-pack extracted/PROT/0865_battle_data.BIN --out /tmp/0865_records

# Bulk-scan a directory of PROT entries for this format.
asset battle-data-pack-scan extracted/PROT --cdname extracted/CDNAME.TXT
```

## Open layout questions

- **Sub-object end offsets** (`u32[1]`, `u32[2]`): meaning of the nested
  section pointers in records with multiple sub-meshes (e.g. rec 10 in
  0865 with `u32[1] = 0x3310`). Likely a multi-mesh record holding several
  TMDs back-to-back, but the offset stride hasn't been validated against
  every variant.

- **Per-texture descriptor** (`u32[4]..u32[7]`): the values `0x010000`,
  `0x010002`, `0x05040303`, `0x0b0a0906`, etc. look like packed
  `(slot, bpp)` tuples or per-CLUT VRAM coordinates. Pinning the exact
  encoding would let the engine compute the (fb_x, fb_y) for each CLUT
  in the post-TMD pool and upload them directly.

- **Texture-pool block format**: image blocks in the pool don't carry
  PSX TIM headers (no leading `0x00000010` magic). The pool is a sequence
  of raw 4bpp pixel pages + 32-byte CLUTs with no per-block size field,
  so a parser needs the header descriptor to know where each block ends.

- **0866 / 0867 / 0868 / 0869 layouts**: PROT 0866 has the same outer
  shape as 0865 but the count u32 is zero in the canonical position -
  records appear to start directly at `chunk0_size + 8` without the
  count + reserved preamble. 0867 / 0868 carry VAB sound banks (0867
  is more complex; 0868 + 0869 are plain VABp banks). Only 0865 (and
  the sister 0863 `edstati3` entry) match the format documented here.

- **Runtime asset-loader chain**: `FUN_8001E890` is the data-field-player
  loader (see [`asset-loader.md`](../subsystems/asset-loader.md)) - it
  reads `data\field\player.lzs` and registers the embedded TMDs into
  `0x8007C018 + idx*4` via `FUN_80026B4C`. The battle_data pack might
  feed into the same registry through a sister loader; tracing where the
  battle scene loader registers character TMDs would close the gap.
