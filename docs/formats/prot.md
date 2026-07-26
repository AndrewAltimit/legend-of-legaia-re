# PROT.DAT / DMY.DAT TOC

`PROT.DAT` is the main asset archive - 1233 numbered entries (extraction indices `0..=1232`) containing every TIM, TMD, VAB, MES, ANM, MDT, DATA_FIELD streaming buffer, scene asset table, and runtime overlay. `DMY.DAT` is a sibling archive that turns out to be developer fixtures (memory-bus test pattern + paired random blobs); see [DMY.DAT](dmy.md).

Implementation: `crates/prot/src/archive.rs`.

## Header (8 bytes at offset 0x000 OR 0x800)

```
u32 file_count_minus_1
u32 header_sectors      // size of TOC in 0x800-byte sectors
```

The detector tries offset 0x000 first, then 0x800, accepting whichever yields plausible values. PROT.DAT uses 0x000.

## TOC (immediately after header)

The TOC is a flat array of `u32` words (`toc[]` below - word 0 is the first u32 after the 8-byte header). `p` is the 0-based **entry index** (the extraction index). Each entry contributes exactly one word, its start LBA at `toc[p+2]`; an entry's size is the gap to the next one. For entry index `p`:

```
start_lba    = toc[p + 2]                 // LBA relative to PROT.DAT
size_sectors = toc[p + 3] - toc[p + 2]    // the gap to entry p+1
byte_offset  = start_lba * 0x800
size_bytes   = size_sectors * 0x800
```

That subtraction is retail's own routine, `FUN_8003E68C`, and the loader passes its result straight to the sector read. Implementation: [`legaia_prot::runtime_toc`](../../crates/prot/src/runtime_toc.rs), called once by [`Archive`](../../crates/prot/src/archive.rs) so the arithmetic exists in one place.

