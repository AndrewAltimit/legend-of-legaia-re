# Slot-4 records (record semantic open; consumer pinned)

> **Status: container confirmed; consumer pinned to two SCUS reader
> clusters; per-record semantic still open.** The container layout
> below is byte-verified against live RAM, and a transition-time Read-
> breakpoint capture (kingdom-bundle scene-load, not dev-menu top-view)
> surfaces explicit reader sites — see
> [Consumer call sites](#consumer-call-sites). The historical "world-
> map wireframe / coastline" reading was falsified; the data is
> heterogeneous and the working interpretation is **a runtime library
> of small object-local 3D meshes** the world-map renderer transforms
> via the GTE and emits as GP0 primitive packets into the active scene
> primitive pool.
>
> The bulk continent terrain itself - the ~4300 POLY_FT4 prims that
> tile the kingdom continent in the dev-menu top-view - is *not*
> sourced from slot 4. It comes from the same kingdom slot-1 TMD pack
> the landmarks draw from, routed through `FUN_80043390`'s
> overlay-mode dispatch table at `0x801F8968` whose eight high-mode
> renderers replace the SCUS variants when the world-map overlay is
> paged in. See
> [`subsystems/world-map.md`](../subsystems/world-map.md#bulk-continent-terrain-emit-mechanism-pinned).

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

## Consumer call sites

Two distinct SCUS-resident reader functions consume slot 4. Both are
**byte-identical across all three kingdoms** — same PC ranges, same
caller RAs — proving the consumer is generic SCUS code, not per-
kingdom overlay code.

| Reader | PC range | RA | What it reads |
|---|---|---|---|
| **Cluster A — TMD-style primitive renderer (`FUN_80043390` + handlers)** | dispatcher entry `0x80043390`; per-kind handler bodies at `0x80043658..0x80045988` | `0x8001B47C` (inside `FUN_8001ada4`), `0x801F78D4` (world-map overlay) — both present in every kingdom; Drake additionally captured `0x8001BC8C` | the outer count, body word offsets, and per-body record bytes — see [Cluster A internals](#cluster-a-internals) below |
| **Cluster B — secondary mid-body reader** | `0x80059DE4` | `0x80059C00` (SCUS) — identical across all three kingdoms | body 4 records start, body 4 mid (+0x800), body 9 region, body 12 later (+0x2800) |

Cluster A's code window contains GTE opcodes (`4A280030` = MVMVA,
`4B400006` = NCLIP, `4812C000` = SWC2/load) interleaved with `LW` reads
of slot-4 record fields - this is the GTE-driven 3D primitive emitter
that consumes slot-4 records and writes GP0 packets. The post-load RAM
window for `map01` is a 75 KB **GP0-primitive pool** (records at 0x20-
byte stride with command bytes like 0x7d / 0x7f for textured triangles),
confirming that the slot-4 records are consumed to produce GPU
primitives in that pool. The pool base is `_DAT_8007B8D0 - 0x12800`
while the overlay is paged in.

### Cluster A internals

`FUN_80043390` (712 bytes / 178 instructions; see
`ghidra/scripts/funcs/80043390.txt`) is the slot-4-consuming primitive
renderer. It takes three arguments:

```c
void FUN_80043390(struct *display_state, u32 cmd_flags, u32 fade_flags);
//   display_state[0]   -> vertex pool base  (a2 in handlers = param_3)
//   display_state[3]   -> non-zero gates the color/light-modulation path
//   display_state[4]   -> command-stream pointer (where slot-4 records feed in)
```

The function reads one command word from the stream
(`*display_state[4]`), extracts a 15-bit `kind` from bits 17-31 and a
16-bit `count` from bits 0-15, optionally re-arms the GTE colour
registers, and **tail-calls a per-kind handler** through a jump table.
Each handler consumes its own command's primitive batch (count items of
a kind-specific stride), emits GP0 packets into the active primitive
pool at `_DAT_8007BB04`, then **chain-calls the next kind handler at
the same dispatch point** — the renderer is a TMD-style display-list
walker, not a fixed-size record loop.

#### Jump tables

Two parallel handler tables drive the dispatch:

| Table | Address | When used |
|---|---|---|
| SCUS handlers | `0x8007657C` | always — the default world-map / overlay-resident render |
| Overlay handlers | `0x801F8968` | when `_DAT_1F800394 & 1` is set — the alternate route for the bulk-terrain pipeline (see `world-map.md`) |

Within the SCUS table the dispatcher adds a **bank offset** to the
`kind*4` index based on the caller's `cmd_flags` argument:

| `cmd_flags` mask | Bank offset | Effect |
|---|---:|---|
| (none of the below) | `0x00` | bank 0 — `kind ∈ [8..11]` shared with all banks; `kind ∈ [12..19]` use the small `0x80043658..0x80043F10` handler set |
| `0x04000000` set | `0x50` | bank 1 — same `kind 8..11`; `kind 12..19` swap to the `0x800448B0..0x80045584` set |
| `0x20000000` set | `0xA0` | bank 2 — `kind 12..17` same as bank 1; `kind 18` / `19` swap to `0x800457C4` / `0x80045988` |

The three banks correspond to three rendering modes the world-map
overlay drives via different `cmd_flags` arguments; the kind-handler
identity is bank-dependent only for kinds 12-19. `kind ∈ [0..7]` and
`kind ≥ 20` are NULL slots — encountering them ends the primitive
stream.

#### Per-kind primitive types

Every handler has the same shape: read N command-stream words, transform
3-or-4 vertices through the GTE, write an `M`-byte GP0 packet at the
primitive-pool pointer (`_DAT_8007BB04`-shaped global, advanced by `M`
each emit). The strides give away the PSX primitive type:

| Kind | Bank 0 entry | Banks 1,2 entry | cmd stride | GP0 stride | Likely primitive |
|---:|---|---|---:|---:|---|
| 8 | `0x8004409c` (shared) | (shared) | 0x14 (20B) | 0x20 (32B) | `POLY_G4` (gouraud quad) |
| 9 | `0x8004423c` (shared) | (shared) | 0x18 (24B) | 0x28 (40B) | `POLY_GT4` (gouraud-textured quad) |
| 10 | `0x80044434` (shared) | (shared) | 0x18 (24B) | 0x28 (40B) | `POLY_GT4` variant |
| 11 | `0x800445b0` (shared) | (shared) | 0x1c (28B) | 0x34 (52B) | extended quad (extra per-vert data) |
| 12 | `0x80043658` | `0x800448b0` | 0x0c (12B) | 0x14 (20B) | `POLY_F3` (flat triangle) |
| 13 | `0x80043768` | `0x80044a3c` | 0x0c (12B) | 0x18 (24B) | `POLY_G3` / `POLY_FT3` (gouraud or textured tri) |
| 14 | `0x80043b58` | `0x80044fdc` | 0x14 (20B) | 0x1c (28B) | `POLY_FT3` (flat textured triangle) |
| 15 | `0x80043c6c` | `0x80045194` | 0x18 (24B) | 0x24 (36B) | `POLY_GT3` (gouraud-textured triangle) |
| 16 | `0x800438b8` | `0x80044c14` | 0x14 (20B) | 0x20 (32B) | `POLY_G4` |
| 17 | `0x800439e4` | `0x80044dc8` | 0x18 (24B) | 0x28 (40B) | `POLY_GT4` |
| 18 | `0x80043dd4` | `0x800453bc` (b1) / `0x800457c4` (b2) | 0x1c (28B) | 0x28 (40B) (b1) / 0x20 (b2) | `POLY_GT4` extended (per-vertex tag word) |
| 19 | `0x80043f10` | `0x80045584` (b1) / `0x80045988` (b2) | 0x24 (36B) | 0x34 (52B) (b1) / 0x28 (b2) | `POLY_GT4` extended-plus (sub-poly) |

Decomp dumps for each handler live at
`ghidra/scripts/funcs/slot4_<kind>_<bank>_<addr>.txt`; the SCUS table is
at `ghidra/scripts/funcs/slot4_handler_table_scus_0x8007657C.txt`.
Each handler decodes the per-command words as two packed vertex indices
per `u32` (low-16 `& 0x7FF8`, high-16 also `& 0x7FF8` — a `>>3` divisor
plus 8-byte vertex stride from `param_3` = the vertex pool base).

#### Mapping captured LW PCs to kinds

Each of the eight cluster-A LW PCs captured by `autorun_slot4_consumer_pcs.lua`
falls inside one of the kind handlers above:

| LW PC | Handler | Kind | Bank |
|---|---|---:|---|
| `0x80044B00` | `0x80044A3C..0x80044C13` | 13 | banks 1,2 |
| `0x80044C70` | `0x80044C14..0x80044DC7` | 16 | banks 1,2 |
| `0x80044E08` | `0x80044DC8..0x80044FDB` | 17 | banks 1,2 |
| `0x80045418` | `0x80045194..0x800453BB` | 15 | banks 1,2 |
| `0x800455E4` | `0x800453BC..0x80045583` | 18 | bank 1 only |
| `0x800455E8` | `0x800453BC..0x80045583` | 18 | bank 1 only |
| `0x8004561C` | `0x800453BC..0x80045583` | 18 | bank 1 only |
| `0x80045658` | `0x800453BC..0x80045583` | 18 | bank 1 only |

The Drake-tuned probe's per-PC labels (`A_lw_count_word`,
`A_lw_body0_offset`, etc.) describe **what RAM region the LW happened
to touch** at the moment of the first hit, not the role of the field
in the underlying handler. Those reads are the handler's normal
load-vertex-from-pool operation; the pool just happened to be backed by
slot-4 record bytes during the kingdom-bundle scene-load transition.

