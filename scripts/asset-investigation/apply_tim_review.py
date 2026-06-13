#!/usr/bin/env python3
"""Apply a human-review selection back into the curated TIM label table.

Reads a `<category>_selection.txt` from `scripts/asset-investigation/build_tim_review.py` (a header
`# category=<cat>` plus one fingerprint per line) and rewrites the coarse
section of `crates/asset/src/data/tim_categories.tsv`:

  - fingerprint in the selection            -> label = <category>   (promote)
  - currently <category> but NOT selected   -> label = "other"      (demote, so
                                               a later category pass reclaims it)
  - everything else                         -> unchanged

The demote step only runs with --allow-demotions. By DEFAULT this script is
promote-only and never demotes, so a partial / hand-built selection can't
silently wipe a category's existing labels. The full-category review workflow
(build_tim_review.py pre-selects every current member of the category, so the
downloaded selection IS the whole category) is the case where demotion is
intended -- pass --allow-demotions there to let deselected cells fall back to
"other".

The byte-exact reverse-engineered pins (labels outside the coarse vocabulary)
are never touched. Regenerate the catalog reference TSVs afterwards.

Usage:
    python3 scripts/asset-investigation/apply_tim_review.py <selection.txt> [--table PATH] [--category CAT]
                                        [--allow-demotions]
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
    ap.add_argument(
        "--allow-demotions",
        action="store_true",
        help="demote existing category members NOT in the selection back to "
        "'other' (the full-category re-review workflow). Off by default so a "
        "partial selection is promote-only and can't wipe existing labels.",
    )
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
    would_demote = [fnv for fnv, rec in coarse.items() if rec[0] == cat and fnv not in sel]
    if args.allow_demotions:
        for fnv in would_demote:
            coarse[fnv][0] = "other"
            demoted += 1
    elif would_demote:
        print(
            f"note: {len(would_demote)} existing '{cat}' row(s) are not in the "
            f"selection; kept as-is (promote-only). Pass --allow-demotions to "
            f"demote them to 'other' (full-category re-review only).",
            file=sys.stderr,
        )

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
