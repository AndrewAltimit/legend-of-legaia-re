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
[`scene_asset_table`](scene-bundles.md#scene_asset_table---count-prefixed-asset-bundle)
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
`kind*4` index based on the caller's `cmd_flags` (`param_2`) and
`fade_flags` (`param_3`) arguments. The selection is the literal
disassembly from
[`ghidra/scripts/funcs/80043390.txt`](../../ghidra/scripts/funcs/80043390.txt)
(lines 230-244):

```c
_DAT_1f800028 = 0;
if (fade_flags != 0) {
    _DAT_1f800028 = 0x50;
    if ((cmd_flags & 0x04000000) != 0) _DAT_1f800028 = 0xA0;
    if ((cmd_flags & 0x20000000) != 0) _DAT_1f800028 = 0xF0;
}
```

So there are **four banks**, not three; the two `if`s are sequential
(not else-if), so the `0x20000000` branch wins when both flags are
set. And bank 0 / bank 1 are gated by `fade_flags`, not by
`cmd_flags` bits.

| `fade_flags` | `cmd_flags` bits | Bank offset | Effect |
|---|---|---:|---|
| `== 0` | (ignored) | `0x00` | bank 0 — `kind ∈ [12..19]` use the small `0x80043658..0x80043F10` handler set |
| `!= 0` | neither `0x04000000` nor `0x20000000` | `0x50` | bank 1 — `kind 12..19` swap to the `0x800448B0..0x80045584` set |
| `!= 0` | `0x04000000` set, `0x20000000` clear | `0xA0` | bank 2 — `kind 12..17` same as bank 1; `kind 18` / `19` swap to `0x800457C4` / `0x80045988` |
| `!= 0` | `0x20000000` set | `0xF0` | bank 3 — likely dev / debug mode; never observed in retail world-map render |

`kind ∈ [0..7]` and `kind ≥ 20` are NULL slots in every bank —
encountering them ends the primitive stream. `kind ∈ [8..11]` is
shared across all banks; only `kind ∈ [12..19]` swaps handler per bank.

#### Banks exercised in retail world-map play

Empirically (Drake post-warp settled, 19,935 dispatcher-entry
hits captured via
[`autorun_slot4_dispatcher_args.lua`](../../scripts/pcsx-redux/autorun_slot4_dispatcher_args.lua)):

| Bank | Drake hits | % | cmd_flags values seen |
|---|---:|---:|---|
| `0x00` (no fade) | 15,257 | 77% | `0x00D0D0D0`, `0xC9000000`, `0x00808080`, …  |
| `0x50` (fade) | 4,678 | 23% | `0x40D0D0D0`, `0x40808080`, `0x50808080`, … |
| `0xA0` | 0 | 0% | (`0x04000000` mask never set) |
| `0xF0` | 0 | 0% | (`0x20000000` mask never set) |

The high cmd_flags bits `0x04000000` and `0x20000000` are **never set**
during retail Drake world-map gameplay; banks 2 and 3 are reachable in
the dispatcher but no caller passes the flags that select them. The
sole bank distinction is `fade_flags != 0` (bank 0 ↔ bank 1).

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

Each of the eight cluster-A LW PCs captured by
[`autorun_slot4_consumer_pcs.lua`](../../scripts/pcsx-redux/autorun_slot4_consumer_pcs.lua)
falls inside one of the bank-1 kind handlers (`0x800448B0..0x80045584`).
The probe doesn't have PCs inside bank 0 (`0x80043658..0x80043F10`) or
bank 2 (`0x800457C4..0x80045988`):

| LW PC | Handler | Kind | Bank |
|---|---|---:|---|
| `0x80044B00` | `0x80044A3C..0x80044C13` | 13 | bank 1 (= bank 2 for this kind) |
| `0x80044C70` | `0x80044C14..0x80044DC7` | 16 | bank 1 (= bank 2 for this kind) |
| `0x80044E08` | `0x80044DC8..0x80044FDB` | 17 | bank 1 (= bank 2 for this kind) |
| `0x80045418` | `0x80045194..0x800453BB` | 15 | bank 1 (= bank 2 for this kind) |
| `0x800455E4` | `0x800453BC..0x80045583` | 18 | bank 1 only (bank 2 uses `0x800457C4`) |
| `0x800455E8` | `0x800453BC..0x80045583` | 18 | bank 1 only |
| `0x8004561C` | `0x800453BC..0x80045583` | 18 | bank 1 only |
| `0x80045658` | `0x800453BC..0x80045583` | 18 | bank 1 only |

The Drake-tuned probe's per-PC labels (`A_lw_count_word`,
`A_lw_body0_offset`, etc.) describe **what RAM region the LW happened
to touch** at the moment of the first hit, not the role of the field
in the underlying handler. Those reads are the handler's normal
load-vertex-from-pool operation; the pool just happened to be backed by
slot-4 record bytes during the kingdom-bundle scene-load transition.

[`autorun_slot4_dispatcher_args.lua`](../../scripts/pcsx-redux/autorun_slot4_dispatcher_args.lua)
captures the *dispatcher prologue* (`0x80043390`) directly — `a0`,
`a1` (cmd_flags), `a2` (fade_flags), and the first command word's
kind / count fields *before* the handlers clobber the registers. Use
that probe to characterise per-call dispatch behaviour; use
`autorun_slot4_consumer_pcs.lua` to count handler-level work (number
of primitives emitted per kind).

### How slot-4 bytes reach cluster A

The cluster-A input pointer originates from `DAT_8007C018` (the global
asset-pointer table — see [reference/memory-map](../reference/memory-map.md)).
Two parallel call paths funnel into the same dispatcher:

