#!/usr/bin/env python3
# ASCII-only. Runs on the host, not inside the Ghidra container.
#
# Call-target integrity check over the Ghidra dump corpus
# (ghidra/scripts/funcs/*.txt).
#
# Premise: a MIPS `jal` encodes its target absolutely --
#   target = (PC + 4)[31:28] || imm26 || 00
# Every Legaia load base (SCUS at 0x800xxxxx, overlays at 0x801Cxxxx+) shares
# the top nibble 0x8, so re-basing a program shifts the *call site* address and
# can never change the *decoded target*. A decoded target is therefore a
# property of the bytes alone.
#
# That makes the resolve rate a base-independent health signal. Code that is
# genuinely resident and genuinely addresses the retail SCUS layout lands on a
# recognized function entry essentially every time. A window of bytes whose
# link base is unrecovered -- or which was never retail-resident code at these
# addresses at all -- keeps decoding to syntactically valid `jal`s, but those
# targets scatter into mid-basic-block addresses, branch instructions and even
# delay slots.
#
# So: a per-dump collapse in resolve rate localizes a mis-attributed byte
# window. It does not, on its own, say what the window is.
#
# Usage:
#   scripts/ghidra-analysis/check-jal-target-integrity.py [--funcs DIR]
#                                                         [--threshold PCT]

import argparse
import collections
import glob
import os
import re
import sys

HEADER_RE = re.compile(r"^==\s+\S*\s*([0-9a-fA-F]{8})\s+\(entry=([0-9a-fA-F]{8})\)")
INSN_RE = re.compile(r"^([0-9a-f]{8})  (_?)(\S+)(.*)$")
JAL_RE = re.compile(r"\bjal 0x([0-9a-f]{8})\b")

# The always-resident executable's address window. Targets outside it belong to
# a swappable overlay slot and cannot be checked against a single image.
SCUS_LO = 0x80010000
SCUS_HI = 0x80090000

# Entry idioms. A Legaia function entry is usually a stack-frame adjust, but
# leaf stubs legitimately start with a bare immediate load or a gp-relative
# load, so treating only `addiu sp,sp,-N` as an entry under-counts.
ENTRY_PREFIXES = ("addiu sp,sp,-",)


def load_corpus(funcs_dir):
    """Return (entries, insns, delay_slots) harvested from every dump."""
    entries = set()
    insns = {}
    delay = set()
    for path in sorted(glob.glob(os.path.join(funcs_dir, "*.txt"))):
        try:
            with open(path) as fh:
                lines = fh.readlines()
        except IOError:
            continue
        if lines:
            m = HEADER_RE.match(lines[0])
            if m:
                entries.add(int(m.group(2), 16))
        for line in lines:
            m = INSN_RE.match(line)
            if not m:
                continue
            addr = int(m.group(1), 16)
            if not (SCUS_LO <= addr < SCUS_HI):
                continue
            if m.group(2) == "_":
                delay.add(addr)
            insns.setdefault(addr, m.group(3) + m.group(4))
    return entries, insns, delay


def classify(target, entries, insns, delay):
    """Bucket a decoded jal target."""
    if target in entries:
        return "entry"
    text = insns.get(target)
    if text is None:
        return "uncovered"
    if any(text.startswith(p) for p in ENTRY_PREFIXES):
        return "entry"
    if target in delay:
        return "delay-slot"
    return "interior"


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    here = os.path.dirname(os.path.abspath(__file__))
    ap.add_argument(
        "--funcs",
        default=os.path.join(here, "..", "..", "ghidra", "scripts", "funcs"),
        help="directory of Ghidra function dumps",
    )
    ap.add_argument(
        "--threshold",
        type=float,
        default=90.0,
        help="flag any dump whose resolve rate falls below this percentage",
    )
    ap.add_argument(
        "--min-sites",
        type=int,
        default=5,
        help="ignore dumps with fewer than this many checkable call sites",
    )
    args = ap.parse_args()

    funcs_dir = os.path.normpath(args.funcs)
    if not os.path.isdir(funcs_dir):
        sys.stderr.write("no such dump directory: %s\n" % funcs_dir)
        return 2

    entries, insns, delay = load_corpus(funcs_dir)
    if not insns:
        sys.stderr.write("no disassembly found under %s\n" % funcs_dir)
        return 2

    flagged = []
    bad_targets = collections.Counter()
    total = good = 0

    for path in sorted(glob.glob(os.path.join(funcs_dir, "*.txt"))):
        with open(path) as fh:
            text = fh.read()
        sites = resolved = 0
        misses = collections.Counter()
        for m in JAL_RE.finditer(text):
            target = int(m.group(1), 16)
            if not (SCUS_LO <= target < SCUS_HI):
                continue
            sites += 1
            kind = classify(target, entries, insns, delay)
            if kind in ("entry", "uncovered"):
                resolved += 1
            else:
                misses[(target, kind)] += 1
        if not sites:
            continue
        total += sites
        good += resolved
        for key, n in misses.items():
            bad_targets[key] += n
        rate = 100.0 * resolved / sites
        if sites >= args.min_sites and rate < args.threshold:
            flagged.append((rate, os.path.basename(path), sites, resolved, misses))

    print("corpus: %d dumps' SCUS-range call sites checked" % total)
    print("resolve rate overall: %.1f%% (%d/%d)" % (100.0 * good / max(total, 1), good, total))
    print("")

    if not flagged:
        print("no dump falls below the %.0f%% threshold" % args.threshold)
        return 0

    print("FLAGGED -- call targets in these dumps do not address the retail layout:")
    for rate, name, sites, resolved, misses in sorted(flagged):
        print("  %-52s %5.1f%%  (%d/%d)" % (name, rate, resolved, sites))
        for (target, kind) in sorted(misses):
            print("        -> 0x%08x  %-11s %s" % (target, kind, insns.get(target, "?")))
    print("")
    print("distinct unresolved targets, by call count:")
    for (target, kind), n in bad_targets.most_common():
        print("  0x%08x  %-11s x%-4d %s" % (target, kind, n, insns.get(target, "?")))
    return 1


if __name__ == "__main__":
    sys.exit(main())
