# legaia-engine-ui

Pure, renderer-agnostic UI draw-list builders for the Legaia engine port. The
wgpu-free leaf that both the native renderer (`legaia-engine-render`) and the
browser play page (`legaia-web-viewer`) share.

## Scope

Every function projects a renderer-agnostic *view* struct - built by the host
from the live `World` - into a `Vec` of `TextDraw` / `SpriteDraw` primitives.
Each primitive is a screen rectangle plus a source rect into either the
proportional font atlas (`legaia-font`) or a VRAM sprite page, with an RGBA
tint. The host renderer rasterises them; neither the geometry nor the menu
navigation logic depends on the GPU backend.

- `ui_overlay` - dialog box, cutscene narration, battle HUD, encounter banner,
  stage-scale text, per-glyph sprite emit helpers.
- `ui_menu` - pause-menu field / status / spell / inventory / equipment panels,
  options + key-rebind, name entry, game-over, tactical-arts editor, the
  world-map battle-records screen (`records_screen_draws_for`, `FUN_801ED710`)
  and the dev-menu list-body geometry (`dev_menu_list_draws_for`,
  `FUN_801EAD98`).
- `ui_title_save` - title menu, 9-slice window chrome, save-select, save-slot
  grid + info panel, "Now checking" dialog.
- `ui_fishing` - fishing-minigame HUD: the ported persistent / catch HUD
  layout, gauge bars, digit field and banner animators, plus
  `fishing_hud_draws_for`, the consumer that renders that draw list.

`ui_fishing` is the one module that owns both halves. The fishing overlay's
HUD helpers are ports in their own right (`FUN_801d13f0`, `FUN_801d1580`,
`FUN_801d1870`, `FUN_801d76e0`, the banner animators), and they decide layout
rather than simulation state, so they sit beside their consumer here instead
of in `engine-core`, which keeps the minigame's numeric kernels.

## Composition

`legaia-engine-render` re-exports every item here at its historical crate-root
path (`pub use legaia_engine_ui::*`) so native shell code, the asset-viewer, and
tests reference the builders unchanged. The GPU-resident batch wrappers
(`TextOverlay` / `SpriteOverlay` / `UploadedSpriteAtlas`) stay in
`legaia-engine-render` because they hold wgpu handles.

Depends only on `legaia-asset`, `legaia-font`, `legaia-tim`, `glam`, `serde`,
and `bytemuck` - no wgpu, no winit - so it links into the lean WASM play build.
