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
import struct
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
