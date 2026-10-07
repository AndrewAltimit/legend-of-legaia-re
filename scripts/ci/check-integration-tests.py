#!/usr/bin/env python3
"""Every `tests/*.rs` of a crate with `autotests = false` is a module of its
`tests/integration.rs`.

A crate whose integration tests build as one binary (`autotests = false` plus
one `[[test]] name = "integration"`) only compiles the files that binary
declares. A new `tests/foo.rs` with no `mod foo;` line is never built and never
run - and nothing reports that, because a test that does not exist cannot
fail. This gate does.

Exits 1 listing each undeclared file (and each declaration with no file).
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent


def main() -> int:
    bad: list[str] = []
    checked = 0
    for manifest in sorted(REPO.glob("crates/*/Cargo.toml")):
        crate = manifest.parent
        if not re.search(r"^autotests\s*=\s*false", manifest.read_text(), re.M):
            continue
        root = crate / "tests" / "integration.rs"
        if not root.is_file():
            bad.append(f"{root.relative_to(REPO)}: missing (crate sets autotests = false)")
            continue
        declared = set(re.findall(r"^mod (\w+);", root.read_text(), re.M))
        files = {p.stem for p in (crate / "tests").glob("*.rs") if p.name != "integration.rs"}
        for name in sorted(files - declared):
            bad.append(f"{root.relative_to(REPO)}: tests/{name}.rs is not declared (add `mod {name};`)")
        for name in sorted(declared - files):
            bad.append(f"{root.relative_to(REPO)}: `mod {name};` has no tests/{name}.rs")
        checked += 1
    if bad:
        print("[integration-tests] FAIL", file=sys.stderr)
        for line in bad:
            print(f"  {line}", file=sys.stderr)
        return 1
    print(f"[integration-tests] OK - {checked} crate(s), every tests/*.rs declared")
    return 0


if __name__ == "__main__":
    sys.exit(main())
