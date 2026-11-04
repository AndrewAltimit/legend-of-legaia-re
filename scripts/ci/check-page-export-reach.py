#!/usr/bin/env python3
"""Every play-page wasm export is reached by the page, or says why not.

`LegaiaRuntime` exports a method for each thing the browser play page can
do. An export no site source names is either a probe a headless driver
reads, a step the page's tick export runs internally, a debug entry, a
superseded surface - or a feature the page never offers. The last kind is a
host-drift defect no other gate sees: `play_fishing_start` was exported,
tested by a Rust oracle and documented, and no page control called it, so the
page could not start fishing the way the native window's `L` key does.

This gate lists the `pub fn`s of every `#[wasm_bindgen] impl LegaiaRuntime`
block under `crates/web-play/src/` and requires each to be

- named by a `site/` source (`site/js/*.js`, `site/_content/*.html`) as a
  whole word, or formed there from a string prefix and a called name (the
  `LegaiaFieldActors.api(rt, 'play_npc_')` binding), or
- covered by a row of `scripts/ci/page-export-reach-waivers.toml`, which
  names its `kind` and a reason.

A waiver that matches no export, or matches one the page now reaches, is
stale and fails too, so the table cannot outlive what it excuses.

It also holds the other direction of the play bundle. A page in
`scripts/ci/page-bundles.toml` loads `site/wasm/play/`, which has no inspector,
patcher or workbench exports, and a page that names one fails only when a player
reaches that line, as `x is not a function`, or not at all where the page
tests for the export first. So no source such a page loads - its own HTML, its
`<script src>`s, the template's, and anything those import - may name a
class or free function only the tool bundle's crates export. Comments do not count.
A method is reachable only through its class, so the classes and free functions
are the whole surface.

    python3 scripts/ci/check-page-export-reach.py          # check
    python3 scripts/ci/check-page-export-reach.py --list   # every unreached export and its waiver

Reference: docs/tooling/host-drift.md#page-export-reach.
"""

from __future__ import annotations

import argparse
import fnmatch
import re
import sys
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
WEB_PLAY_SRC = REPO / "crates" / "web-play" / "src"
SITE_SOURCES = [REPO / "site" / "js", REPO / "site" / "_content"]
PAGE_BUNDLES = REPO / "scripts" / "ci" / "page-bundles.toml"
CRATES = REPO / "crates"
# The crates each bundle links that the other does not count here: the play bundle
# is `web-play` over `web-minigames`, and the tool bundle is those plus these.
TOOL_ONLY_CRATES = ("web-viewer", "web-patcher", "web-expansion")
PLAY_CRATES = ("web-play", "web-minigames")
# What `html_template` in site/_gen.py puts on every page.
TEMPLATE_SCRIPTS = ("site-util.js", "wasm-loader.js", "layout.js", "main.js")
WAIVERS = REPO / "scripts" / "ci" / "page-export-reach-waivers.toml"
KINDS = {
    "probe": "a read-only state or diagnostic read a headless driver or oracle takes",
    "tick-internal": "a step the page's own tick export already runs",
    "driver": "a debug entry a headless driver arms, documented beside it",
    "superseded": "an older surface a newer export replaced on the page",
    "both-absent": "a control neither host offers",
}

IMPL_RE = re.compile(r"#\[wasm_bindgen[^\]]*\]\s*impl\s+LegaiaRuntime\s*\{")
FN_RE = re.compile(r"\n    pub fn ([a-z0-9_]+)\s*(?:<[^>]*>)?\s*\(")


