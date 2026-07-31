#!/usr/bin/env python3
"""Find @media declarations that a LATER unconditional rule silently shadows.

A media query contributes NOTHING to specificity. `@media (max-width: 880px)
{ .rail { display: none } }` and a plain `.rail { display: flex }` are both
specificity (0,1,0), so the one that appears later in the source wins - at
every viewport width. Put the media block above the rule it means to override
and the breakpoint is dead code that still reads like a working feature.

That is not a hypothetical: it is how the site shipped a mobile layout whose
icon rail never hid, leaving the content column laid out full-width and
painted underneath a 76px fixed rail.

The check is deliberately narrow, so a finding is a defect and not a style
opinion. It reports a pair only when ALL of these hold:

  - the same normalised selector text appears in both rules;
  - the later rule is unconditional (no @media / @supports ancestor);
  - the properties collide (same property, or a shorthand covering it);
  - the later rule's specificity is not lower than the media rule's;
  - the media declaration is not `!important` while the later one is not.

Shapes it does NOT flag, because the later rule legitimately wins:
a more specific later selector (`.sidebar.open` after `.sidebar`), a later
media block overriding an earlier one, and a generalisation of the media
selector (which is less specific and therefore loses anyway).

Usage:
    python3 scripts/ci/check-css-cascade-order.py            # scan site/
    python3 scripts/ci/check-css-cascade-order.py --selftest # controls only
    python3 scripts/ci/check-css-cascade-order.py a.css b.css

Exit status is non-zero when any shadowed declaration is found, so it runs as
a gate. The fix is always the same: move the @media block below the rules it
overrides. `site/css/styles.css` keeps a "Responsive shell" section at the end
of the file for exactly this reason.
"""
from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
DEFAULT_ROOTS = ["site"]

# A later `padding` wipes an earlier `padding-top`; the collision test has to
# know which longhands each shorthand swallows.
SHORTHAND: dict[str, list[str]] = {
    "margin": ["margin-top", "margin-right", "margin-bottom", "margin-left"],
    "padding": ["padding-top", "padding-right", "padding-bottom", "padding-left"],
    "inset": ["top", "right", "bottom", "left"],
    "border": ["border-width", "border-style", "border-color",
               "border-top", "border-right", "border-bottom", "border-left"],
    "background": ["background-color", "background-image", "background-position",
                   "background-size", "background-repeat"],
    "font": ["font-size", "font-family", "font-weight", "font-style", "line-height"],
    "flex": ["flex-grow", "flex-shrink", "flex-basis"],
    "grid-template": ["grid-template-columns", "grid-template-rows", "grid-template-areas"],
    "grid-area": ["grid-row-start", "grid-column-start", "grid-row-end", "grid-column-end"],
    "place-items": ["align-items", "justify-items"],
    "place-content": ["align-content", "justify-content"],
    "overflow": ["overflow-x", "overflow-y"],
    "gap": ["row-gap", "column-gap"],
}

CONDITIONAL_AT_RULES = ("@media", "@supports", "@container")


@dataclass
class Decl:
    line: int
    sel: str
    prop: str
    value: str
    important: bool
    conditions: tuple[str, ...]
    source: str = ""
    # Position of this declaration's stylesheet in the cascade (0 = first
    # loaded). Page-level <style> blocks come after the linked stylesheet.
    sheet: int = 0

    @property
    def order(self) -> tuple[int, int]:
        return (self.sheet, self.line)


@dataclass
class Finding:
    media: Decl
    later: Decl

    def render(self) -> str:
        where = (f"line {self.later.line}" if self.later.source == self.media.source
                 else f"{self.later.source}:{self.later.line}")
        return (
            f"{self.media.source}:{self.media.line}: `{self.media.sel} {{ "
            f"{self.media.prop}: {self.media.value} }}` inside "
            f"`{' '.join(self.media.conditions)}`\n"
            f"    is shadowed at {where} by the unconditional "
            f"`{self.later.sel} {{ {self.later.prop}: {self.later.value} }}`\n"
            f"    -> the breakpoint never applies. Move the @media block after it."
        )


