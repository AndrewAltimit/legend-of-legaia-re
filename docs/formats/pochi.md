# Pochi-filler placeholder slots

265 of 1232 PROT entries are placeholder slots filled with a developer fill pattern. Detection class: `pochi_filler`. Detector + class: `crates/asset/src/categorize.rs`.

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
