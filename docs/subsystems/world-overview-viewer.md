# World-overview viewer

The `/world-overview/` page in the static site renders each kingdom's
landmark layer in real-time WebGL 3D from a disc image. It exists to
make the world-map data layer reviewable end-to-end without a save
state or an emulator.

The retail world-map subsystem itself (overlay structure, key functions,
render pipeline, globals) is documented in [`world-map.md`](world-map.md);
this page covers only the viewer + the capture-side tooling that feeds it.

## Contents

- [Layout engine for unplaced slot-1 TMDs](#layout-engine-for-unplaced-slot-1-tmds)
- [Continent ground heightfield](#continent-ground-heightfield) · [walk-frame placed landmarks + decorations](#walk-frame-placed-landmarks--decorations)
- [Distance-cue fog pass](#distance-cue-fog-pass) · [per-kingdom fog colour](#per-kingdom-fog-colour)
- [Bulk-terrain placement resolver (MAN `0x7F` sentinels)](#bulk-terrain-placement-resolver-man-0x7f-sentinels) · [global-pool placement placeholders](#global-pool-placement-placeholders)
- [Ocean tile - disc-side asset + 13-frame CLUT animation](#ocean-tile---disc-side-asset--13-frame-clut-animation) · [web-overview shader plumbing](#web-overview-shader-plumbing)
- [Camera anchors](#camera-anchors) · [continent `.glb` export](#continent-glb-export)

## Layout engine for unplaced slot-1 TMDs

The MAN placement table pins a small subset of each kingdom's slot-1
TMD pack at world coordinates (5 / 6 / 17 slots for Drake / Sebucus /
Karisto). The remaining slots are positioned at runtime by the
field-VM via actor-mesh chains and don't carry a static world coord.
The viewer's "show unplaced slot-1 TMDs" toggle drops those onto a
canonical layout grid, classified by `slot1_classification.toml`:

- **landmark** - row south of the kingdom bounds, sorted by slot.
- **decoration** - row north of the kingdom bounds.
- **ground_tile** - grid west of the kingdom (the runtime tiles
  them via the overlay-routed dispatch table).
- **npc_token** - hidden (reused generic actor bases; reporting
  the count avoids cluttering the view).
- **unknown** - grid east of the kingdom.

Two per-mesh transforms keep the layout legible:

1. **AABB-centroid anchor** - each unplaced TMD is drawn so its
   AABB centroid sits at the assigned grid slot, instead of its
   TMD-local origin (which can be far from the visual centre and
   shift the mesh out of frame).
2. **Class-conditional footprint normalisation** - per-class
   target footprints in world units (landmark ~600, decoration ~200,
   ground_tile ~1200, unknown ~600). Each mesh's larger XZ extent maps
   to the target via a per-placement scale so the row reads at a
   consistent size regardless of the TMD's native scale.

The "normalize unplaced" toggle disables both transforms (falls back
to the legacy constant scale + TMD-local-origin pivot) so the user
can ground-truth against retail.

## Continent ground heightfield

The viewer draws the kingdom's full continent terrain the same way the
native engine's world-map render does: a procedural **heightfield
surface** built from the walk `.MAP` floor grid, per-cell-textured from a
terrain-type-keyed multi-page atlas. This is the bulk ground (grass /
mountain / water / forest), distinct from the sparse slot-1 landmark
meshes layered on top. The heightfield model and per-cell texturing are
pinned in [`world-map.md`](world-map.md) "Ground texturing"; the surface
math is `legaia_asset::field_objects::build_walk_heightfield`
(`FUN_80019278` bilinear floor sampler).

The WASM viewer builds the same surface from the raw PROT.DAT it already
has in hand (`legaia_web_viewer::build_walk_ground`), mirroring
`Scene::walk_heightfield` without the full `ProtIndex` / CDNAME stack:

- **Walk `.MAP`** is the entry two slots before the kingdom block start
  (`prot_base - 2`), identified by its `0x12000` extended footprint - the
  universal field-map resolution (the scene PROT clusters overlap by two
  entries, so the within-block `0x12000` entry is the *next* scene's map;
  for a kingdom it reads as the wrong continent with only a handful of
  `0x1000` cells).
- **Floor-height LUT** is `man[+0x02..+0x22]` (16 `s16` LE) from the
  kingdom bundle's MAN slot (slot 2) - the same bytes
  `Scene::field_floor_height_lut` reads.

`build_walk_ground` reuses `build_walk_heightfield` for the grid math, so
its output is **byte-identical** to the engine's `Scene::walk_heightfield`
(positions + per-cell UVs + per-cell `[clut, tpage]` + indices), asserted
for all three kingdoms by the disc-gated test
`crates/web-viewer/tests/walk_ground_parity.rs`.

Per-cell UVs follow the retail emitter's corner orientation: U along +X/col,
**V flipped vs +Z/row** (the low-Z corner samples the tile's bottom texel row).
This is baked into `build_walk_heightfield`, so the viewer inherits it for free;
it is what makes directional transition tiles (coastline sand, ridge faces) flow
continuously instead of mirroring in place. See
[`world-map.md`](world-map.md) "Ground texturing".

The heightfield's per-cell `[clut, tpage]` reference the terrain atlas
VRAM pages (grass `0x1A` at fb `(640, 256)`, mountain `0x0C` at fb
`(768, 0)`, water `0x1B`/`0x1C`, forest `0x0B`). Those pages are already
resident: the kingdom bundle's slot-0 TIM_LIST (uploaded to VRAM by
`set_scene_kingdom` for the landmark meshes) is the *same* atlas the
native `SceneLoadKind::WorldMap` VRAM build uses, so the ground samples
correct terrain textures from the VRAM the viewer already has.

The `walk_ground_{positions,uvs,cba_tsb,indices,quad_count}` WASM
accessors hand the surface to the WebGL renderer. `TmdRenderer.uploadGround`
keeps it resident as one mesh; `renderAssembled` draws it after the ocean
plane (so land occludes water via depth-test), with a fixed `diag(1, -1, 1)`
model - the same Y-flip the placement models apply, since the heightfield is
already in world coordinates. The "terrain" checkbox toggles the ground pass
(read per-frame, no kingdom re-entry). The heightfield drives the default
camera framing (centred on its XZ centroid, sized to its extent); the
"lock to retail top-view" button still recentres on the captured spawn
anchor on demand.

The assembled world view renders through a **perspective orbit camera**
(`buildWorldOrbitVp` in `site/js/webgl-math.js`) - the retail world map is
genuinely 3D (GTE `RTPT` perspective projection; confirmed by moving the
retail camera with GameShark), so the viewer matches: the eye orbits a
ground-plane target on `(yaw, pitch)` spherical angles, with `pitch = 0` the
straight-down view and larger values tilting toward the horizon. Zoom keeps
its orthographic meaning - `halfWidth`/`halfHeight` define the visible
window at the target plane, and the orbit distance is derived from them
(`dist = halfHeight / tan(fovY/2)`), so wheel zoom and pan speed behave
identically at any tilt. Controls: left-drag pans (through the yaw-rotated
ground basis), right-drag or shift+drag orbits, wheel zooms; "lock to
retail top-view" flattens `pitch` to 0 on the captured spawn anchor and the
reset button restores the default tilt. The gesture handler is the shared
`attachWorldOrbitControls` in `site/js/webgl-math.js`; the asset-viewer
page's assembled full-map mode attaches the same handler over the same
orbit camera, so both assembled views control identically.

Both cameras render the map **rotated 180° and then horizontally flipped**
to match retail's world-map orientation. The up vector is world `+Z` (the
180° rotation: screen-up = world `+Z`, screen-right = world `-X`), and the
projection then mirrors the horizontal screen axis (`buildTopDownVp` swaps
`ortho`'s left/right; `buildWorldOrbitVp` negates the perspective matrix's
X scale), so the net basis at `yaw = 0` is screen-up = world `+Z`,
screen-right = world `+X`. The mirror reverses triangle winding, which is
harmless since `renderAssembled` disables `CULL_FACE`. The pan controls map
drag deltas back through this same post-mirror basis. The ortho
`buildTopDownVp` path remains the projection for `renderAssembled` callers
whose camera record carries no `yaw` field (the asset viewer's full-scene
map mode).

## Walk-frame placed landmarks + decorations

The continent isn't just terrain: two sparse slot-1 pack-mesh layers draw on
top of the heightfield, in the **same `col*128` world frame** as the ground -
the same sets the native `play-window --scene map01 --world-map` render draws
over its heightfield:

- the **placed landmarks** (the `flags & 0x4` objects `FUN_8003A55C` stamps
  on occupied tiles - towers, castles, bridges), and
- the **decoration layer** - walk-visible cells whose record stamps a nonzero
  `+0x10` mesh with the mesh-drawn flag bit `0x2` and no placed flag: the
  crossed-quad billboard trees (a forest cluster stamps one tree mesh from
  dozens of cells), mountain groups, and small props. Cells with a nonzero
  `+0x10` but no `0x2` bit (the riverbank/system record 408 family) are NOT
  decorations and must not be stamped - drawing them tiles a wall mesh down
  every river.

`legaia_web_viewer::build_walk_placements` resolves both from the raw
PROT.DAT the viewer already holds, sharing the walk `.MAP` + floor-LUT
resolution with `build_walk_ground` (`resolve_walk_map_and_lut`):

- Runs `legaia_asset::field_objects::parse_placements` +
  `parse_walk_decorations` on the walk `.MAP` and concatenates.
- Each placement's mesh is the record's `+0x10` field (`pack_index`) - a
  slot into the kingdom's slot-1 TMD pack the `pack_mesh_*` accessors expose.
- World position is the placement's `(world_x, world_z)` plus a world Y of
  `-lut[floor_nibble] + y_off` (the runtime stores the floor LUT negated), so
  the mesh sits on the heightfield. This mirrors the native engine's
  `resolve_placement_draws` exactly; the disc-gated
  `crates/web-viewer/tests/walk_placements_parity.rs` asserts the viewer's
  resolved placements equal `Scene::walk_object_placements` +
  `Scene::walk_decoration_placements` + the floor LUT for all three kingdoms.

- Each placement carries the record's authored **yaw** (`+0x0A`, PSX
  `4096`-per-rev units) - the field `FUN_8003A55C` copies into the actor's
  rotation triple (`+0x24/+0x26/+0x28`) and the render dispatcher turns into
  the draw matrix via `FUN_80026988`. This is what gives the Sebucus island
  bridges their distinct quarter-turn orientations (`0x400`/`0xC00`) and the
  decoration trees their variety; the record's X/Z tilts are zero on every
  retail walk `.MAP` and aren't carried.

The `walk_placement_{count,slots,positions,rot_y}` WASM accessors hand the
list to JS, which uploads each referenced pack mesh once and pushes a draw
record per stamp. `renderAssembled` draws them after the ground with
`placementModelScaledY(x, -world_y, z, rotY, 1)` - the JS yaw is
`-(rot & 0xFFF) * PI / 2048` because retail's pure-Y matrix maps local `+Z`
to `(sin, 0, cos)`, the opposite yaw sense of `placementModelScaled*` -
scale `1` (the slot-1 meshes
are already in true world units, unlike the legacy overview-frame icons that
needed a presentation scale) and the same `(1, -1, 1)` Y-flip the ground and
the native render use. The anchor Y **must be negated on the JS side**:
`placementModelScaledY` flips only the mesh-local geometry, not the
translation, while the ground bakes `-lut` into its vertices and lands at
`+lut` (up) under the shared flip. An un-negated anchor mirrors the stamp
below the surface by `2*(lut - y_off)` - on Drake's mountain cells (LUT up
to 288) that buried whole cave entrances; the same negation the site
viewer's full-map path applies. The "landmarks" checkbox toggles the layer. The
fragment shader's PSX cutout rule (BGR555 `0` with STP `0` discards) is what
makes the tree quads read as foliage; the old `u_no_discard` silhouette
fallback is off in the assembled path now that the kingdom's real VRAM image
is uploaded and CLUTs resolve like retail.

One authored exception (`WALK_STAMP_SUPPRESS` in `world-overview-app.js`):
placed objects are script-managed - `FUN_8003A55C` runs each spawn's MAN
interaction script's leading ops inline, so a placed record's resting
position/visibility isn't necessarily its grid cell (see the placer's MAN
gate in [`world-map.md`](world-map.md)). The static viewer can't run MAN
scripts, so stamps known (from retail) not to rest at their grid cell are
suppressed by `(mesh, world x, world z)` - currently Drake's record-349
golden bridge, whose grid cell sits over the river at `(10688, 5312)` while
retail shows the single bridge at the record-441 road crossing
`(12224, 6336)`. `window.__woWalkStamps` exposes the stamp list actually
queued per render (the headless-verification hook, like `__woCam`).

**The legacy overview-frame placement layers stay hidden.** The
`world-overview.json` MAN-table landmarks, live-RAM actor placements, and the
unplaced-slot-1 layout grid (described under "Layout engine for unplaced
slot-1 TMDs" above) are in the top-down *overview* coordinate frame
(mednafen captures / MAN overview coords), which is misaligned with the
walk-view heightfield. The walk-frame placements above supersede them for the
landmark layer; the `SHOW_LEGACY_PLACEMENTS` flag in `world-overview-app.js`
still re-enables the old overview-frame code (preserved for the snapshot
panel + the unplaced-layout toggles).

## Distance-cue fog pass

The viewer's fog toggle approximates the retail world-map fog: the
diffuse term fades toward a per-kingdom haze colour with distance.
The math splits into two pieces the runtime keeps separate, and the
WebGL port mirrors that split:

- The **LUT** at `gp-0x2BC` (2048 u16 entries that climb from `0x0000`
  at near-Z to `~0x01FF` at far-Z) is a **per-Z scalar**, not a colour
  ramp. The retail overlay leaves at `0x801F7644..0x801F8690` `lh` the
  LUT entry, shift it left by 16, and add it to the high half of
  vertex SXY+offset words via `sw s1, 0x8(t1)` / `0xC(t1)` /
  `0x10(t1)`. The visible effect on flat triangles is a per-vertex
  screen-Y nudge proportional to `Z >> 5`.
- The **haze colour** is set per-kingdom via the GTE `FAR_COLOR`
  control register (loaded via `ctc2` during world-map enter, not
  surfaced by the `lwc2 t0, -0x2dc(t2)` load - that field is the
  `IR0` depth-cue factor, despite earlier doc tables labelling it
  "fog color").

The WebGL port runs this in a vertex + fragment shader:

- Per-vertex: `Z_far = exp2(-zShift) * dist(world, camera_origin)`,
  clamped to `[0, far_ref]` and normalised to `v_fog_t in [0..1]`.
  Approximates the runtime's `Z_far = Z >> shift` against the
  top-down camera origin.
- Per-fragment: sample `lut[clamp(v_fog_t * 2047, 0, 2047)]` as a
  scalar u16; normalise to `factor = lut_word / 511`; then
  `mix(lit, u_fog_color, factor)` with `u_fog_color` = the
  per-kingdom haze tint from `KINGDOM_FOG_TINT`. This produces the
  fade-toward-haze visual instead of treating the LUT entries as
  RGB tints (an earlier port did the latter and produced "richer
  textures" rather than fog).

The shader supports two LUT sources, in priority order:

1. **Disc-extracted LUT (default)** - the WASM viewer locates
   the 4 KiB (2048 u16) LUT inside `SCUS_942.54` via the
   `fog_lut::find` content-scan (monotone non-decreasing ramp with
   leading zero entries + saturating tail) and auto-uploads it on
   disc load. No file picker; one disc upload = full functionality.
   On the retail USA build the LUT sits at SCUS offset `0x05FCC0`
   (vaddr `0x8006FCC0`); the content scan handles regional variants
   without hardcoding.
2. **Kingdom-tinted fallback** - when SCUS extraction doesn't
   surface a LUT (raw PROT.DAT load, regional variant with shifted
   SCUS, modded disc), the shader falls back to using `v_fog_t`
   directly as the mix factor, still toward the kingdom haze tint.

The per-vertex math diverges from retail in one place: retail samples
Z from the GTE's screen-space pipeline after `rtpt`, while the
WebGL2 path uses XZ-plane distance to the fog origin (`fog_origin =
worldCam centre` by default). For a top-down ortho camera the two
quantities are equivalent up to a constant; for the orbit-camera mesh
inspector the fog toggle is hidden because it doesn't carry over.

## Bulk-terrain placement resolver (MAN `0x7F` sentinels)

MAN-record placements where ``(x_enc, z_enc) == (0x7F, 0x7F)`` static-
decode to the literal world coordinate ``(16320, 16320)`` (the
world's NE corner, just outside any visible kingdom). Those actors
are positioned at runtime by the FieldVM prescript embedded in the
record's trailing bytes, dispatched from ``FUN_8003A1E4`` (the MAN
placement walker in SCUS):

```c
// FUN_8003A1E4 lines 326-336 (excerpted):
uVar14 = (uint)*(byte *)(iVar11 + iVar10);    // script[PC]
if ((uVar14 - 0x24 < 2) && (... > 0x1F)) {    // op in {0x24, 0x25}
    while (true) {
        iVar10 = func_0x801de840(...);         // -> FieldVM dispatcher
        *(short *)(iVar9 + 0x9e) = (short)iVar10;
        if (uVar14 == 0x21) break;
        // walk next opcode
    }
}
```

Each actor is allocated by ``FUN_80024C88`` then its prescript runs
once through the FieldVM (``FUN_801DE840``). The prescript can write
``actor[+0x14] / actor[+0x18]`` (X / Z position), so the *resolved*
position differs from the literal MAN-record decode.

**Statically resolving these without running the FieldVM is not
covered by the asset extractor.** The MAN prescript is a per-record
bytecode that picks a position based on actor type, story-flag state,
overlay-resident lookup tables. A full clean-room port would need
the engine-vm field VM driving real actor records.

The practical alternative is a **runtime snapshot capture**:

- ``scripts/mednafen/resolve_bulk_terrain.py`` extracts the
  post-resolve placements out of mednafen save states. It walks every
  actor list head listed in `Globals used` (see [`world-map.md`](world-map.md#globals-used)),
  captures the actor's live ``+0x14 / +0x18`` coords plus its mesh
  chain at ``+0x44`` (resolved back to the kingdom TMD pack via
  reverse-magic-search), and tags each placement ``kind: 'bulk_terrain'``
  when ``actor[+0x90]`` is outside the MAN buffer or ``'man_actor'``
  otherwise.
- ``site/extract-world-placements.py`` merges the resulting JSON into
  ``site/world-overview.json`` under ``bulk_terrain_placements`` per
  kingdom (alongside the existing ``placements`` and
  ``live_placements`` fields). The world-overview viewer renders both
  layers in the same scene.
- ``crates/web-viewer::sentinel_placements`` is the Rust port of the
  RAM-side resolver (record parser + actor-list walker + TMD-pack
  reverse lookup) for downstream callers; the Python script is the
  end-to-end driver.

The Drake-only count produced by the existing PCSX-Redux capture
(``site/world-overview-live.json`` legacy single-bundle dict) lands
as ``man_actor`` under the new tagging since that capture script
predates the ``kind`` field.

## Global-pool placement placeholders

MAN-record placements with ``tmd_slot >= 0xF0`` reference the global TMD
pool (``DAT_8007C018``) rather than the kingdom-local pack at slot 1.
The disc-side global mesh pool is not yet bundled into
``site/world-overview.json``; until that pipeline lands, the viewer
stamps the kingdom pack's slot 0 mesh (typically a ground tile) at the
decoded world coordinates and tags the draw record ``kind: 'global_pool'``.
The snapshot panel surfaces both the underlying ``global TMD refs``
count and the ``+ N global-pool placeholders`` rendered count so the
gap stays visible without dropping the placements silently.

## Per-kingdom fog colour

The atmospheric-tick actor (``actor[+0x0C] == FUN_801E3E00`` at
``0x801E3E00``) interpolates the per-kingdom haze RGB into its
``+0x74`` field per frame. That u32 is the input to ``FUN_80043390``'s
``ctc2`` writers to the GTE ``FAR_COLOR`` control regs (``$21 /
$22 / $23``):

```c
// FUN_8001ADA4 case 5 (line 861):
FUN_80043390(puVar12, piVar2[0x1d], *(undefined2 *)(piVar2 + 0x1e));
//                    ^^^^^^^^^^^^^
//                    actor[+0x74] = current fog RGB (0x00BBGGRR)

// FUN_80043390 (0x80043498..0x800434D0):
andi $s6, $a1, 0x00FF      // R from $a1 = actor[+0x74]
srl  $s5, $a1, 8           // G
andi $s5, $s5, 0x00FF
srl  $s4, $a1, 16          // B
andi $s4, $s4, 0x00FF
sll  $s6, $s6, 4           // 8-bit -> 12-bit
sll  $s5, $s5, 4
sll  $s4, $s4, 4
ctc2 $s6, $21              // FAR_COLOR.R
ctc2 $s5, $22              // FAR_COLOR.G
ctc2 $s4, $23              // FAR_COLOR.B
```

The script that drives ``actor[+0x74]`` lives in
``FUN_801E3E00`` (overlay-resident at
``ghidra/scripts/funcs/overlay_world_map_801e3e00.txt``) and reads
its R/G/B bytes from ``script[PC + 7 / +8 / +9]``. The script source
is a per-kingdom blob at ``actor[+0x94]``; the static walker that
installs it isn't fully reversed yet, so the practical capture path
is the runtime snapshot.

When ``scripts/mednafen/resolve_bulk_terrain.py`` finds an actor
with ``tick == 0x801E3E00`` and ``actor[+0x74] != 0``, it surfaces
the live RGB as ``fog_color: { r, g, b, u24 }`` per kingdom in
``site/world-overview.json``. The world-overview viewer reads that
field at priority above the hand-eyeballed ``KINGDOM_FOG_TINT``
fallback. World-map saves that don't have an active atmospheric tick
fall back to the hardcoded table.

## Ocean tile - disc-side asset + 13-frame CLUT animation

The world-map ocean is a **static 4bpp tile** + **CLUT cycling**
animation, both shipped on disc:

- **Texture:** PSX TIM image at VRAM ``(768, 256)`` 64 halfwords ×
  256 rows (= 256 × 256 logical pixels in 4bpp), inside slot 0
  (TIM_LIST) of each world-map kingdom bundle (PROT 0085 Drake / 0244
  Sebucus / 0391 Karisto). The kingdom-specific TIM is the one with
  CLUT block fb_xy ``(0, 506)`` and image block fb_xy ``(768, 256)``.
  Texture bytes vary per kingdom (each ships its own variant).
- **Wave-ramp region:** the ocean data fills the **top-left 96 × 96
  logical pixels** of the 256 × 256 page; the rest is shared with
  other tile prims in 4bpp mode and reads as CLUT-entry-0 padding at
  world-map entry. Confirmed by walking non-zero byte density across
  every row and byte column of the decompressed image - rows 1-96
  + logical pixel cols 0-95 are 100% non-zero, the rest tapers off
  to zero past row 191. The prim-trace POLY_FT4 cluster UVs for the
  ``clut=0x7E80 tpage=0x001C`` family land entirely inside this
  envelope (UVs from ``(0,0)`` to ``(95,95)``).
- **Base CLUT:** 256-entry BGR555 row at VRAM ``(0, 506)`` (same TIM
  as the texture). The first 16 entries are the ones the runtime
  overwrites per frame; entries 16..255 stay fixed and belong to other
  tiles sharing the row.
- **Animation table:** **13 frames × 16 BGR555 entries = 416 bytes**,
  byte-identical across all three retail kingdoms (SHA-256
  ``dfc6dd263a71152c40ab7726193d79e9658efc04402f4280f5f49f392e32071f``).
  Located by signature scan in each kingdom's decompressed slot 0;
  the disc wraps each frame in a 532-byte "CLUT-only TIM" record at
  TIM_LIST slots 3-5 (Sebucus/Karisto) or 10-15 (Drake), with the
  first frame starting 0x54 bytes into the record (8 zero bytes +
  12-byte CLUT block header + 32 unrelated CLUT entries).

The runtime DMAs one frame at a time onto VRAM ``(0, 506)``,
overwriting the first 16 CLUT entries; the wave peak (``0x3D05``
bright blue) propagates through indices 0..7 over the 13-frame cycle,
creating the horizontal rolling-wave appearance visible in retail.
Frame 0 starts at index 5; the cycle wraps back to index 0 at frame
8 and continues through index 2 at frame 12.

## Web-overview shader plumbing

``crates/web-viewer::ocean::find_ocean_assets`` decompresses the
kingdom bundle's slot 0, locates the ocean TIM by VRAM coords, and
signature-scans the slot for the animation table. The disc-gated
test ``crates/web-viewer/tests/ocean_assets.rs`` verifies extraction
across all three kingdoms.

The viewer animates the water the way retail does: each animation
step writes the frame's 16 BGR555 entries **into the VRAM texture at
`(0, 506)`** - the CLUT row the continent heightfield's water cells
sample (CBA `0x7E80`; the retail per-frame ocean DMA target, same
mechanism as the live engine's `advance_ocean_animation`). Every
water prim in the scene shimmers from that single CLUT write, so
terrain-embedded water and the open sea stay phase-locked as one
layer. The wall-clock frame cadence matches the live engine's tuned
approximation (6 sim ticks at 60 Hz = 0.1 s/frame) and advances even
when the backdrop pass is toggled off.

The ocean *backdrop* (``site/js/webgl-tmd.js``) is a flat quad at
``y=0`` extending past the continent so the sea reaches the horizon
under the orbit camera; its shader samples the same 4bpp texture
through a 16-entry CLUT texture updated from the same frame counter.
The plane is drawn before bulk-terrain meshes so depth-test handles
occlusion; the "ocean" checkbox toggles only this backdrop pass.

Capture pipeline for the procedural-tint fallback used before the
disc is loaded:

```
scripts/mednafen/resolve_bulk_terrain.py --bundles map01,map02,map03 \
    --json site/world-overview-live.json <mc1> <mc2> <mc3>
python3 scripts/asset-investigation/extract-world-placements.py \
    --prot-dir extracted/PROT --out site/world-overview.json
```

``pick_ocean_color`` walks every POLY_FT4 cluster reported by
``mednafen-state prim-trace``, samples each cluster's representative
tile via its CLUT + tpage out of the save's VRAM, and ranks
blue-dominant clusters by ``hits × blue_dominance``. The winner's
average RGB lands as ``site/world-overview.json[kingdom].ocean_color``
and drives the viewer's fallback colour before the textured pipeline
loads.

## Camera anchors

Per-kingdom camera centres + zoom anchors live in two tables and a
JSON override:

- `KINGDOM_CAM` - walk-view spawn anchors (load-time map-origin
  coords from `_DAT_80089118` / `_DAT_80089120`, decoded by
  `mednafen-state world-map-camera --table <save>`). This is the
  default view when a kingdom tab is opened.
- `KINGDOM_TOPVIEW_CAM` - hardcoded fallback for the
  "lock to retail top-view" button.
- ``world-overview.json[kingdom].topview_cam`` - per-kingdom
  capture preferred over `KINGDOM_TOPVIEW_CAM` when present.
  ``resolve_bulk_terrain.py::capture_topview_cam`` writes this from
  ``mednafen-state world-map-camera`` against the user-supplied save
  state for each kingdom. The "lock to retail top-view" button reads
  this first; the values drive the world cam centre + frame the
  kingdom at its captured extent.

The captured anchor is the load-time map origin (`-_DAT_80089118` /
`-_DAT_80089120`). Top-view dev-menu captures (``DAT_801F2B94 != 0``)
would refine this with an interactively-scrolled centre + a refined
``zoom``; walk-view captures (``DAT_801F2B94 == 0``) match the spawn
anchor, which is good enough as a "lock" target since the dev-menu
top-view also enters from this anchor before user input scrolls it.

## Continent `.glb` export

The page's "Download .glb" button bakes the assembled world view - the
continent ground heightfield plus every walk-frame landmark / decoration
stamp on screen - into a single binary glTF via the WASM
`scene_export_*` session (`legaia_web_viewer::scene_export`, bake in
`legaia_asset::scene_gltf`). The page hands the exporter the same mesh
buffers it uploads to WebGL and the same per-draw
`(translation, rotY, scale)` triples it builds model matrices from, so
the file matches the render; the PSX VRAM+CLUT texture indirection is
baked out into one RGBA atlas (one 256x256 tile per distinct
`(cba, tsb-page)` pair the vertices sample, NEAREST-sampled, `MASK`
alpha for the PSX word-0 cutout rule). The animated ocean backdrop is a
screen effect, not geometry, and is excluded. The asset-viewer page's
full-map (town) and single-TMD exports ride the same session; the enemy
table's monster export is the sibling `monster_gltf::export_glb` (it
additionally carries action animations).

## See also

**Reference** -
[World map](world-map.md) ·
[World-map overlay](../formats/world-map-overlay.md) ·
[Renderer](renderer.md)
