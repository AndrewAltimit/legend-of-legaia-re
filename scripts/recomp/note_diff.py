#!/usr/bin/env python3
"""Diff two note-level BGM traces and report the first divergence per channel.

Sibling of `trace_diff.py`, same report shape, different subject: where that
one aligns per-frame state channels, this one aligns *note sequences* - the
stream of key-ons a sequencer asked the SPU for. It answers the question
"which note went missing, and was it never requested or just inaudible".

Inputs are the canonical note JSONL emitted by
`scripts/recomp/audio_note_capture.py` (retail, via the recomp's SPU rings)
and `note-trace` from `crates/engine-audio` (the engine's own sequencer).

Two normalisations make the sides comparable:

**Addresses.** Each side's allocator lays the VAB's VAGs out in SPU RAM
itself, so raw `addr` values never match. Both allocate in bank upload order,
ascending, so the addresses are mapped to dense **VAG ids** by ascending
order within each trace. A note's VAG id is therefore its tone identity, and
a mismatch means tone selection diverged - the handle back to the disc.

**Time.** The recomp's frame stamps and the engine's derived frames start
from unrelated origins, and a capture generally begins mid-track. Alignment
is therefore on note *ordinal*, not wall time: the comparison is over the
sequence of note-ons. `--offset N` skips the first N note-ons of side B;
without it, the best offset is found by maximising agreement on the (vag,
pitch) key over a search window.

Comparison is per channel over the aligned overlap - `vag`, `pitch`, `voice`,
`vol` - and for each divergent channel the FIRST divergent note is shown with
a context window of both sides. Exit status is non-zero when anything
diverged.

A caveat the tool cannot check for you: a capture taken from a recomp
instance whose SPU is not being clocked is an artifact, not ground truth.
`audio_note_capture.py` refuses to produce one; if you bypassed that with
`--allow-unclocked`, this diff is meaningless.

Usage::

    python3 scripts/recomp/note_diff.py recomp_notes.jsonl engine_notes.jsonl
    python3 scripts/recomp/note_diff.py a.jsonl b.jsonl --offset 4 --tol-pitch 8
"""

from __future__ import annotations

import argparse
import json
# Channels compared on each aligned note-on pair.
CHANNELS = ("vag", "pitch", "v", "vol")


def load(path: str) -> tuple[dict, list[dict]]:
    """Read a note JSONL into (header, note-on list) with VAG ids assigned."""
    header: dict = {}
    events: list[dict] = []
    with open(path) as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            obj = json.loads(line)
            if obj.get("kind") == "header":
                header = obj
                continue
            events.append(obj)

    ons = [e for e in events if e.get("ev") == "on"]
    # Dense VAG ids by ascending SPU address - both sides upload the bank in
    # ascending order, so this is allocator-independent tone identity.
    #
    # The renumbering is per-trace, which has a sharp edge: if a tone is
    # played on one side and never on the other, every id above it shifts, and
    # two genuinely different tones can then share an id. So `vag` agreement
    # is only meaningful when both sides used the same NUMBER of distinct
    # VAGs - main() warns when they don't, and the `pitch` channel is the
    # reliable signal in that case.
    addrs = sorted({e["addr"] for e in ons})
    vag_of = {a: i for i, a in enumerate(addrs)}
    for e in ons:
        e["vag"] = vag_of[e["addr"]]
        e["n_vags"] = len(addrs)
        # Single scalar for loudness; the sides split pan slightly differently
        # and the interesting failure is a note at zero volume, not +/-1.
        e["vol"] = abs(e.get("voll", 0)) + abs(e.get("volr", 0))
    return header, ons


def key(note: dict) -> tuple:
    return (note.get("vag"), note.get("pitch"))


