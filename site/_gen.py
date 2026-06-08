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

Run from the repo root:
    python3 site/_gen.py
"""
from __future__ import annotations
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

# Base for linking a committed repo file (the `<repo>/blob/main/<path>` form the
# pages already use for "full reference" links).
REPO_BLOB = "https://github.com/AndrewAltimit/legend-of-legaia-re/blob/main"


def _committed_md_index() -> tuple[set[str], dict[str, str]]:
    """Index the committed Markdown files the site can link to: everything under
    `docs/` and `crates/` plus the top-level `*.md` (README / CLAUDE). Returns
    `(paths, by_basename)` where `paths` is the set of repo-relative paths and
    `by_basename` maps a basename to its path **only when that basename is
    unique** (ambiguous basenames like `README.md` are left out so they only
    resolve via an exact path). Deliberately excludes generated trees (`target/`)
    and the agent-only memory files (which live outside the repo), so an
    unresolved reference — e.g. a `project_*.md` memory note — is left as plain
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
    reference verbatim, then under `docs/`, then by unique basename — covering
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
    "shops",
    "world",
    "minigames",
    "arts",
    "monsters",
    "characters",
    "viewer",
    "media",
    "architecture",
    "world-overview",
}


def html_template(title: str, depth: int, active_key: str, body: str, extra_head: str = "") -> str:
    css = "../" * depth + "css/styles.css"
    layout_js = "../" * depth + "js/layout.js"
    main_js = "../" * depth + "js/main.js"
    favicon = "../" * depth + "img/favicon.svg"
    content_cls = "content wide-page" if active_key in WIDE_PAGES else "content"
    return f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{title} - legend-of-legaia-re</title>
  <link rel="icon" href="{favicon}" type="image/svg+xml">
  <link rel="stylesheet" href="{css}">
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
    ("viewer.html",                "Asset viewer (WASM)",           "viewer",                     "viewer.html"),
    ("media.html",                 "Media browser (WASM)",          "media",                      "media.html"),
    ("world.html",                 "Game world",                    "world",                      "world.html"),
    ("shops.html",                 "Shops & vendors",               "shops",                      "shops.html"),
    ("minigames.html",             "Minigames",                     "minigames",                  "minigames.html"),
    ("arts.html",                  "Tactical Arts",                 "arts",                       "arts.html"),
    ("monsters.html",              "Enemy table (WASM)",            "monsters",                   "monsters.html"),
    ("characters.html",            "Characters (WASM)",             "characters",                 "characters.html"),
    ("world-overview.html",        "World overview (3D)",           "world-overview",             "world-overview.html"),
    # depth = 1
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
    ("subsystems/battle-action.html","Battle action state machine", "subsystems/battle-action",   "subsystems/battle-action.html"),
    ("subsystems/battle-formulas.html","Battle formulas",            "subsystems/battle-formulas", "subsystems/battle-formulas.html"),
    ("subsystems/audio.html",      "Audio",                         "subsystems/audio",           "subsystems/audio.html"),
    ("subsystems/renderer.html",   "Renderer",                      "subsystems/renderer",        "subsystems/renderer.html"),
    ("subsystems/world-map.html",  "World map",                     "subsystems/world-map",       "subsystems/world-map.html"),
    ("subsystems/world-overview-viewer.html","World-overview viewer", "subsystems/world-overview-viewer","subsystems/world-overview-viewer.html"),
    ("subsystems/save-screen.html","Save screen",                   "subsystems/save-screen",     "subsystems/save-screen.html"),
    ("subsystems/shop.html",       "Shop",                          "subsystems/shop",            "subsystems/shop.html"),
    ("subsystems/inn.html",        "Inn",                           "subsystems/inn",             "subsystems/inn.html"),
    ("subsystems/level-up.html",   "Level-up",                      "subsystems/level-up",        "subsystems/level-up.html"),
    ("subsystems/cutscene.html",   "Cutscene (STR mode)",           "subsystems/cutscene",        "subsystems/cutscene.html"),
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
    ("formats/world-map-overlay.html","Slot-4 records",              "formats/world-map-overlay",  "formats/world-map-overlay.html"),
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
    ("formats/mips-overlay.html",  "MIPS overlay code",             "formats/mips-overlay",       "formats/mips-overlay.html"),
    ("formats/overlay-ptr-table.html","Overlay pointer-table code", "formats/overlay-ptr-table",  "formats/overlay-ptr-table.html"),
    ("formats/navmesh.html",       "Per-scene primitive scratch buffer", "formats/navmesh",       "formats/navmesh.html"),
    ("formats/encounter.html",     "Encounter record",              "formats/encounter",          "formats/encounter.html"),
    ("formats/man-relocation.html", "MAN relocation",               "formats/man-relocation",     "formats/man-relocation.html"),
    ("formats/str-fmv-table.html", "STR FMV table",                 "formats/str-fmv-table",      "formats/str-fmv-table.html"),
    ("formats/save-record.html",   "Per-character save record",     "formats/save-record",        "formats/save-record.html"),
    ("tooling/index.html",         "Tooling",                       "tooling/index",              "tooling/index.html"),
    # Per-tooling pages (mirrored from docs/tooling/)
    ("tooling/extraction.html",    "Extraction CLIs",               "tooling/extraction",         "tooling/extraction.html"),
    ("tooling/ghidra.html",        "Ghidra in Docker",              "tooling/ghidra",             "tooling/ghidra.html"),
    ("tooling/overlay-capture.html","Overlay capture",              "tooling/overlay-capture",    "tooling/overlay-capture.html"),
    ("tooling/static-overlay-pipeline.html","Static overlay pipeline","tooling/static-overlay-pipeline","tooling/static-overlay-pipeline.html"),
    ("tooling/mednafen-automation.html","Mednafen automation",      "tooling/mednafen-automation","tooling/mednafen-automation.html"),
    ("tooling/pcsx-redux-automation.html","PCSX-Redux automation",  "tooling/pcsx-redux-automation","tooling/pcsx-redux-automation.html"),
    ("tooling/determinism-replay.html","Determinism + replay",      "tooling/determinism-replay", "tooling/determinism-replay.html"),
    ("tooling/randomizer.html",    "Randomizer / disc patcher",     "tooling/randomizer",         "tooling/randomizer.html"),
    ("tooling/rom-patcher.html",   "ROM patcher (in browser)",      "tooling/rom-patcher",        "tooling/rom-patcher.html"),
    ("reference/index.html",       "Reference",                     "reference/index",            "reference/index.html"),
    ("reference/functions.html",   "Key functions",                 "reference/functions",        "reference/functions.html"),
    ("reference/memory-map.html",  "PSX RAM map",                   "reference/memory-map",       "reference/memory-map.html"),
    ("reference/cheats.html",      "Cheat databases",               "reference/cheats",           "reference/cheats.html"),
    ("reference/gamedata.html",    "Curated game-data tables",      "reference/gamedata",         "reference/gamedata.html"),
    ("reference/open-rev-eng-threads.html","Open RE threads",        "reference/open-rev-eng-threads","reference/open-rev-eng-threads.html"),
]


