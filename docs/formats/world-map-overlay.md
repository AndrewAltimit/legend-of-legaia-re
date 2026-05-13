# Slot-4 records (unidentified)

> **Status: research artifact, runtime semantic open.** The container
> layout below is byte-verified against live RAM, but every hypothesis
> we've tested for what the *records inside* mean has been falsified.
> Visual inspection of every projection (xz / xy / zy) and every
> topology interpretation (points / pair-edges / row-major polylines /
> column-major polylines / heightfield grid) failed to reproduce the
> dev-menu top-view, a continent coastline, or any other recognizable
> 2D map structure. The data is heterogeneous: some bodies clearly
> carry 3D mesh geometry (vertical pillar/column silhouettes visible
> in the xy projection), others are flat or corner-clustered. Slot 4
> is most likely a runtime library of small object-local meshes /
> collision hulls / decorations / particle-emitter shapes, **not**
> world-map outline data. Pinning the consumer in Ghidra is the
> remaining unblock.

Slot 4 of each world-map (kingdom) bundle decompresses to a fixed-size
buffer that the runtime loads verbatim into RAM. Three carriers:

| Bundle | PROT index | CDNAME label | Decoded size |
|---|---|---|---:|
| Drake | 0085 | `map01` | 32304 |
| Sebucus | 0244 | `map02` | 26964 |
| Karisto | 0391 | `map03` | 24444 |

