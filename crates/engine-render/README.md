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

## Future phases

PSX-style affine texturing (matching the GTE's per-vertex projection
quirks), sub-pixel jitter, full GTE emulation, and batched draws are
all phase-2.

## See also

- [`docs/subsystems/renderer.md`](../../docs/subsystems/renderer.md) —
  full rendering pipeline including the GTE-mapped TMD render
  (`FUN_8002735c`, 60 GTE ops).
- [`docs/subsystems/engine.md`](../../docs/subsystems/engine.md) — how
  this slots into the overall engine architecture.
