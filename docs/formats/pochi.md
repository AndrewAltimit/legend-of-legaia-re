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
+0x000..0x786   ASCII "pochipochi..." (37 lines × 52 bytes + "po" = 1926 bytes)
+0x786          0x1A (DOS EOF marker)
+0x787..0x800   scratch / leftover fill to the end of the single sector
```

`pochi` (Japanese `ポチ`) is a generic dog name common in Japanese dev fill - uninitialised memory shows up obviously in a debugger this way.

Detection: `buf.starts_with(b"pochi") && buf[0x786] == 0x1A`.

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
