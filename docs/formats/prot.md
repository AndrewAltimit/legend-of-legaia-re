# PROT.DAT / DMY.DAT TOC

`PROT.DAT` is the main asset archive: 1233 numbered entries (extraction indices `0..=1232`) holding every TIM, TMD, VAB, MES, ANM, MDT, DATA_FIELD streaming buffer, scene asset table and runtime code overlay. It has no filenames and no per-entry size field. The file opens with a table of contents (TOC) that is a flat list of start sectors, and an entry is the run of `0x800`-byte sectors from its own start to the next entry's start. `DMY.DAT` uses the same container for developer fixtures ([`dmy.md`](dmy.md)).

Implementation: [`crates/prot`](../../crates/prot/README.md) - `archive.rs` (reader), `runtime_toc.rs` (the size routine), `tiling.rs` (the partition check).

## At a glance

| Offset | Size | Field | Meaning | Confidence |
|---|---|---|---|---|
| `+0x00` | u32 | unused | Zero in both retail archives | Confirmed |
| `+0x04` | u32 | `lba_row_count - 1` | TOC row count minus one: 1235 in `PROT.DAT`, 15 in `DMY.DAT` | Confirmed |
| `+0x08` | u32 | `toc[0]` = `header_sectors` | Header + TOC size in sectors (3), which is also the first content LBA | Confirmed |
| `+0x0C` | u32 | `toc[1]` | Start LBA of the second boot-UI region | Confirmed |
| `+0x10 + p*4` | u32 | `toc[p+2]` | Start LBA of entry `p`, relative to `PROT.DAT` | Confirmed |
| after the last entry | u32 | end terminator | The archive's sector count (`toc[1235] = 59206`) | Confirmed |

The header sits at file offset `0x000` in both retail archives. `detect_header` also tries `0x800` and accepts whichever offset yields plausible values.

## TOC

`toc[]` is the `u32` array that starts at `+0x08`, so the `header_sectors` word doubles as `toc[0]`. Both readings hold by construction: the first LBA row is the first content sector, which is the sector right after the TOC. `detect_header` uses the word as a sector count to size its TOC read; retail's resolver indexes from `toc[2]` upwards and never reads it as a size.

```
file word:   0        1          2        3        4        5      ...    1237
           +--------+----------+--------+--------+--------+--------+---+-----------+
           | unused | rows - 1 | toc[0] | toc[1] | toc[2] | toc[3] |...| toc[1235] |
           +--------+----------+--------+--------+--------+--------+---+-----------+
                                 boot-UI regions   entry 0  entry 1      end LBA
                                 (LBA 3..120)      start    start

size(entry p) = toc[p+3] - toc[p+2]      one word per entry; the next word bounds it
```

`lba_row_count` counts TOC rows, not entries. `PROT.DAT`'s 1236 rows are `toc[0]` and `toc[1]` (the two boot-UI regions), 1233 entry start LBAs, and one end terminator. `DMY.DAT`'s 16 rows resolve to 13 entries the same way.

`p` is the 0-based **entry index** (the extraction index, the `NNNN` in `extracted/PROT/NNNN_*.BIN`). For entry `p`:

```
start_lba    = toc[p + 2]                 // LBA relative to PROT.DAT
size_sectors = toc[p + 3] - toc[p + 2]    // the gap to entry p+1
byte_offset  = start_lba * 0x800
size_bytes   = size_sectors * 0x800
```

That subtraction is retail's own leaf routine, `FUN_8003E68C`, and the loader passes the same quantity straight to the sector read. Implementation: [`legaia_prot::runtime_toc`](../../crates/prot/src/runtime_toc.rs), called once by [`Archive`](../../crates/prot/src/archive.rs) so the arithmetic exists in one place. `Archive` skips a row whose start LBA is zero or whose span is zero, wraps negative, or runs past the file - the zero-padded TOC tail.

