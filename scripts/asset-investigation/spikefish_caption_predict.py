#!/usr/bin/env python3
"""Predict whether a Spikefish Flute escape freezes the game, per scene.

Backs docs/reference/glitches/field-oddities.md ("Spikefish Flute escape
can freeze the game"). The flute's escape leaves its attack-name caption
widget alive. The widget points at the name inside the battle side-band
buffer (0x800B995C: buffer 0x800B990C + name offset 0x50 of the Spikefish
actor-record slot). After the teardown, PROT 0978's field-back read
allocates the largest free heap block, top-down, ending at 0x800E7C9C:

    sectors = min(largest_free_block // 0x800, 0xC5)
    buf     = 0x800E7C9C - (sectors * 0x800 - 4)

It fills that block with the scene's own disc bytes starting 0x20 sectors
into the scene's first PROT entry (the CDNAME define in
data/gamedata/scenes.toml, e.g. jouind = 680, rugi = 696). So the caption
reads disc bytes at

    PROT.DAT[(start_lba(scene) + 0x20) * 0x800 + (0x800B995C - buf)]

once the read lands. This script reproduces that, then simulates what the
text renderer FUN_80036888 and its substitution expander FUN_80036514 do
with those bytes each frame:

    * renderer: a first byte below 0x20 draws nothing (returns at once);
    * expander pass 1: walks to the NUL, rewriting 5E XX -> CE (XX-0x2D)
      and FF -> CF in place; a 0xCx byte also skips its operand, so a NUL
      operand does not stop the walk;
    * expander pass 2: copies into 0x800740EC, expanding C1..C7
      substitutions through the SCUS tables (jump table 0x80010EC0) and
      nested C1 names, until the token count runs out or a NUL.

A copy that writes more than 64 KiB, or touches an address outside main RAM
and its mirrors (a bus error on hardware), is called FREEZE (on the captured case the read cursor lands in the output buffer and
chases the write cursor forever). Anything else is SURVIVE.

`sectors` follows from the formation. The largest free block after the
teardown is the enemy stager buffer (0x2E390 bytes at 0x800B9900) plus the
per-species monster blocks allocated directly below it, one per DISTINCT
monster id, each `block[+0x08]` bytes plus a 12-byte node header (the
battle-heap cost the encounter randomizer also uses, decoded from PROT
867):

    sectors = min((0x2E390 + 0xA8 + sum(cost[id] + 12 for distinct ids))
                  // 0x800, 0xC5)

The 0xA8 is fitted (one more freed 0x9C node coalescing). This held on every
captured fight (jouind: 3x 161, 3x 158, 1x and 3x 12, 3x 143, [80, 84],
3x 57; rugi: [150, 141, 141] -> 127, 115, 112, 112, 116, 134, 115, 145
sectors), all with one party (the endgame card's Vahn / Noa / Gala). The
captures bound the true slack to 0x6C..0x17F bytes, so a block that lands
close to a sector boundary (fraction < 0x3C or
>= 0x728) is marked "edge(N:OUTCOME)" with the outcome at the neighbouring count N. --formations runs the whole
chain for every random formation of every scene; --sweep lists the outcome
over every possible sector count instead (the caption is only reached for
sectors >= 93); --sectors takes a measured count
(scripts/pcsx-redux/autorun_spikefish_flee_return.lua, fbr_sec column).

The memory backdrop for everything outside the restored block is a 2 MiB
main-RAM image from a battle (e.g. `pcsxr-state extract <state> --start
0x80000000 --end 0x80200000`). Nothing here embeds disc or RAM bytes; all
inputs are the user's own files.

Usage:
  spikefish_caption_predict.py --prot extracted/PROT.DAT \
      --scus extracted/SCUS_942.54 --ram battle_ram.bin \
      --entry 680 --sectors 127
  spikefish_caption_predict.py ... --entry 680 --sweep
  spikefish_caption_predict.py ... --scenes data/gamedata/scenes.toml --sweep
  spikefish_caption_predict.py ... --scenes data/gamedata/scenes.toml \
      --formations --prot-dir extracted/PROT --asset-bin target/release/asset
"""
import argparse
import glob
import os
import re
import struct
import subprocess
import sys

