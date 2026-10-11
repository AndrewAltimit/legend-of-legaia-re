#!/usr/bin/env python3
"""The two figures that say how playable the port is, read off the baselines.

The port catalog and the segmented full-game ladder both sit at their
ceilings, so neither can say what to work on next. These two can still move:

- **Continuous run.** How many story milestones one New Game reaches on pad
  input when it is never re-seeded from a retail save (full-game ladder part
  C). The segmented ladder starts every stretch with a retail player's party
  and bag; this one has to earn them.
- **Image floor.** How many retail-compare states score under the floor on
  the `image` channel. The simulation channels match retail everywhere; what
  a player sees is the channel with room left.

Both are read from committed baselines, so this needs no disc and no build.

    play-depth.py              # the report
    play-depth.py --floor 0.95 # a stricter image floor
"""

from __future__ import annotations

import argparse
import json
import sys
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
LADDER = REPO / "scripts/replays/full_game_baseline.toml"
SPINE = REPO / "scripts/replays/full_game_spine.toml"
RETAIL = REPO / "scripts/ci/retail-compare-baseline.json"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--floor", type=float, default=0.9, help="image-channel floor (default 0.9)")
    ap.add_argument("--worst", type=int, default=8, help="how many of the worst states to list")
    args = ap.parse_args()

    ladder = tomllib.loads(LADDER.read_text())
    segments = len(tomllib.loads(SPINE.read_text()).get("milestone", [])) - 1
    continuous = ladder.get("continuous", {}).get("reached")
    segmented = ladder.get("headline", {}).get("contiguous_pad")
    print("continuous run (one New Game, pad input, own saves only)")
    if continuous is None:
        print("  not baselined - run full_game_ladder part C")
    else:
        print(f"  reaches {continuous} of {segments} milestones")
    print(f"  segmented ladder, for scale: {segmented} of {segments} at the pad tier")

    states = json.loads(RETAIL.read_text())["states"]
    image = {name: ch["image"] for name, ch in states.items() if "image" in ch}
    below = sorted((v, k) for k, v in image.items() if v < args.floor)
    mean = sum(image.values()) / len(image) if image else 0.0
    print()
    print(f"image channel ({len(image)} states scored, mean {mean:.3f})")
    print(f"  {len(below)} under {args.floor}")
    for value, name in below[: args.worst]:
        print(f"    {value:.3f}  {name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
