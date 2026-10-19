#!/usr/bin/env python3
"""Generate the crate map: the site's architecture data and the README diagrams.

The workspace already states its own structure, in five places nobody has to
keep in step by hand:

- every `crates/*/Cargo.toml` - the dependency edges and the binaries,
- each `crates/*/src/lib.rs` - the crate's public modules,
- `CLAUDE.md`'s crate tables - one line of scope per crate,
- `scripts/ci/crate-layering.toml` - kernel ranks, roles and the reasons the
  odd edges stand,
- `scripts/ci/crate-map-layout.toml` - the only hand-written input here: which
  band and track a crate is drawn in, the walkthrough and the host notes.

This script joins them and writes

- `site/crate-map.json` - read by `site/js/crate-map.js` on the crate-map page,
- `site/assets/crate-map-{light,dark}.svg` - the layered map in the README,
- `site/assets/pipeline-{light,dark}.svg` - the disc-to-outputs flow in the README.
- the layer table in `README.md`, between its `crate-map:layers` markers.

It fails when the layout table and the workspace disagree: a crate in no band
or in two, a band naming a crate that does not exist, a crate with no row in
CLAUDE.md, or a dependency that points *up* the stack (a crate may depend only
on crates in its own band or a band below it).

    python3 scripts/ci/gen-crate-map.py           # regenerate
    python3 scripts/ci/gen-crate-map.py --check   # exit 1 if any output is stale

Nothing here reads disc data: the outputs are project structure only.
"""

from __future__ import annotations

import html
import json
import re
import sys
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
LAYOUT = REPO / "scripts" / "ci" / "crate-map-layout.toml"
LAYERING = REPO / "scripts" / "ci" / "crate-layering.toml"
CLAUDE_MD = REPO / "CLAUDE.md"
OUT_JSON = REPO / "site" / "crate-map.json"
OUT_ASSETS = REPO / "site" / "assets"
REPO_BLOB = "https://github.com/AndrewAltimit/legend-of-legaia-re/blob/main"


# ---------------------------------------------------------------------------
# Reading the workspace
# ---------------------------------------------------------------------------


def manifest_info(path: Path) -> dict:
    m = tomllib.loads(path.read_text())

    def names(table: dict) -> set[str]:
        return {k.removeprefix("legaia-") for k in table if k.startswith("legaia-")}

    normal: set[str] = set()
    dev: set[str] = set()
    for scope in [m] + list(m.get("target", {}).values()):
        normal |= names(scope.get("dependencies", {}))
        normal |= names(scope.get("build-dependencies", {}))
        dev |= names(scope.get("dev-dependencies", {}))
    bins = [b["name"] for b in m.get("bin", []) if "name" in b]
    crate_dir = path.parent
    if not bins and (crate_dir / "src" / "main.rs").exists():
        bins = [m["package"]["name"]]
    return {
        "package": m["package"]["name"],
        "deps": sorted(normal),
        "dev_deps": sorted(dev - normal),
        "bins": bins,
        "lib": (crate_dir / "src" / "lib.rs").exists(),
    }


PUB_MOD = re.compile(r"^pub mod ([a-z0-9_]+)\s*[;{]", re.M)


def public_modules(crate_dir: Path) -> list[str]:
    lib = crate_dir / "src" / "lib.rs"
    if not lib.exists():
        return []
    return PUB_MOD.findall(lib.read_text())


ROW = re.compile(r"^\| \[`crates/([a-z0-9-]+)`\]\([^)]*\) \| ([^|]*) \| (.*) \|\s*$")


def md_inline(text: str) -> str:
    """CLAUDE.md cell -> small safe HTML: code spans and bold kept, links
    reduced to their text."""
    out = html.escape(text.strip(), quote=False)
    out = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", out)
    out = re.sub(r"`([^`]+)`", r"<code>\1</code>", out)
    out = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", out)
    return out


def claude_scopes() -> dict[str, str]:
    scopes: dict[str, str] = {}
    for line in CLAUDE_MD.read_text().splitlines():
        m = ROW.match(line)
        if m:
            scopes[m.group(1)] = md_inline(m.group(3))
    return scopes


