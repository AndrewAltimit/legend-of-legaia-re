# legaia-scene-formats

The formats a field or world-map scene is built from.

| Group | Modules |
|---|---|
| Scene bundle shapes | `scene_asset_table`, `scene_scripted_asset_table`, `scene_event_scripts`, `scene_tmd_stream`, `scene_v12_table`, `scene_vdf`, `field_pack`. See [`scene-bundles.md`](../../docs/formats/scene-bundles.md), [`scene-v12-table.md`](../../docs/formats/scene-v12-table.md). |
| MAN | `man_section` (section walker), `man_edit` (variable-length editing, [`man-relocation.md`](../../docs/formats/man-relocation.md)), `man_motion` (motion-VM script table). |
| Field-VM bytecode as data | `field_disasm`, the side-effect-free disassembler and the disc-wide opcode census ([`field-op-census.md`](../../docs/tooling/field-op-census.md)). |
| Field geometry | `field_objects`, `field_map` ([`field-map.md`](../../docs/formats/field-map.md)), `stage_geom`, `battle_backdrop`. |
| Carried inline by a scene script | `shop_stock`, `inn_costs`, `place_names` ([`place-names.md`](../../docs/formats/place-names.md)), `cutscene_text`. |
| World map | `kingdom_bundle`, `world_map_overlay` ([`world-map-overlay.md`](../../docs/formats/world-map-overlay.md)), `clut_walk`, `ocean`. |

## Layering

Sits on [`legaia-container-formats`](../container-formats/README.md) and the
leaf codecs (`legaia-lzs`, `legaia-tim`, `legaia-tmd`, `legaia-bytes`). The one
sideways edge is [`legaia-overlay-images`](../overlay-images/README.md):
`scene_event_scripts` reads the summon part records an event script can
spawn. It must not depend on the UI, minigame or battle-model format crates,
nor on `legaia-disc-audit`. `legaia-asset` re-exports each module at its old
path.