**The TOC LBAs are `PROT.DAT`-relative, not absolute disc LBAs.** `byte_offset = start_lba * 0x800` is an offset *into* `PROT.DAT`, and the in-RAM TOC is raw `PROT.DAT` bytes, so the values are position-independent w.r.t. where `PROT.DAT` sits on the disc. Verified by diffing USA against the PAL discs: entry-0's TOC start LBA is identical on every disc despite `PROT.DAT` living at a different disc LBA per region. This is what makes a whole-sector entry-growth **relayout** tractable - growing an interior entry needs only an internal-TOC shift of the later entries' start-LBA words (at `PROT.DAT` byte `8 + (j+2)*4`), not a disc-wide cascade. See [disc.md § Full-ISO relayout](disc.md#full-iso-relayout).

### The entries tile the archive

The sizes above partition `PROT.DAT`: start LBAs strictly ascending, every entry ending where the next begins, and the sizes summing to exactly the span from entry 0's start (LBA 121) to the archive's last sector (LBA 59206) - no gaps, no overlaps. The two words below entry 0, `toc[0]` and `toc[1]`, tile the rest: the boot-resident system-UI region runs from LBA 3, immediately after the 3-sector TOC.

A partition with that property *is* the entry layout, and it is what makes the reading self-defending rather than merely plausible. [`legaia_prot::tiling`](../../crates/prot/src/tiling.rs) states it as a checkable measurement; `crates/prot/tests/archive_tiling_real.rs` runs it against a real disc.

### `toc[p+5] - toc[p+3] + 4` is not an entry's size

That expression - which this project used as the entry size for a long time, taking `max()` of it and the real one - does not measure entry `p` at all. `toc[p+3]` is entry `p+1`'s start LBA and `toc[p+5]` is entry `p+3`'s, so it expands to `size(p+1) + size(p+2) + 4`: the two *following* entries. It exceeds the real size for 931 of the 1233 entries and falls short for the rest.

`Entry::declared_span_sectors` keeps the value for diagnostics, and `prot-extract list` prints it in the `decl_span` column with an `OVR` flag where it overshoots. Nothing parses against it.

Six independent lines of evidence, none of which relies on the definition of a footprint being "the gap to the next entry" (the "entry `p` ends where `p+1` starts" check is a tautology under that definition and is deliberately not cited):

1. **The tiling above.** The declared spans instead total ~2.5x the archive. No partition of a file can exceed the file.
2. **The runtime uses the gap.** `FUN_8003E8A8` returns `TABLE[idx+3] - TABLE[idx+2]`, and `byindex_sync_loader` (`FUN_8003EB98`) passes that straight to the sector read. See [In-RAM TOC](#in-ram-toc) below.
3. **Known-length files agree with it and not with the other expression.** `readef.DAT` is exactly 78 x `0x10800` and `summon.dat` exactly 103 x `0x10800` ([summon-readef.md](summon-readef.md)); every [field map](field-map.md) is exactly `0x12000`, the sum of its four regions; `bse.dat` is 2 sectors, which is what makes it fit the `0x1800`-byte buffer its loader allocates. The `+4` expression gives a non-multiple, a truncation that stops inside the object table, and a 43x buffer overrun respectively.
4. **A capture-pinned boundary.** Each of the six Cort mid-cast save states holds a summon stager byte-resident in the slot-B buffer, matching its file exactly up to the sector gap and diverging after it - stale bytes of the previous occupant. The battle-action overlay's RAM-verified clean-copy prefix, `0x28800`, is likewise exactly PROT 0898's 81 sectors.
5. **Every asset table lands at offset 0.** Reading each entry's own sectors, all 88 scene-asset tables sit at offset 0 of their entry with every descriptor payload inside it. See [the phantom below](#the-prescript-prefixed-asset-table-was-an-over-read).
6. **The one documented counter-example isn't one.** PROT 899's "trailing overlay" finding is the entry size being right and the `+4` expression being short: 14 declared sectors against 74 real ones, the last 60 of which are the title-screen overlay code (see [`subsystems/boot.md`](../subsystems/boot.md#title-overlay-source-on-disc)).

### What reading the neighbours produced

Anything derived from a window that ran past an entry is suspect, and several such claims were load-bearing:

- **The prescript-prefixed asset table was an over-read.** <a id="the-prescript-prefixed-asset-table-was-an-over-read"></a>A `scene_scripted_asset_table` was a descriptor table found at a 0x800-aligned offset inside an entry that led with an event prescript. Every one of those offsets is a sector boundary that is **the next entry's start LBA** - the neighbour's ordinary offset-0 table, seen through the over-read. Reading each entry's own sectors leaves the 88 bare tables untouched and zero scripted ones; the carriers classify as what they are, event-script prescripts. The `0x1000` variant aliases the entry two rows later; [scene-v12-table.md](scene-v12-table.md#the-embedded-man-at-0x1000-is-an-extended-footprint-over-read) recorded two cases of it.
- **Overlay identity by pointer resolution was inflated.** The static-overlay map cross-checks a base by how many of an image's own LUI+ADDIU self-pointers resolve inside it. A longer window widens the acceptance range *and* adds a neighbour's code, so both halves of that ratio were borrowed. On its own sectors PROT 0901 carries three such pairs, not nine.
- **"Shifted copy" claims between adjacent entries.** `0900[0x2800:0x5000] == 0901[0x0:0x2800]` was cited as making PROT 0900 and 0901 shifted images of one library. PROT 0900 is `0x2800` bytes and 0901 begins exactly `0x2800` past its start, so the claim says only that entry 0901's bytes equal entry 0901's bytes.
- **A detector calibrated on the wrong length.** The `init.pak` parser required `>= 0x30000` bytes - PROT 0895's old declared span. The entry is 75 sectors (`0x25800`), so the floor rejected the real file; all four publisher-logo TIMs sit inside it.

[`legaia_prot::archive::Archive`](../../crates/prot/src/archive.rs) now exposes one view that parses: `Entry::size_sectors` / `size_bytes`, read by `Archive::read_entry`. `read_entry_declared_span` reproduces the old window for diagnostics only.

### A `(entry, offset)` pair is only a coordinate if the offset is inside the entry <a id="a-entry-offset-pair-is-only-a-coordinate-if-the-offset-is-inside-the-entry"></a>

The over-read's other legacy is a **naming** one, and it is the family to check first when an asset stops resolving. Every constant of the form "asset X lives at PROT `N` offset `K`" was measured inside the old window. When `K` was past entry `N`'s real end, the pair still named a real place on the disc - just not the one it said. Re-keying it to the entry whose own sectors hold those bytes changes no byte and fixes the read. Three cases, each with the same shape and each previously mistaken for something else:

| Was | Is | What the wrong reading looked like |
|---|---|---|
| World-map kingdom bundle at PROT `0085` / `0244` / `0391` | `0086` / `0245` / `0392` ([`kingdom_bundle`](../../crates/asset/src/kingdom_bundle.rs)) | The bundle's 7-asset table appeared to be "at `0x1800` of the prescript entry". It is at offset 0 of the next entry. |
| Battle-form character atlases at PROT `1204` offset `0x25804 + k*0x8224`, seven of them, the last truncated | PROT `1205` offset `4 + k*0x8224`, **eight**, none truncated ([`battle_char_pack`](../../crates/asset/src/battle_char_pack.rs)) | `0x25800` is 1204's exact length, so `0x25804` is 1205 offset `4`. The window ended between atlas 6 and 7, so the eighth atlas was invisible and its CLUT row (496) read as "intentionally skipped". |
| Title TIM at PROT `0888` `0x1AA28`, with duplicates at `0889` `0x19A28` and `0890` `0x14228` | PROT `0890` `0x14228`, one copy ([`title_pak`](../../crates/asset/src/title_pak.rs)) | All three expressions resolve to the same absolute offset. A whole-archive byte scan for the TIM header finds one hit. |

Two properties make the corrected form checkable, and both are asserted by the disc-gated tests for those modules: the offset plus the asset's length must fit inside the entry, and a payload's own framing (a streaming chunk chain, a descriptor count in the header) must terminate inside it rather than run to the buffer end. A constant that needs a wider buffer than its entry is naming the wrong entry.

> **Historical note.** An earlier Python proof-of-concept used `start_lba = toc[p+5] - toc[p+2]`. That subtraction actually computes a size in sectors and was misinterpreted as the start LBA - under that math `start_lba` collapsed to a small relative offset within "block 0" of the file, and ~80% of PROT entries ended up reading the SAME few low-LBA byte ranges. Anything written using that formula's outputs is artefacted; trust only post-`toc[p+2]` extractions.

## In-RAM TOC

`SCUS_942.54` keeps a transformed copy of the TOC at RAM address `0x801C70F0`. Used at `FUN_8003E8A8` (the LBA resolver):

```c
start_lba    = TABLE[(idx + 2) * 4 + 0x801C70F0]
end_lba      = TABLE[(idx + 3) * 4 + 0x801C70F0]
size_sectors = end_lba - start_lba
```

The in-RAM copy is **raw `PROT.DAT` from byte 0** - `FUN_8003E4E8` reads the first three sectors of `PROT.DAT` into `0x801C70F0` at boot, header words included (byte-verified against a live save state's RAM). There is no transformation; but the **index space differs by 2** from the extraction's:
the extraction (`crates/prot`, and the `NNNN` in `extracted/PROT/NNNN_*.BIN`) builds its
`toc[]` array *after* the two file-header words, so extraction entry `p`'s `start_lba`
sits at file word `p + 4`, while the resolver's `TABLE[(idx + 2)]` is file word `idx + 2`.
Hence `resolver idx = extraction index + 2` - any PROT index recovered from a
`FUN_8003E8A8` argument must subtract 2 to land in extraction space (byte-verified for
the battle side-band files: TOC indices `0x37F`/`0x380` resolve to extraction entries
893/894, see [`summon-readef.md`](summon-readef.md)). Raw-TOC entries 0 and 1 cover the
pre-`init_data` boot-UI region (LBA 3..120) that extraction indexing leaves unindexed:
two TIM-packs holding the boot-resident **system-UI bundle** (menu-glyph atlas, sprite
sheets, cursor parts; uploaded once at boot by `FUN_800198E0` with flat-strip CLUT
semantics, parser `legaia_asset::system_ui_bundle`) - see
[`tim-pack.md` § boot-resident system-UI instance](tim-pack.md#boot-resident-system-ui-instance-raw-toc-entries-0-and-1).
[`CDNAME.TXT`](cdname.md)'s `#define` numbers are authored in this raw-TOC space - the
extractor's filename labels are shifted +2 relative to the content the defines name; see
[`cdname.md` § numbering space](cdname.md#numbering-space) for the evidence and the
consequential relabelings.

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
