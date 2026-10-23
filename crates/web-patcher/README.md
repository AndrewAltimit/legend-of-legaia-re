# legaia-web-patcher

The site's client-side ROM patcher, as `#[wasm_bindgen]` exports over
[`legaia-patcher`](../patcher/README.md) and its layer crates. The visitor
picks their own disc image; it is edited in WASM memory and downloaded
back, and nothing is uploaded - the same "user supplies the disc" model as
the `legaia-patcher` CLI. Module by module, the exports are in
[`docs/subsystems/browser-host.md`](../../docs/subsystems/browser-host.md#in-browser-rom-patcher-rom_patcher).

## What lives here

| Module | What it does |
|---|---|
| `rom_patcher` | `patch_rom` (randomizer, sliders, mods, PPF), the option tables the page lists, language-pack import / export, and the texture-replacement entry points. |
| `texture_registry` | The texture families the replacer knows: how each is scanned off the disc, labelled and written back. |
| `texture_pack` | Texture change packs: the user's replacement images, each keyed by the coordinates and a content fingerprint of the retail texture it replaces - never the retail pixels. |
| `translate_workbench` | The in-browser language-pack editor over the user's own disc: one resident parsed disc, the working pack, the SCUS name planner and the retail dialog font. |

## What it must not depend on

**No engine crate.** It sits outside the engine layer beside `patcher`, so
`scripts/ci/check-crate-layering.py` fails if it ever names one. The disc
image is read through the format layer's `legaia_iso::image` and
`legaia_prot::toc_index`.

## The bundle

An `rlib`. The wasm module the site loads is
[`legaia-web-viewer`](../web-viewer/README.md)'s `cdylib`, which names this
crate with `extern crate` (nothing else there references it, and rustc links
only referenced crates), so these exports reach the page through the same
`legaia_web_viewer.js` as the viewer pages'.

## Tests

`tests/` holds the texture-pack round trip, the registry parity tests and the
disc-gated workbench test as one `integration` binary; the disc-gated ones
skip without `LEGAIA_DISC_BIN`. Run one file with
`cargo test -p legaia-web-patcher --test integration <name>::`.