def best_offset(a: list[dict], b: list[dict], window: int) -> int:
    """Offset into `b` maximising (vag, pitch) agreement with `a`."""
    best, best_score = 0, -1
    limit = min(window, len(b))
    for off in range(limit + 1):
        n = min(len(a), len(b) - off)
        if n <= 0:
            break
        score = sum(1 for i in range(n) if key(a[i]) == key(b[i + off]))
        # Normalise so a tiny overlap cannot win on a lucky short run.
        score = score * 1000 // max(1, n)
        if score > best_score:
            best, best_score = off, score
    return best


def compare(a: list[dict], b: list[dict], tol_pitch: int, tol_vol: int) -> dict:
    """First divergence per channel over the aligned overlap."""
    n = min(len(a), len(b))
    first: dict[str, int] = {}
    counts: dict[str, int] = {c: 0 for c in CHANNELS}
    for i in range(n):
        for ch in CHANNELS:
            av, bv = a[i].get(ch), b[i].get(ch)
            if av is None or bv is None:
                continue
            if ch == "pitch":
                bad = abs(av - bv) > tol_pitch
            elif ch == "vol":
                bad = abs(av - bv) > tol_vol
            else:
                bad = av != bv
            if bad:
                counts[ch] += 1
                first.setdefault(ch, i)
    return {"overlap": n, "first": first, "counts": counts}


def context(a: list[dict], b: list[dict], idx: int, ch: str, span: int = 3) -> str:
    lines = []
    lo, hi = max(0, idx - span), min(min(len(a), len(b)), idx + span + 1)
    for i in range(lo, hi):
        mark = ">>" if i == idx else "  "
        lines.append(
            f"    {mark} note {i:5d}  A {ch}={a[i].get(ch)!s:<8} "
            f"(frame {a[i].get('frame')}, v{a[i].get('v')})   "
            f"B {ch}={b[i].get(ch)!s:<8} "
            f"(frame {b[i].get('frame')}, v{b[i].get('v')})"
        )
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("trace_a", help="side A (conventionally the recomp/retail capture)")
    ap.add_argument("trace_b", help="side B (conventionally the engine trace)")
    ap.add_argument(
        "--offset",
        type=int,
        default=None,
        help="skip the first N note-ons of B (default: auto-align)",
    )
    ap.add_argument("--align-window", type=int, default=64)
    ap.add_argument("--tol-pitch", type=int, default=0)
    ap.add_argument("--tol-vol", type=int, default=0)
    args = ap.parse_args(argv)

    head_a, a = load(args.trace_a)
    head_b, b = load(args.trace_b)

    print(f"A {args.trace_a}: {len(a)} note-ons  {head_a}")
    print(f"B {args.trace_b}: {len(b)} note-ons  {head_b}")
    if not a or not b:
        print("\none side has no note-ons - nothing to compare")
        return 1

    vags_a = a[0].get("n_vags") if a else 0
    vags_b = b[0].get("n_vags") if b else 0
    if vags_a != vags_b:
        print(
            f"\nWARNING: distinct-VAG counts differ (A={vags_a}, B={vags_b}). "
            "VAG ids are renumbered per trace, so the 'vag' channel is not "
            "comparable here - read the 'pitch' channel instead."
        )

    off = args.offset if args.offset is not None else best_offset(a, b, args.align_window)
    print(f"\nalignment: B offset {off}" + ("" if args.offset is not None else " (auto)"))
    b_al = b[off:]

    res = compare(a, b_al, args.tol_pitch, args.tol_vol)
    print(f"aligned overlap: {res['overlap']} note-ons")
    if len(a) != len(b_al):
        print(
            f"note-count mismatch: A has {len(a)}, B has {len(b_al)} after "
            f"alignment (delta {len(b_al) - len(a):+d})"
        )

    if not res["first"]:
        print("\nno divergence on any channel")
        return 0

    print("\nDIVERGENCE")
    for ch in CHANNELS:
        if ch not in res["first"]:
            continue
        idx = res["first"][ch]
        print(
            f"\n  channel {ch}: first at note {idx} "
            f"({res['counts'][ch]} of {res['overlap']} differ)"
        )
        print(context(a, b_al, idx, ch))
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