def strip_comments(text: str) -> str:
    """Remove /* */ comments, preserving newlines so line numbers hold."""
    out: list[str] = []
    i = 0
    while i < len(text):
        if text.startswith("/*", i):
            j = text.find("*/", i + 2)
            if j < 0:
                j = len(text)
            out.append("\n" * text.count("\n", i, j + 2))
            i = j + 2
        else:
            out.append(text[i])
            i += 1
    return "".join(out)


def norm_sel(s: str) -> str:
    s = re.sub(r"\s+", " ", s.strip())
    return re.sub(r"\s*([>+~])\s*", r" \1 ", s)


def specificity(sel: str) -> tuple[int, int, int]:
    s = re.sub(r"::[a-zA-Z-]+", " ELEM ", sel)
    ids = len(re.findall(r"#[\w-]+", s))
    classes = (len(re.findall(r"\.[\w-]+", s))
               + len(re.findall(r"\[[^\]]*\]", s))
               + len(re.findall(r":(?!not\()[\w-]+", s)))
    elems = len(re.findall(r"(?:^|[\s>+~])([a-zA-Z][\w-]*)", s)) + s.count("ELEM")
    return (ids, classes, elems)


def _split_top_level(text: str, sep: str) -> list[str]:
    parts, depth, cur = [], 0, []
    for ch in text:
        if ch in "([":
            depth += 1
        elif ch in ")]":
            depth -= 1
        if ch == sep and depth == 0:
            parts.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
    parts.append("".join(cur))
    return [p for p in (x.strip() for x in parts) if p]


def parse(text: str) -> list[Decl]:
    """Walk the stylesheet, recording every declaration with its @-conditions."""
    decls: list[Decl] = []
    n = len(text)

    def line_at(pos: int) -> int:
        return text.count("\n", 0, pos) + 1

    def skip_block(pos: int) -> int:
        depth = 1
        while pos < n and depth:
            if text[pos] == "{":
                depth += 1
            elif text[pos] == "}":
                depth -= 1
            pos += 1
        return pos

    def read_body(pos: int) -> tuple[int, str, int]:
        start, depth = pos, 1
        while pos < n and depth:
            if text[pos] == "{":
                depth += 1
            elif text[pos] == "}":
                depth -= 1
                if depth == 0:
                    break
            pos += 1
        return pos + 1, text[start:pos], start

    def one_decl(chunk: str, abs_pos: int) -> list[tuple[int, str, str, bool]]:
        if ":" not in chunk:
            return []
        prop, _, value = chunk.partition(":")
        prop = prop.strip().lower()
        if not prop or prop.startswith("--") or " " in prop:
            return []
        value = value.strip()
        return [(line_at(abs_pos + chunk.index(":")), prop, value,
                 "!important" in value.lower())]

    def split_decls(body: str, body_start: int) -> list[tuple[int, str, str, bool]]:
        out: list[tuple[int, str, str, bool]] = []
        depth, start = 0, 0
        for idx, ch in enumerate(body):
            if ch == "(":
                depth += 1
            elif ch == ")":
                depth -= 1
            elif ch == ";" and depth == 0:
                out.extend(one_decl(body[start:idx], body_start + start))
                start = idx + 1
        out.extend(one_decl(body[start:], body_start + start))
        return out

    def block(pos: int, conditions: list[str]) -> int:
        buf: list[str] = []
        while pos < n:
            ch = text[pos]
            if ch == "{":
                prelude = "".join(buf).strip()
                buf = []
                if prelude.startswith("@"):
                    at = prelude.split()[0].lower()
                    if at in CONDITIONAL_AT_RULES:
                        pos = block(pos + 1, conditions + [prelude])
                    else:
                        # @keyframes / @font-face / @page: not a cascade layer
                        # this check reasons about.
                        pos = skip_block(pos + 1)
                    continue
                pos, body, body_start = read_body(pos + 1)
                for sel in _split_top_level(prelude, ","):
                    for ln, prop, value, imp in split_decls(body, body_start):
                        decls.append(Decl(ln, norm_sel(sel), prop, value, imp,
                                          tuple(conditions)))
                continue
            if ch == "}":
                return pos + 1
            if ch == ";" and "".join(buf).strip().startswith("@"):
                buf = []  # @import / @charset
                pos += 1
                continue
            buf.append(ch)
            pos += 1
        return pos

    block(0, [])
    return decls