# ---------------------------------------------------------------------------
# Curated CDNAME -> category map.
#
# Each entry pins the *first* PROT index for a CDNAME block (the value
# of the `#define <label> N` marker). Coverage runs from each label's
# `prot_start` to the *next* label's start - 1, inclusive. The category
# drives the asset viewer's Scene filter; the display name is the
# walkthrough's English town/area name where one exists.
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

CDNAME_SCENES: list[dict] = [
    # System / boot
    {"label": "init_data",        "start": 0,    "category": "system",    "display": "Boot init"},
    {"label": "gameover_data",    "start": 1,    "category": "system",    "display": "Game over"},
    # Towns + landmark fields - Karisto continent
    {"label": "town01",           "start": 3,    "category": "town",      "display": "Rim Elm"},
    {"label": "town0b",           "start": 12,   "category": "town",      "display": "Town (0b)"},
    {"label": "town0c",           "start": 21,   "category": "town",      "display": "Town (0c)"},
    {"label": "izumi",            "start": 30,   "category": "town",      "display": "Hunter's Spring"},
    {"label": "cave01",           "start": 38,   "category": "field",     "display": "Snowdrift Cave"},
    {"label": "vell",             "start": 45,   "category": "town",      "display": "Drake Castle"},
    {"label": "bylon",            "start": 52,   "category": "town",      "display": "Biron Monastery"},
    {"label": "dolk",             "start": 60,   "category": "field",     "display": "Mist field (dolk)"},
    {"label": "dolk2",            "start": 68,   "category": "field",     "display": "Mist field (dolk2)"},
    {"label": "suimon",           "start": 77,   "category": "field",     "display": "Floodgate"},
    {"label": "map01",            "start": 85,   "category": "world_map", "display": "World map (Drake)"},
    {"label": "garmel",           "start": 94,   "category": "field",     "display": "Field (garmel)"},
    {"label": "vozz",             "start": 103,  "category": "field",     "display": "Voz Forest"},
    {"label": "keikoku",          "start": 111,  "category": "field",     "display": "Ravine (keikoku)"},
    {"label": "rikuroa2",         "start": 120,  "category": "field",     "display": "Mt. Rikuroa (upper)"},
    {"label": "dream",            "start": 128,  "category": "field",     "display": "Dream sequence"},
    {"label": "jiji",             "start": 137,  "category": "field",     "display": "Elder's place"},
    {"label": "retock",           "start": 145,  "category": "field",     "display": "Field (retock)"},
    {"label": "rikuroa",          "start": 155,  "category": "field",     "display": "Mt. Rikuroa"},
    {"label": "geremi",           "start": 165,  "category": "town",      "display": "Jeremi"},
    {"label": "stone",            "start": 174,  "category": "field",     "display": "Stone field"},
    {"label": "balden",           "start": 182,  "category": "town",      "display": "Vidna"},
    {"label": "conc",             "start": 191,  "category": "town",      "display": "Conkram"},
    {"label": "rayman",           "start": 199,  "category": "town",      "display": "Ratayu"},
    {"label": "ropeway",          "start": 207,  "category": "field",     "display": "Ropeway"},
    {"label": "dohaty",           "start": 217,  "category": "field",     "display": "Dohati's Castle"},
    {"label": "station",          "start": 226,  "category": "town",      "display": "Karisto Station"},
    {"label": "tunnela",          "start": 235,  "category": "field",     "display": "Tunnel A"},
    {"label": "map02",            "start": 244,  "category": "world_map", "display": "World map (Sebucus)"},
    {"label": "tower",            "start": 254,  "category": "field",     "display": "Rogue Tower"},
    {"label": "teien",            "start": 263,  "category": "field",     "display": "Sky Gardens"},
    {"label": "tunnelb",          "start": 272,  "category": "field",     "display": "Tunnel B"},
    {"label": "retockin",         "start": 281,  "category": "field",     "display": "Retock interior"},
    {"label": "retona",           "start": 290,  "category": "field",     "display": "Mt. Letona"},
    {"label": "jagaroom",         "start": 300,  "category": "field",     "display": "Juggernaut chamber"},
    {"label": "tunnelc",          "start": 309,  "category": "field",     "display": "Tunnel C"},
    {"label": "balden2",          "start": 318,  "category": "town",      "display": "Vidna (revisit)"},
    {"label": "rayman2",          "start": 328,  "category": "town",      "display": "Ratayu (revisit)"},
    {"label": "ropeway2",         "start": 337,  "category": "field",     "display": "Ropeway (revisit)"},
    # Master continent
    {"label": "town0d",           "start": 347,  "category": "town",      "display": "Sol"},
    {"label": "son",              "start": 354,  "category": "field",     "display": "Field (son)"},
    {"label": "concnow",          "start": 362,  "category": "field",     "display": "Conkram (Mist)"},
    {"label": "taiku",            "start": 371,  "category": "field",     "display": "Muscle Dome"},
    {"label": "deene",            "start": 382,  "category": "town",      "display": "Buma"},
    {"label": "map03",            "start": 391,  "category": "world_map", "display": "World map (Karisto)"},
    {"label": "doman",            "start": 399,  "category": "field",     "display": "Field (doman)"},
    {"label": "bubu1",            "start": 407,  "category": "field",     "display": "Usha Research Center"},
    {"label": "bubu2",            "start": 416,  "category": "field",     "display": "Usha (deeper)"},
    {"label": "taiku2",           "start": 425,  "category": "field",     "display": "Muscle Dome (later)"},
    {"label": "uru",              "start": 434,  "category": "field",     "display": "Uru Mais"},
    {"label": "uru2",             "start": 444,  "category": "field",     "display": "Uru Mais (deeper)"},
    {"label": "urudre1",          "start": 454,  "category": "field",     "display": "Uru Mais (dream 1)"},
    {"label": "urudre2",          "start": 465,  "category": "field",     "display": "Uru Mais (dream 2)"},
    {"label": "urudre3",          "start": 474,  "category": "field",     "display": "Uru Mais (dream 3)"},
    {"label": "kor",              "start": 483,  "category": "field",     "display": "Field (kor)"},
    {"label": "kor3",             "start": 492,  "category": "field",     "display": "Field (kor3)"},
    {"label": "kor4",             "start": 501,  "category": "field",     "display": "Field (kor4)"},
    {"label": "kor5",             "start": 509,  "category": "field",     "display": "Field (kor5)"},
    {"label": "korb2",            "start": 517,  "category": "field",     "display": "Field (korb2)"},
    {"label": "korb3",            "start": 524,  "category": "field",     "display": "Field (korb3)"},
    {"label": "korout",           "start": 533,  "category": "field",     "display": "Field (korout)"},
    {"label": "koin1",            "start": 542,  "category": "field",     "display": "Soren Camp"},
    {"label": "koin2",            "start": 551,  "category": "field",     "display": "Field (koin2)"},
    {"label": "koin3",            "start": 561,  "category": "field",     "display": "Field (koin3)"},
    {"label": "koin4",            "start": 570,  "category": "field",     "display": "Field (koin4)"},
    {"label": "koin6",            "start": 578,  "category": "field",     "display": "Field (koin6)"},
    {"label": "juui1",            "start": 587,  "category": "field",     "display": "Juggernaut interior 1"},
    {"label": "juui2",            "start": 596,  "category": "field",     "display": "Juggernaut interior 2"},
    {"label": "deroa",            "start": 605,  "category": "field",     "display": "Field (deroa)"},
    {"label": "station3",         "start": 615,  "category": "field",     "display": "Karisto Station (late)"},
    {"label": "conc2",            "start": 623,  "category": "field",     "display": "Conkram (late)"},
    {"label": "jou",              "start": 630,  "category": "field",     "display": "Castle"},
    {"label": "nilboa",           "start": 637,  "category": "field",     "display": "Nivora Ravine"},
    {"label": "nilboa2",          "start": 646,  "category": "field",     "display": "Nivora Ravine (deeper)"},
    {"label": "jouina",           "start": 655,  "category": "field",     "display": "Castle interior A"},
    {"label": "jouinb",           "start": 664,  "category": "field",     "display": "Castle interior B"},
    {"label": "jouinc",           "start": 672,  "category": "field",     "display": "Castle interior C"},
    {"label": "jouind",           "start": 680,  "category": "field",     "display": "Castle interior D"},
    {"label": "jouine",           "start": 688,  "category": "field",     "display": "Castle interior E"},
    {"label": "rugi",             "start": 696,  "category": "field",     "display": "Field (rugi)"},
    {"label": "chitei2",          "start": 705,  "category": "field",     "display": "Underground Octam"},
    {"label": "noaru",            "start": 716,  "category": "field",     "display": "Noaru Valley"},
    {"label": "concend",          "start": 725,  "category": "field",     "display": "Conkram (final)"},
    {"label": "conc3",            "start": 733,  "category": "field",     "display": "Conkram (epilogue)"},
    {"label": "town0e",           "start": 741,  "category": "town",      "display": "Town (0e)"},
    # OP cutscenes
    {"label": "opdeene",          "start": 748,  "category": "cutscene",  "display": "Opening (Buma)"},
    {"label": "opstati",          "start": 753,  "category": "cutscene",  "display": "Opening (station)"},
    {"label": "opkorout",         "start": 758,  "category": "cutscene",  "display": "Opening (korout)"},
    {"label": "opurud",           "start": 763,  "category": "cutscene",  "display": "Opening (Uru)"},
    {"label": "opmap01",          "start": 768,  "category": "cutscene",  "display": "Opening (map)"},
    {"label": "koin1b",           "start": 773,  "category": "field",     "display": "Soren Camp (alt)"},
    # ED cutscenes
    {"label": "edteien",          "start": 780,  "category": "cutscene",  "display": "Ending (Sky Gardens)"},
    {"label": "edbylon",          "start": 785,  "category": "cutscene",  "display": "Ending (Biron)"},
    {"label": "edbalden",         "start": 790,  "category": "cutscene",  "display": "Ending (Vidna)"},
    {"label": "edlast",           "start": 795,  "category": "cutscene",  "display": "Ending (final)"},
    {"label": "edretoin",         "start": 800,  "category": "cutscene",  "display": "Ending (retock)"},
    {"label": "edkorout",         "start": 805,  "category": "cutscene",  "display": "Ending (korout)"},
    {"label": "edbubu",           "start": 810,  "category": "cutscene",  "display": "Ending (Usha)"},
    {"label": "eddoman",          "start": 815,  "category": "cutscene",  "display": "Ending (doman)"},
    {"label": "edson",            "start": 820,  "category": "cutscene",  "display": "Ending (son)"},
    {"label": "edstati3",         "start": 825,  "category": "cutscene",  "display": "Ending (station3)"},
    # Battle data
    {"label": "battle_data",      "start": 865,  "category": "battle",    "display": "Battle data (party/monster TMDs + textures)"},
    {"label": "monster_data",     "start": 869,  "category": "battle",    "display": "Monster data"},
    {"label": "sound_data",       "start": 870,  "category": "audio",     "display": "Sound data (driver outputs)"},
    {"label": "befect_data",      "start": 872,  "category": "battle",    "display": "Battle effect data"},
    {"label": "player_data",      "start": 876,  "category": "battle",    "display": "Player data (TMDs / arts)"},
    {"label": "sound_data2",      "start": 877,  "category": "audio",     "display": "Sound data (dev branch)"},
    {"label": "level_up",         "start": 891,  "category": "system",    "display": "Level-up overlay + VABs"},
    {"label": "monster_se",       "start": 893,  "category": "audio",     "display": "Monster SFX"},
    {"label": "card_data",        "start": 894,  "category": "system",    "display": "Card data"},
    {"label": "bat_back_dat",     "start": 895,  "category": "battle",    "display": "Battle backgrounds"},
    {"label": "xxx_dat",          "start": 897,  "category": "system",    "display": "Scene-scripted asset table"},
    {"label": "move_program_no",  "start": 972,  "category": "system",    "display": "Move-program overlay"},
    {"label": "other_game",       "start": 974,  "category": "system",    "display": "Other-game overlays"},
    {"label": "monster_test",     "start": 980,  "category": "system",    "display": "Monster test scenes"},
    {"label": "music_01",         "start": 990,  "category": "audio",     "display": "Music (BGM SEQs)"},
    {"label": "vab_01",           "start": 1072, "category": "audio",     "display": "VAB sound banks"},
    {"label": "other1",           "start": 1195, "category": "system",    "display": "Other (1)"},
    {"label": "other4",           "start": 1200, "category": "system",    "display": "Other (4)"},
    {"label": "other5",           "start": 1203, "category": "system",    "display": "Other (5)"},
    {"label": "other6",           "start": 1222, "category": "system",    "display": "Other (6)"},
    {"label": "other7",           "start": 1228, "category": "system",    "display": "Other (7)"},
]