The 7-asset bundle is the standard
[`scene_asset_table`](scene-bundles.md#scene_asset_table---canonical-7-asset-bundle)
shape with type sequence `(1, 2, 3, 4, 5, 6, 7)`. Slot 4's type byte
is `0x05` per [asset-type](asset-type.md) - the standard table calls
this "MOVE", but the kingdom-bundle consumer interprets the bytes as
something else (see "Falsified hypotheses" below).

## Container layout (confirmed)

### Outer pack

```text
+0x00   u32  count                ; number of sub-bodies
+0x04   u32  byte_offsets[count]  ; absolute byte offset into the
                                  ; decoded payload (NOT word offsets,
                                  ; unlike the slot-1 TMD pack)
+offset bodies[count]             ; contiguous sub-bodies
```

Drake decodes to 32304 bytes with `count = 15`. First entry is
`0x40 = 4 + 4*15` (right after the header).

### Sub-body header (8 bytes)

```text
+0x00   u8   count_a              ; records per group
+0x01   u8   flag_a               ; usually 0; 1 for kind=4 bodies
+0x02   u8   count_b              ; number of groups
+0x03   u8   flag_b               ; usually 0
+0x04   u16  marker               ; constant 0x080C across all bodies
+0x06   u16  kind                 ; 1, 2, or 4 (semantic ambiguous)
```

### Body payload

```text
+0x08   record[count_a * count_b] ; each record is 8 bytes
                                  ; ( i16 x, i16 y, i16 z, i16 attr )
+...    trailer (8 bytes)         ; always 8 zero bytes
```

Total body size is always `8 + count_a * count_b * 8 + 8`. The math
fits every body in all three kingdoms exactly. The container layout
above is fully confirmed; what's **not** confirmed is how the runtime
*interprets* the 8-byte records.

## Per-kingdom body inventory

Drake = 15 bodies; Sebucus = 16; Karisto = 16. The leading three
bodies (`kind = 1`, `count_a = 10`) are byte-identical templates
across all three kingdoms - whatever they encode, the engine ships
the same generic shape in every bundle.

### Drake (`map01`, PROT 0085)

| Body | count_a | count_b | kind | flag_a | records | X span | Y span | Z span |
|---|---|---|---|---|---|---:|---:|---:|
| 0 | 10 | 20 | 1 | 0 | 200 | 16626 | 10767 | 38641 |
| 1 | 10 | 20 | 1 | 0 | 200 | - | - | - |
| 2 | 10 | 30 | 1 | 0 | 300 | - | - | - |
| 3 | 2 | 30 | 2 | 0 | 60 | 0 | 0 | 0 (pinned plane) |
| 4 | 2 | 20 | 2 | 0 | 40 | - | - | - |
| 5 | 10 | 30 | 2 | 0 | 300 | - | - | - |
| 6 | 10 | 26 | 2 | 0 | 260 | - | - | - |
| 7 | 10 | 30 | 2 | 0 | 300 | - | - | - |
| 8 | 10 | 3 | 2 | 0 | 30 | - | - | - (3 identical groups - filler/padding) |
| 9 | 12 | 30 | 2 | 0 | 360 | 10725 | **25856** | 21248 |
| 10 | 12 | 30 | 2 | 0 | 360 | 13056 | 18432 | 31503 |
| 11 | 12 | 10 | 2 | 0 | 120 | 11492 | **27648** | 24064 |
| 12 | 10 | 120 | 2 | 0 | 1200 | 16118 | 4096 | 31473 |
| 13 | 14 | 15 | 4 | 1 | 210 | 65485 | 14336 | 64512 |
| 14 | 2 | 30 | 2 | 0 | 60 | - | - | - |

Bodies 9 / 10 / 11 have Y spans comparable to the X/Z scale - 3D mesh
extent, not 2D contour data. Body 12 is nearly flat (Y span 4K).
Body 13 reaches the full ±32K world bounds on X and Z and clusters in
the corners.

### Sebucus (`map02`, PROT 0244)

| Body | count_a | count_b | kind | flag_a | records |
|---|---|---|---|---|---|
| 0-3 | 10/10/10/2 | 20/20/30/30 | 1/1/1/2 | 0 | 200/200/300/60 |
| 4-7 | 10/10/10/10 | 30/26/30/3 | 2/2/2/2 | 0 | 300/260/300/30 |
| 8-11 | 11/11/1/1 | 30/15/30/15 | 4/4/4/4 | 1 | 330/165/30/15 |
| 12-15 | 12/12/12/10 | 30/30/10/30 | 2/2/2/2 | 0 | 360/360/120/300 |

### Karisto (`map03`, PROT 0391)

| Body | count_a | count_b | kind | flag_a | records |
|---|---|---|---|---|---|
| 0-3 | 10/10/10/1 | 20/20/30/15 | 1/1/1/2 | 0 | 200/200/300/15 |
| 4-7 | 14/14/11/11 | 15/15/30/15 | 4/4/4/4 | 1 | 210/210/330/165 |
| 8-11 | 1/1/10/10 | 15/30/5/15 | 4/4/2/4 | 1/1/1/1 | 15/30/50/150 |
| 12-15 | 12/12/12/10 | 30/30/10/30 | 2/2/2/2 | 0 | 360/360/120/300 |

## RAM layout (confirmed)

Slot 4 is loaded **verbatim into RAM** with zero per-byte diffs vs
disc. Drake's payload starts at `0x8011A624` (the outer pack header)
and ends at `0x80122454` exclusive - exactly 32304 bytes, matching
the disc-decoded length. Body 0's records start at `0x8011A664`
(`0x40` past the base, after the 4-byte count and 15 × 4-byte
offsets). No runtime fixup is applied.

Verified by `scripts/pcsx-redux/diff_slot4_ram_vs_disc.py` against a
PCSX-Redux save state: every byte of all 15 bodies matches the
disc-side LZS-decoded payload. The load base was pinned by signature-
searching the full 2 MiB main RAM for the 64-byte outer pack header
(count = 15 followed by `byte_offsets[0..15]`) - see
`scripts/pcsx-redux/autorun_dump_full_ram.lua` for the procedure.

## Falsified hypotheses

The container is solved. **What slot 4 *encodes* is not.** Three
interpretations were systematically tested and falsified by visual
inspection (PNG renders of every body × every projection plane ×
every topology mode):

1. **Top-down dev-menu wireframe / continent coastline.** The
   strongest historical claim: that body 12 traces a continent
   coastline, body 13 traces the world boundary frame, and the
   remaining bodies are inner contours / decorative outlines visible
   in the developer top-view. Projecting all 15 bodies onto `xz`
   produces no recognizable map silhouette in any kingdom; matching
   PNG renders against the dev-menu top-view captured from PCSX-Redux
   save states found no agreement.

2. **`count_a × count_b` heightfield grid.** Drake body 12 records
   pair up at fixed X-bands and consecutive groups looked like
   differential Z-updates over a shared topology, suggesting a
   coarse 10 × 120 terrain mesh. Rendering body 12 as a grid quad-
   mesh wireframe (`row + column edges`) produces the wrong silhouette;
   pair-wise edge interpretation (`(r0,r1) (r2,r3) ...`) likewise
   doesn't yield a coastline-like contour.

3. **Heterogeneous via a single non-`xz` projection.** Rendering on
   `xy` (front side view) surfaces clean vertical pillar/column
   silhouettes for bodies 9 and 11 (which have 25K-27K Y span). The
   `xy` all-bodies overlay also looks more map-like than `xz` overall.
   But no single axis pair produces a coherent map across **all** 15
   bodies, and the recognizable shapes in `xy` look like 3D objects
   seen sideways - not map outlines.

## Current working hypothesis

Slot 4 is most likely a **runtime library of small object-local 3D
meshes** the world-map controller / dev-menu top-view places at world
coordinates. Plausible roles for individual bodies: collision hulls,
instantiable decoration meshes, particle-emitter shapes, debug-overlay
geometry, or animation rigs. This is consistent with:

- bodies 9 / 11 having full 3D mesh-scale Y extents while body 12 is
  near-flat (different kinds of objects, not 2D contour vs 2D outline)
- the leading three bodies being byte-identical templates across all
  three kingdoms (shared generic objects, not kingdom-specific data)
- the corner-clustered point distribution in body 13 (kind = 4):
  could be four corner-anchored objects, not a single ±32K boundary
  frame
- the in-game-object silhouettes visible in side projections - the
  user identified body-9 features that resemble specific game props

**Pinning the consumer in Ghidra is the unblock.** The reader hasn't
appeared as a direct `LUI+ADDIU` reference to `0x8011A624` in any
captured world-map overlay, suggesting it accesses the buffer via a
runtime pointer rather than a hardcoded address. The next move is
either (a) dynamic memory-watchpoint capture of which function reads
this RAM range during top-view, or (b) static sweep of the captured
overlays for any 8-byte stride iterator that walks a `count_a *
count_b` array starting from the loaded buffer.

## Tooling

These remain useful for future RE work even though their original
purpose ("render the world-map wireframe") is no longer valid:

| Tool | Role |
|---|---|
| `cargo run -p legaia-asset --bin asset -- slot4-png --input <PROT>.BIN --out <png>` | Container PNG renderer. `--style row\|col\|pairs\|grid\|points` toggles between topology interpretations; `--axes xz\|xy\|zy` switches projection plane; `--only-body N` / `--frame-body N` isolate a single body. `--from-raw <bin>` renders a previously-dumped slot-4 payload. |
| `cargo run -p legaia-asset --bin asset -- kingdom-slot <PROT>.BIN --slot 4` | Per-body inventory dump (counts, ranges, kind / flag_a). |
| `legaia_asset::world_map_overlay::{parse, top_down_lines, record_points, body_axis_range}` | Rust API. |
| `scripts/pcsx-redux/run_dump_slot4.sh` + `autorun_dump_slot4.lua` | PCSX-Redux closed-loop dumper: loads a save state, dumps the live slot-4 RAM region, quits. |
| `scripts/pcsx-redux/autorun_dump_full_ram.lua` | Full 2 MiB main RAM dump. Use when the load base is unknown for a new build / state - signature-scan the dump for the 64-byte outer pack prefix. |
| `scripts/pcsx-redux/diff_slot4_ram_vs_disc.py` | Byte-compare a RAM dump against the disc-decoded payload. |

The world-overview web viewer no longer exposes slot 4 - the previous
"show slot-4 wireframe" toggle was removed once the wireframe
hypothesis was falsified. Re-enable from the WASM exports
(`slot4_wireframe_lines` / `slot4_wireframe_points` /
`slot4_wireframe_bounds`) if a future RE pass identifies the correct
draw interpretation.

## Open work

1. **Pin the consumer in Ghidra.** The slot-4 reader hasn't been
   located. Candidate sites: dynamic memory-watchpoint capture, or
   static sweep of captured overlays for stride-8 iterators.
2. **Identify each body.** With the consumer in hand, walking each
   body through the actual draw call should immediately reveal what
   each `kind / flag_a` combination means.
3. **`kind` / `flag_a` semantic.** `kind = 4` always has `flag_a = 1`
   in every kingdom; the reverse doesn't hold (Karisto body 10 is
   `kind = 2, flag_a = 1`).
4. **Per-record 4th `i16` (`attr`).** 0 for body 4, 22 distinct values
   in body 5, 214 distinct in body 12. Body-12 attr-values cluster at
   `±1280, ±1792, 1793, ±1281, ±1025` - look like packed (high-byte,
   low-byte) tags rather than indices.
