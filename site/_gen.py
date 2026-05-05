#!/usr/bin/env python3
"""Generate the multi-page site from per-page content fragments.

Layout is shared via JS (site/js/layout.js), so each generated HTML file is
just <head> + <main> with the page-specific content. The sidebar nav is
injected at runtime by layout.js.

Run from the repo root:
    python3 site/_gen.py

Or after editing _content/*:
    python3 site/_gen.py && open site/index.html
"""
from __future__ import annotations
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
CONTENT = ROOT / "_content"


def html_template(title: str, depth: int, active_key: str, body: str, extra_head: str = "") -> str:
    css = "../" * depth + "css/styles.css"
    layout_js = "../" * depth + "js/layout.js"
    main_js = "../" * depth + "js/main.js"
    favicon = "../" * depth + "img/favicon.svg"
    home_href = "../" * depth + "index.html" if depth else "index.html"
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
    ("index.html",                "Home",                  "home",                       "home.html"),
    ("architecture.html",         "How the layers stack",  "architecture",               "architecture.html"),
    ("quickstart.html",           "Quick start",           "quickstart",                 "quickstart.html"),
    ("viewer.html",               "Asset viewer (WASM)",   "viewer",                     "viewer.html"),
    # depth = 1
    ("subsystems/index.html",     "Subsystems",            "subsystems/index",           "subsystems/index.html"),
    ("subsystems/boot.html",      "Boot path",             "subsystems/boot",            "subsystems/boot.html"),
    ("subsystems/asset-loader.html","Asset loader",        "subsystems/asset-loader",    "subsystems/asset-loader.html"),
    ("subsystems/script-vm.html", "Field / event script VM","subsystems/script-vm",      "subsystems/script-vm.html"),
    ("subsystems/actor-vm.html",  "Actor / sprite VM",     "subsystems/actor-vm",        "subsystems/actor-vm.html"),
    ("subsystems/move-vm.html",   "Move-table VM",         "subsystems/move-vm",         "subsystems/move-vm.html"),
    ("subsystems/effect-vm.html", "Effect VM",             "subsystems/effect-vm",       "subsystems/effect-vm.html"),
    ("subsystems/battle.html",    "Battle",                "subsystems/battle",          "subsystems/battle.html"),
    ("subsystems/battle-action.html","Battle action state machine","subsystems/battle-action","subsystems/battle-action.html"),
    ("subsystems/audio.html",     "Audio",                 "subsystems/audio",           "subsystems/audio.html"),
    ("subsystems/renderer.html",  "Renderer",              "subsystems/renderer",        "subsystems/renderer.html"),
    ("subsystems/engine.html",    "Engine reimplementation","subsystems/engine",         "subsystems/engine.html"),
    ("formats/index.html",        "Formats",               "formats/index",              "formats/index.html"),
    ("tooling/index.html",        "Tooling",               "tooling/index",              "tooling/index.html"),
    ("reference/index.html",      "Reference",             "reference/index",            "reference/index.html"),
    ("reference/functions.html",  "Key functions",         "reference/functions",        "reference/functions.html"),
    ("reference/memory-map.html", "PSX RAM map",           "reference/memory-map",       "reference/memory-map.html"),
]


def main() -> int:
    written = 0
    for out_path, title, active, body_file in PAGES:
        depth = out_path.count("/")
        src = CONTENT / body_file
        if not src.exists():
            print(f"  skip {out_path:40s} (no content yet)")
            continue
        body = src.read_text()
        # Optional per-page head extras (e.g. inline SVG style overrides) marked
        # by a leading `<!--HEAD: ... -->` line.
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
    print(f"\n{written} pages written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
