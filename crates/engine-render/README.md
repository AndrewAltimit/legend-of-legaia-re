# legaia-engine-render

Minimal `wgpu` renderer for the engine reimplementation track.

This crate is where the hard `wgpu` link lives, which is why the
renderer-agnostic UI draw-list builders (`status_screen_draws_for`,
`options_draws_for`, `battle_hud_draws_for` and friends) are **defined in
[`legaia-engine-ui`](../engine-ui/README.md)**, not here - the browser
play page needs them without pulling in wgpu. `engine-render`
re-exports that crate wholesale (`pub use legaia_engine_ui::*`), so
`engine_render::status_screen_draws_for` still resolves; edit them in
`engine-ui`.

Owns a `wgpu` device + surface plus two render pipelines, sharing the
same surface and depth attachment:

- **Textured-quad** - `upload_texture` + `render(RenderTarget::Texture)`.
  Letterbox-preserves aspect ratio. Used by the TIM viewer.
- **Flat-shaded mesh** - `upload_mesh` + `render(RenderTarget::Mesh)`.
  Lit by a single directional light, depth-tested. Uses a
  `glam::Mat4` MVP supplied per-frame so the host can spin the model
  without re-uploading.
- **Vertex-colour mesh** - `upload_color_mesh` + a `Scene`'s `color_draws`.
  Untextured `F*`/`G*` props (per-vertex RGB, no UVs - the meshes the
  VRAM-textured path drops). Flat face-shaded, no VRAM lookup; shares the
  scene depth buffer and per-draw MVP slots. Fed by
  `legaia_tmd::mesh::tmd_to_color_mesh`. `upload_color_mesh_blended`
  additionally takes per-vertex blend words (ABE bit 15 + ABR bits 5..=6,
  `psx_blend::pack_blend_word`) so untextured semi-transparent prims
  blend in PSX mode.
- **Screen-space 2D overlay** - `render(RenderTarget::ScreenOverlay)`. PSX
  `POLY_FT4` textured quads + flat quads in surface pixels, drawn in
  ordering-table order (back-to-front by OT index, LIFO within a bucket)
  with per-ABR semi-transparency. Textured quads sample the shared PSX VRAM
  through the same CBA/TSB CLUT decode as the 3D VRAM-mesh path. Built from
  a `screen_overlay::ScreenPrim` list (see below); this is the draw path the
  afterimage streak rides and the clean public API a `screen_fx`
  (iris / letterbox / panel / sprite) consumer calls.

## Software PSX VRAM model

The renderer carries a 1024×512 R16Uint texture (the canonical PSX VRAM
shape) populated by uploading every TIM associated with the current
scene. Per-primitive CBA + TSB values come from the TMD primitive
walker; the fragment shader does:

1. Sample VRAM at the texture-page coordinates.
2. Decode the resulting 16-bit cell as 4bpp / 8bpp / 15bpp depending on
   the primitive's TSB mode.
3. For 4/8bpp, sample the CLUT row (also in VRAM).
4. Output BGR555 → RGBA8.

This means meshes with textures spread across multiple VRAM pages render
correctly in one draw, instead of needing per-page sub-meshes.

## Stack

- `winit` 0.30 - windowing.
- `wgpu` 26 - GPU API.
- `glam` 0.30 - math.
- `legaia-tim` for `Vram`.

## PSX-style rendering

The 3D mesh pipelines support PSX-faithful rasterisation via
`Renderer::set_psx_mode(true)`:

- **Affine UV interpolation.** UVs interpolate linearly in screen space
  (no perspective-correct division) - this reproduces the warping you
  see on retail PSX surfaces with steep depth gradients. UV is
  `@interpolate(linear)` in WGSL.
- **Sub-pixel vertex snap ("vertex jitter").** Clip-space `x`/`y` are
  snapped to the nearest integer pixel before rasterisation, giving the
  GTE's characteristic per-vertex shimmer on slow-moving geometry.
