# DATA_FIELD streaming format

A stream of typed chunks consumed by `FUN_8002541C` on its `0x14` (DATA_FIELD) branch. The walker passes every chunk through the [asset-type dispatcher](asset-type.md) with `copy_only=1`, so chunks are always uncompressed.

Implementation: `crates/asset/src/lib.rs::parse_streaming`.

> **Scope note.** This doc covers the *typed-chunk streaming* shape that `FUN_8002541C` consumes - not the wider question of "what does the per-scene CDNAME block actually carry?" Per-scene field bundles use multiple shapes; the typed map below identifies which shapes show up where.

## Layout

```
[u32 type_size] [size_bytes of raw data]
[u32 type_size] [size_bytes of raw data]
...
[u32 terminator]    // type_size with low 24 bits all zero
```

Where:
- `type_size = (type_byte << 24) | (size_bytes & 0x00FFFFFF)`.
- `type_byte` matches the [asset type table](asset-type.md).
- The next chunk header starts at `current_pos + 4 + (size & ~3)` - i.e., header + size truncated to a 4-byte boundary. Sizes are always 4-aligned in practice.
- Terminator: any header `u32` whose low 24 bits are zero.

## What's in the wild

`asset scan-stream` strict-validates 34 PROT entries (class `data_field_streaming`), in two families.

**Character / effect streams** - the `other5` run, entries 1204..1219. The common shape is three chunks:

```
chunk[0]: TIM   (single, magic 0x10)        - sprite atlas / texture
chunk[1]: TMD2  (single, magic 0x80000002)  - single Legaia TMD
chunk[2]: MOVE2 (single, magic 0x08)        - animation data
terminator
```

The two heads of the run are homogeneous instead: `1204` is five `TMD2` chunks (the battle-form party meshes) and `1205` is eight `TIM` chunks of `0x8220` bytes each (their atlases) - which is what makes the atlas stride `0x8224`, chunk header included.

**Scene bundles** - `MAN / MES / MOVE / VDF`, four chunks (`dolk2`, `rikuroa`, `rikuroa2`, `rayman`, `station`, `taiku`, `taiku2`, `doman`, `nilboa2`, `edbalden`, `eddoman`). Shorter variants drop the trailing `VDF` (`balden2`, `ropeway2`) or lead with a bare `TMD` where a `MAN` would sit (`balden`, `bubu1`, `edbubu`). Two entries are neither family: `init_data` is `TIM_LIST + TMD`, and `0890_sound_data2` is two `TIM`s.

The chunk layouts are **single assets** in the `other5` family (one TIM, one TMD2, one MOVE2), not packs. Other clusters elsewhere in the corpus do use pack-shaped TIM_LIST / TMD chunks; the [pack format](pack.md) handles that case.

## Trailer data

Some entries contain bytes past the streaming terminator. `asset extract` preserves these as `_trailer.bin` next to the extracted chunks. The function that consumes the trailer hasn't been located; tracing the caller of `FUN_8002541C` in the field/town overlay is the next move if a specific entry's trailer looks structured.

## Related shapes

