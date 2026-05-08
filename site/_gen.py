#!/usr/bin/env python3
"""Generate the multi-page site from per-page content fragments.

Layout is shared via JS (site/js/layout.js), so each generated HTML file is
just <head> + <main> with the page-specific content. The sidebar nav, TOC
rail, and prev/next footer are injected at runtime by layout.js.

Also writes site/search-index.json: one entry per (page, h2/h3 heading)
plus one root entry per page. Drives the in-page search overlay.

Run from the repo root:
    python3 site/_gen.py
"""
from __future__ import annotations
import json
import re
import sys
from html.parser import HTMLParser
from pathlib import Path

ROOT = Path(__file__).resolve().parent
CONTENT = ROOT / "_content"


def html_template(title: str, depth: int, active_key: str, body: str, extra_head: str = "") -> str:
    css = "../" * depth + "css/styles.css"
    layout_js = "../" * depth + "js/layout.js"
    main_js = "../" * depth + "js/main.js"
    favicon = "../" * depth + "img/favicon.svg"
    return f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{title} — legend-of-legaia-re</title>
  <link rel="icon" href="{favicon}" type="image/svg+xml">
  <link rel="stylesheet" href="{css}">
  {extra_head}
</head>
<body>
<a class="skip-link" href="#content">Skip to content</a>
<div class="app">
<main class="content" id="content">
{body}
</main>
</div>
<script src="{layout_js}"></script>
<script>injectLayout({{ active: {active_key!r} }});</script>
<script src="{main_js}"></script>
</body>
</html>
"""


# (out_path, title, active_key, body_file)
PAGES: list[tuple[str, str, str, str]] = [
    # depth = 0 (root)
    ("index.html",                 "Home",                          "home",                       "home.html"),
    ("architecture.html",          "How the layers stack",          "architecture",               "architecture.html"),
    ("quickstart.html",            "Quick start",                   "quickstart",                 "quickstart.html"),
    ("viewer.html",                "Asset viewer (WASM)",           "viewer",                     "viewer.html"),
    # depth = 1
    ("subsystems/index.html",      "Subsystems",                    "subsystems/index",           "subsystems/index.html"),
    ("subsystems/boot.html",       "Boot path",                     "subsystems/boot",            "subsystems/boot.html"),
    ("subsystems/asset-loader.html","Asset loader",                 "subsystems/asset-loader",    "subsystems/asset-loader.html"),
    ("subsystems/script-vm.html",  "Field / event script VM",       "subsystems/script-vm",       "subsystems/script-vm.html"),
    ("subsystems/actor-vm.html",   "Actor / sprite VM",             "subsystems/actor-vm",        "subsystems/actor-vm.html"),
    ("subsystems/move-vm.html",    "Move-table VM",                 "subsystems/move-vm",         "subsystems/move-vm.html"),
    ("subsystems/motion-vm.html",  "Motion VM (camera / NPC)",      "subsystems/motion-vm",       "subsystems/motion-vm.html"),
    ("subsystems/effect-vm.html",  "Effect VM",                     "subsystems/effect-vm",       "subsystems/effect-vm.html"),
    ("subsystems/battle.html",     "Battle",                        "subsystems/battle",          "subsystems/battle.html"),
    ("subsystems/battle-action.html","Battle action state machine", "subsystems/battle-action",   "subsystems/battle-action.html"),
    ("subsystems/battle-formulas.html","Battle formulas",            "subsystems/battle-formulas", "subsystems/battle-formulas.html"),
    ("subsystems/audio.html",      "Audio",                         "subsystems/audio",           "subsystems/audio.html"),
    ("subsystems/renderer.html",   "Renderer",                      "subsystems/renderer",        "subsystems/renderer.html"),
    ("subsystems/world-map.html",  "World map",                     "subsystems/world-map",       "subsystems/world-map.html"),
    ("subsystems/save-screen.html","Save screen",                   "subsystems/save-screen",     "subsystems/save-screen.html"),
    ("subsystems/shop.html",       "Shop",                          "subsystems/shop",            "subsystems/shop.html"),
    ("subsystems/inn.html",        "Inn",                           "subsystems/inn",             "subsystems/inn.html"),
    ("subsystems/level-up.html",   "Level-up",                      "subsystems/level-up",        "subsystems/level-up.html"),
    ("subsystems/cutscene.html",   "Cutscene (STR mode)",           "subsystems/cutscene",        "subsystems/cutscene.html"),
    ("subsystems/engine.html",     "Engine reimplementation",       "subsystems/engine",          "subsystems/engine.html"),
    ("formats/index.html",         "Formats",                       "formats/index",              "formats/index.html"),
    ("tooling/index.html",         "Tooling",                       "tooling/index",              "tooling/index.html"),
    ("reference/index.html",       "Reference",                     "reference/index",            "reference/index.html"),
    ("reference/functions.html",   "Key functions",                 "reference/functions",        "reference/functions.html"),
    ("reference/memory-map.html",  "PSX RAM map",                   "reference/memory-map",       "reference/memory-map.html"),
]


# ---------------------------------------------------------------------------
# Search-index extraction
# ---------------------------------------------------------------------------

class _IndexParser(HTMLParser):
    """Extract: lede paragraph, h2/h3 headings, and surrounding text snippets."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.lede: list[str] = []
        self.headings: list[dict] = []  # [{level, text, id, snippet}]
        self._current_heading: dict | None = None
        self._capture_into: list[str] | None = None
        self._lede_open = False
        self._h_open = False
        self._h_level = 0
        self._h_attrs: dict[str, str] = {}
        self._section_id: str | None = None
        self._snippet_buf: list[str] = []
        self._section_text_buf: list[str] = []

    def handle_starttag(self, tag, attrs):
        attrs_d = dict(attrs)
        if tag == "p" and self._lede_open is False and "lede" in (attrs_d.get("class") or ""):
            self._lede_open = True
            self._capture_into = self.lede
        elif tag in ("h2", "h3"):
            # close any prior heading: store accumulated snippet
            if self._current_heading is not None:
                self._current_heading["snippet"] = " ".join(self._snippet_buf).strip()[:200]
                self.headings.append(self._current_heading)
                self._current_heading = None
            self._h_open = True
            self._h_level = 2 if tag == "h2" else 3
            self._h_attrs = attrs_d
            self._capture_into = []
            self._snippet_buf = []
        elif tag == "section" and "doc-section" in (attrs_d.get("class") or ""):
            self._section_id = attrs_d.get("id")

    def handle_endtag(self, tag):
        if tag == "p" and self._lede_open:
            self._lede_open = False
            self._capture_into = None
        elif tag in ("h2", "h3") and self._h_open:
            text = "".join(self._capture_into or []).strip()
            self._h_open = False
            heading_id = self._h_attrs.get("id") or self._section_id or _slugify(text)
            self._current_heading = {
                "level": self._h_level,
                "text": text,
                "id": heading_id,
            }
            self._capture_into = None
            # subsequent text feeds into snippet
            self._snippet_buf = []
        elif tag == "section" and self._current_heading is not None:
            # finalize last heading on section close
            self._current_heading["snippet"] = " ".join(self._snippet_buf).strip()[:200]
            self.headings.append(self._current_heading)
            self._current_heading = None
            self._snippet_buf = []
            self._section_id = None

    def handle_data(self, data):
        if self._capture_into is not None:
            self._capture_into.append(data)
        elif self._current_heading is not None:
            self._snippet_buf.append(data)

    def close(self) -> None:
        if self._current_heading is not None:
            self._current_heading["snippet"] = " ".join(self._snippet_buf).strip()[:200]
            self.headings.append(self._current_heading)
            self._current_heading = None
        super().close()