RAM_BASE = 0x80000000
RAM_SIZE = 0x200000
CAPTION = 0x800B995C
BLOCK_END = 0x800E7C9C
OUT_BUF = 0x800740EC
SCUS_LOAD = 0x8000F800  # VA - file offset
MAX_SECTORS = 0xC5
RUNAWAY = 0x10000


class Fault(Exception):
    pass


class Mem:
    def __init__(self, ram):
        self.b = bytearray(ram)

    def _o(self, a):
        # main RAM and its mirrors fill the first 8 MiB of each segment
        o = (a & 0x1FFFFFFF)
        if o >= 0x800000:
            raise Fault("bus error: access to unmapped %08x" % a)
        return o & (RAM_SIZE - 1)

    def rd(self, a):
        return self.b[self._o(a)]

    def wr(self, a, v):
        self.b[self._o(a)] = v & 0xFF

    def rd32(self, a):
        o = self._o(a)
        return struct.unpack_from("<I", self.b, o)[0]


def expand(m, src, scus):
    """FUN_80036514(dst=OUT_BUF, src, a2=1). Returns bytes written."""
    # pass 1: normalise in place and count tokens
    t3 = 0
    v1 = src
    if m.rd(v1) != 0:
        while True:
            if m.rd(v1) == 0x5E:
                m.wr(v1 + 1, m.rd(v1 + 1) + 0xD3)
                m.wr(v1, 0xCE)
            if m.rd(v1) == 0xFF:
                m.wr(v1, 0xCF)
            if m.rd(v1) & 0xF0 == 0xC0:
                t3 += 1
                v1 += 1
            v1 += 1
            t3 += 1
            if m.rd(v1) == 0:
                break
            if v1 - src > 0x100000:
                raise Fault("pass 1 never meets a NUL")
    a0, a1 = OUT_BUF, src
    written = 0
    t0 = 0  # stale t0 for C6 / C0: the expander leaves the old value
    while t3 > 0:
        a3 = m.rd(a1)
        if a3 == 0:
            break
        t3 -= 1
        if a3 == 0xCF or a3 == 0xCE:
            m.wr(a0, a3)
            m.wr(a0 + 1, m.rd(a1 + 1))
            a0 += 2
            a1 += 2
            written += 2
            continue
        if ((a3 + 0x40) & 0xFF) >= 8:
            m.wr(a0, a3)
            a0 += 1
            a1 += 1
            written += 1
            continue
        # 0xC0..0xC7 substitution
        op = m.rd(a1 + 1)
        k = a3 - 0xC1
        if 0 <= k < 7:
            tgt = struct.unpack_from("<I", scus, 0x80010EC0 - SCUS_LOAD + 4 * k)[0]
            if tgt == 0x800366B0:  # C1: character name
                idx = op if op != 0x63 else m.rd(0x800849AF - 0x86F + 0x457)
                t0 = idx * 0x414 + 0x800849AF
            elif tgt == 0x80036708:  # C2 / C4: item name pointer
                t0 = m.rd32(0x80074368 + op * 12 + 4)
            elif tgt == 0x800366E4:  # C3: spell name pointer
                t0 = m.rd32(0x800754C8 + op * 12 + 8)
            elif tgt == 0x8003672C:  # C5: table search at 0x80075EC4
                t0v, a3c = op, 0
                while t0v >= 0x40:
                    t0v -= 0x40
                    a3c += 1
                t1, v1p = 0, 0x80075EC4
                if not (a3c < m.rd(0x80075EC4)):
                    while True:
                        if a3c == m.rd(v1p) and m.rd(v1p + 1) == t0v:
                            break
                        v1p += 0x14
                        t1 += 1
                        if a3c < m.rd(v1p):
                            break
                        if t1 > 4096:
                            raise Fault("C5 table walk runs away")
                t0 = m.rd32(0x80075EC4 + t1 * 0x14 + 0xC)
            elif tgt == 0x800367BC:  # C7: inline 8-byte strings
                t0 = 0x80073F24 + op * 8
            # 0x800367D0 (C6): t0 unchanged
        a1 += 2
        # copy loop 0x800367D4..0x80036860 from t0
        while True:
            v = m.rd(t0)
            if v == 0:
                break
            if v == 0xC1:
                n = m.rd(t0 + 1)
                if n == 0x63:
                    n = m.rd(0x800849AF - 0x86F + 0x457)
                p = n * 0x414 + 0x800849AF
                t0 += 2
                c = m.rd(p)
                while c != 0:
                    m.wr(a0, c)
                    a0 += 1
                    written += 1
                    p += 1
                    c = m.rd(p)
                    if written > RUNAWAY:
                        raise Fault("runaway copy (nested name)")
            else:
                m.wr(a0, v)
                t0 += 1
                a0 += 1
                written += 1
            if written > RUNAWAY:
                raise Fault("runaway copy: read cursor %08x, write cursor %08x" % (t0, a0))
    m.wr(a0, 0)
    return written


