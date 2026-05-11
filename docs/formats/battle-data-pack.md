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
header to know where to DMA each slot into VRAM, but the descriptor
*encoding* has not been pinned by the on-disc bytes alone. Until that
encoding (or its runtime resolver) is reverse-engineered, the
[`legaia_asset::battle_data_pack::probe_first_clut_run`](../../crates/asset/src/battle_data_pack.rs)
helper locates the first CLUT-shaped run by structural heuristic, and
[`legaia_asset::battle_data_pack::clut_uploads`](../../crates/asset/src/battle_data_pack.rs)
is a documented no-op stub the engine wires into its CLUT pass so
descriptor decoding lights up VRAM uploads without a separate
integration step.

## Pinning the descriptor: VRAM byte-match corpus

The principled tool for pinning the descriptor is byte-matching: take
each decoded record's post-TMD bytes, slide a 32-byte halfword-aligned
window over them, and search a mednafen-captured VRAM blob for an exact
match. Each hit yields a `(record_idx, record_offset, fb_x, fb_y)`
tuple - a corpus of those tuples narrows the encoding.

The analysis API:

```rust
let pack = battle_data_pack::parse(&prot_entry_bytes)?;
let decoded = battle_data_pack::decode_record(&prot_entry_bytes, &pack, idx)?;
let hits = battle_data_pack::find_clut_in_vram(&decoded, &mednafen_vram_bytes);
for hit in &hits {
    println!("rec_off=0x{:x} -> fb=({}, {})",
             hit.record_byte_offset, hit.fb_x, hit.fb_y);
}
```

The CLI driver that feeds this against a PROT entry + a list of save
states:

```bash
mednafen-state clut-trace \
  --pack extracted/PROT/0865_battle_data.BIN \
  --json /tmp/clut_corpus.json \
  ~/.mednafen/mcs/Legend\ of\ Legaia\ \(USA\).*.mc2 \
  ~/.mednafen/mcs/Legend\ of\ Legaia\ \(USA\).*.mc6
```

### Findings from a four-save corpus

Sliding the byte-match across PROT 0865 against four save states
(`mc2` = Rim Elm town01 with NPCs, `mc3` = Izumi town, `mc4` =
pre-battle load, `mc6` = active battle) yields:

| Record | Header signature | VRAM placement (fb_x, fb_y range) |
| ------ | ---------------- | --------------------------------- |
| 40 (id 0x66) | `..., 0x010000, 0x0b0a0906, 0x000e0d0c, ...` | (864, 426..433) — town only |
| 41 (id 0x00) | `..., 0x010000, 0x0b0a0906, 0x000e0d0c, ...` | (864, 388..507) — town only |
| 2 (id 0x54)  | `..., 0x010000, 0x010002, 0x000000, ...`     | (768, 441) — battle only |
| 3 (id 0x53)  | `..., 0x010000, 0x010002, 0x000000, ...`     | (768, 393..441) — battle |
| 4 (id 0x00)  | `..., 0x010000, 0x010002, 0x000000, ...`     | (768, 385..496) — battle |
| 5-8 (id 0x42..0x3f) | `..., 0x010000, 0x000201, 0x000000, ...` | (768, 272..310) — battle |
| 9 (id 0x00)  | `..., 0x010000, 0x000201, 0x000000, ...`     | (768, 272..331) — battle |

Consecutive record offsets step by `0x40` for each `+1` in `fb_y`,
confirming the post-TMD pool is uploaded as a 32-halfword-wide
(128-pixel-wide @ 4bpp) contiguous block at `(fb_x, fb_y_base)`. Within
each header-signature cluster the per-record (fb_x, fb_y) placement is
*not* recoverable from the on-disc bytes alone - the encoding of
`u32[5..7]` doesn't appear to be a direct `(fb_x, fb_y)` packing.

### What's *not* in the pack: the NPC palettes at row 479

The four town01 NPC TMDs sample CLUTs at CBAs `0x77C8..0x77CF`, which
decode to fb_x=128..240 at row 479. The actual 32-byte palette payloads
at those VRAM positions are **not present verbatim** in any decoded
battle_data record:

- A direct byte-match of each row-479 slot's 32 bytes against every
  decoded record in PROT 0865 returns zero hits.
- An 8-byte prefix of the same slot bytes is not present in any raw
  PROT entry (0865-0868 inclusive) or in `SCUS_942.54`.
- The CLUT halfwords themselves form a structured hue cycle (HSV-style
  rainbow at constant value) - consistent with a runtime *palette
  generator* rather than an on-disc payload.

