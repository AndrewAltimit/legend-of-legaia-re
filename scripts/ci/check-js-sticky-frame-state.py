#!/usr/bin/env python3
"""Detect sticky renderer state the play page stages on one draw branch only.

The browser play page's frame loop (`site/js/play-app.js`, `_frame`) draws
through one of three branches - an in-world minigame venue, a battle, or the
field - over ONE `TmdRenderer` (`site/js/webgl-tmd.js`). Several renderer
setters store a value the renderer keeps until the next call (the NCLIP cull
word, the prologue colour grade and depth-cue ramp, the palette-collapse half
of the grade, the overworld curvature). A setter like that, called inside one
branch only, leaks the last value that branch left into every other branch.

That is not hypothetical: all five were staged inside the field branch, so a
battle drew under the last field frame's state - the field's NCLIP cull armed
on the stage dome, and every fight entered from the overworld under the
overworld's screen-Y bend (the mountains folded out of the battle backdrop).
The native window stages the same five once a frame ahead of its mode
branches; no Rust-side tier can see the page's GL state, so this audit reads
the two js files.

The audit:

  1. finds every `set*` method of `TmdRenderer` whose body makes no `gl.` call
     - a pure state store, i.e. sticky;
  2. finds every `this.renderer.<setter>(` call in `play-app.js`;
  3. requires each such call to sit inside `_stageFrameState` (the one
     per-frame staging function, called ahead of every branch), unless the
     setter is declared in BRANCH_OWNED with the reason its value cannot leak.

A new sticky setter fails until it is either staged in `_stageFrameState` or
classified here - which is the point: the decision is made once, on purpose.

Usage:
    python3 scripts/ci/check-js-sticky-frame-state.py            # audit
    python3 scripts/ci/check-js-sticky-frame-state.py --selftest # controls only

Exit status: 0 = clean, 1 = findings, 2 = self-test failed.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
RENDERER_JS = "site/js/webgl-tmd.js"
PAGE_JS = "site/js/play-app.js"
STAGING_FN = "_stageFrameState"

# Sticky setters whose value is re-decided on every branch, with the reason.
BRANCH_OWNED = {
    "setOcclusionFocus": "cleared every frame ahead of the branches by "
    "`clearOcclusionFocus()`; only the field branch re-stages it",
    "setGroundEnable": "the battle branch turns the field ground pass off and "
    "the non-battle path turns it back on, every frame",
    "setFogOrigin": "not called by the play page",
    "setOceanColor": "not called by the play page",
}

METHOD_RE = re.compile(r"^  (set[A-Z]\w*)\s*\([^)]*\)\s*\{\s*$")
CALL_RE = re.compile(r"this\.renderer\.(set[A-Z]\w*)\s*\(")
FN_HEAD_RE = re.compile(r"^    (_?\w+)\s*\([^)]*\)\s*\{\s*$")


def sticky_setters(renderer_src: str) -> set[str]:
    """`set*` methods of the renderer whose body makes no `gl.` call."""
    out: set[str] = set()
    lines = renderer_src.splitlines()
    i = 0
    while i < len(lines):
        m = METHOD_RE.match(lines[i])
        if not m:
            i += 1
            continue
        name = m.group(1)
        body: list[str] = []
        i += 1
        while i < len(lines) and lines[i] != "  }":
            body.append(lines[i])
            i += 1
        text = "\n".join(body)
        if not re.search(r"\bgl\.", text) and "this.gl" not in text and "_upload" not in text:
            out.add(name)
        i += 1
    return out


def calls_by_function(page_src: str) -> list[tuple[int, str, str]]:
    """Every `this.renderer.set*(` call as `(line, setter, enclosing method)`.

    The enclosing method is the nearest preceding class-method head at the
    class body's indent (four spaces), which is how play-app.js is laid out.
    """
    out = []
    current = "<top>"
    for n, line in enumerate(page_src.splitlines(), 1):
        h = FN_HEAD_RE.match(line)
        if h and h.group(1) not in ("if", "for", "while", "switch", "catch"):
            current = h.group(1)
        for c in CALL_RE.finditer(line):
            out.append((n, c.group(1), current))
    return out


def audit(renderer_src: str, page_src: str) -> list[str]:
    sticky = sticky_setters(renderer_src)
    findings = []
    for line, setter, fn in calls_by_function(page_src):
        if setter not in sticky or setter in BRANCH_OWNED:
            continue
        if fn != STAGING_FN:
            findings.append(
                f"{PAGE_JS}:{line}: sticky renderer setter `{setter}` called in "
                f"`{fn}`, not in `{STAGING_FN}` - its value leaks into the other "
                f"draw branches. Stage it in `{STAGING_FN}`, or declare it in "
                f"BRANCH_OWNED with the reason it cannot leak."
            )
    return findings


SELFTEST_RENDERER = """\
class TmdRenderer {
  setSticky(v) {
    this.sticky = v;
  }
  setUploaded(v) {
    const gl = this.gl;
    gl.bindTexture(gl.TEXTURE_2D, v);
  }
  setOcclusionFocus(p) {
    this.occl = p;
  }
}
"""

SELFTEST_GOOD = """\
    _frame(rt) {
      this._stageFrameState(rt);
      this.renderer.setUploaded(1);
      this.renderer.setOcclusionFocus(null);
    }
    _stageFrameState(rt) {
      this.renderer.setSticky(1);
    }
"""

SELFTEST_BAD = """\
    _frame(rt) {
      if (battle) {
        return;
      }
      this.renderer.setSticky(1);
    }
"""


def selftest() -> bool:
    ok = True
    sticky = sticky_setters(SELFTEST_RENDERER)
    if sticky != {"setSticky", "setOcclusionFocus"}:
        print(f"selftest: sticky-setter classifier wrong: {sorted(sticky)}", file=sys.stderr)
        ok = False
    if audit(SELFTEST_RENDERER, SELFTEST_GOOD):
        print("selftest: a staged setter was reported", file=sys.stderr)
        ok = False
    bad = audit(SELFTEST_RENDERER, SELFTEST_BAD)
    if len(bad) != 1 or "`setSticky`" not in bad[0] or "`_frame`" not in bad[0]:
        print(f"selftest: a branch-staged setter was not reported: {bad}", file=sys.stderr)
        ok = False
    return ok


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()
    if not selftest():
        return 2
    if args.selftest:
        print("selftest: ok")
        return 0
    renderer_src = (REPO_ROOT / RENDERER_JS).read_text(encoding="utf-8")
    page_src = (REPO_ROOT / PAGE_JS).read_text(encoding="utf-8")
    sticky = sticky_setters(renderer_src)
    if not sticky:
        print(f"no sticky setters found in {RENDERER_JS} - classifier broken?", file=sys.stderr)
        return 2
    staged = {s for _, s, fn in calls_by_function(page_src) if fn == STAGING_FN}
    findings = audit(renderer_src, page_src)
    for f in findings:
        print(f)
    if not staged and not findings:
        print(f"{PAGE_JS}: `{STAGING_FN}` stages nothing - renamed?", file=sys.stderr)
        return 2
    if not args.quiet or findings:
        print(
            f"sticky frame state: {len(sticky)} sticky setter(s), "
            f"{len(staged)} staged in `{STAGING_FN}`, {len(findings)} finding(s)"
        )
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
