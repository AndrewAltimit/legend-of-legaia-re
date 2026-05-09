# legaia-engine-render

Minimal `wgpu` renderer for the engine reimplementation track.

Owns a `wgpu` device + surface plus two render pipelines, sharing the
same surface and depth attachment:

- **Textured-quad** — `upload_texture` + `render(RenderTarget::Texture)`.
  Letterbox-preserves aspect ratio. Used by the TIM viewer.
- **Flat-shaded mesh** — `upload_mesh` + `render(RenderTarget::Mesh)`.
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

- `winit` 0.30 — windowing.
- `wgpu` 26 — GPU API.
- `glam` 0.30 — math.
- `legaia-tim` for `Vram`.

## PSX-style rendering

The VRAM-mesh pipeline supports PSX-faithful rasterisation via
`Renderer::set_psx_mode(true)`:

- **Affine UV interpolation.** UVs interpolate linearly in screen space
  (no perspective-correct division) — this reproduces the warping you
  see on retail PSX surfaces with steep depth gradients. UV is
  `@interpolate(linear)` in WGSL.
- **Sub-pixel vertex snap ("vertex jitter").** Clip-space `x`/`y` are
  snapped to the nearest integer pixel before rasterisation, giving the
  GTE's characteristic per-vertex shimmer on slow-moving geometry.
- **TSB / CBA flat shading per primitive.** Texture page and CLUT base
  remain `@interpolate(flat)` so each triangle samples from the same
  page and palette, matching `GP0(0x24)` semantics.

Toggle is global — apply once per frame before submitting draws.
Fixed-point GTE math helpers (`q3.12` rotation, `q19.12` translation)
live in [`gte`](src/gte.rs); production rendering still uses f32 wgpu
math, but the module is the single citation point for retail-correct
fixed-point arithmetic when re-targeting captured GTE traces.

## Future phases

Bit-exact GTE emulation (re-using the `gte` module's accumulator shape),
batched draws, and reverse-engineered TSB / CBA per-mode descriptor
overrides are deferred.

## See also

- [`docs/subsystems/renderer.md`](../../docs/subsystems/renderer.md) —
  full rendering pipeline including the GTE-mapped TMD render
  (`FUN_8002735c`, 60 GTE ops).
- [`docs/subsystems/engine.md`](../../docs/subsystems/engine.md) — how
  this slots into the overall engine architecture.
