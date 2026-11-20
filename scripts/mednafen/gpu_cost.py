"""What one retail ordering table costs the GPU to draw.

Walks the DMA2 linked list `DrawOTag` hands the GPU and prices each GP0 command on
the v2 GPU's measured costs as DuckStation's `src/core/gpu.cpp` records them
(`AddDrawTriangleTicks`, `AddDrawRectangleTicks`, `AddDrawLineTicks`, the polygon
setup table, the VRAM fill), and each list node on its DMA costs (`src/core/dma.cpp`:
a header read of 8, a block setup of 5, a cycle a word). Every figure is in CPU cycles
(33.8688 MHz); one NTSC scanline is 3413 video cycles, 3413 * 7 / 11 CPU cycles
(psx-spx: the video clock is the CPU's * 11 / 7). Nothing is fitted to a capture.
`scripts/pcsx-redux/lib/probe/gpu_cost.lua` is the same model for PCSX-Redux probes.
See docs/subsystems/battle-stage-camera.md, "What a real console's frame costs".
"""
import math, struct
HBLANK_CPU = 3413 * 7 / 11  # one NTSC scanline in CPU cycles (video clock = CPU * 11/7)
SETUP = {(False,False,False):50,(False,False,True):130,(False,True,False):200,(False,True,True):295,
         (True,False,False):50,(True,False,True):215,(True,True,False):335,(True,True,True):495}
LL_HEADER, LL_BLOCK = 8, 5   # DMA2 linked-list header read / block setup (CPU halted)

def s11(v): v &= 0x7FF; return v - 0x800 if v & 0x400 else v
def xy(w): return s11(w), s11(w >> 16)

class Area:
    def __init__(s): s.x0,s.y0,s.x1,s.y1=0,0,319,239; s.ox=s.oy=0; s.tmode=0; s.mask=False

def clampv(a, v):
    x=min(max(v[0]+a.ox,a.x0),a.x1+1); y=min(max(v[1]+a.oy,a.y0),a.y1+1); return (x,y)

def tri_ticks(a, v1,v2,v3, shaded, textured, semi):
    v1,v2,v3 = clampv(a,v1),clampv(a,v2),clampv(a,v3)
    pix = abs(v1[0]*v2[1]+v2[0]*v3[1]+v3[0]*v1[1]-v1[0]*v3[1]-v2[0]*v1[1]-v3[0]*v2[1])//2
    rows = max(v1[1],v2[1],v3[1]) - min(v1[1],v2[1],v3[1])
    semi = semi or a.mask
    flat = not shaded and not textured
    q = (3 if semi else 2) if flat else 4
    rt = 2 + ((3 if flat else 1) if semi else 0)
    return (pix*q+3)//4 + rows*rt

