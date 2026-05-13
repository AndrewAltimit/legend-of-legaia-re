# World-map overlay outlines

The `slot 4` payload of each world-map (kingdom) bundle holds the
wireframe/outline data the dev-menu top-view renderer draws over the
world map. Three carriers, one per kingdom:

| Bundle | PROT index | CDNAME label |
|---|---|---|
| Drake | 0085 | `map01` |
| Sebucus | 0244 | `map02` |
| Karisto | 0391 | `map03` |

The 7-asset bundle these live in is the [`scene_asset_table`](scene-bundles.md#scene_asset_table---canonical-7-asset-bundle)
shape (`(1, 2, 3, 4, 5, 6, 7)` type sequence). Slot 4's type byte is
`0x05` per [asset-type](asset-type.md) - the standard table calls this
"MOVE", but the kingdom-bundle consumer interprets the bytes as
world-map outline data, **not** as a move table. (Similar mismatch to
the `vab_01` / `move_program_no` CDNAME labels - the type byte is
just a routing tag, and the kingdom path uses it differently.)

## Layout

### Outer pack

```text
+0x00   u32  count                ; number of sub-bodies
+0x04   u32  byte_offsets[count]  ; absolute byte offset into the
                                  ; decoded payload (NOT word offsets,
                                  ; unlike the slot-1 TMD pack)
+offset bodies[count]             ; contiguous sub-bodies
```

Drake decodes to 32304 bytes with `count = 15`. First entry is
`0x40 = 4 + 4*15` (right after the header). Each subsequent entry is
strictly greater; the last body extends to end-of-decoded-payload.

### Sub-body header (8 bytes)

```text
+0x00   u8   count_a              ; records per group
+0x01   u8   flag_a               ; usually 0; 1 in Drake body 13
+0x02   u8   count_b              ; number of groups
+0x03   u8   flag_b               ; usually 0
+0x04   u16  marker               ; constant 0x080C across all bodies
+0x06   u16  kind                 ; 1, 2, or 4 (semantic ambiguous)
```

### Body payload

```text
+0x08   record[count_a * count_b] ; each record is 8 bytes
                                  ; ( i16 x, i16 y, i16 z, i16 attr )
+...    trailer (8 bytes)         ; always 8 zero bytes in Drake
```

Total body size is always `8 + count_a * count_b * 8 + 8`. The math
fits all 15 of Drake's bodies exactly.

### Per-kingdom body inventory

Drake = 15 bodies; Sebucus = 16; Karisto = 16. The `flag_a` byte is
`0` for all the `kind = 1` and most `kind = 2` bodies; it flips to `1`
for the `kind = 4` boundary-frame bodies in every kingdom and also for
one anomalous `kind = 2` body (Karisto body 10, `count_a=10` /
`count_b=5`), so the simple "`flag_a` = is-boundary" rule doesn't hold
universally. Whether it selects a draw mode or modifies the count
interpretation is still open.

#### Drake (`map01`, PROT 0085)

| Body | count_a | count_b | kind | flag_a | records | Notes |
|---|---|---|---|---|---|---|
| 0 | 10 | 20 | 1 | 0 | 200 | Inner contour, ~200 vertices |
| 1 | 10 | 20 | 1 | 0 | 200 | Sister contour to body 0 |
| 2 | 10 | 30 | 1 | 0 | 300 | Inner contour, ~300 vertices |
| 3 | 2 | 30 | 2 | 0 | 60 | Polyline, X stepping with `(0, 0, 0, 0)` pad records between |
| 4 | 2 | 20 | 2 | 0 | 40 | Polyline (20 vertices stepping in X by 256 units, fixed Y/Z) |
| 5 | 10 | 30 | 2 | 0 | 300 | Inner contour (25 unique of 30 groups) |
| 6 | 10 | 26 | 2 | 0 | 260 | Inner contour |
| 7 | 10 | 30 | 2 | 0 | 300 | Inner contour (25 unique of 30 groups) |
| 8 | 10 | 3 | 2 | 0 | 30 | 3 IDENTICAL groups of 10 records (reserved/padding) |
| 9 | 12 | 30 | 2 | 0 | 360 | Mid-density feature |
| 10 | 12 | 30 | 2 | 0 | 360 | Mid-density feature |
| 11 | 12 | 10 | 2 | 0 | 120 | Small feature |
| 12 | 10 | 120 | 2 | 0 | 1200 | **Continent coastline** - 120 segments × 10 sub-points |
| 13 | 14 | 15 | 4 | 1 | 210 | **World-map boundary frame** at ±32K (perimeter only) |
| 14 | 2 | 30 | 2 | 0 | 60 | Polyline |

#### Sebucus (`map02`, PROT 0244)

| Body | count_a | count_b | kind | flag_a | records | Notes |
|---|---|---|---|---|---|---|
| 0 | 10 | 20 | 1 | 0 | 200 | Inner contour (matches Drake body 0) |
| 1 | 10 | 20 | 1 | 0 | 200 | Inner contour (matches Drake body 1) |
| 2 | 10 | 30 | 1 | 0 | 300 | Inner contour (matches Drake body 2) |
| 3 | 2 | 30 | 2 | 0 | 60 | Polyline (matches Drake body 3) |
| 4 | 10 | 30 | 2 | 0 | 300 | Inner contour |
| 5 | 10 | 26 | 2 | 0 | 260 | Inner contour |
| 6 | 10 | 30 | 2 | 0 | 300 | Inner contour |
| 7 | 10 | 3 | 2 | 0 | 30 | 3 IDENTICAL groups (reserved/padding) |
| 8 | 11 | 30 | 4 | 1 | 330 | Boundary-style frame |
| 9 | 11 | 15 | 4 | 1 | 165 | Boundary-style frame |
| 10 | 1 | 30 | 4 | 1 | 30 | Single-strand boundary marker |
| 11 | 1 | 15 | 4 | 1 | 15 | Single-strand boundary marker |
| 12 | 12 | 30 | 2 | 0 | 360 | Mid-density feature |
| 13 | 12 | 30 | 2 | 0 | 360 | Mid-density feature |
| 14 | 12 | 10 | 2 | 0 | 120 | Small feature |
| 15 | 10 | 30 | 2 | 0 | 300 | Inner contour |

#### Karisto (`map03`, PROT 0391)

| Body | count_a | count_b | kind | flag_a | records | Notes |
|---|---|---|---|---|---|---|
| 0 | 10 | 20 | 1 | 0 | 200 | Inner contour |
| 1 | 10 | 20 | 1 | 0 | 200 | Inner contour |
| 2 | 10 | 30 | 1 | 0 | 300 | Inner contour |
| 3 | 1 | 15 | 2 | 0 | 15 | Single-strand contour |
| 4 | 14 | 15 | 4 | 1 | 210 | Boundary frame (mirrors Drake body 13) |
| 5 | 14 | 15 | 4 | 1 | 210 | Boundary frame |
| 6 | 11 | 30 | 4 | 1 | 330 | Boundary-style |
| 7 | 11 | 15 | 4 | 1 | 165 | Boundary-style |
| 8 | 1 | 15 | 4 | 1 | 15 | Single-strand boundary marker |
| 9 | 1 | 30 | 4 | 1 | 30 | Single-strand boundary marker |
| 10 | 10 | 5 | 2 | 1 | 50 | Small feature |
| 11 | 10 | 15 | 4 | 1 | 150 | Boundary-style |
| 12 | 12 | 30 | 2 | 0 | 360 | Mid-density feature |
| 13 | 12 | 30 | 2 | 0 | 360 | Mid-density feature |
| 14 | 12 | 10 | 2 | 0 | 120 | Small feature |
| 15 | 10 | 30 | 2 | 0 | 300 | Inner contour |

Across all three kingdoms, the leading three bodies (`kind=1`,
`count_a=10`, `count_b=20/20/30`) are byte-identical templates -
they're the same generic inner-contour shape installed at the front
of each slot. The kingdom-specific data lives in the trailing bodies.

## RAM layout

Slot 4 is loaded **verbatim into RAM** with zero per-byte diffs vs
disc. Drake's load address is `0x8011A664`; bodies sit contiguously
through `0x80122454` (32240 bytes total - the 64-byte header is
the only RAM/disc-offset slip). No runtime fixup is applied.

Confirmed by `scripts/pcsx-redux/verify_slot4_in_ram.py` against a
real PCSX-Redux save state.

## Why it isn't the bulk continent terrain

The top-down rendering of all bodies' points (see
`scripts/slot4_topdown_png.py`) reveals body 12 traces a continent
coastline and body 13 traces the world boundary - both are 1D
contours, not 2D terrain meshes. Total ~4000 vertices across all 15
bodies is far below the ~17000+ vertex positions the textured
continent in the GPU prim pool would need (Drake's pool has 4994
prims, mostly POLY_FT4 quads with 4 verts each).