1. **Top-view dispatcher (`FUN_801F69D8`)**: reads
   `DAT_8007C018[(visible_object_kind8 + DAT_8007B6F8) * 4]` per tile
   and passes `entry + 0xC` (the TMD's group-descriptor array start)
   to `FUN_80043390`. This is the warp-into-world-map render path the
   Read-bp probe captured.
2. **Per-actor renderer (`FUN_8001ada4`, caller RA `0x8001B47C`)**:
   walks `actor+0x44 = [u32 count, u32 mesh_ptr[count]]` and passes
   each `mesh_ptr` to `FUN_80043390`. The mesh pointers came from
   `actor+0x44`, which is populated by `FUN_80021B04`/`FUN_80024D78`
   from `DAT_8007C018[actor[+0x64].i16]` — same table, different
   actor-allocator path.

Slot 4 of the kingdom bundle (`type = 0x05` = MOVE) is the largest
data slot but **is not directly consumed by cluster A**. The MOVE
buffer at the kingdom-load destination (a mid-warp Drake dump pinned
it to `0x8011A624`, but the address varies per build / save state)
gets overwritten by later
TMD-pack installs whose TMDs occupy the same physical RAM by the time
the world-map enters steady state. The Read-bp probe that captured
"slot-4 bytes being read by cluster A" was sampling that buffer
*after* the TMD overwrite — cluster A was reading TMDs at addresses
that had once held slot-4 bytes, not the slot-4 bytes themselves.

The slot-4 body header `kind ∈ {1, 2, 4}` therefore has **no link** to
the cluster-A bank selector (the previously listed hypothesis is
empirically falsified — see
[Banks exercised in retail world-map play](#banks-exercised-in-retail-world-map-play)).
Whatever slot 4 encodes, it's consumed during the warp's first asset
pass and converted into TMD blobs by an as-yet-unpinned step before
the world map runs.

See [reference/memory-map](../reference/memory-map.md#world-map-tmd-and-actor-tables)
for the `DAT_8007C018` snapshot breakdown and the kingdom-TMD prefix
counter `DAT_8007B6F8`.

### Cross-kingdom hit-count comparison

Exec-breakpoint hit counts at the eight cluster-A LW PCs + the
cluster-B LW PC during a single warp-tile transition. All three
kingdoms captured with `LEGAIA_PC_CAP=50000` over 1800 vsyncs; no PC
saturates the cap, so the per-kingdom totals are exact:

| Kingdom | Capture state | Cluster A total | Cluster B | Cluster A RAs observed |
|---|---|---:|---:|---|
| Drake | already on map01, held UP | 71,331 | 178 | 0x8001B47C, 0x8001BC8C, 0x801F78D4 |
| Sebucus | town → map02, held DOWN | 90,096 | 67 | 0x8001B47C, 0x801F78D4 |
| Karisto | town → map03, held DOWN | 13,593 | 115 | 0x8001B47C, 0x801F78D4 |

Sebucus's cluster-A total is *higher* than Drake's despite Sebucus's
slot-4 being smaller — confirming hit-count tracks scene-render
volume, not slot-4 record count. Cluster B's variance is the inverse:
Drake walks the most slot-4 bodies, then Karisto, then Sebucus. The
per-kind breakdown ([Per-kind delta](#per-kind-delta) below) makes the
per-handler differences visible.

#### Per-kind delta

With the cluster-A LW PCs mapped to specific kind handlers (see
[Cluster A internals](#cluster-a-internals) above), the per-PC × per-
kingdom hit counts surface a clean signal. All three kingdoms
captured uncapped (`LEGAIA_PC_CAP=50000` over 1800 vsyncs):

| Kind handler | Primitive (likely) | Drake hits | Sebucus hits | Karisto hits |
|---:|---|---:|---:|---:|
| 13 banks 1,2 (`0x80044A3C`, LW `0x80044B00`) | `POLY_G3`/`POLY_FT3` triangle | **9,465** | **2,040** | 49 |
| 17 banks 1,2 (`0x80044DC8`, LW `0x80044E08`) | `POLY_GT4` textured quad | **762** | **240** | 147 |
| 18 bank 1 (`0x800453BC`, 4 LW PCs `0x800455E4..0x80045658`) | `POLY_GT4` extended quad | **13,561** (×4) | **20,601** (×4) | **1,820** (×4) |
| 16 banks 1,2 (`0x80044C14`, LW `0x80044C70`) | `POLY_G4` quad | **7,688** | **878** | **2,058** |
| 15 banks 1,2 (`0x80045194`, LW `0x80045418`) | `POLY_GT3` textured triangle | **6,860** | **5,412** | **2,059** |
| cluster B (`0x80059DE4`) | mid-body reader | **178** | **67** | **115** |

Cross-kingdom picture (now with all entries uncapped):

- **Kind 13** scales sharply: Drake (9,465) ≫ Sebucus (2,040) ≫
  Karisto (49). Drake / Sebucus have continental geometry with many
  small triangle primitives; Karisto barely uses them.
- **Kind 17** scales with overall scene weight: Drake (762) >
  Sebucus (240) > Karisto (147). Ratio Drake / Karisto ≈ 5.2.
- **Kind 16** is the inverse of kind 13: Karisto-heavy (2,058) /
  Drake-heavy (7,688) but Sebucus uses it least (878). Drake's quad
  count dwarfs the others.
- **Kind 18 (extended quad)** is the absolute workhorse — Sebucus
  dispatches **20,601 instances** of it (~80% of the cluster-A
  primitive count), Drake 13,561, Karisto 1,820. This is the dominant
  per-frame primitive across every kingdom.
- **Cluster B** (the mid-body reader): Drake (178) > Karisto (115) >
  Sebucus (67) — Drake's larger slot 4 visits more of the secondary
  reader's body subset.

The captured CSVs land under
[`captures/slot4_uncapped/`](../../captures/slot4_uncapped/) (per-row
flushed; safe to inspect mid-run). The dispatcher-entry probe CSV at
[`captures/slot4_dispatcher/`](../../captures/slot4_dispatcher/) gives
the first-kind / `cmd_flags` / `fade_flags` per call — see the bank-
breakdown table above.

### Reproducing the capture

An earlier Drake-tuned `autorun_slot4_readers.lua` probe (now archived
under `archive/pcsx-redux-probes/`) walked Read breakpoints across
Drake's 15-body slot-4 layout to surface the original reader PCs. It
doesn't generalise — the offsets are Drake-specific. The
kingdom-agnostic replacement is
[`autorun_slot4_consumer_pcs.lua`](../../scripts/pcsx-redux/autorun_slot4_consumer_pcs.lua),
which arms Exec breakpoints at the cluster-A + cluster-B PCs surfaced
during that earlier capture and fires identically across all three
kingdoms:

```bash
LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/<your-sebucus-warp-save>.sstate \
LEGAIA_HOLD_BUTTON=6 LEGAIA_HOLD=60 \
LEGAIA_FRAMES=1800 \
LEGAIA_PC_CAP=50000 \
LEGAIA_OUT=captures/slot4_uncapped/sebucus.csv \
LEGAIA_LUA=scripts/pcsx-redux/autorun_slot4_consumer_pcs.lua \
    timeout --kill-after=30s 1500s bash scripts/pcsx-redux/run_probe.sh
```

Each CSV row records `probe_idx, cluster, pc, name, ra, a0..a3, s8`
at the moment the Exec breakpoint fires — enough to cross-reference
caller RA + register state per hit when comparing kingdoms. A
`.detail.txt` sidecar carries the first-hit call-context for each PC
(32 GPRs, 16-word code window around PC, 32-word stack window at sp).

`pcsx-redux` in `-interpreter -debugger` mode does not reliably
self-terminate within a tractable wall-clock window even though
`probe.lua` calls `PCSX.quit(0)` after the capture window — the
PSX vsync timer is game-time, not wall-time, and interpreter overhead
with active breakpoints stretches the 1830-vsync wall-clock by an
order of magnitude. The `timeout --kill-after=30s 900s` wrapper above
forces a clean shutdown after 15 minutes; the CSV is flushed per row,
so the partial capture remains usable even after an explicit kill.

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
top-view** in steady state registered **zero reads** during
300 vsyncs, but Exec-breakpoint capture at the identified reader PCs
during the **kingdom-bundle scene-load transition** (warp into each of
Drake / Sebucus / Karisto, from one save state per kingdom) hits all
three with the same PCs and the same caller RAs — see
[consumer call sites](#consumer-call-sites)
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
| `scripts/pcsx-redux/run_probe.sh --lua scripts/pcsx-redux/autorun_dump_slot4.lua` | PCSX-Redux closed-loop dumper: loads a save state, dumps the live slot-4 RAM region, quits. |
| `scripts/pcsx-redux/autorun_dump_full_ram.lua` | Full 2 MiB main RAM dump. Use when the load base is unknown for a new build / state - signature-scan the dump for the 64-byte outer pack prefix. |
| `scripts/pcsx-redux/diff_slot4_ram_vs_disc.py` | Byte-compare a RAM dump against the disc-decoded payload. |

The world-overview web viewer no longer exposes slot 4 - the previous
"show slot-4 wireframe" toggle was removed once the wireframe
hypothesis was falsified. Re-enable from the WASM exports
(`slot4_wireframe_lines` / `slot4_wireframe_points` /
`slot4_wireframe_bounds`) if a future RE pass identifies the correct
draw interpretation.

### Slot-4 loader (loader-hunt probe)

Running `autorun_slot4_loader_hunt.lua` (now archived under
`archive/pcsx-redux-probes/` — investigation resolved) against Drake
(held UP for 60 vsyncs into the warp) with Write bps tiled across
slot-4 RAM (`0x8011A624 + offset[0..7000]`) surfaced **the LZS
decoder** as the sole writer:

| Caller chain | PC of write | Notes |
|---|---|---|
| `FUN_8001A55C` (LZS decoder, body at `0x8001A55C..0x8001A6XX`) | `0x8001A604` (`sb v1, 0(s1)` — literal-byte write) | Dominant: 5-byte bursts at every probed offset |
| same | `0x8001A664` / `0x8001A668` / `0x8001A610` / `0x8001A5AC` | Back-reference copy / literal-run / dictionary-byte paths inside the LZS loop |

Every captured first-write shows:

```text
pc = 0x8001A604, ra = 0x8001A58C  (LZS decoder calling itself)
stack:
  +0x20  0x8001F194    <- next caller up (inside asset dispatcher region)
  +0x24  0x8001F0A0    <- asset dispatcher FUN_8001F05C-area
  +0x30  0x801E3DC0
```

The chain is the **standard asset-load path**: scene loader →
`FUN_8001F05C` (asset dispatcher) → LZS decoder → writes slot 4 at its
allocated RAM destination (`0x8011A624` for Drake). No special slot-4
transcoder; the asset is just LZS-decoded verbatim into RAM, matching
the byte-verified `disc → RAM` finding documented in
[RAM layout](#ram-layout-confirmed) above.

### Working-buffer writers (transcoder-hunt probe, 2026-05-14)

Running `autorun_slot4_transcoder_hunt.lua` (now archived under
`archive/pcsx-redux-probes/` — investigation resolved) against Drake
(held UP for 60 vsyncs into the warp transition) with Write bps tiled
across the `0x801BA000` working buffer surfaced **two distinct
writers**, not a single transcoder:

| Working-buffer offset | First-write PC | RA | Writer function | Role |
|---|---|---|---|---|
| `+0x7F8` (`0x801BA7F8`, cluster A's `vertex_base`) | `0x80028710` / `0x8002871C` (paired `sh` instructions) | `0x8001B160` | `FUN_80028158` (5580 B / 1395 instructions) | **per-frame procedural mesh builder**, called from `FUN_8001ada4` case 4 |
| `+0x8E4` (`0x801BA8E4`, cluster A's `command_stream`) | `0x800293C8` / `0x800296A0` (paired `sw` instructions) | `0x8001B160` | same `FUN_80028158` | per-frame procedural primitive-batch writer (same call) |
| `+0x6000` (`0x801C0000`, deeper region) | `0x8001A8C8` (memcpy inner loop) | `0x8001E758` | `FUN_8001E54C` (836 B), the streaming chunk processor | **scene-load chunk loader** — copies streaming-format chunks (`[type, size, data]`) to the buffer |

**`FUN_80028158`** decompiles as a switch on `(param_2 >> 3) & 0xf`
with per-case mesh layouts; it reads only the actor's `+0x9C` params
struct (offsets `+0x10..+0x22`) and writes the working buffer
directly. **No slot-4 RAM pointers appear in its arguments** — it is a
procedural mesh generator (probably waves / sky / particle-emitter
sheets), not a slot-4 transcoder.

**`FUN_8001E54C`** is the `[type, size, data]` streaming chunk
dispatcher: it switches on `*(char*)(chunk + 3)` (the chunk type byte)
and routes each chunk to one of memcpy (case 0/2), LZS decode (case
1/3), or another decoder (case 12). Its 4 captured writes at
`0x801C0000` are scene-load chunk copies that land deeper into the
buffer than cluster A's per-frame inputs at `+0x7F8` / `+0x8E4`.

**Revised model**: slot 4 is not transcoded into a single working-
buffer region. Instead:

1. At scene load, `FUN_8001E54C` (or a sibling streaming-chunk processor)
   reads the kingdom bundle's chunks and **distributes their bytes
   across multiple destinations** — actor structs, working buffer at
   different offsets, etc.
2. Some destinations are read by cluster A during the same scene-load
   pass (Drake Read-bp captures show this — slot-4 RAM is touched once
   during the warp transition).
3. Per-frame, cluster A reads the working buffer (now populated with
   scene-load data plus per-frame procedural patches from
   `FUN_80028158`).

The cross-kingdom Exec-bp captures sample **per-frame steady state**,
where cluster A reads the working buffer — NOT slot 4 directly. The
high per-frame cluster-A hit counts (~2000 in 1800 frames) are
procedural rendering volume, not slot-4 walks.

The remaining open question is whether slot 4 ends up in
`0x801BA000`-region (close to where cluster A reads per-frame, so
maybe accessed via per-actor mesh-table indirection) or in some other
region (so accessed via a different cluster-A entry path). A finer
probe that arms Read bps on slot-4 RAM during the warp transition,
plus Exec bps at `FUN_8001E54C`'s case-0 / case-1 / case-2 / case-12
arms, would pin which chunks come from slot 4 and where they land.

### Cluster-A caller (`FUN_8001ada4`)

`FUN_8001ada4` (2456 bytes / 614 instructions; see
`ghidra/scripts/funcs/8001b47c.txt`) is the per-actor renderer that
walks a linked list of actor records. For each record at
`piVar2 = head_ptr`, then chained via `piVar2 = piVar2[0]`, it:

1. Pre-transforms the actor's local origin through the GTE
   (`copFunction(2, 0x480012)`) and writes the transformed coordinates
   back into `piVar2[+0x2C..+0x34]`.
2. Switches on `piVar2[+0x56]` (a u16 actor type, values 1..6) to do
   type-specific drawing.

The cluster-A call at PC `0x8001B474` is one of those drawing paths.
The relevant disasm slice (lines 415-442 of `8001b47c.txt`):

```text
8001b40c  lw v1,0x44(s0)        ; v1 = actor+0x44 = mesh-table base
8001b414  lw v0,0x0(v1)         ; v0 = *v1 = mesh count (terminator if 0)
8001b41c  beq v0,zero,...       ; if no meshes, skip render
8001b430  addu v0,v1,s2<<2      ; v0 = mesh-table[index]
8001b438  lw s3,0x4(v0)         ; s3 = (actor+0x44 + index*4 + 4) = mesh_ptr
...
8001b46c  lw a1,0x74(s0)        ; a1 = actor+0x74 (FUN_80043390's cmd_flags arg)
8001b470  lhu a2,0x78(s0)       ; a2 = actor+0x78 (FUN_80043390's fade_flags arg)
8001b474  jal 0x80043390        ; call cluster A with s3 = mesh struct
8001b478  _move a0,s3
```

The mesh-table at `actor+0x44` is a contiguous `[u32 count, u32
mesh_ptr[count]]` array. Each `mesh_ptr` is the pointer FUN_80043390
receives as `param_1` (= the struct exposing `vertex_base` at +0,
`flag_word` at +0xC, `command_stream` at +0x10). Case 3 inside the
type switch contains the same pattern explicitly:

```c
puVar5 = (uint *)piVar2[0x11];  // = actor+0x44
if (*puVar5 != 0) {
  do {
    uVar11 = puVar5[uVar10 + 1];     // mesh_ptr
    FUN_80043390(uVar11, piVar2[0x1d] | uVar8, *(undefined2 *)(piVar2 + 0x1e));
    ...
  } while (uVar10 < *puVar5);
}
```

The Exec-bp register snapshots from `autorun_slot4_consumer_pcs.lua`
captured `a1 = 0x801BA8E4` and `a2 = 0x801BA7F8` at the cluster-A LW
PCs — both in the **`0x801BA000`-ish working buffer**, not in slot 4's
documented RAM base (`0x8011A624..0x80122454` for Drake). Combined
with the `actor+0x44 → mesh_ptr_array → mesh_struct` chain, this
**confirms the transcoder pattern**: slot 4 is read once at scene
load, decoded into TMD-style mesh structs in the working buffer at
`0x801BA000`-ish, and the actor's mesh-table is populated with
pointers to those decoded structs. Per-frame, `FUN_8001ada4` walks the
mesh-table and FUN_80043390 walks each mesh's vertex pool + command
stream — never touching slot 4 directly after the scene-load pass.

## DAT_8007C018 — global TMD pointer table (the *actual* cluster-A source)

`FUN_80043390`'s `display_state` arg points at a TMD's group-descriptor
array (offset `+0xC` into a TMD blob whose `+0x00` carries the Legaia
magic `0x80000002`). Those TMD pointers live in a global runtime table:

```
DAT_8007C018 : array of u32 TMD pointers; entry stride = 4
DAT_8007B774 : install counter (next free index)
DAT_8007BB38 : walker counter (last installed index, used by the table walker)
DAT_8007B824 : per-pack count (set by case 2 to `*pack_header[0]`)
```

The installer is **`FUN_80026B4C` @ PC `0x80026BA8`** (called per-TMD
from the asset dispatcher's case 2 TMD-pack handler):

```
80026b90  lui   v1, 0x8008
80026b94  lw    v1, -0x488c(v1)     ; v1 = *DAT_8007B774 (next free idx)
80026b98  addiu v0, v0, -0x3fe8     ; v0 = 0x8007C018
80026b9c  sll   v1, v1, 0x2
80026ba0  addu  v1, v1, v0          ; v1 = &DAT_8007C018[idx]
80026ba4  jal   FUN_800268dc        ; build per-group descriptor array at tmd+0xC
80026ba8  _sw   a0, 0x0(v1)         ; install: DAT_8007C018[idx] = tmd_ptr
                                     ;          and, via gp+0x820: DAT_8007BB38 = idx
                                     ; (gp[+0x820] aliases DAT_8007BB38 in SCUS)
```

Ghidra's static reference-database doesn't surface either store because
the `addu` between the `lui+addiu` and the `sw` defeats its constant
propagation. The materialisation scan
[`ghidra/scripts/find_addr_materializer_dat_8007c018.py`](../../ghidra/scripts/find_addr_materializer_dat_8007c018.py)
walks every `lui+addiu` pair that produces `0x8007C018` across SCUS +
every world-map overlay; that's how the installer was pinned. A scan
across `SCUS_942.54`, `overlay_world_map.bin`, `overlay_world_map_top.bin`,
`overlay_world_map_walk.bin`, and `overlay_world_map_top_ext.bin`
returns **only one store site** (`FUN_80026B4C @ 0x80026BA8`); every
other materialisation in those programs is a read.

After installation, each pointed-to TMD has the runtime shape:

```
[+0x00] u32 magic = 0x80000002
[+0x04] u32 flags     (= 1 post-fixup; FUN_800268DC's idempotency guard)
[+0x08] u32 group_count
[+0x0C] array of group_count × 0x1C-byte group descriptors
        each starts with `vertex_base_ptr (u32) + vertex_count (u32)`
        followed by 0x14 bytes of per-group state
```

### Readers (all access `DAT_8007C018[*]` read-only)

| Function | Site | Role |
|---|---|---|
| `FUN_80021B04` (SCUS actor allocator) | reads `DAT_8007C018[actor[+0x64].i16]` | populates `actor[+0x44] = [count, mesh_ptr[count]]` from TMD groups |
| `FUN_80024D78` (SCUS actor allocator — variant) | reads `DAT_8007C018[actor[+0x64].i16]` | same shape as `FUN_80021B04` but also OR-sets `actor[+0x10] \|= 0x08000000` (a per-actor enable flag) |
| `FUN_801D77F4` (overlay alt allocator) | reads `DAT_8007C018[(i16)param_2]` | copies vertex pool from sub-records into `actor[+0x90]` |
| `FUN_801D8280` (overlay table walker) | iterates `DAT_8007C018[0..DAT_8007BB38]` | hands each sub-record to `FUN_801D5E20` |
| `FUN_801F69D8` (world-map top-view dispatcher in `world_map_top_ext`) | reads `DAT_8007C018[(visible_object_kind8 + DAT_8007B6F8) * 4]` | walks per-tile visibility scratchpad and calls `FUN_80043390(tmd+0xC, color, fog)` |
| `FUN_8001E890` | sets `entry[+0x8] = 10` for three consecutive table indices at `DAT_8007B824 + 0..2` | per-pack count override (overwrites the installed TMD's `group_count` field) |
| `FUN_8001EBEC` | reads `DAT_8007C018[DAT_8007B824 + 0..2]` (3 consecutive party-character TMDs) | per-party-member group-descriptor patch — for each of 3 chars, picks one of two pre-built 0x1C-byte descriptors (`TMD+0x124` vs `TMD+0x140`) based on a per-character byte at `0x80084xxx + char_stride*N + offset`, then overwrites the indexed group descriptor in the TMD. Drives equipment-conditional mesh swaps |

The world-map top-view dispatcher `FUN_801F69D8` (2572 B / 643 instr at
prologue `0x801F69D8`, dumped in
`ghidra/scripts/funcs/overlay_world_map_top_ext_wm_ext_dispatcher_caller_801f69d8.txt`)
is the route the warp-into-world-map Read-bp probe captured. Its body
copies a 0x20-byte camera struct from `0x8007BF10` into scratchpad,
nested-loops over Y/X tile indices (padded by ±10), dereferences each
visible tile's 0x20-byte object record from
`_DAT_1F8003EC + 0x8000 + Y*0x100 + X*2`, applies frustum + GTE RTPT,
then routes the TMD via `DAT_8007C018` and calls `FUN_80043390`. The
`color` arg is `0xD0D0D0` default, switched to `0x40D0D0D0` if the
object record's `[+0x1E]` flag is set, and OR'd with `0x10000000` if
`record[+0x12] & 0x800`. The `fog` arg is
`clamp((GTE_screen_z - 0x5000) >> 3, 0, 0x1000)`.

### Live snapshot (Drake post-warp settled)

Drake world-map RAM dump after the warp has settled
([`captures/ram_dumps/drake_world.bin`](../../captures/ram_dumps/)):

| Field | Value |
|---|---:|
| `DAT_8007B774` (install counter) | `143` |
| `DAT_8007BB38` (walker counter) | `142` |
| `DAT_8007B6F8` (kingdom-TMD prefix) | `5` |
| `DAT_8007B828` (error bits) | `0x00000000` (no magic mismatches during install) |

Entry contents (per
[`scripts/classify_dat_8007c018.py`](../../scripts/classify_dat_8007c018.py)):

| Index range | Count | Content |
|---|---:|---|
| `[0..4]` | 5 | Character-mesh TMDs at `0x8014D554..0x801585C0`, group_count 10/10/10/3/2. Disc source: [§ Disc-side source of `[0..4]`](#disc-side-source-of-04) below. |
| `[5..142]` | 138 | Kingdom-derived TMDs at `0x800F7908..0x80138D44` (group_count 1..10, mixed sizes) |
| `[143..255]` | 113 | Either zero (uninstalled — never written) or stale junk past the walker counter — **never read by code** because every reader gates on `DAT_8007BB38` or an explicit index ≤ install counter |

**Every populated entry is a valid Legaia TMD** (magic
`0x80000002`, flags = 1, group_count > 0). The table is homogeneous in
the steady state. The kingdom-derived 138 TMDs include the slot-4
body-aligned addresses previously described (e.g. `[94..113]` land in
`0x8011A7B0..0x8012202C` — all inside the slot-4 RAM window — but the
bytes there have already been overwritten with TMD blobs by the time
the snapshot is taken; the slot-4 outer-pack signature is *absent*
from steady-state RAM).

### Disc-side source of `[0..4]`

The five character-mesh TMDs at `DAT_8007C018[0..4]` originate from
**PROT entry 0874 (`befect_data`)**, not from the dev-tree path
`data\field\player.lzs` (whose runtime name maps to PROT 876 —
`player_data` — which actually carries a VAB + TIM_LIST + SEQ
streaming-format payload with **zero TMDs**; see [data-field.md](data-field.md)
for the chunk shape).

PROT 0874 is a [`parse_player_lzs(buf, 3)`](asset-descriptor.md)-shaped
container with three LZS-compressed sections:

| Section | Type byte | Compressed size | File offset | Content |
|---:|:---:|---:|---:|---|
| 0 | `0x01` | `0xB49C` (46 236 B) | `0x20` | 5-TMD pack (LZS decodes to 65 536 B) |
| 1 | `0x02` | `0x41E0` (16 864 B) | `0x5037` | Secondary TMD payload |
| 2 | `0x03` | `0x1D524` (120 100 B) | `0x7055` | MAN-shape data |

Decoding section 0 (LZS-decompress from file offset `0x20`) yields a
canonical TMD pack — `[u32 count][u32 word_offsets[count]][TMD bodies]`
with word offsets in 4-byte units (same convention as
[`tim-pack`](tim-pack.md) / kingdom slot 1):

| Pack slot | Body offset | nobj (disc) | Body bytes (to next slot) |
|---:|---:|---:|---:|
| 0 | `0x0018` | 12 | 13 220 |
| 1 | `0x33BC` | 12 | 13 800 |
| 2 | `0x69A4` | 12 | 11 656 |
| 3 | `0x972C` | 3  | 6 488 |
| 4 | `0xB084` | 2  | 20 348 (trailing padding to pack end) |

Byte-equality check against a Drake post-warp RAM snapshot
([`captures/ram_dumps/drake_world.bin`](../../captures/ram_dumps/),
local-only):

- **Pack slot 3 vs RAM `DAT_8007C018[3]`** (un-fixup the runtime's
  absolute-pointer group descriptors back to disc-form offsets using
  `disc_off = abs_ptr - (tmd_base + 0xC)`): the full 6488-byte body
  matches byte-for-byte (0 differences over 0x1958 bytes compared).
- **Pack slot 4 vs RAM `DAT_8007C018[4]`**: the first 1048 bytes match
  byte-for-byte. The runtime allocates only the in-use prefix; the
  trailing ~19 KB of disc padding is not copied.
- **Pack slots 0/1/2 vs RAM `DAT_8007C018[0..2]`**: the first three
  group descriptors and groups 4..9 match byte-for-byte. RAM's `nobj=10`
  vs disc's `nobj=12` is a deliberate runtime override (see "10-group
  cap" below); RAM's slot-3 group descriptor is sourced from disc's
  group 11 (see the FUN_8001EBEC patch below).

### 10-group cap + equipment-conditional group patch

The five disc TMDs ship with `nobj=12` (for the three active-party
slots) and `nobj=3 / 2` (for the trailing two — confirmed `nobj` from
the disc pack matches RAM exactly for those). The active-party
post-install loop in `FUN_8001E890` overwrites
`DAT_8007C018[DAT_8007B824 + 0..2]`'s `entry[+0x08]` (TMD `group_count`)
to **10**, capping each of the first three TMDs at 10 active groups.
The last two disc groups (10 and 11) are *equipment-conditional*
descriptors: `FUN_8001EBEC` reads two per-character bytes (from
`0x80084xxx`, equipment slots) and for each of the three active party
slots picks either `TMD+0x124` (= group 10) or `TMD+0x140` (= group
11) and overwrites the indexed live group descriptor with that
pre-built 0x1C-byte template. This is the equipment-conditional mesh
swap (weapon variant, etc.) — see the
[`dat-8007c018-global-tmd-pointer-table`](#dat_8007c018--global-tmd-pointer-table-the-actual-cluster-a-source)
section's `FUN_8001EBEC` row in the readers table for the matching
asm trace.

### Loader chain — partly open

`FUN_8001E890`'s retail-PROT branch (`DAT_8007B8C2 != 0`) calls
`FUN_8003eb98(0x36C, piVar2, 1)`, which loads PROT 876's raw bytes
into `piVar2`. The downstream LZS calls then interpret
`piVar2[2..7]` as three `(size, offset)` pairs — but PROT 876's
bytes there are streaming-format chunk data (the start of a VABp
header inside chunk 0), not LZS descriptors. That branch is
therefore **incompatible with PROT 876's actual layout** in retail
and either (a) is gated off by `DAT_8007B8C2 == 0` in retail or (b)
is dead code. The `data\field\player.lzs` string and PROT-876 fast
path both fall over the same shape mismatch.

The retail loader that actually installs PROT 0874's section 0 into
`DAT_8007C018[0..4]` is not yet pinned. `FUN_800520F0` (the battle
scene loader) loads PROT 873+874 contiguously, but its two install
loops walk both buffers as flat `[count, offsets[], data]` packs and
process PROT 874's `count = 3` entries via `FUN_8001fbcc` (VDF
install). PROT 874 section 0 is gated on the type byte (`0x01`), so
a different dispatch site must funnel section 0 through the TMD-pack
handler (`FUN_8001F05C case 2` → `FUN_80026B4C`) rather than VDF.
Tracing that site is in [Open work](#open-work) below.

### Live snapshot (Sebucus mid-warp)

The Sebucus dump captures the warp transition partway through the
asset install:

| Field | Value |
|---|---:|
| `DAT_8007B774` (install counter) | `92` |
| `DAT_8007BB38` (walker counter) | `91` |
| `DAT_8007B6F8` (kingdom-TMD prefix) | `5` |
| `DAT_8007B828` (error bits) | `0x00000000` |

Entries `[0..91]` are valid TMDs; the install is in flight, so the
TMD-pack handler has not yet completed pushing every member. Entries
`[92..]` carry leftover pointers from a previous game-state's table
fill, but `DAT_8007BB38 = 91` means **no consumer ever reads past index 91**.

The mid-load Sebucus state is what historical "non-TMD entry
classification" passes appear to have sampled. Those mid-load reads
went *past* the walker counter and treated stale leftover pointers as
table content — producing the previously-reported "[45..53] FFFAFFFA",
"[114..193] mixed text/vertex/texture" classifications. With
`DAT_8007BB38` as the authoritative bound, those characterisations are
**out-of-bounds reads, not table contents**.

### Implication for slot 4 — partly resolved

The Read-bp probe that originally captured cluster-A reads against
slot-4 RAM observed reads to addresses that, in steady-state world-
map RAM, hold *real TMD blobs* (Legaia magic, post-fixup) **placed by
the kingdom asset chain into RAM that was the slot-4 load destination
during the warp**. Slot 4 of the kingdom bundle (type `0x05` = MOVE)
*is* decoded into that RAM region by the warp's first pass, but the
same physical RAM is then reused by later TMD-pack installs whose TMDs
replace the slot-4 bytes byte-by-byte before the world-map enters
steady-state. The slot-4 outer-pack signature is therefore absent from
the post-warp dump even though the buffer base RAM address is still
populated.

In other words: cluster A reads TMDs (via `DAT_8007C018` indirection
or actor mesh tables), and those TMDs *happen* to occupy what was once
the slot-4 buffer. Slot 4's MOVE bytes are gone by the time cluster A
runs; the geometry being rendered came from a different (later-loaded)
TMD-pack.

## Open work

1. **Slot-4 → TMD converter.** Slot 4 *is* loaded into RAM as MOVE
   bytes during the warp's first pass, but the same physical RAM gets
   overwritten by TMD blobs from a later-loaded TMD-pack before the
   world-map settles. The intermediate converter that walks the slot-4
   15/16-body outer pack and emits TMDs at the right RAM offsets has
   not been pinned. Candidates: a chunk-handler case in `FUN_8001E54C`
   that turns slot-4 sub-bodies into TMD-shaped blobs in-place, or an
   asset-load-chain step that doesn't lower through `FUN_8001F05C` at
   all. A Read-bp probe watching `DAT_8007C018[*]` entry addresses
   (not slot-4 RAM) during the install pass would identify which
   loader call populates the kingdom-derived entries `[5..N]`.

   **Static-side evidence narrowing the hunt** (sweep via
   [`scripts/scan_funcs_for_addr_range.py`](../../scripts/scan_funcs_for_addr_range.py)
   across SCUS + every captured overlay dump under
   `ghidra/scripts/funcs/`):

   - **`_DAT_8007B888` (MOVE-buffer pointer set by `FUN_8001F05C` case 5):**
     7 accessor sites in the entire dump corpus. SCUS: `FUN_8001F05C`
     (writer, the case-5 store), `FUN_8002541C` (writer, streaming-walker
     reset), `FUN_800204F8` (reader — Tactical Arts move-table parser).
     Overlays: 4 reader sites, **all in `overlay_baka_fighter`** (the
     boxing minigame). **Zero readers in any `overlay_world_map*` dump.**
     If the kingdom slot-4 → world-map pipeline went through the
     standard MOVE buffer, the world-map controller would need to read
     `_DAT_8007B888` somewhere — and it doesn't, statically. So the
     converter either runs *before* the warp's overlay swap-in (in
     SCUS code that doesn't read the MOVE pointer by name — e.g. via
     a function-pointer table populated at boot), or the slot-4 MOVE
     bytes are consumed via the asset-loader chain itself (a hook
     inside `FUN_8002541C` or its descriptor-walker sibling
     `FUN_80020224`) before the world-map overlay even sees them.
   - **`DAT_8007C018[94..113]` (the index range whose live snapshot
     once held slot-4-body-aligned pointers):** zero specialized
     readers — no function statically materializes any address in
     `0x8007C190..0x8007C1E0` via `lui+addiu`, `lui+lw_with_offset`,
     or positive-offset `lw` from the table base. Consistent with the
     [Live snapshot](#live-snapshot-drake-post-warp-settled) finding
     that those entries are real TMDs in steady state and are reached
     only through the generic table walkers that iterate
     `[0..DAT_8007BB38]`.

2. **Per-record 4th `i16` (`attr`).** 0 for body 4, 22 distinct values
   in body 5, 214 distinct in body 12. Body-12 attr-values cluster at
   `±1280, ±1792, 1793, ±1281, ±1025` - look like packed (high-byte,
   low-byte) tags rather than indices. The body-kind ↔ cmd_flags
   bank hypothesis (previously listed as open work) is **falsified**
   — Drake's dispatcher-entry probe shows neither `0x04000000` nor
   `0x20000000` is ever set in retail world-map play; only banks
   `0x00` and `0x50` (the fade-flag distinction) are exercised. See
   [Banks exercised in retail world-map play](#banks-exercised-in-retail-world-map-play).
   So body `kind ∈ {1, 2, 4}` is slot-4-internal data with no link to
   cluster-A bank dispatch.

3. **Banks 2 (`0xA0`) and 3 (`0xF0`).** Banks reachable in the
   dispatcher but never observed during retail world-map play.
   Candidates: dev/debug menu render modes, battle-overlay re-use of
   the dispatcher, or cutscene render paths. Setting up a wide-
   coverage `cmd_flags`-capture probe across multiple non-world-map
   game modes would pin which (if any) caller passes those flags.

4. **PROT 0874 section-0 loader site.** The byte-equality match
   between PROT 0874 section 0 (LZS-decoded from file offset `0x20`)
   and the 5 TMDs at `DAT_8007C018[0..4]` is conclusive (see
   [§ Disc-side source of `[0..4]`](#disc-side-source-of-04)
   above). The **inner dispatch** is fully pinned:

   - `FUN_80020224(asset_type)` walks `_DAT_8007B85C` as an
     [`asset-descriptor`](asset-descriptor.md) pack, calling
     `FUN_8001F05C(buf + offset, size, type, 0)` for each record.
   - `FUN_8001F05C case 2` is the TMD-pack installer: it
     LZS-decodes the section, walks the `[u32 count][u32
     word_offsets[count]][TMD bodies]` pack, and calls
     `FUN_80026B4C(pack + word_offsets[i] * 4, 0)` for each TMD.
   - The retail callers of `FUN_80026B4C` (from a corpus grep over
     `ghidra/scripts/funcs/`) are `FUN_8001E890`, `FUN_8001E928`,
     `FUN_800520F0`, `FUN_8001F05C` itself (recursive), `FUN_800513F0`,
     `FUN_800542C8`, and the muscle-dome minigame loader at
     `overlay_muscle_dome_801f19ec.txt`.

   The **outer producer** that feeds PROT 0874's bytes into this
   dispatch chain is not pinned in the static `SCUS_942.54` dumps:

   - `FUN_8001E890`'s retail-PROT branch (`_DAT_8007B8C2 != 0`)
     loads PROT **876** (`0x36c`), not 874, via
     `FUN_8003eb98(0x36C, piVar2, 1)`. PROT 876 is a streaming-format
     file (VAB + TIM_LIST + SEQ) whose first bytes are a `VABp`
     header, not a 3-section `parse_player_lzs(buf, 3)` container.
     The branch's downstream `FUN_8001a55c(piVar2[2] & 0xffffff,
     ...)` calls read those VAB-header bytes as `(size, offset)`
     pairs - shape-incompatible with PROT 876's actual layout.
     Either the branch is dead code in retail, or
     `_DAT_8007B85C` is populated by another caller first and
     `FUN_8001E890` only fires the dispatch.
   - `FUN_800520F0` (battle scene loader) is the only static SCUS
     caller that issues `FUN_8003e68c(0x36a)` / `FUN_8003eb98` with
     PROT 0x369+0x36A, but it loads them as a contiguous block via
     the debug `_DAT_8007B8C2 != 0` branch and processes the result
     through `FUN_8001fbcc` (VDF install) rather than as a
     3-section `parse_player_lzs` container.

   **Conclusion**: the dispatch goes through the generic
   `FUN_80020224` → `FUN_8001F05C case 2` → `FUN_80026B4C` chain
   from an overlay-resident scene loader (`FUN_801D6704` family
   - present in many scene/menu overlay dumps), not from any
   static `SCUS_942.54` site. The overlay populates
   `_DAT_8007B85C` from PROT 874 before invoking the asset-pack
   walker, but the exact CDNAME indirection (which overlay, on
   which path) needs a write-bp probe on `DAT_8007C018[0]` to
   isolate. The Lua-probe approach described in
   [`docs/tooling/pcsx-redux-automation.md`](../tooling/pcsx-redux-automation.md)
   is the appropriate next step.

   A further static narrowing: the `FUN_8001F05C` case-2
   "freeze" sub-path (`if (param_3 == 1) { _DAT_8007B704 =
   size; _DAT_8007B824 = pack_count; }`) is the sole SCUS
   `sw` writer of `_DAT_8007B824` (at PC `0x8001F2F8`). The
   freeze sets the persistent-base index that
   `FUN_8001E1B4` later reads to reset the install cursor
   (`DAT_8007B774 = _DAT_8007B824`), so a non-zero
   `_DAT_8007B824` would mark slots `[0..pack_count-1]` as
   carried across mode transitions. A corpus grep over every
   call site shows zero static SCUS callers of
   `FUN_8001F05C` pass `param_3 == 1` (the three direct
   callers - `FUN_80020224`, `FUN_8002541C`, and
   `overlay_baka_fighter_801d4c50` - pass `s6`, `0`, and
   `0` respectively), and zero dumped overlay callers of
   `FUN_80020224` pass `param_1 == 1`. So either the freeze
   path is in an overlay not yet captured, or
   `_DAT_8007B824` stays at its BSS-init value of zero
   throughout retail play and every mode rebuilds the TMD
   pool from index 0 (in which case the "persistent slots"
   semantic is vestigial, not load-bearing). The dynamic
   probe should also break on `_DAT_8007B824` writes to
   settle which case holds.

## See also

- [`subsystems/world-map.md`](../subsystems/world-map.md) - the world-map controller and render pipeline.
- [`subsystems/world-overview-viewer.md`](../subsystems/world-overview-viewer.md) - the static-site WebGL viewer.
- [Legaia TMD](tmd.md) - the candidate mesh format for the slot-4 records.