**The TOC LBAs are `PROT.DAT`-relative, not absolute disc LBAs.** `byte_offset` is an offset *into* `PROT.DAT`, and the in-RAM TOC is raw `PROT.DAT` bytes, so the values do not depend on where `PROT.DAT` sits on the disc: entry 0's start LBA is identical on the USA and PAL discs although the file lives at a different disc LBA per region. That is what makes a whole-sector entry-growth **relayout** tractable - growing an interior entry needs only a shift of the later entries' start-LBA words (at `PROT.DAT` byte `8 + (j+2)*4`), not a disc-wide cascade. See [disc.md § Full-ISO relayout](disc.md#full-iso-relayout).

### The entries tile the archive

The sizes above partition `PROT.DAT`: start LBAs strictly ascending, every entry ending where the next begins, and the sizes summing to exactly the span from entry 0's start (LBA 121) to the archive's last sector (LBA 59206) - no gaps, no overlaps. The two words below entry 0, `toc[0]` and `toc[1]`, tile the rest: the boot-resident system-UI region runs from LBA 3, immediately after the 3-sector TOC.

A partition with that property *is* the entry layout, and it is what makes the reading self-defending rather than merely plausible. [`legaia_prot::tiling`](../../crates/prot/src/tiling.rs) states it as a checkable measurement; `crates/prot/tests/archive_tiling_real.rs` runs it against a real disc.

### `toc[p+5] - toc[p+3] + 4` is not an entry's size

That expression does not measure entry `p` at all. `toc[p+3]` is entry `p+1`'s start LBA and `toc[p+5]` is entry `p+3`'s, so it expands to `size(p+1) + size(p+2) + 4`: the two *following* entries. It exceeds the real size for 931 of the 1233 entries and falls short for the rest.

`Entry::declared_span_sectors` keeps the value for diagnostics, `prot-extract list` prints it in the `decl_span` column with an `OVR` flag where it overshoots, and `Archive::read_entry_declared_span` reproduces the window. Nothing parses against it: `Entry::size_sectors` / `size_bytes`, read by `Archive::read_entry`, is the one view that parses.

Six independent lines of evidence for the gap reading. None relies on defining a footprint as "the gap to the next entry" (under that definition "entry `p` ends where `p+1` starts" is a tautology, so it is not cited):

1. **The tiling above.** The `+4` spans total about 2.5x the archive. No partition of a file can exceed the file.
2. **The runtime uses the gap.** `FUN_8003E8A8` returns `TABLE[idx+3] - TABLE[idx+2]`, and `byindex_sync_loader` (`FUN_8003EB98`) passes that straight to the sector read. See [In-RAM TOC](#in-ram-toc).
3. **Known-length files agree with it.** `readef.DAT` is exactly 78 x `0x10800` and `summon.dat` exactly 103 x `0x10800` ([summon-readef.md](summon-readef.md)); every [field map](field-map.md) is exactly `0x12000`, the sum of its four regions; `bse.dat` is 2 sectors, which is what makes it fit the `0x1800`-byte buffer its loader allocates. The `+4` expression gives a non-multiple, a truncation that stops inside the object table, and a 43x buffer overrun respectively.
4. **A capture-pinned boundary.** Each of the six Cort mid-cast save states holds a summon stager byte-resident in the slot-B buffer, matching its file exactly up to the sector gap and diverging after it (stale bytes of the previous occupant). The battle-action overlay's RAM-verified clean-copy prefix, `0x28800`, is likewise exactly PROT 0898's 81 sectors.
5. **Every asset table lands at offset 0.** Reading each entry's own sectors, all 88 scene-asset tables sit at offset 0 of their entry with every descriptor payload inside it.
6. **PROT 899 is not a counter-example.** Its `+4` span is 14 sectors against 74 real ones; the last 60 are the title-screen overlay code ([`subsystems/boot.md`](../subsystems/boot.md#title-overlay-source-on-disc)).

> **Not `toc[p+5] - toc[p+2]` either.** That subtraction is a size in sectors, not a start LBA: used as one, it collapses to a small offset inside the file's first block, and about 80% of entries read the same few low-LBA byte ranges.

### Readings that only exist in an over-read window

A window that runs past an entry appends its neighbours, and a parser pointed at it finds structure that belongs to them. These four readings are artifacts of that and do not hold on an entry's own sectors:

- **The prescript-prefixed asset table.** <a id="the-prescript-prefixed-asset-table-was-an-over-read"></a>A `scene_scripted_asset_table` was a descriptor table at a `0x800`-aligned offset inside an entry that led with an event prescript. Each such offset is a sector boundary that is **the next entry's start LBA** - the neighbour's ordinary offset-0 table. On their own sectors there are 88 bare tables and zero scripted ones; the carriers classify as event-script prescripts. The `0x1000` variant aliases the entry two rows later ([scene-v12-table.md](scene-v12-table.md#the-embedded-man-at-0x1000-is-an-extended-footprint-over-read)).
- **Overlay identity by pointer resolution.** The static-overlay map cross-checks a base by how many of an image's own LUI+ADDIU self-pointers resolve inside it. A longer window widens the acceptance range *and* adds a neighbour's code. On its own sectors PROT 0901 carries three such pairs, not nine.
- **"Shifted copy" between adjacent entries.** `0900[0x2800:0x5000] == 0901[0x0:0x2800]` does not make PROT 0900 and 0901 shifted images of one library. PROT 0900 is `0x2800` bytes and 0901 begins exactly `0x2800` past its start, so the equality compares entry 0901 with itself.
- **A length floor calibrated on the window.** PROT 0895 (`init.pak`) is 75 sectors (`0x25800`), not `0x30000`; all four publisher-logo TIMs sit inside it.

### A `(entry, offset)` pair is only a coordinate if the offset is inside the entry <a id="a-entry-offset-pair-is-only-a-coordinate-if-the-offset-is-inside-the-entry"></a>

This is the family to check first when an asset stops resolving. A constant of the form "asset X lives at PROT `N` offset `K`" that was measured inside an over-read window can have `K` past entry `N`'s real end. The pair still names a real place on the disc - just not the entry it says. Re-keying it to the entry whose own sectors hold those bytes changes no byte and fixes the read. Three cases with that shape:

| Over-read coordinate | Real coordinate | What the over-read looked like |
|---|---|---|
| World-map kingdom bundle at PROT `0085` / `0244` / `0391` | `0086` / `0245` / `0392` ([`kingdom_bundle`](../../crates/asset/src/kingdom_bundle.rs)) | The bundle's 7-asset table appeared to be "at `0x1800` of the prescript entry". It is at offset 0 of the next entry. |
| Battle-form character atlases at PROT `1204` offset `0x25804 + k*0x8224`, seven of them, the last truncated | PROT `1205` offset `4 + k*0x8224`, **eight**, none truncated ([`battle_char_pack`](../../crates/battle-models/src/battle_char_pack.rs)) | `0x25800` is 1204's exact length, so `0x25804` is 1205 offset `4`. The window ended between atlas 6 and 7, so the eighth atlas was invisible and its CLUT row (496) read as "intentionally skipped". |
| Title TIM at PROT `0888` `0x1AA28`, with duplicates at `0889` `0x19A28` and `0890` `0x14228` | PROT `0890` `0x14228`, one copy ([`title_pak`](../../crates/asset/src/title_pak.rs)) | All three expressions resolve to the same absolute offset. A whole-archive byte scan for the TIM header finds one hit. |

Two properties make the corrected form checkable, and both are asserted by the disc-gated tests for those modules: the offset plus the asset's length must fit inside the entry, and a payload's own framing (a streaming chunk chain, a descriptor count in the header) must terminate inside it rather than run to the buffer end. A constant that needs a wider buffer than its entry is naming the wrong entry.

## In-RAM TOC

`SCUS_942.54` keeps a copy of the TOC at RAM address `0x801C70F0`. Read at `FUN_8003E8A8` (the TOC resolver):

```c
start_lba    = TABLE[(idx + 2) * 4 + 0x801C70F0]
end_lba      = TABLE[(idx + 3) * 4 + 0x801C70F0]
size_sectors = end_lba - start_lba
```

The routine **returns the span, not the LBA**: it stores `start_lba` at `gp+0x8f0` and folds it into the CD position the read consumes (`msf_to_lba(base) + start_lba`, back through `lba_to_msf`), leaving `size_sectors` in `v0`. That is why `FUN_8003EB98` can hand its return value straight to `FUN_8003E800` as a sector count.

The in-RAM copy is **raw `PROT.DAT` from byte 0**: `FUN_8003E4E8` reads the first three sectors of `PROT.DAT` into `0x801C70F0` at boot, header words included (byte-verified against a live save state's RAM). Nothing is transformed, but the **index space differs by 2** from the extraction's:

| Index space | Entry's start LBA is at | Used by |
|---|---|---|
| Extraction index `p` | `toc[p + 2]` = file word `p + 4` | `crates/prot`, the `NNNN` in `extracted/PROT/NNNN_*.BIN`, these docs |
| Resolver / raw-TOC index `idx` | `TABLE[idx + 2]` = file word `idx + 2` | `FUN_8003E8A8` / `FUN_8003E68C` arguments, [`CDNAME.TXT`](cdname.md) `#define` numbers |

So `resolver idx = extraction index + 2`: a PROT index recovered from a `FUN_8003E8A8` argument must subtract 2 to land in extraction space. The battle side-band files byte-verify it - TOC indices `0x37F` / `0x380` resolve to extraction entries 893 / 894 ([`summon-readef.md`](summon-readef.md)).

Raw-TOC entries 0 and 1 cover the boot-UI region (LBA 3..120) that extraction indexing leaves unindexed. They are two TIM packs holding the boot-resident **system-UI bundle** (menu-glyph atlas, sprite sheets, cursor parts), uploaded once at boot by `FUN_800198E0` with flat-strip CLUT semantics; parser `legaia_asset::system_ui_bundle`, layout in [`tim-pack.md`](tim-pack.md#boot-resident-system-ui-instance-raw-toc-entries-0-and-1).

Because `CDNAME.TXT` is authored in raw-TOC space, the extractor's filename labels are shifted +2 relative to the content the defines name. [`cdname.md` § numbering space](cdname.md#numbering-space) has the evidence and the relabelings that follow.

## Resolving entries by name vs by index

Two entry points:

- `FUN_8003E8A8` - index-based (consumed directly by the streaming loader and the dev-build sound branch).
- `FUN_8003E6BC` - path-based; resolves dev paths like `data\battle\efect.dat` or `h:\PROT\FIELD\<scene>\…` into an index via the CDNAME-driven name map, then delegates to the LBA resolver. Most retail-build code paths land here.

Names come from [`CDNAME.TXT`](cdname.md), which lives at the top level of the disc.

## Overlay loaders (parallel slots)

Two paired wrappers on top of `FUN_8003E8A8` + `FUN_8003E800` (async LBA-based loader) manage two **independently swappable** overlay slots. Both call `FUN_8003E8A8(param + 0x381)` - which, per the index-space note above, is **extraction entry `param + 0x37F`** (e.g. param 2 → 0897 field, 3 → 0898 battle, 4 → 0899 menu):

| Loader | Destination buffer ptr | Current-id tracker |
|---|---|---|
| `FUN_8003EBE4` | `*DAT_8001038C` | `gp+0x924` |
| `FUN_8003EC70` | `*DAT_80010390` | `gp+0x934` |

This means two overlays can be RAM-resident at the same time (e.g., a title-overlay code blob in slot A and a sister asset blob in slot B). Mode-init handlers use one or the other depending on what they're loading. The full CD-read API stack that backs these is documented in [`subsystems/boot.md` § CD-read API stack](../subsystems/boot.md#cd-read-api-stack).

`FUN_8003E360` shows a **dual-mode loader pattern**: in retail (`_DAT_8007B8C2 != 0`, the value retail boots with) it loads via the PROT TOC index (`FUN_8003E8A8` / `FUN_8003E800`); in dev (`_DAT_8007B8C2 == 0`) it opens an `h:\` path through `FUN_800608F0` / `FUN_80060944`, where `FUN_800608F0` is a `break 0x103` dev-station host trap. Only the retail branch runs on a real disc.

## See also

- [Disc layout](disc.md) - the Mode2/2352 geometry that holds PROT.DAT.
- [CDNAME map](cdname.md) - the name labels for PROT indices.
- [LZS compression](lzs.md) - the decompression most entries need.
- [Asset-type dispatch](asset-type.md) - the per-entry type-byte handler.
- [`tooling/extraction.md`](../tooling/extraction.md) - the extraction pipeline that walks the TOC.
