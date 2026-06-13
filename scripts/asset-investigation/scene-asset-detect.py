#!/usr/bin/env python3
"""Scene-asset detector: surface PROT entries that hold geometry + texture
content but aren't yet classified by a single-magic detector.

Cross-references three signals per PROT entry:
  - categorize.json class
  - per-entry TMD count from `tmd_scan/<stem>/*.tmd`
  - per-entry TIM count from `tim_scan/<stem>/*.tim`

A "scene-asset" candidate is an entry where:
  - class is `unknown_other` (or one of the other unknown buckets), AND
  - TMD count >= 1 AND TIM count >= 1

Output (default text):
  - summary counts
  - top-N candidates by total sub-asset count
  - per-CDNAME-block grouping (which named clusters dominate)

The output drives the field-loader six-file-per-scene investigation
described in docs/subsystems/asset-loader.md.
"""

import argparse
import json
from collections import Counter
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
EXTRACTED = REPO / "extracted"


def cdname_block(path: str) -> str:
    """Extract CDNAME block name from `<index>_<block>.BIN`."""
    stem = Path(path).stem
    parts = stem.split("_", 1)
    return parts[1] if len(parts) > 1 else "(none)"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--top", type=int, default=30)
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--include", default="unknown_other,unknown_high_entropy,unknown_low_entropy",
                    help="comma-separated class names to consider")
    args = ap.parse_args()

    cat_path = EXTRACTED / "PROT" / "categorize.json"
    if not cat_path.exists():
        print(f"missing {cat_path}; run `legaia-asset categorize` first")
        return 1

    cat = json.load(cat_path.open())
    classes_of_interest = set(args.include.split(","))

    tim_dir = EXTRACTED / "tim_scan"
    tmd_dir = EXTRACTED / "tmd_scan"

    candidates = []
    for e in cat["per_file"]:
        if e["class"] not in classes_of_interest:
            continue
        stem = Path(e["path"]).stem
        n_tim = sum(1 for _ in (tim_dir / stem).glob("*.tim")) if (tim_dir / stem).exists() else 0
        n_tmd = sum(1 for _ in (tmd_dir / stem).glob("*.tmd")) if (tmd_dir / stem).exists() else 0
        if n_tim >= 1 and n_tmd >= 1:
            candidates.append({
                "path": e["path"],
                "class": e["class"],
                "size": e["size"],
                "tim": n_tim,
                "tmd": n_tmd,
                "total": n_tim + n_tmd,
                "block": cdname_block(e["path"]),
            })

    # Sort by total sub-asset count, then by size.
    candidates.sort(key=lambda c: (-c["total"], -c["size"]))

    if args.json:
        print(json.dumps(candidates, indent=2))
        return 0

    print(f"scene-asset candidates: {len(candidates)} entries")
    print(f"  with TMD AND TIM, in classes: {sorted(classes_of_interest)}")
    print()

    blocks = Counter(c["block"] for c in candidates)
    print(f"top CDNAME blocks (by candidate count):")
    for blk, n in blocks.most_common(15):
        print(f"  {blk:<30s} {n}")
    print()

    iterable = candidates if args.all else candidates[: args.top]
    print(f"top {len(iterable)} candidates by sub-asset count:")
    print(f"{'path':<35s} {'class':<22s} {'size':>8s} {'tmd':>4s} {'tim':>4s}")
    print(f"{'-'*35} {'-'*22} {'-'*8} {'-'*4} {'-'*4}")
    for c in iterable:
        print(f"{c['path']:<35s} {c['class']:<22s} {c['size']:>8} {c['tmd']:>4} {c['tim']:>4}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
