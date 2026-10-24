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

    print(
        f"[page-export-reach] {len(exports)} LegaiaRuntime exports, {len(exports) - len(unreached)} "
        f"named by the page, {len(unreached)} waived or flagged"
    )
    if problems:
        for p in problems:
            print(f"  - {p}")
        return 1
    print("[page-export-reach] ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
