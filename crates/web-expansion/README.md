# legaia-web-expansion

The site's expansion workbench, as `#[wasm_bindgen]` exports: build,
validate, preview and export an [expansion pack](../../docs/formats/expansion-pack.md)
in the browser against the visitor's own disc. The disc is read in WASM
memory for the pickers and the disc half of validation; nothing is uploaded,
and nothing read from it is written into a pack. Page and exports:
[`docs/tooling/expansion-workbench.md`](../../docs/tooling/expansion-workbench.md).

## What lives here

| Module | What it does |
|---|---|
| `catalog` | `DiscCatalog`: the picker lists read off the disc (monster records with base stats, item and spell names, CDNAME scenes, music), a base scene's geometry pack, and the `DiscResolver` the validator checks coordinates through. |
| `xref` | The workbench's reference tools over the page's pack JSON: Find's site index, the rename plan and rename, the flow graph (`legaia_expansion_format::{xref, flow}`). |
| `session` | `Core`, the native-testable session: pack trees and bundles in (shared bundles through the import gates), the pack's JSON form, the diff against an older bundle and the sources zip, validation over the exported files, the `.lxpack`, and the XP, dialogue and map previews. |
| `wasm` | `ExpansionWorkbench`, the `#[wasm_bindgen]` edge over `Core`: JSON and bytes in and out. |

## Layering

An engine-layer crate above the kernels: the map preview is
`legaia-engine-scene`'s own synthetic field image, a base scene's geometry
pack comes from its scene assembly, and the level curve is
`legaia-engine-expansion`'s `xp_table`. No kernel crate depends on it.

## The bundle

An `rlib`. The wasm module the site loads is
[`legaia-web-viewer`](../web-viewer/README.md)'s `cdylib`, which names this
crate with `extern crate` so its exports are linked.

## Tests

`cargo test -p legaia-web-expansion` runs the Rebirth round trip disc-free;
`tests/disc_catalog.rs` needs `LEGAIA_DISC_BIN`.
