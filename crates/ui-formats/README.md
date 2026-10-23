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
| `name_entry_prompts` | The new-game name-entry screen's prompts. |
| `tim_palette_context` | Which palette the game draws a given texture with. |

The proportional dialog font is [`legaia-font`](../font/README.md); the menu
overlay's window descriptors and scripts are data tables in
[`legaia-game-tables`](../game-tables/README.md).

## Layering

Sits on [`legaia-container-formats`](../container-formats/README.md) (for
the TIM scanner), `legaia-prot` and `legaia-tim`. It must not depend on the
scene, minigame or actor format crates, nor on `legaia-disc-audit`.

## Module reference

Entry points, CLI subcommands (`asset ...` is the binary in
[`crates/asset`](../asset/README.md)) and the doc page each module is specified on.

| Module | Covers |
|---|---|
| `interior_page` | The shared interior texture page: a 256×256 4bpp TIM at image `(960,256)` whose declared CLUT block uploads as a flat 256-entry strip on **row 510**. Member 8 of the raw-TOC-entry-0 TIM-pack (PROT.DAT byte `0x11218` - the head region extraction indexing skips), read straight off `PROT.DAT` (`read_from_prot_dat` / `upload_to_vram`). Sampled by town env meshes (town01 incl. the spawn plaza). |
| `system_ui_bundle` | The whole boot-resident **system-UI TIM bundle** = the `prot::timpack`s at raw PROT TOC entries 0/1 (menu-glyph atlas, sprite sheets, cursor parts, six bare atlas row-patch blocks, the raw-entry-1 row-480 UI page). `upload_to_vram` reproduces the retail `FUN_800198E0` boot upload: pack-member order, image at declared rect, CLUT as a flattened `(x, y, w*h, 1)` strip. Layered under every engine scene build via `SceneResources` `BuildOptions::system_ui`. See [`tim-pack.md`](../../docs/formats/tim-pack.md). |
| `init_pak` | The four publisher-logo TIMs from PROT 0895 (CDNAME says `bat_back_dat`; actually init.pak). `parse` doubles as the `categorize` detector for `Class::InitPak`. |
| `title_pak` | The "Legend of Legaia" title-screen TIM + the system-UI sheet (load-screen panel / slot pills). |
| `menu_glyph_atlas` | The small-caps menu font atlas (title menu rows + shared menu UI). |
| `save_icon` | The **save-slot portrait sheet**: one 4bpp TIM in PROT 899 (`0x1F908`, image `(960,224)` 64x16, CLUT `(0,1)` 256x1) holding sixteen 16x16 tiles row-interleaved across a 256-px strip, one 16-colour palette per tile. Owns the de-interleave to PSX save-block tile layout. Tile N is the memory-card icon for save number N+1; tile 15 is blank padding. See [`save-icon.md`](../../docs/formats/save-icon.md). |
| `ui_widgets` | The **widget-class table** (`SCUS_942.54` VA `0x800732A4`, `0x0C` stride, `0x9D` records) the whole 2-D chrome draws itself out of, plus the frame tile-set pool behind it. Per record: layout class, tile-set, signed chain delta, packed CLUT byte, sheet rect and seat bias. A `screen_elements` record's `+0x0E` kind byte is an index into it. See [`battle.md`](../../docs/subsystems/battle.md#the-widget-class-table---where-every-chrome-sprite-comes-from). |
| `tim_palette_context` | Which palette the game really draws a texture with: the boot-VRAM CLUT state after the system-UI bundle upload (`BootClutVram` - which TIMs lose their own palette at boot, which cells a row holds), the system-UI page's rectangles from the SCUS widget table (`sheet_palette_regions`), the one per-texel palette attribution (`texel_palettes`: overlap precedence, the button-glyph cover), and an "as drawn" composite decode (`composite_rgba`). Drives the asset viewer's palette list and the ROM patcher's texture map; see [`shading.md`](../../docs/subsystems/shading.md). |
| `battle_ui_strings` | Pinned disc coordinates of the battle command / prompt / banner labels, each with the code or data site that forms its address (`SCUS_REFS`, `OVERLAY_REFS`) so a label a language pack moved still reads. |
| `name_entry_prompts` | The name-entry prompts and its `Yes` / `No`, read through the field overlay's (PROT 0897) seven-word pointer table at `0x801F3324`, which is each prompt's only reference. |
