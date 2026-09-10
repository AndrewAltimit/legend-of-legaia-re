# Format Reference

Byte-level specifications for every format on the Legend of Legaia disc. Each page gives a layout, the Ghidra-traced function that reads the format at runtime, and the from-scratch Rust parser that reimplements it.

**Read the page for a format before writing a parser against it.** Several of these look like their standard PSX counterparts and are not: the TMD variant uses its own magic and primitive grouping, the SEQ meta-event encoding omits MIDI's length field, and three unrelated containers are all called "pack".

New here? [`../overview.md`](../overview.md) explains how the layers stack from disc down to sub-asset.

## Confidence levels

Every page states how solid its decode is. The label is a claim about *evidence*, not about how complete the page looks - trust it accordingly.

- **Confirmed** - verified end-to-end against real on-disc data, with passing tests.
- **Inferred** - deduced empirically from byte patterns; structurally consistent but not yet exhaustively validated.
- **Unknown** - known to exist but not yet decoded.

A page may mix levels, and the good ones say so per-field rather than per-page: [`encounter.md`](encounter.md#confidence) rates its record shape and reader Confirmed while leaving the encoding within scripts Inferred. Where a page carries a `Confidence` section, that section is authoritative over the summary column below.

## Disc + container layer

| Page | Confidence | What it covers |
|---|---|---|
| [PSX disc geometry](disc.md) | Confirmed | Mode2/2352 sector layout, ISO9660 walk |
| [PROT.DAT / DMY.DAT TOC](prot.md) | Confirmed | Top-level archive: 1233 numbered entries whose extents tile the file exactly, TOC math, in-RAM TOC at `0x801C70F0` |
| [CDNAME.TXT name map](cdname.md) | Confirmed | The `#define`-driven naming for PROT entries. **Numbers are raw in-RAM TOC indices - extraction labels are shifted +2.** |
| [DMY.DAT (dev fixtures)](dmy.md) | Confirmed | Memory-bus test pattern + paired random blobs. Not real game data. |

## Compression + dispatch

| Page | Confidence | What it covers |
|---|---|---|
| [Legaia LZS](lzs.md) | Confirmed | The custom LZSS variant (`FUN_8001A55C`); 4096-byte ring buffer, init pos 0xFEE, LSB-first control bits. **A clean decode is not a validity signal** - magic-check the output. |
| [Asset type dispatcher](asset-type.md) | Confirmed | `FUN_8001F05C` - type-byte table that routes per-asset payloads |
| [Asset descriptor format](asset-descriptor.md) | Confirmed | `(type_size, data_offset)` pair walker (`FUN_80020224`), reached at runtime from town init `FUN_801D6704`. No top-level PROT entry matches it. |
| [Pack format](pack.md) | Confirmed | `u32 count + u32 offsets[]` used inside DATA_FIELD chunks |
| [Standalone TIM-pack](tim-pack.md) | Confirmed | The reader for a pack behind a `TIM_LIST` chunk header - members at `word_index * 4 + 4`. Not a container of its own: the `marker=0x01` it keys on is that chunk header's type byte |

There is **one** pack format and two readers for it. `pack.md` reads it at offset 0 (the bare form); `tim-pack.md` reads it four bytes in, past a `(TIM_LIST << 24) | size` DATA_FIELD chunk header, which is what its `word_index * 4 + 4` and its `marker == 0x01` byte test both encode. The former "field-pack" ([field-pack](field-pack.md)) is the same pack behind that same chunk header, not a third format. `categorize` now classifies by which form an entry is - `data_field_streaming` for the chunk-headered carriers, `pack` for the bare ones - so the `field_pack` and `tim_pack` classes are both empty against retail.

## Per-asset formats

| Page | Confidence | What it covers |
|---|---|---|
| [PSX TIM](tim.md) | Confirmed | Texture format. 4/8/16/24bpp. CLUT-aware. PNG export round-trips. |
| [Legaia TMD](tmd.md) | Confirmed | Custom PSX TMD variant (magic `0x80000002`). 8-byte group header, `count × ilen*4` stride. Renderer at `FUN_8002735C`. |
| [VAB sound bank](vab.md) | Confirmed | Sony's standard SPU instrument bank - `VABp` magic, 128 program × 16 tone slots, SPU-ADPCM bodies. |
| [PsyQ SEQ](seq.md) | Confirmed | PsyQ's MIDI-derived sequence format (`pQES` magic). 13-byte header, delta-time + MIDI events with running status. Drives `SsSeqOpen` / `SsSeqPlay`. |
| [XA-ADPCM](xa.md) | Confirmed | CD-XA Mode 2 Form 2 audio. `crates/xa` demuxes per `(file_no, ch_no)` channel, one WAV per stream. |
| [MES dialog](mes.md) | Confirmed | Two variants (Compact `0x404` and Records `0x44 0x78`); offset table + bytecode. Renderer is overlay-resident. |
| [Dialog font](dialog-font.md) | Confirmed | Proportional Latin font for dialog/menu text. Width table at `0x80073F1C`, escape table at `0x80074050`, glyph bitmaps in VRAM at `(896, 0)`. |
| [ANM animation](anm.md) | Confirmed | `(u16 count, u16 offsets[count], records)` layout. Asset type `0x06`. |
| [Player-character meshes](character-mesh.md) | Confirmed | Field form = PROT 0874 §0 (low-poly). Battle form = **assembled per character** from the player battle files' equipment-id sections; PROT 1204 `other5` is the sibling default-equipment pack. |
| [Monster animation](monster-animation.md) | Confirmed | Per-object rigid-transform keyframes inside the monster archive (PROT 867). Per-action stream at entry `+0x8c`: `[u8 parts][u8 frames][parts×frames × 9-byte TRS]`. Action 0 = idle. |
| [MDT move table](mdt.md) | Confirmed | Tactical Arts move tables. Two on-disc layouts the consumer accepts. |
| [Place names](place-names.md) | Confirmed | The three carriers one place name has - the SCUS quick-travel cells, the world-map label table trailing each kingdom MAN, and each scene MAN's section-2 banner name. Editing one changes one display. |
| [Save-slot portraits](save-icon.md) | Confirmed | Sixteen 16x16 character tiles row-interleaved across one 256x16 4bpp strip, a 16-colour palette each. Tile N is save N+1's memory-card icon; tile 15 is blank. |
| [Art data](art-data.md) | Inferred | Per-character art records in the player-data `record0` (extraction 863..865): Action Constants, command sequences, power-byte encoding, Miracle/Super Art trigger tables. Prefix + power byte are Confirmed; the variable-field tail is not. |
| [Per-character save record](save-record.md) | Confirmed | Runtime `0x414`-byte record at `0x80084708 + slot * 0x414`. Cheat-database-pinned offset table for stats / level / magic rank / spells / summons / equipment. |

## Battle / stat tables

Static `SCUS_942.54` rodata tables that drive stats, items, and magic. These are contiguous executable data rather than disc assets, so they decode without a disc walk, and each is byte-pinned against the executable.

| Page | Confidence | What it covers |
|---|---|---|
| [Spell table](spell-table.md) | Confirmed | `DAT_800754C8` stats / `DAT_800754D0` name pointers, 12-byte stride. MP cost + target + name per id; player Seru-magic block `0x81..=0x8b` pinned. |
| [Item-name table](item-table.md) | Confirmed | `PTR_DAT_8007436C[id*3]` (256 ids, 12-byte stride). The one id space that drops, steals, and equipment all index. |
| [Item-effect descriptor table](item-effect-table.md) | Confirmed | `DAT_800752C0`, 130 records, 4-byte stride. Effect class + tier + all-party/field/battle usability flags. Literal restore amounts are overlay-resident, **not** here. |
| [Equipment stat-bonus table](equipment-table.md) | Confirmed | `DAT_80074F68`, 8-byte stride. Per-equip attack/def bonuses + equip-character mask + slot type + Ra-Seru flag. |
| [Accessory passive-effect table](accessory-passive-table.md) | Confirmed | Accessory ("Goods") passives: a 64-slot index space feeding the per-character ability bitfield `char+0xF4` (`FUN_80042558`). Name/description table at `0x8007625C`. |
| [Move-power table](move-power.md) | Confirmed | Battle-action per-move power + behaviour records (26-byte stride, VA `0x801F4F5C`, PROT 0898). Damage roll, homing, hit reaction, sound cue, spawned effects; id → index map at `0x801F4E63`. |
| [Attack-camera track table](battle-attack-camera-table.md) | Confirmed | Per-art camera offsets the Arts-swing camera `FUN_801D71B8` folds into its pose (VA `0x801F4E10`, PROT 0898). 20 rows of two signed halfwords, indexed `row*4 + phase*2`. |
| [Steal table](steal-table.md) | Confirmed | `DAT_80077828`, 1-based monster id, 2-byte `[chance, item]` - chance FIRST, the reverse of the drop field order. What the Evil God Icon steals; **not** in the PROT 867 record. |
| [SFX descriptor table](sfx-table.md) | Confirmed | `DAT_8006F198`, 8-byte stride, 100 entries `0x00..=0x63`. Per cue: VAB program/tone, voice count, mixer channel. |
| [New-game starting party](new-game-table.md) | Confirmed | 4-record template at `0x80078C4C` (26-byte stride) that seeds the live `0x80084708` character records; opening scene `town01`. |
| [Window widget scripts](window-script.md) | Confirmed | The window-script VM's bytecode programs: fixed 4-byte `[opcode][window id][operand u16]` instructions, resident as a program table in the menu overlay's own data segment (PROT 0899). Parser + `jal`-site scanner `legaia_asset::widget_script`. |

## Streaming + scene containers

| Page | Confidence | What it covers |
|---|---|---|
| [DATA_FIELD streaming](data-field.md) | Confirmed | `[type, size, data]` chunk stream consumed by `FUN_8002541C` |
| [Scene bundles](scene-bundles.md) | Confirmed | Scene-prefixed wrappers (`scene_tmd_stream`, `scene_vab_stream`, `scene_v12_table`, `scene_asset_table`) - the dominant per-scene asset shapes. Descriptor count is unbounded in retail (two scenes ship `count = 5`); the anchor is descriptor 0 at `8 + count*8`, and the header's `+0x04` word is `Σ descriptor.size`, never read |
| [scene_v12_table](scene-v12-table.md) | Confirmed | Per-scene `.PCH` walk-on trigger sidecar: a four-kind sub-table directory + the kind-1 trigger records, one `0x800` sector exactly. 97 PROT entries (one per scene); the event-script prescript is the **next** entry. |
| [Per-scene field map](field-map.md) | Confirmed | `DATA\FIELD\<scene>.MAP` - the fixed `0x12000`-byte slot 0 of every scene block. Four regions (object descriptors, collision + floor grid, object-index map, trigger block) whose sizes sum to the footprint exactly. |
| [Effect bundles](effect.md) | Confirmed | Both the on-disc bundle (magic `0x02018B0C`) and the runtime 2-pack wrapper used by `efect.dat` |
| [summon.dat / readef.DAT](summon-readef.md) | Confirmed | Battle side-band streaming slots (`0x10800` bytes each): per-special-attack CLUTs + 4bpp texture pages + summon-creature actor records. Extraction PROT 893 / 894 (retail TOC `0x37F` / `0x380`) |
| [Field-pack](field-pack.md) | Confirmed | Not a format: a DATA_FIELD `TIM_LIST` chunk (`0x01059B84` = type byte + payload length) holding an `asset::pack` of one scene's TIMs at raw-TOC `+4` of its block; loaded by `FUN_800255B8` / `FUN_8002541C` |
| [Player battle files](battle-data-pack.md) | Confirmed | `data\battle\PLAYER1..4` (extraction 863..866). Header + LZS `record[0]` + 12-byte `[id, offset, size]` descriptor table + per-slot LZS streams decoding to `[header + Legaia TMD + texture pool]`. |
| [Row-479 NPC CLUTs](npc-palette.md) | Confirmed | Plain PSX TIMs in scene PROT entries with CLUT block at `(fb_x=0, fb_y=479, w=256, h=1)`. Uploaded via the targeted-upload CLUT pass with merge-zeros semantics, so multiple scene-pack TIMs on the same row coexist. |
| [Encounter record](encounter.md) | Confirmed | Layout `[3 reserved][count: u8][monster_ids: u8[count]]`. Installed at `actor[+0x94]` by the script-VM, read by `FUN_801DA51C` to populate the formation cell at `0x8007BD0C`. Also the MAN section-3 camera-region table and the camera's visible tile window `0x1F8003E8..EB` (field-VM op `0x46`; not fog). |
| [MAN relocation](man-relocation.md) | Confirmed | Variable-length editing of a decompressed MAN - how to resize a `0x3F` door destination and keep every internal offset valid. Powers the door randomizer. |
| [STR FMV table](str-fmv-table.md) | Confirmed | FMV dispatch table at `0x801D0A6C` - 23 × 32-byte slots, of which nine are the retail `fmv_id 0..=8`. See below for the neighbouring table it is easily confused with. |
| [World-map slot-4 records](world-map-overlay.md) | Confirmed | Slot 4 of each kingdom bundle (PROT 0086 / 0245 / 0392, type byte `0x05`): the world-map scene's ANM animation bank - per-clip rigid transforms (12-bit translations + 8-bit rotations), consumed by `FUN_800204F8` / `FUN_8001B964` |
| [Per-scene primitive scratch buffer](navmesh.md) | Inferred | Documented negative finding - `0x80108EA4..0x80109550` is per-scene rendering scratch, not navmesh data. Reproduction commands included. |

### Two easily-confused windows

The STR FMV overlay holds **two** tables. The FMV dispatch table at `0x801D0A6C` is the play engine's source: 23 slots of `[path_ptr, depth, start_frame, end_frame, fb_x, fb_y, w, h]`, static overlay data that decodes straight from the disc. The `0x801CAE08` window nearby is the generic libcd directory-record cache - PsyQ `CdlFILE`-shape records, **not** an FMV table.

World-map slot 4 is likewise not what it was first read as - twice. It is neither a coastline wireframe nor a library of small meshes: it is the world-map scene's asset-type-`0x05` **ANM animation bank**, the same clip container every field scene carries, whose 8-byte entries pack three 12-bit translations and three 8-bit rotations (`world-map-overlay.md`). The "GTE vertex `(i16 x, y, z, attr)` walked by `FUN_80044c14`" reading came from assuming that prim handler owned the pool; `attr` is the Y / Z rotation pair, read every frame.

## Runtime overlay carriers

| Page | Confidence | What it covers |
|---|---|---|
| [MIPS overlay code](mips-overlay.md) | Inferred | PROT entries that carry runtime code blobs (recognized by `addiu sp, sp, -X` prologue) |
| [Overlay pointer-table code](overlay-ptr-table.md) | Inferred | Sister format - entries whose first chunk is a function/jump-table header pointing into `0x801C0000..=0x801FFFFF` |
| [Slot-B module image layout](slot-b-module-layout.md) | Confirmed (band shape + extent); Inferred (the highest record's end) | The file layout of the 64 cast / summon images `0903..=0966`: head table, code, the `[i16 model_sel][u16 flags][move-VM bytecode]` **spawn-record band**, and the inherited tail every image ends in. Records are recovered from the consumer's own `lui`/`addiu` pointer, not from a directory, so the topmost record has no static upper bound. Parser `legaia_asset::slot_b_module` |

## Audio path-strings

| Page | Confidence | What it covers |
|---|---|---|
| [Sound-driver path-string cluster](sound-driver.md) | Confirmed | The string-builder cluster at `0x8007B38C` and the eight file extensions the runtime resolves through it (`.spk`, `.LZS`, `.dpk`, `.MAP`, `.PCH`, `.pac`, `STR`, `bse.dat`) |
| [`bse.dat` battle SFX bank](bse-dat.md) | Confirmed | The **battle** occupant of the runtime SFX descriptor bank (cue ids `>= 0x200`): `[u16 tag][u16 body_offset][8-byte records]`, loaded per battle by `FUN_8001FA88` from battle init `FUN_800513F0` - not at boot. Row index = `cue id - 0x200`; the columns are the [`sfx-table`](sfx-table.md) columns, `+4` a `u8` category the cue router rewrites through `gp+0x678`. Extraction 888 (the loader's `0x37A`); 1195 is a scene prescript filling the same slot in the field. |

The dispatch chain *into* these formats is fully traced. The byte-level layout of the individual `.spk` / `.dpk` / `.MAP` / `.PCH` files is still open.

## Placeholders + dev fixtures

| Page | Confidence | What it covers |
|---|---|---|
| [Pochi-filler placeholder slots](pochi.md) | Confirmed | 266 PROT entries are dev-fill placeholders - recognised by `pochipochi…` ASCII + `0x1A` DOS-EOF marker at `+0x786`. Each is exactly one 2048-byte sector of fill and none carries a parseable TIM; the page records why the "stale scratch that parses as a TIM" reading was the *next* entry seen through the old entry size. |

## Video / pre-rendered

`MOV/MV*.STR` files are PSX MDEC video streams. Legaia's are the **Iki** bitstream (LZSS-compressed per-block qscale/DC table + an AC-only entropy stream, 16-bit-LE MSB-first, column-major macroblocks) rather than STRv2.

[`crates/mdec`](../../crates/mdec/README.md) is a from-scratch decoder for it: `mdec decode-str` writes frames to disk, and `legaia-engine play-str` plays a movie back in a window with synced XA audio. See [`subsystems/cutscene.md`](../subsystems/cutscene.md) for the decode algorithm and the A/V sync path.

For the audio side, `XA/XA*.XA` files are XA-ADPCM streams in standard CD-XA Mode 2 Form 2. The decoder in `crates/xa` is spec-correct, and the [`xa demux-disc`](../../crates/xa/src/bin/xa.rs) subcommand reads raw 2352-byte sectors directly off the `.bin`, parses each `(file_no, ch_no)` subheader, and emits one WAV per channel. The earlier "non-standard interleave" framing was Form-1-truncation damage, not a bespoke Legaia muxing scheme - see [`xa.md`](xa.md).
