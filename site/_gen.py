#!/usr/bin/env python3
"""Generate the multi-page site from per-page content fragments.

Layout is shared via JS (site/js/layout.js), so each generated HTML file is
just <head> + <main> with the page-specific content. The sidebar nav, TOC
rail, and prev/next footer are injected at runtime by layout.js.

Also writes:
  - site/search-index.json: one entry per (page, h2/h3 heading) plus one
    root entry per page. Drives the in-page search overlay.
  - site/scenes.json: curated CDNAME -> category map for the asset viewer's
    Scene filter (Towns / Field areas / Battle / Cutscenes / Audio / Other).
  - site/shops.json: joined shop + item + weapon + armor + accessory data
    that the interactive shops page consumes.
  - site/world.json: per-town summary (CDNAME labels, enemies, bosses,
    shops, casino, fishing) for the world page.
  - site/sitemap.xml, site/robots.txt, site/404.html: SEO surface. Every
    page also gets a meta description (curated for landing pages, derived
    from the lede otherwise), a canonical URL, Open Graph / Twitter cards
    pointing at img/social-card.png, and schema.org JSON-LD.

Run from the repo root:
    python3 site/_gen.py
"""
from __future__ import annotations
import hashlib
import html
import json
import re
import sys
import tomllib
from html.parser import HTMLParser
from pathlib import Path

ROOT = Path(__file__).resolve().parent
CONTENT = ROOT / "_content"
REPO_ROOT = ROOT.parent
GAMEDATA = REPO_ROOT / "data" / "gamedata"
DISC_PATCHING_TOML = CONTENT / "writeups" / "disc-patching" / "mods.toml"

# Base for linking a committed repo file (the `<repo>/blob/main/<path>` form the
# pages already use for "full reference" links).
REPO_BLOB = "https://github.com/AndrewAltimit/legend-of-legaia-re/blob/main"

# ---------------------------------------------------------------------------
# SEO / social metadata
# ---------------------------------------------------------------------------
# GitHub Pages project-site origin. Canonical URLs, the sitemap, and the
# Open Graph tags are all absolute against this base.
SITE_URL = "https://andrewaltimit.github.io/legend-of-legaia-re"
SITE_NAME = "Legend of Legaia RE"
SOCIAL_CARD = f"{SITE_URL}/img/social-card.png"
THEME_COLOR = "#0d1117"

# Site-wide fallback description (also the home page's og:description).
DEFAULT_DESCRIPTION = (
    "Legend of Legaia (PSX, 1998) reverse engineered end to end: play the "
    "from-scratch engine port in your browser via WebAssembly, browse every "
    "asset, patch your disc with the randomizer, and read byte-level format "
    "docs. Bring your own disc image - no Sony data is distributed."
)

# Hand-written <title> overrides for the landing pages search engines should
# surface. Everything else gets "{title} - Legend of Legaia RE". The override
# is the FULL title (no suffix appended), so it can lead with the game name.
TITLE_OVERRIDES: dict[str, str] = {
    "home": "Legend of Legaia Reverse Engineering - Playable Engine Port, Asset Viewers & Randomizer",
    "play": "Play Legend of Legaia in the Browser - WASM Engine Port",
    "viewer": "Legend of Legaia Asset Viewer - Textures, Models & Sound (WASM)",
    "media": "Legend of Legaia Media Browser - Music, FMVs & Voice (WASM)",
    "monsters": "Legend of Legaia Enemy Table - Stats, Drops & 3D Models",
    "characters": "Legend of Legaia Characters - Field & Battle Models",
    "npcs": "Legend of Legaia NPCs - Every Townsperson in 3D",
    "minigames": "Play Legend of Legaia Minigames in the Browser",
    "world": "Legend of Legaia World Guide - Towns, Enemies, Shops & Fishing",
    "world-overview": "Legend of Legaia World Map in 3D - WebGL Viewer",
    "shops": "Legend of Legaia Shops & Vendors - Full Inventories and Prices",
    "arts": "Legend of Legaia Tactical Arts List - Inputs, AP & Damage",
    "magic": "Legend of Legaia Seru Magic & Summons - Every Cast in 3D",
    "tooling/rom-patcher": "Legend of Legaia Randomizer - In-Browser ROM Patcher",
    "tooling/translation-workbench": "Legend of Legaia Translation Workbench - Edit a Language Pack in the Browser",
    "guides/vrchat-world": "Legend of Legaia in VRChat - Build a World from Your Own Disc",
}

# Hand-written meta descriptions for the same landing pages. Everything else
# derives its description from the page's own lede paragraph, falling back to
# DEFAULT_DESCRIPTION when a page has no lede.
DESCRIPTIONS: dict[str, str] = {
    "home": DEFAULT_DESCRIPTION,
    "play": (
        "Play Legend of Legaia in your browser: an in-progress WebAssembly "
        "engine port with retail movement, NPC dialogue, menus, and memory-"
        "card saves, running from your own disc image. Works flat or in VR "
        "(WebXR)."
    ),
    "viewer": (
        "Browse Legend of Legaia's textures, 3D models, dialog, and sound "
        "banks in your browser. The WASM asset viewer decodes everything "
        "locally from your own disc image - nothing is uploaded."
    ),
    "media": (
        "Listen to Legend of Legaia's music, watch its FMV cutscenes, and "
        "hear the voice audio in your browser - decoded locally from your "
        "own disc image."
    ),
    "monsters": (
        "Every Legend of Legaia enemy with stats, item drops, steals, which "
        "Seru-magic side-effects it can take, and rotating 3D battle models, "
        "decoded from the disc's own data tables."
    ),
    "characters": (
        "Legend of Legaia's party in 3D: Vahn, Noa, Gala and their Ra-Seru "
        "forms, with field and battle meshes assembled the way the game "
        "itself builds them."
    ),
    "npcs": (
        "Every Legend of Legaia NPC rendered in 3D from the scene files: "
        "townsfolk, shopkeepers, and story characters, browsable per town."
    ),
    "minigames": (
        "Play Legend of Legaia's minigames in the browser: the casino slot "
        "machine, Noa's dance, and Baka Fighter - with the retail step "
        "charts, odds, and payout tables read from your disc."
    ),
    "world": (
        "Town-by-town guide to Legend of Legaia's world, assembled from the "
        "disc: enemies, bosses, shops, casino games, and fishing spots per "
        "area."
    ),
    "world-overview": (
        "Fly through Legend of Legaia's entire world in 3D: every kingdom, "
        "town, and field scene rendered in WebGL from your own disc image, "
        "with VR support."
    ),
    "shops": (
        "Every shop and vendor in Legend of Legaia with full inventories and "
        "prices, joined from the game's own item, weapon, and armor tables."
    ),
    "arts": (
        "Complete Legend of Legaia Tactical Arts list with button inputs, AP "
        "costs, and damage data, cross-checked against the game's own move "
        "tables."
    ),
    "magic": (
        "Every Legend of Legaia Seru-magic cast animated in 3D, including the "
        "Ra-Seru and Sim-Seru summons Meta, Ozma, Terra, Horn, Jedo, Palma "
        "and Mule - decoded in your browser from your own disc image."
    ),
    "tooling/rom-patcher": (
        "Randomize Legend of Legaia in your browser: drops, encounters, "
        "chests, shops, doors, and more, patched client-side onto your own "
        "disc image - which never leaves your machine."
    ),
    "tooling/translation-workbench": (
        "Translate Legend of Legaia in your browser: edit a language pack "
        "against your own disc with live byte and on-screen width checks, see "
        "how much room every name, label and scene has, and download a "
        "shareable pack. The disc never leaves your machine."
    ),
    "quickstart": (
        "Get started with the Legend of Legaia RE tools: extract every asset "
        "from your disc, view them interactively, and boot the engine port "
        "in minutes."
    ),
    "architecture": (
        "How the Legend of Legaia RE project stacks from raw PSX disc "
        "sectors to a running from-scratch engine: formats, parsers, "
        "subsystems, and the WASM port."
    ),
    "guides/vrchat-world": (
        "Turn a Legend of Legaia town into a private VRChat world: export any "
        "scene from your own disc as glTF, then build it in Unity with the "
        "project's kit - walkable retail terrain, animated villagers, doors "
        "that teleport you inside, and the scene's music on loop."
    ),
    "writeups/index": (
        "Technical deep-dives from reverse engineering Legend of Legaia: a "
        "retail softlock's anatomy, fishing's rarest catch, and six tiers of "
        "patching a sealed disc."
    ),
}

# Pages that are interactive WASM/WebGL applications rather than articles -
# they get WebApplication JSON-LD and og:type=website.
APP_PAGE_KEYS: set[str] = {
    "play", "viewer", "media", "minigames", "monsters", "characters",
    "npcs", "world-overview", "tooling/rom-patcher",
    "tooling/translation-workbench",
}


def canonical_url(out_path: str) -> str:
    """Canonical URL for a generated page (directory indexes canonicalize to
    the trailing-slash form GitHub Pages serves)."""
    if out_path == "index.html":
        return SITE_URL + "/"
    if out_path.endswith("/index.html"):
        return f"{SITE_URL}/{out_path[: -len('index.html')]}"
    return f"{SITE_URL}/{out_path}"


def _clip_description(text: str, limit: int = 220) -> str:
    """Clip lede text to a meta-description length on a word boundary."""
    text = re.sub(r"\s+", " ", text).strip()
    if len(text) <= limit:
        return text
    cut = text[:limit].rsplit(" ", 1)[0].rstrip(",;:")
    return cut + "…"


