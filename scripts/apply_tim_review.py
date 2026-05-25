#!/usr/bin/env python3
"""Apply a human-review selection back into the curated TIM label table.

Reads a `<category>_selection.txt` from `scripts/build_tim_review.py` (a header
`# category=<cat>` plus one fingerprint per line) and rewrites the coarse
section of `crates/asset/src/data/tim_categories.tsv`:

  - fingerprint in the selection            -> label = <category>   (promote)
  - currently <category> but NOT selected   -> label = "other"      (demote, so
                                               a later category pass reclaims it)
  - everything else                         -> unchanged

The byte-exact reverse-engineered pins (labels outside the coarse vocabulary)
are never touched. Regenerate the catalog reference TSVs afterwards.

Usage:
    python3 scripts/apply_tim_review.py <selection.txt> [--table PATH] [--category CAT]
"""
import argparse
import sys

VOCAB = {"environment", "terrain", "foliage", "character", "ui-text", "effect", "other"}
COARSE_MARKER = "# --- Coarse"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("selection")
    ap.add_argument("--table", default="crates/asset/src/data/tim_categories.tsv")
    ap.add_argument("--category", default=None, help="override category (else read from file header)")
    args = ap.parse_args()

    # Parse selection.
    cat = args.category
    sel = set()
    with open(args.selection) as f:
        for line in f:
            s = line.strip()
            if not s:
                continue
            if s.startswith("#"):
                if "category=" in s and cat is None:
                    cat = s.split("category=", 1)[1].strip()
                continue
            sel.add(s.lower())
    if cat not in VOCAB:
        print(f"bad/unknown category: {cat!r}", file=sys.stderr)
        return 1

    # Split the table into prefix (comments + header + pin block + coarse
    # marker) and the coarse rows that follow it.
    lines = open(args.table).read().splitlines()
    try:
        cut = next(i for i, l in enumerate(lines) if l.startswith(COARSE_MARKER))
    except StopIteration:
        print(f"no '{COARSE_MARKER}' marker in {args.table}", file=sys.stderr)
        return 1
    prefix = lines[: cut + 1]

    # Pin fingerprints (data rows in the prefix) are protected.
    pins = set()
    for l in prefix:
        if l.startswith("#") or l.startswith("fnv1a") or not l.strip():
            continue
        pins.add(l.split("\t")[0].strip().lower())

    # Existing coarse rows: fnv -> (label, note). Notes are preserved so manual
    # annotations (e.g. a specific texture's description) survive later passes.
    coarse = {}
    for l in lines[cut + 1 :]:
        if not l.strip() or l.startswith("#"):
            continue
        c = l.split("\t")
        note = c[2].strip() if len(c) > 2 else ""
        coarse[c[0].strip().lower()] = [c[1].strip(), note]

    promoted = demoted = protected = 0
    for fnv in sel:
        if fnv in pins:
            protected += 1
            continue
        cur = coarse.get(fnv)
        if cur is None:
            coarse[fnv] = [cat, ""]
            promoted += 1
        else:
            if cur[0] != cat:
                promoted += 1
            cur[0] = cat
    for fnv, rec in coarse.items():
        if rec[0] == cat and fnv not in sel:
            rec[0] = "other"
            demoted += 1

    out = list(prefix)
    for fnv in sorted(coarse):
        lbl, note = coarse[fnv]
        out.append(f"{fnv}\t{lbl}\t{note}")
    with open(args.table, "w") as f:
        f.write("\n".join(out) + "\n")

    total = sum(1 for v in coarse.values() if v[0] == cat)
    print(
        f"category '{cat}': promoted {promoted}, demoted {demoted} -> other, "
        f"protected {protected} pins. now {total} '{cat}' rows."
    )
    print(f"wrote {args.table} -- regenerate the catalog reference TSVs next.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
