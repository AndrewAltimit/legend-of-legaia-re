# Pochi-filler placeholder slots

**Not an asset format: a placeholder.** 266 of the 1233 PROT entries are reserved-but-unused slots holding a developer fill pattern, which the game never loads. Each is exactly one 2048-byte sector, and none carries a parseable TIM (measured over all 266, asserted as a test). Detection class `pochi_filler`; detector `legaia_asset::categorize::is_pochi_filler` in `crates/asset/src/categorize.rs`.

## Layout

| Offset | Size | Content | Confidence |
|---|---|---|---|
| `0x000` | `0x784` | 37 lines of `pochi` x 10 (50 ASCII bytes), each closed by CRLF | Confirmed |
| `0x784` | 2 | one empty line (CRLF) | Confirmed |
| `0x786` | 1 | `0x1A`, the DOS EOF marker; the fill file ends here at 1927 bytes | Confirmed |
| `0x787` | 121 | another entry's bytes, showing through (below) | Confirmed |

`pochi` (Japanese `ポチ`) is a generic dog name common in Japanese dev fill - uninitialised memory shows up obviously in a debugger this way.

Detection: `buf.starts_with(b"pochi") && buf[0x786] == 0x1A`. Parser
`legaia_asset::categorize::is_pochi_filler`; the file length is
`POCHI_FILL_LEN`.

`0x786 = 37 * 52 + 2`: the two bytes before the EOF marker are the empty
line's `\r\n`, not a truncated `"po"`.

### The fill file is one file, and it is also a bundle descriptor

All 266 slots carry **byte-identical** bytes through `+0x786` - one 1927-byte
file, written 266 times.

The same 1927 bytes appear again, LZS-compressed to 239 bytes, as the trailing
type-`0x14` `FLAG` descriptor of 28 of the disc's 105 count-prefixed
[scene bundles](scene-bundles.md#the-flag-slot-is-the-pochi-fill-file) - not
only the count-4/5 ones: 9 of 10 count-4, 4 of 4 count-5, 3 of 8 count-6 and
12 of 80 count-7 - and as the *leading* type-`0x0A` descriptor of three more
(`town0c` / `town0d` / `town0e`). The
dispatcher answers a `0x14` with `type << 8` without reading the payload
([`asset-type.md`](asset-type.md)), so those bytes are never decompressed at
runtime: the authoring tool filled a reserved descriptor rather than dropping
it, with the same file it fills a reserved PROT slot with. Pinned by
`crates/asset/tests/byte_account_entries.rs`.

### The 121-byte tail is another entry's bytes

The run from `+0x787` to the end of the sector is **not** fill and not
scratch in the vague sense: for every one of the 266 slots it is
byte-identical to the bytes some *other*, non-filler PROT entry carries at
the same file offset `0x787`. Most often that entry is the immediately
preceding one in TOC order.

This is the same mechanism [`disc-coverage.md`](../tooling/disc-coverage.md)
measures on the overlay images: the mastering buffer is indexed by file
offset and is not cleared between writes, so a file shorter than the buffer
flushes its own bytes followed by whatever the previous, longer file left
there. A 2048-byte sector holding a 1927-byte file leaves exactly 121 bytes
of the previous write showing through.

Two consequences. The tail is **not** evidence of anything about the slot -
it belongs to the donor. And 109 distinct tails across 266 slots is a property
of the packer's write order, not of the slots.

`asset account` claims both regions as `pad` with separate details, so the
whole sector accounts structurally and none of it ranks as work.

## Why so many

These slots cluster at fixed *offsets within their CDNAME block* - typically positions 2, 4, 5, 6 inside a scene's reserved 6-8-slot block. Each scene reserves N PROT slots for asset variants, but most scenes only fill some; unused slots get pochi-filled.

Some scene blocks are almost entirely pochi. The `edstati3` block (likely "ending station 3", possibly cut content) has 36 of ~38 pochi entries.

## How to handle

Treat as known-empty:
- Don't run format detectors against them.
- Don't include in TMD/TIM bulk-scan totals.
- Skip in any "what's still uncategorised" tally.

## The stale-scratch hazard belongs to the next entry

A pochi slot cannot put a texture page anywhere: it has one sector, and its tail is another entry's leftover bytes. The reading "a pochi slot holds stale scratch that parses as a TIM" is falsified; the full account is in [`re-do-not-re-walk.md`](../reference/re-do-not-re-walk.md#pochi-fill-slots-as-stale-mastering-scratch).

The rendering hazard behind that reading is real, and belongs to the **`scene_tmd_stream` entry that follows the pochi slot**:

- Its `FUN_8001FE70` type-`0x01` chunks carry two `64 x 256` pages for framebuffer `(768, 0)` and `(832, 0)` - the block's battle-side character pages, CLUT rows 473 / 479.
- Framebuffer `(768, 0)` is tpage `0x0C`, where most field scenes put their ground-tile atlas (the per-cell page in the `.MAP` object record's `+0x15`; see [`world-map.md`](../subsystems/world-map.md) "Ground texturing").
- Uploaded last in a field build, those pages erase the atlas. Jeremi's floor becomes a grid of grey "tombstones", Mt. Dhini's a repeating vine / crack pattern.
- A sweep that scans every entry of a CDNAME block for TIMs reaches them only by reading past the pochi slot's end, which the superseded entry-size expression allowed ([`prot.md`](prot.md)). Rim Elm is unaffected because its sibling slots are all `scene_tmd_stream` entries, which the field build already excludes.

With an entry sized as the sector gap to its successor, a pochi slot has no reach. The engine's field VRAM pre-pass skips `Class::PochiFiller` entries outright (`legaia_engine_core::scene_resources`). The disc-gated regression `field_ground_texture_pages_disc` pins both halves: every pochi-filler entry is one sector carrying no TIM, and the built VRAM does not contain the neighbour's page.

## See also

- [PROT TOC](prot.md) - the index whose unused slots get pochi-filled.
- [DMY.DAT](dmy.md) - the other dev-fixture container in the corpus.