So the source of the town01 NPC palettes is *external* to the
battle_data pack. The full picture is documented in
[`npc-palette.md`](npc-palette.md): row 479 is populated by plain PSX
TIMs that live inside the scene's [`scene_tmd_stream`](scene-bundles.md)
PROT entries (e.g. `0006_town01.BIN @ 0x1ee4c` for town01), wrapped
in a type-0x01 chunk header and uploaded by `FUN_8001FE70` during
battle init (field/town scene-load does not touch them). The
engine's targeted-upload CLUT pass picks them up naturally with
merge-zeros semantics so multiple scene-pack TIMs targeting the same
row coexist.

## Why this matters

Town01's four NPC TMDs reference CLUT row y=479 slots x=128..240
(CBA `0x77C8..0x77CF`). Earlier engine work assumed those palettes
live inside the post-TMD pool of one or more `battle_data` records;
the byte-match corpus above shows they don't. Closing the ~388-prim
MissingClut gap therefore requires more than decoding the pack
descriptor - it needs the runtime palette source identified
separately. The pack parser remains the entry point for CLUTs in
*battle* scenes (where on-disc palettes do drive the renderer) once
the descriptor is pinned.

## CLI

```bash
# Inspect one pack-shaped PROT entry.
asset battle-data-pack extracted/PROT/0865_battle_data.BIN

# Dump every decoded record to a directory.
asset battle-data-pack extracted/PROT/0865_battle_data.BIN --out /tmp/0865_records

# Bulk-scan a directory of PROT entries for this format.
asset battle-data-pack-scan extracted/PROT --cdname extracted/CDNAME.TXT

# Byte-match decoded records against PSX VRAM in mednafen save states.
# Emits one `(record, fb_x, fb_y)` row per hit; with `--json`, also
# writes the corpus structured. Useful when reverse-engineering the
# post-TMD descriptor.
mednafen-state clut-trace \
  --pack extracted/PROT/0865_battle_data.BIN \
  --json /tmp/clut_corpus.json \
  ~/.mednafen/mcs/Legend\ of\ Legaia*.mc2 \
  ~/.mednafen/mcs/Legend\ of\ Legaia*.mc6
```

## Open layout questions

- **Sub-object end offsets** (`u32[1]`, `u32[2]`): meaning of the nested
  section pointers in records with multiple sub-meshes (e.g. rec 10 in
  0865 with `u32[1] = 0x3310`). Likely a multi-mesh record holding several
  TMDs back-to-back, but the offset stride hasn't been validated against
  every variant.

- **Per-texture descriptor** (`u32[4]..u32[7]`): the byte-match corpus
  above shows the descriptor *encoding* doesn't map directly onto the
  empirical `(fb_x, fb_y)` placements - records that share an identical
  `u32[5..7]` signature still land at different VRAM coords. The
  placement is likely *runtime-resolved*: a separate dispatch table or
  asset-loader function consults the descriptor bytes plus the record
  id, the current scene mode, or both. Tracing the runtime resolver
  through an overlay sweep is the next step.

- **Texture-pool block format**: image blocks in the pool don't carry
  PSX TIM headers (no leading `0x00000010` magic). The pool is a sequence
  of raw 4bpp pixel pages + 32-byte CLUTs with no per-block size field,
  so a parser needs the header descriptor to know where each block ends.
  The byte-match corpus shows that the pool is uploaded as a contiguous
  128-pixel-wide @ 4bpp block (stride = 64 record bytes per VRAM row),
  but the per-block subdivision inside that span is still TBD.

- **0866 / 0867 / 0868 / 0869 layouts**: PROT 0866 has the same outer
  shape as 0865 but the count u32 is zero in the canonical position -
  records appear to start directly at `chunk0_size + 8` without the
  count + reserved preamble. 0867 / 0868 carry VAB sound banks (0867
  is more complex; 0868 + 0869 are plain VABp banks). Only 0865 (and
  the sister 0863 `edstati3` entry) match the format documented here.

- **Town01 NPC palette source**: not in the battle_data pack — the
  CLUTs are plain PSX TIMs in town01's own
  [`scene_tmd_stream`](scene-bundles.md) PROT entries (e.g.
  `0006_town01.BIN @ 0x1ee4c`). Each is wrapped in a type-0x01 chunk
  header that `FUN_8001FE70` dispatches during battle init (field /
  town scene-load does not upload them). The engine's targeted-upload
  CLUT pass picks them up via `legaia_asset::tim_scan` and uploads them
  with merge-zeros semantics so the "full" (slots 0..14) and "partial"
  (slots 0..7) variants coexist. See [`npc-palette.md`](npc-palette.md).

- **Runtime asset-loader chain**: `FUN_8001E890` is the data-field-player
  loader (see [`asset-loader.md`](../subsystems/asset-loader.md)) - it
  reads `data\field\player.lzs` and registers the embedded TMDs into
  `0x8007C018 + idx*4` via `FUN_80026B4C`. The battle_data pack might
  feed into the same registry through a sister loader; tracing where the
  battle scene loader registers character TMDs would close the gap.