def _jsonld_for(active_key: str, page_title: str, description: str, canonical: str) -> str:
    """Structured data: the home page declares the site + the application and
    ties both to the original game; app pages are WebApplications; everything
    else is a TechArticle."""
    website = {"@type": "WebSite", "name": SITE_NAME, "url": SITE_URL + "/"}
    if active_key == "home":
        data: dict = {
            "@context": "https://schema.org",
            "@graph": [
                {**website, "description": DEFAULT_DESCRIPTION},
                {
                    "@type": "SoftwareApplication",
                    "name": SITE_NAME,
                    "url": SITE_URL + "/",
                    "description": DEFAULT_DESCRIPTION,
                    "applicationCategory": "GameApplication",
                    "operatingSystem": "Web browser (WebAssembly), Linux, Windows",
                    "offers": {"@type": "Offer", "price": "0", "priceCurrency": "USD"},
                    "license": f"{REPO_BLOB}/LICENSE",
                    "codeRepository": "https://github.com/AndrewAltimit/legend-of-legaia-re",
                    "about": {
                        "@type": "VideoGame",
                        "name": "Legend of Legaia",
                        "gamePlatform": "PlayStation",
                        "datePublished": "1998",
                    },
                },
            ],
        }
    elif active_key in APP_PAGE_KEYS:
        data = {
            "@context": "https://schema.org",
            "@type": "WebApplication",
            "name": page_title,
            "url": canonical,
            "description": description,
            "applicationCategory": "GameApplication",
            "browserRequirements": "Requires WebAssembly",
            "isPartOf": website,
        }
    else:
        data = {
            "@context": "https://schema.org",
            "@type": "TechArticle",
            "headline": page_title,
            "url": canonical,
            "description": description,
            "isPartOf": website,
        }
    return json.dumps(data, ensure_ascii=False, separators=(",", ":"))


def seo_head(out_path: str, active_key: str, title: str, description: str) -> tuple[str, str]:
    """Return (page_title, head-metadata block) for a generated page."""
    page_title = TITLE_OVERRIDES.get(active_key, f"{title} - {SITE_NAME}")
    canonical = canonical_url(out_path)
    og_type = "website" if active_key == "home" or active_key in APP_PAGE_KEYS else "article"
    esc_title = html.escape(page_title, quote=True)
    esc_desc = html.escape(description, quote=True)
    jsonld = _jsonld_for(active_key, page_title, description, canonical)
    # </script> can't appear inside an inline script; escape defensively.
    jsonld = jsonld.replace("</", "<\\/")
    lines = [
        f'<meta name="description" content="{esc_desc}">',
        f'  <link rel="canonical" href="{canonical}">',
        f'  <meta name="theme-color" content="{THEME_COLOR}">',
        f'  <meta property="og:site_name" content="{SITE_NAME}">',
        f'  <meta property="og:type" content="{og_type}">',
        f'  <meta property="og:title" content="{esc_title}">',
        f'  <meta property="og:description" content="{esc_desc}">',
        f'  <meta property="og:url" content="{canonical}">',
        f'  <meta property="og:image" content="{SOCIAL_CARD}">',
        '  <meta property="og:image:width" content="1200">',
        '  <meta property="og:image:height" content="630">',
        '  <meta name="twitter:card" content="summary_large_image">',
        f'  <meta name="twitter:title" content="{esc_title}">',
        f'  <meta name="twitter:description" content="{esc_desc}">',
        f'  <meta name="twitter:image" content="{SOCIAL_CARD}">',
        f'  <script type="application/ld+json">{jsonld}</script>',
    ]
    return page_title, "\n".join(lines)


def _committed_md_index() -> tuple[set[str], dict[str, str]]:
    """Index the committed Markdown files the site can link to: everything under
    `docs/` and `crates/` plus the top-level `*.md` (README / CLAUDE). Returns
    `(paths, by_basename)` where `paths` is the set of repo-relative paths and
    `by_basename` maps a basename to its path **only when that basename is
    unique** (ambiguous basenames like `README.md` are left out so they only
    resolve via an exact path). Deliberately excludes generated trees (`target/`)
    and the agent-only memory files (which live outside the repo), so an
    unresolved reference - e.g. a `project_*.md` memory note - is left as plain
    text rather than linked to a 404."""
    paths: set[str] = set()
    for base in ("docs", "crates"):
        for p in (REPO_ROOT / base).rglob("*.md"):
            paths.add(p.relative_to(REPO_ROOT).as_posix())
    for p in REPO_ROOT.glob("*.md"):
        paths.add(p.name)
    by_basename: dict[str, str] = {}
    clash: set[str] = set()
    for path in paths:
        name = path.rsplit("/", 1)[-1]
        if name in by_basename:
            clash.add(name)
        by_basename[name] = path
    for name in clash:
        by_basename.pop(name, None)
    return paths, by_basename


def _resolve_md(ref: str, paths: set[str], by_basename: dict[str, str]) -> str | None:
    """Resolve a Markdown reference as written in page prose to a committed
    repo-relative path, or `None` if it isn't a committed file. Tries the
    reference verbatim, then under `docs/`, then by unique basename - covering
    full paths (`docs/...`, `crates/.../README.md`), docs-relative paths
    (`subsystems/foo.md`), and bare filenames (`extraction.md`)."""
    ref = ref.strip().removeprefix("./")
    if ref in paths:
        return ref
    docs_rel = f"docs/{ref}"
    if docs_rel in paths:
        return docs_rel
    return by_basename.get(ref.rsplit("/", 1)[-1])


_MD_CODE_RE = re.compile(r"<code>([^<>]+?\.md)</code>")


def autolink_md_refs(body: str, paths: set[str], by_basename: dict[str, str]) -> str:
    """Wrap every bare `<code>PATH.md</code>` whose path resolves to a committed
    repo file in a link to that file on GitHub. Skips `<code>` spans already
    inside an `<a>` (so the existing full-reference links aren't double-wrapped)
    and references that don't resolve to a committed file."""

    def inside_anchor(upto: str) -> bool:
        return upto.rfind("<a ") > upto.rfind("</a>")

    def repl(m: re.Match) -> str:
        if inside_anchor(body[: m.start()]):
            return m.group(0)
        resolved = _resolve_md(m.group(1), paths, by_basename)
        if resolved is None:
            return m.group(0)
        return (
            f'<a href="{REPO_BLOB}/{resolved}" target="_blank" rel="noopener">'
            f"{m.group(0)}</a>"
        )

    return _MD_CODE_RE.sub(repl, body)


# Pages that benefit from breaking out of the prose reading-width cap.
# Multi-pane interactive surfaces; everything else (format / subsystem /
# tooling / reference) stays narrow for readability.
WIDE_PAGES: set[str] = {
    "play",
    "shops",
    "world",
    "minigames",
    "arts",
    "magic",
    "monsters",
    "characters",
    "npcs",
    "viewer",
    "media",
    "world-overview",
    "reference/music-tracks",
    "reference/scene-names",
    "tooling/rom-patcher",
    "tooling/translation-workbench",
}


# ---------------------------------------------------------------------------
# Script cache-busting
# ---------------------------------------------------------------------------
# Every `js/*.js` a page loads is rewritten to carry `?v=<content hash>`,
# replacing whatever hand-written marker was there.
#
# Hand-written markers are the failure: they only bust when someone remembers
# to bump them, and forgetting is silent - a stale script and a current one
# deploy identically and read identically in a diff. `webgl-tmd.js` and
# `webgl-math.js` both changed while still shipping `?v=zfight-1`, and
# `layout.js` never carried a marker at all, so a returning browser kept
# running the old file against a rebuilt engine until its cache expired on its
# own. A content hash cannot be forgotten: it changes exactly when the bytes
# change, and only then.
_SCRIPT_SRC_RE = re.compile(r'(src=")((?:\.\./)*js/[A-Za-z0-9._-]+\.js)(?:\?[^"]*)?(")')
_asset_hash_cache: dict[str, str] = {}


def _asset_hash(rel: str) -> str | None:
    """Short content hash of `site/<rel>`; None when there is no such file."""
    if rel not in _asset_hash_cache:
        path = ROOT / rel
        _asset_hash_cache[rel] = (
            hashlib.sha256(path.read_bytes()).hexdigest()[:10] if path.exists() else ""
        )
    return _asset_hash_cache[rel] or None


def version_script_srcs(page: str) -> str:
    """Stamp every `js/*.js` reference in one page with its content hash."""

    def sub(m: re.Match[str]) -> str:
        head, ref, tail = m.group(1), m.group(2), m.group(3)
        digest = _asset_hash(ref.replace("../", ""))
        # An unresolvable reference keeps whatever it had: inventing a version
        # for a file that is not there would hide the missing file.
        return m.group(0) if digest is None else f"{head}{ref}?v={digest}{tail}"

    return _SCRIPT_SRC_RE.sub(sub, page)


def _wasm_version() -> str:
    """Short content version of the shipped WASM bundle (glue + binary).

    Injected into every page as `window.LEGAIA_WASM_V`; the JS wasm
    loaders append it as `?v=` to BOTH the glue module import and the
    `_bg.wasm` binary URL. Dynamic `import()` runs after page load, so
    even a hard refresh does not reliably bypass the HTTP cache for the
    module - only a URL that changes with the bytes does. `0` when the
    bundle is absent (CI builds it at deploy).
    """
    glue = _asset_hash("wasm/legaia_web_viewer.js") or ""
    bg = _asset_hash("wasm/legaia_web_viewer_bg.wasm") or ""
    return f"{glue[:6]}{bg[:6]}" if glue or bg else "0"