def layering_ranks(rows: dict[str, dict]) -> dict[str, int]:
    ranks: dict[str, int] = {}
    pending = set(rows)
    while pending:
        ready = [c for c in pending if all(d in ranks for d in rows[c].get("deps", []) if d in rows)]
        if not ready:
            raise SystemExit(f"crate-layering.toml has a cycle among: {', '.join(sorted(pending))}")
        for c in ready:
            below = [ranks[d] for d in rows[c].get("deps", []) if d in rows]
            ranks[c] = 1 + max(below) if below else 0
            pending.discard(c)
    return ranks


# ---------------------------------------------------------------------------
# The model
# ---------------------------------------------------------------------------


def build_model() -> tuple[dict, list[str]]:
    layout = tomllib.loads(LAYOUT.read_text())
    layering = tomllib.loads(LAYERING.read_text())
    lrows: dict[str, dict] = layering.get("crates", {})
    ranks = layering_ranks(lrows)
    scopes = claude_scopes()
    errors: list[str] = []

    crates: dict[str, dict] = {}
    for path in sorted(REPO.glob("crates/*/Cargo.toml")):
        info = manifest_info(path)
        info["modules"] = public_modules(path.parent)
        crates[path.parent.name] = info

    overrides = layout.get("crates", {})
    bands_out = []
    placed: dict[str, str] = {}
    for bi, band in enumerate(layout["bands"]):
        rows: list[dict] = []
        if band.get("from_layering"):
            by_rank: dict[int, list[str]] = {}
            for c, r in ranks.items():
                by_rank.setdefault(r, []).append(c)
            for r in sorted(by_rank, reverse=True):
                rows.append({"label": f"rank {r}", "crates": sorted(by_rank[r])})
        else:
            rows.append({"label": "", "crates": list(band["crates"])})
        for row in rows:
            for c in row["crates"]:
                if c not in crates:
                    errors.append(f"band `{band['id']}` names `{c}`, which has no crates/{c}/Cargo.toml")
                    continue
                if c in placed:
                    errors.append(f"`{c}` is in band `{placed[c]}` and band `{band['id']}`")
                placed[c] = band["id"]
                info = crates[c]
                info["band"] = band["id"]
                info["band_index"] = bi
                info["track"] = overrides.get(c, {}).get("track", band["track"])
                if c in lrows:
                    info["rank"] = ranks[c]
                    info["role"] = lrows[c].get("role", "")
        bands_out.append({"id": band["id"], "title": band["title"], "blurb": band["blurb"], "rows": rows})

    for c in sorted(set(crates) - set(placed)):
        errors.append(f"`{c}` is in no band of scripts/ci/crate-map-layout.toml")
    for c in sorted(set(overrides) - set(crates)):
        errors.append(f"[crates] override for `{c}`, which does not exist")
    for c, info in sorted(crates.items()):
        if c not in scopes:
            errors.append(f"`{c}` has no row in CLAUDE.md's crate tables")
        info["scope"] = scopes.get(c, "")
        if c not in placed:
            continue
        for d in info["deps"]:
            if d in crates and d in placed and crates[d]["band_index"] < info["band_index"]:
                errors.append(
                    f"{c} -> {d} points up the stack (band `{info['band']}` uses band "
                    f"`{crates[d]['band']}` above it); move one of them in crate-map-layout.toml"
                )
            if d in crates and info.get("rank") is not None and crates[d].get("rank") is not None:
                if crates[d]["rank"] >= info["rank"]:
                    errors.append(f"{c} -> {d}: kernel rank does not descend")

    dependents: dict[str, list[str]] = {c: [] for c in crates}
    for c, info in crates.items():
        for d in info["deps"]:
            if d in dependents:
                dependents[d].append(c)
    for c, info in crates.items():
        info["dependents"] = sorted(dependents[c])
        info["readme"] = f"{REPO_BLOB}/crates/{c}/README.md"
        info.pop("band_index", None)

    tracks = [{"id": k, **v} for k, v in layout["tracks"].items()]
    for c, info in crates.items():
        if info.get("track") not in layout["tracks"]:
            errors.append(f"`{c}` has unknown track `{info.get('track')}`")

    for step in layout.get("walk", []):
        for c in step["crates"]:
            if c not in crates:
                errors.append(f"walkthrough step `{step['title']}` names unknown crate `{c}`")
    for host in layout.get("hosts", []):
        if host["crate"] not in crates:
            errors.append(f"host `{host['name']}` names unknown crate `{host['crate']}`")

    model = {
        "generated_by": "scripts/ci/gen-crate-map.py",
        "tracks": tracks,
        "bands": bands_out,
        "crates": dict(sorted(crates.items())),
        "couplings": layering.get("coupling", []),
        "hosts": layout.get("hosts", []),
        "walk": layout.get("walk", []),
    }
    return {"model": model, "layout": layout}, errors


