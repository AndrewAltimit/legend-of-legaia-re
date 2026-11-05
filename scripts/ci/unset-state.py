#!/usr/bin/env python3
"""List engine state a live play-host path reads and no play-host path sets up.

`port-catalog.py --live-audit` asks whether a *port* is reached. A kernel whose
reader is live passes that audit even if the table it reads was never
installed: it runs correctly on empty inputs and the game silently misbehaves.
This walks the same call graph from the play hosts' roots only (`play-window`
and the wasm play exports; no parity, CLI or viewer) and prints three tiers:

  A  an `install_* / set_* / seed_* / load_* / with_* ...` no play host reaches,
     writing a field live code reads, with no other live writer of that field
  B  a field live code reads whose only writers are unreached functions
  C  a field live code reads that nothing writes beyond its zero value

Fields are matched by name and receivers are not typed, so a field name another
struct also carries counts every same-named write: the tool misses rows rather
than inventing them, except that a value written through a `&mut` local or a
reference handed to a function reads as no write. Prints only, asserts nothing;
docs/tooling/live-audit-triage.md carries how a row is settled. Run from the
repo root.
"""

import argparse
import re
import sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from port_catalog.rust_graph import build_rust_graph, reachable_fns  # noqa: E402
from port_catalog.rust_source import (  # noqa: E402
    CRATES_DIR,
    collect_roots,
    load_rust_sources,
    match_brace,
)

# The crates a running game is made of. Parity, the patcher family and the format
# readers set state for their own callers.
ENGINE_CRATE = re.compile(r"^(engine-|render-kernels$|web-play$|web-minigames$|netplay$)")
EXCLUDED_CRATES = {"engine-export", "engine-capture"}
PLAY_WASM_CRATES = {"web-play", "web-minigames"}
PLAY_NATIVE_ROOTS = {"cmd_play_window", "cmd_play_window_with_record"}

SETTER = re.compile(
    r"^(install|set|seed|load|with|register|attach|enable|configure|init|populate|stage)_\w+$"
)
ASSIGN_OP = r"(?:=(?![=>])|\+=|-=|\*=|/=|\|=|&=|\^=|<<=|>>=)"
CHAIN_WRITE = re.compile(
    r"((?:\.\s*[A-Za-z_]\w*(?:\[[^\]\n]*\])*)+)\s*" + ASSIGN_OP
)
BORROW_WRITE = re.compile(r"&\s*mut\s+(?:[A-Za-z_]\w*)?((?:\.\s*[A-Za-z_]\w*(?:\[[^\]\n]*\])*)+)")
CHAIN_METHOD = re.compile(r"((?:\.\s*[A-Za-z_]\w*(?:\[[^\]\n]*\])*)+?)\s*\.\s*([A-Za-z_]\w*)\s*\(")
FIELD_NAME = re.compile(r"\.\s*([A-Za-z_]\w*)")
FIELD_READ = re.compile(r"\.\s*([A-Za-z_]\w*)\b(?!\s*\()(?!\s*::)")
STRUCT_DEF = re.compile(r"\bstruct\s+([A-Z]\w*)\s*(?:<[^{;(]*>)?\s*(?:where[^{;]*)?\{")
ITEM_TABLE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const|static)\s+[A-Z_][A-Z0-9_]*\s*:", re.M)
CONST_KEY = re.compile(r"(?:^|[{,\s])([a-z_]\w*)\s*:(?!:)")
STD_MUTATORS = frozenset(
    "push push_back push_front insert extend clear retain resize fill copy_from_slice "
    "clone_from_slice sort sort_by sort_by_key sort_unstable remove pop pop_front pop_back "
    "truncate swap drain entry get_mut iter_mut as_mut get_or_insert get_or_insert_with "
    "take replace last_mut first_mut values_mut append dedup rotate_left rotate_right "
    "extend_from_slice as_deref_mut as_mut_slice".split()
)
TRIVIAL_VALUE = re.compile(
    r"^(?:0x0+|0+(?:\.0+)?(?:_?[iuf]\d+)?|false|None|\[\]|vec!\[\]|Vec::new\(\)|"
    r"[\w:]*(?:Default::default|default|new|empty)\(\)|\[.*;\s*\w+\]|String::new\(\)|"
    r"HashMap::new\(\)|BTreeMap::new\(\))$"
)


