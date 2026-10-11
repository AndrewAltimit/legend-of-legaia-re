# legaia-ui-formats

The formats the game's 2D interface is drawn from.

| Module | Covers |
|---|---|
| `init_pak` | Boot `init.pak`: the publisher-logo TIMs. |
| `title_pak` | Title-screen TIMs. |
| `system_ui_bundle`, `interior_page` | The boot-resident system-UI TIM bundle and its shared interior page. See [`tim-pack.md`](../../docs/formats/tim-pack.md). |
| `menu_glyph_atlas` | The small-caps menu glyph atlas. |
| `save_icon` | The save-slot portrait sheet. See [`save-icon.md`](../../docs/formats/save-icon.md). |
| `screen_elements`, `ui_widgets` | The screen-element placement table and the widget-class table in `SCUS_942.54`. See [`battle-hud.md`](../../docs/subsystems/battle-hud.md). |
| `battle_ui_strings` | The battle screen's command / prompt / banner labels. |
| `tim_palette_context` | Which palette the game draws a given texture with. |

The proportional dialog font is [`legaia-font`](../font/README.md); the menu
overlay's window descriptors and scripts are data tables in
[`legaia-game-tables`](../game-tables/README.md).

## Layering

Sits on [`legaia-container-formats`](../container-formats/README.md) (for
the TIM scanner), `legaia-prot` and `legaia-tim`. It must not depend on the
scene, minigame or battle-model format crates, nor on `legaia-disc-audit`.
`legaia-asset` re-exports each module at its old path.
