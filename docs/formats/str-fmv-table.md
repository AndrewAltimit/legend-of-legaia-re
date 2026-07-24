# In-RAM STR FMV file table

Which movie plays, from which frame to which frame, and where on screen - the cutscene / MDEC overlay resolves all of it through a lookup table in its data section.

**The catch: there are two tables, and only one of them is the FMV table.** They sit near each other, they both hold per-file records, and the wrong one has been mistaken for the right one before.

1. **libcd directory-record cache** at `0x801CAE08` - the `CdSearchFile` per-directory file cache, an array of PsyQ `CdlFILE`-shape records. This is generic libcd state, *not* an FMV table. (Earlier captures sighted the window at `0x801CAE40`.) See [Directory-record cache](#directory-record-cache-0x801cae08-24-b-cdlfile-records).
2. **FMV dispatch table** at `0x801D0A6C` - 23 entries, 32 bytes each. This is the actual play-engine source.

The dispatch table is what you want. Each record is `[path_ptr, color_depth_flag, start_frame, end_frame, fb_x, fb_y, width, height]`: a path-string pointer into the **path string table** at the overlay start (`0x801CE818`), plus the frame range that slot plays. The play loop `FUN_801CF098` receives one entry from the master dispatch's selector.

Because it is static overlay data, it decodes straight from the disc - no capture needed. Parser: `legaia_asset::fmv_dispatch`.

A third copy of the six MV files appears nearby in full ISO9660 directory-record form (`0x801CCA80`, 56-byte stride) - the raw sectors backing the parsed cache.

## Confidence

**Confirmed.**

The dispatch-table stride and field layout are pinned from the disc bytes of PROT 0970 at its static base (selector `sll v0,v0,0x5` at `0x801CEC9C`), and are byte-identical in the FMV-overlay-resident RAM capture (`overlay_str_fmv.bin`). The play loop's field reads (`FUN_801CF098 +0x38..+0x60`) cross-validate every field.

The retail trigger range (`0..=8`) is pinned independently by the per-STR FMV trigger corpus - nine save states, all with `_DAT_8007BA78 ∈ 0..=8` - which exactly matches the table's nine retail slots.

## Directory-record cache (`0x801CAE08`, 24 B `CdlFILE` records)

What earlier captures read as a "compact MV-file table at `0x801CAE40`" is libcd's directory cache: one PsyQ `CdlFILE` record per file of the last-searched directory,

```text
+0x00  u32       CdlLOC   - byte 0 = BCD minute, 1 = BCD second,
                            2 = BCD frame, 3 = zero
+0x04  u32       size     - file size in bytes (LE)
+0x08  char[16]  name     - "MV1.STR;1\0..." (null-padded)
```

starting with the `.` / `..` entries at `0x801CAE08` / `0x801CAE20`; the first named file record sits at `0x801CAE38`. The earlier name-first 24-byte parse (name at `+0x00`, MSF at `+0x10`) was **phase-shifted 8 bytes**, which paired each name with the *next* record's location - manufacturing the apparent one-entry shift ("`MV1` points at disc `MV2`", "`MV6` points at `XA15.XA`"). At the `CdlFILE` phase every record is self-consistent: `MV1.STR;1` carries MV1's own LBA and size. A title-screen capture shows the same cache holding the `XA` directory (`XA1.XA;1..XA34.XA;1`); the FMV capture shows the `MOV` directory. Convert `CdlLOC` to LBA with the standard identity `LBA = ((M*60)+S)*75 + F - 150`.

The `legaia_asset::str_fmv_table` parser still reads the historical name-first window (its `bcd_msf` is the *following* record's location); treat it as a capture-forensics helper, not a format decoder.

## Path string table (`0x801CE810`, null-terminated)

The dispatch slots' path-pointer field (`+0x00`) points into this packed string table. Nine null-padded paths in storage order:

| Path-table offset | String                |
|------------------:|-----------------------|
| `+0x008`          | `\DATA\MOV.STR;1`     |
| `+0x018`          | `\DATA\MOV15.STR;1`   |
| `+0x02C`          | `\MOV\MV1A.STR;1`     |
| `+0x03C`          | `\MOV\MV6.STR;1`      |
| `+0x04C`          | `\MOV\MV5.STR;1`      |
| `+0x05C`          | `\MOV\MV4.STR;1`      |
| `+0x06C`          | `\MOV\MV3.STR;1`      |
| `+0x07C`          | `\MOV\MV2.STR;1`      |
| `+0x08C`          | `\MOV\MV1.STR;1`      |

Three of the nine paths (`\DATA\MOV.STR;1`, `\DATA\MOV15.STR;1`, `\MOV\MV1A.STR;1`) are dev-only - the corresponding files are not on the retail disc.

## Return-scene labels (`0x801CE8AC`)

The same overlay data section carries seven CDNAME-shape labels: `town0b`, `map01`, `chitei2`, `map02`, `jou`, `uru2`, `town0e`. These are the **post-FMV return scenes**: after the play loop exits, the master dispatch (`FUN_801CEA3C`) copies the label for the just-played `fmv_id` into the next-scene name global `0x80084548`, writes a spawn/door word to `0x80084540`, and hands the game mode back to the field chain (see the mapping table below). They match `legaia_engine_core::scene::FMV_TRIGGER_FIELD_SCENES`.

## FMV dispatch table (`0x801D0A6C`, 23 × 32 B)

The selector lives in the master dispatch `FUN_801CEA3C`:

```text
0x801CEC94: lh   v0, -0x4588(s0)        ; v0 = (s16) _DAT_8007BA78
0x801CEC9C: sll  v0, v0, 0x5            ; v0 = fmv_id * 32
0x801CECA0: jal  FUN_801CF098
0x801CECA4:  _addu a1, v0, 0x801D0A6C   ; param_2 = &dispatch_table[fmv_id]
```

Each 32-byte record:

```text
+0x00  u32  path_ptr     ; -> path string at the overlay start
+0x04  u32  color_flag   ; non-0 = 24-bit color (VRAM footprint width * 3/2)
+0x08  u32  start_frame  ; 1-based; the loop seeks (start-1)*10 sectors in
+0x0C  u32  end_frame    ; last frame of the segment (demux stops here)
+0x10  u32  fb_x         ; VRAM decode-target x (0 on retail slots)
+0x14  u32  fb_y         ; VRAM decode-target y (8 retail; the double-buffer
                         ;   sibling rect sits at fb_y + height)
+0x18  u32  width        ; frame width  (320 retail)
+0x1C  u32  height       ; frame height (240 retail)
```

The play loop opens the file at `path_ptr`, seeks `(start_frame - 1) * 10` sectors in (the 15 fps cadence), and streams until the demuxed frame number reaches `end_frame` - which is how one `MVn.STR` carries several cutscenes by frame range. `path_ptr` resolves into the path-string table at the **overlay start** (`0x801CE818`).

### Every word of the record is a play-loop input

The record has no field reserved for another subsystem and no undecoded margin: `FUN_801CF098`
reads all eight words itself, and each one is traceable to the instruction that consumes it.

| Offset | Consumed at | For |
|---|---|---|
| `+0x00` | `801cf0d0` | `CdSearchFile` on the path string |
| `+0x04` | `801cf100`, `801cf27c`, `801cf2f4`, `801cf478` | the `* 3/2` VRAM scaling, the MDEC depth bit, `DISPENV.isrgb24` |
| `+0x08` | `801cf1b8`, `801cf9e0` | the `(start - 1) * 10` seek, and `StSetStream`'s armed seek |
| `+0x0C` | `801cf788` (in `FUN_801CF740`) | the end-of-stream latch, tested per demuxed frame |
| `+0x10` | `801cf110`, `801cf144` | the decode rect's VRAM x |
| `+0x14` | `801cf168`, `801cf180` | the decode rect's VRAM y |
| `+0x18` | `801cf28c` | the `DISPENV` width |
| `+0x1C` | `801cf168`, `801cf2c8` | the sibling rect at `fb_y + height`, and the display rect's `h = height * 2` |

`FmvEntry` keeps six of the eight; `fb_x` / `fb_y` land in `legaia_mdec::str_player::FmvSlot`,
which builds the two decode rects from them. So the record **is** playback-only, on disassembly
rather than on inference.

The `+0x18` / `+0x1C` pair is the one place the record can be **overruled**. It sizes the decode
rects once, at `FUN_801CF8B0`; from the first demuxed frame onward `FUN_801CF740` overwrites those
rects' width and height from the STR **sector header**'s own `+0x10` / `+0x12` (cached in
`DAT_801D0D50` / `DAT_801D0D54`). A slot whose dimensions disagree with the movie loses. See
[`cutscene.md`](../subsystems/cutscene.md#engine-port---legaia_mdecstr_player).

This table is **static initialised data** in the cutscene overlay (PROT 0970), not a runtime-built structure, so it decodes straight from the disc: `legaia_asset::fmv_dispatch::FmvTable::from_str_overlay` reads it (per-`fmv_id` path + frame range + dimensions), pinned by the disc-gated `fmv_dispatch_real` test. The windowed-cutscene player uses the frame range to seek to the right segment (`cutscene_av::fmv_segment_window`).

An earlier reading used a 64-byte stride (a `sll v0,v0,6` transcription error), pairing wrong slot halves - it concluded `MV2`/`MV5` were never referenced and slots 5..11 pointed at cut files. The disc bytes and the resident RAM capture both encode `sll v0,v0,0x5`; under the 32-byte stride every movie on the disc is dispatched. That reading is **superseded**. The engine resolver `legaia_engine_core::cutscene::fmv_index_to_str_filename` mirrors the corrected nine-slot map; the disc-parsed `FmvTable` remains the authoritative source.

`_DAT_8007BA78` is a `s16` written by the field-VM FMV-trigger op (`0x4C 0xE2 lo hi …`); see [`cutscene.md`](../subsystems/cutscene.md#field-vm-fmv-trigger-op) for the full opcode trace.

### Authoritative runtime mapping

The retail USA build's 23 dispatch slots resolve as:

| `fmv_id` | movie                | frames          | post-play hand-off (`FUN_801CEA3C`) |
|---------:|----------------------|-----------------|-------------------------------------|
| 0        | `\MOV\MV1.STR;1`     | 1..0x53a        | mode `0x16` (22 = CARD init, the menu/memory-card pair) with `_DAT_8007BB00 = 2`; the intro/attract movie |
| 1        | `\MOV\MV2.STR;1`     | 1..0xf4         | scene `town0b`, spawn word `0x0C` |
| 2        | `\MOV\MV3.STR;1`     | 1..0xe1         | scene `map01`, spawn word `0x55` |
| 3        | `\MOV\MV3.STR;1`     | 0xe2..0x1a4     | scene `chitei2`, spawn word `0x2C1` |
| 4        | `\MOV\MV3.STR;1`     | 0x1a5..0x27b    | scene `map02`, spawn word `0xF4` |
| 5        | `\MOV\MV3.STR;1`     | 0x27c..0x36a    | mode 2 with `_DAT_8007B8B8 = 2` (no scene-name write) |
| 6        | `\MOV\MV4.STR;1`     | 1..0x152        | scene `jou`, spawn word `0x276` |
| 7        | `\MOV\MV5.STR;1`     | 1..0x288        | scene `uru2`, spawn word `0x1BC` |
| 8        | `\MOV\MV6.STR;1`     | 1..0x297        | scene `town0e`, spawn word `0x2E5` |
| 9        | `\MOV\MV1A.STR;1`    | 1..0xad4        | dev slot (file not on retail disc); mode `0x16` with `_DAT_8007BB00 = 1` |
| 10       | `\DATA\MOV15.STR;1`  | 1..0xad4        | dev slot; mode 0 |
| 11..=22  | `\DATA\MOV.STR;1`    | 1..0x64         | dev multi-window test slots (varying `fb_x`/`fb_y`/width) |

`MV3.STR`'s four segments abut exactly (`0xe1+1 = 0xe2`, `0x1a4+1 = 0x1a5`, `0x27b+1 = 0x27c`). Slots 9/10 are the only slots with non-default `fb_x`/`fb_y`; slots 11..=22 tile three `MOV.STR` windows across three widths (`0x100`/`0x140`... `0x280`) - a dev display test. The `scene + spawn word` hand-offs write the next-scene name global `0x80084548` (from the label table at `0x801CE8AC`) + `0x80084540` and set game mode 2, so each mid-game FMV returns to a *specific* field scene rather than the trigger scene.

## Per-STR FMV trigger corpus

The current corpus carries nine save states captured RIGHT before each FMV begins playing, one per `_DAT_8007BA78` value (`fmv_id ∈ 0..=8` - exactly the retail slot range). Each save pins:

- `_DAT_8007BA78 = expected_fmv_id` (s16 LE)
- `_DAT_8007B83C = 0x1A` (StrInit game mode)
- `_DAT_8007BAC8 = 2000` (BGM ID; global pool index 0)
- Active scene = `map01` (one of the seven return-scene labels)
- `recover_base()` = `0x80139530` (the `map01` field-pack base)

The `0x4C 0xE2 lo hi` byte sequence does NOT appear in the field-pack RAM region for any save in the corpus - the saves were generated by debug-menu-driven trigger paths, NOT by stepping the field VM through a per-scene FMV trigger op. The corpus pins the trigger-side state across the full `0..=8` range. The two direct `_DAT_8007BA78` store sites (the field-VM op `4C E2` handler and the title-attract tick) are **corpus-exhaustive** - a raw-byte scan of all 1235 PROT entries (every MIPS addressing form, all alignment phases) finds no third store - so the debug path writes the global through the dev-menu's register-pointer editor (`FUN_801DBD04` family, field overlay 0897), which no static addressing-form scan can see.

The corpus is codified at `legaia_engine_core::capture_observations::cutscene_trigger_corpus` and exercised by the disc-gated test `crates/mednafen/tests/real_saves.rs::cutscene_trigger_corpus_pins_fmv_id_across_nine_saves`.

## Per-scene trigger assignment (disc-sourced)

Which `fmv_id` each scene fires is **inline script data**, not a runtime value: the trigger op carries its id as a literal `i16` operand, so walking every scene MAN's partition-1 scripts with the field-VM disassembler recovers the full assignment straight from the disc (`legaia_engine_core::man_field_scripts::scene_fmv_triggers`, the `0x3F`-destination walk's sibling; pinned by the disc-gated `scene_fmv_triggers_disc` test over all 88 scene MANs):

| Scene (extraction entry) | `fmv_id` | movie segment | returns to |
|---|---:|---|---|
| `0004_town01`  | 1 | `MV2.STR`               | `town0b`  |
| `0095_garmel`  | 2 | `MV3.STR` 1..0xe1       | `map01`   |
| `0606_deroa`   | 3 | `MV3.STR` 0xe2..0x1a4   | `chitei2` |
| `0706_chitei2` | 3 | `MV3.STR` 0xe2..0x1a4   | `chitei2` |
| `0218_dohaty`  | 4 | `MV3.STR` 0x1a5..0x27b  | `map02`   |
| `0348_town0d`  | 6 | `MV4.STR`               | `jou`     |
| `0435_uru`     | 7 | `MV5.STR`               | `uru2`    |
| `0689_jouine`  | 8 | `MV6.STR`               | `town0e`  |

One trigger op per scene; no other scene MAN carries one. `fmv_id 0` (the `MV1.STR` intro) fires from the title/new-game path rather than a scene script. `fmv_id 5` (the fourth `MV3.STR` segment, the "stay in mode 2" slot) appears in no scene MAN partition-1 script; the raw-byte `4C E2` candidate for it inside `taiku`'s uncompressed scene structures is the matching suspect (uncontextualized byte match, kept as a candidate). The earlier reading that the `town0d` / `uru` / `jouine` triggers are vestigial pointers at cut movies is **superseded** - under the correct table stride they play `MV4` / `MV5` / `MV6`.

## Rust API

```rust
use legaia_asset::fmv_dispatch::FmvTable;

// Decode the dispatch table straight from the PROT 0970 entry bytes.
let overlay = std::fs::read("extracted/PROT/0970_xxx_dat.BIN")?;
let table = FmvTable::from_str_overlay(&overlay).expect("dispatch table");

// fmv_id (the value the field VM writes to _DAT_8007BA78) -> movie + range.
let e = table.entry(1).unwrap();
assert_eq!(e.engine_path(), "MOV/MV2.STR");
assert_eq!((e.start_frame, e.end_frame), (1, 0xf4));

// Dev slots decode too but engine_path() declines them.
assert_eq!(table.engine_path(10), None); // MOV15.STR, not on the retail disc
```

## Provenance

| Subject                                    | Source |
|---|---|
| Dispatch-table stride + field layout       | PROT 0970 disc bytes at base `0x801CE818` (`asset overlay …`); selector disasm `0x801CEC94..A4`; byte-identical in the `overlay_str_fmv.bin` RAM capture |
| Play-loop field reads                      | `FUN_801CF098` (`see ghidra/scripts/funcs/str0970_801cf098.txt`) |
| Master-dispatch return-scene hand-offs     | `FUN_801CEA3C` (`see ghidra/scripts/funcs/overlay_cutscene_str_0970_801cea3c.txt`) |
| Directory-record cache (`CdlFILE` phase)   | title-screen + FMV-overlay RAM captures; PSX-SPX libcd `CdlFILE` definition |
| Path string table at `0x801CE810`          | FMV-overlay binary data section |
| `fmv_id ∈ 0..=8` range                     | Per-STR FMV trigger corpus (nine save states); `cutscene_trigger_corpus` |
| Trigger-side state at game mode `0x1A`     | Per-STR FMV trigger corpus |
| Residency signature                        | `legaia_engine_core::capture_observations::str_fmv_overlay::is_resident` |

## See also

- [`subsystems/cutscene.md`](../subsystems/cutscene.md) - the STR game modes, demux state machine, and MDEC decode loop.
- [XA audio](xa.md) - the XA-ADPCM audio interleaved with the STR video.