The bulk continent geometry source is therefore separate. The most
likely candidate is a procedural emitter, sibling of the horizon
emitter [`FUN_801D7EA0`](../subsystems/world-map.md#fun_801d7ea0---world-map-poly_ft4-batch-emitter)
reachable from the per-frame world-map tick `FUN_80016444`. Pinning
that is still open.

## Consumer (presumed)

Slot 4 is loaded by the standard kingdom-bundle loader. The reader
that interprets these bytes is presumably the world-map controller
`FUN_801E76D4` (top-view branch) and/or the developer menu renderer
`FUN_801EAD98`, both documented under [world map subsystem](../subsystems/world-map.md).
The exact reading call has not been pinned down in Ghidra; the
overlap with PSX SVECTOR shape (3 int16 + pad) and the 0x080C marker
suggest the data feeds a small VRAM/GPU-prim-pool generator rather
than the standard TMD renderer (which would have used the type
`0x02` TMD-pack slot 1 instead).

## Tooling

| Tool | Role |
|---|---|
| `cargo run -p legaia-asset --bin asset -- slot4-png --input <PROT>.BIN --out <png>` | Engine-side standalone PNG renderer. `--style row\|col\|pairs\|grid\|points` toggles between row-major polylines, column-major (record-slot-as-strand) polylines, pair-wise edges (every two records = one segment), heightfield-grid quad-mesh wireframe, and a topology-free point cloud. `--only-body N` / `--frame-body N` isolate a single body. `--from-raw <ram-dump>.bin` renders a previously-dumped slot-4 payload (e.g. from PCSX-Redux live RAM). |
| `cargo run -p legaia-asset --bin asset -- kingdom-slot <PROT>.BIN --slot 4 --wireframe-obj <out>.obj` | Per-body inventory dump + Wavefront-line OBJ export. Available for slots 0..6. |
| `legaia_asset::world_map_overlay::parse` + `top_down_lines` / `record_points` | Rust API consumed by the [world overview web viewer](../../site/world-overview.html) (`LegaiaViewer::slot4_wireframe_lines`). |
| `scripts/pcsx-redux/run_dump_slot4.sh` + `autorun_dump_slot4.lua` | PCSX-Redux closed-loop autorun: loads a save state, waits for the kingdom to settle, dumps the live slot-4 RAM region (32304 / 26964 / 24444 bytes) to `slot4_ram_<kingdom>.bin`, quits. Same pattern as the existing world-map probes. |
| `scripts/pcsx-redux/diff_slot4_ram_vs_disc.py` | Byte-compare the RAM dump against the disc-decoded payload (per-body diff counts + first 32 offsets). Confirms whether disc bytes hit RAM verbatim. |
| `scripts/decode_slot4_subbodies.py` | Per-body hex dump + header parse + grid-hypothesis analysis; OBJ export per body. |
| `scripts/slot4_to_obj.py` | Combined OBJ writer (polys / lines / points modes). |
| `scripts/slot4_topdown_png.py` | Top-down PGM/PNG renderer of the point cloud (X-Z plane). |
| `scripts/pcsx-redux/verify_slot4_in_ram.py` | Byte-for-byte cross-check of disc-decoded bodies against PCSX-Redux save-state RAM (legacy; superseded by `run_dump_slot4.sh` + `diff_slot4_ram_vs_disc.py`). |

### Validation flow

To confirm whether the disc bytes we render are what the live runtime
actually consumes:

```bash
# 1. dump slot 4 from a top-view save state
LEGAIA_SSTATE=~/Tools/pcsx-redux/SCUS94254.sstate2 \
LEGAIA_KINGDOM=drake \
scripts/pcsx-redux/run_dump_slot4.sh

# 2. byte-compare against the disc-decoded payload
python3 scripts/pcsx-redux/diff_slot4_ram_vs_disc.py \
    slot4_ram_drake.bin --bundle map01

# 3. visualize the RAM dump and the disc dump side-by-side
./target/release/asset slot4-png --from-raw slot4_ram_drake.bin \
    --style points --out /tmp/ram.png
./target/release/asset slot4-png --input extracted/PROT/0085_map01.BIN \
    --style points --out /tmp/disc.png
```

## Topology hypothesis

Drake body 12 (`count_a=10`, `count_b=120`, `kind=2`) is the largest
contour body in any kingdom and was used to probe the per-group record
layout. Walking group 0 reveals records pair up at fixed X-bands:

```text
r0  (-22016, -1280)   r1  (-14848,     0)   <- band 0, low/high X
r2  (-18965,  -512)   r3  (-14872, -1024)   <- band 1
r4  (-19179,   256)   r5  (-15080,     0)   <- band 2
r6  (-10759,  -512)   r7   (-5898,  -512)   <- band 3
r8  (-11001,   512)   r9   (-6134,   256)   <- band 4
```

Group 0 and group 1 are byte-identical except for r8/r9's Z values -
consecutive groups are differential updates to a shared topology. The
records are therefore a `count_a x count_b` grid of vertices forming a
heightfield strip, **not** a chained polyline. Three rendering
candidates are wired into the CLI:

- `--style pairs` - each group emits `count_a / 2` independent edge
  pairs (`(r0,r1), (r2,r3), ...`); 120 z-stride groups × 5 edges per
  group = 600 line segments;
- `--style grid` - emits both row edges (`(k, g) -> (k + 1, g)`) and
  column edges (`(k, g) -> (k, g + 1)`); shows the heightfield as a
  quad-mesh wireframe;
- `--style points` - topology-free; matches the visible structure most
  closely and is the recommended raw-validation lens.

None of `row`, `col`, `pairs`, or `grid` cleanly reproduce the dev-menu
top-view's continent silhouette by itself, so the runtime consumer
likely encodes more than one draw mode (probably keyed off `kind` /
`flag_a`).

## Open questions

1. **`kind = 1, 2, 4` semantic.** Not yet tied to a draw-mode or
   sub-format. `kind = 4` correlates strongly with `flag_a = 1`
   across all three kingdoms (every `kind = 4` body has `flag_a = 1`)
   and those bodies plot as world-boundary frames at ±32K /
   large-perimeter rings - but the reverse doesn't hold (Karisto body
   10 is `kind = 2` with `flag_a = 1`), so `(kind, flag_a)` together
   select more than a simple "border-strand" toggle.
2. **Per-record 4th `int16` column (`attr`).** Always 0 for body 4,
   has 22 distinct values across 300 records in body 5, 214 distinct
   values in body 12. Body 12 attr-values cluster at `±1280, ±1792,
   1793, ±1281, ±1025` - which look like packed `(high_byte = facing,
   low_byte = sub-id)` tags rather than indices. Depends on the
   consumer.
3. **Per-body kind→draw routine mapping.** Needs Ghidra capture of the
   consumer (likely in the `world_map_top` overlay).