def build_scenes_json() -> list[dict]:
    """Expand CDNAME_SCENES into a sorted list with prot_end inclusive.

    The last entry runs to PROT_MAX so the viewer always has coverage.
    """
    PROT_MAX = 1233
    entries = sorted(CDNAME_SCENES, key=lambda s: s["start"])
    out: list[dict] = []
    for i, s in enumerate(entries):
        end = entries[i + 1]["start"] - 1 if i + 1 < len(entries) else PROT_MAX
        out.append({
            "label": s["label"],
            "display": s["display"],
            "category": s["category"],
            "prot_start": s["start"],
            "prot_end": end,
        })
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

    # Reverse map: walkthrough town name -> scene label + category from
    # the curated CDNAME map. The walkthrough's town names don't perfectly
    # match CDNAME labels (Vidna == balden, etc.), so we hand-map.
    TOWN_TO_SCENE = {
        "Rim Elm":              "town01",
        "Hunter's Spring":      "izumi",
        "Drake Castle":         "vell",
        "Biron Monastery":      "bylon",
        "Wind Cave":            None,        # not a CDNAME scene we ID
        "Jeremi":               "geremi",
        "Vidna":                "balden",
        "Octam":                None,        # likely town0b / town0c / town0e
        "Underground Octam":    "chitei2",
        "Ratayu":               "rayman",
        "Karisto Station":      "station",
        "Sol":                  "town0d",
        "Buma":                 "deene",
        "Usha Research Center": "bubu1",
        "Soren Camp":           "koin1",
        "Conkram":              "conc",
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
    lines = header + sorted(generated) + [""]
    (ROOT / ".gitignore").write_text("\n".join(lines))


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

        body = autolink_md_refs(body, md_paths, md_by_basename)

        html = html_template(title, depth, active, body, extra_head)
        out = ROOT / out_path
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(html)
        written += 1
        generated.append(out_path)
        print(f"  wrote {out_path}")

        # Build search index entries from body fragment
        search_index.extend(
            build_search_entries(out_path, title, body, section_label_for(out_path))
        )

    # Write search-index.json
    idx_path = ROOT / "search-index.json"
    idx_path.write_text(json.dumps(search_index, ensure_ascii=False, separators=(",", ":")))
    generated.append("search-index.json")

    # Write scenes.json (CDNAME -> category map for the asset viewer's
    # Scene filter).
    scenes_payload = build_scenes_json()
    (ROOT / "scenes.json").write_text(
        json.dumps(scenes_payload, ensure_ascii=False, separators=(",", ":"))
    )
    generated.append("scenes.json")

    # Write shops.json + world.json + minigames.json (gamedata join for the
    # interactive shops / world / minigames pages).
    shops_payload, world_payload, minigames_payload = build_gamedata_json()
    (ROOT / "shops.json").write_text(
        json.dumps(shops_payload, ensure_ascii=False, separators=(",", ":"))
    )
    (ROOT / "world.json").write_text(
        json.dumps(world_payload, ensure_ascii=False, separators=(",", ":"))
    )
    (ROOT / "minigames.json").write_text(
        json.dumps(minigames_payload, ensure_ascii=False, separators=(",", ":"))
    )
    generated += ["shops.json", "world.json", "minigames.json"]

    arts_payload = build_arts_json()
    (ROOT / "arts.json").write_text(
        json.dumps(arts_payload, ensure_ascii=False, separators=(",", ":"))
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
    return 0


if __name__ == "__main__":
    sys.exit(main())
