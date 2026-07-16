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
packet consumes plus the OT-bucket depth. `afterimage::
project_streak_corners` reproduces the streak caller's invocation
(`+0x120` Y push, dynamic half-width, half-height `0x100`). `psx_sin` /
`psx_cos` reproduce the retail `RotMatrix*` trig LUT -
`trunc(4096·sin(2π·a/4096))`, pinned entry-for-entry by the disc-gated
`gte_sin_lut_real` oracle in `engine-shell`.

Fixed-point GTE math helpers (`q3.12` rotation, `q19.12` translation)
live in [`gte`](src/gte.rs); production rendering still uses f32 wgpu
math, but the module is the single citation point for retail-correct
fixed-point arithmetic when re-targeting captured GTE traces.

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

`battle_hud_draws_for(font, slots, popups, log, pen)` produces a
`Vec<TextDraw>` for the in-battle HUD. The view types `HudSlotView` /
`HudPopupView` / `HudLogView` keep the renderer agnostic to engine-core /
engine-vm types (matches the existing `ShopRow` / `level_up_draws_for`
pattern). HP rows pulse red when ≤25%; status icons render at row_y - 12
with 8 px stride; popups sit at slot_y - 16 (heal = green, crit =
yellow, plain damage = cyan); fade alpha multiplies into the text
colour's alpha channel.

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