def struct_fields(srcs):
    """`{field: {struct}}` and `{struct: path}` over every parsed struct."""
    fields = defaultdict(set)
    home = {}
    for path, src in srcs.items():
        s = src.stripped
        for m in STRUCT_DEF.finditer(s):
            end = match_brace(s, m.end() - 1)
            body = s[m.end() : end - 1]
            home.setdefault(m.group(1), path)
            depth = 0
            start = 0
            parts = []
            for i, ch in enumerate(body):
                if ch in "<([{":
                    depth += 1
                elif ch in ">)]}":
                    depth -= 1
                elif ch == "," and depth == 0:
                    parts.append(body[start:i])
                    start = i + 1
            parts.append(body[start:])
            for part in parts:
                f = re.match(r"\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?([a-z_]\w*)\s*:", part)
                if f:
                    fields[f.group(1)].add(m.group(1))
    return fields, home


def literal_keys(body, impl_type, known_structs):
    """Keys a struct literal gives a value other than the type's zero."""
    out = set()
    for m in re.finditer(r"\b([A-Z]\w*)\s*\{", body):
        name = impl_type if m.group(1) == "Self" else m.group(1)
        if name not in known_structs:
            continue
        end = match_brace(body, m.end() - 1)
        inner = body[m.end() : end - 1]
        depth = 0
        start = 0
        parts = []
        for i, ch in enumerate(inner):
            if ch in "([{":
                depth += 1
            elif ch in ")]}":
                depth -= 1
            elif ch == "," and depth == 0:
                parts.append(inner[start:i])
                start = i + 1
        parts.append(inner[start:])
        for part in parts:
            part = part.strip()
            if not part or part.startswith(".."):
                continue
            kv = re.match(r"^([a-z_]\w*)\s*(?::(?!:)\s*(.*))?$", part, re.S)
            if not kv:
                continue
            value = (kv.group(2) or kv.group(1)).strip()
            if not TRIVIAL_VALUE.match(value):
                out.add(kv.group(1))
    return out


def mutating_methods(fns, srcs):
    out = set(STD_MUTATORS)
    for f in fns:
        if f.impl_type and "&mut self" in srcs[f.path].stripped[f.def_pos : f.body_start]:
            out.add(f.name)
    return out


def scan_fn(f, srcs, mutators, known_structs):
    body = srcs[f.path].stripped[f.body_start : f.body_end]
    writes = set()
    spans = []
    for rx in (CHAIN_WRITE, BORROW_WRITE):
        for m in rx.finditer(body):
            writes.update(FIELD_NAME.findall(m.group(1)))
            spans.append(m.span(1))
    for m in CHAIN_METHOD.finditer(body):
        if m.group(2) in mutators:
            writes.update(FIELD_NAME.findall(m.group(1)))
    writes |= literal_keys(body, f.impl_type, known_structs)
    masked = list(body)
    for a, b in spans:
        masked[a:b] = " " * (b - a)
    reads = set(FIELD_READ.findall("".join(masked)))
    return writes, reads


def play_roots(srcs):
    """`collect_roots`, narrowed to the two play hosts."""
    roots = []
    for r in collect_roots(srcs):
        if r.crate in PLAY_WASM_CRATES:
            roots.append(r)
        elif r.crate == "engine-shell" and r.name != "main":
            roots.append(r)
    for src in srcs.values():
        if src.crate == "engine-shell":
            roots += [f for f in src.fns if f.name in PLAY_NATIVE_ROOTS and not f.is_test]
    return roots


