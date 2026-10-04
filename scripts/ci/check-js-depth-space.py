#!/usr/bin/env python3
"""Detect a fragment shader that depth-tests in a different space from the scene.

The play page's mesh program writes ``log2(w) / LOG_DEPTH_RANGE`` to
``gl_FragDepth`` on its perspective frames (``LOG_DEPTH_GLSL`` in
``site/js/webgl-shaders.js``) - WebGL2 cannot select the native renderer's
float reversed-Z buffer, so the page keeps its depth precision that way. Any
other program drawn into the same depth buffer with the depth test on has to
write the same space: a fragment that keeps the rasterised ``gl_FragCoord.z``
compares ~0.99 at field distances against a scene written at ~0.4, and LEQUAL
rejects nearly all of it.

That is not hypothetical: the enhanced-lighting glow program (lamp halos and
light shafts) wrote ``gl_FragCoord.z``, so every halo the native window
blooms around a lamp all but vanished on the page, with no error anywhere. No
Rust tier sees GLSL, and the shader still compiled, linked and drew - into a
test it failed.

The audit reads every GLSL ES 3.00 fragment source (a ``#version 300 es``
template literal with no ``gl_Position``) under ``site/js/`` and requires it
to write ``gl_FragDepth``, unless it is waived in ``WAIVED`` with the reason
its depth cannot meet the log-depth buffer. A new fragment shader fails until
that decision is made on purpose.

Usage:
    python3 scripts/ci/check-js-depth-space.py            # audit
    python3 scripts/ci/check-js-depth-space.py --selftest # controls only

Exit status: 0 = clean, 1 = findings, 2 = self-test failed / nothing found.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
JS_DIR = "site/js"

# (file name, shader constant) -> why its depth never meets the log buffer.
WAIVED = {
    ("webgl-shaders.js", "OCEAN_FS_SRC"): "the sea backdrop plane draws first in "
    "`renderAssembled`, ahead of every log-depth write of the frame, and the "
    "page uses it only on the overworld, where every later surface is meant to "
    "win against it",
    ("webgl-prim-replay.js", "FS_REPLAY"): "no page loads webgl-prim-replay.js; "
    "it draws into its own context",
}

SHADER_RE = re.compile(r"const\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*`#version 300 es(.*?)`", re.S)


def fragment_shaders(src: str) -> list[tuple[str, int, str]]:
    """(constant name, 1-based line, body) for every fragment source."""
    out = []
    for m in SHADER_RE.finditer(src):
        body = m.group(2)
        if "gl_Position" in body:
            continue
        out.append((m.group(1), src.count("\n", 0, m.start()) + 1, body))
    return out


def audit(files: dict[str, str]) -> tuple[int, list[str]]:
    findings = []
    n = 0
    for name, src in sorted(files.items()):
        for const, line, body in fragment_shaders(src):
            n += 1
            if (name, const) in WAIVED:
                continue
            if "gl_FragDepth" not in body:
                findings.append(
                    f"{JS_DIR}/{name}:{line}: fragment shader `{const}` keeps the "
                    "rasterised depth; the play page's scene writes log2(w) "
                    "(LOG_DEPTH_GLSL) - write gl_FragDepth through logDepthOfW "
                    "under the same flag, or waive it in WAIVED with the reason"
                )
    return n, findings


SELFTEST_GOOD = """
const VS = `#version 300 es
void main() { gl_Position = vec4(0.0); }
`;
const GOOD_FS = `#version 300 es
out vec4 o;
void main() { gl_FragDepth = gl_FragCoord.z; o = vec4(1.0); }
`;
"""

# The glow program as it shipped broken: depth-tested, no depth write.
SELFTEST_BAD = """
const GLOW_FS_SRC = `#version 300 es
in vec2 v_uv;
out vec4 o_color;
void main() { o_color = vec4(v_uv, 0.0, 1.0); }
`;
"""


def selftest() -> bool:
    ok = True
    n, f = audit({"good.js": SELFTEST_GOOD})
    if n != 1 or f:
        print(f"selftest: clean control misread ({n} shaders, {f})", file=sys.stderr)
        ok = False
    n, f = audit({"bad.js": SELFTEST_BAD})
    if n != 1 or len(f) != 1 or "GLOW_FS_SRC" not in f[0]:
        print(f"selftest: the shipped glow defect was not reported: {f}", file=sys.stderr)
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
    files = {
        p.name: p.read_text(encoding="utf-8") for p in sorted((REPO_ROOT / JS_DIR).glob("*.js"))
    }
    n, findings = audit(files)
    if n == 0:
        print(f"no fragment shaders found under {JS_DIR} - classifier broken?", file=sys.stderr)
        return 2
    seen = {(name, c) for name, src in files.items() for c, _, _ in fragment_shaders(src)}
    for stale in sorted(set(WAIVED) - seen):
        findings.append(f"WAIVED entry {stale} names no fragment shader - delete it")
    for f in findings:
        print(f)
    if not args.quiet or findings:
        print(f"depth space: {n} fragment shader(s), {len(WAIVED)} waived, {len(findings)} finding(s)")
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