- The [scene-TMD-prefixed streaming](scene-bundles.md) shape is structurally similar but the leading chunk has no `[u32 type_size]` header and the inner content is a bare TMD instead of a TIM.
- The [scene-VAB-prefixed streaming](scene-bundles.md) shape uses the same chunk0-header trick but with VAB content.
- [Pack format](pack.md) lives *inside* TIM_LIST / TMD chunks when the chunk's data is a pack rather than a single asset.
- One entry (extraction `0892`) matches the `data_field_truncated` detector (`crates/asset/src/data_field_truncated.rs`) - three clean leading chunks and an over-large fourth header. It is **not** a streaming carrier: the runtime reads it as an [`asset::pack`](pack.md), and the "chunks" are that pack's own header words. See [below](#entry-0892-card_data-is-a-pack-not-a-truncated-stream).
- Three scene entries commonly cited in this class - `0157_rikuroa`, `0228_station`, `0373_taiku` - are **not** truncated. Each is one of the four-chunk `MAN / MES / MOVE / VDF` bundles above, and each is a case where the superseded `toc[p+5] - toc[p+3] + 4` span fell *short* of the real entry (`0157` declares 163840 bytes against a real 186368; the other two are short by 69632 and 8192), so the last chunk overran a buffer that ended early. Against their own sectors all three terminate cleanly. See [`prot.md`](prot.md#tocp5---tocp3--4-is-not-an-entrys-size).

## Entry 0892 (`card_data`) is a pack, not a truncated stream

Extraction entry `0892` is the only retail hit for the `data_field_truncated` class, and the hit is an artefact. The entry's first three words are an [`asset::pack`](pack.md) header - `count = 2`, `word_offsets = [3, 0x208B]` - and the streaming reader takes those three words for chunk headers of type `0x00` and sizes 2, 3 and 8331, which walks it to `+0x2094`, inside the first member's pixel data, where the next word declares a body far past the entry. Nothing in the entry is a `[u32 type_size]` chunk. The companion "12 MB LZS container" figure on [`cdname.md`](cdname.md#consequential-relabelings) came from the superseded declared span (`toc[p+5] - toc[p+3] + 4` = 5977 sectors); the entry is 33 sectors, 67,584 bytes.

### What the runtime does

Retail reads the entry as a pack and dispatches each member. Mode-22 `CARD INIT` (`FUN_8002574C`, SCUS-resident) is the only loader:

| Address | What it does |
|---|---|
| `0x800257D0` | `jal 0x80017888`, `a1 = 0x19000` (`lui a1,0x1` / `ori a1,a1,0x9000`) - allocate the staging buffer |
| `0x800257DC` | `lh v1,-0x473e(v1)` - the dev/retail flag `_DAT_8007B8C2`, the same one [`bse.dat`](bse-dat.md#which-entry-it-is-and-when-it-loads) branches on |
| `0x800257F4` | dev leg: `jal 0x8003e6bc`, the by-name path opener |
| `0x8002580C` | retail leg: `li a0,0x37e` then `jal 0x8003eb98` - `byindex_sync_loader(0x37E, buf, 1)`. Raw TOC `0x37E` is extraction `0892` under the [+2 correction](cdname.md#numbering-space) |
| `0x8002581C..0x80025850` | the pack walk: `count = *buf`, then per member `FUN_800198E0(buf + word_offsets[i] * 4)` - the packed-image uploader |
| `0x80025858` | `jal 0x80017b94` - free the staging buffer. The members live only in VRAM |

`see ghidra/scripts/funcs/8002574c.txt`, `8003eb98.txt`.

The entry-context selector is `gp+0x7E8` (`0x8007BB00`), read twice: `0` skips the whole block (`0x8002576C`, straight to the epilogue that sets game mode `0x17`), `1` takes the disc load (`0x800257C0`), and any other value takes a third arm that instead `MoveImage`s the same VRAM footprint back from a parked copy - `(0,492) 256x1` to `(0,475)` and `(704,0) 128x256` to `(320,256)` (`0x80025880` / `0x800258A8`, `FUN_80058490`).

Every writer of the slot stores `1` (`0x8003B5E0`, `0x8003C808`, field overlay `0x801D84CC`, cutscene overlay `0x801CF048`) and the menu overlay clears it to `0` (`0x801DFB10`), so no retail path reaches that third arm. Its inverse is live: the menu overlay parks the pair the other way - `(0,475) 256x1` to `(0,492)`, then `(320,256) 128x256` to `(704,0)`, at `0x801DDD0C` / `0x801DDD34` - on the way into game mode `0x1A`.

### The two members

Both are complete 4bpp TIMs of exactly `0x8220` bytes:

| Member | Byte offset | CLUT rect | Image rect | Page |
|---|---|---|---|---|
| 0 | `0x0C` | `(0, 475) 16x16` | `(320, 256) 64x256` | 256x256 px |
| 1 | `0x822C` | `(0, 475) 16x16` | `(384, 256) 64x256` | 256x256 px |

Together they tile VRAM `(320..447, 256..511)`, which is exactly the rect both `MoveImage` pairs above move. The 948 bytes past member 1 (from `0x1044C`) are not referenced by the pack and are the entry's own tail slack.

### The content: a JIS X 0208 level-1 kanji font

The CLUT block is byte-identical in the two members and is not a palette bank in the ordinary sense. Rows `0..3` hold the index patterns `0101…`, `0011…`, `00001111…` and `00000000 11111111` against one ink colour, and rows `4..7` repeat them against a darker ink - each row selects **one bit of the 4bpp index**. The texture is therefore a 1bpp bitmap packed four bit-planes deep, and a draw picks its plane by CLUT row (`0..3` light, `4..7` dark). Rows `8..15` are ordinary 16-colour palettes for other content sharing those rows.

Per plane the glyph grid is a 12-pixel pitch with an 11x11 ink box: columns and rows `11, 23, 35, …, 239` carry no ink in any plane, and nothing is inked past column or row 238. That is 20x20 = 400 cells per plane, 4 planes per member, 8 planes in all. Seven planes are full and the last is inked through cell 164, for **2965 inked cells** - exactly the JIS X 0208 level-1 kanji count. Plane 0 confirms the order is the level-1 ku-ten sequence from its first glyph, with cell 21 landing on the 21st glyph of that sequence, so a glyph's cell is `plane = g / 400`, `row = (g % 400) / 20`, `col = g % 20`.

### Which routine samples the page

Still **open**, but narrowed to a shape. A sampler has to carry two constants,
and both are now pinned by the members' own headers: the pages are 4bpp at
`(320, 256)` and `(384, 256)`, so their **tpage ids are `0x15` and `0x16`**
(`(y >> 8) << 4 | x >> 6`, texture mode `0`), and a glyph draw has to pick its
bit-plane by CLUT row, so its **CBA is `0x76C0 + plane * 0x40`** for
`plane` `0..7` (rows `475..482`).

Neither constant appears anywhere on the disc. A byte-level sweep of
`SCUS_942.54` and all 83 statically based overlay images, in **both** forms - an
`addi` / `addiu` / `ori` / `lui` immediate, and a bare halfword at any even
offset, which is what a sprite-descriptor table entry would be - finds:

- **no** materialisation of any of the eight CLUT ids `0x76C0`, `0x7700`, …,
  `0x7880` outside incidental instruction halves (the `0x7700` / `0x77C0` hits
  are `lui` upper halves and battle-overlay packet words, a different screen's
  use of the same VRAM rows);
- **two** `0x1DB` (`475`) immediates disc-wide, and both are already accounted
  for: `0x8002586C` in `FUN_8002574C`'s third arm and `0x801DDCF4` in the menu
  overlay's park - the two `MoveImage` sites above. Every other `0x1DB` in any
  image is a data halfword in an unrelated ramp table (`0x800702E0..`, a
  monotonic `+1`-per-six-entries curve) or the upper half of an instruction.

The uploader does not leave a handle behind either: `FUN_800198E0` builds its
rect straight from the TIM header and calls `LoadImage`; it records no CBA or
tpage anywhere for a later draw to read
(`see ghidra/scripts/funcs/800198e0.txt`).

So the consumer, if this build has one, must compose the CBA at runtime from a
value that is itself not a constant. What would close it is a capture rather
than another sweep: enter the card screen under PCSX-Redux and watch the GPU
FIFO for any primitive whose CBA halfword lands in `0x76C0..=0x7880` or whose
tpage is `0x15` / `0x16`. If none appears, the page is loaded, parked and never
drawn in this build - which the surrounding code already makes plausible, since
the live path parks it into the world-map texture region `(704, 0)` on the way
out, and the save/load screen's own text is pinned to the
[dialog font](dialog-font.md) page at `(896, 0)`, not to this one
([`save-screen.md`](../subsystems/save-screen.md)).

## Per-scene field bundles - what's still open

The CDNAME block for a typical field/town scene (e.g. `town01`, `bubu1`) carries 8–12 PROT entries. Categorize identifies several known shapes per block:

| Common slot | Class | Typical content |
|---|---|---|
| 0 / 1 | `SceneTmdStream` or `TmdSizePrefix` | scene mesh (room geometry) |
| 1 / 2 | `Pack` (TIM-pack) | scene textures / sprite atlas |
| 2 / 3 | `SceneEventScripts` | per-scene move-VM stager records (not field-VM bytecode) |
| 3 / 4 | `MesContainer` | dialog text |
| 4 / 5 | `Pack` (ANM-pack) | per-actor animation sets |
| 5..7 | `PochiFiller` | reserved-but-unused dev fillers |
| 6..8 | `SceneVabStream` (rare; only on scenes with custom audio) | per-scene VAB + SEQ |

What's **NOT** modelled yet:
- Cross-entry pointers (NPC references that point into other PROT entries - the asset chain in [asset-loader.md](../subsystems/asset-loader.md) is best-effort).
- Which function consumes the post-terminator trailer bytes (see [Trailer data](#trailer-data)).

Two entries on this list are now closed, and both closed by shrinking rather than by new
machinery:

- **The `field-pack` "slot semantics"** were the pack format on this page. The
  `0x01059B84` "magic" is a `(TIM_LIST << 24) | size` chunk header, the "97-entry schema"
  is that chunk's `[u32 count][u32 word_offsets]` [pack](pack.md) table, and the "124
  entries" figure came from the superseded over-reading entry size - 23 blocks carry such
  a file, one apiece. See [field-pack.md](field-pack.md).
- **The per-scene asset-table indirection** needs no capture: the walk is positional
  (`FUN_80020224`, `count` at `+0x00`, 8-byte descriptors from `+0x08`) and the bundle
  reaches the walker's base by a whole-sector block copy of the entry. `+0x04` is the sum
  of the descriptor sizes. `SceneScriptedAssetTable` fires on nothing - it was the same
  over-read. See [scene-bundles.md](scene-bundles.md#scene_asset_table---count-prefixed-asset-bundle).

The categorize sweep covers the bulk of bytes - every PROT entry classifies to *something*, and ~95% of bytes fall into known classes. Refining the residual classes is the work tracked under "Reverse-engineer DATA_FIELD per-scene layout" in [`docs/subsystems/engine.md`](../subsystems/engine.md).

## See also

- [asset::pack](pack.md) - the pack format carried inside DATA_FIELD chunks.
- [Asset-type dispatch](asset-type.md) - the type byte that opens each chunk.
- [Asset descriptor](asset-descriptor.md) - the descriptor layout the chunks reference.