def rect_ticks(a, x, y, w, h, textured, semi):
    x0=max(x+a.ox,a.x0); y0=max(y+a.oy,a.y0); x1=min(x+a.ox+w,a.x1+1); y1=min(y+a.oy+h,a.y1+1)
    dw=max(0,x1-x0); dh=max(0,y1-y0)
    semi = semi or a.mask
    ppr = dw if semi else (dw & ~1)
    e = (64 if semi else 16) + ppr*(6 if semi else 4)
    if textured:
        m=a.tmode
        if m==0: e += dw*6
        elif m==1:
            if dw>128: e+=dw*11
            elif dw*dh>2048: e+=((dw//4)*(4*(128//dw)))*11//2 if dw else 0
            else: e+=dw*11//2
        else:
            if dw>128: e+=dw*20
            elif dw*dh>1024: e+=((dw//4)*(8*(128//dw)))*5 if dw else 0
            else: e+=dw*5
    if 0<dw<8: e=max(e,36)
    return 50 + (e*dh+7)//8

def packet_ticks(a, w, stats):
    """Ticks for one packet's GPU commands (list of words)."""
    t=0; i=0
    while i < len(w):
        c = w[i] >> 24
        if 0x20 <= c <= 0x3F:
            quad=bool(c&8); shaded=bool(c&0x10); tex=bool(c&4); semi=bool(c&2)
            nv = 4 if quad else 3
            per = 1 + (1 if tex else 0) + (1 if shaded else 0)
            n = 1 + nv*per - (1 if shaded else 0)
            if i+n > len(w): break
            vs=[]; j=i+1
            for k in range(nv):
                if shaded and k>0: j+=1
                vs.append(xy(w[j])); j+=1
                if tex:
                    if k==1: a.tmode=(w[j]>>23)&3
                    j+=1
            t += SETUP[(quad,shaded,tex)]
            t += tri_ticks(a,vs[0],vs[1],vs[2],shaded,tex,semi)
            if quad: t += tri_ticks(a,vs[1],vs[2],vs[3],shaded,tex,semi)
            stats['poly']+=1; i+=n
        elif 0x60 <= c <= 0x7F:
            tex=bool(c&4); semi=bool(c&2); size=(c>>3)&3
            n = 2 + (1 if tex else 0) + (1 if size==0 else 0)
            if i+n>len(w): break
            x,y = xy(w[i+1])
            if size==0: wh=w[i+2+(1 if tex else 0)]; ww,hh=wh&0x3FF,(wh>>16)&0x1FF
            else: ww=hh={1:1,2:8,3:16}[size]
            t += rect_ticks(a,x,y,ww,hh,tex,semi); stats['rect']+=1; i+=n
        elif 0x40 <= c <= 0x5F:
            shaded=bool(c&0x10); semi=bool(c&2); poly=bool(c&8)
            # walk vertices until terminator for polylines
            j=i+1; pts=[]
            while j < len(w):
                if shaded and pts: j+=1
                if j>=len(w): break
                if poly and pts and (w[j] & 0xF000F000)==0x50005000: j+=1; break
                pts.append(xy(w[j])); j+=1
                if not poly and len(pts)==2: break
            for p,q in zip(pts,pts[1:]):
                dw=abs(p[0]-q[0]); dh=abs(p[1]-q[1])
                t += (124 if shaded else 60) + (max(dw,dh)*4 + dh*(26 if semi else 16)+3)//4
            stats['line']+=1; i=j
        elif c == 0x02:
            if i+3>len(w): break
            wh=w[i+2]; ww=((wh&0x3FF)+0xF)&~0xF; hh=(wh>>16)&0x1FF
            t += 10 + 5*hh + (ww*hh)//12; stats['fill']+=1; i+=3
        elif c == 0xE1: a.tmode=(w[i]>>7)&3; t+=1; i+=1
        elif c == 0xE3: a.x0=w[i]&0x3FF; a.y0=(w[i]>>10)&0x1FF; t+=1; i+=1
        elif c == 0xE4: a.x1=w[i]&0x3FF; a.y1=(w[i]>>10)&0x1FF; t+=1; i+=1
        elif c == 0xE5: a.ox=s11(w[i]); a.oy=s11(w[i]>>11); t+=1; i+=1
        elif c == 0xE6: a.mask=bool(w[i]&2); t+=1; i+=1
        elif c in (0x80,0xA0,0xC0): break  # VRAM copies: not costed
        else: i+=1
    return t

def ot_cost(ram, start_va, area=None, limit=40000):
    """Walk the DMA2 linked list from `start_va` in a main-RAM image: draw cycles,
    DMA cycles and per-kind counts."""
    a = area or Area()
    stats={'poly':0,'rect':0,'line':0,'fill':0,'nodes':0,'words':0}
    gpu=dma=0; addr=start_va & 0x1FFFFF; n=0
    while n < limit:
        if addr+4 > len(ram): break
        h = struct.unpack_from('<I', ram, addr)[0]
        cnt = h >> 24; nxt = h & 0xFFFFFF
        stats['nodes']+=1; stats['words']+=cnt
        dma += LL_HEADER + (LL_BLOCK + cnt if cnt else 0)
        if cnt:
            ws = list(struct.unpack_from('<%dI'%cnt, ram, addr+4))
            gpu += packet_ticks(a, ws, stats)
        if nxt == 0xFFFFFF or (nxt & 0x800000): break
        addr = nxt & 0x1FFFFC; n+=1
    return gpu, dma, stats


def drawn_table(ram, scratch=None):
    """The table the GPU was handed last, in hblanks: (draw, dma, polygons). It is the
    record the frame-end flip left behind the current one (`gp+0x434`), walked from its
    last bucket (`DrawOTag(ot + len * 4 - 4)`, `0x80017264..0x80017288`; `len` is the
    halfword at `0x1F8003A6`, 4096 in every battle state)."""
    idx = struct.unpack_from('<h', ram, 0x7B74C)[0] & 1
    base = struct.unpack_from('<I', ram, 0x7BF30 + (idx ^ 1) * 0x74 + 0x70)[0]
    otlen = struct.unpack_from('<H', scratch, 0x3A6)[0] if scratch else 4096
    if not (0x80000000 <= base < 0x80200000) or otlen == 0:
        return 0.0, 0.0, 0
    draw, dma, st = ot_cost(ram, base + otlen * 4 - 4)
    return draw / HBLANK_CPU, dma / HBLANK_CPU, st['poly']


# --- Mednafen's own rules -------------------------------------------------------------
#
# What the mednafen captures measure. Mednafen 1.29.0 (`src/psx/gpu.cpp`,
# `gpu_polygon.cpp`, `gpu_sprite.cpp`, `gpu_line.cpp`, `gpu_common.inc`, `dma.cpp`) keeps
# a GPU time budget `DrawTimeAvail` that grows two units a CPU cycle, saturates at 256,
# and each command spends; a command waits in the FIFO while the budget is negative, and
# the linked-list DMA feeds the FIFO only when it is ready, so drawing and DMA run in
# series. The units below are that budget's (half CPU cycles). See
# docs/tooling/emulator-timing.md for the rules and where each lives.

MED_HBLANK_CPU = 3412.5 * 44100 * 768 / 53693181.818  # an NTSC line: 3412.5 GPU clocks

class MedGpu:
    """GPU state the costs depend on: clip, offset, texture page, mask, CLUT and
    texture caches."""
    def __init__(s):
        s.cx0, s.cy0, s.cx1, s.cy1 = 0, 0, 319, 239
        s.ox = s.oy = 0
        s.tpx = s.tpy = s.tmode = 0
        s.mask = False
        s.clut = None
        s.tag = [None] * 256
        s.stats = dict(poly=0, rect=0, line=0, fill=0, texmiss=0, clut=0)

    def inval(s):
        s.tag = [None] * 256

    def tpage(s, w):
        # SetTPage: a new page or a palette/direct switch drops the texture cache.
        px, py, m = (w & 0xF) * 64, (w & 0x10) * 16, (w >> 7) & 3
        if (not m) != (not s.tmode) or px != s.tpx or py != s.tpy:
            s.inval()
        s.tpx, s.tpy, s.tmode = px, py, m

    def clut_load(s, raw):
        # Update_CLUT_Cache: 16 or 256 units when the palette word changes.
        if s.tmode >= 2:
            return 0
        k = (raw & 0x7FFF) | (s.tmode << 16)
        if k == s.clut:
            return 0
        s.clut = k
        c = 256 if s.tmode else 16
        s.stats['clut'] += c
        return c

    def texel(s, u, v):
        # GetTexel: a 256-entry cache of four-halfword VRAM blocks, 4 units a miss.
        m = min(s.tmode, 2)
        gro = ((v & 0xFF) + s.tpy & 511) * 1024 + ((((u & 0xFF) + (s.tpx << (2 - m))) >> (2 - m)) & 1023)
        i = ((gro >> 2) & 3) | ((gro >> 8) & 0xFC) if m == 0 else ((gro >> 2) & 7) | ((gro >> 7) & 0xF8)
        if s.tag[i] == gro & ~3:
            return 0
        s.tag[i] = gro & ~3
        s.stats['texmiss'] += 1
        return 4


def med_triangle(s, vs, shaded, tex, semi):
    """DrawTriangle: spans of the rows inside the clip, two units a row walked above it,
    and a texel fetch a textured pixel. `vs` is four-tuples `(x, y, u, v)`."""
    vs = list(vs)
    if vs[1][0] <= vs[0][0]:
        cv = 4 if vs[2][0] <= vs[1][0] else 2
    else:
        cv = 4 if vs[2][0] < vs[0][0] else 1
    if vs[2][1] < vs[1][1]:
        vs[1], vs[2] = vs[2], vs[1]; cv = ((cv >> 1) & 2) | ((cv << 1) & 4) | (cv & 1)
    if vs[1][1] < vs[0][1]:
        vs[0], vs[1] = vs[1], vs[0]; cv = ((cv >> 1) & 1) | ((cv << 1) & 2) | (cv & 4)
    if vs[2][1] < vs[1][1]:
        vs[1], vs[2] = vs[2], vs[1]; cv = ((cv >> 1) & 2) | ((cv << 1) & 4) | (cv & 1)
    core = cv >> 1
    (x0, y0, u0, v0), (x1, y1, u1, v1), (x2, y2, u2, v2) = vs
    if y0 == y2 or y2 - y0 >= 512 or max(abs(x2 - x0), abs(x2 - x1), abs(x1 - x0)) >= 1024:
        return 0
    den = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0)
    if den == 0:
        return 0
    if tex:
        dudx = ((u1 - u0) * (y2 - y0) - (u2 - u0) * (y1 - y0)) / den
        dudy = ((u2 - u0) * (x1 - x0) - (u1 - u0) * (x2 - x0)) / den
        dvdx = ((v1 - v0) * (y2 - y0) - (v2 - v0) * (y1 - y0)) / den
        dvdy = ((v2 - v0) * (x1 - x0) - (v1 - v0) * (x2 - x0)) / den
        cx, cy, cu, cvv = vs[core]
    cost = 0
    for ya, yb, dec in ((y0, y1, core != 0), (y1, y2, core == 2)):
        if ya == yb:
            continue
        for y in (range(yb - 1, ya - 1, -1) if dec else range(ya, yb)):
            # The walk stops at the clip edge it walks towards and pays two units a
            # row on the edge it walks away from.
            if (y < s.cy0) if dec else (y > s.cy1):
                break
            if (y > s.cy1) if dec else (y < s.cy0):
                cost += 2
                continue
            xl = x0 + (x2 - x0) * (y - y0) / (y2 - y0)
            xr = (x0 + (x1 - x0) * (y - y0) / (y1 - y0)) if y < y1 else \
                 (x1 + (x2 - x1) * (y - y1) / (y2 - y1))
            a, b = min(xl, xr), max(xl, xr)
            xs, xe = max(math.ceil(a - 1e-9), s.cx0), min(math.ceil(b - 1e-9), s.cx1 + 1)
            w = xe - xs
            if w <= 0:
                continue
            # DrawSpan: 2 units a shaded or textured pixel, 1.5 a flat blended or
            # mask-tested one, 1 a flat one.
            cost += 2 * w if (shaded or tex) else (w + ((w + 1) >> 1)) if (semi or s.mask) else w
            if tex:
                bu = cu + dudx * (xs - cx) + dudy * (y - cy) + 0.5
                bv = cvv + dvdx * (xs - cx) + dvdy * (y - cy) + 0.5
                for k in range(w):
                    cost += s.texel(int(bu + dudx * k), int(bv + dvdx * k))
    return cost


def med_commands(s, w):
    """`(words, units)` per GPU command of one packet, in order. A quad is two commands
    to the FIFO: the first triangle with the packet's words, the second with none."""
    out, i = [], 0
    while i < len(w):
        c = w[i] >> 24
        if 0x20 <= c <= 0x3F:
            quad, shaded, tex, semi = bool(c & 8), bool(c & 0x10), bool(c & 4), bool(c & 2)
            nv = 4 if quad else 3
            n = 1 + nv * (1 + tex + shaded) - shaded
            if i + n > len(w):
                break
            vs, j, clut, tp = [], i + 1, 0, None
            for k in range(nv):
                if shaded and k > 0:
                    j += 1
                x, y = s11(w[j]) + s.ox, s11(w[j] >> 16) + s.oy
                j += 1
                uu = vv = 0
                if tex:
                    uu, vv = w[j] & 0xFF, (w[j] >> 8) & 0xFF
                    if k == 0: clut = (w[j] >> 16) & 0xFFFF
                    if k == 1: tp = (w[j] >> 16) & 0xFFFF
                    j += 1
                vs.append((x, y, uu, vv))
            if tp is not None:
                s.tpage(tp)
            # Command_DrawPolygon: 64 + 18 units (28 + 18 for a quad's second half),
            # plus 450 Gouraud-textured, 288 Gouraud, 180 textured; 2 a command.
            ext = 450 if (shaded and tex) else 288 if shaded else 180 if tex else 0
            u = 2 + 82 + ext + (s.clut_load(clut) if tex else 0)
            u += med_triangle(s, vs[:3], shaded, tex, semi)
            out.append((n, u))
            if quad:
                out.append((0, 46 + ext + med_triangle(s, vs[1:4], shaded, tex, semi)))
            s.stats['poly'] += 1
            i += n
        elif 0x60 <= c <= 0x7F:
            tex, semi, size = bool(c & 4), bool(c & 2), (c >> 3) & 3
            n = 2 + tex + (size == 0)
            if i + n > len(w):
                break
            x, y = s11(w[i + 1]) + s.ox, s11(w[i + 1] >> 16) + s.oy
            u0 = v0 = 0
            u = 2 + 16  # Command_DrawSprite's flat 16
            if tex:
                u0, v0 = w[i + 2] & 0xFF, (w[i + 2] >> 8) & 0xFF
                u += s.clut_load((w[i + 2] >> 16) & 0xFFFF)
            if size == 0:
                wh = w[i + 2 + tex]; ww, hh = wh & 0x3FF, (wh >> 16) & 0x1FF
            else:
                ww = hh = {1: 1, 2: 8, 3: 16}[size]
            xs, xe = max(x, s.cx0), min(x + ww, s.cx1 + 1)
            for yy in range(max(y, s.cy0), min(y + hh, s.cy1 + 1)):
                if xe > xs:
                    u += (xe - xs) + (((((xe + 1) & ~1) - (xs & ~1)) >> 1) if (semi or s.mask) else 0)
                    if tex:
                        for xx in range(xs, xe):
                            u += s.texel(u0 + xx - x, v0 + yy - y)
            out.append((n, u)); s.stats['rect'] += 1; i += n
        elif 0x40 <= c <= 0x5F:
            shaded, poly = bool(c & 0x10), bool(c & 8)
            j, pts = i + 1, []
            while j < len(w):
                if shaded and pts: j += 1
                if j >= len(w): break
                if poly and pts and (w[j] & 0xF000F000) == 0x50005000: j += 1; break
                pts.append(xy(w[j])); j += 1
                if not poly and len(pts) == 2: break
            u = 2
            for p, q in zip(pts, pts[1:]):
                dx, dy = abs(p[0] - q[0]), abs(p[1] - q[1])
                if dx < 1024 and dy < 512:
                    u += 16 + 2 * max(dx, dy)  # Command_DrawLine: 16, then 2 a step
            out.append((j - i, u)); s.stats['line'] += 1; i = j
        elif c == 0x02:
            if i + 3 > len(w): break
            wh = w[i + 2]; ww = ((wh & 0x3FF) + 0xF) & ~0xF; hh = (wh >> 16) & 0x1FF
            out.append((3, 2 + 46 + hh * ((ww >> 3) + 9))); s.stats['fill'] += 1; i += 3
        elif c == 0x01:
            s.clut = None; s.inval(); out.append((1, 2)); i += 1
        elif c == 0xE1:
            s.tpage(w[i]); out.append((1, 2)); i += 1
        elif c == 0xE3:  # the clip and offset words are "ss" commands: free
            s.cx0, s.cy0 = w[i] & 0x3FF, (w[i] >> 10) & 0x3FF; out.append((1, 0)); i += 1
        elif c == 0xE4:
            s.cx1, s.cy1 = w[i] & 0x3FF, (w[i] >> 10) & 0x3FF; out.append((1, 0)); i += 1
        elif c == 0xE5:
            s.ox, s.oy = s11(w[i]), s11(w[i] >> 11); out.append((1, 0)); i += 1
        elif c == 0xE6:
            s.mask = bool(w[i] & 2); out.append((1, 2)); i += 1
        elif c in (0xE2,):
            out.append((1, 2)); i += 1
        elif 0x80 <= c <= 0x9F:
            if i + 4 > len(w): break
            wh = w[i + 3]; ww = (wh & 0x3FF) or 0x400; hh = ((wh >> 16) & 0x1FF) or 0x200
            s.inval(); out.append((4, 2 + 2 * ww * hh)); i += 4  # Command_FBCopy
        elif 0xA0 <= c <= 0xDF:
            s.inval(); out.append((len(w) - i, 2)); break  # VRAM transfers: not costed
        else:
            out.append((1, 0)); i += 1
    return out


def med_ot_cost(ram, start_va, limit=40000):
    """Mednafen's time from `DrawOTag` to an idle GPU over the DMA2 linked list at
    `start_va`, in CPU cycles, with the DMA's share and per-kind counts. The DMA reads
    a node header in 15 cycles (10 for an empty node) and a word a cycle, and waits for
    the GPU before each node and each command."""
    s = MedGpu()
    t, avail, dma, nodes = 0.0, 256.0, 0, 0

    def run(c):
        nonlocal t, avail
        t += c
        avail = min(256.0, avail + 2 * c)

    def drain():
        nonlocal t, avail
        if avail < 0:
            t += -avail / 2
            avail = 0.0

    addr, n = start_va & 0x1FFFFF, 0
    while n < limit and addr + 4 <= len(ram):
        h = struct.unpack_from('<I', ram, addr)[0]
        cnt, nxt = h >> 24, h & 0xFFFFFF
        drain()
        run(15 if cnt else 10); dma += 15 if cnt else 10; nodes += 1
        if cnt:
            for words, units in med_commands(s, list(struct.unpack_from('<%dI' % cnt, ram, addr + 4))):
                run(words); dma += words
                drain()
                avail -= units
        if nxt == 0xFFFFFF or (nxt & 0x800000):
            break
        addr, n = nxt & 0x1FFFFC, n + 1
    drain()
    s.stats.update(nodes=nodes)
    return t, dma, s.stats


def drawn_table_mednafen(ram, scratch=None):
    """`drawn_table` on Mednafen's rules: (total, dma) in hblanks, and the polygons.
    The total is the whole wait (drawing and the DMA in series), not a sum to add."""
    idx = struct.unpack_from('<h', ram, 0x7B74C)[0] & 1
    base = struct.unpack_from('<I', ram, 0x7BF30 + (idx ^ 1) * 0x74 + 0x70)[0]
    otlen = struct.unpack_from('<H', scratch, 0x3A6)[0] if scratch else 4096
    if not (0x80000000 <= base < 0x80200000) or otlen == 0:
        return 0.0, 0.0, 0
    t, dma, st = med_ot_cost(ram, base + otlen * 4 - 4)
    return t / MED_HBLANK_CPU, dma / MED_HBLANK_CPU, st['poly']
