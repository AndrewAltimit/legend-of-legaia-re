#!/usr/bin/env python3
"""Batch-scan every catalogued save state for the slot-B loader current-id at
0x8007BC4C (gp+0x934).  Reports every state's id; flags 0x1D/0x1E/0x1F.
PCSX-Redux .sstate: gzipped-protobuf walk (extract_ram from
scripts/pcsx-redux/match_prim_groups_to_disc.py).  Mednafen .mcr backups:
`mednafen-state extract` window."""
import sys, os, struct, subprocess, re
sys.path.insert(0, "/home/mikunpc/Documents/repos/legend-of-legaia-re/scripts/pcsx-redux")
from match_prim_groups_to_disc import extract_ram

REPO = "/home/mikunpc/Documents/repos/legend-of-legaia-re"
LIB = os.path.join(REPO, "saves/library")
ADDR = 0x8007BC4C
BASE = 0x80000000

# label map from scenarios.toml backup_fingerprint / pcsx_redux fields
labels = {}
cur = None
for line in open(os.path.join(REPO, "scripts/scenarios.toml")):
    m = re.match(r'label\s*=\s*"([^"]+)"', line.strip())
    if m: cur = m.group(1)
    m = re.match(r'\w*backup_fingerprint\w*\s*=\s*"([0-9a-f]{64})"', line.strip())
    if m and cur: labels[m.group(1)] = cur
    m = re.search(r'([0-9a-f]{64})', line)
    if m and cur and ("fingerprint" in line or "sstate" in line):
        labels.setdefault(m.group(1), cur)

hits = []
rows = []
pdir = os.path.join(LIB, "pcsx-redux")
for f in sorted(os.listdir(pdir)):
    if not f.endswith(".sstate"): continue
    p = os.path.join(pdir, f)
    try:
        ram = extract_ram(p)
        v = struct.unpack("<I", ram[ADDR-BASE:ADDR-BASE+4])[0]
    except Exception as e:
        rows.append((f, "ERR:"+str(e)[:40])); continue
    lab = labels.get(f.split(".")[0], "?")
    rows.append((f[:16]+" P "+lab, v))
    if v in (0x1D,0x1E,0x1F): hits.append((p,lab,v))

mdir = os.path.join(LIB, "mednafen")
for f in sorted(os.listdir(mdir)):
    p = os.path.join(mdir, f)
    try:
        out = subprocess.run([os.path.join(REPO,"target/release/mednafen-state"),
            "extract", p, "--start", hex(ADDR), "--end", hex(ADDR+4),
            "--out", "/tmp/claude-1000/win.bin"],
            capture_output=True, timeout=60)
        blob = open("/tmp/claude-1000/win.bin","rb").read()
        if len(blob) < 4:
            rows.append((f, "ERR len %d" % len(blob))); continue
        v = struct.unpack("<I", blob[:4])[0]
    except Exception as e:
        rows.append((f, "ERR:"+str(e)[:40])); continue
    lab = labels.get(f.split(".")[0], "?")
    rows.append((f[:16]+" M "+lab, v))
    if v in (0x1D,0x1E,0x1F): hits.append((p,lab,v))

for name, v in rows:
    print("%-60s %s" % (name, v if isinstance(v,str) else hex(v)))
print("\n=== hits (0x1D/0x1E/0x1F) ===")
for p,lab,v in hits: print(hex(v), lab, p)
