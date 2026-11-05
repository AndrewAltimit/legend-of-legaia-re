#!/usr/bin/env python3
"""List host-trait methods whose default body does nothing and that no real impl overrides.

A VM arm that calls such a method is `live` to port-catalog.py and does nothing
in a running game: the audit sees the call, not that the callee is empty. Each
row needs a verdict; docs/tooling/live-audit-triage.md carries the settled ones.
Prints only, asserts nothing. Run from the repo root.
"""

import collections
import os
import re

TEST_IMPL = re.compile(r"Null|Mock|Test|Noop|Fake|Dummy|Recording|NoHost|^H$|^Rec")
TRAIT = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?trait\s+(\w+)")
FN = re.compile(r"^\s*fn\s+(\w+)\s*(?:<[^>]*>)?\s*\(")
IMPL = re.compile(r"impl(?:<[^>]*>)?\s+(?:[\w:]+::)?(\w+)(?:<[^>]*>)?\s+for\s+([^\{]+)\{")


def code(line):
    return re.sub(r"//.*", "", line)


def sources():
    out = {}
    for root, _, files in os.walk("crates"):
        if "/tests" in root:
            continue
        for name in files:
            if name.endswith(".rs") and name != "tests.rs":
                path = os.path.join(root, name)
                out[path] = open(path, errors="replace").read()
    return out


def trivial_defaults(src):
    """{trait: [(method, body, path)]} for each default body with no call on self."""
    found = collections.defaultdict(list)
    for path, text in src.items():
        lines = text.split("\n")
        i = 0
        while i < len(lines):
            m = TRAIT.match(lines[i])
            if not m or "{" not in "".join(lines[i : i + 6]):
                i += 1
                continue
            depth, j, started = 0, i, False
            while j < len(lines):
                c = code(lines[j])
                depth += c.count("{") - c.count("}")
                started = started or "{" in c
                if started and depth <= 0:
                    break
                j += 1
            body = lines[i : j + 1]
            k = 0
            while k < len(body):
                f = FN.match(body[k])
                if f:
                    d, t, s, txt = 0, k, False, ""
                    while t < len(body):
                        c = code(body[t])
                        txt += c + "\n"
                        if ";" in c and not s and "{" not in c:
                            break
                        d += c.count("{") - c.count("}")
                        s = s or "{" in c
                        if s and d <= 0:
                            break
                        t += 1
                    if s:
                        inner = txt[txt.index("{") + 1 : txt.rindex("}")].strip()
                        if len(inner) < 40 and not re.search(r"self\.\w+\(", inner):
                            found[m.group(1)].append((f.group(1), " ".join(inner.split()), path))
                    k = t
                k += 1
            i = j + 1
    return found


def impl_bodies(src):
    out = collections.defaultdict(list)
    for text in src.values():
        for m in IMPL.finditer(text):
            d, e = 1, m.end()
            while e < len(text) and d:
                d += (text[e] == "{") - (text[e] == "}")
                e += 1
            out[m.group(1)].append((m.group(2).strip(), text[m.end() : e]))
    return out


def main():
    src = sources()
    impls = impl_bodies(src)
    for trait, methods in sorted(trivial_defaults(src).items()):
        real = [i for i in impls.get(trait, []) if not TEST_IMPL.search(i[0])]
        if not real:
            continue
        for name, body, path in methods:
            if not any(re.search(rf"\bfn\s+{name}\b", b) for _, b in real):
                print(f"{trait}::{name}  `{body}`  {path}")


if __name__ == "__main__":
    main()