def in_scope(f):
    return (
        not f.is_test
        and bool(ENGINE_CRATE.match(f.crate))
        and f.crate not in EXCLUDED_CRATES
    )


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--tier", default="ABC", help="tiers to print (default ABC)")
    ap.add_argument(
        "--soft",
        action="store_true",
        help="judge writers on the receiver-gated graph too: more rows, some false",
    )
    ap.add_argument(
        "--loose-readers",
        action="store_true",
        help="judge readers on the permissive graph: more rows, many on parked kernels",
    )
    ap.add_argument("--shared", action="store_true", help="keep fields other structs also name")
    args = ap.parse_args()

    srcs = load_rust_sources()
    # Two graphs, for the reason `build_rust_graph` gives. A writer must be unreached
    # on the over-approximating one for a row to be firm; `--soft` judges it on the
    # receiver-gated graph, which finds more and also drops real `.tick()` edges.
    fns, edges = build_rust_graph(srcs, strict=False)
    reach = reachable_fns(fns, edges, play_roots(srcs))
    strict_fns, strict_edges = build_rust_graph(srcs, strict=True)
    strict_reach = reachable_fns(strict_fns, strict_edges, play_roots(srcs))
    fields, structs = struct_fields(srcs)
    mutators = mutating_methods(fns, srcs)

    writers = defaultdict(list)
    readers = defaultdict(list)
    per_fn_writes = {}
    for f in fns:
        if f.is_test:
            continue
        w, r = scan_fn(f, srcs, mutators, structs)
        per_fn_writes[f.uid] = w
        for name in w:
            writers[name].append(f)
        for name in r:
            readers[name].append(f)

    # A literal in a `const` / `static` table sits in no `fn`: it is data, and any
    # value it gives is a setup, zero included.
    const_set = set()
    for src in srcs.values():
        for m in ITEM_TABLE.finditer(src.stripped):
            start = m.end()
            end = src.stripped.find(";\n", start)
            if end > 0:
                const_set |= set(CONST_KEY.findall(src.stripped[start:end]))

    def fmt(f):
        owner = f"{f.impl_type}::" if f.impl_type else ""
        return f"{owner}{f.name}"

    # A reader is "live" on the gated graph unless asked: on the permissive one
    # a `.tick()` anywhere makes every parked kernel's input read live.
    read_gate = reach if args.loose_readers else strict_reach

    def live_readers(name):
        return [f for f in readers[name] if f.uid in read_gate]

    gate = strict_reach if args.soft else reach

    def live_writers(name):
        return [f for f in writers[name] if f.uid in gate]

    tier_a = []
    for f in fns:
        if not in_scope(f) or f.uid in gate or not SETTER.match(f.name):
            continue
        hits = [
            name
            for name in sorted(per_fn_writes.get(f.uid, ()))
            if name in fields and live_readers(name) and not live_writers(name)
        ]
        if hits:
            tier_a.append((f, hits))

    if "A" in args.tier:
        print("# A: setter no play host reaches, field read live, no live writer")
        for f, hits in sorted(tier_a, key=lambda r: str(r[0].path)):
            print(f"{fmt(f)}  {f.path}:{f.line}")
            for name in hits:
                sample = ", ".join(sorted({fmt(r) for r in live_readers(name)})[:3])
                print(f"    .{name}  read by {sample}")

    in_scope_struct = {
        s for s, p in structs.items() if ENGINE_CRATE.match(p.relative_to(CRATES_DIR).parts[0])
    }
    tier_b = []
    tier_c = []
    for name, owners in sorted(fields.items()):
        if not (owners & in_scope_struct):
            continue
        if len(owners) > 1 and not args.shared:
            continue
        rd = live_readers(name)
        if not rd or live_writers(name):
            continue
        if name in const_set:
            continue
        (tier_b if writers[name] else tier_c).append((name, sorted(owners), rd))
    if "B" in args.tier:
        print("\n# B: field read live; every writer is unreached from a play host")
        for name, owners, rd in tier_b:
            sample = ", ".join(sorted({fmt(r) for r in rd})[:3])
            wr = ", ".join(sorted({fmt(w) for w in writers[name]})[:3])
            print(f"{'/'.join(owners)}.{name}  read by {sample}  written by {wr}")
    if "C" in args.tier:
        print("\n# C: field read live; nothing writes it beyond its zero value")
        for name, owners, rd in tier_c:
            sample = ", ".join(sorted({fmt(r) for r in rd})[:3])
            print(f"{'/'.join(owners)}.{name}  read by {sample}")


if __name__ == "__main__":
    main()
