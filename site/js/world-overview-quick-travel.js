/* world-overview-quick-travel.js - the world-overview page's quick-travel
 * menu panel.
 *
 * Draws the in-game world-map menu's records (parsed live from SCUS_942.54)
 * onto the panel's canvas, with a hover readout and a placeholder until a
 * disc is loaded. The panel keeps its own state; the page hands it the
 * parsed menu through LegaiaWorldOverviewQuickTravel.renderQuickTravel.
 *
 * Classic script; loads before world-overview-app.js. */

(function () {
  /* ---------- Quick-travel menu overlay (whole Legaia) ---------- */
  /* Visual style guide for the 20-record overlay - subtle but readable. */
  const QT_DOT_COLOR = '#F4B41A';
  const QT_DOT_RADIUS = 5;
  let qtRecords = null;  /* { names: string[], placements: [{name_idx, ...}] } */
  let qtScreen = [];     /* [{ x, y, p, name }] hit-test entries in canvas coords */

  function renderQuickTravel(menu) {
    qtRecords = menu;
    const $qtCanvas   = document.getElementById('wo-qt-canvas');
    const $qtNameList = document.getElementById('wo-qt-namelist');
    if (!$qtCanvas || !$qtNameList) return;
    /* Sidebar: 16 names with placement counts (sub-entries per landmark). */
    const counts = new Array(menu.names.length).fill(0);
    menu.placements.forEach(p => { counts[p.name_idx]++; });
    $qtNameList.innerHTML = menu.names.map((name, i) => {
      const c = counts[i];
      const tag = c === 0
        ? '<span style="color:#888">(no record)</span>'
        : `<span class="wo-coord">x${c}</span>`;
      return `<li><span class="wo-name">[0x${i.toString(16).padStart(2,'0').toUpperCase()}] ${escapeHtml(name)}</span> ${tag}</li>`;
    }).join('');
    /* Canvas: scale menu_x/menu_y (PSX menu screen space, ~128x128) to
     * canvas pixels. menu_x range observed 22..102, menu_y 13..108. */
    drawQuickTravelCanvas();
  }
  function drawQuickTravelCanvas() {
    const $qtCanvas = document.getElementById('wo-qt-canvas');
    if (!$qtCanvas || !qtRecords) return;
    const ctx = $qtCanvas.getContext('2d');
    const W = $qtCanvas.width, H = $qtCanvas.height;
    ctx.fillStyle = '#0a0a1a'; ctx.fillRect(0, 0, W, H);
    /* Margin + scale from a 0..128 menu-coord box to canvas. */
    const margin = 30;
    const aW = W - margin * 2;
    const aH = H - margin * 2;
    const sx = aW / 128;
    const sy = aH / 128;
    /* Grid */
    ctx.strokeStyle = 'rgba(120,130,200,0.12)';
    for (let i = 0; i <= 8; i++) {
      ctx.beginPath();
      ctx.moveTo(margin + (i/8)*aW, margin);
      ctx.lineTo(margin + (i/8)*aW, margin + aH);
      ctx.moveTo(margin, margin + (i/8)*aH);
      ctx.lineTo(margin + aW, margin + (i/8)*aH);
      ctx.stroke();
    }
    qtScreen = [];
    /* Draw dots first so labels render on top. */
    qtRecords.placements.forEach(p => {
      const x = margin + p.menu_x * sx;
      const y = margin + p.menu_y * sy;
      ctx.beginPath();
      ctx.arc(x, y, QT_DOT_RADIUS, 0, 2*Math.PI);
      ctx.fillStyle = QT_DOT_COLOR; ctx.fill();
      ctx.strokeStyle = 'rgba(0,0,0,0.5)'; ctx.lineWidth = 1; ctx.stroke();
      qtScreen.push({ x, y, p, name: qtRecords.names[p.name_idx] || '?' });
    });
    /* Label one entry per landmark (the first record we see for each
     * name_idx). De-dupes the closely-grouped sub-entry triplets. */
    ctx.font = '11px monospace';
    ctx.fillStyle = '#ddd';
    ctx.textBaseline = 'middle';
    const labeled = new Set();
    qtScreen.forEach(d => {
      if (labeled.has(d.p.name_idx)) return;
      labeled.add(d.p.name_idx);
      const label = d.name;
      const tx = d.x + QT_DOT_RADIUS + 4;
      const tw = ctx.measureText(label).width;
      /* Background pill so labels stay readable over the grid. */
      ctx.fillStyle = 'rgba(10,10,26,0.78)';
      ctx.fillRect(tx - 2, d.y - 7, tw + 4, 14);
      ctx.fillStyle = '#ddd';
      ctx.fillText(label, tx, d.y);
    });
  }
  /* Hover tooltip over the quick-travel canvas. */
  const $qtCanvasEl = document.getElementById('wo-qt-canvas');
  const $qtHover    = document.getElementById('wo-qt-hover');
  if ($qtCanvasEl && $qtHover) {
    $qtCanvasEl.addEventListener('mousemove', ev => {
      const rect = $qtCanvasEl.getBoundingClientRect();
      const mx = (ev.clientX - rect.left) * ($qtCanvasEl.width / rect.width);
      const my = (ev.clientY - rect.top) * ($qtCanvasEl.height / rect.height);
      let best = null, bestD = Infinity;
      for (const d of qtScreen) {
        const dx = d.x - mx, dy = d.y - my;
        const r2 = dx*dx + dy*dy;
        if (r2 < bestD) { bestD = r2; best = d; }
      }
      if (best && bestD < 96) {
        const p = best.p;
        $qtHover.textContent =
          `${best.name}  -  rec ${p.index}  menu (${p.menu_x}, ${p.menu_y})  ` +
          `scene_id=0x${p.scene_id.toString(16).padStart(4,'0').toUpperCase()}  ` +
          `flag=0x${p.discovery_flag.toString(16).padStart(2,'0').toUpperCase()}`;
      } else {
        $qtHover.innerHTML = '&nbsp;';
      }
    });
    $qtCanvasEl.addEventListener('mouseleave', () => { $qtHover.innerHTML = '&nbsp;'; });
  }
  /* Initial placeholder. */
  (function paintQtPlaceholder() {
    const $c = document.getElementById('wo-qt-canvas');
    if (!$c) return;
    const ctx = $c.getContext('2d');
    ctx.fillStyle = '#0a0a1a'; ctx.fillRect(0, 0, $c.width, $c.height);
    ctx.fillStyle = '#888'; ctx.font = '13px monospace';
    ctx.textAlign = 'center';
    ctx.fillText('Load a disc above to see the in-game world-map menu', $c.width / 2, $c.height / 2 - 10);
    ctx.fillText('(parsed live from SCUS_942.54)', $c.width / 2, $c.height / 2 + 10);
  })();

  window.LegaiaWorldOverviewQuickTravel = { renderQuickTravel };
})();