# ---------------------------------------------------------------------------
# README SVGs
# ---------------------------------------------------------------------------

THEMES = {
    "light": {
        "bg": "#ffffff",
        "card": "#f6f8fa",
        "text": "#1f2328",
        "muted": "#59636e",
        "border": "#d1d9e0",
        "band": "#ffffff",
        "tracks": {
            "engine": "#0969da",
            "preservation": "#1a7f37",
            "patcher": "#bc4c00",
            "tooling": "#8250df",
        },
        "chip_alpha": "0.10",
    },
    "dark": {
        "bg": "#0d1117",
        "card": "#151b23",
        "text": "#e6edf3",
        "muted": "#9198a1",
        "border": "#3d444d",
        "band": "#0d1117",
        "tracks": {
            "engine": "#4493f8",
            "preservation": "#3fb950",
            "patcher": "#f0883e",
            "tooling": "#ab7df8",
        },
        "chip_alpha": "0.16",
    },
}

MONO = "ui-monospace, SFMono-Regular, Menlo, Consolas, 'Liberation Mono', monospace"
SANS = "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', Helvetica, Arial, sans-serif"
CHAR_W = 7.9  # 13px monospace advance, with a little slack


def esc(s: str) -> str:
    return html.escape(s, quote=True)


def svg_crate_map(model: dict, theme: str) -> str:
    t = THEMES[theme]
    W = 1100
    pad = 20
    label_w = 250
    chips_x = pad + label_w
    chips_w = W - chips_x - pad - 14
    chip_h = 26
    gap = 7
    out: list[str] = []
    y = pad

    # Legend.
    legend: list[str] = []
    lx = pad
    for tr in model["tracks"]:
        col = t["tracks"][tr["id"]]
        legend.append(
            f'<rect x="{lx}" y="{y + 3}" width="14" height="14" rx="3" fill="{col}" fill-opacity="{t["chip_alpha"]}" stroke="{col}" stroke-width="1.5"/>'
            f'<text x="{lx + 21}" y="{y + 15}" class="lg">{esc(tr["title"])}</text>'
        )
        lx += 21 + len(tr["title"]) * 8.2 + 26
    legend.append(
        f'<text x="{W - pad}" y="{y + 15}" class="mu" text-anchor="end">each band uses only the bands below it</text>'
    )
    out.extend(legend)
    y += 34

    for band in model["bands"]:
        band_top = y
        inner: list[str] = []
        cy = y + 12
        for row in band["rows"]:
            cx = chips_x
            row_top = cy
            if row["label"]:
                inner.append(f'<text x="{chips_x - 12}" y="{cy + 16}" class="rk" text-anchor="end">{esc(row["label"])}</text>')
            for c in row["crates"]:
                info = model["crates"][c]
                w = round(len(c) * CHAR_W + 22)
                if cx + w > chips_x + chips_w:
                    cx = chips_x
                    cy += chip_h + gap
                col = t["tracks"][info["track"]]
                inner.append(
                    f'<g><title>{esc(c)}</title><rect x="{cx}" y="{cy}" width="{w}" height="{chip_h}" rx="5" '
                    f'fill="{col}" fill-opacity="{t["chip_alpha"]}" stroke="{col}" stroke-width="1.2"/>'
                    f'<text x="{cx + w / 2}" y="{cy + 17.5}" class="ch" text-anchor="middle">{esc(c)}</text></g>'
                )
                cx += w + gap
            cy += chip_h + gap
            del row_top
        band_h = max(cy - y + 5, 58)
        out.append(
            f'<rect x="{pad}" y="{band_top}" width="{W - 2 * pad}" height="{band_h}" rx="8" fill="{t["band"]}" stroke="{t["border"]}"/>'
        )
        out.append(f'<text x="{pad + 14}" y="{band_top + 24}" class="bt">{esc(band["title"])}</text>')
        n = sum(len(r["crates"]) for r in band["rows"])
        out.append(f'<text x="{pad + 14}" y="{band_top + 42}" class="mu">{n} crate{"s" if n != 1 else ""}</text>')
        out.extend(inner)
        y = band_top + band_h + 10

    H = y + pad - 10
    head = (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" '
        f'role="img" aria-labelledby="t d">'
        f'<title id="t">Legend of Legaia RE crate map</title>'
        f'<desc id="d">The workspace drawn as bands, hosts at the top and the disc parsers at the bottom; '
        f'each crate depends only on crates in its own band or a band below. Colour marks the track.</desc>'
        f"<style>"
        f'text{{font-family:{SANS};fill:{t["text"]}}}'
        f".ch{{font-family:{MONO};font-size:13px}}"
        f".bt{{font-size:15px;font-weight:600}}"
        f'.mu{{font-size:12px;fill:{t["muted"]}}}'
        f'.rk{{font-family:{MONO};font-size:11px;fill:{t["muted"]}}}'
        f".lg{{font-size:14px}}"
        f"</style>"
        f'<rect width="{W}" height="{H}" rx="12" fill="{t["card"]}"/>'
    )
    return head + "".join(out) + "</svg>\n"