def _slugify(s: str) -> str:
    out = re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-")
    return out or "section"


def build_search_entries(out_path: str, title: str, body: str, section_label: str) -> list[dict]:
    parser = _IndexParser()
    parser.feed(body)
    parser.close()

    lede_text = re.sub(r"\s+", " ", "".join(parser.lede)).strip()
    entries: list[dict] = []

    # Page-root entry
    entries.append({
        "href": out_path,
        "title": title,
        "section": section_label,
        "snippet": lede_text[:240],
    })

    # Per-heading entries
    for h in parser.headings:
        if not h["text"]:
            continue
        entries.append({
            "href": out_path,
            "anchor": h["id"],
            "title": h["text"],
            "section": title,
            "snippet": (h.get("snippet") or "")[:200],
        })

    return entries


def section_label_for(out_path: str) -> str:
    if "/" not in out_path:
        return "overview"
    return out_path.split("/", 1)[0]


def main() -> int:
    written = 0
    search_index: list[dict] = []

    for out_path, title, active, body_file in PAGES:
        depth = out_path.count("/")
        src = CONTENT / body_file
        if not src.exists():
            print(f"  skip {out_path:40s} (no content yet)")
            continue
        body = src.read_text()

        extra_head = ""
        if body.startswith("<!--HEAD:"):
            end = body.find("-->")
            extra_head = body[len("<!--HEAD:"):end].strip()
            body = body[end + 3:].lstrip()

        html = html_template(title, depth, active, body, extra_head)
        out = ROOT / out_path
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(html)
        written += 1
        print(f"  wrote {out_path}")

        # Build search index entries from body fragment
        search_index.extend(
            build_search_entries(out_path, title, body, section_label_for(out_path))
        )

    # Write search-index.json
    idx_path = ROOT / "search-index.json"
    idx_path.write_text(json.dumps(search_index, ensure_ascii=False, separators=(",", ":")))
    print(f"\n{written} pages written, {len(search_index)} search entries")
    return 0


if __name__ == "__main__":
    sys.exit(main())