### Cross-kingdom hit-count comparison

Exec-breakpoint hit counts at the eight cluster-A LW PCs + the
cluster-B LW PC during a single warp-tile transition, per-probe cap at
200 (so cluster A maxes at 8 × 200 = 1600, cluster B at 200):

| Kingdom | sstate | Cluster A | Cluster B | Cluster A RAs observed |
|---|---|---:|---:|---|
| Drake | sstate1 (already on map01, held UP) | 1400 (capped on 7 of 8 PCs) | 178 | 0x8001B47C, 0x8001BC8C, 0x801F78D4 |
| Sebucus | sstate4 (town → map02, held DOWN) | 1400 (capped on 7 of 8 PCs) | 67 | 0x8001B47C, 0x801F78D4 |
| Karisto | sstate5 (town → map03, held DOWN) | 1196 | 115 | 0x8001B47C, 0x801F78D4 |

Karisto's lower cluster-A total tracks its smaller slot 4 (24444 bytes
/ 16 bodies vs Drake's 32304 / 15 bodies and Sebucus's 26964 / 16) — a
hint that hit-count scales with record-count once per-record-kind
semantics are pinned. Cluster B's variance across kingdoms is similar:
it walks a subset of bodies, and per-kingdom body inventory differs.

#### Per-kind delta

