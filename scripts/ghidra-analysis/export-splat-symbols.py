#!/usr/bin/env python3
"""Export the project's SCUS_942.54 function names as a splat `symbol_addrs.txt`.

For a matching-decompilation project that splits the executable with splat: it
names what this repository has already named, so the first split already reads.

    python3 scripts/ghidra-analysis/export-splat-symbols.py -o symbol_addrs.txt
    python3 scripts/ghidra-analysis/export-splat-symbols.py --lib-guesses --dumps ghidra/scripts/funcs

Sources, in priority order (the first to name an address wins; a disagreement
is reported on stderr, never silently resolved):

| source | what it gives |
|---|---|
| `ghidra/scripts/lib/known_symbols.py` | the hand-pinned SCUS names the Ghidra label applier uses |
| `docs/reference/functions/*.md` | rows whose Role cell opens with a backticked identifier (`` `InitHeap` - ...``) |
| `scripts/ci/port-catalog-ignore.toml` (`--lib-guesses`) | PsyQ / BIOS names read off the library sections' reason strings |

The last source is prose, not a symbol table, so its names are guesses: only a
CamelCase or underscored identifier that opens the reason and names one address
is kept, and the output marks each such line. Review them before trusting one.

`--dumps DIR` adds `size:` from the local decompiled-dump headers (gitignored,
never redistributed; only the header's byte count is read). Without it no size
is emitted and splat finds each function's end itself.

Scope is the executable's code band only. Overlays are deliberately not
exported: every slot-A overlay links at the same base, so an overlay address
names different code per image and a by-address symbol file would mislabel all
but one of them (`docs/reference/overlay-va-aliases.md`). An overlay's symbols
need a file of their own per PROT entry.

Names are this project's, not Sony's: library names are the PsyQ API names the
routines were identified as, everything else is our own descriptive naming.
"""

from __future__ import annotations

import argparse
import collections
import glob
import os
import re
import sys

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    import tomli as tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)
import dump_header  # noqa: E402  (sibling module)

# The executable's code band: text starts at the image base, and the last
# routine (the kernel-patch cluster) ends below the initialised data at
# 0x8006F180 (docs/reference/functions/runtime-libs.md).
CODE_LO = 0x80010000
CODE_HI = 0x8006F180

KNOWN_SYMBOLS = os.path.join(REPO, "ghidra", "scripts", "lib", "known_symbols.py")
FUNCTION_DOCS = os.path.join(REPO, "docs", "reference", "functions", "*.md")
IGNORE_TOML = os.path.join(REPO, "scripts", "ci", "port-catalog-ignore.toml")

# The ignore list's library sections - the ones whose reasons name a PsyQ / BIOS
# routine. Every other section's reasons describe game code or address claims.
LIB_SECTIONS = (
    "bios", "libc", "libgte", "libcd", "libgs", "libetc", "libapi", "libsnd",
    "libspu", "libgpu", "joy_serial", "kernel_callback",
)

IDENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
# C keywords and type names a Role cell can open with that are not names.
NOT_NAMES = {"short", "int", "char", "long", "void", "unsigned", "signed", "struct"}

_KNOWN_ROW = re.compile(r"\(\s*0x([0-9A-Fa-f]{8})\s*,\s*\"([^\"]+)\"")
_DOC_ROW = re.compile(
    r"^\|\s*`([0-9A-Fa-f]{8})`\s*\|\s*`([A-Za-z_][A-Za-z0-9_]*)(?:\(\))?`")
_REASON_LEAD = re.compile(r"^`?([A-Za-z_][A-Za-z0-9_]{2,})`?(?:\(\))?(?=[\s(,:;`]|$)")


def in_code(va):
    return CODE_LO <= va < CODE_HI


def looks_like_api_name(name):
    """A PsyQ-style identifier rather than an English word opening a sentence."""
    if "_" in name.strip("_"):
        return True
    humps = sum(1 for a, b in zip(name, name[1:]) if a.islower() and b.isupper())
    return humps >= 1 and not name.isupper()