def svg_pipeline(layout: dict, theme: str) -> str:
    t = THEMES[theme]
    nodes = {n["id"]: n for n in layout["pipeline"]}
    col_w, row_h = 216, 86
    box_w, box_h = 190, 68
    pad = 20
    cols = 1 + max(n["col"] for n in nodes.values())
    rows = 1 + max(n["row"] for n in nodes.values())
    W = pad * 2 + cols * col_w - (col_w - box_w)
    H = pad * 2 + rows * row_h - (row_h - box_h)
    accent = t["tracks"]["engine"]
    patch = t["tracks"]["patcher"]
    pres = t["tracks"]["preservation"]

    def pos(n):
        return pad + n["col"] * col_w, pad + n["row"] * row_h

    colour = {"port": accent, "patcher": patch, "patched": patch}
    parts: list[str] = []
    for a, b in layout["pipeline_edges"][0]["pairs"]:
        ax, ay = pos(nodes[a])
        bx, by = pos(nodes[b])
        col = patch if a == "patcher" or b == "patcher" else t["muted"]
        if nodes[a]["row"] == nodes[b]["row"]:
            x1, y1 = ax + box_w, ay + box_h / 2
            x2, y2 = bx - 3, by + box_h / 2
            d = f"M{x1},{y1} L{x2},{y2}"
        elif nodes[a]["col"] == nodes[b]["col"]:
            x1, y1 = ax + box_w / 2, ay + box_h
            x2, y2 = bx + box_w / 2, by - 3
            d = f"M{x1},{y1} L{x2},{y2}"
        else:
            x1, y1 = ax + box_w, ay + box_h / 2
            x2, y2 = bx - 3, by + box_h / 2
            if nodes[b]["row"] > nodes[a]["row"] + 1 or a == "prot":
                x1, y1 = ax + box_w / 2, ay + box_h
                d = f"M{x1},{y1} L{x1},{y2 - 10} Q{x1},{y2} {x1 + 10},{y2} L{x2},{y2}"
            else:
                mx = (x1 + x2) / 2
                d = f"M{x1},{y1} C{mx},{y1} {mx},{y2} {x2},{y2}"
        mk = "mp" if col == patch else "mm"
        parts.append(f'<path d="{d}" fill="none" stroke="{col}" stroke-width="1.6" marker-end="url(#{mk})"/>')
    for n in nodes.values():
        x, y = pos(n)
        col = colour.get(n["id"], pres)
        parts.append(
            f'<rect x="{x}" y="{y}" width="{box_w}" height="{box_h}" rx="8" fill="{col}" fill-opacity="{t["chip_alpha"]}" stroke="{col}" stroke-width="1.4"/>'
            f'<text x="{x + box_w / 2}" y="{y + 26}" class="nl" text-anchor="middle">{esc(n["label"])}</text>'
        )
        sub = n["sub"]
        # Two short lines at most for the sub-label.
        lines = [sub]
        if len(sub) * 7.0 > box_w - 14:
            words, lines, cur = sub.split(), [], ""
            for w in words:
                if len(cur + " " + w) * 7.0 > box_w - 14 and cur:
                    lines.append(cur)
                    cur = w
                else:
                    cur = (cur + " " + w).strip()
            lines.append(cur)
        for i, ln in enumerate(lines[:2]):
            parts.append(f'<text x="{x + box_w / 2}" y="{y + 45 + i * 14}" class="ns" text-anchor="middle">{esc(ln)}</text>')

    head = (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" role="img" aria-labelledby="t d">'
        f'<title id="t">From your disc to everything the project makes</title>'
        f"<desc id=\"d\">Your disc image is read into its files, PROT.DAT's entries and decoded assets, which feed "
        f"the port, the viewers and exported files; legaia-patcher writes a patched image or a PPF.</desc>"
        f"<defs>"
        f'<marker id="mm" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="{t["muted"]}"/></marker>'
        f'<marker id="mp" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="{patch}"/></marker>'
        f"</defs>"
        f"<style>"
        f'text{{font-family:{SANS};fill:{t["text"]}}}'
        f".nl{{font-size:15px;font-weight:600}}"
        f'.ns{{font-size:12.5px;fill:{t["muted"]}}}'
        f"</style>"
        f'<rect width="{W}" height="{H}" rx="12" fill="{t["card"]}"/>'
    )
    return head + "".join(parts) + "</svg>\n"


