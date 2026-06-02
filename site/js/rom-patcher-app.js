/* In-browser ROM patcher: drive the Track-1 randomizer (compiled to WASM) on a
 * user-supplied disc image, entirely client-side, and download the patched
 * image. Nothing is uploaded; the disc bytes never leave the browser.
 *
 * The WASM module (legaia_web_viewer) exposes `patch_rom(image, seed, drops,
 * encounters, chests, steals, doors, door_coupling) -> { data, summary, seed }`
 * and `resolve_seed(str)`.
 * Imports resolve relative to THIS file (site/js/), so the package at
 * site/wasm/ is `../wasm/...`.
 */

let wasmMod = null;

async function ensureWasm(setStatus) {
  if (wasmMod) return wasmMod;
  setStatus('Loading patcher (WASM) ...');
  wasmMod = await import('../wasm/legaia_web_viewer.js');
  await wasmMod.default();
  return wasmMod;
}

function $(id) {
  return document.getElementById(id);
}

function triggerDownload(bytes, filename) {
  const blob = new Blob([bytes], { type: 'application/octet-stream' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  // Revoke a tick later so the download has started.
  setTimeout(() => URL.revokeObjectURL(url), 4000);
}

function patchedName(original, seed) {
  const base = (original || 'disc.bin').replace(/\.bin$/i, '');
  return `${base}.legaia-rando-${seed}.bin`;
}

function init() {
  const fileInput = $('rom-file');
  const seedInput = $('rom-seed');
  const dropsSel = $('rom-drops');
  const encSel = $('rom-encounters');
  const chestSel = $('rom-chests');
  const stealSel = $('rom-steals');
  const doorSel = $('rom-doors');
  const doorCouplingSel = $('rom-door-coupling');
  const runBtn = $('rom-run');
  const statusEl = $('rom-status');
  const summaryEl = $('rom-summary');
  if (!fileInput || !runBtn) return; // not on this page

  const setStatus = (msg, kind) => {
    statusEl.textContent = msg;
    statusEl.className = 'rom-status' + (kind ? ' rom-status-' + kind : '');
  };

  runBtn.addEventListener('click', async () => {
    const file = fileInput.files && fileInput.files[0];
    if (!file) {
      setStatus('Choose a disc image (.bin) first.', 'err');
      return;
    }
    const drops = dropsSel.value;
    const encounters = encSel.value;
    const chests = chestSel.value;
    const steals = stealSel ? stealSel.value : 'none';
    const doors = doorSel ? doorSel.value : 'none';
    const doorCoupling = doorCouplingSel ? doorCouplingSel.value : 'coupled';
    if (
      drops === 'none' &&
      encounters === 'none' &&
      chests === 'none' &&
      steals === 'none' &&
      doors === 'none'
    ) {
      setStatus('Enable at least one of drops / encounters / chests / steals / doors.', 'err');
      return;
    }
    const seed = (seedInput.value || '').trim() || String(Date.now());

    runBtn.disabled = true;
    summaryEl.textContent = '';
    try {
      const mod = await ensureWasm(setStatus);
      setStatus('Reading disc image ...');
      const buf = new Uint8Array(await file.arrayBuffer());
      setStatus('Patching (this can take a moment for a full disc) ...');
      // Yield so the status paints before the synchronous WASM call.
      await new Promise((r) => setTimeout(r, 30));
      const result = mod.patch_rom(buf, seed, drops, encounters, chests, steals, doors, doorCoupling);
      const data = result.data;
      const usedSeed = result.seed;
      const name = patchedName(file.name, usedSeed);
      triggerDownload(data, name);
      setStatus('Done. Downloaded ' + name, 'ok');
      summaryEl.textContent =
        'seed: ' + usedSeed + '\n' + (result.summary || '');
    } catch (e) {
      setStatus('Error: ' + (e && e.message ? e.message : e), 'err');
    } finally {
      runBtn.disabled = false;
    }
  });

  // Live-resolve the seed string to its numeric value as a hint.
  seedInput.addEventListener('change', async () => {
    const s = (seedInput.value || '').trim();
    if (!s) return;
    try {
      const mod = await ensureWasm(setStatus);
      setStatus('seed "' + s + '" -> ' + mod.resolve_seed(s));
    } catch {
      /* ignore */
    }
  });
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', init);
} else {
  init();
}
