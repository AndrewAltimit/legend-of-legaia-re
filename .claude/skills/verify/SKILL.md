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
2. Serve: `cd site && python3 -m http.server 8749` (python serves `.wasm` with the right MIME).
3. Drive with playwright-core + the cached Playwright Chromium (no full playwright install needed):
   - browsers live at `~/.cache/ms-playwright/chromium-*/chrome-linux/chrome`; pass as `executablePath`, `headless: true`, context `{ acceptDownloads: true }`.
   - real disc for file inputs: `$LEGAIA_DISC_BIN` (`~/.bashrc` exports it). `setInputFiles` with the 700 MB .bin works; first parse takes ~10-60 s.
   - the disc is cached cross-page via rom-cache (IndexedDB), so after one page loads it, other pages in the same context auto-load it.
   - headless-verification hooks: `window.__fsLoad(label)` / `window.__fsState` (viewer full map), `window.__woWalkStamps` (world-overview).
   - rom-patcher page: options live inside collapsed `<details class="rom-group">` - set `d.open = true` before `selectOption`/`check`. Full-disc patch completes in well under a minute; cancel the download events.
4. Working example scripts from a past session: scratchpad `e2e/run.js` + `e2e/run-patcher.js` (glb download + header/JSON-chunk validation, patch-summary assertions).

## Engine / CLI changes

`cargo build --release`, then drive `target/release/legaia-engine play-window ...` or the per-crate CLI named in the crate README. Disc-gated flows need `LEGAIA_DISC_BIN`.
