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

The VRAM-mesh pipeline supports PSX-faithful rasterisation via
`Renderer::set_psx_mode(true)`:

- **Affine UV interpolation.** UVs interpolate linearly in screen space
  (no perspective-correct division) - this reproduces the warping you
  see on retail PSX surfaces with steep depth gradients. UV is
  `@interpolate(linear)` in WGSL.
- **Sub-pixel vertex snap ("vertex jitter").** Clip-space `x`/`y` are
  snapped to the nearest integer pixel before rasterisation, giving the
  GTE's characteristic per-vertex shimmer on slow-moving geometry.
- **TSB / CBA flat shading per primitive.** Texture page and CLUT base
  remain `@interpolate(flat)` so each triangle samples from the same
  page and palette, matching `GP0(0x24)` semantics.

Toggle is global - apply once per frame before submitting draws.
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
