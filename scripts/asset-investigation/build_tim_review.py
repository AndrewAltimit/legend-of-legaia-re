#!/usr/bin/env python3
"""Generate a local grid UI for human review of one TIM label category.

Pairs with `asset tim-render-distinct` (the decoded PNGs) and
`crates/asset/src/data/tim_categories.tsv` (the curated labels). Emits a
self-contained `review.html` into the render dir: a thumbnail grid where every
distinct texture currently labeled the TARGET category is pre-selected. Click a
cell to toggle; filter to scan quickly; "Download selection" saves the chosen
fingerprints. Feed that file to `scripts/asset-investigation/apply_tim_review.py` to write the
labels back. Because this grid pre-selects every current member of the
category, the downloaded selection is the WHOLE category — so pass
`--allow-demotions` to apply_tim_review for this full-review workflow if you
want deselected cells to fall back to "other".

The PNGs are decoded pixel data and stay local; only the resulting
fingerprint->label table is committed. review.html lives beside them (local).

Usage:
    python3 scripts/asset-investigation/build_tim_review.py <render_dir> <category> [--table PATH]
"""
import argparse
import html
import json
import os
import sys

VOCAB = [
    "environment",
    "terrain",
    "foliage",
    "character",
    "ui-text",
    "effect",
    "other",
]


def load_labels(table_path):
    """fingerprint -> label from the curated table (data rows only)."""
    labels = {}
    with open(table_path) as f:
        for line in f:
            s = line.rstrip("\n")
            if s.startswith("#") or s.startswith("fnv1a") or not s.strip():
                continue
            cols = s.split("\t")
            labels[cols[0].strip()] = cols[1].strip()
    return labels


def load_manifest(render_dir):
    rows = []
    with open(os.path.join(render_dir, "manifest.tsv")) as f:
        next(f)
        for line in f:
            p = line.rstrip("\n").split("\t")
            if len(p) < 6:
                continue
            rows.append({"fnv": p[0], "w": int(p[2]), "h": int(p[3]), "bpp": int(p[4])})
    rows.sort(key=lambda r: r["fnv"])
    return rows


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("render_dir")
    ap.add_argument("category", choices=VOCAB)
    ap.add_argument(
        "--table",
        default="crates/asset/src/data/tim_categories.tsv",
        help="curated label table",
    )
    args = ap.parse_args()

    labels = load_labels(args.table)
    rows = load_manifest(args.render_dir)
    # Attach current label; only cells that have a PNG are reviewable.
    cells = []
    for r in rows:
        if not os.path.exists(os.path.join(args.render_dir, r["fnv"] + ".png")):
            continue
        cells.append(
            {
                "fnv": r["fnv"],
                "w": r["w"],
                "h": r["h"],
                "bpp": r["bpp"],
                "label": labels.get(r["fnv"], ""),
            }
        )

    cat = args.category
    pre = sum(1 for c in cells if c["label"] == cat)
    data_json = json.dumps(cells, separators=(",", ":"))

    page = HTML_TEMPLATE.replace("__CATEGORY__", html.escape(cat))
    page = page.replace("__PRECOUNT__", str(pre))
    page = page.replace("__TOTAL__", str(len(cells)))
    page = page.replace("__VOCAB__", json.dumps(VOCAB))
    page = page.replace("__DATA__", data_json)

    out = os.path.join(args.render_dir, "review.html")
    with open(out, "w") as f:
        f.write(page)
    print(f"wrote {out}")
    print(f"  {len(cells)} reviewable textures; {pre} pre-selected as '{cat}'")
    print(f"  open: file://{os.path.abspath(out)}")
    return 0


