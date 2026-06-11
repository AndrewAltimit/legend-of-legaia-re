# legaia-engine-render

Minimal `wgpu` renderer for the engine reimplementation track.

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
- **No synthetic lighting.** The engine's default readability hack - a
  per-frame directional Lambert from a made-up light direction - is
  disabled. Retail bakes its GTE lighting into the per-vertex colours and
  texels; faithful mode shows that source data unlit (the textured/colour
  meshes pass texel/vertex colour straight through). Default mode keeps
  the ambient-biased shade so untextured silhouettes stay readable.

- **Semi-transparency blend modes.** Per-prim PSX blending on the
  VRAM-mesh and colour-mesh paths (see the `psx_blend` module). The TMD
  mode byte's ABE bit travels in bit 15 of the per-vertex TSB attribute
  (packed by the `legaia_tmd::mesh` builders); the blend equation is the
  texpage ABR field (TSB bits 5..=6): mode 0 `0.5*B + 0.5*F`, 1 `B + F`,
  2 `B - F`, 3 `B + 0.25*F`. Because the STP decision is *per texel* (a
  texel's BGR555 bit 15 picks blend-vs-opaque inside one semi-transparent
  prim), the renderer draws two passes: the opaque pass draws everything
  but discards STP texels of semi-transparent prims, then a blend pass
  re-draws only the semi-transparent triangles (a per-ABR-mode index
  tail appended at upload time by `psx_blend::append_semi_tail`) keeping
  only STP texels, with one fixed-function blend pipeline per mode
  (mode 0 uses a 0.5 blend constant; mode 2 is reverse-subtract; mode 3
  pre-scales F by 0.25 in its fragment entry point). Blend draws depth-
  test (`LessEqual`) but don't write depth, and run after all opaque
  scene draws. `blend_apply` is the CPU mirror the blend-state mapping
  is unit-tested against.
  Untextured (`F*`/`G*`) ABE prims have no per-texel STP gate - they
  blend *all* their pixels. `upload_color_mesh_blended` carries that
  state in a per-vertex blend word (same ABE/ABR bit positions,
  `psx_blend::pack_blend_word`); in PSX mode the opaque colour pass
  discards ABE prims and the colour-mesh blend pipelines re-draw their
  per-ABR-mode index tail (`psx_blend::append_semi_tail_words`) with
  the prim colour as `F`. Untextured TMD prims carry no texpage, so the
  caller resolves ABR from draw-env state (mode 0 = the PSX default);
  plain `upload_color_mesh` keeps every prim opaque.
  Blend-draw *ordering* approximates the retail ordering table at
  per-draw granularity: all semi-carrying draws (textured + untextured
  in one sequence) blend far-to-near by their model origin's clip-space
  depth (`psx_blend::sort_far_to_near`), not scene-list order. Dither
  parity in the blend pass follows retail's rule that only shading
  arithmetic is dithered: the untextured blend entries dither `F` (a
  gouraud result) before the blend; the textured blend pass draws raw
  texels and stays undithered.

In the `legaia-engine play-window` binary this whole mode is opt-in via
the `LEGAIA_PSX_RENDER=1` environment variable.

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
unit-tested.

The corner projection itself is ported in [`billboard`](src/billboard.rs)
(`FUN_800195a8`): `project_billboard` transforms a center point to view
space under the ambient camera (MVMVA, low-halfword wrap), fans out the
four ±half-size corners, optionally spins them in-plane (`Rz` from the
12-bit PSX angle space), and perspective-divides each (RTPT×3 + RTPS),
returning the screen corners in the exact order the retail `POLY_FT4`
packet consumes plus the OT-bucket depth. `afterimage::
project_streak_corners` reproduces the streak caller's invocation
(`+0x120` Y push, dynamic half-width, half-height `0x100`). `psx_sin` /
`psx_cos` reproduce the retail `RotMatrix*` trig LUT —
`trunc(4096·sin(2π·a/4096))`, pinned entry-for-entry by the disc-gated
`gte_sin_lut_real` oracle in `engine-shell`.

Fixed-point GTE math helpers (`q3.12` rotation, `q19.12` translation)
live in [`gte`](src/gte.rs); production rendering still uses f32 wgpu
math, but the module is the single citation point for retail-correct
fixed-point arithmetic when re-targeting captured GTE traces.

## GTE Phase 6 - register-transfer + memory ops

Beyond the cop2 instruction set the [`gte`](src/gte.rs) module now ships
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

## Future phases

Batched draws and reverse-engineered TSB / CBA per-mode descriptor
overrides are deferred.

## See also

- [`docs/subsystems/renderer.md`](../../docs/subsystems/renderer.md) -
  full rendering pipeline including the GTE-mapped TMD render
  (`FUN_8002735c`, 60 GTE ops).
- [`docs/subsystems/engine.md`](../../docs/subsystems/engine.md) - how
  this slots into the overall engine architecture.