def html_template(page_title: str, depth: int, active_key: str, body: str, extra_head: str = "", head_meta: str = "") -> str:
    css = "../" * depth + "css/styles.css"
    layout_js = "../" * depth + "js/layout.js"
    main_js = "../" * depth + "js/main.js"
    favicon = "../" * depth + "img/favicon.svg"
    if active_key in WIDE_PAGES:
        content_cls = "content wide-page"
    elif active_key.startswith("writeups/disc-patching/"):
        # Write-up pages use one consistent measure for prose, diagrams, and the
        # mod-catalog table so nothing is wider than the running text.
        content_cls = "content dp-page"
    else:
        content_cls = "content"
    return f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{html.escape(page_title)}</title>
  {head_meta}
  <link rel="icon" href="{favicon}" type="image/svg+xml">
  <link rel="stylesheet" href="{css}">
  <script>window.LEGAIA_WASM_V="{_wasm_version()}";</script>
  {extra_head}
</head>
<body>
<a class="skip-link" href="#content">Skip to content</a>
<div class="app">
<main class="{content_cls}" id="content">
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
    ("play.html",                  "Play the port (WASM)",          "play",                       "play.html"),
    ("viewer.html",                "Asset viewer (WASM)",           "viewer",                     "viewer.html"),
    ("media.html",                 "Media browser (WASM)",          "media",                      "media.html"),
    ("world.html",                 "Game world",                    "world",                      "world.html"),
    ("shops.html",                 "Shops & vendors",               "shops",                      "shops.html"),
    ("minigames.html",             "Minigames",                     "minigames",                  "minigames.html"),
    ("arts.html",                  "Tactical Arts",                 "arts",                       "arts.html"),
    ("magic.html",                 "Seru Magic & Summons",          "magic",                      "magic.html"),
    ("monsters.html",              "Enemy table (WASM)",            "monsters",                   "monsters.html"),
    ("characters.html",            "Characters (WASM)",             "characters",                 "characters.html"),
    ("npcs.html",                  "NPCs (WASM)",                   "npcs",                       "npcs.html"),
    ("world-overview.html",        "World overview",                "world-overview",             "world-overview.html"),
    # depth = 1
    # User guides (release-binary walkthroughs, mirrored from docs/guides/)
    ("guides/getting-started.html", "Getting started (release tools)", "guides/getting-started",     "guides/getting-started.html"),
    ("guides/extracting-assets.html","Extracting assets",             "guides/extracting-assets",   "guides/extracting-assets.html"),
    ("guides/playing-and-viewing.html","Playing and viewing",         "guides/playing-and-viewing", "guides/playing-and-viewing.html"),
    ("guides/modding-and-translation.html","Modding and translation", "guides/modding-and-translation","guides/modding-and-translation.html"),
    ("guides/translating.html",    "Translating the game",           "guides/translating",         "guides/translating.html"),
    ("guides/vrchat-world.html",   "Importing a world into VRChat",  "guides/vrchat-world",        "guides/vrchat-world.html"),
    # Technical write-ups (narrative deep-dives)
    ("writeups/index.html",        "Technical write-ups",           "writeups/index",             "writeups/index.html"),
    ("writeups/gaza-orbit-softlock.html", "The endless camera orbit - anatomy of a retail softlock", "writeups/gaza-orbit-softlock", "writeups/gaza-orbit-softlock.html"),
    ("writeups/spirit-fish.html", "The Spirit fish gate - fishing's rarest catch, decompiled", "writeups/spirit-fish", "writeups/spirit-fish.html"),
    ("writeups/disc-patching/index.html","Patching a sealed disc",   "writeups/disc-patching/index","writeups/disc-patching/index.html"),
    ("writeups/disc-patching/a-static-tables.html","Tier A - static-table overwrites","writeups/disc-patching/a-static-tables","writeups/disc-patching/a-static-tables.html"),
    ("writeups/disc-patching/b-lzs-slots.html","Tier B - editing inside LZS","writeups/disc-patching/b-lzs-slots","writeups/disc-patching/b-lzs-slots.html"),
    ("writeups/disc-patching/c-field-vm-operands.html","Tier C - rewriting field-VM bytecode","writeups/disc-patching/c-field-vm-operands","writeups/disc-patching/c-field-vm-operands.html"),
    ("writeups/disc-patching/d-man-relocation.html","Tier D - variable-length relocation","writeups/disc-patching/d-man-relocation","writeups/disc-patching/d-man-relocation.html"),
    ("writeups/disc-patching/e-rodata-gap-code.html","Tier E - rodata-gap code injection","writeups/disc-patching/e-rodata-gap-code","writeups/disc-patching/e-rodata-gap-code.html"),
    ("writeups/disc-patching/f-overlay-dead-region.html","Tier F - overlay dead-region injection","writeups/disc-patching/f-overlay-dead-region","writeups/disc-patching/f-overlay-dead-region.html"),
    ("writeups/disc-patching/g-ram-payload-carrier.html","Tier G - carrying a RAM patch on disc","writeups/disc-patching/g-ram-payload-carrier","writeups/disc-patching/g-ram-payload-carrier.html"),
    ("subsystems/index.html",      "Subsystems",                    "subsystems/index",           "subsystems/index.html"),
    ("subsystems/boot.html",       "Boot path",                     "subsystems/boot",            "subsystems/boot.html"),
    ("subsystems/asset-loader.html","Asset loader",                 "subsystems/asset-loader",    "subsystems/asset-loader.html"),
    ("subsystems/script-vm.html",  "Field / event script VM",       "subsystems/script-vm",       "subsystems/script-vm.html"),
    ("subsystems/field-locomotion.html","Field locomotion",          "subsystems/field-locomotion","subsystems/field-locomotion.html"),
    ("subsystems/actor-vm.html",   "Actor / sprite VM",             "subsystems/actor-vm",        "subsystems/actor-vm.html"),
    ("subsystems/move-vm.html",    "Move-table VM",                 "subsystems/move-vm",         "subsystems/move-vm.html"),
    ("subsystems/motion-vm.html",  "Motion VM (camera / NPC)",      "subsystems/motion-vm",       "subsystems/motion-vm.html"),
    ("subsystems/effect-vm.html",  "Effect VM",                     "subsystems/effect-vm",       "subsystems/effect-vm.html"),
    ("subsystems/battle.html",     "Battle",                        "subsystems/battle",          "subsystems/battle.html"),
    ("subsystems/battle-internals.html","Battle: internals",       "subsystems/battle-internals","subsystems/battle-internals.html"),
    ("subsystems/battle-action.html","Battle action state machine", "subsystems/battle-action",   "subsystems/battle-action.html"),
    ("subsystems/history-battle.html","Battle: capture notes",      "subsystems/history-battle",  "subsystems/history-battle.html"),
    ("subsystems/battle-formulas.html","Battle formulas",            "subsystems/battle-formulas", "subsystems/battle-formulas.html"),
    ("subsystems/arts-command-gauge.html","Arts command gauge",      "subsystems/arts-command-gauge","subsystems/arts-command-gauge.html"),
    ("subsystems/inventory.html",  "Inventory",                     "subsystems/inventory",       "subsystems/inventory.html"),
    ("subsystems/audio.html",      "Audio",                         "subsystems/audio",           "subsystems/audio.html"),
    ("subsystems/renderer.html",   "Renderer",                      "subsystems/renderer",        "subsystems/renderer.html"),
    ("subsystems/shading.html",    "Shading and palettes",          "subsystems/shading",         "subsystems/shading.html"),
    ("subsystems/world-map.html",  "World map",                     "subsystems/world-map",       "subsystems/world-map.html"),
    ("subsystems/history-world-map.html","Chapter-1 hub sweep (history)","subsystems/history-world-map","subsystems/history-world-map.html"),
    ("subsystems/world-overview-viewer.html","World-overview viewer", "subsystems/world-overview-viewer","subsystems/world-overview-viewer.html"),
    ("subsystems/vr-mode.html",   "VR mode (WebXR)",               "subsystems/vr-mode",         "subsystems/vr-mode.html"),
    ("subsystems/save-screen.html","Save screen",                   "subsystems/save-screen",     "subsystems/save-screen.html"),
    ("subsystems/field-menu.html", "Field menu (status panel)",     "subsystems/field-menu",      "subsystems/field-menu.html"),
    ("subsystems/shop.html",       "Shop",                          "subsystems/shop",            "subsystems/shop.html"),
    ("subsystems/inn.html",        "Inn",                           "subsystems/inn",             "subsystems/inn.html"),
    ("subsystems/level-up.html",   "Level-up",                      "subsystems/level-up",        "subsystems/level-up.html"),
    ("subsystems/cutscene.html",   "Cutscene (STR mode)",           "subsystems/cutscene",        "subsystems/cutscene.html"),
    ("subsystems/cutscene-internals.html","Cutscene: internals",     "subsystems/cutscene-internals","subsystems/cutscene-internals.html"),
    ("subsystems/engine.html",     "Engine reimplementation",       "subsystems/engine",          "subsystems/engine.html"),
    ("formats/index.html",         "Formats",                       "formats/index",              "formats/index.html"),
    # Per-format pages (mirrored from docs/formats/)
    ("formats/disc.html",          "PSX disc geometry",             "formats/disc",               "formats/disc.html"),
    ("formats/prot.html",          "PROT.DAT TOC",                  "formats/prot",               "formats/prot.html"),
    ("formats/cdname.html",        "CDNAME.TXT name map",           "formats/cdname",             "formats/cdname.html"),
    ("formats/dmy.html",           "DMY.DAT (dev fixtures)",        "formats/dmy",                "formats/dmy.html"),
    ("formats/lzs.html",           "Legaia LZS",                    "formats/lzs",                "formats/lzs.html"),
    ("formats/asset-type.html",    "Asset type dispatcher",         "formats/asset-type",         "formats/asset-type.html"),
    ("formats/asset-descriptor.html","Asset descriptor",            "formats/asset-descriptor",   "formats/asset-descriptor.html"),
    ("formats/data-field.html",    "DATA_FIELD streaming",          "formats/data-field",         "formats/data-field.html"),
    ("formats/pack.html",          "Pack format",                   "formats/pack",               "formats/pack.html"),
    ("formats/tim-pack.html",      "Standalone TIM-pack",           "formats/tim-pack",           "formats/tim-pack.html"),
    ("formats/field-pack.html",    "Field-pack format",             "formats/field-pack",         "formats/field-pack.html"),
    ("formats/battle-data-pack.html","Battle-data pack",             "formats/battle-data-pack",   "formats/battle-data-pack.html"),
    ("formats/npc-palette.html",   "Row-479 NPC CLUTs",             "formats/npc-palette",        "formats/npc-palette.html"),
    ("formats/effect.html",        "Effect bundles",                "formats/effect",             "formats/effect.html"),
    ("formats/scene-bundles.html", "Scene bundles",                 "formats/scene-bundles",      "formats/scene-bundles.html"),
    ("formats/scene-v12-table.html","scene_v12_table",              "formats/scene-v12-table",    "formats/scene-v12-table.html"),
    ("formats/slot-b-module-layout.html","Slot-B module layout",     "formats/slot-b-module-layout","formats/slot-b-module-layout.html"),
    ("formats/world-map-overlay.html","Slot-4 records",              "formats/world-map-overlay",  "formats/world-map-overlay.html"),
    ("formats/place-names.html",   "Place names",                   "formats/place-names",        "formats/place-names.html"),
    ("formats/tim.html",           "PSX TIM",                       "formats/tim",                "formats/tim.html"),
    ("formats/tmd.html",           "Legaia TMD",                    "formats/tmd",                "formats/tmd.html"),
    ("formats/vab.html",           "VAB sound bank",                "formats/vab",                "formats/vab.html"),
    ("formats/seq.html",           "PsyQ SEQ",                      "formats/seq",                "formats/seq.html"),
    ("formats/xa.html",            "XA-ADPCM",                      "formats/xa",                 "formats/xa.html"),
    ("formats/mes.html",           "MES dialog",                    "formats/mes",                "formats/mes.html"),
    ("formats/anm.html",           "ANM animation",                 "formats/anm",                "formats/anm.html"),
    ("formats/monster-animation.html","Monster animation",           "formats/monster-animation",  "formats/monster-animation.html"),
    ("formats/character-mesh.html","Player-character mesh pack",     "formats/character-mesh",     "formats/character-mesh.html"),
    ("formats/mdt.html",           "MDT move table",                "formats/mdt",                "formats/mdt.html"),
    ("formats/move-power.html",    "Move-power table",              "formats/move-power",         "formats/move-power.html"),
    ("formats/art-data.html",      "Art data",                      "formats/art-data",           "formats/art-data.html"),
    ("formats/dialog-font.html",   "Dialog font",                   "formats/dialog-font",        "formats/dialog-font.html"),
    ("formats/sfx-table.html",     "SFX descriptor table",          "formats/sfx-table",          "formats/sfx-table.html"),
    ("formats/sound-driver.html",  "Sound-driver paths",            "formats/sound-driver",       "formats/sound-driver.html"),
    ("formats/pochi.html",         "Pochi-filler placeholders",     "formats/pochi",              "formats/pochi.html"),
    ("formats/ringside-still.html","Headerless 16bpp stills",       "formats/ringside-still",     "formats/ringside-still.html"),
    ("formats/mips-overlay.html",  "MIPS overlay code",             "formats/mips-overlay",       "formats/mips-overlay.html"),
    ("formats/overlay-ptr-table.html","Overlay pointer-table code", "formats/overlay-ptr-table",  "formats/overlay-ptr-table.html"),
    ("formats/navmesh.html",       "Per-scene primitive scratch buffer", "formats/navmesh",       "formats/navmesh.html"),
    ("formats/encounter.html",     "Encounter record",              "formats/encounter",          "formats/encounter.html"),
    ("formats/man-relocation.html", "MAN relocation",               "formats/man-relocation",     "formats/man-relocation.html"),
    ("formats/str-fmv-table.html", "STR FMV table",                 "formats/str-fmv-table",      "formats/str-fmv-table.html"),
    ("formats/save-record.html",   "Per-character save record",     "formats/save-record",        "formats/save-record.html"),
    ("formats/window-script.html", "Window widget scripts",         "formats/window-script",      "formats/window-script.html"),
    # Battle / stat tables (static SCUS_942.54 data)
    ("formats/spell-table.html",   "Spell table",                   "formats/spell-table",        "formats/spell-table.html"),
    ("formats/item-table.html",    "Item-name table",               "formats/item-table",         "formats/item-table.html"),
    ("formats/item-effect-table.html","Item-effect table",          "formats/item-effect-table",  "formats/item-effect-table.html"),
    ("formats/equipment-table.html","Equipment stat-bonus table",   "formats/equipment-table",    "formats/equipment-table.html"),
    ("formats/accessory-passive-table.html","Accessory passive table","formats/accessory-passive-table","formats/accessory-passive-table.html"),
    ("formats/steal-table.html",   "Steal table",                   "formats/steal-table",        "formats/steal-table.html"),
    ("formats/new-game-table.html","New-game party template",       "formats/new-game-table",     "formats/new-game-table.html"),
    ("formats/summon-readef.html", "summon.dat / readef.DAT slots", "formats/summon-readef",      "formats/summon-readef.html"),
    ("tooling/index.html",         "Tooling",                       "tooling/index",              "tooling/index.html"),
    # Per-tooling pages (mirrored from docs/tooling/)
    ("tooling/extraction.html",    "Extraction CLIs",               "tooling/extraction",         "tooling/extraction.html"),
    ("tooling/ghidra.html",        "Ghidra in Docker",              "tooling/ghidra",             "tooling/ghidra.html"),
    ("tooling/overlay-capture.html","Overlay capture",              "tooling/overlay-capture",    "tooling/overlay-capture.html"),
    ("tooling/static-overlay-pipeline.html","Static overlay pipeline","tooling/static-overlay-pipeline","tooling/static-overlay-pipeline.html"),
    ("tooling/mednafen-automation.html","Mednafen automation",      "tooling/mednafen-automation","tooling/mednafen-automation.html"),
    ("tooling/pcsx-redux-automation.html","PCSX-Redux automation",  "tooling/pcsx-redux-automation","tooling/pcsx-redux-automation.html"),
    ("tooling/recomp-differential.html","Recomp differential oracle","tooling/recomp-differential","tooling/recomp-differential.html"),
    ("tooling/spine-flag-writers-capture.html","Spine flag-writers capture","tooling/spine-flag-writers-capture","tooling/spine-flag-writers-capture.html"),
    ("tooling/determinism-replay.html","Determinism + replay",      "tooling/determinism-replay", "tooling/determinism-replay.html"),
    ("tooling/randomizer.html",    "Randomizer / disc patcher",     "tooling/randomizer",         "tooling/randomizer.html"),
    ("tooling/randomizer-internals.html","Randomizer: internals",   "tooling/randomizer-internals","tooling/randomizer-internals.html"),
    ("tooling/translation.html",   "Translation / language packs",  "tooling/translation",        "tooling/translation.html"),
    ("tooling/port-catalog.html",  "Port catalog",                  "tooling/port-catalog",       "tooling/port-catalog.html"),
    ("tooling/disc-coverage.html","Disc coverage",                 "tooling/disc-coverage",      "tooling/disc-coverage.html"),
    ("tooling/byte-accounting.html","Byte accounting",              "tooling/byte-accounting",    "tooling/byte-accounting.html"),
    ("tooling/address-reference-scan.html","Address-reference scan","tooling/address-reference-scan","tooling/address-reference-scan.html"),
    ("tooling/rom-patcher.html",   "ROM patcher (in browser)",      "tooling/rom-patcher",        "tooling/rom-patcher.html"),
    ("tooling/translation-workbench.html","Translation workbench",   "tooling/translation-workbench","tooling/translation-workbench.html"),
    ("reference/index.html",       "Reference",                     "reference/index",            "reference/index.html"),
    ("reference/functions.html",   "Key functions",                 "reference/functions",        "reference/functions.html"),
    ("reference/memory-map.html",  "PSX RAM map",                   "reference/memory-map",       "reference/memory-map.html"),
    ("reference/cheats.html",      "Cheat databases",               "reference/cheats",           "reference/cheats.html"),
    ("reference/gamedata.html",    "Curated game-data tables",      "reference/gamedata",         "reference/gamedata.html"),
    ("reference/music-tracks.html","Music-track disambiguation",     "reference/music-tracks",     "reference/music-tracks.html"),
    ("reference/scene-names.html", "Scene names",                   "reference/scene-names",      "reference/scene-names.html"),
    ("reference/open-rev-eng-threads.html","Open RE threads",        "reference/open-rev-eng-threads","reference/open-rev-eng-threads.html"),
    ("reference/re-settled-threads.html","Settled RE threads",       "reference/re-settled-threads","reference/re-settled-threads.html"),
    ("reference/re-do-not-re-walk.html","Do not re-walk",            "reference/re-do-not-re-walk","reference/re-do-not-re-walk.html"),
]


