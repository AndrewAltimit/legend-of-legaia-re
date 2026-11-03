#!/usr/bin/env python3
"""Read a scripts/profile/sampler.c run: self and inclusive CPU by symbol.

    report.py <binary> <samples-file> [--top N] [--under REGEX]

`--under REGEX` adds the split of samples below the first frame matching
REGEX: what that function's time went to. The binary must be the file that
ran (cargo relinks test binaries in place: copy it first when a later build
may replace it). Samples in libc or the kernel's vdso show as `?`.
"""

import argparse
import bisect
import collections
import re
import struct
import subprocess


def load_base(samples, binary):
    """The address the binary's first mapping starts from, less its offset."""
    name = binary.rsplit("/", 1)[-1]
    with open(samples + ".maps") as maps:
        for line in maps:
            fields = line.split()
            if len(fields) >= 6 and fields[5].endswith(name):
                return int(fields[0].split("-")[0], 16) - int(fields[2], 16)
    raise SystemExit(f"{name} is not in {samples}.maps")


def load_symbols(binary):
    out = subprocess.run(
        ["nm", "-C", "--defined-only", binary], capture_output=True, text=True, check=True
    ).stdout
    symbols = []
    for line in out.splitlines():
        fields = line.split(" ", 2)
        if len(fields) == 3 and fields[1] in "tTwW":
            symbols.append((int(fields[0], 16), re.sub(r"::h[0-9a-f]{16}$", "", fields[2])))
    symbols.sort()
    return symbols


def read_samples(path):
    """Each record: a word count, then that many return addresses."""
    with open(path, "rb") as f:
        data = f.read()
    at = 0
    while at + 8 <= len(data):
        count = struct.unpack_from("<q", data, at)[0]
        at += 8
        if not 0 <= count <= 64 or at + 8 * count > len(data):
            return
        yield struct.unpack_from(f"<{count}Q", data, at)
        at += 8 * count


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("binary")
    ap.add_argument("samples")
    ap.add_argument("--top", type=int, default=40)
    ap.add_argument("--under")
    args = ap.parse_args()
    base = load_base(args.samples, args.binary)
    symbols = load_symbols(args.binary)
    starts = [s[0] for s in symbols]
    span = symbols[-1][0] + 0x1000

    def name(addr):
        addr -= base
        i = bisect.bisect_right(starts, addr) - 1
        return symbols[i][1] if i >= 0 and addr < span else "?"

    under = re.compile(args.under) if args.under else None
    own, inclusive, below = collections.Counter(), collections.Counter(), collections.Counter()
    total = 0
    for frames in read_samples(args.samples):
        # The two innermost frames are the handler and the signal trampoline.
        stack = [name(a) for a in frames[2:]]
        total += 1
        if stack:
            own[stack[0]] += 1
        inclusive.update(set(stack))
        if under:
            for i, frame in enumerate(stack):
                if under.search(frame):
                    below[stack[i - 1] if i else "(self)"] += 1
                    break
    print(f"{total} samples, {total / 100:.1f} CPU seconds")
    sections = [("self", own), ("inclusive", inclusive)]
    if under:
        sections.append((f"under {args.under}", below))
    for title, counts in sections:
        print(f"-- {title}")
        for sym, n in counts.most_common(args.top):
            print(f"{100 * n / max(total, 1):5.1f}% {sym[:120]}")


if __name__ == "__main__":
    main()
