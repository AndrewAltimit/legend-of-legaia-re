# The port's wgpu renderer

How [`legaia-engine-render`](../../crates/engine-render/README.md) draws a frame: the PSX-mode rasterisation switch, the enhancement passes, the screen-space overlay, the VRAM capture path, the battle HUD and menu pipelines, the frame profiler and the retail draw-decision kernels kept as reference ports. Retail's own renderer is [`renderer.md`](renderer.md); how a pixel gets its colour is [`shading.md`](shading.md).

## PSX-style rendering

The 3D mesh pipelines carry the PSX rasterisation traits below. Only two of
them - vertex snap and dithering - are gated, by
`Renderer::set_psx_mode(true)` (off by default); the rest are unconditional,
and semi-transparency has its own switch.

- **Affine UV interpolation.** UVs interpolate linearly in screen space
  (no perspective-correct division) - this reproduces the warping you
  see on retail PSX surfaces with steep depth gradients. UV is
  `@interpolate(linear)` in WGSL, on every path regardless of PSX mode.
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
  VRAM-mesh and colour-mesh paths, gated by `Renderer::set_semi_blend`
  (**on by default**, independent of PSX mode) - see
  [Semi-transparency](#semi-transparency) below.

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
word (same ABE/ABR bit positions, `psx_blend::pack_blend_word`); with
semi-blend on, the opaque colour pass discards ABE prims and the colour-mesh blend
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
`DrawOTag` walks it head-first). A key is snapped to a whole `SZ` first
(`psx_blend::snap_depth_key`): retail's `OTZ` is an integer, while one depth
reached through two model matrices can come out a thousandth apart in `f32`.
`opdeene`'s caption is the case - two camera-locked sprite-arm quads at view
depth `792`, the subtractive shadow keyed `791.999`, which drew it over the
additive face until the keys were snapped. The per-prim metadata
(`psx_blend::SemiPrim`: centroid + ABR mode + tail location) is recorded
once at upload time by the tail builders; the per-frame list lives in a
reused buffer, and contiguous same-draw, same-mode runs coalesce into
single indexed draws (`psx_blend::coalesce_sorted`).