def runtime_exports() -> dict[str, Path]:
    """`name -> file` for every method of a `#[wasm_bindgen] impl LegaiaRuntime`."""
    out: dict[str, Path] = {}
    for path in sorted(WEB_PLAY_SRC.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        for m in IMPL_RE.finditer(text):
            depth, i = 0, m.end() - 1
            start = i
            while i < len(text):
                if text[i] == "{":
                    depth += 1
                elif text[i] == "}":
                    depth -= 1
                    if depth == 0:
                        break
                i += 1
            for f in FN_RE.finditer(text[start:i]):
                out.setdefault(f.group(1), path.relative_to(REPO))
    return out


def site_text() -> str:
    parts = []
    for root in SITE_SOURCES:
        for path in sorted(root.rglob("*")):
            if path.suffix in (".js", ".html") and path.is_file() and "wasm" not in path.parts:
                parts.append(path.read_text(encoding="utf-8", errors="replace"))
    return "\n".join(parts)


def reached_names(text: str) -> set[str]:
    words = set(re.findall(r"\b[a-z][a-z0-9_]*\b", text))
    prefixes = set(re.findall(r"['\"]([a-z][a-z0-9_]*_)['\"]", text))
    called = set(re.findall(r"\.([a-z][a-z0-9_]*)\s*\(", text)) | set(
        re.findall(r"['\"]([a-z][a-z0-9_]*)['\"]", text)
    )
    return words | {p + c for p in prefixes for c in called}


EXPORT_RE = re.compile(
    r"#\[wasm_bindgen([^\]]*)\](?:\s*#\[[^\]]*\])*\s*pub\s+(?:async\s+)?(?:fn|struct|enum)\s+([A-Za-z0-9_]+)"
)
JS_NAME_RE = re.compile(r"js_name\s*=\s*\"?([A-Za-z0-9_]+)")


def module_names(crates: tuple[str, ...]) -> set[str]:
    """The classes and free functions these crates add to a module's namespace."""
    out: set[str] = set()
    for crate in crates:
        for path in sorted((CRATES / crate / "src").rglob("*.rs")):
            for attrs, name in EXPORT_RE.findall(path.read_text(encoding="utf-8")):
                js = JS_NAME_RE.search(attrs)
                out.add(js.group(1) if js else name)
    return out


def tool_only_names() -> set[str]:
    return module_names(TOOL_ONLY_CRATES) - module_names(PLAY_CRATES)


def strip_comments(text: str) -> str:
    """Source text without `/* */`, `<!-- -->` and line comments: prose may name anything."""
    text = re.sub(r"/\*.*?\*/", " ", text, flags=re.S)
    text = re.sub(r"<!--.*?-->", " ", text, flags=re.S)
    return re.sub(r"(^|[\s;{}(),])//[^\n]*", r"\1", text)


def page_sources(key: str) -> list[Path]:
    """A page's HTML and every site script it loads, following imports."""
    js_root = REPO / "site" / "js"
    page = REPO / "site" / "_content" / f"{key}.html"
    seen: dict[Path, None] = {page: None}
    queue = [page] + [js_root / n for n in TEMPLATE_SCRIPTS]
    while queue:
        path = queue.pop()
        seen[path] = None
        text = path.read_text(encoding="utf-8", errors="replace")
        refs = re.findall(r"""src=["'](?:\.\./)*js/([^"'?]+)""", text)
        refs += re.findall(r"""(?:from\s+|import\s*\(\s*|import\s+)["']\./([^"'?]+\.js)""", text)
        refs += re.findall(r"""new URL\(\s*["']\./([^"'?]+\.js)""", text)
        for ref in refs:
            dep = js_root / ref
            if dep.is_file() and dep not in seen:
                seen[dep] = None
                queue.append(dep)
    return list(seen)


def named_tool_exports(text: str, tool_only: set[str]) -> set[str]:
    return tool_only & set(re.findall(r"[A-Za-z_][A-Za-z0-9_]*", strip_comments(text)))


def check_page_bundles(tool_only: set[str]) -> tuple[list[str], int]:
    """Every problem, and how many pages the table puts on the play bundle."""
    keys = tomllib.loads(PAGE_BUNDLES.read_text(encoding="utf-8"))["play"]
    problems: list[str] = []
    for key in keys:
        if not (REPO / "site" / "_content" / f"{key}.html").is_file():
            problems.append(f"PAGE BUNDLE {key!r} in {PAGE_BUNDLES.name}: no such page")
            continue
        for path in page_sources(key):
            text = path.read_text(encoding="utf-8", errors="replace")
            for name in sorted(named_tool_exports(text, tool_only)):
                problems.append(
                    f"WRONG BUNDLE {key}.html loads {path.relative_to(REPO)}, which names `{name}`: only the "
                    f"tool bundle exports it. Move the export into {' or '.join(PLAY_CRATES)}, or take the "
                    f"page off the play row of {PAGE_BUNDLES.relative_to(REPO)}"
                )
    return problems, len(keys)


def selftest(tool_only: set[str]) -> list[str]:
    """The rule against planted text, so a broken export scan cannot read as a clean page."""
    out = []
    for must in ("LegaiaViewer", "ExpansionWorkbench", "Workbench", "patch_rom"):
        if must not in tool_only:
            out.append(f"SELFTEST: `{must}` is not among the tool-only exports; the scan is broken")
    for must_not in ("LegaiaRuntime", "LegaiaMinigames", "LegaiaModes", "card_saves_json"):
        if must_not in tool_only:
            out.append(f"SELFTEST: `{must_not}` reads as tool-only; the scan is broken")
    if named_tool_exports("const v = new wasm.LegaiaViewer();", tool_only) != {"LegaiaViewer"}:
        out.append("SELFTEST: a constructed tool class was not caught")
    if named_tool_exports("wasm.patch_rom(a); // keep", tool_only) != {"patch_rom"}:
        out.append("SELFTEST: a called tool function was not caught")
    prose = "/* `LegaiaViewer`'s exports */\n// patch_rom\n<!-- Workbench -->\nconst u = 'https://x/y';"
    if named_tool_exports(prose, tool_only):
        out.append("SELFTEST: a comment read as a use")
    return out


def load_waivers() -> list[dict]:
    if not WAIVERS.is_file():
        return []
    rows = tomllib.loads(WAIVERS.read_text(encoding="utf-8")).get("waiver", [])
    return rows


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--list", action="store_true", help="print every unreached export")
    args = ap.parse_args()

    exports = runtime_exports()
    reached = reached_names(site_text())
    waivers = load_waivers()
    problems: list[str] = []

    for i, w in enumerate(waivers):
        kind, reason, names = w.get("kind"), w.get("reason", "").strip(), w.get("names", [])
        if kind not in KINDS:
            problems.append(f"WAIVER #{i + 1}: kind {kind!r} is not one of {sorted(KINDS)}")
        if not reason:
            problems.append(f"WAIVER #{i + 1} ({kind}): no reason")
        for pat in names:
            hits = fnmatch.filter(exports, pat)
            if not hits:
                problems.append(f"STALE WAIVER {pat!r} ({kind}): matches no LegaiaRuntime export")
            for h in hits:
                if h in reached:
                    problems.append(
                        f"STALE WAIVER {pat!r} ({kind}): `{h}` is named by the page now - drop it"
                    )

    def waiver_for(name: str):
        for w in waivers:
            if any(fnmatch.fnmatchcase(name, p) for p in w.get("names", [])):
                return w
        return None

    unreached = sorted(n for n in exports if n not in reached)
    for name in unreached:
        w = waiver_for(name)
        if args.list:
            tag = f"{w['kind']}" if w else "UNWAIVED"
            print(f"  {tag:13} {exports[name]}: {name}")
        if w is None:
            problems.append(
                f"UNREACHED EXPORT {exports[name]}: `{name}` - no site source names it. Wire the "
                f"page control it stands for, or waive it in {WAIVERS.relative_to(REPO)} with a kind "
                f"and a reason"
            )

    tool_only = tool_only_names()
    bundle_problems, play_pages = check_page_bundles(tool_only)
    problems += selftest(tool_only) + bundle_problems

    print(
        f"[page-export-reach] {len(exports)} LegaiaRuntime exports, {len(exports) - len(unreached)} "
        f"named by the page, {len(unreached)} waived or flagged"
    )
    if problems:
        for p in problems:
            print(f"  - {p}")
        return 1
    print(
        f"[page-export-reach] {play_pages} pages on the play bundle name none of the "
        f"{len(tool_only)} tool-only exports"
    )
    print("[page-export-reach] ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
