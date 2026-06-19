# CDNAME.TXT - entry name map

A plain-text file at the disc root that maps PROT.DAT entry indices to human-readable names. One C-style `#define` per line:

```
#define init_data 0
#define gameover_data 1
#define town01 3
#define town0b 12
#define town0c 21
...
#define vab_01 1072
```

Implementation: `crates/prot/src/cdname.rs`.

## Block-start semantics

Each `#define name N` marks **the start of a block** of N entries. Subsequent PROT entries inherit the name of the most recent block:

- entry 3 → block `town01`
- entry 11 → block `town01` (since `town0b` starts at 12)
- entry 12 → block `town0b`

`prot-extract` uses these names to produce filenames like `0148_retock.BIN`.

## Numbering space

The `#define` numbers are **raw in-RAM PROT-TOC indices** - the index space `FUN_8003E8A8` consumes - **not** extraction-entry indices. The boot TOC loader copies `PROT.DAT` verbatim (8-byte header included) into `0x801C70F0`, so `raw index = extraction index + 2` (see [`prot.md` § In-RAM TOC](prot.md#in-ram-toc)). The content `#define name N` actually names lives at **extraction entry `N − 2`**, and the extractor's filename labels (which apply define numbers as extraction indices directly) are systematically shifted +2.

The default `NNNN_<name>.BIN` naming is kept for stability; `legaia_prot::cdname::block_for_extraction_index` resolves the retail-space name for an extraction index, and `scripts/asset-investigation/cdname_shift_analysis.py` reproduces the full quantitative analysis against a local extraction.

### Evidence

**Loader-constant identities** (strongest - retail hard-codes raw-TOC indices for dev-named files, and each constant *equals* the same-named define):

| Block (defines) | Retail raw-TOC constant | Provenance |
|---|---|---|
| `battle_data 865..868` | `0x361..0x364` = 865..868, `data\battle\PLAYER1..4` | `FUN_800558FC(char+0x360)` live trace; extraction 863..866 start at the traced PROT.DAT offsets |
| `monster_se 893` | `0x37D` = 893, `h:\mpack\monster.snd` | `FUN_8003E104` (`li v0,0x37d`); extraction 891 is a 206-bank multi-VAB SE archive |
| `bat_back_dat 895..896` | `0x37F`/`0x380` = 895/896, `summon.dat`/`readef.DAT` | `FUN_801F17F8`; byte-verified RAM↔disc at extraction 893/894 ([`summon-readef.md`](summon-readef.md)) |
| `xxx_dat 897..` | `0x381+`, overlay slots | `FUN_8003EBE4`/`FUN_8003EC70` call `FUN_8003E8A8(param + 0x381)`; param 2 = field overlay = extraction 0897 |

**Structural (scene region, defines 3..864):** the per-scene [v12 fixup table](scene-v12-table.md) recurs once per scene block, and scene block *lengths vary* (7..11 slots), so its slot position is shift-sensitive. At shift 0 the 96 scene-region v12 tables scatter across slots 4..10 (modal slot holds only 41); at −2 **all 96 sit at slot 1**. Slot constancy alone admits any shift ≤ −1; the identity anchors pin −2 exactly (the same resolver serves every block). The universal field-`.MAP`-at-`define − 2` rule is this same fact: each scene's `.MAP` is retail **slot 0** of its block, the v12 table slot 1, the event prescript slot 2, the 7-asset table slot 3.

**Semantic scoring:** over the name blocks with checkable expectations (`sound_data`/`sound_data2`/`level_up`/`monster_se`/`music_01`/`vab_01` → VAB/SEQ shapes; `move_program_no` → `\DATA\MOV*.STR` program table; `other_game` → `OTHER<n>` overlay banners), shift −2 matches 217/225 decidable entries vs 209/225 at shift 0 - e.g. `vab_01` → extraction 1070..1192 is 121/121 VAB-headed.

### Consequential relabelings

Per-entry content claims in this repo stay in extraction space (unambiguous); the retail-space names below are what the dev defines actually cover. The most consequential corrections (extraction entry → retail-space block):

| Extraction | Filename label says | Retail-space block (content) |
|---|---|---|
| raw 0..1 (LBA 3..120, unindexed by extraction) | - | `init_data` + `gameover_data` slot 0 - the boot-UI gap with the menu-glyph atlas |
| 0000 | `init_data` | `gameover_data` (second slot) |
| 0863..0866 | `edstati3`/`battle_data` | `battle_data` = the four per-character battle files `PLAYER1..4` ([battle palettes](../reference/open-rev-eng-threads.md)) |
| 0867 | `battle_data` | `monster_data` - the 16 MB monster stat archive (resolves the long-standing "battle_data 0865 vs monster archive 0867" tangle) |
| 0868..0869 | `monster_data`/`sound_data` | `sound_data` (VAB-prefixed streams) |
| 0870..0873 | `sound_data`/`befect_data` | `befect_data` - `etim` (0870, the pixel-verified effect-texture source), `etmd`+`vdf`, billboard pack, `efect.dat` (0873) |
| 0874 | `befect_data` | `player_data` - the field character-mesh pack ([character-mesh.md](character-mesh.md)); the field-char-texture hunt first searched 0876 because of the shifted label |
| 0875..0888 | `player_data`/`sound_data2` | `sound_data2` (VAB streams) |
| 0889..0890 | `sound_data2`/`level_up` | `level_up` (large VAB carriers) |
| 0891 | `level_up` | `monster_se` = `monster.snd`, the 206-bank monster-SE archive |
| 0892 | `level_up` | `card_data` (12 MB LZS container; content not yet pinned) |
| 0893..0894 | `monster_se`/`card_data` | `bat_back_dat` = `summon.dat`/`readef.DAT` mid-cast backdrop streams |
| 0895..0969 | `bat_back_dat`/`xxx_dat` | `xxx_dat` - slot 0 (extraction 0895) is the boot `init.pak` bundle loaded through overlay-slot param 0 ([`boot.md`](../subsystems/boot.md#boot-initpak-prot-0895)); overlay code blobs follow ([MIPS overlay](mips-overlay.md), [overlay pointer-table](overlay-ptr-table.md)) |
| 0970..0971 | `xxx_dat` | `move_program_no` - a `\DATA\MOV*.STR` FMV program/path table + debug strings. Dissolves the old "move_program_no doesn't match `move.mdt`" puzzle: the block names **MOV**ie program numbers ([str-fmv-table.md](str-fmv-table.md)), not Tactical-Arts moves, and the extraction files `0972/0973` it was tested against are `other_game` overlays |
| 0972..0977 | `move_program_no`/`other_game` | `other_game` - casino/minigame overlays; extraction 0973/0974 open with literal `OTHER2` / `OTHER3` banners |
| 1070..1192 | `music_01`/`vab_01` | `vab_01` (121/121 VAB-headed) |

### Exceptions and caveats

Honest residue from the quantitative pass (none of it contradicts the uniform −2):

- `level_up` → extraction 0890 is a DATA_FIELD streaming carrier (its VABs are wrapped inside), so bare-magic checks miss it at *every* shift.
- Extraction 0888 (in `sound_data2`) and 1062 (in `music_01`) are unidentified non-VAB blobs under any shift.
- `other_game`: only 2 of 6 entries carry an `OTHER<n>` banner; the rest are banner-less minigame data, undecidable by name.
- Extraction 0893 (`summon.dat`) opens with a `[u32 2][u16 table…]` shape that mimics a sound-address bank - a shift-0 reading "confirms" `monster_se` on it spuriously; the byte-pins show texture streaming slots. Trust byte-pins over shape coincidences.
- One v12-shaped header sits at extraction 1227 (`other7` region), outside the scene region.
- Opaque names (`other1`/`other4`/`other5`/`other6`/`other7`, `card_data`) are unscoreable; nothing about them conflicts with −2.

## Block names can be misleading

A block name describes the developer's organisation, not the runtime semantics of every entry inside the block - and any name read off an extraction *filename* must first be corrected by the +2 numbering shift above (several historical "mislabeled block" findings - `vab_01` "without VAB headers", the `move_program_no` layout mismatch, `0895_bat_back_dat` = init.pak - were the filename shift, not dev mislabeling). When the block name conflicts with what the bytes actually look like, trust the bytes: re-derive structure from the leading magic + the loader-call constant in SCUS, not from the CDNAME label.

## Per-scene asset reservations

Most scene blocks reserve 6–8 PROT slots for asset variants. Unused slots get filled with the dev placeholder pattern documented in [pochi-filler](pochi.md). The `edstati3` block (likely "ending station 3", possibly cut content) is almost entirely pochi-filled.

## See also

- [PROT TOC](prot.md) - the index this name map labels.
- [Disc layout](disc.md) - the on-disc geometry that holds both files.
- [`subsystems/asset-loader.md`](../subsystems/asset-loader.md) - the loader that resolves CDNAME labels to PROT entries.
- [`subsystems/boot.md`](../subsystems/boot.md) - the boot sequence that loads the TOC.