HTML_TEMPLATE = r"""<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<title>TIM review - __CATEGORY__</title>
<style>
  :root { color-scheme: dark; }
  body { background:#0a0e15; color:#cdd6e0; font-family:system-ui,sans-serif; margin:0; }
  header { position:sticky; top:0; z-index:10; background:#0d1320; border-bottom:1px solid #243; padding:8px 12px; display:flex; gap:12px; align-items:center; flex-wrap:wrap; }
  header h1 { font-size:15px; margin:0; font-weight:600; }
  header .cat { color:#9ccc7a; }
  button, select { background:#16202c; color:#cdd6e0; border:1px solid #2c3a4a; border-radius:4px; padding:5px 9px; font-size:13px; cursor:pointer; }
  button:hover { border-color:#6ab0f3; }
  button.primary { background:#2c5a2c; border-color:#3a7a3a; }
  #count { font-variant-numeric:tabular-nums; }
  #grid { display:grid; grid-template-columns:repeat(auto-fill,minmax(96px,1fr)); gap:6px; padding:10px; }
  .cell { position:relative; background:#11161f; border:2px solid transparent; border-radius:4px; padding:4px; cursor:pointer; display:flex; flex-direction:column; align-items:center; min-height:118px; }
  .cell img { width:100%; height:auto; max-height:84px; object-fit:contain; image-rendering:pixelated; background:#0a0e15; }
  .cell .meta { font-size:10px; color:#7e8a99; margin-top:3px; text-align:center; line-height:1.15; font-family:monospace; }
  .cell.sel { border-color:#5fd35f; background:#14210f; }
  .cell .chk { position:absolute; top:3px; left:3px; width:18px; height:18px; border-radius:3px; background:#243; color:#9aa; font-size:13px; line-height:18px; text-align:center; }
  .cell.sel .chk { background:#5fd35f; color:#06210a; }
  .hint { color:#7e8a99; font-size:12px; }
</style></head><body>
<header>
  <h1>Mark all <span class="cat">__CATEGORY__</span> textures</h1>
  <span id="count">selected: __PRECOUNT__ / __TOTAL__</span>
  <label class="hint">show
    <select id="filter">
      <option value="all">all</option>
      <option value="sel">selected</option>
      <option value="unsel">unselected</option>
      <optgroup label="by current label" id="labelopts"></optgroup>
    </select>
  </label>
  <button id="selshown">Select shown</button>
  <button id="clrshown">Clear shown</button>
  <button id="reset">Reset to current</button>
  <button id="dl" class="primary">Download selection</button>
  <span class="hint">click a cell to toggle &middot; saves <code>__CATEGORY___selection.txt</code></span>
</header>
<div id="grid"></div>
<script>
const CAT = "__CATEGORY__";
const VOCAB = __VOCAB__;
const DATA = __DATA__;          // [{fnv,w,h,bpp,label}]
const sel = new Set(DATA.filter(d => d.label === CAT).map(d => d.fnv));
const grid = document.getElementById('grid');
const countEl = document.getElementById('count');
const filterEl = document.getElementById('filter');

// populate by-label filter options
const lo = document.getElementById('labelopts');
for (const v of VOCAB) { const o=document.createElement('option'); o.value='label:'+v; o.textContent=v; lo.appendChild(o); }
{ const o=document.createElement('option'); o.value='label:'; o.textContent='(unlabeled)'; lo.appendChild(o); }

function updateCount(){ countEl.textContent = `selected: ${sel.size} / ${DATA.length}`; }

const cellEls = new Map();
function build(){
  const frag = document.createDocumentFragment();
  for (const d of DATA){
    const c = document.createElement('div');
    c.className = 'cell' + (sel.has(d.fnv) ? ' sel' : '');
    c.dataset.fnv = d.fnv;
    c.dataset.label = d.label;
    c.innerHTML = `<div class="chk">${sel.has(d.fnv)?'✓':''}</div>`
      + `<img loading="lazy" src="${d.fnv}.png" alt="">`
      + `<div class="meta">${d.w}x${d.h} ${d.bpp}b<br>${d.label||'—'}</div>`;
    c.addEventListener('click', () => toggle(d.fnv));
    frag.appendChild(c);
    cellEls.set(d.fnv, c);
  }
  grid.appendChild(frag);
  updateCount();
}
function toggle(fnv){
  const c = cellEls.get(fnv);
  if (sel.has(fnv)){ sel.delete(fnv); c.classList.remove('sel'); c.querySelector('.chk').textContent=''; }
  else { sel.add(fnv); c.classList.add('sel'); c.querySelector('.chk').textContent='✓'; }
  updateCount();
}
function applyFilter(){
  const f = filterEl.value;
  for (const [fnv,c] of cellEls){
    let show = true;
    if (f==='sel') show = sel.has(fnv);
    else if (f==='unsel') show = !sel.has(fnv);
    else if (f.startsWith('label:')) show = (c.dataset.label === f.slice(6));
    c.style.display = show ? '' : 'none';
  }
}
function shownFnvs(){ return [...cellEls].filter(([_,c])=>c.style.display!=='none').map(([f])=>f); }

filterEl.addEventListener('change', applyFilter);
document.getElementById('selshown').onclick = ()=>{ for(const f of shownFnvs()) if(!sel.has(f)) toggle(f); };
document.getElementById('clrshown').onclick = ()=>{ for(const f of shownFnvs()) if(sel.has(f)) toggle(f); };
document.getElementById('reset').onclick = ()=>{
  sel.clear(); for(const d of DATA) if(d.label===CAT) sel.add(d.fnv);
  for(const [fnv,c] of cellEls){ const on=sel.has(fnv); c.classList.toggle('sel',on); c.querySelector('.chk').textContent=on?'✓':''; }
  updateCount();
};
document.getElementById('dl').onclick = ()=>{
  const lines = ['# category='+CAT, ...[...sel].sort()];
  const blob = new Blob([lines.join('\n')+'\n'], {type:'text/plain'});
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob); a.download = CAT+'_selection.txt'; a.click();
  URL.revokeObjectURL(a.href);
};
build();
</script></body></html>
"""


if __name__ == "__main__":
    sys.exit(main())
