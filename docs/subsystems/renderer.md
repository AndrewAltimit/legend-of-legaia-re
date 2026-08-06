# Renderer (Legaia TMD)

How the game draws its 3D meshes, and how the clean-room port reproduces that.

**Retail side.** The renderer is `FUN_8002735C` - 60 GTE ops, driven by a
per-mode descriptor table at `DAT_8007326C` that says how each primitive group
is laid out and which GP0 packet shape to emit. `SCUS_942.54` also carries a
light-source sibling, `FUN_80029888`.

**Port side.** `crates/tmd::legaia_prims` walks the primitives;
`crates/engine-render` draws them, emulating a 1024x512 PSX VRAM page so the
per-primitive texture-page / CLUT selectors decode in a fragment shader.

**The thing that catches people out:** retail runs **no light source** on the
field path - shading is baked into the TMD's per-primitive colour words, and the
GPU modulates the texel by them. The port does the same by default, so the
shading you get out of the box *is* retail's. What is not on by default is the
strict-PS1 rasterisation: vertex snap and 15-bit dither sit behind
[`psx_mode`](#rendering-knobs-what-is-faithful-what-is-a-choice). Summary:
**simulation is faithful with no opt-out; shading defaults to retail;
rasterisation defaults to clean.** See [Lighting](#lighting).

A second surprise, deliberate: the port draws **every** mesh a scene loads,
every frame - no frustum cull, no draw distance, no LOD. See
[No distance culling](#no-distance-culling-every-loaded-body-is-drawn).

## Per-mode descriptor table

The renderer treats the 8-byte-stride table at `0x8007326C` as a packed `{u32
first; u32 second}` per row, selects `row = ((flags >> 1) - 8) >> 1`, and reads
**byte3 = `first >> 24`** (the shape selector, `& 3` = `F`/`FT`/`G`/`GT`) and
**byte4 = `second & 0xFF`** (the base vertex-index offset in u16 units):

| flags   | row | raw 8 bytes               | byte3 (shape) | byte4 (vtx off) |
|---------|-----|---------------------------|---------------|-----------------|
| 0x10/11 | 0   | `04 00 00 05 07 00 00 00` | 0x05          | 0x07            |
| 0x12/13 | 1   | `09 00 00 07 06 00 00 00` | 0x07          | 0x06            |
| 0x14/15 | 2   | `04 00 00 00 02 00 00 00` | 0x00          | 0x02            |
| 0x16/17 | 3   | `06 00 00 02 06 00 00 00` | 0x02          | 0x06            |
| 0x18/19 | 4   | `07 03 00 01 07 00 00 00` | 0x01          | 0x07            |
| 0x1A/1B | 5   | `09 03 00 03 0B 00 00 00` | 0x03          | 0x0B            |
| 0x20-27 | (re-uses rows 0-5 via the same `(flags>>1)-8` math) |  |  |  |

The low 2 bits of byte3 select the OT packet shape (`0`=flat untextured,
`1`=flat textured, `2`=gouraud untextured, `3`=gouraud textured); the quad bit
`(flags>>1)&1` picks tri vs quad. Byte1 says whether the prim carries a leading
**colour** block: rows 4/5 (`byte1 = 3`) do, rows 0-3 (`byte1 = 0`) do not.
Rows 0/1 are the *light-source-lit* textured rows - their texture block starts
at prim offset 0 and normal indices trail the vertex indices. See
[`formats/tmd.md`](../formats/tmd.md) for the full per-mode record layout.

## Lighting

**The two retail TMD mesh renderers issue no light op** (the field object/decoration
path through `FUN_80043390` is a separate question - see below). `SCUS_942.54` has
two TMD renderers over this table - `FUN_8002735C` and its light-source sibling
`FUN_80029888` - and between them they issue exactly **one** GTE colour op:
`DPCS` (`cop2 0x780010`; real command `0x10`, `sf = 1`), the depth cue. Neither
ever issues `NCDS` / `NCDT` / `NCS` / `NCT` / `NCCS` / `NCCT` / `CDP` / `CC`, so
no light matrix is consulted and no vertex normal is transformed. The GTE light
matrix `L` (cr8-12) and light-colour matrix `LC` (cr16-20) *are* populated
(`FUN_8005B648` = `SetLightMatrix`, `FUN_8005B678` = `SetColorMatrix`), and the
only functions that *statically* consume them - via `NCCS`/`NCCT` - are the four
handlers `FUN_8004409C` / `FUN_8004423C` / `FUN_80044434` / `FUN_800445B0` (dispatch
kinds 8..11; see the dispatch table below).

**The field decoration path does not dispatch the NCC light handlers either -
it stays on the depth-cue path.** `FUN_8002735C`/`FUN_80029888` are statically
NCC-free; the per-scene field render library's static-object pass (`FUN_801F7088`)
emits through the `FUN_80043390` *dispatcher*, which owns the kind-8..11 NCC handlers,
so the field *could* light in principle. A cold-boot capture settles that it does not:
in a live `town01` field (reached New Game → prologue → Rim Elm), a `dirty_exec_hot`
sweep of ~46M interpreted instructions across idle + attempted walk lands **entirely**
in the kind-19 bank-1 depth-cue body `FUN_80045584` `[0x80045584,0x800457C4)`
(`DPCT`+`DPCS`), with **zero** hits anywhere in the kind-8..11 NCC band
`[0x800445B0,0x80044798)` - in particular zero at the two light-op sites
`NCCT` `0x80044724` and `NCCS` `0x80044750` (disassembled from the handler body).
This matches the battle, summon and `map01` samples: across every robustly-sampled
scene the field renders through `DPCS`/`DPCT` depth cue and the NCC handlers are not
observed executing. So the retail field runs **no hardware light source** at runtime;
the baked-colour + depth-cue model below is faithful for the object path too, and the
kind-8..11 `NccMode` in [`prim_dispatch`](../../crates/engine-vm/src/prim_dispatch.rs)
is a static data model with no runtime consumer (wire it only if a lit mesh path -
e.g. a 3D world-map renderer - is ever built).

Why the earlier evidence looked open, and two instrument caveats. A lone prior
`town01` capture (~31 K interp hits) showed the kind-11 NCC body and the fog bodies
hot in roughly equal measure; against the cold-boot sweep's ~46 M hits with exactly
zero NCC, that ~1500×-smaller window does not reproduce and is discounted as a
transitional/mislabeled sample. (1) The recomp's `gte_ring` records **only**
`RTPS`/`RTPT` - never `NCCS`/`NCCT`/`DPCS`/`DPCT` (`gte.cpp` records func `0x01`/`0x30`
into the RTP ring and `0x11` into the INTPL ring, nothing else) - so any "zero NCC in
a GTE-ring dump" is vacuous; only `dirty_exec_hot` (interpreted-PC histogram) is a
valid liveness probe here. (2) The `map01`-class world map dispatches through a
**different** jump table (`0x801F8968` → the 0901 overlay's own emit leaves at
`0x801F76xx`, hot via `dirty_exec_hot` at `0x801F6E6C`), so it never reaches the SCUS
NCC handlers - its "no NCC" is a different-renderer fact, not a light-path test.
Remaining caveat: the town sweep covered the Mist-era prologue arrival area (Vahn's
movement is script-locked there) and `map02`/`map03` are unreached; a free-roam
multi-screen town sweep is blocked by the recomp savestate-load freeze. See
[`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).

Field shading is instead **baked into the TMD**. Every primitive carries a
colour word `[R][G][B][GP0 code]`, the code byte being one of `0x20` (`F3`),
`0x24` (`FT3`), `0x28` (`F4`), `0x2C` (`FT4`), `0x30` (`G3`), `0x34` (`GT3`),
`0x38` (`G4`), `0x3C` (`GT4`), each optionally `| 2` for the semi-transparent
variant. A flat prim stores one word; a gouraud prim one per corner (only the
leading word carries the command byte - the rest have a zero top byte). The
renderer loads it into the GTE's `RGBC`, runs `DPCS`, and hands the result to
the GPU as the packet colour. The GPU then blends it with the texel:

```text
out = texel * colour / 128
```

`0x80` is therefore the neutral colour (texel unchanged), below darkens, and
above brightens - up to `255/128` ≈ 2x. That factor-of-two headroom is why
retail's field has more contrast than an unlit render: across the field scenes'
environment packs, ~79% of colour components sit below `0x80`, ~12% at it, and
~10% above. An untextured prim has no texel and is simply filled with the
colour.

`DPCS` blends that colour toward the far colour (`RFC`/`GFC`/`BFC`, cr21-23) by
`IR0`: `out = c + (fc - c) * IR0`. Both are staged per drawn object -
`FUN_80029888` writes the far colour from its `param_2` (each byte `<< 4`) and
`IR0` from `param_3` - and by `FUN_80043390`, which also stages the ambient /
background colour (`RBK`/`GBK`/`BBK`, cr13-15; consumed only by the `NC*` ops,
so inert on the field path). An unfogged field scene passes `IR0 = 0`, making
the depth cue the identity: a retail town0c capture's GTE register file shows
`RGB.Raw8 = 30 30 30 34` (a `GT3` prim) and an `RGB_FIFO` of `0x30, 0x60, 0x30`
- the prim's three baked corner colours, out of the depth-cue op byte-unchanged.

Engine port: `legaia_tmd::legaia_prims::Prim::colors` (populated for every prim;
the lit rows, having no colour word, get `MODULATION_NEUTRAL`) →
`legaia_tmd::mesh::VramMesh::colors` → a per-vertex attribute on the VRAM-mesh
pipeline → `psx_modulate` / `psx_depth_cue` in the shader prelude, mirrored on
the CPU by `legaia_engine_render::psx_light` and pinned by its tests. The far
colour and `IR0` are set with `Renderer::set_depth_cue` (default `IR0 = 0`); the opening
prologue's per-render-node pull is staged separately as a view-depth `IR0` ramp
(`Renderer::set_depth_cue_ramp` - see [the grade section](#full-scene-colour-grade)).

### The same shading in an exported `.glb`

An export is a fourth surface the packet colour has to survive, and the trap
there is the one that keeps recurring: a renderer with no colour stream draws
white, and white is `texel * 255/128`, so the loss reads as "too bright", not
as "unlit". The three exporters (`legaia_asset::monster_gltf` /
`scene_gltf` / `character_gltf`) carry it as a **`COLOR_0` vertex attribute**
under the one convention in `legaia_asset::gltf_color`:

| prim | attribute | over |
|---|---|---|
| textured | `colour / 128` | `baseColorTexture` (the atlas) |
| untextured | `colour / 255` | a white base (the fill) |

Two consequences worth stating outright. The divisor differs between the
halves, so reading one array for both is a real defect in both directions -
halving a textured model or doubling an untextured one. And a faithful
modulation factor **exceeds 1.0** (`0xFF / 128 = 1.99`), which the normalized
integer `COLOR_0` encodings cannot express, so the accessors are float; the
PSX clamps the *product* at 255, exactly where an LDR renderer clamps its own
output, so an unclamped float is equal-or-closer to the canvas than a
pre-clamped one in every viewer. Materials additionally declare
`KHR_materials_unlit` (in `extensionsUsed`, never `extensionsRequired`, so
non-supporting viewers fall back to the same material's PBR fields): retail
issues no light op, so handing the model to a viewer's lights would
re-introduce precisely the synthetic Lambert this project keeps deleting.

Verified on the exported bytes rather than at the call site -
`crates/web-viewer/tests/glb_packet_colour_real.rs` re-reads a summon `.glb`
and compares its `COLOR_0` accessors against the stream the canvas uploads.

### Per-prim dispatch table (`FUN_80043390`)

`FUN_80043390` is the per-prim *dispatcher* behind the two TMD renderers: it
decodes a primitive kind (`0..19`) and count, then tail-calls a **20-slot × 4
alpha-bank jump table** - `0x8007657C` on the SCUS path, `0x801F8968` when the
world-map overlay is paged in (`_DAT_1F800394 & 1`). Each handler does
`RTPT`/`RTPS` → `NCLIP` backface cull → `AVSZ3`/`AVSZ4` depth → packet write into
an ordering table (deferred `DrawOTag`; no direct GPU DMA). The alpha bank is the
`_DAT_1F800028` offset (`0x00`/`0x50`/`0xA0`/`0xF0` = opaque / half / additive /
subtractive).

| kind | bank 0 (opaque) | banks 1-3 (fog) | topo | colour op |
|---:|---|---|---|---|
| 0-7 | — | — | — | none (unused) |
| 8 | `0x8004409C` | (shared) | tri | **NCCS** (lit) |
| 9 | `0x8004423C` | (shared) | quad | **NCCS** (lit) |
| 10 | `0x80044434` | (shared) | tri | **NCCT** (lit) |
| 11 | `0x800445B0` | (shared) | quad | **NCCT+NCCS** (lit) |
| 12 | `0x80043658` | `0x800448B0` | tri | DPCS (fog banks) |
| 13 | `0x80043768` | `0x80044A3C` | quad | DPCS |
| 14 | `0x80043B58` | `0x80044FDC` | tri | DPCT |
| 15 | `0x80043C6C` | `0x80045194` | quad | DPCT+DPCS |
| 16 | `0x800438B8` | `0x80044C14` | tri | DPCS |
| 17 | `0x800439E4` | `0x80044DC8` | quad | DPCS |
| 18 | `0x80043DD4` | `0x800453BC` (b2 `0x800457C4`) | tri | DPCT/DPCS |
| 19 | `0x80043F10` | `0x80045584` (b2 `0x80045988`, b3 `0x80045BB4`) | quad | DPCT/DPCS |

Bank 3 (subtractive) is the only bank that selects `0x80045BB4`, and no retail
world-map caller observed so far sets the flag that reaches it - see
[`formats/world-map-overlay.md`](../formats/world-map-overlay.md) for the per-bank
capture counts. **Unreached in one capture is not unreachable**: the handler is a
real render mode and belongs in any sweep claiming to cover the prim-dispatch
family. Note also that the bulk-terrain path swaps in eight *overlay-resident*
replacements for kinds 12..19 (`0x801F7644..0x801F8690`, PROT 0901), so a sweep
bounded to the contiguous SCUS span misses them entirely.

Structural facts (raw table): slots **0-7 are NULL** in every bank; **8-11 are
bank-invariant** and the *only* handlers carrying a light source (`NCCS`/`NCCT`);
**12-19 are bank-dependent** - bank 0 is opaque with no colour op, banks 1/2/3 add
the `DPCS`/`DPCT` depth cue. Kinds 8..11 are the only handlers with an `NCC*` light
op, but `dirty_exec_hot` never catches them executing on any robustly-sampled scene -
battle, summon, `map01`, and a cold-boot `town01` field (~46M interp hits, zero NCC;
see "no light source on the field path" above) - so treat them as the ROM's
light-*capable* handlers, not a live light path. (The presumed consumer is the
world-map slot-4 landmark meshes, but `map01` renders through a *different* overlay
table and never reaches these SCUS handlers, so that consumer is unconfirmed at
runtime.)
**Topology is parity-based** (definitive from `AVSZ3` vs `AVSZ4`): even kinds are
triangles, odd kinds are quads, so neither range is a uniform vertex count. Kind
19 bank 3 (`0x80045BB4`) is a composite/tessellating body (emits both `POLY_G3`
and `LINE_F2`, dual `RTPT`).

The body at `0x80044798` - which sits *between* the lit set (`8..11`) and the fog
banks - is **not a table entry**: it is a transform-free `mfc2`-only GTE
result-read-back / packet-pack helper. The per-kind fog-bank bodies for `12..19`
continue past it, running to `~0x80045BB4`. (An earlier "the per-kind entries are
stubs ending ~`0x80044798`" reading is wrong - that address is only the end of the
bank-invariant lit set, and the fog bodies are full per-kind handlers beyond it.)
Unlike the lit `8..11`, the fog-bank bodies **are** on the live path: runtime GTE
sampling of a summon / battle catches kinds 16 (bank 1, `0x80044C14`) and 18/19
(bank 2, `0x800457C4` / `0x80045988`) executing. The depth-cued rasterizer is the
hot render path; the `NCC`-lit one is not.

Provenance: the jump table's computed `jr` is not statically resolvable - a static
recompilation of `SCUS_942.54` *does* emit the handler bodies (verified against the
recomp's `func_80043658` / `func_800448B0` / `func_800460AC`), but the table →
handler mapping is not recoverable by following calls - so the map itself is read
from the SCUS PSX-EXE directly (`t_addr = 0x80010000`, file offset =
`VA − 0x80010000 + 0x800`).

Engine port: `legaia_engine_vm::prim_dispatch` models this table - `slot_to_kind`
(topology-correct `PolyKind`), `slot_lit` (`NccMode` for slots 8-11), and
`RenderMode::applies_depth_cue` (the SCUS fog banks). The `NCCS`/`NCCT` kernels
live in `legaia_engine_render::gte::lighting` and are exercised by the `gte_trace`
parity oracle, but no wgpu path yet draws the world-map slot-4 meshes, so the lit
handlers are a faithful data model rather than a wired render path. This is a low
priority: retail itself is not observed dispatching to these handlers at runtime
(the world map renders unlit), so leaving them unwired matches observed retail
output; the `NccMode` metadata is kept for fidelity if a light-using scene is ever
found.

## 2D gradient-tile primitive - `FUN_8002BDC4`

Distinct from the two 3D TMD renderers: `FUN_8002BDC4`
(`ghidra/scripts/funcs/8002bdc4.txt`) fills a screen rectangle with a
tiled, double-gradient textured quad strip - the UI/backdrop primitive
behind gradient panels and bar fills. It takes an origin `(x, y)`, a texture
descriptor `param_3` (`[0]=u0, [1]=v0, [2]=tile_w, [3]=tile_h`), a
`tpage/clut` word `param_4`, and optional `w`/`h` overrides (`0` = take the
descriptor's tile size). It walks the rectangle in `tile_h + 8` row bands
and `tile_w`-wide columns, emitting a `0x34`-byte gouraud-textured quad
(`0x0C000000` tag) per cell straight into the ordering-table cursor
`_DAT_1F8003A0` and adding it through `FUN_8003D2C4`. The shading is a
bilinear ramp: the luminance runs from `0x40` and steps by
`0x900 / (h + 8)` down each band and by a per-column delta across each row,
so the fill is a smooth two-axis gradient rather than a flat tint. `param_4
& 0x80` toggles the base RGB word between two brightness constants
(`0x3E800000` vs `0x3C800000`). It is a pure primitive-buffer writer with no
GTE transform.

## Other SCUS-band emitters (documented, not ported)

Beyond the two 3D TMD renderers and the gradient-tile primitive, the SCUS render
band carries a set of smaller GTE/GPU emitters. The clean-room engine reproduces
all of these through its own wgpu path, so they are **documented, not ported** -
their per-address roles live in
[`reference/functions.md` § Renderer / GPU primitives](../reference/functions/renderer.md#renderer--gpu-primitives):

- **`FUN_80028158` / `FUN_8002A5A4` / `FUN_801CFA48`** - the three multi-target
  primitive emitters the per-actor RENDER dispatcher `FUN_8001ADA4` case 4 picks
  on `actor[+0x9e]`. Each is a GPU packet builder over a caller buffer, unpacking
  a primitive count from the high byte of its packed param (`801CFA48` OR-s the
  GT4 command base `0x3C000000`).
- **`FUN_80019D50`** - a BGR555 cell-grid emitter: one coloured quad per non-zero
  `u16` cell (5-5-5 + `0x8000` STP bit) into the OT cursor `_DAT_1F800314+0x8c`.
- **`FUN_800351C0`** - the full-screen `320×224` backdrop quad (tag `0x08000000`).
- **`FUN_8001B73C`** - a GTE on-screen visibility test (RTPT the four corners of
  an actor box, accept if any projects inside the `320×240` screen), not an
  emitter - a cull probe. See
  [On-screen probes](#on-screen-probes-two-tests-that-are-not-the-same-test).
- **`FUN_80029DD8`** - a 39-`cop2`-op 3D primitive emitter, sibling of
  `FUN_8002735C` / `FUN_80029888`.

## The billboard projector (`FUN_800195A8`)

The one helper every camera-facing rectangle in the game goes through: MVMVA the
centre point into view space, fan four corners out around it with 16-bit adds,
reset the GTE rotation to identity with TR zeroed, optionally compose an
in-plane `Rz` from the 12-bit-angle LUT, then `RTPT` + `RTPS` the corners and
hand back four SXY words plus `SZ3 >> 2` as the OT bucket. Riders include the
battle move-FX afterimage streak and ribbon (`FUN_801E1AB0` / `FUN_801E1D98`),
the on-screen probe `FUN_8005126C` below, and the cutscene / world-map sprite
emitters.

Two properties of the projection are easy to get wrong in a port, and both are
hardware, not detail:

- the perspective divide is the GTE's **UNR reciprocal**, not an exact
  `h * x / z`, and `MAC0 >> 16` is an *arithmetic* shift - it floors. A
  symmetric box therefore does not project symmetrically about the screen
  centre.
- `swc2` stores the **SXY FIFO** entry, which the GTE has already saturated to
  signed 11 bits (`[-0x400, 0x3FF]`). A behind-camera corner is not a special
  case: `SZ3` clamps to `0`, the divide returns its saturated `0x1FFFF`
  quotient, and the corner lands near `OFX + 2 * IR1` rather than at any
  sentinel - the classic PSX behind-the-lens smear.

Port: [`legaia_engine_render::billboard`](../../crates/engine-ui/src/billboard.rs),
which runs both through the same `gte_divide` / `saturate_sxy` kernels the
`Camera::transform` COP2 oracle is pinned against.

## On-screen probes: two tests that are not the same test

Retail has two GTE-backed "is this thing visible" probes. They read alike from
the outside - project a box about an actor and return a boolean - and they are
**not** interchangeable. Both are worth keeping straight because the engine port
does no culling of its own (see
[No distance culling](#no-distance-culling-every-loaded-body-is-drawn)), so
anything that consults one of these is deciding to remove geometry the port
otherwise draws.

| | `FUN_8001B73C` | `FUN_8005126C` |
|---|---|---|
| box | actor `+0x14` ± `(size+1)<<6` in X/Z, `<<7` in Y | seat position ± `actor[+0x58]`, square |
| projection | `RTPT` per corner triple, in-line | the billboard projector `FUN_800195A8` |
| accept | any corner inside a real rectangle | the box's horizontal **span** overlaps the screen band |
| Y tested | yes, `0 <= y < 0xF1` | **no** |

`FUN_8001B73C` is the rectangle test: it RTPTs corner triples and takes the
first corner whose X passes `sltiu 0x140` (an unsigned compare, so negatives
fail on the same instruction) and whose Y is in `0 ..= 0xF0`.

`FUN_8005126C` is the battle sprite's re-anchor + horizontal test, and it is
the narrower of the two in every respect. It resolves the sprite's owner
through the 8-slot battle actor table `&DAT_801C9370` at `actor[+0x5A]`, copies
that actor's `+0x3C` `SVECTOR` verbatim into its own `+0x14`, projects a square
box of half-extent `actor[+0x58]` about it with no in-plane spin, and then reads
back exactly **two** halfwords: the X of corner 0 and the X of corner 1. It
rejects only when both are `>= 0x141` or both are `< 0`, i.e. when the span
misses `[0, 0x140]` entirely. No Y is read at any point, so an actor a full
screen above or below the viewport still reads as on-screen.

Port: [`legaia_engine_render::battle_on_screen`](../../crates/engine-render/src/battle_on_screen.rs)
(`battle_actor_on_screen`), riding
[`billboard::project_billboard`](../../crates/engine-ui/src/billboard.rs).
It is inert, and so is retail's. `0x8005126C` has **no reference of any kind**
on the disc - no literal address word in any table or actor template, no `jal`,
no `j`, no PC-relative branch, no `lui`+`addiu` materialisation - across
`SCUS_942.54`, every based overlay image and the raw bytes of every extracted
`PROT.DAT` entry
([`address-reference-scan.md`](../tooling/address-reference-scan.md);
[`battle.md` § Unreferenced SCUS entry points](../reference/functions/battle.md#unreferenced-scus-entry-points)).
So "what does retail do with a `0`" has an answer, and it is *nothing*: no pass
consults this verdict. The port's lack of a cull here is parity, not a gap
waiting on a caller.

## Per-primitive TMD render helpers (`FUN_8002735C` family)

Three helpers hang off the main TMD renderer `FUN_8002735C`, documented but not
ported (the clean-room engine projects and rasterises through wgpu):

- **`FUN_80027C6C`** - the per-primitive GTE emitter. Loads a group's vertices
  into the GTE (`lwc2`), runs `RTPT` (`cop2 0x280030`), reads the group mode byte
  and dispatches on its low 2 bits (`F`/`FT`/`G`/`GT`) to pack the matching
  `POLY_*` packet straight into the active primitive cursor `_DAT_1F8003A0`. See
  `ghidra/scripts/funcs/80027c6c.txt`.
- **`FUN_80027F00`** - the near/far vertex clip loop. For each edge whose endpoint
  falls outside the screen-space clip bound at `[0x1F800314]+0x6C`, it computes the
  crossing fraction `((bound - a) << 12) / (b - a)` and calls the interpolator to
  synthesise a clipped vertex before handing the group to `FUN_80027C6C`. See
  `ghidra/scripts/funcs/80027f00.txt`.
- **`FUN_80029724`** - the vertex-attribute interpolation kernel the clip loop
  calls. Given an output slot, two vertices and a q12 fraction `a3`, it lerps
  X/Y/Z (`out = b + ((a-b)*frac >> 12)`) and, gated by the flag word `a2` (bit 0
  `0x1` the UV pair at `+0x18/0x19`, bit 1 `0x2` the RGB triple at `+0x14..0x16`,
  bit `0x800` selects the trailing endpoint), the packed RGB and UV bytes. Pure integer arithmetic, but kept unported because it exists only to
  service retail's software near-plane clip. See
  `ghidra/scripts/funcs/80029724.txt`.

## 2D `POLY_*` packet emitters

A small family builds flat 2D `POLY_G3` / `POLY_G4` packets from an
already-projected screen-XY vertex array (no GTE transform) into the primitive
cursor `_DAT_1F8003A0`, advancing it by the packet size and linking through
`FUN_8003D2C4`. They back the HUD / menu number and panel draws:

| Addr | Packet | Bytes | Role |
|---|---|---|---|
| `FUN_8003C510` | `POLY_G3` (cmd `0x28`) | 24 | gouraud triangle; copies 3 XY pairs + inline per-vertex RGB |
| `FUN_8003C43C` | `POLY_G4` (cmd `0x38`) | 36 | gouraud quad; copies 4 XY pairs, then fills colours via `FUN_80036C4C` |
| `FUN_80036C4C` | colour writer | - | packs per-vertex RGB into a `POLY_*` packet, `a2` = 3 (tri) or 4 (quad) |

`(a2 << 1) | a3` forms the semi-transparent-bit + command byte; the leading tag
word is the packet-length code (`0x05000000` / `0x08000000`). See
`ghidra/scripts/funcs/8003c510.txt`, `8003c43c.txt`, `80036c4c.txt`.

## Frame setup + present

- **`FUN_800271A8`** - graphics-scratch init. Allocates two `0x8000`-byte buffers
  (`FUN_80017888`) into `0x8007BB04` / `0x8007BB08`, fills the second with a
  `0x4000`-entry `u16` depth ramp (`sra(acc, 18)`, `acc` stepping quadratically),
  then resets the GTE / primitive buffers (`FUN_8005B268`, `FUN_8003D1A4`,
  `FUN_8003D254`). Runs once before the emitters have a buffer to write into. See
  `ghidra/scripts/funcs/800271a8.txt`.
- **`FUN_8003DAA8`** is **not** a present driver, despite the counters it
  advances. It is the CD load-kick / completion driver the asset queue drains
  through - the four routines it calls are the **libcd** family, not libgpu:
  `FUN_8005C42C` is the LBA→BCD-MSF conversion, `FUN_8005C034` the `CdControl`
  retry wrapper over `FUN_8005CF80`, and `FUN_8005BEE4` / `FUN_8005BECC` the
  ready / sync callback installers. The `gp+0x8E8` / `gp+0x964` pair it maintains
  are load counters, and `_DAT_8007B876 & 1` is the read-in-progress flag, not a
  display mode. Full contract in [`boot.md`](boot.md) § the CD-read API; the
  `FUN_8005C034` identity is settled in
  [`re-settled-threads.md`](../reference/re-settled-threads.md#fun_80018db0-is-a-rumble-cadence-not-an-audio-one).
  See `ghidra/scripts/funcs/8003daa8.txt`.

### The battle backdrop is built by a different mesh builder on each host

One TMD, one second-copy transform, two builders - and the two do not agree on
screen. Same stage, same disc:

| | native `play-window` | browser play page |
|---|---|---|
| Textured half | `tmd_to_vram_mesh` | `tmd_to_vram_mesh_field_hybrid` |
| Untextured `F*`/`G*` half | `tmd_to_color_mesh`, uploaded through `upload_color_mesh_blended` with its per-prim `blend` channel, drawn on the colour pipeline | folded into the same mesh, surfaced as a per-vertex RGBA `flat` channel (alpha `0` = untextured) |
| Draws per frame | two (one textured, one colour) | one |

Both hosts append the second copy at build time (`VramMesh::append_scaled`
under `SecondCopy`), so neither draws it as a second `SceneDraw` - the single
`draws.push` in the battle branch is *not* evidence that the shell is drawn
once. Retail draws it twice, and which transform the copy takes is per stage
(`legaia_asset::battle_backdrop`).

> **Resolved, and the builder split was not the cause.** The visible
> divergence - repeating vertical lighter bands across the browser's sky and
> mountain arc - was **one multiply in the fragment shader**, not the two
> builders: both carry the same per-vertex colours and the same ABE bit.
> `site/js/webgl-shaders.js` applied a synthetic Lambert on its untextured
> path, and a sky dome's panels sweep every azimuth, so a term in the
> screen-space geometric normal painted exactly those bands. The untextured
> branch now draws the packet colour, as retail and as the native renderer do.
> The table above still describes the builder split accurately - it is real,
> and it is still something the host-drift gate cannot see, because both hosts
> *do* reach a backdrop builder - but it is not what was on screen. Full
> analysis, including how the textured path closed the same way (the page now
> uploads the per-vertex packet colour and applies `texel * colour / 128`), is
> in [`host-drift.md`](../tooling/host-drift.md).
>
> The reproduction cost is also gone: `play_battle.rs` exports a wasm
> `debug_force_battle(row)` that mirrors native `--battle <ROW>`, so a headless
> page can enter a named formation directly. Before it, `debug_start_test_battle`
> was `#[cfg(not(target_arch = "wasm32"))]` and town stages - which roll no
> encounters - could not be reached in the browser at all, which is why nobody
> had ever looked at one.

### The screen the GTE projects onto is 320x224, not 320x240

Two numbers travel together and are easy to get half-right. The GTE screen
centre and the size of the frame it is the centre *of*:

| Quantity | Retail value | Where it is read |
|---|---|---|
| `OFX` | `160` (`160 << 16` in the control word) | GTE control file, save state |
| `OFY` | `114` (`114 << 16`) | GTE control file, save state |
| Drawing area | `320 x 224` | GPU `ClipX0/Y0/X1/Y1` |
| Draw offset | `(0, 4)` / `(0, 244)` | GPU `OffsX/OffsY`, alternating buffers |
| Display window | `320` x **228** scanlines | GPU `DisplayFB_*` + `DisplayVStart/End` = `(28, 256)` |

All five hold across a nine-state corpus spanning field, battle, battle load
and the dance minigame, on both halves of the double buffer. `H` is the
counter-example that makes the constancy a finding rather than a property of
the measurement: it is written per phase (`256` in battle, `512` in the field).
Oracle: `crates/mednafen/tests/gte_projection_real.rs` for the control file,
`mednafen-state vram-dump --regs` for the GPU registers.

So `OFY = 114` is **not** an off-centre offset in retail's own frame - it is
`228 / 2`, dead centre of the display window, and two rows below the centre of
the 224-row drawing area. It reads as "six pixels above centre" only against a
240-row frame, which retail never draws. `SetGeomOffset` writes
`(width / 2, height / 2)`; Legaia's height is 228, not 240.

**What the port does with that.** Both hosts keep a 320x**240** logical screen,
because every 2D rect the port draws is a retail draw-area coordinate copied
verbatim (HUD panels, dialog boxes, menu windows) and those are authored in the
space `OFY = 114` lives in. The projection therefore has to put the GTE origin
on row 114 of the port's frame as well, or the 3D and the chrome disagree by six
rows. Both matrices carry it as a constant `w`-scaled term on clip `y`,
`GTE_OFY_NDC_BIAS = (120 - OFY) / 120`, so it survives the perspective divide:
`legaia_engine_vm::battle_cam_script::battle_vp` (shared by the browser play
page) and `psx_camera_mvp` (native window). Guard:
`the_projected_origin_lands_on_the_retail_screen_centre`, which reads the
expected row from `GTE_OFY` rather than from the matrix, so a projection rebuilt
on `240 / 2` fails it.

The residual is the frame height itself: the port's 240 rows against retail's
224 drawn / 228 displayed means retail's picture occupies `224 / 240` of the
port's vertical extent, i.e. everything reads about 7% smaller relative to the
window and the bottom 16 rows are frame the hardware never shows. Closing that
moves every pinned 2D rect, so it is a screen-convention change rather than a
projection one.

## Numeric-glyph string emitters

`FUN_80034CC4` / `FUN_80034FA0` draw a base-10 integer as a run of font glyphs.
Both divide the value against the place-value table at `0x80073DCC`, offset each
digit by the glyph base `0x82` (ones digit `+0x4F`), assemble the string in a
stack buffer seeded from the `0x80010C10` template, and submit it through the
sprite drawer `FUN_80036888`. `FUN_80034FA0` presets the leading-digit flag
`gp+0x15C = 1` (zero-padded / fixed-width form); `FUN_80034CC4` honours the flag
as passed. See `ghidra/scripts/funcs/80034cc4.txt`, `80034fa0.txt`.

## Arts-list panel renderer

`FUN_80034358` draws a scrollable list of the active character's learned Arts.
The active character index is `gp+0x874`; per-character state is the `0x414`-byte
block at `0x80084140 + char*0x414`. The learned-art id list starts at block
`+0x74E`, its length at block `+0x74D`, and `gp+0x140` is the scroll top. Visible
rows = `param+0x10 / 0x1C`, laid out `0x1E` apart in Y from `param+0xC`, at base
X `param+0xA`.

For each visible slot it scans the arts-name table `DAT_80075EC4` (stride `0x14`,
terminated when a record's first byte reaches `99`) for the entry keyed on
`[character, art-id]`, then:

- draws the art name via the glyph-string primitive `FUN_80036888` under text
  attribute `gp+0x13C = 7` (CLUT 7);
- draws the art's AP cost - decimal-split against the place-value table
  `0x80073DCC` - through the sprite primitive `FUN_8003C11C`, halved when the
  character block's flag word `+0x6C0` has bit `0x800` set;
- draws the art's input command as arrow sprites via `FUN_8003C310`, one per
  input, keyed on the four direction codes `DAT_80073E4D` / `4F` / `51` / `53`.

It is the SCUS-resident sibling of the overlay "Moves" submenu (same arts-table
data, same name / AP / command-arrow layout) in
[`field-menu.md`](field-menu.md#moves-list-submenu-3); the two differ in row
pitch and host screen. See `ghidra/scripts/funcs/80034358.txt`.

## TMD pointer table

`FUN_80026B4C` writes registered TMDs to `*(int **)(idx * 4 + 0x8007C018)`. Consumers in retail (4 functions, all setup-not-render):

- `FUN_80021B04` - actor-spawn helper, builds per-actor OBJECT pointer table.
- `FUN_80024D78` - per-actor OBJECT-table rebuild.
- `FUN_8001EBEC` - per-frame OBJECT[10/11] swap (pose select for player TMDs).
- `FUN_8001E890` - the "DATA_FIELD player loader"; see below, its name misleads.

The per-actor `OBJECT[i]` is a 28-byte struct copied into `actor[0x44][i+1]` from `tmd + 12 + i*28` - `sizeof(OBJECT) = 28`.

### `FUN_8001E890` does not load the player meshes

The name is inherited from the dev string `data\field\player.lzs`, and it is a
trap. That string maps to PROT 876 (`player_data`), which is what the loader's
retail-PROT branch targets - and PROT 876 is a streaming-format VAB + TIM_LIST +
SEQ payload, **not** a TMD pack.

The `DAT_8007C018[0..4]` character TMDs actually come from PROT 0874
(`befect_data`) section 0. See
[`docs/formats/world-map-overlay.md` § Disc-side source of `[0..4]`](../formats/world-map-overlay.md#disc-side-source-of-04).

What `FUN_8001E890` does write into `DAT_8007C018[0..2]` is the post-install
group-count cap (`entry[+0x08] = 10`) and the equipment-conditional patch
dispatch into `FUN_8001EBEC`.

## VRAM emulation in the engine port

`crates/engine-render` emulates a 1024×512 R16Uint VRAM page so the per-prim CBA/TSB selectors plus 4/8/15bpp + CLUT decoding can happen in a fragment shader. The viewer uploads every sibling TIM into VRAM so multi-page meshes render correctly.

CLUT data scatters across PROT entries - many character meshes reference CLUT
rows that live in *different* PROT entries from their TMD source. This is the
problem the rest of this section is about.

Engine-side scene loads resolve it from the disc: `SceneResources::build_targeted`
walks the scene's own entries plus the shared and boot-resident blocks (see
[Engine-side targeted upload + shared blocks](#engine-side-targeted-upload--shared-blocks)),
with no hand-supplied directory.

The asset-viewer's `--vram-extra-dir` is a *viewer* flag for browsing extracted
`tim_scan/` dirs that are not tied to a CDNAME scene; it is not on the engine's
scene-load path (`engine-core` never reads it). See
[`asset-loader.md`](asset-loader.md#clut-data-scattering).

### Capturing a drawn frame into VRAM

On the console the framebuffer *is* VRAM: the display area is a rect inside the
same 1024×512 halfword page textures are read from, so a primitive can sample
pixels the GPU drew moments earlier. That is not a curiosity - it is how the
field-to-battle transitions work. The curtain style slices a **captured field
frame** into 240 row strips and 320 column strips and stretches them apart, and
each strip is an ordinary textured quad whose texture page is the capture.

The port's renderer draws through wgpu into a colour attachment with no
relationship to `legaia_tim::Vram`: `Renderer::upload_vram` pushes the software
page *to* the GPU and nothing comes back. `engine-render`'s `vram_capture`
module adds the missing direction, and `Renderer::capture_into_vram` is
`capture_rgba` plus that blit.

**Where retail parks the capture** falls out of the curtain's own texture-page
words, with no capture needed to read it. `0x105` / `0x108` decode to 15-bpp
pages at VRAM `(320, 0)` and `(512, 0)`; the column pass' `0x115` / `0x118` to
`(320, 256)` and `(512, 256)`. A page is 256 halfwords wide, and the row pass
draws a `0xC0`-wide strip from the first page followed by a `0x80`-wide strip
from the second - `0xC0 + 0x80 = 0x140`, one 320-pixel scanline spanning VRAM
columns `320..=639`. So the capture is a 320×240 15-bpp image at `(320, 0)`,
with a second copy at `(320, 256)`, parked to the right of the two display
buffers at `(0, 0)` and `(0, 240)`.

Three properties worth knowing before using it:

- **The quantisation is exact.** Every colour on the engine's path is a
  display-referred PSX framebuffer byte (see
  [Colour space](#colour-space-psx-framebuffer-values-end-to-end)), and the last
  stage of every 3D shader expands a 5-bit channel as `(c5 << 3) | (c5 >> 2)`.
  `byte >> 3` therefore recovers `c5` for all 32 values, so a dithered frame
  round-trips bit-for-bit; an undithered one takes the same 24→15 bit truncation
  the PSX GPU applies on store.
- **The resample is the one deviation.** Retail captures at the native 320×240;
  the port renders at the window size, so the blit point-samples down into the
  destination rect. At 320×240 the map is the identity and nothing is resampled.
- **The mask bit is the caller's choice.** A 15-bpp texel of `0x0000` is
  *transparent* when sampled, and black framebuffer pixels are exactly `0x0000`
  - which is also the codebase-wide "unpopulated" word. Setting bit 15 (the
  default) makes a captured black pixel opaque; clearing it reproduces the
  "black reads as a hole" behaviour, at the cost of making the capture
  indistinguishable from an unwritten region.

The write lands in the **CPU-side** page, not a GPU-only copy, so a capture is
equally visible to `Vram::move_image`, `region_has_data` and the VRAM parity
oracle. That costs a full readback per capture, which is why this is a
transition-frame primitive rather than something to run every frame.

### Screen-space ordering-table pass

The 2D half of the renderer: PSX `POLY_FT4` / `POLY_GT4` quads plus flat quads,
drawn back-to-front by ordering-table bucket with per-ABR semi-transparency,
sampling the shared VRAM through the same CLUT decode the 3D VRAM-mesh path
uses. `order_primitives` reproduces `AddPrim` + `DrawOTag` exactly: descending
OT index, LIFO within a bucket.

**The model is renderer-agnostic and lives in `engine-ui::screen_prim`**: the
primitive record (`ScreenPrim` / `ScreenQuad` / `FlatQuad`), the ABR extraction,
the ordering-table sort, and `build_geometry`, which is the *only* public route
from a primitive list to something drawable. `engine-render`'s `screen_overlay`
re-exports all of it at its old path and adds the wgpu-side wiring plus the
afterimage packet builder; the browser play page links the same module and
uploads the same three arrays to WebGL2.

A host never receives a primitive list, only `build_geometry`'s output - a
vertex buffer, an index buffer and a run table - so the OT order is baked into
the index buffer before either host sees it. Two hosts cannot disagree about a
sort neither of them performs, which is the point of putting it in the shared
leaf: an ordering divergence surfaces as one blended quad stacking wrong on one
host, under one camera angle, which no diff and no single screenshot catches.

Two things about how it reaches the frame:

- **It composites.** `RenderTarget::ScreenOverlay` is a whole-frame mode - it
  clears and draws nothing but quads - so it cannot put a streak over a battle
  scene or a transition strip over a field scene, which is what every consumer
  actually needs. `RenderTarget::SceneWithScreenPrims` draws a `Scene` and then
  the quad list in the same frame, at the reversed-Z near plane so the quads
  pass the depth test against any scene geometry. Retail has no such
  distinction: 3D primitives and screen-space packets go into the *same*
  ordering table and `DrawOTag` walks it once, so the two-path split is a port
  artifact and this is where the halves meet.
- **Its coordinate space is the PSX display, not the window.** Every retail
  screen-space emitter authors in 320×240 and clamps against it, so the staging
  pass maps that space across the whole surface - the same mapping the shell's
  `screen_fx` meshes get from `orthographic_rh(0, 320, 240, 0)`. Handing the
  geometry builder the surface size instead pins a 320×240 overlay into the
  top-left corner of a larger frame.

A quad may carry one flat modulation colour (`POLY_FT4`) or four per-vertex
colours (`POLY_GT4`). The gradient is not decoration: a transition quad's
descriptor carries a separate top-edge and bottom-edge colour, and the two
differing is what makes the quad a gradient.

Both the native window and the browser play page draw this list; the page's
WebGL2 pass runs the same fragment decode, binds the same four ABR equations its
3D blend pass uses, and consumes the shared builder's output verbatim. Both
also *produce* it from the one shared emitter: `battle_intro` is wgpu-free
(`engine-ui`, re-exported at its old `engine-render` path), and each host owns
only the readback that lands the captured field frame the styles sample -
`capture_rgba` on the native window, `gl.readPixels` on the page. See
[`host-drift.md`](../tooling/host-drift.md#screen-space-psx-primitives-across-the-two-hosts)
for how the capture reaches each host's VRAM.

### The field-to-battle transition emitter

The two capabilities above exist for one consumer, and
`battle_intro` is it: the per-frame, per-style working-set owner that stands
between the transition state machine (live in `engine-core`, driven by
`World::tick_encounter`) and the ordering table. It seeds the selected style's
working set, advances it off the transition entity's own `+0x1A` clock rather
than counting for itself, and emits `ScreenPrim`s plus the per-style fade.

**All five styles draw**, each through its own retail packet builder. The
curtain's builder (`FUN_801CF1B0`) emits *screen-space* corners with texture
page, CLUT, UVs and a top/bottom colour pair, so it needs no projection step;
its `0x14`-stride descriptor table parses out of PROT 0979, and its texture
pages decode to the capture rects above. The other four end in the same GTE
projection the module reproduces (the FT4 handler's NCLIP-pair / `AVSZ4` /
near-cutoff accept chain): the tile shatter and the swirl assemble synthetic
Legaia-TMD objects for the per-prim dispatcher (`FUN_801D0E54` /
`FUN_801D1A20` - the swirl dispatches double-sided via flag bit 27, which is
what lets its x-mirrored half draw), and the two particle fields write
`POLY_FT4` patches of the captured frame straight into the ordering table
(`FUN_801CFDA0` / `FUN_801D0370`, plus the spin-up style's expanding-ring tail
`FUN_801D1CFC`).

What this means in play: every battle opens with the transition retail gives
it - the ordinary random encounter's tile shatter included - see
[`cutscene.md`](cutscene.md#which-style-a-battle-gets) for how a battle's style
is selected.

Both hosts reach the style bodies through the one emitter. The native window
re-renders the scene offscreen and lands the readback through
`battle_intro::update_field_capture` (the crate's one renderer-bound wrapper);
the browser page reads back its own drawn frame (`gl.readPixels`, rows
bottom-up - the emitter's blit flips them) and re-uploads its single VRAM
texture with the captured clone for the length of the window. The clone also
covers the page's one-texture nuance: its field meshes sample the same texture
the capture rects land in, but the emitter's opaque backdrop covers the
display from the first armed frame, so nothing corrupted is ever visible. See
[`host-drift.md`](../tooling/host-drift.md#screen-space-psx-primitives-across-the-two-hosts).

### Targeted VRAM upload

The TIM corpus on a single PROT entry can run into the hundreds. Uploading every
one of them into the 1MB VRAM clobbers regions a different mesh references as its
CLUT row, and the paletted decode then reads image pixels as palette entries -
the rainbow noise.

The asset viewer and the `tmd` CLI both go through
`legaia_tmd::vram_targeted::build_vram_targeted`. For every TIM, the image block
and the CLUT block are decided *independently* against the prim-target rectangles
for the current TMD, so a TIM can contribute one block, both, or neither.

`legaia_tim::vram::Vram::prim_texture_status` then classifies each prim's
`(cba, tsb, uv)` lookup as `Ok` / `MissingClut` / `MissingTexturePage` (plus
the retired `ClutDepthMismatch`, which nothing produces - see below). The
viewer drops bad prims at mesh-build time; the CLI can explain *why* a prim
was dropped. The common case is a prim whose CLUT row or texture page no
loaded TIM has uploaded at all.

The same filter is wired into engine-side scene loads through `ResolvedTmd::build_filtered_vram_mesh`, so battle / field actor meshes inherit the same cleanup the asset viewer has.

### Engine-side targeted upload + shared blocks

`SceneResources::build_targeted` is the engine-side mirror of the asset-viewer's
targeted-upload path. It parses every TMD in a scene, collects the union of all
prim-target rectangles (CLUT rows + texture-page UV bboxes), then walks every TIM
and decides per-block whether to write it.

This matches what the retail field loader does - DMA only the texture bytes the
current scene's meshes need - and avoids the CLUT-row collisions that drop 80%+
of textured prims under the naive "upload every TIM" path.

**Shared blocks.** `build_targeted` also accepts a list of *shared* CDNAME blocks
via the [`FIELD_SHARED_BLOCKS`](../../crates/engine-core/src/scene_resources.rs)
constant (`init_data` + `player_data`) - the blocks the retail engine keeps
resident across field-scene transitions.

`player_data` (PROT 876) is a streaming file - VAB + an empty `TIM_LIST` + a SEQ
trailer - and carries **neither** the character meshes nor the player textures.
Both come from **PROT 0874** instead: §0 is the 5-TMD character mesh pack that
populates `DAT_8007C018[0..4]`, and §2 is the field-character texture pack whose
entries 1/2/3 are the Vahn/Noa/Gala atlas pages at texpage `(832, 256)` with
per-character CLUTs on row 478. See
[`character-mesh.md` § Textures (field form)](../formats/character-mesh.md#textures-field-form)
and [`world-map-overlay.md` § Disc-side source of `[0..4]`](../formats/world-map-overlay.md#disc-side-source-of-04).

(An earlier reading placed the player atlas in PROT 876 at `fb=(768, 0)` with
CLUT `(0, 500)`. That is **falsified** - don't re-derive it.)

`init_data` (PROT 0) holds shared UI / sprite tiles.

The shared blocks are uploaded *first*, so scene-local TIMs win any slot
collision - mirroring the retail boot-then-scene order.

`SceneHost::enter_field_scene` calls `build_targeted` with the field shared blocks by default; the legacy `SceneResources::build` / `build_with_shared` paths remain for tests and engines that want the unfiltered upload for diagnostic purposes.

**Render vs parity: targeted vs DMA-every-TIM.** The two paths want different
things, so both exist.

The targeted upload is a *render* optimisation: it writes only the texture bytes
the current meshes sample, so the prim filter and the uploaded set stay
consistent and CLUT-row collisions don't drop prims. The *retail field loader*,
by contrast, DMAs **every** scene TIM to VRAM regardless of which prim samples
it.

The VRAM **parity oracle** reproduces the live VRAM, not the minimal render set,
so it needs retail's behaviour. `BuildOptions { upload_all_tims: true }` switches
`build_targeted` to `build_vram_full_from_buffers`: every parseable collected TIM
is written to its header destination - images first as sequential DMA, then CLUTs
with merge-zeros to preserve the row-479 palette split.

On town01 this lifts oracle coverage from ~4% (targeted) to ~38% of the runtime
texture region, with wrong (engine-only) texels dropping from ~11.5k to ~250. The
flag defaults `false`, so the render path is unchanged.

The TIM scan walks both raw entry bytes and any LZS-decompressed sections (via `legaia_asset::tim_scan::scan_entry`), so battle / level-up bundles that pack their character TIMs inside an LZS container don't need a raw-byte fallback path.

**Diagnostics.** `legaia-engine info --scene <name> --tmd-stats` reports per-TMD
`kept / miss_clut / depth_mm / miss_page` counts, so regressions in the
targeted-upload pipeline are visible without firing up the windowed viewer.

`--vram-png` / `--vram-bin` write the engine VRAM as a 1024x512 PNG / raw BGR555
blob. `--runtime-vram <bin>` (paired with `mednafen-state vram-dump --out-bin`)
reports per-region pixel-coverage statistics against the runtime ground truth,
and `--vram-diff-png` writes a colour-coded diff: red = runtime has, engine
missing; green = engine extras; blue = both populated but different.

#### Two-pass upload ordering

Inside `build_vram_targeted_from_buffers` the targeted upload runs in two passes:

1. **Image pass** writes every useful TIM image block (image overlaps a mesh's tex page region AND does NOT overlap another mesh's CLUT row).
2. **CLUT pass** writes every useful TIM CLUT block (CLUT overlaps a mesh's CLUT row), unconditionally with respect to image-page collisions.

The ordering matters because of a trap an earlier design fell into. That version
filtered CLUT uploads with a `clut_collides_page` suppression, which dropped
legitimate palette rows whenever *any* mesh's UV bbox happened to brush the CLUT
row's y-coordinate.

The town01 character TMDs hit exactly that: their 256-pixel-wide palette at y=479
overlapped a separate scene mesh's texture-page rectangle, so the CLUT upload was
suppressed and 388 prims dropped as `MissingClut`.

PSX games routinely place palette rows on the bottom of texture pages, so the
collision is normal, not a bug to detect. Image-then-CLUT ordering keeps those
rows coherent with no per-prim heuristic at all.

#### Field static-object placement (town01)

The field static-object table (`FUN_8003A55C`, `legaia_asset::field_objects`)
places 46 environment-pack meshes in town01. Of those, **45 draw** on the
VRAM-textured path and **1 drops** (pack 31 / obj 315) - it is untextured, and
the colour pipeline below picks it up. No placement drops for a missing CLUT.
Pinned by `field_object_placement_disc::town01_dropped_placements_split_untextured_vs_missing_clut`.

Getting there meant resolving two separate root causes. Neither was a
render-filter tweak, and both are worth knowing because the same shapes recur in
other scenes.

**Untextured props take a different pipeline.** A prop whose prims carry no UVs
(flat / gouraud per-vertex-colour primitives) is skipped by the VRAM-textured
mesh builder - correctly, there is nothing to sample.

The per-prim **colour block** is reversed (F4/G3/G4 layouts, the `00 01 03 02`
quad winding remap, and the negative "no per-prim normal" result; see
[`formats/tmd.md`](../formats/tmd.md#per-prim-color--texture-block)), so
`legaia_tmd::mesh::tmd_to_color_mesh` builds a `ColorMesh` from those prims and
the renderer's **vertex-colour pipeline** draws them: `scene_color_mesh_pipeline`,
`Renderer::upload_color_mesh`, `Scene::color_draws` - flat face-shaded, no VRAM
lookup. `play-window` builds a colour mesh whenever the textured build comes back
empty, resolving placement transforms exactly as it does for textured props.

A *mixed* mesh (some textured + some untextured prims) renders **both** halves at
the same placement. The colour mesh is built unconditionally and is disjoint from
the VRAM mesh, because `tmd_to_color_mesh` skips textured groups.

**Some env prims sample a boot-resident page, not a scene TIM.** Pack 74 /
obj 347 is the example: all four of its prims sample the same texture page
`(960, 256)` + CLUT `(64, 510)`, which the `Field` pre-pass's band exclusion plus
`upload_all_tims: true` never fills.

The source is not a runtime targeted upload at all. It is the **boot-resident
system-UI TIM bundle** - `prot::timpack` at raw PROT TOC entry 0 = CDNAME
`init_data`, the pre-extraction head "gap". See
[`formats/npc-palette.md`](../formats/npc-palette.md#boot-resident-strip-band-rows-510511)
for the row layout and evidence.

The atlas TIM at `PROT.DAT[0x11218]` supplies both the `(960,256)` page and, via
the flat-strip CLUT semantics of the per-TIM uploader `FUN_800198E0`, the
256-entry strip on row 510. CBA `(64, 510)` selects strip entries 64..79 - the
declared CLUT bank's sub-row 4.

The same reference pattern recurs in other scenes' env packs: `rikuroa` env slots
50/51/63 alongside town01 slots 21/26/74, all CBA `(64,510)` / tpage `(960,256)`
4bpp. Their UVs sample a small constant mid-grey texel patch (u `0..2`,
v `240..242` → VRAM rows 496..498 of the page) - a flat-material trick that
modulates the prim colour through the textured pipeline.

So the pre-pass uploads the whole bundle. `legaia_asset::system_ui_bundle` parses
raw TOC entries 0/1 (20 + 1 members, including the six bare
`(960, 456..462, 256, 1)` row-patch members that overlay the atlas image) with the
flat-strip CLUT semantics; `SceneResources::build_targeted` underlays it beneath
the scene uploads (boot-then-scene order, scene words win); the web-viewer
full-map path and the VRAM oracles ride the same source.

`vram_oracle_e1` stays byte-exact on the static masks - the row patches are what
the "runtime-overwritten atlas rows" actually were: disc content from the same
pack.

**A falsified rule worth not re-deriving.** An object's mesh id is the placement
record's `+0x10` field (retail `FUN_80020f88`), *not* its position in the pack.
The positional rule `pack = obj_idx - 5` is wrong: it maps obj 114 to the
untextured pack 109, when the record resolves it to the textured pack 84 -
which is what the live battle-scene actor list shows, and why obj 114 draws.

The full retail binding pass - both refresh arms, the kind table, the `0x9C`-byte
render-node allocation and the mesh-chain follow-through - is written up under
[`FUN_80020F88`](../reference/functions/renderer.md#80020f88) and ported as
`legaia_engine_render::actor_bind`, which carries the index rule as a unit test
against the obj-114 case above.

#### CLUT-trace + VRAM-oracle diagnostics

Two `legaia-engine` subcommands surface where the engine's loader still has gaps against a captured runtime VRAM:

**`legaia-engine clut-trace --scene <name> --disc <bin> [--runtime-vram <bin>]`**
walks every dropping `MissingClut` prim, groups by `(cba, depth)`, and reports
which PROT entries on the disc carry a TIM whose CLUT block covers each missing
row.

Coverage is by rectangle containment, because the standard PSX pattern packs 16
distinct 16-entry palettes into one 256-wide row - so a CBA's 16-pixel slot sits
*inside* a wider supplier block.

The `--runtime-vram` cross-check tells the two failure modes apart. "Row absent
from engine but present at runtime" is an engine loader gap. "Row absent from
runtime too" means the mesh references an unreachable CLUT - likely a parser-side
issue, or a CLUT loaded by an unported sub-pack walker.

**`legaia-engine vram-oracle --scene <name> --disc <bin> --runtime-vram <bin> [--diff-png <path>] [--tiles]`**
rebuilds the scene's engine VRAM and reports per-band overlap counts plus an
optional 64x64-tile breakdown.

`--diff-png` writes a 1024x512 colour-coded diff - greyscale = exact match, blue =
both non-zero but different, red = runtime-only, green = engine-only. Same
encoding as `info --vram-diff-png`, exposed as a dedicated comparison surface.

The oracle's standalone VRAM build picks its load kind via `oracle_load_kind`,
mirroring the live `enter_field_scene` choice: world-map scenes (`map\d\d`) build
with `SceneLoadKind::WorldMap` so the kingdom bundle's slot-0 terrain atlas
(opaque to the generic TIM scanner) lands in VRAM. Without that, the oracle
reports the grass/water terrain pages as a phantom gap the engine does not
actually have; the alignment roughly doubles `map01` texpage residency
(`world_map_vram_alignment.rs`).

Both work without any pre-extracted `tim_scan/` tree - they operate straight off `PROT.DAT` + `CDNAME.TXT` (extracted-root or in-place disc image).

### A CLUT row's width says nothing about a prim (retired check)

`Vram::prim_texture_status` used to flag `ClutDepthMismatch` when a 4bpp
prim's CLUT scanline was populated past 256 pixels, reading a wide row as
another TIM's image bytes having spilled onto the palette.

The check is unsound and has been removed. A 4bpp `CBA` addresses **64**
distinct 16-entry palettes on a row (`(cba & 0x3F) * 16` spans `0..=1008`),
so a densely packed row is legitimately populated across all 1024 pixels -
four times the width the threshold called impossible; 8bpp has the same
property with four 256-entry palettes. And the GPU reads only the 16 (or
256) entries at the prim's own `CBA`, so no measurement of the *rest* of the
scanline can decide whether those entries are a palette. `MissingClut`,
which reads exactly that window, is the sound test and still runs.

It was not a theoretical fault. Scenes really do pack rows that densely, and
the threshold then deleted whole buildings: in `town0b` every primitive of
the placed house at tile `(37, 42)` sampled row 491 and was dropped, leaving
a hole through to the clear colour where the house should stand - 17 of the
scene's env-pack meshes were emptied outright. Regression cover:
`crates/engine-core/tests/scene_clut_row_packing_disc.rs`, which asserts
both that the house keeps its prims and that the row really is packed past
the old bound, so it cannot pass vacuously.

The variant and its per-reason counter are retained so the diagnostic
surfaces keep their column; the count is now always zero.

### Texture-window register

`Renderer::set_texture_window(mask_x, mask_y, off_x, off_y)` maps to GP0(0xE2)
"Texture Window setting": four 5-bit values in 8-pixel steps that clamp / wrap
texture-coordinate sampling to a smaller window inside the texture page. The
fragment shader applies `coord = (coord & ~(mask*8)) | ((offset & mask)*8)`
per pixel, before the texture-page lookup.

Default is all-zero (a no-op), and retail Legaia leaves the register at zero
almost everywhere. The API exists so that runtime LoadImage / DMA-to-VRAM trace
work can replay the register state faithfully.

### Full-scene colour grade

`Renderer::set_color_grade(gold, strength)` stages a per-frame `(gold_rgb, strength)` into every
field `MeshUniforms`; the textured / VRAM / colour mesh shaders' `apply_grade` cross-fades each
shaded pixel toward the per-channel multiply `rgb · gold` by `strength` (`strength = 0`, the
default, is a no-op; text/UI overlays use separate shaders and are never graded).

For the opening prologue's gold sepia (`opdeene` / `opstati` / `opurud`) the multiply is
superseded by the **palette-collapse mode** (`Renderer::set_palette_grade`): retail applies the
grade to the *loaded assets* - every uploaded CLUT entry rewritten to
`L = max(r, g, b) -> (L, max(L-1, 0), L >> 1)` and the loaded TMD colour words collapsed to
`gold · max(rgb)` - and the engine's shaders apply the identical law per decoded texel / packet
colour, with runtime-neutral `0x80` words kept neutral and the view-depth cue ramp inert (see
[`cutscene.md`](cutscene.md#full-scene-sepia-grade-the-gold-prologue-look) for the capture that
pins it). The gold coefficients `(1.0, 0.94, 0.43)` are the measured amber-family ratio, stored
as display ratios as-is - see
[Colour space](#colour-space-psx-framebuffer-values-end-to-end). Driven by
[`World::scene_color_grade`](../../crates/engine-core/src/world/narration.rs) (only the prologue
cutscene legs grade); every other scene renders with both grade paths off, bit-identical to the
ungraded pipeline.

Retail's per-render-node depth cue additionally crushes far-field blue (`B/R` down to ~`0.13`).
`Renderer::set_depth_cue_ramp(far, near_z, far_z, max_ir0)` stages that pull as a view-depth
`IR0` ramp (`cue_ramp` in `MeshUniforms`, `cue_ramp_ir0` in the shader prelude): each fragment's
projected view depth maps to `ir0 = clamp((z - near_z) / (far_z - near_z), 0, 1) * max_ir0`, and
the shaders blend toward the far term in retail's order - DPCS runs on the packet colour before
the GPU texel multiply, so a textured prim's far term is `texel * far / 128` and an untextured
prim pulls to the far colour directly. Driven by
[`World::scene_depth_cue`](../../crates/engine-core/src/world/narration.rs) on the same prologue
gate ([`fade::DepthCueRamp`](../../crates/engine-core/src/fade.rs) has the calibration); cleared
(`clear_depth_cue_ramp`) on every other scene, where the ramp-off path is pixel-identical to the
pre-ramp render. See
[`cutscene.md`](cutscene.md#full-scene-sepia-grade-the-gold-prologue-look) for the calibration
measurements and the per-node residual.

The scripted screen fade (field-VM op `0x4C 0x12` global tint - the scene-entry
fade-from-black) reuses this same staging rather than adding a shader term: the host multiplies
the fade tint into the staged grade gold (at full strength) *and* into the depth-cue far colour,
so both branches of the shaders' cue mix carry it and the product distributes to the final
pixel. A neutral tint stages the identity values - byte-identical to the fade-free path. See
[`cutscene.md`](cutscene.md#scripted-screen-fade-op-0x4c-0x12--the-effect-colour-op-0x34-sub-0).

### Colour space: PSX framebuffer values end to end

Every colour in the engine - texels, CLUT entries, vertex colours, menu inks, grade coefficients -
is a **PSX framebuffer value**: display-referred, exactly what the console clocks out to the
display. Nothing on the path converts colour spaces, and nothing may:

- The swapchain is presented through a **UNORM** view (`choose_surface_format`), never sRGB. An
  sRGB attachment treats a shader's output as *linear* and applies the linear→sRGB transfer on
  store, lifting every midtone - retail's mid-grey (5-bit `16` → byte `132`) presents as `190`.
  That is a visible, global wash-out.
- Sampled RGBA textures (the TIM-decoded texture uploads, the font atlas) are `Rgba8Unorm` for the
  same reason: an sRGB *source* would be decoded to linear on sample and then written verbatim into
  the UNORM attachment, darkening instead.
- The last stage of every 3D shader is `psx_dither`, which quantises to 5 bits and expands with
  `(c5 << 3) | (c5 >> 2)`. That quantisation only survives to the screen if the attachment stores
  the byte unmodified.
- PSX semi-transparency (`psx_blend`) blends raw 5-bit values on retail, so the fixed-function
  blend must run in the same display-referred space.

Pinned by `tests::color_space` (engine-render): the attachment is never sRGB for any surface the
adapter might offer, and a known BGR555 texel presents at the byte retail puts on the wire -
checked against an sRGB target too, which lifts it out of tolerance.

### Asset-viewer flat-shaded fallback

`asset-viewer tmd <PATH> --no-textures` (alias `--flat-shaded`) suppresses the VRAM path entirely and renders unlit flat geometry. Useful for inspecting mesh silhouettes without battling palette guesses (the runtime LoadImage trace for field / town scenes is not yet captured, so some palette rows always render as garbage in textured mode).

### `tmd` CLI VRAM diagnostics

`tmd prims <PATH> --vram-dir extracted/tim_scan/<entry>` simulates the targeted upload and adds a per-prim verdict trailer (`-> Ok` / `-> MISSING CLUT (row N)` / `-> DEPTH MISMATCH (row N populated with K entries; prim expects M)` / `-> MISSING TEXTURE PAGE (tpage 0xNN)`). `tmd vram-dump <PATH> -o vram.png [--vram-dir ...] [--annotate]` exports the post-upload software VRAM as a 1024x512 PNG with optional red CLUT-row + green texture-page outlines, so collisions are obvious without firing up the GUI.

## No distance culling: every loaded body is drawn

The engine draws **every** mesh a scene loads, every frame. There is no
frustum cull, no draw-distance heuristic, no per-object radius test, and no
LOD: the field draw lists (`field_placement_draws`, `field_terrain_draws`, the
ground heightfield, the posed props, the NPCs) are resolved once at scene load
and submitted whole on every frame. A town is a few hundred draws of a few
thousand triangles - the budget the port is not on is the PSX's.

The one thing that can still remove geometry is the projection's own clip
volume, so the clip planes are sized to hold an entire scene from any vantage
rather than to frame the current view:

- [`window::SCENE_FAR`](../../crates/engine-render/src/window.rs) = `1e6` for
  every camera. A field map is `256 x 256` tiles of 128 units (~23 k units on
  the diagonal), and the **overworld walk camera composes a 6x world scale**
  onto `psx_camera_mvp`, so eye-space depth there runs to ~140 k. Raising the
  far plane costs no depth precision: the renderer runs **reversed-Z** (see
  the coplanar-surfaces section below), where resolution is proportional to
  view depth at every distance.
- `window::scene_clip_planes(distance)` gives the orbit-family cameras
  (`orbit_camera_mvp`, `world_map_camera_mvp`, `walk_view_camera_mvp`,
  `cutscene_camera_mvp`) a near plane of `distance * 0.005` clamped into
  `[0.05, 8]` - a few units in front of the lens on any scene-sized framing,
  small enough that a wall or floor body the camera sweeps over is never
  clipped, while the asset-viewer's unit-radius TMD previews keep a sub-unit
  plane.

Both are pinned by `camera_tests` in `window.rs`: a full-size field map's
corners must project inside the depth range even though the camera frames only
a small player-sized box, and the near plane must stay within a few units of
the lens at every framing distance the engine uses.

The **site play page** (`site/js/play-app.js`) draws the whole scene every
frame, unconditionally, matching this renderer - `OCCLUDER_CULL = false`. It
once ran a per-frame occlusion cull (drop a body the eye-to-player segment
pierces, since the page has a single follow camera where retail authors one per
scene), but even the exact segment-vs-world-AABB form culled legitimate bodies:
the placement boxes are axis-aligned over whole terrain tiles, walls, and
buildings, so as the camera orbited or the player walked, the lens-to-player
segment swept through a *neighbour's* box and blinked it out. The cull code is
kept for reference but the branch is never taken. Both hosts now solve the
same "wall between lens and player" problem at per-fragment granularity
instead - the
[camera-occlusion fade](#camera-occlusion-fade-see-through-walls-opt-in-enhancement),
which dissolves pixels rather than culling bodies and so has no
neighbour-blink failure mode.

## Coplanar surfaces: retail's ordering model, the port's depth policy

Retail has **no depth buffer**. Every primitive is inserted into the ordering
table by its mean GTE Z and the table is drawn back-to-front, so coplanar
surfaces - which the assets use freely - resolve painter-style:

- a small decal prim on a large base surface usually lands in a **nearer OT
  bucket** than the base (its mean Z is its own local Z; the base's mean Z
  averages a span that reaches deeper), so the decal paints after the base
  and wins;
- prims in the **same** bucket resolve by insertion order (`AddPrim` is a
  head insertion, so the earliest-emitted prim draws last, on top);
- **double-sided prims** - the same triangle authored once per visible side,
  with opposite winding - never conflict at all, because the per-prim NCLIP
  test rasterises only the camera-facing copy.

A depth-tested port turns each of those into per-pixel z-fighting: the two
surfaces interpolate to depths that differ only by float rounding, and the
winner flickers per fragment as the camera moves. The scene data is full of
them - a disc census over `town01` / `rikuroa` / `jou` finds ~200 double-sided
pairs per cave/town environment pack (about half differing per side in UVs,
i.e. visibly), dozens of exactly-coplanar decal prims inside single meshes,
and hundreds of cross-draw pairs where terrain tiles overlap each other or a
placed slab on an identical world plane.

The port resolves each class at the level it occurs, shared by the native
renderer and the site's WebGL viewers:

- **Double-sided pairs** - `legaia_tmd::mesh::mark_double_sided_pairs` (an
  opt-in post-pass the scene-assembly consumers run; preservation/export
  builders stay byte-faithful) flags both copies via bit 15 of the per-vertex
  CBA attribute (unused by the PSX CBA encoding). The fragment shaders then
  discard the away-facing copy of *flagged* prims only - retail's per-prim
  NCLIP, without the unsafe global cull (winding is not globally consistent
  across the corpus). Which facing is "away" depends on the view chain's
  reflection parity: the native field frame and the site's `buildMvp` carry
  one net reflection, the site's assembled views add the retail screen-X
  mirror on top; the WebGL shader takes the parity as the `u_pair_front`
  uniform, the native shader's parity is fixed by its field frame.
- **Intra-mesh coplanar decals** - `legaia_tmd::mesh::separate_coplanar_prims`
  nudges each successive overlapping layer half a unit toward its visible
  side (the negative cross-product side of the emitted winding), ranks
  assigned by greedy graph colouring so chains of edge-adjacent prims
  alternate 0/1 instead of accumulating. Retail art itself authors ~1-unit
  offsets for the decals it separates explicitly, so the nudge stays inside
  authored practice and far below visibility against 128-unit tiles.

  Both passes run over the **hybrid** of a TMD's textured and untextured
  halves as one stream (`legaia_tmd::mesh::resolve_hybrid`): the baked floor
  shadows and wall paintings are untextured `F*`/`G*` colour prims lying on
  textured bases, pairs neither single-mesh pass can see. The kernel merges,
  runs both passes, then splits positions back to their owning half; a
  flagged colour vert carries the pair bit in blend bit 14
  (`BLEND_DOUBLE_SIDED_BIT`) - the colour-mesh shader's facing discard reads
  it natively, and the web merge re-keys it onto the merged CBA bit 15. Both
  hosts call the one kernel (native `assets.rs` static-env + posed-placement
  builds; web `build_hybrid_env_mesh(_posed)`) - running the passes on the
  textured half alone left every colour-on-textured decal z-fighting in the
  native window while the browser was clean.
- **Cross-draw overlaps** - `legaia_engine_core::coplanar_draws` detects the
  coplanar overlap clusters across a scene's resolved `EnvDraw` list (terrain
  + placed layers combined) and returns a per-draw world offset: the largest
  surface in a cluster stays put, each overlapping smaller/later draw lifts
  `DRAW_NUDGE` per rank toward the surface's visible side - the same "small
  decal wins" outcome retail's mean-Z bucketing produces. All three hosts
  that assemble field scenes apply the same map: the native play-window in
  its placement/posed-prop resolvers, the browser play page and the
  field-scene viewer in their placement/terrain position exporters (the
  drift gate's sim-pair rows pin all three call sites).

  Five properties of the kernel each close a failure mode that shipped as a
  visible, view-angle-dependent fight:

  - Conflicts are partitioned into **normal families** (clustered by angle,
    never by quantizing the normal into a hash key) and each family gets its
    own colouring and its own lift component; the components sum. One rank
    per draw resolves only the largest conflict - a draw lifted along Z for
    its floor tie keeps fighting a wall on X.
  - `DRAW_NUDGE = 0.75` is deliberately **not commensurate** with either the
    intra-mesh `0.5` nudge or retail's ~1-unit authored decal lattice: a
    whole-draw lift of exactly `1.0` maps a mesh's authored offset layer
    straight onto the neighbouring draw's base plane, creating the very
    coincidence the pass removes.
  - Rank capacity (16) exceeds the largest mutual-overlap clique in the
    corpus - a low clamp parks the overflow draws on one shared lift
    (taiku's plaza stacks six-plus instances of one slab on a single plane).
  - Plane *distance* is always measured point-through-plane, never `d - d`:
    two matching sloped clusters carry representative normals that differ by
    float noise, and at world coordinates that noise scales into tens of
    units of spurious `d` difference (the bucket walk computes its distance
    key against the bucket's own quantized normal for the same reason).
  - A final **repair pass** re-measures every detected pair under the summed
    offsets and bumps any still-coincident pair apart - per-family lifts of
    a pair that conflicts in many families at once (two shells stacked at
    one position touch on every family) can otherwise sum back into
    coincidence.

- **The walk-ground heightfield** - the `.MAP` walkable base grid is a
  *fourth* surface layer, and indoors/in towns the env pack lays authored
  floor art on the very same plane (koin6: the entire 972-triangle ground
  grid and the env floor slabs all at `y = 0`) with a different
  tessellation - so every such floor z-fights as wedge streaks along the
  grid's cell diagonals, most visible from steep pulled-back cameras.
  Retail resolves the tie painter-style; the port sinks the ground layer
  `coplanar_draws::GROUND_SINK` (`0.4`) units below its authored height at
  every **render** site (native `heightfield_to_vram_mesh`, the play page /
  field-scene viewer / kingdom-walk position exporters, the dance stage),
  so the authored art wins deterministically and the ground still draws
  wherever nothing covers it. The heightfield struct itself keeps its
  authored heights - the `.glb` exporter and the fishing shore-anchor
  height queries must not shift.

  The disc-gated oracle for all of this is
  `engine-core/tests/coplanar_residual_disc.rs`: it rebuilds a scene's final
  world-space triangle soup exactly as the hosts draw it (hybrid passes,
  yaw-instanced draws, lifts applied, ground layer sunk) and scans for
  coplanar overlapping pairs that survive - `DIAG_ALL=1` sweeps the whole
  field corpus. The known residual tail it tolerates: same-position stacks
  of *curved* shells (jouine's flesh walls), which no translation offset can
  separate everywhere, and sub-cluster slivers below the detection floor.
- **Depth precision** - the native renderer runs **reversed-Z**
  (`renderer/helpers.rs::reverse_z`: clip `z' = w - z`, depth cleared to 0,
  compares mirrored to Greater/GreaterEqual, applied centrally at the two MVP
  upload sites so every camera inherits it). With the `Depth32Float` target
  this keeps depth resolution proportional to view depth (`~z * 2^-23`), so
  the sub-unit nudges above - and the assets' own ~1-unit authored offsets -
  resolve at every camera distance up to `SCENE_FAR`. The clip `w` row is
  untouched, so CPU blend-order keys and the shaders' `clip_pos.w` depth-cue
  reads are unaffected. The WebGL side keeps a fixed-function 24-bit buffer
  (no clip-control on WebGL2), so its single-mesh projection instead scales
  the near plane with the framing distance (`buildMvp`), which restores the
  same sub-unit resolution at real mesh scale.

## Render kernels by surface

The port draws through **five** surfaces, not two, and every one of them
assembles its own draw list before a shader ever runs. Two of the five are
easy to forget because they live in the minigames page rather than in
something called a renderer, and both resolve `EnvDraw`s and instance
env-pack meshes exactly like the field hosts do.

| # | Surface | Draw-list assembly |
|---|---|---|
| 1 | native `play-window` | `engine-shell` `window/field_render.rs` + `window/geometry.rs` |
| 2 | browser play page | `web-viewer` `play.rs` + `play_battle_render.rs` |
| 3 | browser field-scene viewer | `web-viewer` `field_scene.rs`; `scene_geom.rs` for the world map |
| 4 | browser dance-hall venue | `web-viewer` `minigames_dance.rs` |
| 5 | browser fishing venue | `web-viewer` `minigames_fishing_scene.rs` |

A kernel wired into one and not another is invisible in a diff, because no
file holds two of the columns. The gate that measures this is host-drift
[tier 7](../tooling/host-drift.md#tier-7---render-kernels-same-draw-list-same-kernel-every-surface);
what follows is the shape of the answer it measures, not a status table.

### The structural split: what a browser surface *can* share

`engine-render` links wgpu, so `web-viewer` cannot depend on it. Every kernel
that lives there - `psx_dither`, `psx_blend`, `dyn_light`, `scene_lights`,
`occlusion_fade`, `billboard`, `streak_pass` - is native-only **as code**, and
a browser twin has to be a second implementation in GLSL or JS. Kernels that
live in `engine-core`, `engine-vm`, `engine-ui` or `legaia-tmd` are shared as
one implementation and are the ones tier 7 can pair by name - which is why
`gte`, `vram_capture` and the `battle_intro` emitter migrated to `engine-ui`
(re-exported at their old `engine-render` paths): they were ordinary
arithmetic that happened to live beside wgpu, and moving them is what let the
play page draw the transition style bodies at all.

That is why the two halves of the render law are policed differently: the
draw-list half is one kernel both hosts call, so a checker can ask "does this
surface call it"; the fragment half is one law expressed twice in two shading
languages, and only a rendered frame from each host can compare those (see
[host-drift.md](../tooling/host-drift.md#the-two-hosts-do-not-share-a-shading-law)).

### Kernels that are native-only by intent

Not every asymmetry is a defect. Three are deliberate:

- **`set_psx_mode`** (vertex snap + 15-bit dither) is opt-in and defaults off,
  so a browser with no toggle renders the same *default* the native window
  does. What the browser lacks is the switch, not the faithful output.
- **`set_dynamic_lighting`** and its shadow sub-layer are an enhancement,
  default off, pixel-identical when off.
- **`LEGAIA_DIAG_*`** bisect gates are development instruments; the
  [tier 6](../tooling/host-drift.md#tier-6---diagnostics-is-a-debug-draw-off-on-both-hosts)
  rule is only that an *additive* one needs a default-off twin.

### Placement rotation is three angles on every surface

A `.MAP` object record carries three authored angles (`+0x08` pitch, `+0x0A`
yaw, `+0x0C` roll) and retail composes all three in `Rx * Ry * Rz`
(`FUN_80026988`; the port's pinned copy is
`engine_ui::battle_intro::placement_rotation`, and the browser's is
`placementModelEuler` in `site/js/webgl-math.js`).

The yaw-only builders (`placementModelScaled*`) take a **negated** yaw,
because their inline `Ry` is transposed and the two negations cancel. That
cancellation is specific to `Ry`: a tilted placement cannot go through them at
all, it has to be handed a whole model matrix. Which is why "most placements
are pure yaw" is a performance note and never a licence to drop the other two
angles.

The corpus, measured across 49 field scenes: **94 of 1667 placements carry a
nonzero `rot_x` or `rot_z`**, and the distribution is lumpy rather than
uniform - `juui1` tilts all nine of its by a quarter turn about X, `vozz` 31 of
119, `jouina` 21 of 103, `koin3` 17 of 63. Terrain-layer cells tilt too, and
more often than the placed layer: `retona` composes 56 tilted draws in total
against 3 tilted *placements*. A surface that reads only the yaw does not lose
a curiosity; on those scenes it loses the scene.

## Rendering knobs: what is faithful, what is a choice

**Simulation is faithful, with no opt-out. Shading defaults to retail;
rasterisation defaults to clean.**

That is the whole story, and it is worth being precise, because "faithful" and
"default" are not the same axis. Three render toggles exist, each with a
different default:

| Knob | Default | Off/on is retail? | Gates |
|---|---|---|---|
| `Renderer::set_psx_mode` | **off** | *on* is retail | vertex snap + 15-bit dither, and nothing else |
| `Renderer::set_semi_blend` | **on** | *on* is retail | ABE semi-transparency. Independent of `psx_mode` |
| `Renderer::set_dynamic_lighting` | **off** | *off* is retail, pixel-identical to the faithful render | the opt-in soft-light enhancement |
| `Renderer::set_dyn_shadows` | **on** | inert while `set_dynamic_lighting` is off | the point-light + shadow sub-layer of dynamic lighting |
| `Renderer::set_occlusion_fade` | **off** in the renderer, **on** in `play-window` | *off* is retail, pixel-identical to the faithful render | the see-through-walls camera-occlusion fade |

Two things routinely get mis-stated about this table, so they are worth saying
plainly:

**Lighting is not a `psx_mode` knob, and the default is already faithful.** The
game's field/town meshes go through the VRAM-mesh and vertex-colour pipelines,
which draw the TMD's baked colour words with no light source at all - exactly
what retail does (see [Lighting](#lighting)). There is no synthetic Lambert on
those paths. The only non-retail light is
[dynamic lighting](#dynamic-lighting-opt-in-enhancement), which is off by
default.

**Affine UVs are not gated either - they are always on.** `@interpolate(linear)`
is a static qualifier on the vertex-output struct, not a uniform-driven branch,
so every path interpolates UVs affinely on every frame. `psx_mode` produces
exactly one value, `snap`, which drives the vertex snap and is shared as the
dither enable.

### `set_psx_mode` - vertex snap + dither

`Renderer::set_psx_mode(true)` enables the two strict-PS1 rasterisation artefacts
that are *not* on by default. Default is off; in `legaia-engine play-window`, opt
in with `LEGAIA_PSX_RENDER=1`.

- **Sub-pixel vertex snap ("vertex jitter").** Clip-space `x` / `y` are snapped to integer pixel positions inside the vertex shader (NDC → pixel grid → NDC round-trip). Reproduces the GTE's per-vertex sub-pixel-truncation jitter that gives PSX rendering its characteristic shimmer on slowly-moving geometry.
- **15-bit ordered dithering.** When packing the 24-bit shaded colour into the 15-bit (BGR555) framebuffer, the PSX GPU adds a signed 4x4 ordered-dither offset per pixel before truncating each channel to 5 bits. The shader helper `PSX_DITHER_WGSL` (prepended to every shaded 3D shader) reproduces it and mirrors the unit-tested CPU `psx_dither` module; the composed shader sources are naga-validated in the engine-render test suite (the GPU-free guard that the WGSL stays well-formed).

#### Retail's dither law, stated separately from the port's default

The two are different claims and get confused, so they are written apart here.

**Retail law: dither is on at boot and script-controlled.** The GPU's `dtd` bit
lives in the DRAWENV byte at `+0x2A` of each of the two draw environments that
the frame-begin driver swaps. Four sites, all read off the disassembly:

| Site | Instruction | Effect |
|---|---|---|
| `0x8002004C` | `sb zero, 0x2a(a0)` | DRAWENV pair initialiser (`FUN_80020038`) stamps `dtd = 0` |
| `0x80017208` / `0x80017210` | `lbu v1, -0x459a(v1)` / `sb v1, 0x2a(v0)` | frame-begin driver `FUN_80016B6C` **re-stamps** `dtd` from `_DAT_8007BA66` every frame, indexing the pair by `gp+0x434` at stride `0x74` |
| `0x8001D520` | `sh s2, -0x459a(at)`, `s2 = 1` | boot (`FUN_8001D424`) writes `1` to `_DAT_8007BA66` |
| `0x801E350C` | `lbu v1, 0x1(s6)` / `sh v1, -0x459a(v0)` | field-VM opcode takes a one-byte script operand into `_DAT_8007BA66`, then advances the VM PC by 3 |

So the initialiser's `dtd = 0` never survives a frame: the per-frame refresh
overwrites it from the global, the global boots at `1`, and a scene script can
flip it at will. `FUN_80026CE4` reads the same global as an `lh` and passes it
to the mode-`0x15` STR packet submit, so the FMV blit path honours the same bit.

**Port default: off, by choice.** `Renderer::set_psx_mode` gates the engine's
dither, and it is opt-in. That is a project decision about the default look, not
a reading of the executable - the retail bit above is what the toggle reproduces
when you turn it on.

### Affine UV interpolation (always on)

Per-vertex UVs interpolate linearly in screen space, with no perspective-correct
division, on every path in every mode. This reproduces the texture warping you
see on retail surfaces with steep depth gradients: the GP0(0x24)-class triangle
commands transmit only `(u, v)` per vertex, and the rasteriser does not divide by
`1/w`. WGSL `@interpolate(linear)` gives the same behaviour.

The baked per-prim colour carries the same qualifier, for the same reason - PSX
gouraud interpolation is affine in screen space too.

Texture page (`tsb`) and CLUT base address (`cba`) stay `@interpolate(flat)` - they are per-primitive in retail because GP0(0x24) sets them once per draw call, not per vertex.

### Dynamic lighting (opt-in enhancement)

`Renderer::set_dynamic_lighting` is the one lighting knob, staged into
`MeshUniforms.light_dir[3]`. It is **off by default, and off is retail**: the
disabled path is pixel-identical to the faithful baked-shading render, so the
parity oracles are unaffected.

Enabled, the VRAM / colour mesh shaders layer a soft warm directional light (off
the smoothed per-vertex normals, with a screen-space-derivative fallback for the
normal-less colour-mesh prims) plus a screen-centred light pool over the baked
colours, with the global gain capped at ~1.3x (and ~1.9x once the point-light
layer below adds on top). This is explicitly a non-retail enhancement - retail's
field path has no light source. See `crates/engine-render/src/dyn_light.rs` and
the `DYN_*` tunables in `renderer/state.rs`.

### Per-scene point lights + shadows (sub-toggle)

`Renderer::set_dyn_shadows` (default on; `--no-dyn-shadows` / the `Y` key in
`play-window`) gates the enhancement's second layer: real point lights at the
scene's own light-emitting props, each with a shadow map. It only acts while
dynamic lighting is on and the host has staged lights, so the faithful path
pays one uniform read and nothing else.

**Light derivation** (`crates/engine-render/src/scene_lights.rs`). What the
player reads as candles and wall lights in retail interiors is emissive-looking
geometry - small additive-blended (ABE, ABR mode 1) flame prims and
bright-modulated lamp meshes. The derivation reads that authoring signal back
out of the mesh data: a triangle is an emitter sample when it is an additive
semi prim, or an ABE prim whose baked colour is bright and warm
(`EMIT_MIN_BRIGHT`, red >= blue - excludes blue water/glass); triangles above
`EMIT_MAX_TRI_AREA` are rejected as sheets. Every *placement instance* of an
emitting mesh contributes its samples in world space; greedy clustering
(`CLUSTER_MERGE_DIST`, wide clusters dropped at `CLUSTER_MAX_EXTENT`) yields at
most `MAX_SCENE_LIGHTS` (8) lights, position/colour/radius weighted by
`sqrt(area) * luminance`. The play-window derives lights from the field
placement + terrain layers at scene load (`upload_assets`) and stages them per
frame with the camera (`Renderer::set_scene_lights`) so the shaders can recover
world space from each draw's MVP (`model = view_proj^-1 * mvp`, carried in
`MeshUniforms.model`).

**Shadows.** Per light, a depth-only pass renders the scene's draws into one
layer of an 8-layer `Depth32Float` array (512x512 per light) from a downward
cone (`scene_lights::light_view_proj` - fov ~120 deg, near plane clipping out
the emitting flame quad itself so it never occludes its own light). The scene
fragment shaders (`scene_point_gain` in the group-2-bound scene shader
variants) apply distance attenuation `(1 - (d/r)^2)^2`, the same
mixed-winding `|N.L|` term as the global light, and a 3x3 PCF comparison
(`textureSampleCompareLevel`) for soft shadow edges; fragments outside a
light's cone shade unshadowed. Known approximations, on purpose: casters
render opaque in the shadow pass (cutout texels shadow as solid), and the
downward cone is a spot approximation of a point source - geometry above the
light attenuates but is never shadowed.

The single-mesh (non-scene) pipelines compile a zero-stub `scene_point_gain`,
so only the scene pipelines carry the lights bind group; both shader variants
are naga-validated by the engine-render test suite, and
`scene_lights::point_gain` / `point_attenuation` are the CPU mirror of the
analytic part, asserted in lockstep with the WGSL.

### Camera-occlusion fade (see-through walls, opt-in enhancement)

`Renderer::set_occlusion_fade` gates the port's answer to a problem retail
solves with authored camera placement: the engine's one field follow camera
regularly puts walls, roofs and cliff faces between the lens and the player.
When the fade is on and the host stages a focus
(`Renderer::set_occlusion_focus` - the player's clip-space position under the
frame's scene camera), scene fragments that would bury the character dissolve
to a screen-door dither so the player always reads through. The renderer
default is **off, and off is retail** - the disabled path is pixel-identical
and the parity oracles never see it; `legaia-engine play-window` turns it on
by default as a clearly-better play experience (`--no-occlusion-fade`, or the
`D` key, restores authored-framing-only).

The fade has two halves: a per-frame **visibility gate** deciding WHETHER to
fade at all, and a per-fragment rule deciding WHICH pixels dissolve.

**The visibility gate** (`legaia_engine_core::field_occlusion`, one kernel
shared by both hosts - the native window calls it directly, the browser play
page through the wasm export `field_player_occluded`): a CPU ray-cast of a
5-point body cross (centre, head, hips, both shoulders) from the camera eye
to the character, against the scene's **static occluder set**, in retail
Y-down world coordinates. The fade arms only when **every** sample is
blocked - a partially visible character gets no fade at all, so geometry
merely near the eye-player corridor (an upper-tier floor beside a tower pit)
never dithers while the player is plainly on screen. The occluder set is
built per scene load from the resolved terrain + placement draw lists, and
it deliberately excludes everything that draws see-through: **semi-
transparent (ABE) prims** (the keikoku canyon's corridor-spanning mist veils
armed the gate while the character was visible), prims dropped by the
renderers' own VRAM-coverage filter, prims below the
`OCCLUDER_MIN_OPACITY` texel floor (cutout foliage / grates / tile skirts),
and animated draws. The native eye is the follow camera's analytic position
(`field_follow_camera_eye`, the exact inverse of the camera composition -
its clip `w` under the frame's matrix is 0); the web passes its orbit
camera's `_eye()` with the draw-frame Y negated. The gate verdict drives an
eased **strength ramp** (a quarter of the gap per frame on both hosts), so
the screen-door dissolves in and out instead of popping at cover edges.
`keikoku_corridor_walk_does_not_arm_the_gate` (disc-gated) pins the mist-
veil regression.

**The per-fragment rule** (`occl_keep` in the scene-lights WGSL layer; CPU
lockstep mirror + tunables in `crates/engine-render/src/occlusion_fade.rs`).
A fragment fades only when all three hold:

- its draw is **environment geometry**: the host stages a per-frame draw
  watermark (`Renderer::set_occlusion_env_draws` -> `MeshUniforms.flags[2]`)
  splitting the scene lists into environment (terrain / placements / posed
  props - fadeable) and actors (the player's own halves, NPCs - never
  faded), so the depth test below does not need slack for the player mesh;
- it is **nearer the camera than the player** by more than the depth margin
  (16 view units - just enough to shield the floor tier and coplanar decals
  at the focus depth; because actors are exempted per draw, an occluder
  hugging the character - a plant stalk, an inner wall - still opens up,
  and stacked occluders all open at once since the Bayer pattern is
  screen-aligned across layers); and
- it lies within the **fade circle** around the player's projected centre,
  so only the patch of wall near the character opens up.

The circle is sized in **world units** (`OCCL_RADIUS_WORLD` = 250, about two
character heights) and projected to pixels per frame at the focus's own view
depth (`occlusion_fade::radius_px`, JS twin `occlRadiusPx`), so the hole
covers the same amount of world - and so the same share of the character - at
every camera distance. Across the play page's whole zoom range that holds it
at ~1.9x the character's on-screen height. The projection needs the frame
camera's vertical scale `P[1][1]`, which each host recovers from the
view-projection it already holds: `view_proj = P * V` with `V` rigid, so the
product's second row is `P[1][1]` times a unit axis and its length is the
scale (`view_proj_scale_y` / `occlProjScaleY`).

A fraction of the *viewport* is the obvious alternative and it is wrong -
being screen-relative it is zoom-invariant by construction, so no single
value serves two framings. The knob was retuned twice under that model
before the model itself was replaced: at `0.30` a character standing ~12.5%
of the viewport height got a hole ~4.8x their height (~31x their screen
area), reading as the scene dissolving; retuned to `0.12` it framed that one
distance well and then collapsed to a peephole around the character's head
as the camera pushed in, because the character grows on screen and a screen
fraction does not.

The radius clamps (`OCCL_RADIUS_MIN_FRAC` 0.04, `OCCL_RADIUS_MAX_FRAC` 0.9 of
the viewport height) are guards on that `1/z`, not tuning. The upper one is
deliberately far above anything the follow camera reaches - the tightest
play-page zoom projects to ~0.57 - because a clamp set near the working range
silently caps the close-up hole and re-introduces exactly the zoom dependence
the world-space radius exists to remove.

The fade itself is a **screen-door discard** against a 4x4 Bayer threshold
(`occl_bayer` in the shader prelude): keep probability ramps from 1.0 at the
circle's rim down to 0.25 at the centre over a feather band, then blends
toward the identity by the gate's strength. Discarding in the opaque pass
(and identically in the semi-blend entries) means no new pipelines, no blend
state, no CPU sorting, and depth writes stay intact - and the dither pattern
reads naturally next to the PSX ordered dither. The focus + strength ride in
the per-frame group-2 scene uniform (`SceneLightsUniform`, which is staged
every scene frame whether or not the point-light layer is live), so the
faithful path pays one uniform read; the single-mesh pipelines compile a
`return 1.0` stub, exactly like `scene_point_gain`.

**Per-fragment on purpose.** The site play page once shipped this feature at
per-*body* granularity (drop any placement whose AABB the eye-to-player
segment pierces) and had to disable it - see the `OCCLUDER_CULL` history
above. Placement boxes span whole terrain tiles and buildings, so neighbours
blinked out as the camera orbited. The fragment test has no such failure
mode: nothing is ever culled, geometry only loses individual pixels inside
the fade circle.

**The browser play page ships the same fade** (default-on, the
"See-through walls" checkbox): `occl_keep` / `occl_bayer` GLSL twins in
`site/js/webgl-shaders.js` (tunables mirrored at the top of that file -
keep them in lockstep with `occlusion_fade.rs`), staged through
`TmdRenderer.setOcclusionFocus(world_pos, strength)` - `renderAssembled`
projects the focus with the same view-projection it builds for the scene
draws, so the page never duplicates camera math - and the per-draw actor
exemption rides the `u_occl_allow` uniform (`noOccl` on the player / NPC
placements). `play-app.js` runs the same gate through the runtime's
`field_player_occluded` export (falling back to always-armed on a stale
wasm bundle) and clears the focus for battle and VR first-person (where
the eye *is* the player). The other WebGL pages never stage a focus, so
they are untouched.

The host stages the focus in **field free-roam only** - the player's floor
tier (the same sampler the follow camera anchors to) lifted half a character
height - and clears it for cutscenes (an occluded player there is a
directorial choice), battle, world map and the boot UI. Replays force the
fade off alongside dynamic lighting, keeping recorded sessions on the
faithful render.

**The viewer-only exception, so nobody re-derives the wrong conclusion.** There
*is* a fixed directional light with a `max(dot(n, l), 0.0)` diffuse term in the
shader set - but it lives only in `MESH_SHADER_SRC`, the bare-geometry **preview**
pipeline behind the asset-viewer's raw-TMD view. Those meshes carry neither
texture nor colour, so there is nothing of the game's own shading to show; the
light is a viewer aid, not a claim about retail, and no game path uses it.

### `set_semi_blend` - semi-transparency blend modes

PSX per-prim blending on the VRAM-mesh (textured) and colour-mesh (untextured)
paths, staged into `MeshUniforms.flags[1]`.

**This is independent of `psx_mode`, and it is on by default.** Retail's GPU
always blends ABE prims, so field water (e.g. the Hunter's Spring fountain),
glass and additive effects composite correctly in the clean "enhanced" render
too; only the strict-PS1 artefacts (vertex jitter / affine UVs / 15-bit dither)
ride `psx_mode`. Turning it off draws every ABE prim fully opaque - the
harsh-edged "solid water" look.

**Which prims blend, and how.** A prim is semi-transparent when its packet ABE
bit is set (the TMD group mode byte's bit 1). The `legaia_tmd::mesh` builders
pack that bit into bit 15 of the per-vertex TSB attribute (unused by the TMD TSB
encoding), so it reaches the shader with no new vertex format.

The blend equation comes from texpage ABR (TSB bits 5..=6): mode 0
`0.5*B + 0.5*F`, 1 `B + F`, 2 `B - F`, 3 `B + 0.25*F` (`B` = framebuffer,
`F` = texel / prim colour).

**Textured prims: the choice is per *texel*.** BGR555 bit 15 (STP) set blends,
clear draws opaque, `0x0000` never draws, `0x8000` blends black.

With one fixed blend state per pipeline, that per-texel split needs two passes:
the opaque pass draws every triangle and discards STP texels of semi-transparent
prims; a blend pass then re-draws only the semi-transparent triangles (a
per-ABR-mode index tail appended at upload time), discarding everything except
STP texels.

**Untextured (`F*`/`G*`) prims have no per-texel STP gate** - an ABE prim blends
**all** its pixels. The colour-mesh vertex format carries a per-vertex blend word
(ABE bit 15 + ABR bits 5..=6, `psx_blend::pack_blend_word` - the same packing the
textured path rides on TSB) via `Renderer::upload_color_mesh_blended`.

With semi-blend on, the opaque colour pass discards ABE prims entirely and a
per-ABR-mode blend pass (same index-tail scheme,
`psx_blend::append_semi_tail_words`) re-draws them with the prim colour as `F`.
Untextured TMD prims carry no texpage of their own, so ABR comes from whatever
draw-env state the caller resolves - mode 0 is the PSX draw-env default.
`upload_color_mesh` without blend words keeps every prim opaque.

**Pipelines.** One blend pipeline per mode, per path: mode 0 via blend constant
0.5, mode 2 via reverse-subtract, mode 3 pre-scales `F` by 0.25 in its fragment
entry point. Blend draws depth-test `LessEqual` without writing depth, and run
after all opaque scene draws.

The blend pass skips the dither stage. Retail dithers the post-blend value during
the VRAM write, which a fixed-function blend cannot reproduce without a
destination read-back.

The `psx_blend` module holds the pure mapping - ABR extraction, blend-word
packing, blend-state selection, index partition, ordering list, and the CPU
reference `blend_apply` - unit-tested against the PSX equations.

**Ordering is per *primitive*, mirroring the retail ordering table.** Each semi
prim's depth key is its model-space centroid's clip-space `w` under the draw MVP
(`psx_blend::prim_depth_key`) - equal, by MVP linearity, to the average of its
vertices' clip `w`, which is the GTE avg-Z the OT bins on.

All semi prims across all of a scene's draws (textured + untextured in one list)
blend far-to-near regardless of draw boundaries, so depth-interleaved prims from
overlapping draws blend in correct global order.

Equal keys form one OT bucket and draw later-submitted-first - the retail LIFO
bucket order (`AddPrim` prepends to a bucket's linked list, `DrawOTag` walks it
head-first). Per-prim metadata (`psx_blend::SemiPrim`) is recorded once at mesh
upload; the per-frame ordering list reuses one renderer-owned buffer, and
contiguous same-draw, same-mode tail runs coalesce into single indexed draws
(`psx_blend::coalesce_sorted`).

## GTE math module

A fixed-point GTE math module at `crates/engine-render/src/gte.rs` mirrors the
retail accumulator shape: q3.12 rotation matrices, q19.12 translation vectors,
i64-widened multiply-add to absorb three-term sums without overflow.

It also exposes the GTE's higher-level primitives: a `Camera` bundle that runs
`RTPT` (rotate-translate-perspective) end-to-end with PSX-correct saturation on
behind-camera vertices, `nclip` for back-face rejection, `avsz3` / `avsz4` for
OT-bucket selection, and a small CPU rasterizer scaffold
(`raster::rasterize_triangle`, top-left fill rule, integer-pixel bounding-box
iterator) that downstream tooling uses to validate captured traces.

Production rendering still uses f32 wgpu math. This module is the single citation
point for code that needs to reproduce per-vertex GTE behaviour: effect spawners,
hit-detection, animation re-targeting, offline regression checks.

**Perspective divide (UNR reciprocal).** The projection step is not an exact
`OFX + (H * IR1) / SZ3`. The GTE approximates `1 / SZ3` with an Unsigned
Newton-Raphson step seeded from a 257-entry table, then applies it as
`OFX + (IR1 * (H / SZ3)) >> 16` with an arithmetic (floor) shift. Two hardware
quirks follow and are reproduced by `gte::math::gte_divide`, used by the GTE
emulation sites `Gte::rtps` (the register-level cop2 oracle) and its
`Camera::transform` RTPT shim: near or behind the camera (`2 * SZ3 <= H`,
including `SZ3 == 0`) the quotient saturates to `0x1FFFF` and the divide-overflow
FLAG bit (17) is set instead of dividing; elsewhere the reciprocal diverges from
an exact divide by ±1 (up to a couple of units for extreme numerators near the
overflow boundary). The seed table is *computed* from the published algorithm
(no$psx "GTE Division Inaccuracy"), not copied Sony data — the same clean-room
provenance class as the SPU Gaussian / reverb tables. These sites hold MAC/IR in
q19.12 (4096× the hardware IR/SZ scale) and reduce with a `>>12` before the
divide.

A behind-camera vertex is not a special case: `SZ3` clamps to `0` (raising the
SZ3/OTZ FLAG bit on the FIFO push) and `gte_divide(H, 0)` overflows to `0x1FFFF`
exactly as above, so the projection flows through the one path and sets
`DIVIDE_OVERFLOW` — never the MAC3-negative-overflow bit, which is reserved for a
genuine 44-bit MAC3 overflow. The port adds `OFX`/`OFY` as an integer pixel value
*after* the `>>16` floor-shift, whereas hardware adds the fixed-point control
word *before* it. These are bit-identical because retail writes the offsets via
`SetGeomOffset` (`FUN_8005B7F8`: `sll a0,a0,0x10` then `ctc2` to cop2 control 24 /
25, `OFX = (width/2) << 16`), so the low 16 bits are always zero and the two
orderings agree for every numerator sign.

This UNR path is the faithful GTE-register behaviour that the parity oracles
measure; it does not change on-screen rendering, which projects through the
clean f32 pipeline (the field's modern `perspective_rh`, the battle GTE
projection, and `project_billboard`'s effect quads all use the exact divide).
The `gte_divide` primitive is the ready hook should a future faithful-render
mode gate the GTE projection under `Renderer::set_psx_mode`.

The full `Gte::rtps` projection is cross-checked against an independent
second implementation of `gte_rtps_internal` (in the hardware register scale,
with the reference's OFX-before-shift ordering) over a wide input sweep, at
zero tolerance on the projection outputs. Because `Gte` keeps MAC/IR in q19.12,
only the hardware-scale outputs are register-comparable — the **SXY** FIFO, the
**SZ** FIFO, and the FLAG bits that do not depend on the MAC/IR scale
(`DIVIDE_OVERFLOW`, `SZ3_OTZ`, `SX2`/`SY2` saturation). The IR/MAC-saturation
bits (and thus the `ANY_ERROR` roll-up) diverge by that scale convention.

That same comparable subset is also checked against a **real cop2 register
file** by the env-gated `rtpt_matches_recomp_cop2_capture` oracle, which replays
RTPT input tuples captured from a Beetle-validated static recompilation of the
retail game through `Gte::rtpt` and asserts bit-exact SXY/SZ/flag-subset (the
capture holds game-derived bytes, so it is supplied out-of-tree via
`LEGAIA_RECOMP_GTE_CAPTURE` and skip-passes when unset). This is what pinned the
SXY-FIFO saturation bound: the GTE clamps the stored screen coordinate to signed
11 bits `[-0x400, 0x3FF]` (raising `SX2`/`SY2`), matching the PSX GPU's drawing
range — **not** the i16 IR-numerator range. The distinction only shows off-screen,
so the self-consistent in-repo sweep (which shared the earlier i16 assumption on
both sides) could not surface it; the real-cop2 capture did.

### Statically-linked libgte residue (retail side)

Retail's render paths issue their COP2 ops **inline**: the TMD renderer
(`FUN_8002735C`), the cluster-A per-prim dispatcher (`FUN_80043390`), and the
world-map handlers all embed raw `cop2` instructions rather than calling
per-op wrappers. The libgte per-op wrapper family the link carries anyway
(`MulMatrix0`, `Square12/0`, `AverageZ3/4`, `OuterProduct12/0`,
`DpqColorLight`/`DpqColor3`/`Intpl`, the `RotTransPers3`-shaped RTPT
projector, and the staging loaders) has **no static caller** in `SCUS_942.54`
and no hit in any runtime hot profile - library link residue, not a render
seam. The full per-address table lives in
[`reference/functions.md` § libgte primitives](../reference/functions/runtime-libs.md#libgte-primitives);
the family is ignore-listed in the port catalog.

### GTE register-state emulator

`Gte` is a register-level cop2 emulator next to the math module, mirroring the PSX hardware register file: V0..V2 input vectors, MAC0..MAC3 wide accumulators (i64), IR0..IR3 saturating shorts, the SXY (3-deep) / SZ (4-deep) / RGB (3-deep) FIFOs, OTZ, and the FLAG sticky-saturation register with hardware-matching bit positions exposed via `gte::flag_bits` (engines comparing against captured FLAG dumps mask the same bits). Control registers cover the rotation matrix, translation, focal length `H`, screen offset `OFX/OFY`, the average-Z scale factors `ZSF3` / `ZSF4`, the depth-cue interpolation slope/intercept `DQA` / `DQB`, the light source matrix `L`, the light color matrix, and the `back_color` / `far_color` triplets used by the depth-cue pipeline.

Instructions covered:

| Mnemonic | Purpose |
|---|---|
| `RTPS` / `RTPT` | Rotate-translate-perspective (single / triple vertex). |
| `NCLIP` | Signed area of the SXY-FIFO triangle (back-face cull). |
| `AVSZ3` / `AVSZ4` | OT-bucket selection from the SZ FIFO. |
| `MVMVA` | Generic matrix × vector + translation, with shift-frac and lower-clamp flags. |
| `NCDS` / `NCDT` | Normal-color depth shading (single / triple vertex). |
| `DCPL` | Depth-cued primary-color blend. |
| `DPCS` / `DPCT` | Depth-cued color blend (single / triple). |
| `INTPL` | Far-color interpolation primitive (used internally by DCPL / DPCS). |
| `SQR` | Squares IR1..IR3 in place. |
| `OP` | Cross product of the rotation-matrix diagonal with IR. |
| `GPF` / `GPL` | General-purpose IR×IR0 multiply / accumulate (alpha-blend kernel). |

Each instruction sets MAC1..MAC3 / IR1..IR3 / FLAG with the same saturation semantics the hardware uses; the `Camera::transform` shim and the cop2 `RTPT` produce identical SXY output (cross-validated by the `gte_rtpt_matches_camera_transform` test).

### GTE register-transfer + memory ops

Beyond the cop2 instruction set the module exposes the four MIPS register-transfer ops (`MFC2` / `MTC2` / `CFC2` / `CTC2`) plus the two memory ops (`LWC2` / `SWC2`) so engines can replay a captured GTE trace without re-deriving the cop2 register layout:

- `read_data(idx)` / `write_data(idx, val)` - map the 32 cop2 data registers to typed fields. Indices 0..5 = V0..V2 (xy packed pairs + sign-extended z), 6 = RGBC, 7 = OTZ, 8..11 = IR0..IR3, 12..14 = SXY0..SXY2, 15 = SXYP (push-only write), 16..19 = SZ0..SZ3, 20..22 = RGB0..RGB2, 23 = RES1 (reserved), 24..27 = MAC0..MAC3, 28..29 = packed `IRGB` / `ORGB` (BGR555), 30 = LZCS, 31 = LZCR (count leading zeros / ones of LZCS).
- `read_ctrl(idx)` / `write_ctrl(idx, val)` - map the 32 cop2 control registers. Rotation / light / light-color matrices live as packed two-i16-per-word entries (RT11RT12 → cop2cr0, RT13RT21 → cop2cr1, RT22RT23 → cop2cr2, RT31RT32 → cop2cr3, RT33 → cop2cr4 sign-extended, etc.); translation triple at 5..7; back-color at 13..15; far-color at 21..23; viewport offsets `OFX` / `OFY` / `H` at 24..26; depth-cue slope / intercept `DQA` / `DQB` at 27..28; `ZSF3` / `ZSF4` at 29..30; `FLAG` at 31 (writable so captured traces can replay the post-instruction FLAG state).
- `LWC2 rd, addr` / `SWC2 rd, addr` - load / store cop2 data register `rd` from memory through the `Cop2Mem` trait. `VecMem` is shipped for replay against captured RAM snapshots; `NullMem` for tests that don't exercise memory at all. `load_vertices(mem, addr)` is a 24-byte bulk-load helper for the canonical retail 3-vertex emit (`LWC2 0..5` covering V0.xy / V0.z / V1.xy / V1.z / V2.xy / V2.z at 8-byte stride).

Each transfer op charges one cycle into `Gte::cycles` (matches the un-pipelined hardware budget). FLAG / cycle bookkeeping is identical between the higher-level instruction methods and the bare register-transfer path.

The per-mode descriptor table from `DAT_8007326C` is also exposed as a typed lookup at `crates/tmd/src/descriptor.rs`: `Descriptor::for_flags(flags)` returns the resolved `PacketShape` (one of `F3` / `FT3` / `G3` / `GT3` / `F4` / `FT4` / `G4` / `GT4`) and the per-prim vertex-index offset. Same on-disc bytes as the older `legaia_prims::vertex_offset_bytes` free function, exposed as typed fields so consumers can branch on shading mode (flat vs gouraud) and texture presence without re-deriving the bit math.

## Stage geometry detector (legacy, signal only)

A "12-byte fixed prefix `00 F0 84 7F 01 F0 1F 00 00 F1 00 00` repeated at 20-byte stride" detector lives at `crates/asset/src/stage_geom.rs`. It's not real stage geometry - it's the standard primitive-group header for Legaia TMD primitive group data when `((flags >> 1) - 8) >> 1 == K` (where K is the group type that uses 20-byte stride).

The detector is preserved as a signal during exploration ("this buffer contains a TMD with effect-style primitives") but for actual geometry extraction use the TMD parser (`crates/tmd::legaia_prims`).

## See also

**Reference** -
[Legaia TMD](../formats/tmd.md) ·
[PSX TIM](../formats/tim.md) ·
[NPC palettes](../formats/npc-palette.md) ·
[World-overview viewer](world-overview-viewer.md)