# ---------------------------------------------------------------------------
# CDNAME -> display name / category map.
#
# Read from data/gamedata/scenes.toml, the one scene-name table (the Rust side
# is `legaia_gamedata::Database::scene_names`; provenance and the per-scene
# evidence live in docs/reference/scene-names.md). Each row's `cdname` is the
# block's `#define <label> N` number: a **raw in-RAM PROT-TOC** index, not the
# extraction space every consumer of scenes.json indexes in, so
# `build_scenes_json` converts it (`RAW_TOC_INDEX_OFFSET`) on the way out.
# Coverage runs from each label's converted start to the next label's
# converted start - 1, inclusive. The category drives the asset viewer's
# Scene filter and the play page's scene picker.
#
# Categories:
#   town       - visitable settlement (NPC dialog, shops, inn)
#   field      - field map (dungeon / overworld pocket / mountain / cave)
#   world_map  - world-map scenes (map01/02/03)
#   cutscene   - op*/ed* engine cutscene scenes
#   battle     - battle-only data blocks (battle_data, monster_data, ...)
#   audio      - audio-only data blocks (sound_data, vab_*, music_*)
#   system     - everything else (init, gameover, level_up, card_data, ...)
# ---------------------------------------------------------------------------

def _load_scene_names() -> list[dict]:
    """The one scene-name table: `data/gamedata/scenes.toml`.

    Also compiled into `legaia_gamedata::Database::scene_names`, so the site,
    the CLIs and the engine name a scene the same way. Rows keep the raw
    `#define` number as `start` and the display name as `display` - the shape
    the rest of this generator reads.
    """
    with (GAMEDATA / "scenes.toml").open("rb") as f:
        rows = tomllib.load(f).get("scene", [])
    return [
        {"label": r["id"], "start": r["cdname"], "category": r["category"],
         "display": r["name"], "banner": r.get("banner"),
         "contributor": r.get("contributor"), "note": r.get("note")}
        for r in rows
    ]