def from_known_symbols():
    with open(KNOWN_SYMBOLS) as fh:
        text = fh.read()
    for va, name in _KNOWN_ROW.findall(text):
        yield int(va, 16), name, "known_symbols.py"


def from_function_docs():
    for path in sorted(glob.glob(FUNCTION_DOCS)):
        doc = "functions/" + os.path.basename(path)
        with open(path) as fh:
            for line in fh:
                m = _DOC_ROW.match(line)
                if m and m.group(2) not in NOT_NAMES:
                    yield int(m.group(1), 16), m.group(2), doc


def from_ignore_reasons():
    with open(IGNORE_TOML, "rb") as fh:
        table = tomllib.load(fh)
    rows = []
    for section in LIB_SECTIONS:
        for addr, reason in table.get(section, {}).items():
            m = _REASON_LEAD.match(reason.strip())
            if m and looks_like_api_name(m.group(1)):
                rows.append((int(addr, 16), m.group(1), f"ignore:{section} (guess)"))
    # A name the prose opens several rows with ("SsSeqPlay internals: ...") is
    # the subject of a cluster, not the name of any one of its addresses.
    per_name = collections.Counter(name for _, name, _ in rows)
    for va, name, src in rows:
        if per_name[name] == 1:
            yield va, name, src


def dump_sizes(funcs_dir):
    sizes = {}
    # Only the SCUS-imported dumps (`<addr>.txt`); `overlay_*` files are keyed
    # on an overlay base and would alias into the executable's band.
    for path in glob.glob(os.path.join(funcs_dir, "[0-9a-f]" * 8 + ".txt")):
        dump, _ = dump_header.parse_file(path)
        if dump and in_code(dump.entry) and dump.nbytes >= 8:
            sizes[dump.entry] = dump.nbytes
    return sizes


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("-o", "--out", help="output path (default: stdout)")
    ap.add_argument("--lib-guesses", action="store_true",
                    help="also read PsyQ / BIOS names off the ignore list's reason strings")
    ap.add_argument("--dumps", metavar="DIR",
                    help="local dump corpus to read function sizes from")
    args = ap.parse_args()

    sources = [from_known_symbols(), from_function_docs()]
    if args.lib_guesses:
        sources.append(from_ignore_reasons())

    chosen = {}  # va -> (name, source)
    conflicts = 0
    for rows in sources:
        for va, name, src in rows:
            if not in_code(va) or not IDENT.match(name):
                continue
            if va in chosen:
                if chosen[va][0] != name:
                    conflicts += 1
                    print(f"conflict 0x{va:08X}: {chosen[va][0]} ({chosen[va][1]}) "
                          f"kept over {name} ({src})", file=sys.stderr)
                continue
            chosen[va] = (name, src)

    # splat needs unique names. Duplicate copies of one library routine are
    # common (the linker pulled several), so later copies take the address.
    seen = collections.Counter()
    for va in sorted(chosen):
        name, src = chosen[va]
        seen[name] += 1
        if seen[name] > 1:
            chosen[va] = (f"{name}_{va:08X}", src)

    sizes = dump_sizes(args.dumps) if args.dumps else {}

    out = open(args.out, "w") if args.out else sys.stdout
    out.write("// splat symbol_addrs for SCUS_942.54 (USA), generated by\n"
              "// scripts/ghidra-analysis/export-splat-symbols.py in legend-of-legaia-re.\n"
              "// Names are the project's own; `(guess)` sources need review.\n")
    by_source = collections.defaultdict(list)
    for va in sorted(chosen):
        by_source[chosen[va][1]].append(va)
    for src in sorted(by_source, key=lambda s: min(by_source[s])):
        out.write(f"\n// from {src}\n")
        for va in by_source[src]:
            size = f" size:0x{sizes[va]:X}" if va in sizes else ""
            out.write(f"{chosen[va][0]} = 0x{va:08X}; // type:func{size}\n")
    if out is not sys.stdout:
        out.close()
    print(f"{len(chosen)} symbol(s), {len(sizes)} size(s) available, "
          f"{conflicts} conflict(s)", file=sys.stderr)


if __name__ == "__main__":
    main()
