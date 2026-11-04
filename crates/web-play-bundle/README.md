# legaia-web-play-bundle

The wasm module the play, minigames and game-modes pages load: a `cdylib` over
[`legaia-web-play`](../web-play/README.md) and
[`legaia-web-minigames`](../web-minigames/README.md) and nothing else, so a player
downloads no inspector, ROM patcher or expansion workbench.

The crate holds no product of its own. It is the link, plus the measurement
build's counting allocator (`bench-alloc`, read by
[`frame-bench-web.mjs`](../../scripts/ci/frame-bench-web.mjs)).

## The two bundles

[`legaia-web-viewer`](../web-viewer/README.md) stays the tool bundle, and it is a
superset: it links `web-play` too, because the viewer pages stand on `web-play`'s
field actor layer, so its exports include everything here.

| Bundle | Root crate | Output | Pages |
|---|---|---|---|
| Tool | `legaia-web-viewer` | `site/wasm/` | Every page not named below |
| Play | `legaia-web-play-bundle` | `site/wasm/play/` | The rows of [`page-bundles.toml`](../../scripts/ci/page-bundles.toml) |

A page does not choose at its call sites. `site/_gen.py` reads the table and tells
the page which bundle it holds, and [`wasm-loader.js`](../../site/js/wasm-loader.js)
loads that one. What a page on this bundle may name is a gate:
[`check-page-export-reach.py`](../../scripts/ci/check-page-export-reach.py).

## Build

[`scripts/ci/build-wasm.sh`](../../scripts/ci/build-wasm.sh) builds both, or one with
`--only play`. What the second build costs is in
[`build-and-test-speed.md`](../../docs/tooling/build-and-test-speed.md#the-wasm-bundle).