CDNAME_SCENES: list[dict] = _load_scene_names()


# The boot TOC loader copies PROT.DAT verbatim - 8-byte header included - into
# 0x801C70F0, so a CDNAME `#define` number is two rows ahead of the same
# content's extraction-entry index: `extraction = raw - 2`. Mirrors
# `legaia_prot::cdname::RAW_TOC_INDEX_OFFSET`; see docs/formats/cdname.md
# "Numbering space" for the loader-constant identities that pin it.
RAW_TOC_INDEX_OFFSET = 2


def build_scenes_json() -> list[dict]:
    """Expand CDNAME_SCENES into a sorted list with prot_end inclusive.

    Starts are converted from the raw-TOC frame the `#define` numbers use to
    the extraction frame, because that is the space every consumer indexes:
    `prot_index` in the WASM viewer's entry list is `legaia_prot::Entry::index`,
    the TOC row `p` whose start LBA is `toc[p + 2]`. Left unconverted, a block's
    window drops its own field `.MAP` and v12 sidecar (retail slots 0 and 1)
    and bleeds in the next block's - so `town01` would cover the two entries
    that open `town0b` while hiding its own map. Same conversion as
    `legaia_prot::cdname::block_range_for_name_extraction`.

    `init_data` drops out: its raw 0..1 rows *are* the TOC header, so it owns
    no extraction entry, and extraction 0 belongs to `gameover_data`'s second
    slot (the `0000_init_data.BIN` filename label carries the same +2 skew).

    The last entry runs to PROT_MAX so the viewer always has coverage.
    """
    PROT_MAX = 1233
    entries = sorted(CDNAME_SCENES, key=lambda s: s["start"])
    starts = [max(s["start"] - RAW_TOC_INDEX_OFFSET, 0) for s in entries]
    out: list[dict] = []
    for i, s in enumerate(entries):
        start = starts[i]
        end = starts[i + 1] - 1 if i + 1 < len(entries) else PROT_MAX
        if end < start:
            # A block whose whole window fell inside the TOC header rows.
            continue
        row = {
            "label": s["label"],
            "display": s["display"],
            "category": s["category"],
            "prot_start": start,
            "prot_end": end,
        }
        for k in ("banner", "contributor", "note"):
            if s.get(k):
                row[k] = s[k]
        out.append(row)
    return out


# ---------------------------------------------------------------------------
# Gamedata aggregation (drives shops.html + world.html).
# ---------------------------------------------------------------------------

def _load_toml(name: str) -> dict:
    p = GAMEDATA / name
    if not p.exists():
        return {}
    with p.open("rb") as f:
        return tomllib.load(f)


def _index_by_key(rows: list[dict], plural: str, singular: str) -> dict[str, dict]:
    """Index rows from a TOML table-array by their `key` field, tagging origin."""
    out: dict[str, dict] = {}
    for r in rows:
        if "key" not in r:
            continue
        rec = dict(r)
        rec["_kind"] = singular  # one of: item / weapon / armor / accessory
        out[r["key"]] = rec
    return out


