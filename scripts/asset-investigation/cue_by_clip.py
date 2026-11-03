#!/usr/bin/env python3
"""Per seat and clip, the sound cues of `cast-sequence.py --physical` recordings.

    scripts/asset-investigation/cue_by_clip.py <dir holding one recording per state>

Each cue is filed under the most recent clip commit on any seat, with its offset
from that commit, on both sides, so a clip both sides played reads cue for cue
whichever clip each stream's rolls chose. The recordings hold retail RAM: keep
them out of the repo. See docs/tooling/cast-sequence-oracle.md.
"""
import collections
import csv
import pathlib
import sys

def load(d,engine):
    d=pathlib.Path(d)
    seats={}
    if engine:
        for r in csv.reader(open(d/'seats.csv')):
            t=int(r[0]); seats[t]={}
            for s in range(8):
                o=1+s*4
                if o+3>=len(r): break
                if r[o]=='-1' and r[o+1]=='-1': continue
                seats[t][s]=(int(r[o]),int(r[o+3]),int(r[o+2]))  # clip, reaction, cue
        sfx=[(int(r['tick']),int(r['id'])) for r in csv.DictReader(open(d/'sfx.csv'))]
    else:
        for r in csv.DictReader(open(d/'seats.csv')):
            t=int(r['vsync']); seats[t]={}
            for s in range(8):
                seats[t][s]=(int(r[f's{s}clip']),-1,int(r[f's{s}cue']))
        sfx=[(int(r['vsync']),int(r['id'])) for r in csv.DictReader(open(d/'sfx.csv')) if r['drain']=='1']
    return seats,sfx
def spans(seats,engine):
    """(seat,clip) -> [start ticks]"""
    out=collections.defaultdict(list); prev={}
    for t in sorted(seats):
        for s,(c,rc,_) in seats[t].items():
            eff = rc if (engine and rc>0) else c
            if eff!=prev.get(s,0) and eff>0: out[(s,eff)].append(t)
            prev[s]=eff
    return out
def attrib(seats,sfx,engine):
    ts=sorted(seats); res=collections.defaultdict(list); pc={}
    sp=spans(seats,engine)
    for t,i in sfx:
        best=None
        for (s,c),st in sp.items():
            for b in st:
                if b<=t and (best is None or t-b<best[0]): best=(t-b,s,c)
        res[(best[1],best[2]) if best else None].append((best[0] if best else t,i))
    return res
def main(root):
    for d in sorted(pathlib.Path(root).iterdir()):
        if not (d/'retail/sfx.csv').exists() or not (d/'engine/sfx.csv').exists() or not (d/'engine/seats.csv').exists():
            print(d.name,'-- incomplete'); continue
        rs,rf=load(d/'retail',False); es,ef=load(d/'engine',True)
        ra=attrib(rs,rf,False); ea=attrib(es,ef,True)
        print('==',d.name,'retail',[i for _,i in rf],'engine',[i for _,i in ef])
        for k in sorted(set(ra)|set(ea),key=str):
            print('  ',k,'R',sorted(ra.get(k,[])),'E',sorted(ea.get(k,[])))
main(sys.argv[1])
