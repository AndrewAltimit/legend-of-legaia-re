# Pochi-filler placeholder slots

265 of 1232 PROT entries are placeholder slots filled with a developer fill pattern - reserved-but-unused asset slots the game never loads. Detection class: `pochi_filler`. Detector + class: `crates/asset/src/categorize.rs`.

> **These slots are a trap, not just dead weight.** The bytes behind the
> `pochipochi…` prefix are **not zeros** - they are stale mastering scratch, and
> in most scene blocks that scratch parses as a *complete, valid* TIM. Any
> "scan the block for TIMs" sweep that does not skip `pochi_filler` will upload
> a stale texture page over one the scene is actively using. See
> [the scratch tail is live-looking data](#the-scratch-tail-is-live-looking-data---never-load-it).

## Layout

```
+0x000..0x786   ASCII "pochipochi..." (37 lines × 52 bytes + "po" = 1926 bytes)
+0x786          0x1A (DOS EOF marker)
+0x787..end     scratch / leftover data (no consistent format)
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

## The scratch tail is *live-looking* data - never load it

The bytes after the `0x1A` are **not zeros**: they are whatever the mastering
tool left in the sector, and a large share of scene-block pochi slots carry a
complete, well-formed `256 x 256` 4bpp PSX TIM (often two: image blocks at fb
`(768, 0)` and `(832, 0)` with CLUT rows 473 / 479 - the block's battle-side
character pages). They parse. They upload. Retail never touches them: the field
loader dispatches the scene's asset table, not its reserved slots.

An engine-side "scan every entry in the CDNAME block for TIMs" sweep therefore
uploads a page of stale texels *over a page the scene is actively using*. The
collision is not hypothetical - fb `(768, 0)` is tpage `0x0C`, which is where
most field scenes put their **ground-tile atlas** (the per-cell page in the
`.MAP` object record's `+0x15`; see [`world-map.md`](../subsystems/world-map.md)
"Ground texturing"). A pochi upload lands last and erases it, and the ground
quads then sample character / backdrop texels: Jeremi's floor becomes a grid of
grey "tombstones", Mt. Dhini's becomes a repeating vine/crack pattern. Rim Elm
escapes only because its sibling slots are all `scene_tmd_stream` entries, which
the field build already excludes.

The engine's field VRAM pre-pass skips `Class::PochiFiller` entries outright
(`legaia_engine_core::scene_resources`); the disc-gated regression
`field_ground_texture_pages_disc` pins both halves - the hazard exists on the
disc, and the built VRAM does not contain it.

## See also

- [PROT TOC](prot.md) - the index whose unused slots get pochi-filled.
- [DMY.DAT](dmy.md) - the other dev-fixture container in the corpus.