def build_gamedata_json() -> tuple[dict, dict]:
    """Build (shops_json, world_json) from data/gamedata/*.toml.

    shops_json shape:
        {
          "towns": [
            { "name": "Rim Elm", "scene_label": "town01",
              "shops": [
                { "name": "Variety Shop", "merchant": null, "phase": null,
                  "featured": [...keys...],
                  "items": [ <item-detail>, ... ]
                }, ...
              ]
            }, ...
          ],
          "lookup_origin": "data/gamedata/*.toml"
        }

    world_json shape:
        {
          "locations": [
            { "name": "Rim Elm", "scene_label": "town01",
              "category": "town", "display": "Rim Elm",
              "enemies": [...], "bosses": [...],
              "shop_count": N, "has_casino": bool, "has_fishing": bool
            }, ...
          ]
        }
    """
    items     = _index_by_key(_load_toml("items.toml").get("item", []),         "items",       "item")
    weapons   = _index_by_key(_load_toml("weapons.toml").get("weapon", []),     "weapons",     "weapon")
    armor     = _index_by_key(_load_toml("armor.toml").get("armor", []),        "armor",       "armor")
    accs      = _index_by_key(_load_toml("accessories.toml").get("accessory", []), "accessories", "accessory")

    catalog: dict[str, dict] = {}
    catalog.update(items)
    catalog.update(weapons)
    catalog.update(armor)
    catalog.update(accs)

    def resolve(key: str) -> dict:
        r = catalog.get(key)
        if r is None:
            return {"key": key, "name": key, "_kind": "unknown",
                    "missing": True}
        return r

    # Reverse map: walkthrough town name -> the scene that town's shops live
    # in (data/gamedata/scenes.toml names every scene; this only picks which of
    # a town's scenes the walkthrough's shop list belongs to). Each pick is the
    # scene whose MAN carries that town's op-0x49 shop records
    # (`asset shop-stock`), not a guess from the label.
    TOWN_TO_SCENE = {
        "Rim Elm":              "town01",
        "Hunter's Spring":      "izumi",
        "Drake Castle":         "dolk2",
        "Biron Monastery":      "bylon",
        "Wind Cave":            "jiji",      # banner "Ancient Wind Cave"
        "Jeremi":               "geremi",
        "Vidna":                "balden",
        "Octam":                "ropeway",   # surface Octam after the Mist
        "Underground Octam":    "rayman",    # Hari's underground city
        "Ratayu":               "retock",
        "Karisto Station":      "station3",
        "Sol":                  "koin4",     # Sol Tower's shop floor
        "Buma":                 "bubu1",     # the thawed town; bubu2 is frozen
        "Usha Research Center": "doman",
        "Soren Camp":           "son",
        "Conkram":              "conc",      # past Conkram (banner "Conkram (Past)")
    }

    shops_raw = _load_toml("shops.toml").get("shop", [])
    casino_raw = _load_toml("casino.toml")
    slot_prizes = casino_raw.get("slot_prize", [])
    muscle_courses = casino_raw.get("muscle_dome_course", [])
    muscle_bosses_raw = casino_raw.get("muscle_dome_boss", [])
    muscle_rounds_raw = casino_raw.get("muscle_dome_round", [])
    baka_fighter_meta = casino_raw.get("baka_fighter_meta", {})
    baka_fighter_rounds = casino_raw.get("baka_fighter", [])
    muscle_secrets = casino_raw.get("muscle_paradise_secret", [])
    sol_tower_raw = _load_toml("sol_tower.toml")
    fishing_raw = _load_toml("fishing.toml").get("fishing_prize", [])
    enemies_raw = _load_toml("enemies.toml").get("enemy", [])
    bosses_raw  = _load_toml("bosses.toml").get("boss",  [])

    # Group shops by town
    towns_to_shops: dict[str, list[dict]] = {}
    for sh in shops_raw:
        town = sh.get("town", "Unknown")
        shop_record = {
            "name":     sh.get("name") or "(shop)",
            "merchant": sh.get("merchant"),
            "phase":    sh.get("phase"),
            "featured": sh.get("featured", []),
            "items":    [resolve(k) for k in sh.get("inventory", [])],
        }
        towns_to_shops.setdefault(town, []).append(shop_record)

    # Casino + fishing prize lists, keyed by location
    casino_by_town: dict[str, list[dict]] = {}
    for p in slot_prizes:
        item = resolve(p["item"])
        casino_by_town.setdefault(p["location"], []).append({
            "kind": "slot",
            "cost_coins": p.get("cost_coins"),
            "item": item,
        })

    fishing_by_town: dict[str, list[dict]] = {}
    for p in fishing_raw:
        item = resolve(p["item"])
        fishing_by_town.setdefault(p["location"], []).append({
            "kind": "fishing",
            "cost_points": p.get("cost_points"),
            "notes":       p.get("notes"),
            "item": item,
        })

    # Walkthrough's `location` strings on enemies/bosses cover a much wider
    # taxonomy than just towns (Mt. Letona, Snowdrift Cave, ...). For the
    # world page we surface every location that has at least one enemy.
    all_locations: dict[str, dict] = {}
    for e in enemies_raw:
        loc = e.get("location") or "(unknown)"
        for piece in [p.strip() for p in re.split(r"[,/]", loc)]:
            if not piece:
                continue
            all_locations.setdefault(piece, {"enemies": [], "bosses": []})
            all_locations[piece]["enemies"].append(e)
    for b in bosses_raw:
        loc = b.get("location") or "(unknown)"
        for piece in [p.strip() for p in re.split(r"[,/]", loc)]:
            if not piece:
                continue
            all_locations.setdefault(piece, {"enemies": [], "bosses": []})
            all_locations[piece]["bosses"].append(b)

    # ------ shops_json
    shops_payload = {
        "towns": [
            {
                "name":        town,
                "scene_label": TOWN_TO_SCENE.get(town),
                "shops":       shops_list,
                "casino":      casino_by_town.get(town, []),
                "fishing":     fishing_by_town.get(town, []),
            }
            for town, shops_list in sorted(towns_to_shops.items())
        ],
    }

    # ------ world_json: every walkthrough location, joined w/ scene info
    world_locations: list[dict] = []
    for loc, agg in sorted(all_locations.items()):
        scene_label = TOWN_TO_SCENE.get(loc)
        scene = None
        if scene_label:
            for s in CDNAME_SCENES:
                if s["label"] == scene_label:
                    scene = {"label": s["label"], "category": s["category"],
                             "display": s["display"]}
                    break
        # Enemy / boss summaries (drop bulky fields for the JSON payload)
        def short_enemy(e: dict) -> dict:
            return {
                "name":    e.get("name"),
                "element": e.get("element"),
                "drop":    e.get("drop"),
                "steal":   e.get("steal"),
                "steal_chance": e.get("steal_chance"),
            }
        def short_boss(b: dict) -> dict:
            return {
                "name":    b.get("name"),
                "hp_min":  b.get("hp_min"),
                "hp_max":  b.get("hp_max"),
                "tournament": b.get("tournament"),
            }
        world_locations.append({
            "name":        loc,
            "scene":       scene,
            "is_town":     loc in TOWN_TO_SCENE,
            "enemy_count": len(agg["enemies"]),
            "boss_count":  len(agg["bosses"]),
            "enemies":     [short_enemy(e) for e in agg["enemies"][:40]],
            "bosses":      [short_boss(b)  for b in agg["bosses"]],
            "shop_count":  len(towns_to_shops.get(loc, [])),
            "has_casino":  bool(casino_by_town.get(loc)),
            "has_fishing": bool(fishing_by_town.get(loc)),
        })

    world_payload = {"locations": world_locations}

    # ------ minigames_json: a single payload that drives the minigames page.
    # Joins the casino + sol_tower tables, resolves item references against
    # the unified catalog so the page renders effects without a second fetch.
    def resolve_or_none(key):
        r = catalog.get(key)
        if r is None:
            return {"key": key, "name": key, "_kind": "unknown", "missing": True}
        return r

    # Build a lookup from boss-slug -> normalised boss record. Each Seru carries
    # an array of `seru_levels`; monster/boss kinds have stats at top-level.
    boss_by_key: dict[str, dict] = {}
    for b in muscle_bosses_raw:
        key = b.get("key")
        if not key:
            continue
        boss_by_key[key] = {
            "key":         key,
            "name":        b.get("name"),
            "kind":        b.get("kind"),
            "romaji":      b.get("romaji"),
            "script":      b.get("script"),
            "element":     b.get("element"),
            "weakness":    b.get("weakness", []),
            "strength":    b.get("strength", []),
            "hp":          b.get("hp"),
            "mp":          b.get("mp"),
            "atk":         b.get("atk"),
            "udf":         b.get("udf"),
            "ldf":         b.get("ldf"),
            "intelligence": b.get("intelligence"),
            "spd":         b.get("spd"),
            "agl":         b.get("agl"),
            "exp":         b.get("exp"),
            "gold":        b.get("gold"),
            "location":    b.get("location"),
            "steal":       b.get("steal"),
            "steal_chance": b.get("steal_chance"),
            "drop":        b.get("drop"),
            "drop_chance": b.get("drop_chance"),
            "attacks":     b.get("attacks", []),
            "immune_to":   b.get("immune_to", []),
            "courses":     b.get("courses", []),
            "wiki_path":   b.get("wiki_path"),
            "seru_levels": b.get("seru_level", []),
        }

    # Group rounds by course_key for ordered roster output.
    rounds_by_course: dict[str, list[dict]] = {}
    for r in muscle_rounds_raw:
        rounds_by_course.setdefault(r["course_key"], []).append(r)
    for cs in rounds_by_course.values():
        cs.sort(key=lambda x: x.get("round", 0))

    muscle_dome_payload = []
    for course in muscle_courses:
        course_key = course.get("key") or course.get("name", "").lower()
        roster = []
        for r in rounds_by_course.get(course_key, []):
            roster.append({
                "round":       r.get("round"),
                "boss_key":    r.get("boss_key"),
                "seru_level":  r.get("seru_level"),
            })
        muscle_dome_payload.append({
            "key":             course_key,
            "name":            course.get("name"),
            "entry_fee":       course.get("entry_fee"),
            "reward_coins":    course.get("reward_coins"),
            "restrictions":    course.get("restrictions", []),
            "allowed":         course.get("allowed", []),
            "roster":          roster,
            "reward_first_clear":          course.get("reward_first_clear"),
            "reward_first_clear_item":     resolve_or_none(course["reward_first_clear"]) if course.get("reward_first_clear") else None,
            "reward_first_clear_requires": course.get("reward_first_clear_requires"),
        })

    slot_payload_by_loc: dict[str, list[dict]] = {}
    for p in slot_prizes:
        slot_payload_by_loc.setdefault(p["location"], []).append({
            "cost_coins": p.get("cost_coins"),
            "notes":      p.get("notes"),
            "item":       resolve_or_none(p["item"]),
        })

    secrets_payload = []
    for s in muscle_secrets:
        secrets_payload.append({
            "key":     s.get("key"),
            "name":    s.get("name"),
            "trigger": s.get("trigger"),
            "notes":   s.get("notes"),
            "reward_item": resolve_or_none(s["reward"]) if s.get("reward") else None,
        })

    side_quests_payload = []
    for sq in sol_tower_raw.get("side_quest", []):
        side_quests_payload.append({
            "key":         sq.get("key"),
            "name":        sq.get("name"),
            "chain":       sq.get("chain", []),
            "reward":      sq.get("reward"),
            "reward_item": resolve_or_none(sq["reward_item"]) if sq.get("reward_item") else None,
            "notes":       sq.get("notes"),
        })

    minigames_payload = {
        "muscle_dome":     muscle_dome_payload,
        "bosses":          boss_by_key,
        "slot_machines":   [
            {"location": loc, "prizes": prizes}
            for loc, prizes in sorted(slot_payload_by_loc.items())
        ],
        "baka_fighter": {
            "meta":   baka_fighter_meta,
            "rounds": baka_fighter_rounds,
        },
        "secrets":      secrets_payload,
        "sol_tower":    {
            "meta":        sol_tower_raw.get("meta", {}),
            "floors":      sol_tower_raw.get("floor", []),
            "side_quests": side_quests_payload,
        },
    }

    return shops_payload, world_payload, minigames_payload


# ---------------------------------------------------------------------------
# Arts payload (drives arts.html).
#
# Per-character grouping by kind (regular / hyper / super / miracle), preserving
# the order rows appear in arts.toml so the page mirrors the curated layout.
# ---------------------------------------------------------------------------

ARTS_CHARACTERS: list[str] = ["Vahn", "Noa", "Gala"]
ARTS_KINDS: list[str] = ["regular", "hyper", "super", "miracle"]


def _esc(s: str) -> str:
    """Minimal HTML-escape for text pulled from the TOML catalog."""
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def build_progress_meter() -> str:
    """Render the landing page's per-track progress meter from
    `scripts/ci/progress-metrics.json`.

    The metrics are a COMMITTED build input, not something computed here: they
    are derived from the Ghidra dump corpus and the extracted disc tree, both of
    which are gitignored and absent wherever the site is deployed. A machine
    with the disc refreshes them via `scripts/ci/update-progress-metrics.py`.

    Each bar carries its own denominator, because the tracks are not comparable
    with one another - two are measured against the game's own bytes and two
    against the set of functions this project has identified. Presenting them as
    one number would be the obvious way to mislead, so the markup keeps the
    denominator attached to the bar rather than in a footnote.

    Returns "" when the metrics file is missing, so the page degrades to its
    static status pills rather than failing the build.
    """
    src = ROOT.parent / "scripts" / "ci" / "progress-metrics.json"
    if not src.exists():
        return ""
    try:
        tracks = json.loads(src.read_text(encoding="utf-8")).get("tracks", [])
    except (ValueError, OSError):
        return ""
    if not tracks:
        return ""

    # Compact stat strip: big number + one-line label + thin meter. The full
    # methodology paragraph moves off the landing page - each tile links out
    # via the strip's single "How these are measured" link, and the per-track
    # detail survives as the tile's title (hover) text.
    compact = {
        "decompilation": ("{pct:.1f}<small>%</small>", "of the executable's code traced in Ghidra"),
        "formats": ("{pct:.1f}<small>%</small>", "of disc bytes resolve to a documented format"),
        "port": ("{count}", "retail functions reimplemented in Rust"),
        "wiring": ("{pct:.1f}<small>%</small>", "of ported code a host is owed for, wired"),
    }
    rows = []
    for t in tracks:
        pct = max(0.0, min(100.0, float(t.get("pct", 0.0))))
        key = str(t.get("key", ""))
        label = html.escape(str(t.get("label", "")))
        headline = str(t.get("headline", ""))
        detail = html.escape(str(t.get("detail", "")))
        denom = html.escape(str(t.get("denominator", "")))
        count = headline.split(" ", 1)[0] if headline else ""
        num_tpl, lbl = compact.get(key, ("{pct:.1f}<small>%</small>", ""))
        num = num_tpl.format(pct=pct, count=html.escape(count))
        if not lbl:
            lbl = f"{t.get('label', '')} — {t.get('denominator', '')}"
        rows.append(
            f'  <div class="stat" title="{detail}" role="img" '
            f'aria-label="{label}: {pct:.1f} percent of {denom}">\n'
            f'    <div class="num">{num}</div>\n'
            f'    <div class="lbl">{html.escape(lbl)}</div>\n'
            f'    <div class="meter"><i style="width: {pct:.1f}%"></i></div>\n'
            f'  </div>'
        )
    return (
        '<div class="stats-block">\n<div class="stats">\n' + "\n".join(rows) + "\n</div>\n"
        '<div class="stats-foot"><a href="tooling/disc-coverage.html">'
        "How these are measured →</a></div>\n</div>"
    )


