# legaia-engine-render

The native `wgpu` renderer behind `legaia-engine play-window` and the asset viewer: a software PSX VRAM sampled per primitive in the fragment shader, the retail shading path, and the opt-in enhancement passes.

This crate is where the hard `wgpu` link lives, which is why the
renderer-agnostic UI draw-list builders (`status_screen_draws_for`,
`options_draws_for`, `battle_hud_draws_for` and friends) are **defined in
[`legaia-engine-ui`](../engine-ui/README.md)**, not here - the browser
play page needs them without pulling in wgpu. Native callers name them in
`legaia_engine_ui`, and the wgpu-free render kernels in
`legaia_render_kernels`.

Owns a `wgpu` device + surface plus several render pipelines, sharing the
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

The [`window`](src/window.rs) module is the shared winit + wgpu app scaffold
(`EngineWindow`: open, resize, fixed-step tick pacing) that `asset-viewer` and
`engine-shell` both build on.

## What it must not depend on

`engine-core`, the session and the hosts. The renderer sits on `engine-ui`
and `render-kernels` (and reads the battle camera from `engine-battle-vm`);
it draws what a host hands it and never reaches into the simulation.

## How it draws

The passes themselves - PSX-mode rasterisation, the enhancement passes, the
screen-space overlay, landing a drawn frame back in VRAM, the battle HUD and
menu pipelines, the frame profiler and the retail draw-decision reference
ports - are in
[`docs/subsystems/renderer-port.md`](../../docs/subsystems/renderer-port.md).

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