def predict(prot, scus, ram, toc, entry, sectors, frames=4):
    if sectors < 1:
        return "SURVIVE", "no read"
    sectors = min(sectors, MAX_SECTORS)
    size = sectors * 0x800 - 4
    buf = BLOCK_END - size
    off = CAPTION - buf
    if off < 0 or off >= size:
        return "SURVIVE", "caption outside the read (sectors < 93)"
    m = Mem(ram)
    base = (toc[entry + 2] + 0x20) * 0x800
    data = prot[base:base + size]
    o = buf - RAM_BASE
    m.b[o:o + len(data)] = data
    first = m.rd(CAPTION)
    if first < 0x20:
        return "SURVIVE", "first byte %02x < 0x20: nothing drawn" % first
    try:
        for _ in range(frames):
            n = expand(m, CAPTION, scus)
    except Fault as e:
        return "FREEZE", str(e)
    return "SURVIVE", "expands to %d bytes" % n


def lzs(src, size):
    """Legaia LZS (docs/formats/lzs.md): 4 KB ring at 0xFEE, LSB-first flags."""
    ring = bytearray(0x1000)
    w, ctl, i, out = 0xFEE, 0, 0, bytearray()
    while len(out) < size:
        if not ctl & 0x100:
            ctl = src[i] | 0xFF00
            i += 1
        if ctl & 1:
            v = src[i]
            i += 1
            out.append(v)
            ring[w] = v
            w = (w + 1) & 0xFFF
        else:
            b0, b1 = src[i], src[i + 1]
            i += 2
            pos, n = b0 | ((b1 & 0xF0) << 4), (b1 & 0xF) + 3
            for k in range(n):
                if len(out) >= size:
                    break
                v = ring[(pos + k) & 0xFFF]
                out.append(v)
                ring[w] = v
                w = (w + 1) & 0xFFF
        ctl >>= 1
    return bytes(out)


def monster_costs(archive):
    """block[+0x08] per monster id from PROT 867 (0x14000-byte slots)."""
    costs = {}
    for mid in range(1, 256):
        slot = (mid - 1) * 0x14000
        if slot + 4 > len(archive):
            break
        n = struct.unpack_from("<I", archive, slot)[0]
        if not 0x100 <= n <= 0x14000 * 8:
            continue
        head = lzs(archive[slot + 4:slot + 4 + 64], 12)
        costs[mid] = struct.unpack_from("<I", head, 8)[0]
    return costs


# Fitted slack: one more freed 0x9C-byte node (plus its header) coalesces
# into the block on every captured fight; without it id 57 predicts 114
# sectors where the capture reads 115.
SLACK = 0xA8


def free_block(ids, costs):
    distinct = sorted(set(i for i in ids if i))
    return 0x2E390 + SLACK + sum(costs.get(i, 0) + 12 for i in distinct)