With the cluster-A LW PCs mapped to specific kind handlers (see
[Cluster A internals](#cluster-a-internals) above), the per-PC × per-
kingdom hit counts surface a clean signal. Karisto was re-captured
with `LEGAIA_PC_CAP=5000` to lift the 200-cap and read true per-kind
totals across the same warp-tile transition; Drake / Sebucus are still
capped at 200 (interpreter-mode capture runs ~60 min uncapped, deferred):

| Kind handler | Primitive (likely) | Drake hits | Sebucus hits | Karisto hits (uncapped) |
|---:|---|---:|---:|---:|
| 13 banks 1,2 (`0x80044A3C`, LW `0x80044B00`) | `POLY_G3`/`POLY_FT3` triangle | 200 (cap) | 200 (cap) | **49** |
| 17 banks 1,2 (`0x80044DC8`, LW `0x80044E08`) | `POLY_GT4` textured quad | 200 (cap) | 200 (cap) | **147** |
| 18 bank 1 (`0x800453BC`, 4 LW PCs `0x800455E4..0x80045658`) | `POLY_GT4` extended quad | 200 (cap, ×4) | 200 (cap, ×4) | **1820** (×4) |
| 16 banks 1,2 (`0x80044C14`, LW `0x80044C70`) | `POLY_G4` quad | 200 (cap) | 200 (cap) | **2058** |
| 15 banks 1,2 (`0x80045194`, LW `0x80045418`) | `POLY_GT3` textured triangle | 200 (cap) | 200 (cap) | **2059** |

Karisto's per-kind profile shows the **POLY_F3-textured + gouraud
quad mix dominates**: kinds 15 / 16 / 18 each draw 1820-2059 batches
per warp-tile transition window, while kinds 13 (textured triangle)
and 17 (gouraud-textured quad) draw only **49 / 147** respectively. The
ratio kind-15 : kind-13 ≈ 42:1 inside Karisto identifies kind 13 as a
**rare-use** primitive in the slot-4 vocabulary, and kind 15 / 16 / 18
as the workhorses. Re-running the high-cap probe against Drake +
Sebucus (defer to a faster CPU mode or short-window capture) would
let us complete the cross-kingdom delta.

### Reproducing the capture

The original
[`autorun_slot4_readers.lua`](../../scripts/pcsx-redux/autorun_slot4_readers.lua)
probe is **Drake-tuned** — its record-region offsets are Drake's
15-body layout, and they don't reliably land on records in Sebucus
(16 bodies) or Karisto (16 bodies, smaller total). The Drake-specific
form:

```bash
LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate1 \
LEGAIA_HOLD_BUTTON=4 LEGAIA_HOLD=60 \
LEGAIA_S4_DETAIL=1 LEGAIA_S4_QUIT_AFTER_ALL=1 \
LEGAIA_FRAMES=900 \
LEGAIA_OUT=/tmp/slot4_drake.csv \
LEGAIA_LUA=scripts/pcsx-redux/autorun_slot4_readers.lua \
    bash scripts/pcsx-redux/run_world_map_probe.sh
```

`BTN.UP = 4` / `BTN.DOWN = 6` per
[`probe.lua`](../../scripts/pcsx-redux/lib/probe.lua) drives the held-
direction input that triggers the warp transition from inside the
probe. For cross-kingdom verification, use
[`autorun_slot4_consumer_pcs.lua`](../../scripts/pcsx-redux/autorun_slot4_consumer_pcs.lua)
instead — it arms Exec breakpoints at the eight identified cluster-A
LW PCs + the cluster-B LW PC, so the probe is kingdom-agnostic:

```bash
LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate4 \
LEGAIA_HOLD_BUTTON=6 LEGAIA_HOLD=60 \
LEGAIA_FRAMES=1800 \
LEGAIA_OUT=/tmp/slot4_pcs_sebucus.csv \
LEGAIA_LUA=scripts/pcsx-redux/autorun_slot4_consumer_pcs.lua \
    bash scripts/pcsx-redux/run_world_map_probe.sh
```

Each CSV row records `probe_idx, cluster, pc, name, ra, a0..a3, s8`
at the moment the Exec breakpoint fires — enough to cross-reference
caller RA + register state per hit when comparing kingdoms. A
`.detail.txt` sidecar carries the first-hit call-context for each PC
(32 GPRs, 16-word code window around PC, 32-word stack window at sp).

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

**Consumer pinned to two SCUS-resident reader functions, byte-
identical across all three kingdoms; per-record semantic still open.**
The reader accesses the buffer via a runtime pointer, not a static
`LUI+ADDIU` reference - which is why the static sweep returned empty.
Dynamic memory-watchpoint capture against the **world-map dev-menu
top-view** (steady-state, sstate2) registered **zero reads** during
300 vsyncs, but Exec-breakpoint capture at the identified reader PCs
during the **kingdom-bundle scene-load transition** (sstate1: warp
into Drake region, sstate4: town → Sebucus map02, sstate5: town →
Karisto map03) hits all three kingdoms with the same PCs and the
same caller RAs - see [consumer call sites](#consumer-call-sites)
above. Slot 4 is *not* re-read every frame; it's walked during the
kingdom-entry transition, transformed via the GTE, and emitted as GP0
primitive packets into the scene's primitive pool. The dev-menu top-
view sees the GP0 packets, never re-reading slot 4 directly.

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

1. **Verify slot 4 IS the cluster-A command stream.** The Read-bp
   capture (`autorun_slot4_readers.lua`, Drake sstate1 + held UP)
   proves cluster A reads slot-4 RAM during the warp-tile transition.
   The Exec-bp captures show the same PCs fire on Drake/Sebucus/Karisto
   warps. But the register snapshots in `.detail.txt` show the cluster-A
   handler's `a1` (command stream) and `a2` (vertex pool) pointing at
   `0x801BA8E4` / `0x801BA7F8` — **not the documented slot-4 RAM range
   (`0x8011A624..0x80122454` for Drake)**. Possibilities:
   (a) slot 4 is **copied/transcoded** into the working buffer at
       `0x801BA000` before cluster A walks it (one-shot during scene
       load);
   (b) cluster A walks slot 4 once at scene-load time and then walks
       the resulting GP0 packet pool many times per-frame thereafter
       — the Exec-bp captures sample the per-frame work, not the
       one-shot scene-load reads.
   A finer probe that arms Read bps on `0x801BA000`-region AND Exec bps
   at the cluster-A PCs, run only during the warp transition window,
   would disambiguate.
2. **Per-record-kind semantic.** Cluster A renders kinds 8-19 as a
   TMD-style primitive list (see [Cluster A internals](#cluster-a-internals));
   the question is which fields of the slot-4 body header / body-record
   payload select / parametrise those primitives. Likely path: dump
   `FUN_8001ada4` (the cluster-A caller at PC `0x8001B474`) to see how
   it builds the `display_state` struct passed to `FUN_80043390` — that
   struct's vertex-pool, command-stream, and flag-word source addresses
   reveal whether slot-4 body bytes are the command stream, the vertex
   pool, both, or input to a copy that produces both.
3. **`kind` / `flag_a` semantic.** `kind = 4` always has `flag_a = 1`
   in every kingdom; the reverse doesn't hold (Karisto body 10 is
   `kind = 2, flag_a = 1`). With the cluster-A primitive vocabulary
   pinned, the body header's `kind` field is plausibly a **draw mode**
   selector that picks one of the three SCUS handler banks (bank 0 /
   bank 1 / bank 2) via the `cmd_flags` argument FUN_80043390 receives
   — i.e., body `kind` → caller's `cmd_flags` → dispatcher's bank
   offset. Body `kind` is not the same as primitive `kind`.
4. **Per-record 4th `i16` (`attr`).** 0 for body 4, 22 distinct values
   in body 5, 214 distinct in body 12. Body-12 attr-values cluster at
   `±1280, ±1792, 1793, ±1281, ±1025` - look like packed (high-byte,
   low-byte) tags rather than indices. With the GTE pipeline pinned,
   the `attr` field likely feeds either the GP0 packet header (color /
   blend / texture-page) or a per-record draw-flag mask.
5. **Re-run probe with higher cap.** Bump `MAX_HITS_PER_PROBE` in
   `autorun_slot4_consumer_pcs.lua` from 200 to ~5000 and re-run for
   all three kingdoms; the resulting per-PC × per-kingdom totals fill
   in the kind-12/14/15/16/18/19 hit counts where every current
   capture is capped, and complete the per-kind delta table above.