def build_image_port_status() -> str:
    """Render the landing page's per-image port table from
    `scripts/ci/image-port-status.json`.

    The whole-project tiles above it sit at 100% and no longer move; this is
    the figure that still does. One card per runtime code image (the main
    executable and each overlay, the 64 slot-B cast modules folded into one
    band), named by what the image does in the game.

    Each card carries two numbers on purpose. The big one is the share of the
    image's functions that are ported or excused natively; it is measured over
    the functions the dump corpus places in the image, so it cannot see a
    routine nobody dumped. The second, "code identified", is the share of the
    image's code BYTES inside a placed dump - the companion that keeps a 100%
    honest. "Fully ported" needs both (see `update-progress-metrics.py`).

    Like the tiles, the JSON is a committed build input (the corpus and the
    disc are absent at deploy time), so a missing file renders nothing.
    """
    src = ROOT.parent / "scripts" / "ci" / "image-port-status.json"
    if not src.exists():
        return ""
    try:
        images = json.loads(src.read_text(encoding="utf-8")).get("images", [])
    except (ValueError, OSError):
        return ""
    if not images:
        return ""

    def pct(n: int, d: int) -> float:
        return max(0.0, min(100.0, 100.0 * n / d)) if d else 0.0

    def card(r: dict) -> str:
        fn = int(r.get("functions", 0))
        ported = int(r.get("ported", 0))
        native = int(r.get("native", 0)) + int(r.get("no_behaviour", 0))
        open_ = int(r.get("open", 0))
        done = bool(r.get("fully_ported"))
        acc = float(r.get("pct_accounted", 0.0))
        ident = float(r.get("pct_identified", 0.0))
        name = html.escape(str(r.get("name", r.get("key", ""))))
        desc = html.escape(str(r.get("desc", "")))
        prot = r.get("prot")
        where = "SCUS_942.54" if prot is None else f"PROT {html.escape(str(prot))}"
        if r.get("images"):
            sub = (f'{int(r.get("images_complete", 0))} of '
                   f'{int(r["images"]) - int(r.get("images_no_code", 0))} modules complete')
        else:
            sub = ""
        tip = (
            f"{fn} functions placed in this image by the dump corpus: {ported} ported "
            f"(of which {int(r.get('replaced', 0))} replaced by an engine mechanism), "
            f"{int(r.get('native', 0))} covered natively (PsyQ / BIOS / GPU / CD), "
            f"{int(r.get('no_behaviour', 0))} with no retail behaviour, {open_} open. "
            f"Code identified: {ident:.1f}% of the image's code bytes sit in a dump "
            f"placed here; {int(r.get('undumped_code_bytes', 0)):,} code bytes are un-dumped. "
            f"{int(r.get('unplaced', 0))} further dumps fall in its address span but "
            f"the bytes could not place them."
        )
        badge = '<span class="ip-badge">Fully ported</span>' if done else ""
        open_txt = (f'<b class="ip-open">{open_} open</b>' if open_ else "0 open")
        return (
            f'<div class="ip-card{" done" if done else ""}" title="{html.escape(tip)}" '
            f'role="img" aria-label="{name}: {acc:.1f} percent of {fn} functions '
            f'accounted for, {open_} open; code identified {ident:.1f} percent'
            f'{", fully ported" if done else ""}">\n'
            f'  <div class="ip-top"><span class="ip-name">{name}</span>{badge}</div>\n'
            f'  <div class="ip-desc">{desc} <span class="ip-where">{where}</span></div>\n'
            f'  <div class="ip-num">{acc:.1f}<small>%</small>'
            f'<span class="ip-of">of {fn:,} functions</span></div>\n'
            f'  <div class="ip-bar"><i class="p" style="width: {pct(ported, fn):.2f}%"></i>'
            f'<i class="n" style="width: {pct(native, fn):.2f}%"></i></div>\n'
            f'  <div class="ip-foot">{ported:,} ported · {native:,} native · {open_txt}'
            f'{" · " + sub if sub else ""}<br>code identified {ident:.1f}%</div>\n'
            f'</div>'
        )

    game = [r for r in images if r.get("kind", "game") == "game"]
    other = [r for r in images if r.get("kind", "game") != "game"]
    n_done = sum(1 for r in game if r.get("fully_ported"))
    out = [
        '<div class="ip-block">',
        '<div class="ip-head"><h2>Port status by code image</h2>'
        f'<span class="ip-count">{n_done} of {len(game)} game images fully ported</span></div>',
        '<p class="ip-lede">The game\'s logic ships as one executable plus overlays the disc '
        'swaps in per mode. Each card counts the functions the disc places in that image: '
        '<span class="ip-key p"></span>ported to Rust, '
        '<span class="ip-key n"></span>covered natively (PsyQ libraries, BIOS, GPU and CD '
        'plumbing) or with no retail behaviour, and what is still open. '
        '<em>Code identified</em> is the share of the image\'s code bytes inside an identified '
        'function, so a 100% never hides code nobody has looked at.</p>',
        '<div class="ip-grid">',
        *(card(r) for r in game),
        "</div>",
    ]
    if other:
        out += [
            '<details class="ip-more"><summary>Developer and unused images '
            f"({len(other)})</summary>",
            '<div class="ip-grid">',
            *(card(r) for r in other),
            "</div></details>",
        ]
    out.append('<div class="stats-foot"><a href="tooling/disc-coverage.html'
               '#per-image-port-status">How this is measured →</a></div>')
    out.append("</div>")
    return "\n".join(out)


def build_disc_patching_table() -> str:
    """Render the mod -> technique-tier master table for the disc-patching
    write-up index from `mods.toml`, grouped by tier in ladder order. Returns
    the HTML substituted for the `<!--DISC_PATCHING_TABLE-->` placeholder, or ""
    if the catalog is absent. A tier title links to its page only when that page
    is published (`published = true`), so the internal-link gate stays green
    while tier pages are still being written."""
    if not DISC_PATCHING_TOML.exists():
        return ""
    with DISC_PATCHING_TOML.open("rb") as f:
        data = tomllib.load(f)
    tiers = data.get("tier", [])
    variants = {v["id"]: v["title"] for v in data.get("variant", [])}
    by_tier: dict[str, list[dict]] = {}
    for m in data.get("mod", []):
        by_tier.setdefault(m["tier"], []).append(m)

    out = [
        '<div class="table-wrap">',
        "<table>",
        "<thead><tr>"
        "<th>Mod</th><th>Flag</th><th>Target</th>"
        "<th>Edit</th><th>Gap</th><th>Status</th><th>Test</th>"
        "</tr></thead>",
        "<tbody>",
    ]
    for t in tiers:
        tid = t["id"]
        title = f'Tier {tid} - {_esc(t["title"])}'
        if t.get("published"):
            title = f'<a href="{t["page"]}">{title}</a>'
        out.append(
            f'<tr class="dp-tier"><td colspan="7"><strong>{title}</strong> '
            f'- {_esc(t.get("summary", ""))}</td></tr>'
        )
        for m in by_tier.get(tid, []):
            name = f'<strong>{_esc(m["name"])}</strong>'
            var = m.get("variant")
            if var:
                name += f' <span class="tag" title="{_esc(variants.get(var, var))}">{var}</span>'
            target = _esc(m.get("target", ""))
            notes = m.get("notes")
            if notes:
                target += f'<span class="dp-note">{_esc(notes)}</span>'
            gap = "shared gap" if m.get("gap") else "-"
            status = m.get("status", "")
            status_cls = "tag-done" if status == "shipped" else "tag-planned"
            test = m.get("test")
            test_html = f"<code>{_esc(test)}</code>" if test else "-"
            out.append(
                f"<tr><td>{name}</td>"
                f'<td><code>{_esc(m.get("flag", ""))}</code></td>'
                f"<td>{target}</td>"
                f'<td>{_esc(m.get("edit", ""))}</td>'
                f"<td>{gap}</td>"
                f'<td class="col-status"><span class="tag {status_cls}">{status}</span></td>'
                f"<td>{test_html}</td></tr>"
            )
    out += ["</tbody>", "</table>", "</div>"]
    return "\n".join(out)


