#!/usr/bin/env python3
"""The primitives one retail battle frame draws over one pixel, in draw order.

Walks the ordering table the frame-end flip handed the GPU last (the DRAWENV
record `0x8007BF30 + 0x74 * table`, `+0x70`, from its last bucket; 4096 buckets
in every battle state, as `scripts/mednafen/gpu_cost.py` reads it) out of a main
RAM image, and for every polygon covering the pixel prints its command, its
colour at the pixel, its blend mode, and for a textured one the texel it samples
out of a VRAM image (`sstate_vram.py`). It then prints the pixel the VRAM holds
there, so a layer law can be checked against what the GPU drew.

Inputs come from a cast-sequence recording made with `LEGAIA_RAM_AT`
(`ram_<vsync>.bin`, `state_<vsync>.rawsstate`). Read-only; writes nothing.

Usage: ot_layers.py <ram.bin> <vram.bin> <x> <y> [--table N] [--y-offset N]
"""
import argparse
import struct


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("ram")
    ap.add_argument("vram")
    ap.add_argument("x", type=int)
    ap.add_argument("y", type=int)
    ap.add_argument("--table", type=int, help="DRAWENV record (default: the one behind 0x8007B74C)")
    ap.add_argument("--y-offset", type=int, help="the table's draw-area row (default: the record's own)")
    a = ap.parse_args()
    ram = open(a.ram, "rb").read()
    vr = open(a.vram, "rb").read()

    def w(o):
        return struct.unpack_from("<I", ram, o)[0]

    def h16(o):
        return struct.unpack_from("<H", ram, o)[0]

    def vram(x, y):
        return struct.unpack_from("<H", vr, ((y & 511) * 1024 + (x & 1023)) * 2)[0]

    def s11(v):
        v &= 0x7FF
        return v - 0x800 if v & 0x400 else v

    table = a.table if a.table is not None else (h16(0x7B74C) & 1) ^ 1
    rec = 0x7BF30 + table * 0x74
    yoff = a.y_offset if a.y_offset is not None else h16(rec + 2)
    off = (w(rec + 0x70) + 4096 * 4 - 4) & 0x1FFFFC

    def bary(p, va, vb, vc):
        d = (vb[1] - vc[1]) * (va[0] - vc[0]) + (vc[0] - vb[0]) * (va[1] - vc[1])
        if d == 0:
            return None
        l1 = ((vb[1] - vc[1]) * (p[0] - vc[0]) + (vc[0] - vb[0]) * (p[1] - vc[1])) / d
        l2 = ((vc[1] - va[1]) * (p[0] - vc[0]) + (va[0] - vc[0]) * (p[1] - vc[1])) / d
        l3 = 1 - l1 - l2
        return None if min(l1, l2, l3) < -1e-9 else (l1, l2, l3)

    def texel(tpage, clut, u, v):
        px, py, m = (tpage & 0xF) * 64, ((tpage >> 4) & 1) * 256, (tpage >> 7) & 3
        cx, cy = (clut & 0x3F) * 16, (clut >> 6) & 0x1FF
        if m == 0:
            return vram(cx + ((vram(px + (u >> 2), py + v) >> ((u & 3) * 4)) & 0xF), cy)
        if m == 1:
            return vram(cx + ((vram(px + (u >> 1), py + v) >> ((u & 1) * 8)) & 0xFF), cy)
        return vram(px + u, py + v)

    def c5(h):
        return (h & 31, (h >> 5) & 31, (h >> 10) & 31)

    drawmode = 0
    for _ in range(200000):
        head = w(off)
        cnt, nxt = head >> 24, head & 0xFFFFFF
        ws = [w(off + 4 + 4 * k) for k in range(cnt)]
        i = 0
        while i < cnt:
            c = ws[i] >> 24
            if c == 0xE1:
                drawmode = ws[i] & 0xFFFF
                i += 1
                continue
            if not 0x20 <= c <= 0x3F:
                i += 1
                continue
            quad, tex, shaded = c & 8, c & 4, c & 0x10
            nv = 4 if quad else 3
            ln = 1 + nv * (1 + bool(tex) + bool(shaded)) - bool(shaded)
            p = ws[i:i + ln]
            j, vs, uvs, cols = 1, [], [], [p[0] & 0xFFFFFF]
            for k in range(nv):
                if shaded and k > 0:
                    cols.append(p[j] & 0xFFFFFF)
                    j += 1
                vs.append((s11(p[j]), s11(p[j] >> 16)))
                j += 1
                if tex:
                    uvs.append(p[j])
                    j += 1
            if not shaded:
                cols = cols * nv
            for t in [(0, 1, 2)] + ([(1, 2, 3)] if quad else []):
                b = bary((a.x, a.y), vs[t[0]], vs[t[1]], vs[t[2]])
                if not b:
                    continue
                col = [round(sum(b[k] * ((cols[t[k]] >> (8 * ch)) & 0xFF) for k in range(3))) for ch in range(3)]
                line = f"{c:#04x} semi={bool(c & 2)} colour={col}"
                if tex:
                    u = round(sum(b[k] * (uvs[t[k]] & 0xFF) for k in range(3)))
                    v = round(sum(b[k] * ((uvs[t[k]] >> 8) & 0xFF) for k in range(3)))
                    tp, cl = uvs[1] >> 16, uvs[0] >> 16
                    tx = texel(tp, cl, u, v)
                    line += f" tpage={tp:#06x} abr={(tp >> 5) & 3} texel={c5(tx)} stp={tx >> 15}"
                else:
                    line += f" abr={(drawmode >> 5) & 3} (draw mode)"
                print(line)
                break
            i += ln
        if nxt == 0xFFFFFF or nxt & 0x800000:
            break
        off = nxt & 0x1FFFFC
    print(f"vram pixel {c5(vram(a.x, a.y + yoff))} (5-bit), table {table}, row offset {yoff}")


if __name__ == "__main__":
    main()
