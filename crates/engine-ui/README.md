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
  stage-scale text, per-glyph sprite emit helpers, and the floating value
  readout's font fallback (`battle_value_readout_draws_for` - retail's own
  cells and pitch, for hosts with no screen-space VRAM sink to draw the real
  24x24 sheet through).
- `ui_menu` - pause-menu field / status / spell / inventory / equipment panels,
  options + key-rebind, name entry, game-over, the post-battle spoils panel
  (`battle_spoils_draws_for`), tactical-arts editor, the
  world-map battle-records screen (`records_screen_draws_for`, `FUN_801ED710`)
  and the dev-menu list-body geometry (`dev_menu_list_draws_for`,
  `FUN_801EAD98`).
- `ui_title_save` - title menu, 9-slice window chrome, save-select, save-slot
  grid + info panel, "Now checking" dialog.
- `battle_tutorial_box` - the sparring-tutorial prompt window: text rows at the
  emitter's measured rect plus that rect's window skin. Stage-space, so both
  hosts run it through their stage transform.
- `arts_input` - the **arts command-input** screen: the four direction chips
  and their D-pad glyph, the pennant input bar and its cost-weighted seat
  law, the AP plate, and the Triangle arts-list window. Retail runs one such
  screen for the battle Arts command and for the Muscle Dome's Attack, so
  the battle hosts and the dome page all compose from this module.
  Packet-pinned - see
  [`minigame-muscle-dome.md`](../../docs/subsystems/minigame-muscle-dome.md).
- `battle_command_ui` - the **battle command menu**'s chip cluster: the blue
  plate 3-slice with its clipped final body tile, the packet-pinned four-arm
  diamond at `(228, 70)` and the `Begin | Run` pair at `(160, 92)`, the D-pad
  glyph they share with `arts_input`, and the `-` chip retail draws for a
  command that cannot be chosen. Both battle hosts seat their command menu
  through it. Geometry mirrors `legaia_engine_vm::battle_chrome`, which
  `engine-shell`'s HUD tests pin equal.
- `battle_hud_chrome` - the battle surfaces that are **widget-table records**
  rather than plate runs: the class-0 **message banner** (`banner_frame` /
  `message_banner_chrome_draws_for` / `message_banner_text_draws_for` - which
  draws no interior fill, and shares content pen `(16, 12)` with the
  actor-name plaque, so the two never coexist) and the **badge cells**
  (`BattleBadgeRects`) the HUD blits for a slot's status element and for an
  actor's element badge.
- `ui_fishing` - fishing-minigame HUD: the ported persistent / catch HUD
  layout, gauge bars, digit field and banner animators, plus
  `fishing_hud_draws_for`, the consumer that renders that draw list.
- `ui_menu_window_painters` (+ `_large`) - content painters for the
  menu-overlay **window-descriptor table**: title tabs, prompt / counter /
  choice windows, the shop's item-info and sell-quantity panels, and the two
  equip stat-compare panels.
- `ui_menu_window_dispatch` - which of those painters draws a given
  descriptor, keyed on its `renderer_va` the way the retail window walker
  keys on the live window's `+0x28`. A host resolves a parsed descriptor
  (`painter_at`) or walks a whole table (`menu_window_painters`) instead of
  hard-coding a screen; the disc oracle is
  `engine-shell/tests/menu_window_dispatch_real.rs`.

`ui_fishing` is the one module that owns both halves. The fishing overlay's
HUD helpers are ports in their own right (`FUN_801d13f0`, `FUN_801d1580`,
`FUN_801d1870`, `FUN_801d76e0`, the banner animators), and they decide layout
rather than simulation state, so they sit beside their consumer here instead
of in `engine-core`, which keeps the minigame's numeric kernels.

Three modules are not UI at all but live here for the same structural reason -
they are wgpu-free draw kernels both hosts must share, and this crate is the
shared leaf:

- `screen_prim` - screen-space PSX primitives (`ScreenPrim` / `ScreenQuad` /
  `FlatQuad`), the four ABR blend classes, and `build_geometry`, the one
  ordering-table walk either host consumes.
- `gte` - fixed-point GTE arithmetic (`q3.12` rotation, `q19.12`
  translation, the UNR divide, NCLIP/AVSZ, register-transfer + memory ops,
  the clean-room `psx_sin` / `psx_cos` trig LUT).
- `vram_capture` - quantising an RGBA8 frame readback to BGR555 and blitting
  it into a `legaia_tim::Vram` rect, plus the transition's capture-rect
  constants.
- `battle_intro` - the field-to-battle transition emitter: per-style working
  sets, the five retail packet builders, the curtain's CPU two-pass
  composition, and the `land_capture_rgba` / `refresh_captured_page` seam a
  host feeds its own frame readback through (native: `capture_rgba`; browser:
  `gl.readPixels`).

## Composition

`legaia-engine-render` re-exports every item here at its historical crate-root
path (`pub use legaia_engine_ui::*`) so native shell code, the asset-viewer, and
tests reference the builders unchanged. The GPU-resident batch wrappers
(`TextOverlay` / `SpriteOverlay` / `UploadedSpriteAtlas`) stay in
`legaia-engine-render` because they hold wgpu handles.

Depends only on `legaia-asset`, `legaia-engine-vm`, `legaia-font`,
`legaia-tim`, `glam`, `serde`, and `bytemuck` - no wgpu, no winit - so it
links into the lean WASM play build.

## Whole-screen compositions

Most builders paint one window. A few paint a **whole sub-screen** - several
windows plus the rules that decide which of them draws - and those exist so the
two hosts cannot answer that question differently.

`recipient_picker_draws_for` is the pattern: the shop's equipment-buy recipient
sub-screen (retail `0x1C`, `FUN_801DB380`) is windows 36 / 25 / 41 together,
and the row order, the "row 0 is the bag" cursor offset, the note precedence
("already equipped" beats "cannot equip") and the compare-category chain are
all screen-level decisions rather than window-level ones. A host supplies the
three rects it resolved off the disc window table plus a plain-data model, and
gets the draws back. The alternative - each host composing the three painters
itself - is what put the browser page a screen ahead of the native window for
a release.

## Host-drift gate

`scripts/ci/check-ui-host-drift.py` treats the set of draw builders here as a
machine-checkable feature surface: a host "has" a screen when its source calls
that builder, transitively through this crate's own builder-to-builder edges.
Native-only is a CI failure unless waived; unused is a failure unless waived;
web-ahead is reported for information. Waivers live in
`scripts/ci/ui-host-drift-waivers.toml` and each must name the capability that
blocks the wire, because the checker validates a waiver's bucket but cannot
read its prose.

What the gate cannot see is drift *inside* a shared builder's inputs - two
hosts calling the same function with different models. Three such gaps were
found by hand rather than by the gate; the two live ones are recorded in the
[web-viewer README](../web-viewer/README.md#platform-drift-against-the-native-window).