def build_arts_json() -> dict:
    arts_raw = _load_toml("arts.toml").get("arts", [])
    by_char: dict[str, dict[str, list[dict]]] = {
        c: {k: [] for k in ARTS_KINDS} for c in ARTS_CHARACTERS
    }
    for a in arts_raw:
        ch = a.get("character")
        kd = a.get("kind")
        if ch not in by_char or kd not in by_char[ch]:
            continue
        by_char[ch][kd].append({
            "name":            a.get("name"),
            "kind":            kd,
            "ap":              a.get("ap"),
            "command":         a.get("command", []),
            "directions":      a.get("directions", []),
            "action_constant": a.get("action_constant"),
        })
    return {
        "characters": [
            {
                "name":         ch,
                "arts_by_kind": by_char[ch],
                "total":        sum(len(v) for v in by_char[ch].values()),
            }
            for ch in ARTS_CHARACTERS
        ],
    }


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


def write_gitignore(generated: list[str]) -> None:
    """Write site/.gitignore listing every artifact this run produced.

    Self-maintaining: the ignore list is exactly what _gen.py writes, so it
    can never drift from the real outputs. The .gitignore itself stays
    tracked (it's the manifest); the listed files do not.
    """
    header = [
        "# Generated by site/_gen.py - do NOT edit, do NOT commit the listed files.",
        "# These are build artifacts derived from _content/ + data/gamedata/.",
        "# Run `python3 site/_gen.py` for local file:// preview; CI regenerates",
        "# them on the GitHub Pages deploy. This manifest file is itself tracked.",
        "",
    ]
    # Anchor every entry to the site/ root with a leading slash. A bare path
    # like `index.html` would otherwise match that name anywhere in the tree -
    # including the source fragments under _content/ (e.g. the writeups index
    # pages), silently un-tracking them so CI checks out without their content.
    lines = header + [f"/{p}" for p in sorted(generated)] + [""]
    (ROOT / ".gitignore").write_text("\n".join(lines), encoding="utf-8", newline="\n")


def main() -> int:
    written = 0
    search_index: list[dict] = []
    # Every path this run writes under site/. Used to emit site/.gitignore
    # so the generated artifacts stay untracked (they're rebuilt by CI on
    # deploy and by `python3 site/_gen.py` for local preview).
    generated: list[str] = []

    # Index the committed Markdown files once, so a bare `<code>...md</code>` in
    # any page links to the real file in the repo.
    md_paths, md_by_basename = _committed_md_index()

    # The disc-patching master table is rendered once from mods.toml and spliced
    # into whichever page carries the placeholder.
    disc_patching_table = build_disc_patching_table()
    progress_meter = build_progress_meter()
    image_port_status = build_image_port_status()

    for out_path, title, active, body_file in PAGES:
        depth = out_path.count("/")
        src = CONTENT / body_file
        if not src.exists():
            print(f"  skip {out_path:40s} (no content yet)")
            continue
        body = src.read_text(encoding="utf-8")

        extra_head = ""
        if body.startswith("<!--HEAD:"):
            end = body.find("-->")
            extra_head = body[len("<!--HEAD:"):end].strip()
            body = body[end + 3:].lstrip()

        body = autolink_md_refs(body, md_paths, md_by_basename)
        if "<!--DISC_PATCHING_TABLE-->" in body:
            body = body.replace("<!--DISC_PATCHING_TABLE-->", disc_patching_table)
        if "<!--PROGRESS_METER-->" in body:
            body = body.replace("<!--PROGRESS_METER-->", progress_meter)
        if "<!--IMAGE_PORT_STATUS-->" in body:
            body = body.replace("<!--IMAGE_PORT_STATUS-->", image_port_status)

        # Build search index entries from body fragment (first entry carries
        # the page's lede paragraph, which doubles as the meta description).
        entries = build_search_entries(out_path, title, body, section_label_for(out_path))
        search_index.extend(entries)

        lede = entries[0]["snippet"] if entries else ""
        description = DESCRIPTIONS.get(active) or _clip_description(lede) or DEFAULT_DESCRIPTION
        page_title, head_meta = seo_head(out_path, active, title, description)

        page = version_script_srcs(
            html_template(page_title, depth, active, body, extra_head, head_meta)
        )
        out = ROOT / out_path
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(page, encoding="utf-8")
        written += 1
        generated.append(out_path)
        print(f"  wrote {out_path}")

    # Write search-index.json
    idx_path = ROOT / "search-index.json"
    idx_path.write_text(json.dumps(search_index, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
    generated.append("search-index.json")

    # Write sitemap.xml over every generated page's canonical URL. GitHub
    # Pages serves it at <SITE_URL>/sitemap.xml; submit that URL in Google
    # Search Console (a project-site robots.txt is not at the domain root,
    # so crawlers only find the sitemap when it is submitted or the site
    # gains a custom domain).
    urls = [canonical_url(p) for p in generated if p.endswith(".html")]
    sitemap = ['<?xml version="1.0" encoding="UTF-8"?>',
               '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">']
    for u in urls:
        sitemap.append(f"  <url><loc>{html.escape(u)}</loc></url>")
    sitemap.append("</urlset>\n")
    (ROOT / "sitemap.xml").write_text("\n".join(sitemap), encoding="utf-8")
    generated.append("sitemap.xml")

    # robots.txt is only authoritative at a domain root, so on the project
    # subpath it is inert for crawlers - kept anyway so the site is ready if
    # it ever moves to a custom domain, and as a human-readable pointer.
    (ROOT / "robots.txt").write_text(
        "User-agent: *\n"
        "Allow: /\n"
        f"Sitemap: {SITE_URL}/sitemap.xml\n",
        encoding="utf-8",
    )
    generated.append("robots.txt")

    # 404.html - GitHub Pages serves this for any missing path, at any depth,
    # so every asset reference must be absolute. noindex keeps it out of
    # search results. It also carries no layout.js chrome (whose hrefs are
    # relative, hence depth-dependent), so `.app` has to opt out of the icon
    # rail's left offset - `no-chrome` does that.
    (ROOT / "404.html").write_text(f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <meta name="robots" content="noindex">
  <title>Page not found - {SITE_NAME}</title>
  <link rel="icon" href="{SITE_URL}/img/favicon.svg" type="image/svg+xml">
  <link rel="stylesheet" href="{SITE_URL}/css/styles.css">
</head>
<body>
<div class="app no-chrome">
<main class="content" id="content">
<header class="page-header">
  <div class="breadcrumb">404</div>
  <h1>Page not found</h1>
  <p class="lede">That page doesn't exist (or moved). Head back to the
  <a href="{SITE_URL}/">project home</a>, or try the
  <a href="{SITE_URL}/viewer.html">asset viewer</a> /
  <a href="{SITE_URL}/play.html">playable port</a>.</p>
</header>
</main>
</div>
</body>
</html>
""", encoding="utf-8")
    generated.append("404.html")

    # Write scenes.json (CDNAME -> category map for the asset viewer's
    # Scene filter).
    scenes_payload = build_scenes_json()
    (ROOT / "scenes.json").write_text(
        json.dumps(scenes_payload, ensure_ascii=False, separators=(",", ":")),
        encoding="utf-8",
    )
    generated.append("scenes.json")

    # Write shops.json + world.json + minigames.json (gamedata join for the
    # interactive shops / world / minigames pages).
    shops_payload, world_payload, minigames_payload = build_gamedata_json()
    (ROOT / "shops.json").write_text(
        json.dumps(shops_payload, ensure_ascii=False, separators=(",", ":")),
        encoding="utf-8",
    )
    (ROOT / "world.json").write_text(
        json.dumps(world_payload, ensure_ascii=False, separators=(",", ":")),
        encoding="utf-8",
    )
    (ROOT / "minigames.json").write_text(
        json.dumps(minigames_payload, ensure_ascii=False, separators=(",", ":")),
        encoding="utf-8",
    )
    generated += ["shops.json", "world.json", "minigames.json"]

    arts_payload = build_arts_json()
    (ROOT / "arts.json").write_text(
        json.dumps(arts_payload, ensure_ascii=False, separators=(",", ":")),
        encoding="utf-8",
    )
    generated.append("arts.json")

    # Emit site/.gitignore so the generated artifacts above stay untracked.
    # They're rebuilt by CI on deploy (`python3 site/_gen.py`) and locally
    # for file:// preview, so committing them would only duplicate content
    # that already lives in _content/ + the gamedata TOMLs. This file is
    # itself tracked - it's the manifest of what _gen.py produces.
    write_gitignore(generated)

    print(f"\n{written} pages written, {len(search_index)} search entries")
    print(f"  scenes.json:    {len(scenes_payload)} CDNAME blocks")
    print(f"  shops.json:     {len(shops_payload['towns'])} towns")
    print(f"  world.json:     {len(world_payload['locations'])} locations")
    print(f"  minigames.json: {len(minigames_payload['muscle_dome'])} courses, "
          f"{sum(len(s['prizes']) for s in minigames_payload['slot_machines'])} slot prizes, "
          f"{len(minigames_payload['baka_fighter']['rounds'])} baka rounds, "
          f"{len(minigames_payload['sol_tower']['floors'])} sol-tower floors")
    print(f"  arts.json:      {sum(c['total'] for c in arts_payload['characters'])} arts across "
          f"{len(arts_payload['characters'])} characters")

    # Internal-link gate over the freshly generated pages: a broken relative
    # href/src or a dangling #anchor fails the build (and the deploy that
    # reruns this script). See scripts/ci/check-site-links.py.
    import subprocess
    link_check = subprocess.run(
        [sys.executable, str(REPO_ROOT / "scripts" / "ci" / "check-site-links.py")],
    )
    if link_check.returncode != 0:
        print("[_gen] broken internal links above -- fix the _content fragment hrefs",
              file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