- **15-bit ordered dithering.** The shaded colour is dithered with the
  PSX GPU's 4x4 offset matrix and quantized to 5 bits per channel (BGR555
  framebuffer depth) - the cross-hatch gradients of retail output. The
  WGSL helper (`PSX_DITHER_WGSL`) mirrors the unit-tested CPU `psx_dither`
  module, and the composed shaders are naga-validated in the test suite.
- **TSB / CBA flat shading per primitive.** Texture page and CLUT base
  remain `@interpolate(flat)` so each triangle samples from the same
  page and palette, matching `GP0(0x24)` semantics.
- **PSX texture blending - the lighting.** Retail runs *no light source* on
  the field path: its two TMD renderers issue exactly one GTE colour op
  between them (`DPCS`, the depth cue) and never an `NC*` op, so the
  shading is baked into each prim's colour word and applied by the GPU as
  `texel * colour / 128` (`0x80` neutral, below darkens, above brightens
  up to ~2x). The mesh shaders do the same: each vertex carries the baked
  colour, `psx_modulate` applies the blend, `psx_depth_cue` applies `DPCS`
  (identity at the field's `IR0 = 0`; set with `Renderer::set_depth_cue`).
  The `psx_light` module mirrors both on the CPU and pins them. There is
  no synthetic Lambert on any path that has real colour data to draw.

- **Semi-transparency blend modes.** Per-prim PSX blending on the
  VRAM-mesh and colour-mesh paths - see [Semi-transparency](#semi-transparency)
  below, which is the one part of PSX mode with real structure to it.

### Semi-transparency

Lives in the `psx_blend` module. The TMD mode byte's ABE bit travels in
bit 15 of the per-vertex TSB attribute (packed by the `legaia_tmd::mesh`
builders); the blend equation is the texpage ABR field (TSB bits 5..=6):
mode 0 `0.5*B + 0.5*F`, 1 `B + F`, 2 `B - F`, 3 `B + 0.25*F`.

**Why two passes.** The STP decision is *per texel* - a texel's BGR555
bit 15 picks blend-vs-opaque inside a single semi-transparent prim. So
the opaque pass draws everything but discards the STP texels of
semi-transparent prims, then a blend pass re-draws only the
semi-transparent triangles keeping only STP texels - a per-ABR-mode index
tail appended at upload time by `psx_blend::append_semi_tail`. The blend pass uses
one fixed-function pipeline per mode (mode 0 uses a 0.5 blend constant,
mode 2 is reverse-subtract, mode 3 pre-scales F by 0.25 in its fragment
entry point). Blend draws depth-test (`LessEqual`) but don't write
depth, and run after all opaque scene draws. `blend_apply` is the CPU
mirror the blend-state mapping is unit-tested against.

**Untextured prims are the exception.** Untextured (`F*`/`G*`) ABE prims
have no per-texel STP gate - they blend *all* their pixels.
`upload_color_mesh_blended` carries that state in a per-vertex blend
word (same ABE/ABR bit positions, `psx_blend::pack_blend_word`); in PSX
mode the opaque colour pass discards ABE prims and the colour-mesh blend
pipelines re-draw their per-ABR-mode index tail
(`psx_blend::append_semi_tail_words`) with the prim colour as
`F`. Untextured TMD prims carry no texpage, so the caller resolves ABR
from draw-env state (mode 0 = the PSX default); plain
`upload_color_mesh` keeps every prim opaque.

**Ordering mirrors the retail ordering table, at per-primitive
granularity.** Every semi prim of every semi-carrying draw (textured and
untextured in one shared sequence) is keyed by its model-space
centroid's clip-space `w` under the draw's MVP
(`psx_blend::prim_depth_key`). By linearity of the MVP that equals the
average of the prim's vertices' clip `w` - the GTE avg-Z the OT bins on.
The whole list blends far-to-near *regardless of draw boundaries*, so
prims that interleave in depth across overlapping draws still blend in
correct global order (`psx_blend::sort_blend_list`).

Equal keys form one OT bucket and draw later-submitted-first - the
retail LIFO bucket order (`AddPrim` prepends to a bucket's list,
`DrawOTag` walks it head-first). The per-prim metadata
(`psx_blend::SemiPrim`: centroid + ABR mode + tail location) is recorded
once at upload time by the tail builders; the per-frame list lives in a
reused buffer, and contiguous same-draw, same-mode runs coalesce into
single indexed draws (`psx_blend::coalesce_sorted`).

**Dither parity** follows retail's rule that only shading arithmetic is
dithered: the untextured blend entries dither `F` (a gouraud result)
before the blend; the textured blend pass draws raw texels and stays
undithered.

In the `legaia-engine play-window` binary this whole mode is opt-in via
the `LEGAIA_PSX_RENDER=1` environment variable.

## Opt-in dynamic lighting (enhancement, NOT retail)

`Renderer::set_dynamic_lighting(true)` layers a soft, warm dynamic light
over the baked shading on the VRAM-mesh and colour-mesh passes. **Off by
default, and off IS retail**: the field path has no runtime light source
(see `psx_light` above), so the disabled path is pixel-identical to the
faithful render and the parity oracles are unaffected - the WGSL helper
(`dyn_light`) early-returns the input colour when the uniform enable is
zero.

When enabled, each fragment's post-`psx_modulate` colour is scaled by

```text
gain = ambient + (diffuse * |N.L| + pool) * warm_tint    (capped at ~1.3x)
```

- `N` is the smoothed per-vertex normal the VRAM-mesh vertex format
  already carries (area-weighted face normals accumulated per shared
  position by `legaia_tmd::mesh` - continuous across connected surfaces,
  so lighting varies within primitives, not per-prim). The normal-less
  colour-mesh prims and zero-normal singletons fall back to the
  screen-space-derivative face normal. `|N.L|` (not `max(N.L, 0)`)
  because prim winding in the corpus is mixed - walls shade with their
  orientation while a Y-flip in the draw parity changes nothing.
- `pool` is a soft screen-space "pool of light" centred slightly above
  frame centre, fading toward the corners - the gentle
  vignette-of-light gradient over the ground.
- Texels stay crisp: the gain is a smooth per-pixel scale on the same
  nearest-sampled PSX texel path, never a filter.

Tunables: `DYN_LIGHT_DIR` / `DYN_LIGHT_TINT` / `DYN_LIGHT_AMBIENT`
(renderer state) plus the `DYN_*` weights in the `dyn_light` WGSL helper;
the CPU mirror + lockstep tests live in the `dyn_light` module. In
`play-window` this is the `--dynamic-lighting` flag, toggled at runtime
with the `I` key and reflected on the HUD status line.

`Renderer::set_texture_window(mask_x, mask_y, off_x, off_y)` maps to
GP0(0xE2) "Texture Window setting" - four 5-bit values in 8-pixel steps
that clamp / wrap texture-coordinate sampling to a smaller window inside
the texture page. Default all-zero is a no-op. The fragment shader
applies the per-pixel
`coord = (coord & ~(mask*8)) | ((offset & mask)*8)` transformation
before texture-page lookup. Retail Legaia leaves the register at zero
almost everywhere; the API is wired primarily so future runtime
LoadImage / DMA-to-VRAM trace work can replay the register state
faithfully.

Toggle is global - apply once per frame before submitting draws.

The [`afterimage`](src/afterimage.rs) module ports the battle move-FX
streak draw (`FUN_801e1ab0`): `build_afterimage_quad` assembles one
jittered, semi-transparent textured quad (`POLY_FT4`) from four projected
screen corners + the move's trail-texture id, reproducing the per-corner
`rand` wobble, the random brightness band that picks a texture sub-column,
and the exact UV / CLUT / texpage layout. It takes an injected rng (the
retail source is the BIOS `rand`) so the construction is pure and
unit-tested. The finished quad is no longer parked: `screen_overlay::
afterimage_screen_quad` lifts it into a `ScreenPrim` that the
[`screen_overlay`](src/screen_overlay.rs) pass links into the screen-space
ordering table and the wgpu renderer draws via `RenderTarget::ScreenOverlay`.

## Screen-space overlay pass

[`screen_overlay`](src/screen_overlay.rs) is the render capability behind
`RenderTarget::ScreenOverlay`: a `ScreenPrim` is either a textured
`POLY_FT4` (`ScreenQuad` - four screen corners + UV/CLUT/texpage + a 24-bit
modulation colour) or a solid/blended `FlatQuad`, each carrying an OT
`ot_index`. `order_primitives` reproduces the retail `AddPrim`/`DrawOTag`
walk - farthest bucket first, later-submitted-first within a bucket (the
same convention as `psx_blend::sort_blend_list`). `build_geometry` emits a
flat NDC vertex/index buffer plus a run list coalesced by blend class, which
the renderer uploads once per frame and draws one indexed run at a time
(opaque pipeline or the matching per-ABR blend pipeline). A semi-transparent
prim is treated as fully blended (no per-texel STP split yet - faithful for
the additive afterimage trail and flat quads; documented in the module).

The corner projection itself is ported in [`billboard`](src/billboard.rs)
(`FUN_800195a8`): `project_billboard` transforms a center point to view
space under the ambient camera (MVMVA, low-halfword wrap), fans out the
four ±half-size corners, optionally spins them in-plane (`Rz` from the
12-bit PSX angle space), and perspective-divides each (RTPT×3 + RTPS),
returning the screen corners in the exact order the retail `POLY_FT4`
packet consumes plus the OT-bucket depth. The divide is the GTE's UNR
reciprocal and each stored corner is saturated to the SXY FIFO's signed
11 bits, both through the same kernels the `Camera::transform` COP2
oracle is pinned against - so a behind-camera corner takes the hardware's
`0x1FFFF`-quotient smear rather than a sentinel. `afterimage::
project_streak_corners` reproduces the streak caller's invocation
(`+0x120` Y push, dynamic half-width, half-height `0x100`). `psx_sin` /
`psx_cos` reproduce the retail `RotMatrix*` trig LUT -
`trunc(4096·sin(2π·a/4096))`, pinned entry-for-entry by the disc-gated
`gte_sin_lut_real` oracle in `engine-shell`.

A `ScreenQuad` also carries an optional per-vertex `gouraud` array, which
makes it a `POLY_GT4` rather than a `POLY_FT4`. The transition styles need
it: a built transition quad carries a separate top-edge and bottom-edge
colour, and the gradient between them *is* the effect.

Two render targets consume the pass. `RenderTarget::ScreenOverlay` is the
whole-frame form - it clears and draws nothing but quads.
`RenderTarget::SceneWithScreenPrims` is the compositing form, drawing the
ordered quads on top of a `Scene` in the same frame; that is what any real
consumer needs, since a streak over a battle scene or a transition strip
over a field scene cannot be expressed by the whole-frame form. Retail draws
no such distinction - 3D primitives and screen-space packets share one
ordering table and one `DrawOTag` walk - so the split is a port artifact and
this variant is where the halves meet.

The primitives are authored in the **PSX display space** (320x240), not the
window: every retail emitter clamps against `0x140`/`0xF0`, so the renderer
hands `build_geometry` that space and the overlay stretches over the whole
surface.

Fixed-point GTE math helpers (`q3.12` rotation, `q19.12` translation)
live in [`gte`](src/gte.rs); production rendering still uses f32 wgpu
math, but the module is the single citation point for retail-correct
fixed-point arithmetic when re-targeting captured GTE traces.

## Landing a drawn frame back in VRAM

On the console the framebuffer *is* VRAM - the display area is a rect inside
the same 1024x512 halfword page textures are read from - so a primitive can
sample pixels the GPU drew moments earlier. The renderer only ever pushed
the software page *to* the GPU; [`vram_capture`](src/vram_capture.rs) is the
missing direction. `blit_rgba_into_vram` quantises an RGBA8 readback to
BGR555 and writes it into a `legaia_tim::Vram` rect, and
`Renderer::capture_into_vram` wires that to `capture_rgba`. The write lands
in the CPU-side page, so the capture is equally visible to `move_image`,
`region_has_data` and the VRAM parity oracle.

The quantisation is exact rather than approximate: the shaders' last stage
expands a 5-bit channel as `(c5 << 3) | (c5 >> 2)`, so `byte >> 3` recovers
`c5` for all 32 values. A capture at the native 320x240 is not resampled at
all; a window-sized frame is point-sampled down.

`FIELD_CAPTURE_ROWS` / `FIELD_CAPTURE_COLS` name where retail parks the
field-to-battle capture. That falls out of the transition's own texture-page
words with no capture needed - they decode to 15-bpp pages whose strips span
VRAM columns `320..=639` on two rows.

## Field-to-battle transition emitter

[`battle_intro`](src/battle_intro.rs) is the per-frame, per-style working-set
owner that stands between the (already live) transition state machine in
`engine-core` and the ordering table above. It seeds the selected style's
working set, advances it off the transition entity's own clock, and emits
`ScreenPrim`s plus the per-style fade.

Style coverage is **not** uniform, and the difference is which retail packet
builder is ported rather than effort:

| Style | Ticks | Emits |
|---|---|---|
| Curtain | yes | yes - complete |
| Tile shatter | yes | yes - complete |
| Scatter / spin-up particles, swirl | yes | no |

The curtain is complete because its packet builder is itself ported and
produces screen-space corners, so there is no projection step to invent; its
descriptor table is disc data that parses, and its texture pages decode to
the capture rects above. The tile shatter - the default random-encounter
style - is complete because all of its inputs are pinned: the ten-face packet
is `engine-vm`'s `tile_face_quads`, the projection + accept chain is the FT4
handler's (`emit_tile`, with `euler_rot_psx` as the `FUN_80026988` port), and
the 4bpp shade page its side faces sample is `field_char_textures` entry 0,
re-landed in the transition's cloned page at capture time. The other three
end in a GTE/GPU packet emitter that is documented but not ported, and the
swirl's fan is triangles, which `ScreenPrim` has no variant for at all. Their
working sets still tick, because the fade and the battle handoff both ride
the same clock.

## GTE register-transfer + memory ops

Beyond the cop2 instruction set the [`gte`](src/gte.rs) module ships
the four MIPS register-transfer ops (`MFC2` / `MTC2` / `CFC2` / `CTC2`)
and the two memory ops (`LWC2` / `SWC2`) so engines can replay a captured
GTE trace without re-deriving the cop2 register layout. `read_data` /
`write_data` map the 32 cop2 data registers (V0..V2 packed pairs, RGBC,
OTZ, IR0..IR3, SXY-FIFO push slot `SXYP`, SZ-FIFO entries, RGB-FIFO
entries, MAC0..MAC3, packed `IRGB` / `ORGB`, `LZCS` / `LZCR`) to typed
register fields; `read_ctrl` / `write_ctrl` handle the 32 control
registers. LWC2 / SWC2 thread through a `Cop2Mem` trait - `VecMem`
backs replay against captured RAM snapshots; `NullMem` is the default
for tests that don't exercise memory.

## Battle HUD pipeline

`battle_hud_draws_for(font, frame, pen)` produces a `BattleHudDraws` for the
in-battle HUD: a `text` list sampling the dialog-font atlas and a `sprites`
list sampling the resident system-UI atlas. The view types `HudSlotView` /
`HudPopupView` / `HudLogView` keep the renderer agnostic to engine-core /
engine-vm types (matches the existing `ShopRow` / `level_up_draws_for`
pattern).

The default surface is retail's, off the packet-pinned
`engine-vm::battle_chrome`: per-member roster panels (102x48 at `y 164`) at
rest, replaced for the acting member by the full-width active-actor bar at
`(8, 188)`, each carrying name / `HP` label + `cur` right-aligned + `max`
running forward / the same pair for `MP` - and **no gauge bar**. A top-left
plaque names the actor the frame belongs to, which is also the port's whole
monster readout since retail draws no monster gauge. Popups sit at slot_y - 16 (heal = green, crit = yellow, plain damage =
cyan); fade alpha multiplies into the text colour's alpha channel. Monster
rows, the LV / AP tail and the "ENCOUNTER!" banner are diagnostic-only,
behind `LEGAIA_DIAG_HUD`. Geometry provenance, including which retail table
the measurement falsified, is on
[`docs/subsystems/battle.md`](../../docs/subsystems/battle.md#the-drawn-surface).

HP and MP readouts are tinted by the **four-tier retail colour law**
(`hp_bar_color_index` / `mp_bar_color_index`, ports of FUN_800349EC /
FUN_80035EA8): danger at `cur <= max >> 2`, caution at `cur <= max >> 1` or any
active status flag, normal above that, and - HP only - a K.O. tier at zero. The
native window is the consumer: `engine-shell/.../window/hud.rs` calls this
builder for every battle frame from the `BattleHud` model that
`window/battle.rs::sync_battle_hud_rows` refreshes.

Column offsets are pinned, not guessed - the bar's and the panel's off the
display-list walk, the diagnostic row's off retail-dialog-font advances. A
disc-gated test walks all three sets against the real font, which is what
catches a field overrunning the next column.

## Menu chrome

`menu_window_chrome_draws_for(rects, dst_rect, origin, scale)` is the
reusable 9-slice bordered-window primitive shared by every faithful menu
panel. It composes the interior fill + border of an arbitrary
`(x, y, w, h)` stage rect from the resident system-UI atlas tiles
(`SaveMenuAtlasRects`, the same `PROT.DAT[0x018E0]` sprite sheet the save
screen builds). `scale_stage_text_draws(draws, origin, scale)` is its text
companion: it maps a menu's glyphs, laid out in 320×240 stage pixels, into
surface coordinates so text and frame stay locked at any window size. The
field pause menu and its sub-screens (status / spells / items / equip /
arts) route through both, framed by the play-window at the placement rects
documented in [`docs/subsystems/field-menu.md`](../../docs/subsystems/field-menu.md).

## Frame profiler

`profile` is an opt-in per-frame timing breakdown for `play-window`. It is
off by default and free when off (every entry point short-circuits on one
cached `bool`), so the instrumented call sites cost a predicted branch per
frame. Enable it with `LEGAIA_PROFILE=1`; a rolling one-second summary goes
to stderr:

```text
[profile] 1052.7 fps over 629 frames | frame avg  0.93ms p50  0.82 p99  3.49 \
  | draws 288+92 | tick 0.00 pose:actor 0.25 pose:prop 0.11 pose 0.01 \
    drawlist 0.02 acquire 0.08 uniforms 0.02 encode 0.33 submit 0.07 present 0.03
```

The stages carve the frame at the boundaries that matter: `tick` (world
sim), `pose:*` (per-frame mesh skinning + upload), `drawlist` (building the
scene draw list), then the renderer's own `acquire` (swapchain wait),
`uniforms` (per-draw uniform staging), `encode`, `submit` and `present`.
`draws N+M` is the scene's textured + untextured draw-call count, so the
`encode` cost is attributable per draw.

Two companion knobs make it a repeatable benchmark:

| Env var | Effect |
|---|---|
| `LEGAIA_PROFILE=1` | Enable the breakdown. |
| `LEGAIA_PROFILE_FRAMES=N` | Print a final summary and exit after `N` frames. |
| `LEGAIA_VSYNC=off` | Configure the surface with an uncapped present mode. |

`LEGAIA_VSYNC=off` matters for measurement: with the default `AutoVsync`
the frame time is pinned to the display refresh interval and the whole cost
of a frame lands in `acquire`, so a vsync'd run reads the refresh rate
rather than the engine's own headroom.

**Skinned actors are memoised, not rebuilt.** A field NPC's ANM clip is a
short loop over a fixed set of poses, so the skinned mesh for a given
`(placement slot, clip frame)` is a constant. `play-window` skins and
uploads it on the first visit to that frame and reuses the GPU buffers
afterwards; the playhead still advances every frame, so the animation is
unchanged. Rebuilding it per frame instead - re-deriving identical vertex
bytes into freshly allocated GPU buffers - dominates the field frame in a
populated town. `LEGAIA_POSE_CACHE_VERIFY=1` re-checks the memo against the
live pose on every cache hit and logs any mismatch, which is what pins the
`(slot, frame)` key as non-aliasing.

## Retail draw-decision kernels (no host yet)

A family of modules ports SCUS passes that *decide* what the draw path does
rather than emit geometry. Each is pure, unit-tested and carries a
`NOT WIRED` disclosure naming the host it is missing - the state they act on
(actor pool, battle context, mode dispatcher) lives in `engine-core`.

- [`actor_bind`](src/actor_bind.rs) (`FUN_80020f88`) - resolves an actor's
  mesh-pool index off its `.MAP` placement record (`rec[+0x10] + prefix`,
  the rule [`renderer.md`](../../docs/subsystems/renderer.md) records as
  replacing the falsified positional one) and reports whether a `0x9C`-byte
  render node must be allocated.
- [`battle_actor_tick`](src/battle_actor_tick.rs) (`FUN_800480d8`) - the
  ordered tint / signature-effect / after-image / draw schedule for one
  battle actor, plus the defeated-monster grey stamp (`0x00808080`, a
  24-bit RGB colour word - not a `0x80808080` flag).
- [`attach_swap`](src/attach_swap.rs) (`FUN_8004ccd4`) - picks default vs
  variant equipment mesh per attach-bone channel from the playing entry's
  `+0xA4` frame windows, with the part-count-mismatch escape. Layout in
  [`battle-data-pack.md`](../../docs/formats/battle-data-pack.md).
- [`battle_on_screen`](src/battle_on_screen.rs) (`FUN_8005126C`) - re-anchors
  a battle sprite on its seat actor and tests the projected box's horizontal
  span against `[0, 0x140]`. Horizontal only: retail reads the X of two
  corners and no Y at all, which is what separates it from the rectangle
  probe `FUN_8001B73C`.
- [`battle_sideband`](src/battle_sideband.rs) (`FUN_80056208`) - the battle
  intro / in-battle / outro sideband state machine and its cadence-invariant
  camera pull-back ramp.
- [`mode_transition`](src/mode_transition.rs) (`FUN_80016230`) - the
  mode-entry prologue: frame-pacing reset, the RAM-cached-overlay word-sum
  verdict, and the field snapshot a battle / cutscene / minigame mode is
  entered behind.

## Current limitations

Draws are not batched, and the TSB / CBA per-mode descriptor overrides
are not applied - the renderer uses the per-prim TSB / CBA values as
uploaded.

## See also

- [`docs/subsystems/renderer.md`](../../docs/subsystems/renderer.md) -
  full rendering pipeline including the GTE-mapped TMD render
  (`FUN_8002735c`, 60 GTE ops).
- [`docs/subsystems/engine.md`](../../docs/subsystems/engine.md) - how
  this slots into the overall engine architecture.