def covers(prop: str) -> set[str]:
    return {prop} | set(SHORTHAND.get(prop, []))


STYLE_BLOCK = re.compile(r"<style[^>]*>(.*?)</style>", re.S | re.I)


def html_style_sheet(text: str) -> str:
    """Blank out everything but <style> bodies, keeping line numbers intact.

    A page's style blocks cascade in document order, so flattening them into
    one line-aligned pseudo-stylesheet gives both the right ordering and file
    line numbers that point at the real source.
    """
    out = list("\n" if ch == "\n" else " " for ch in text)
    for m in STYLE_BLOCK.finditer(text):
        start, end = m.start(1), m.end(1)
        out[start:end] = list(text[start:end])
    return "".join(out)


def scan_sources(sources: list[tuple[str, str]]) -> list[Finding]:
    """Scan stylesheets given in cascade order as (label, css_text) pairs."""
    decls: list[Decl] = []
    for sheet, (label, text) in enumerate(sources):
        for d in parse(strip_comments(text)):
            d.source = label
            d.sheet = sheet
            decls.append(d)
    conditional = [d for d in decls if d.conditions]
    unconditional = [d for d in decls if not d.conditions]
    findings: list[Finding] = []
    for m in conditional:
        m_spec = specificity(m.sel)
        m_props = covers(m.prop)
        for later in unconditional:
            if later.order <= m.order or later.sel != m.sel:
                continue
            if not (covers(later.prop) & m_props):
                continue
            if m.important and not later.important:
                continue
            if specificity(later.sel) < m_spec:
                continue
            findings.append(Finding(m, later))
    return findings


def scan_text(path: str, text: str) -> list[Finding]:
    return scan_sources([(path, text)])


# --- controls ---------------------------------------------------------------
# The exact shape the site shipped, plus the shorthand-swallows-longhand
# variant, plus the three shapes that must NOT fire.
SELFTEST_SHADOWED: list[tuple[str, str]] = [
    ("equal-specificity display",
     "@media (max-width: 880px) { .rail { display: none; } }\n.rail { display: flex; }"),
    ("shorthand swallows longhand",
     "@media (max-width: 880px) { .content { padding-top: 64px; } }\n"
     ".content { padding: 2rem 2.4rem 4rem; }"),
    ("later rule is more specific",
     "@media (max-width: 700px) { .a { color: red; } }\n.a { color: blue; }"),
    ("@supports counts too",
     "@supports (display: grid) { .g { display: grid; } }\n.g { display: block; }"),
]
SELFTEST_CLEAN: list[tuple[str, str]] = [
    ("media block comes last",
     ".rail { display: flex; }\n@media (max-width: 880px) { .rail { display: none; } }"),
    ("later selector is a different one",
     "@media (max-width: 880px) { .rail { display: none; } }\n.sidebar { display: flex; }"),
    ("later rule sets a different property",
     "@media (max-width: 880px) { .rail { display: none; } }\n.rail { color: red; }"),
    ("later rule is less specific",
     "@media (max-width: 880px) { .zone-docs .app { margin-left: 0; } }\n.app { margin-left: 76px; }"),
    ("media declaration is !important",
     "@media (max-width: 880px) { .rail { display: none !important; } }\n.rail { display: flex; }"),
    ("keyframes are not rules",
     "@media (max-width: 880px) { .rail { display: none; } }\n"
     "@keyframes spin { from { display: flex; } to { display: block; } }"),
]


