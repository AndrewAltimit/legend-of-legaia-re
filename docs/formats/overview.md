# Format Reference

Every format documented here has a clean-room Rust parser somewhere in the workspace, an Ghidra-traced provenance, and a byte-level layout. Confidence levels:

- **Confirmed** - verified end-to-end against real on-disc data, with passing tests.
- **Inferred** - deduced empirically from byte patterns; structurally consistent but not yet exhaustively validated.
- **Unknown** - known to exist but not yet decoded.

## Disc + container layer

| Page | What it covers |
|---|---|
| [PSX disc geometry](disc.md) | Mode2/2352 sector layout, ISO9660 walk |
| [PROT.DAT / DMY.DAT TOC](prot.md) | Top-level archive: 1232 numbered entries, TOC math, in-RAM TOC at `0x801C70F0` |
| [CDNAME.TXT name map](cdname.md) | The `#define`-driven naming for PROT entries |

## Compression + dispatch

| Page | What it covers |
|---|---|
| [Legaia LZS](lzs.md) | The custom LZSS variant (`FUN_8001A55C`); 4096-byte ring buffer, init pos 0xFEE, LSB-first control bits |
| [Asset type dispatcher](asset-type.md) | `FUN_8001F05C` - type-byte table that routes per-asset payloads |
| [Pack format](pack.md) | `u32 count + u32 offsets[]` used inside DATA_FIELD chunks |
| [Standalone TIM-pack](tim-pack.md) | Distinct outer container with `(magic_lo, magic_hi, count<16, marker=0x01)` header |

## Per-asset formats

| Page | What it covers |
|---|---|
| [PSX TIM](tim.md) | Texture format. 4/8/16/24bpp. CLUT-aware. PNG export round-trips. |
| [Legaia TMD](tmd.md) | Custom PSX TMD variant (magic `0x80000002`). 8-byte group header, `count × ilen*4` stride. Renderer at `FUN_8002735C`. |
| [VAB sound bank](vab.md) | Sony's standard SPU instrument bank - `VABp` magic, 128 program × 16 tone slots, SPU-ADPCM bodies. |
| [PsyQ SEQ](seq.md) | PsyQ's MIDI-derived sequence format (`pQES` magic). 13-byte header, delta-time + MIDI events with running status. Drives `SsSeqOpen` / `SsSeqPlay`. |
| [MES dialog](mes.md) | Two variants (Compact `0x404` and Records `0x44 0x78`); offset table + bytecode. Renderer is overlay-resident. |
| [Dialog font](dialog-font.md) | Proportional Latin font for dialog/menu text. Width table at `0x80073F1C`, escape table at `0x80074050`, glyph bitmaps in VRAM at `(896, 0)`. |
| [ANM animation](anm.md) | `(u16 count, u16 offsets[count], records)` layout. Asset type `0x06`. |
| [MDT move table](mdt.md) | Tactical Arts move tables. Two on-disc layouts the consumer accepts. |
| [Art data](art-data.md) | Per-character art records: Action Constants, command sequences, power-byte encoding, Miracle/Super Art trigger tables. PROT entry `0x05C4`. |
| [Per-character save record](save-record.md) | Runtime `0x414`-byte record at `0x80084708 + slot * 0x414`. Cheat-database-pinned offset table for stats / level / magic rank / spells / summons / equipment. |

## Streaming + scene containers

| Page | What it covers |
|---|---|
| [DATA_FIELD streaming](data-field.md) | `[type, size, data]` chunk stream consumed by `FUN_8002541C` |
| [Scene bundles](scene-bundles.md) | Scene-prefixed wrappers (`scene_tmd_stream`, `scene_vab_stream`, `scene_v12_table`, `scene_asset_table`) - the dominant per-scene asset shapes |
| [Effect bundles](effect.md) | Both the on-disc bundle (magic `0x02018B0C`) and the runtime 2-pack wrapper used by `efect.dat` |
| [Field-pack format](field-pack.md) | Magic `0x01059B84` plus a 97-entry strict schema preceding packed TIMs/TMDs |
| [Battle-data pack](battle-data-pack.md) | Custom 16 MB-ish container for the `battle_data` block (PROT 0865 + sister `edstati3`). Streaming preamble + 12-byte record table + per-record LZS streams that decompress to `[header + Legaia TMD + texture pool]`. |
| [Global hue-ramp palette (row 479)](npc-palette.md) | 15-slot CLUT ramp at `(fb_x=0..240, fb_y=479)` that the runtime stages in RAM at `0x800F19xx` and DMAs to VRAM during early init. Town/field NPC TMDs sample slots 8..14 via CBA `0x77C8..0x77CF`. Bytes captured verbatim - generator MIPS still uncovered. |
| [Per-scene primitive scratch buffer](navmesh.md) | Documented negative finding — `0x80108EA4..0x80109550` is per-scene rendering scratch, not navmesh data. Reproduction commands included. |
| [Encounter record](encounter.md) | Layout `[3 reserved][count: u8][monster_ids: u8[count]]`. Pointer installed at `actor[+0x94]` by the script-VM, read by `FUN_801DA51C` to populate the formation cell at `0x8007BD0C`. |
| [STR FMV table](str-fmv-table.md) | In-RAM compact table the cutscene / MDEC overlay uses to look up `MV*.STR` files. Six 24-byte entries at `0x801CAE40`: filename + libcd BCD MSF + size. |

## Runtime overlay carriers

| Page | What it covers |
|---|---|
| [MIPS overlay code](mips-overlay.md) | PROT entries that carry runtime code blobs (recognized by `addiu sp, sp, -X` prologue) |
| [Overlay pointer-table code](overlay-ptr-table.md) | Sister format - entries whose first chunk is a function/jump-table header pointing into `0x801C0000..=0x801FFFFF` |

## Audio path-strings

| Page | What it covers |
|---|---|
| [Sound-driver path-string cluster](sound-driver.md) | The string-builder cluster at `0x8007B38C` and the eight file extensions the runtime resolves through it (`.spk`, `.LZS`, `.dpk`, `.MAP`, `.PCH`, `.pac`, `STR`, `bse.dat`) |

## Placeholders + dev fixtures

| Page | What it covers |
|---|---|
| [Pochi-filler placeholder slots](pochi.md) | 265 PROT entries are dev-fill placeholders - recognised by `pochipochi…` ASCII + `0x1A` DOS-EOF marker at `+0x786` |
| [DMY.DAT (dev fixtures)](dmy.md) | Memory-bus test pattern + paired random blobs. Not real game data. |

## Asset-descriptor format (still hunting a runtime caller)

| Page | What it covers |
|---|---|
| [Asset descriptor format](asset-descriptor.md) | `(type_size, data_offset)` pair walker (`FUN_80020224`) - known structure, runtime caller is `FUN_801D6704` (town init) |

## Video / pre-rendered

`MOV/MV*.STR` files are PSX MDEC video streams. Public decoders exist (jPSXdec, PSX-MDEC docs); the engine track delegates to those rather than re-implementing.

`XA/XA*.XA` files are XA-ADPCM audio streams in standard CD-XA Mode 2 Form 2. The decoder in `crates/xa` is spec-correct, and the [`xa demux-disc`](../../crates/xa/src/bin/xa.rs) subcommand reads raw 2352-byte sectors directly off the `.bin`, parses each `(file_no, ch_no)` subheader, and emits one WAV per channel. See [`xa.md`](xa.md) - the earlier "non-standard interleave" framing was Form-1-truncation damage, not a bespoke Legaia muxing scheme.
