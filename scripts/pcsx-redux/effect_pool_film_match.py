#!/usr/bin/env python3
"""Find the vsync of an effect-pool film whose pool equals a capture's.

Reads the `pool.txt` that `autorun_effect_pool_film.lua` writes and a raw
2 MiB main-RAM image of the later capture (`pcsxr-state extract --start
0x80000000 --end 0x80200000`, or `mednafen-state extract`), and prints the
best-matching vsyncs with the live child slots of the lines before the best
one: the children the capture's displayed frame (two game frames older than
its RAM) drew.

    effect_pool_film_match.py <pool.txt> <ram.bin> [--before N]
"""
import argparse
import struct

POOL_PTR = 0x8007BD30


def ram_pool(ram):
    u32 = lambda a: struct.unpack_from('<I', ram, a - 0x80000000)[0]
    i32 = lambda a: struct.unpack_from('<i', ram, a - 0x80000000)[0]
    pool = u32(POOL_PTR)
    out = {}
    for s in range(128):
        c = pool + 0x10 + s * 0x20
        b = ram[c - 0x80000000:c - 0x80000000 + 4]
        if b[0] == 0:
            continue
        out[s] = (b[0], b[1], b[2], b[3], i32(c + 12), i32(c + 16), i32(c + 20), u32(c + 0x1C))
    return out


def film(path):
    frames = []
    for line in open(path):
        tok = line.split()
        if len(tok) < 4:
            continue
        kids = {}
        for t in tok[4:]:
            f = t.split(':')
            kids[int(f[0])] = tuple(int(x) for x in f[1:8]) + (int(f[8], 16),)
        frames.append((int(tok[1]), kids))
    return frames


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('pool')
    ap.add_argument('ram')
    ap.add_argument('--before', type=int, default=8)
    a = ap.parse_args()
    target = ram_pool(open(a.ram, 'rb').read())
    frames = film(a.pool)
    scored = []
    for i, (v, kids) in enumerate(frames):
        same = sum(1 for s, k in target.items() if kids.get(s) == k)
        scored.append((same - abs(len(kids) - len(target)), i, v, same, len(kids)))
    scored.sort(reverse=True)
    print(f'capture pool: {len(target)} live children')
    for sc, i, v, same, n in scored[:5]:
        print(f'  vsync {v}: {same}/{len(target)} slots identical, film holds {n}')
    best = scored[0][1]
    for j in range(max(0, best - a.before), best + 1):
        v, kids = frames[j]
        print(f'vsync {v}:')
        for s, k in sorted(kids.items()):
            print(f'  slot {s:3}: fc {k[0]} mirror {k[1]} cursor {k[2]} wait {k[3]} '
                  f'pos ({k[4] >> 8}, {k[5] >> 8}, {k[6] >> 8}) anim {k[7]:08X}')


if __name__ == '__main__':
    main()