# ---------------------------------------------------------------------------


README = REPO / "README.md"
README_BEGIN = "<!-- crate-map:layers:begin (generated by scripts/ci/gen-crate-map.py; edit crate-map-layout.toml) -->"
README_END = "<!-- crate-map:layers:end -->"


def readme_layers(model: dict) -> str:
    """The README's band-by-band crate list, top of the stack first. Each crate
    links its own README; no counts, so the prose has nothing to rot."""
    # A list, not a table: a band's links wrap over several source lines,
    # which keeps every line under the doc-density gate's length limit.
    lines = [README_BEGIN, ""]
    for band in model["bands"]:
        lines.append(f"- **{band['title']}.** {band['blurb']} \\")
        for row in band["rows"]:
            links = [f"[`{c}`](crates/{c}/README.md)" for c in row["crates"]]
            prefix = f"{row['label'].capitalize()}: " if row["label"] else ""
            chunks = [links[i : i + 5] for i in range(0, len(links), 5)]
            for i, chunk in enumerate(chunks):
                last = i == len(chunks) - 1
                text = ", ".join(chunk) + ("" if last else ",")
                lead = prefix if i == 0 else ""
                # A trailing backslash is a hard line break between rank rows.
                brk = " \\" if last and row["label"] and row is not band["rows"][-1] else ""
                lines.append(f"  {lead}{text}{brk}")
    lines += ["", README_END]
    return "\n".join(lines)


def readme_with_layers(model: dict, errors: list[str]) -> str | None:
    text = README.read_text()
    if README_BEGIN not in text or README_END not in text:
        errors.append("README.md has no crate-map:layers markers")
        return None
    head, rest = text.split(README_BEGIN, 1)
    _, tail = rest.split(README_END, 1)
    return head + readme_layers(model) + tail


def outputs() -> tuple[dict[Path, str], list[str]]:
    built, errors = build_model()
    model, layout = built["model"], built["layout"]
    files: dict[Path, str] = {OUT_JSON: json.dumps(model, indent=1, ensure_ascii=False) + "\n"}
    for theme in THEMES:
        files[OUT_ASSETS / f"crate-map-{theme}.svg"] = svg_crate_map(model, theme)
        files[OUT_ASSETS / f"pipeline-{theme}.svg"] = svg_pipeline(layout, theme)
    readme = readme_with_layers(model, errors)
    if readme is not None:
        files[README] = readme
    return files, errors


def main() -> int:
    check = "--check" in sys.argv[1:]
    files, errors = outputs()
    if errors:
        for e in errors:
            print(f"[crate-map] {e}", file=sys.stderr)
        return 1
    stale = [p for p, text in files.items() if not p.exists() or p.read_text() != text]
    if check:
        if stale:
            for p in stale:
                print(f"[crate-map] stale: {p.relative_to(REPO)}", file=sys.stderr)
            print("[crate-map] run python3 scripts/ci/gen-crate-map.py and commit the result", file=sys.stderr)
            return 1
        return 0
    for p in stale:
        p.write_text(files[p])
        print(f"[crate-map] wrote {p.relative_to(REPO)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
