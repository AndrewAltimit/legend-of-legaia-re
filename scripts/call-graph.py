#!/usr/bin/env python3
"""Call-graph utility over `ghidra/scripts/funcs/*.txt`.

Reads every dump, extracts every `jal 0xXXXXXXXX` (and the C-decompile's
`FUN_XXXXXXXX` / `func_0xXXXXXXXX`) cite, and answers two questions:

  - `--callees <addr>`: what does this function call?
  - `--callers <addr>`: which other dumped functions call this address?

Plus an `--xref <addr>` mode that prints both sides plus the lines around each
hit so you can read context.

Limitations: only finds callers whose dump exists. To find callers of an SCUS
function from an overlay, the overlay program's functions must already have
been dumped (`overlay_<addr>.txt`).
"""

import argparse
import re
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
FUNCS_DIR = REPO / "ghidra" / "scripts" / "funcs"

CITE_RE = re.compile(
    r"(?:FUN_|func_0x|jal\s+0x|jalr\s+\w+,0x|->\s*func_0x)"
    r"(80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4})"
)

# Filename forms we recognise:
#   8001a55c.txt                  - SCUS function
#   overlay_801d6628.txt          - overlay function (un-tagged)
#   overlay_<label>_<addr>.txt    - overlay function (tagged with overlay)
FNAME_ADDR = re.compile(r"([0-9a-fA-F]{8})\.txt$")


def own_addr(path: Path) -> str | None:
    m = FNAME_ADDR.search(path.name)
    return m.group(1).lower() if m else None


def callees_for(path: Path) -> set[str]:
    text = path.read_text(errors="ignore")
    self_addr = own_addr(path)
    out = set()
    for m in CITE_RE.finditer(text):
        a = m.group(1).lower()
        if a == self_addr:
            continue
        out.add(a)
    return out


def all_dumps() -> list[Path]:
    return sorted(FUNCS_DIR.glob("*.txt"))


def callers_of(target: str) -> list[tuple[Path, list[str]]]:
    target = target.lower()
    out = []
    for p in all_dumps():
        if own_addr(p) == target:
            continue
        text = p.read_text(errors="ignore")
        # Find every line with the target address as a call cite.
        hits = []
        for line in text.splitlines():
            if re.search(
                r"(?:FUN_|func_0x|jal\s+0x|jalr\s+\w+,0x|->\s*func_0x)"
                + re.escape(target),
                line,
            ):
                hits.append(line.strip())
        if hits:
            out.append((p, hits))
    return out


def find_dump_for(addr: str) -> Path | None:
    addr = addr.lower()
    for p in FUNCS_DIR.glob(f"*{addr}.txt"):
        return p
    return None


def cmd_callees(args) -> int:
    p = find_dump_for(args.addr)
    if p is None:
        print(f"no dump for {args.addr}")
        return 1
    targets = sorted(callees_for(p))
    print(f"{p.name} → {len(targets)} unique callees:")
    for t in targets:
        own = find_dump_for(t)
        suffix = f" → {own.name}" if own else " [not yet dumped]"
        print(f"  {t}{suffix}")
    return 0


def cmd_callers(args) -> int:
    hits = callers_of(args.addr)
    print(f"callers of {args.addr}: {len(hits)} dumps")
    for path, lines in hits:
        print(f"  {path.name} ({len(lines)} call sites)")
        if args.context:
            for line in lines[:3]:
                print(f"      {line}")
    return 0


def cmd_xref(args) -> int:
    target = args.addr.lower()
    print(f"=== {target} ===")
    own = find_dump_for(target)
    if own:
        print(f"own dump: {own.name}")
        callees = sorted(callees_for(own))
        print(f"  calls {len(callees)} other functions")
    else:
        print("(no own dump)")
    print()
    print("called from:")
    hits = callers_of(target)
    for path, lines in hits:
        print(f"  {path.name}  ({len(lines)} sites)")
        for line in lines[:2]:
            print(f"      {line}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)

    p1 = sub.add_parser("callees", help="what does <addr> call?")
    p1.add_argument("addr")
    p1.set_defaults(fn=cmd_callees)

    p2 = sub.add_parser("callers", help="who calls <addr>?")
    p2.add_argument("addr")
    p2.add_argument("--context", action="store_true", help="show call-site lines")
    p2.set_defaults(fn=cmd_callers)

    p3 = sub.add_parser("xref", help="full xref report for <addr>")
    p3.add_argument("addr")
    p3.set_defaults(fn=cmd_xref)

    args = ap.parse_args()
    return args.fn(args)


if __name__ == "__main__":
    raise SystemExit(main())
