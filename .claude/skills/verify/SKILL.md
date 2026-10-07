---
name: verify
description: How to runtime-verify changes in this repo - static site (WASM viewer pages) via headless Chromium, engine via play-window, CLIs via target/release.
---

# Verifying changes in legend-of-legaia-re

## Static site (site/ pages + crates/web-viewer WASM)

1. Rebuild WASM + regenerate pages after Rust/site changes:
   ```bash
   bash scripts/ci/build-wasm.sh     # wasm-pack build + sync into site/wasm/
   python3 site/_gen.py              # _content/*.html -> site/*.html (generated pages are gitignored)
   ```
   Run `_gen.py` *after* the wasm build: it writes `LEGAIA_WASM_V` (the content hash `site/js/wasm-loader.js` puts on both wasm URLs), so a stale page pairs the new bundle with an old cache key. Pages load the bundle only through `LegaiaWasm.load()` - see `docs/tooling/site-shell.md`. `python3 scripts/ci/check-wasm-freshness.py` says whether `site/wasm/` still matches the tree.
2. Serve: `cd site && python3 -m http.server 8749` (python serves `.wasm` with the right MIME).
3. Drive with playwright-core + the cached Playwright Chromium (no full playwright install needed):
   - browsers live at `~/.cache/ms-playwright/chromium-*/chrome-linux/chrome` (Linux) or `%LOCALAPPDATA%\ms-playwright\chromium-*\chrome-win\chrome.exe` (Windows); pass as `executablePath`, `headless: true`, context `{ acceptDownloads: true }`.
   - real disc for file inputs: `$LEGAIA_DISC_BIN`. `setInputFiles` with the 700 MB .bin works; first parse takes ~10-60 s.
   - the disc is cached cross-page via rom-cache (IndexedDB), so after one page loads it, other pages in the same context auto-load it.
   - headless-verification hooks: `window.__fsLoad(label)` / `window.__fsState` (viewer full map), `window.__woWalkStamps` (world-overview).
   - rom-patcher page: options live inside collapsed `<details class="rom-group">` - set `d.open = true` before `selectOption`/`check`. Full-disc patch completes in well under a minute; cancel the download events.
4. Keep the driver scripts in the session scratchpad, not the repo; useful checks are a `.glb` download's header + JSON-chunk validation and the rom-patcher's patch-summary assertions.

## Engine / CLI changes

`cargo build --release`, then drive `target/release/legaia-engine play-window ...` or the per-crate CLI named in the crate README (`legaia-engine --help` groups every subcommand). Disc-gated flows need `LEGAIA_DISC_BIN`.

A crate with `autotests = false` in its `Cargo.toml` (every multi-test crate, e.g. `engine-core`, `engine-shell`, `patcher`) compiles its `tests/*.rs` into one `integration` binary, so one test file runs as `cargo test -p <crate> --test integration <file_stem>::`; add `--profile release-test` for CI's profile. The parity oracles live in `crates/parity` and the headless `BootSession` in `crates/engine-session`, but most of their disc-gated tests sit under `crates/engine-shell/tests/`.