**Dither parity** follows retail's rule that only shading arithmetic is
dithered: the untextured blend entries dither `F` (a gouraud result)
before the blend; the textured blend pass draws raw texels and stays
undithered. Its foreground is truncated to a 5-bit channel before the blend
equation reads it (`psx_blend_foreground`, both hosts, every render mode),
which is how the GPU hands the modulated texel to the blend
([shading](shading.md#step-6-semi-transparency)).

In the `legaia-engine play-window` binary PSX mode (vertex snap + dither)
is opt-in via the `LEGAIA_PSX_RENDER=1` environment variable.

## Enhanced lighting (enhancement, NOT retail)

`Renderer::set_dynamic_lighting(true)` shades the VRAM-mesh and colour-mesh
passes under a staged `LightingMood` (`set_lighting_mood`), adds the staged
point lights (`set_scene_lights`), draws emissive-tagged prims (TSB / blend
bit 13) at the mood's emissive gain, and composites the staged glow sprites
(`set_glow_sprites` - additive halos + light shafts, the `glow_pipeline`).
**Off by default in the renderer, and off IS retail**: the field path has no
runtime light source (see `psx_light` above), so the WGSL helper
(`dyn_light`) early-returns the input colour when the uniform enable is zero
and the disabled path is pixel-identical to the faithful render.
`play-window` turns it on from the persisted `enhanced_lighting` option
(`I` toggles, `F8` cycles the time of day, `--no-dynamic-lighting` forces
it off).

The derivation - which prims glow, the light list, the moods, the glow
geometry, and the CPU mirror of the shading law - lives in the shared
`legaia_render_kernels::scene_lighting` kernel, so the browser play page lights from the same values.
This crate keeps the WGSL twin (`dyn_light` / `scene_point_gain` in
`shaders.rs`, held to the mirror by the `dyn_light` module's tests) and the
part only wgpu has: `scene_lights::light_view_proj` and the per-light PCF
shadow pass (`set_dyn_shadows`, default on; `--no-dyn-shadows` / the `Y`
key). The full model is on
[renderer.md](renderer.md#enhanced-lighting-enhancement-default-on).

## Camera-occlusion fade (enhancement, NOT retail)

`Renderer::set_occlusion_fade(true)` plus a per-frame
`Renderer::set_occlusion_focus(clip_pos, feet_clip, strength, proj_scale_y)`
dissolve scene fragments that sit between the camera and the player to a
4x4-Bayer screen-door, so the character reads through walls / roofs. **Off
by default in the renderer, and off IS retail** - the disabled path is pixel-identical
(the WGSL `occl_keep` returns 1.0, which no Bayer threshold reaches).
`strength` is the host's eased output of the **visibility gate**
(`engine-scene::field_occlusion`): a 5-point eye-to-player ray-cast against
the static scene that arms the fade only when the character is completely
hidden - a partially visible character never fades. A fragment then fades
only when its draw is environment geometry (the
`set_occlusion_env_draws` watermark exempts the player / NPC draws via
`flags[2]`), it is nearer the camera than the player by more than a small
depth margin (16 units - hugging occluders still open, actors are
protected by the watermark instead), and it lies inside a screen-space
circle around the player's projected centre and above the line through the
projected feet (`feet_clip`, `occlusion_fade::lift_factor` - the ground in
front of the character and the foot of a wall never dissolve); the keep probability
feathers from 1.0 at the rim to 0.25 at the centre, then blends toward
identity by the strength. Implemented as a fragment `discard` in the
opaque + semi-blend scene entries - no extra pipelines, depth writes
intact. The focus rides in the per-frame group-2 scene uniform;
single-mesh pipelines compile a `return 1.0` stub, like
`scene_point_gain`. Tunables + the CPU lockstep mirror live in the
`occlusion_fade` module. In `play-window` this is ON by default
(`--no-occlusion-fade`, or the `F4` key at runtime - a function key precisely
because it must not collide with a pad-bindable letter), and the feature is
deliberately per-fragment + per-triangle - the site's abandoned per-body
occluder cull is the failure mode it avoids (see
`docs/subsystems/renderer.md`). The browser play page ships the GLSL twin
(`occl_keep`/`occl_bayer` in `site/js/webgl-shaders.js`, constants
mirrored there - keep them in lockstep with this module).

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

The [`afterimage`](../../crates/render-kernels/src/afterimage.rs) module (wgpu-free, in `legaia-render-kernels`) ports the battle move-FX
streak draw (`FUN_801e1ab0`): `build_afterimage_quad` assembles one
jittered, semi-transparent textured quad (`POLY_FT4`) from four projected
screen corners + the move's trail-texture id, reproducing the per-corner
`rand` wobble, the random brightness band that picks a texture sub-column,
and the exact UV / CLUT / texpage layout. It takes an injected rng (the
retail source is the BIOS `rand`) so the construction is pure and
unit-tested. `screen_overlay::
afterimage_screen_quad` lifts it into a `ScreenPrim` that the
[`screen_overlay`](../../crates/engine-render/src/screen_overlay.rs) pass links into the screen-space
ordering table and the wgpu renderer draws via `RenderTarget::ScreenOverlay`.

## Volumetric ground fog (enhancement, NOT retail)

`Renderer::set_fog_volume` stages one frame's mist bank
(`render-kernels::fog_volume`, simulated by the `World`); `None` draws
nothing and, if the pass was never built, costs nothing. The pass
(`renderer/fog_volume.rs`) draws twelve instanced horizontal sheets over a
48 x 48 quad mesh between the two halves of the scene pass, depth-tested
against the scene and reading its depth for a soft intersection. The browser
play page draws the same frame through `site/js/webgl-fog-volume.js`, a GLSL
transcription. The simulation, the scene styles and the shading recipe are
on [renderer.md](renderer.md#volumetric-ground-fog-enhancement).

**Ground field.** Each field scene's `FogGround`
(`engine-scene::fog_ground::build_for_scene`, built once at scene load by
both hosts over their static draw lists) puts the sheet mesh on the drawn
floor and stops it over open air, so a tower top such as `teien` no longer
carries a flat bank across the sky around it. Past a rim the mesh keeps
going as a near-vertical spill curtain hugging the wall face, carrying a
third of the bank (`SPILL_SHARE`), then, further out and far below the rim,
as a faint distant haze layer (a negative mesh weight) that lies only where
nothing drawn stands above it.

The field is built from what is on screen, not from what the data holds
(`fog_ground::FogSurfaces`): the placed objects a script hid or scaled to
nothing are left out and the rest are taken at their render scale, as the
hosts draw them; a prim whose texels are almost all the transparent word
(`SURFACE_MIN_OPACITY`) is not a surface; and a semi-transparent prim is
floor only where it lies level (a pond, not a mist veil hung across a
corridor) and is never a wall the spill drapes over. The bank's movers
follow the same rule: a field channel that draws nothing (render scale
`0`, the off-map hide box, a flag word that opts out of the drop shadow)
does not part it. The field also
reaches the shader as an RGBA8 texture at binding 3, visible to both stages
(coverage, spill, flow; at most 256 x 256 texels of 64 world units, uploaded
only when its generation changes): it fades the bank across the floor's edge
at fragment resolution, and breaks the spill into slow, falling tendrils
with gaps - a pour mask along the rim, eroded tendrils, puffs - spread
outward as a slab, with the constants in the frame header
(`FOG_SPILL_CONSTANTS`, `FOG_HAZE_CONSTANTS`). A frame without a field binds
one full-coverage texel, which is the bank as it was before. The recipe is
on [renderer.md](renderer.md#volumetric-ground-fog-enhancement).

**Battle profile.** `fog_volume::BATTLE_FOG` sets a battle's bank to a third
of the field opacity and an ankle-high depth, so the stage floor stays
readable; a field-to-battle switch eases it in from zero instead of popping.

**Cost.** The field is CPU work at scene load only: one rasterisation of
the static triangles plus two nearest-site sweeps over at most 65,536
cells. Per frame the pass adds one texture sample per sheet fragment and
per vertex (four more on spill vertices and fragments), a handful of noise
evaluations on spill and haze fragments, and nothing on the CPU.

## Bloom (enhancement, NOT retail)

`Renderer::set_bloom(true)` makes the bright parts of the 3D frame - lamp
halos, emissive props, lit windows, sunlit sky - bleed a soft glow into
their surroundings. **Off by default, and off IS the frame without it**: none
of the passes below run and the scene is the single pass it always was.

With it on, the scene half of a `RenderTarget::Scene` frame (the 3D draws,
the glow sprites, the volumetric fog) renders into an offscreen target
instead of the presented one, and `renderer/bloom.rs` runs an extract and
four blur passes at a quarter of the surface on each axis, then a composite:

1. **extract** - four bilinear taps cover the 4x4 scene block under one
   bloom texel, each weighted on its own by a soft-knee brightness law (the
   largest channel against a threshold, a quadratic ease across the knee),
   so one bright pixel does not lift its dark neighbours;
2. **blur** - a separable 9-tap Gaussian as five linear taps, horizontal then
   vertical, twice;
3. **composite** - the scene texel plus the glow times the intensity,
   saturating, written to the presented target.

The screen-space layers (the attached-light pools, sprites, text, the
composited ordering-table tail) draw after the composite, so the HUD never
blooms.

The response comes from `legaia_render_kernels::bloom::BloomParams`, keyed on
enhanced lighting's mood: the darker the mood the lower the threshold and the
stronger the glow (`for_glow` over `LightingMood::glow`), so at night the
lamps, halos and the Genesis Tree carry it while daylight takes only a faint
lift on its brightest surfaces; with enhanced lighting off the daylight
response applies. The renderer derives it from its own staged mood.

The browser play page draws the same passes through `site/js/webgl-bloom.js`:
it blits the finished default framebuffer into a texture (it cannot sample
the framebuffer itself), runs the same extract and blur, and adds the glow
back with an additive blend - the sum the native composite writes - at the
engine's response (`play_bloom_params`). The `bloom_parity` test evaluates
each of the three fragment stages on both hosts through `shader_eval` and
requires them to agree.

**Cost.** One full-resolution composite pass plus five passes over a
sixteenth of the pixels; on the page, one extra resolve blit. Nothing on the
CPU beyond two uniform writes a frame.

## Screen-space overlay pass

**The tint is retail's, 5-bit.** The text / sprite shader tints every quad
through `ui_tint` - WGSL generated from `legaia_render_kernels::ui_tint`, the
same kernel the browser page tints its atlas copies with: the texel truncated
to 5 bits, times the colour, truncated again (`(T5 * C) >> 7`), and a font
drop shadow under a known ink drawn as that ink's own CLUT entry rather than a
multiply of the fill ([dialog font](../formats/dialog-font.md)). The
truncation is the pipeline-overridable `UI_QUANTIZE`; the renderer keeps both
pipeline sets and `Renderer::set_ui_psx_tint` picks one, retail by default.

[`screen_overlay`](../../crates/engine-render/src/screen_overlay.rs) is the render capability behind
`RenderTarget::ScreenOverlay`: a `ScreenPrim` is either a textured
`POLY_FT4` (`ScreenQuad` - four screen corners + UV/CLUT/texpage + a 24-bit
modulation colour) or a solid/blended `FlatQuad`, each carrying an OT
`ot_index`. `order_primitives` reproduces the retail `AddPrim`/`DrawOTag`
walk - farthest bucket first, later-submitted-first within a bucket (the
same convention as `psx_blend::sort_blend_list`). `build_geometry` emits a
flat NDC vertex/index buffer plus a run list coalesced by blend class, which
the renderer uploads once per frame and draws one indexed run at a time
(opaque pipeline or the matching per-ABR blend pipeline). Semi-transparency
is per texel, as on the GPU: a semi run draws twice, its textured prims'
STP-clear texels solid through `fs_semi_opaque` and then the STP-set texels
plus every pixel of its untextured prims through the blend pipeline. The play
page's screen-prim pass (`u_stp_pass`) and the CPU twin
`screen_prim_raster` apply the same rule. Blending every texel instead is
visible: the fishing HUD draws an additive `m` glyph under its LINE plate,
which retail's all-STP-clear palette hides and a whole-quad blend shows
through the digits.

The corner projection itself is ported in [`billboard`](../../crates/render-kernels/src/billboard.rs)
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

**Backdrops draw first.** A screen primitive whose four corners sit at the
far plane (`ScreenPrim::is_backdrop`) is one retail links into the farthest
bucket: PROT 0898's cast colour wash (`ot + 0x3FFC`) and the overworld sky
band. Composited after the 3D pass, a far-plane depth test keeps it off
opaque geometry but not off a blended primitive, which writes no depth, so a
wash drawn there erases every additive effect over the sky. Both hosts split the under
list (`screen_prim::split_backdrop`) and draw the backdrops in the scene pass
after the clear and before the first 3D draw: the native renderer's
`draw_screen_overlay_backdrop_runs`, the play page's `preSceneHook` on
`renderAssembled` (`play_screen_prim_backdrop_run_end` names the span).

The primitives are authored in the **PSX display space** (320x240), not the
window: every retail emitter clamps against `0x140`/`0xF0`, so the renderer
hands `build_geometry` that space and the overlay stretches over the whole
surface.

Fixed-point GTE math helpers (`q3.12` rotation, `q19.12` translation)
live in [`gte`](../../crates/render-kernels/src/gte.rs) - hosted wgpu-free in
`legaia-render-kernels` so the browser play page links the same arithmetic;
production rendering still uses f32
wgpu math, but the module is the single citation point for retail-correct
fixed-point arithmetic when re-targeting captured GTE traces.

## Landing a drawn frame back in VRAM

On the console the framebuffer *is* VRAM - the display area is a rect inside
the same 1024x512 halfword page textures are read from - so a primitive can
sample pixels the GPU drew moments earlier. The renderer only ever pushed
the software page *to* the GPU; [`vram_capture`](../../crates/render-kernels/src/vram_capture.rs)
(wgpu-free, in `legaia-render-kernels`) is the
missing direction. `blit_rgba_into_vram` quantises an RGBA8 readback to
BGR555 and writes it into a `legaia_tim::Vram` rect, and
`Renderer::capture_into_vram` wires that to `capture_rgba`. The write lands
in the CPU-side page, so the capture is equally visible to `move_image`,
`region_has_data` and the VRAM parity oracle.

The quantisation is exact rather than approximate: the shaders' last stage
expands a 5-bit channel as `(c5 << 3) | (c5 >> 2)`, so `byte >> 3` recovers
`c5` for all 32 values. A capture at the native 320x240 is not resampled at
all; a window-sized frame is point-sampled down.

`FIELD_CAPTURE_ROWS` / `FIELD_CAPTURE_COLS` name the two 15-bpp rects the
transition's texture-page words decode to, each spanning VRAM columns
`320..=639` on its own row. Only **`FIELD_CAPTURE_COLS` is a capture
destination**: the rows rect is the intermediate the curtain's column pass
renders into every frame, and no style samples it from the capture.
Inside the columns page the frame lands four rows down
(`FIELD_CAPTURE_LAND`, retail's drawn-row-to-`v` offset) and the rows around
it are cleared to transparent (`FIELD_CAPTURE_CLEARS`), as the transition
init does
([where the capture lands](battle-stage-camera.md#where-the-capture-lands-in-the-page)).

## Field-to-battle transition emitter

`battle_intro` is the per-frame, per-style working-set
owner that stands between the (already live) transition state machine in
`engine-core` and the ordering table above. It seeds the selected style's
working set, advances it off the transition entity's own clock, and emits
`ScreenPrim`s plus the per-style fade. The emitter itself is wgpu-free and
lives in [`legaia-render-kernels`](../../crates/render-kernels/src/battle_intro.rs) - both hosts arm it -
and [`src/battle_intro.rs`](../../crates/engine-render/src/battle_intro.rs) here holds the
one renderer-bound wrapper, `update_field_capture`, which turns
`Renderer::capture_scene_rgba` output into the emitter's `land_capture_rgba` /
`refresh_captured_page` calls. The target it reads back is the caller's: the
play window hands it the frame it presents, screen layers and enhancement
passes included, less the transition's own primitives
([what the capture holds](battle-stage-camera.md#what-the-capture-holds)).

All five styles tick and all five emit: `emit_particle_field` (both particle
arms) plus `emit_spinup_ring`, `emit_tile`, `emit_swirl_band`, and
`intro_quad_to_screen` for the curtain.

The curtain is the one with a second output. Its packet builder produces
screen-space corners, so there is no projection step to invent - but retail
draws its two passes into two different places. The column pass runs under a
`SetDrawArea(320, 0, 320, 240)` that the row pass then samples as texture pages
`0x105` / `0x108`, so the effect is a render-to-texture and only the row pass is
on screen. `compose_curtain_intermediate` rasterises the column quads into that
rect on the CPU each frame (there is no render-to-VRAM target for a
screen-space quad list), and the one-shot field capture goes into the *columns*
rect only. The tile shatter - the default random-encounter style - has all of
its inputs pinned too: the ten-face packet is `engine-vm`'s `tile_face_quads`,
the projection + accept chain is the FT4 handler's (`emit_tile`, with
`euler_rot_psx` as the `FUN_80026988` port), and the 4bpp shade page its side
faces sample is `field_char_textures` entry 0, re-landed in the transition's
cloned page at capture time.

Two retail nuances stay un-carried and are stated rather than hidden: the
dispatcher's depth-cue bank (the screen overlay has no channel for it) and the
frame-to-frame **accumulation** every style rides on - retail subtracts a small
constant from the display buffer instead of clearing, so its strips leave
fading trails where the port's leave black.

## GTE register-transfer + memory ops

Beyond the cop2 instruction set the [`gte`](../../crates/render-kernels/src/gte.rs) module ships
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
`engine-ui::battle_chrome`: per-member roster panels (102x48 at `y 164`) at
rest, replaced for the acting member by the full-width active-actor bar at
`(8, 188)`, each carrying name / `HP` label + `cur` right-aligned + `max`
running forward / the same pair for `MP` - and **no gauge bar**. A top-left
plaque names the actor the frame belongs to, which is also the port's whole
monster readout since retail draws no monster gauge. Popups sit at slot_y - 16 (heal = green, crit = yellow, plain damage =
cyan); fade alpha multiplies into the text colour's alpha channel. Monster
rows, the LV / AP tail and the "ENCOUNTER!" banner are diagnostic-only,
behind `LEGAIA_DIAG_HUD`. Geometry provenance, including which retail table
the measurement falsified, is on
[`docs/subsystems/battle.md`](battle.md#the-drawn-surface).

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
documented in [`docs/subsystems/field-menu.md`](field-menu.md).

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

## Retail draw-decision kernels (reference ports)

A family of modules ports SCUS passes that *decide* what the draw path does
rather than emit geometry. Each is pure and unit-tested, and none is called by
a host: they carry `REPLACED-BY` naming the engine mechanism
that does the routine's job (or, for `battle_on_screen`, that nothing on the
disc references it).

- [`actor_bind`](../../crates/engine-render/src/actor_bind.rs) (`FUN_80020f88`) - resolves an actor's
  mesh-pool index off its `.MAP` placement record (`rec[+0x10] + prefix`,
  the rule [`renderer.md`](renderer.md) records as
  replacing the falsified positional one) and reports whether a `0x9C`-byte
  render node must be allocated.
- [`battle_on_screen`](../../crates/engine-render/src/battle_on_screen.rs) (`FUN_8005126C`) - re-anchors
  a battle sprite on its seat actor and tests the projected box's horizontal
  span against `[0, 0x140]`. Horizontal only: retail reads the X of two
  corners and no Y at all, which is what separates it from the rectangle
  probe `FUN_8001B73C`.
- [`mode_transition`](../../crates/engine-render/src/mode_transition.rs) (`FUN_80016230`) - the
  mode-entry prologue: frame-pacing reset, the RAM-cached-overlay word-sum
  verdict, and the field snapshot a battle / cutscene / minigame mode is
  entered behind.
- [`actor_cull`](../../crates/engine-render/src/actor_cull.rs) (`FUN_801CF754`) - the field actor-contact
  **broad phase**, not a render cull: the per-frame candidate list (a
  `+/-0x180` window around the player, capped at `0x20`, falling back to the
  full actor walk when full) that the contact probe `FUN_801CFC40` reads.
  `REPLACED-BY` the engine's contact probes, which test every candidate
  directly; the retail-numbering reference stays here.

[`gte_trace`](../../crates/engine-render/src/gte_trace.rs) sits beside them as the harness rather than a
kernel: a deterministic record of the cop2 register state before and after
each op, so a captured GTE trace can be replayed against the `gte` module's
arithmetic.
