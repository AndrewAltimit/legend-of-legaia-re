# Pochi-filler placeholder slots

266 of 1233 PROT entries are placeholder slots filled with a developer fill pattern - reserved-but-unused asset slots the game never loads. Detection class: `pochi_filler`. Detector + class: `crates/asset/src/categorize.rs`.

> **The "stale scratch that parses as a TIM" reading is falsified.** Every one
> of the 266 slots is exactly **one 2048-byte sector**, and **zero** of them
> carry a parseable TIM - measured over the whole corpus, and asserted as a
> test. A pochi slot is a reserved sector of fill and nothing else.
>
> The rendering bug that produced the old reading was real: two `64x256` pages
> at framebuffer `(768,0)` and `(832,0)` erased a scene's ground atlas. They
> came from the **`scene_tmd_stream` entry that follows the pochi slot**,
> reached through the entry-size expression that spanned neighbouring entries
> (corrected in [`prot.md`](prot.md)). The symptom was attributed to the slot
> a sweep was standing on rather than to the entry it over-read into. See
> [the hazard's real source](#the-stale-scratch-hazard-belongs-to-the-next-entry).

## Layout

```
+0x000..0x784   37 lines of "pochi" x 10 (50 ASCII bytes) each closed by CRLF
+0x784..0x786   one empty line (CRLF)
+0x786          0x1A (DOS EOF marker)   -- the fill file ends here, at 1927 bytes
+0x787..0x800   121 bytes that are NOT fill (see below)
```

`pochi` (Japanese `ポチ`) is a generic dog name common in Japanese dev fill - uninitialised memory shows up obviously in a debugger this way.

Detection: `buf.starts_with(b"pochi") && buf[0x786] == 0x1A`. Parser
`legaia_asset::categorize::is_pochi_filler`; the file length is
`POCHI_FILL_LEN`.

The two bytes before the EOF marker are that empty line, not the truncated
`"po"` an earlier reading of this page recorded. Both readings arrive at
`0x786 = 37 * 52 + 2`; they differ on what the `+ 2` is, and the bytes say
`\r\n`.

### The fill file is one file, and it is also a bundle descriptor

All 266 slots carry **byte-identical** bytes through `+0x786` - one 1927-byte
file, written 266 times.

The same 1927 bytes appear once more, LZS-compressed, as the type-`0x14`
descriptor slot of every count-4 and count-5
[scene bundle](scene-bundles.md#the-flag-slot-is-the-pochi-fill-file). The
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
it belongs to the donor, and reading it as content of the slot is how the
falsified "stale scratch" reading above got its start. And 109 distinct tails
across 266 slots is a property of the packer's write order, not of the slots.

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

A pochi slot's own tail - the bytes between the `0x1A` and the end of its single
sector - is fill. There is no second sector for a stale asset to sit in, and
nothing in the 266-slot corpus parses as a TIM. The slot cannot put a page
anywhere.

The **rendering hazard is real**, and it is worth keeping because the symptom
points at the wrong entry. Two `64 x 256` pages land at framebuffer `(768, 0)`
and `(832, 0)` - the block's battle-side character pages, CLUT rows 473 / 479 -
and fb `(768, 0)` is tpage `0x0C`, where most field scenes put their
**ground-tile atlas** (the per-cell page in the `.MAP` object record's `+0x15`;
see [`world-map.md`](../subsystems/world-map.md) "Ground texturing"). Uploaded
last, they erase the atlas and the ground quads sample character / backdrop
texels: Jeremi's floor becomes a grid of grey "tombstones", Mt. Dhini's a
repeating vine/crack pattern.

Those pages are the **`scene_tmd_stream` entry that follows the pochi slot** -
its `FUN_8001FE70` type-`0x01` chunks. A "scan every entry in the CDNAME block
for TIMs" sweep reached them by standing on the pochi slot and reading past its
end, under the superseded entry-size expression ([`prot.md`](prot.md)). Rim Elm
escapes because its sibling slots are all `scene_tmd_stream` entries, which the
field build already excludes - which is the same statement about the neighbour,
read as a statement about the slot.

With an entry sized as the sector gap to its successor, a pochi slot has no
reach at all. The engine's field VRAM pre-pass skips `Class::PochiFiller`
entries outright (`legaia_engine_core::scene_resources`); the disc-gated
regression `field_ground_texture_pages_disc` pins both halves - every
pochi-filler entry is one sector carrying no TIM, and the built VRAM does not
contain the neighbour's page.

## See also

- [PROT TOC](prot.md) - the index whose unused slots get pochi-filled.
- [DMY.DAT](dmy.md) - the other dev-fixture container in the corpus.