def sectors_for(ids, costs):
    return min(free_block(ids, costs) // 0x800, MAX_SECTORS)


def random_formations(asset_bin, bundle):
    """Formation rows the scene's random regions (rate > 0) can roll."""
    o = subprocess.run([asset_bin, "man", bundle, "--with-encounter",
                        "--max-formations", "999", "--max-regions", "999"],
                       capture_output=True, text=True).stdout
    rows = {}
    for m in re.finditer(r"^\s+\[\s*(\d+)\] count=(\d+) ids=\[([^\]]*)\]", o, re.M):
        ids = [int(x) for x in m.group(3).split(",") if x.strip()]
        rows[int(m.group(1))] = ids[:int(m.group(2))]
    want = set()
    for m in re.finditer(r"rate\+=(\d+) formations=\[(\d+)\.\.\+(\d+)\)", o):
        if int(m.group(1)) > 0:
            a, n = int(m.group(2)), int(m.group(3))
            want.update(range(a, a + n))
    return [(r, rows[r]) for r in sorted(want) if r in rows]


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--prot", required=True)
    ap.add_argument("--scus", required=True)
    ap.add_argument("--ram", required=True, help="2 MiB battle RAM image")
    ap.add_argument("--entry", type=int, help="scene's first PROT entry (CDNAME define)")
    ap.add_argument("--sectors", type=int)
    ap.add_argument("--sweep", action="store_true", help="every sectors value 93..197")
    ap.add_argument("--scenes", help="scenes.toml: run every field scene")
    ap.add_argument("--formations", action="store_true",
                    help="every random formation of each scene (needs --prot-dir, --asset-bin)")
    ap.add_argument("--prot-dir", help="directory of extracted PROT entries")
    ap.add_argument("--asset-bin", default="target/release/asset")
    a = ap.parse_args()
    prot = open(a.prot, "rb").read()
    scus = open(a.scus, "rb").read()
    ram = open(a.ram, "rb").read()
    toc = struct.unpack_from("<%dI" % 4096, prot, 0)
    jobs = []
    if a.scenes:
        txt = open(a.scenes).read()
        for blk in txt.split("[[scene]]")[1:]:
            mi = re.search(r'^id = "([^"]+)"', blk, re.M)
            mc = re.search(r"^cdname = (\d+)", blk, re.M)
            mk = re.search(r'^category = "([^"]+)"', blk, re.M)
            if mi and mc and (a.formations or not mk or mk.group(1) == "field"):
                jobs.append((mi.group(1), int(mc.group(1))))
    elif a.entry is not None:
        jobs.append(("entry%d" % a.entry, a.entry))
    else:
        sys.exit("give --entry or --scenes")
    if a.formations:
        arch = open(glob.glob(os.path.join(a.prot_dir, "0867_*.BIN"))[0], "rb").read()
        costs = monster_costs(arch)
        for name, entry in jobs:
            b = glob.glob(os.path.join(a.prot_dir, "%04d_*.BIN" % (entry + 1)))
            if not b:
                continue
            for row, ids in random_formations(a.asset_bin, b[0]):
                fb = free_block(ids, costs)
                sec = sectors_for(ids, costs)
                frac = fb % 0x800
                edge = ""
                if sec < MAX_SECTORS and (frac < 0x3C or frac >= 0x728):
                    alt = sec - 1 if frac < 0x3C else sec + 1
                    edge = " edge(%d:%s)" % (alt, predict(prot, scus, ram, toc, entry, alt)[0])
                r, why = predict(prot, scus, ram, toc, entry, sec)
                print("%-10s row %3d ids %-18s sectors %3d%-19s %s  (%s)" % (
                    name, row, ",".join(map(str, ids)), sec, edge, r, why))
        return
    for name, entry in jobs:
        if a.sweep:
            bad = [s for s in range(93, MAX_SECTORS + 1)
                   if predict(prot, scus, ram, toc, entry, s)[0] == "FREEZE"]
            print("%-10s entry %4d  freeze at %3d of %3d sector counts: %s" % (
                name, entry, len(bad), MAX_SECTORS - 92, compress(bad)))
        else:
            r, why = predict(prot, scus, ram, toc, entry, a.sectors)
            print("%-10s entry %4d sectors %3d  %s  (%s)" % (name, entry, a.sectors, r, why))


def compress(vals):
    out, i = [], 0
    while i < len(vals):
        j = i
        while j + 1 < len(vals) and vals[j + 1] == vals[j] + 1:
            j += 1
        out.append(str(vals[i]) if i == j else "%d-%d" % (vals[i], vals[j]))
        i = j + 1
    return ",".join(out) if out else "-"


if __name__ == "__main__":
    main()