def run_selftest() -> int:
    bad = 0
    for name, css in SELFTEST_SHADOWED:
        if not scan_text("<control>", css):
            print(f"  MISS  positive control did not fire: {name}")
            bad += 1
        else:
            print(f"  ok    fires: {name}")
    for name, css in SELFTEST_CLEAN:
        hits = scan_text("<control>", css)
        if hits:
            print(f"  FALSE positive control fired on clean case: {name}")
            bad += 1
        else:
            print(f"  ok    silent: {name}")
    if bad:
        print(f"\nself-test: {bad} control(s) failed")
        return 1
    print(f"\nself-test: all {len(SELFTEST_SHADOWED) + len(SELFTEST_CLEAN)} cases pass")
    return 0


def iter_sources(roots: list[Path]) -> tuple[list[Path], list[Path]]:
    """(stylesheets, html pages with <style> blocks) under the given roots."""
    css: list[Path] = []
    html: list[Path] = []
    for root in roots:
        if root.is_file():
            (css if root.suffix == ".css" else html).append(root)
            continue
        if not root.is_dir():
            continue
        css.extend(sorted(p for p in root.rglob("*.css")))
        # Only the authored fragments: the generated pages under site/ are
        # built from them, so scanning both would double-report.
        html.extend(sorted(p for p in (root / "_content").rglob("*.html"))
                    if (root / "_content").is_dir()
                    else sorted(p for p in root.rglob("*.html")))
    return css, html


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("paths", nargs="*", help="css files or dirs (default: site/)")
    ap.add_argument("--selftest", action="store_true",
                    help="run the positive/negative controls and exit")
    ap.add_argument("--quiet", action="store_true", help="only print findings")
    args = ap.parse_args()

    if args.selftest:
        print("check-css-cascade-order self-test")
        return run_selftest()

    # A "clean" verdict from a detector that never matches is the failure this
    # gate exists to prevent, so the controls run on every invocation.
    for name, css in SELFTEST_SHADOWED:
        if not scan_text("<control>", css):
            print(f"ERROR: built-in positive control '{name}' failed; result is "
                  "not trustworthy. Run --selftest.", file=sys.stderr)
            return 2

    roots = [Path(p) for p in args.paths] if args.paths else [REPO_ROOT / r for r in DEFAULT_ROOTS]
    css_files, html_files = iter_sources(roots)

    def label(p: Path) -> str:
        try:
            return str(p.relative_to(REPO_ROOT))
        except ValueError:
            return str(p)

    # The linked stylesheets load first; a page's own <style> blocks come after
    # them, so an unconditional page rule can shadow a stylesheet breakpoint.
    # Each page is scanned against that shared prefix.
    prefix = [(label(f), f.read_text(errors="replace")) for f in css_files]

    findings: list[Finding] = []
    findings.extend(scan_sources(prefix))
    for h in html_files:
        text = h.read_text(errors="replace")
        if "<style" not in text.lower():
            continue
        page_findings = scan_sources(prefix + [(label(h), html_style_sheet(text))])
        # Drop the ones already reported from the shared prefix alone.
        findings.extend(f for f in page_findings if h.name in f.later.source
                        or h.name in f.media.source)

    if not args.quiet:
        print(f"scanned {len(css_files)} stylesheet(s) + "
              f"{len(html_files)} page(s) (positive controls: passed)")

    if findings:
        print()
        for fi in findings:
            print(fi.render())
            print()
        print(f"{len(findings)} shadowed @media declaration(s)")
        return 1

    if not args.quiet:
        print("no shadowed @media declarations")
    return 0


if __name__ == "__main__":
    sys.exit(main())
