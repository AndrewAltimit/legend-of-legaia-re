#!/usr/bin/env python3
"""CDNAME.TXT numbering-space analysis.

Tests which index space the `#define name N` numbers in CDNAME.TXT live in:
the extraction index space (entry N of `crates/prot`'s header-stripped TOC,
i.e. the `NNNN` in `extracted/PROT/NNNN_*.BIN`) or the retail in-RAM raw-TOC
space consumed by `FUN_8003E8A8` (raw index = extraction index + 2, because
the boot loader copies PROT.DAT verbatim - 8-byte header included - to
0x801C70F0; see docs/formats/prot.md "In-RAM TOC").

Reads only the user's own extraction (CDNAME.TXT + PROT.DAT); no Sony bytes
are embedded here - only TOC offsets, loader-constant indices, and format
magic values already published in docs/.

Evidence classes, in decreasing strength:
  1. Loader-constant identities: retail code passes hard-coded raw-TOC
     indices for dev-named files (PLAYER1..4, monster.snd, summon.dat,
     readef.DAT, the overlay slots). If the CDNAME define for the matching
     name block EQUALS the raw constant, the define space is the raw space.
  2. Format/magic checks at extraction entry N+shift for blocks whose names
     carry a checkable semantic expectation (vab_* -> VAB banks, music_* ->
     VAB/SEQ streams, move_program_no -> \\DATA\\MOV*.STR program table,
     other_game -> "OTHER<n>" overlay banners).
  3. Scene-region structural test: the per-scene v12 fixup table
     (docs/formats/scene-v12-table.md) recurs once per scene block. Because
     scene block lengths vary (7..11 slots), a wrong shift puts the v12
     entry at a VARIABLE offset from the shifted block start; the right
     alignment family puts it at a CONSTANT offset.

Usage:
  python3 scripts/asset-investigation/cdname_shift_analysis.py [--extracted DIR]

Exit status: 0 = analysis ran (read the verdict), 2 = inputs missing.
"""

import argparse
import struct
import sys
from pathlib import Path

SHIFTS = (0, -1, -2, -3)

# --------------------------------------------------------------------------
# Inputs
# --------------------------------------------------------------------------


def parse_cdname(path):
    """-> sorted list of (define_index, name). Mirrors crates/prot cdname.rs
    (later duplicate index wins, e.g. music_01 over music_test at 990)."""
    defines = {}
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line.startswith("#define"):
            continue
        parts = line.split()
        if len(parts) < 3:
            continue
        try:
            defines[int(parts[2])] = parts[1]
        except ValueError:
            continue
    return sorted(defines.items())


class Toc:
    """PROT.DAT TOC in both index spaces.

    File words: w[0..1] = header, raw entry i starts at LBA w[i+2];
    extraction entry p starts at LBA w[p+4]. So raw = extraction + 2.
    """

    def __init__(self, prot_path):
        self.f = open(prot_path, "rb")
        head = self.f.read(0x3000)
        self.words = struct.unpack_from("<%dI" % (len(head) // 4), head)
        # Walk until LBAs stop being monotonically increasing.
        self.n_ext = 0
        while (
            self.words[self.n_ext + 5] > self.words[self.n_ext + 4]
            and self.n_ext + 6 < len(self.words)
        ):
            self.n_ext += 1

    def ext_start(self, p):
        return self.words[p + 4] * 0x800

    def ext_footprint(self, p):
        return (self.words[p + 5] - self.words[p + 4]) * 0x800

    def raw_start(self, i):
        return self.words[i + 2] * 0x800

    def read_at(self, p, off, n):
        self.f.seek(self.ext_start(p) + off)
        return self.f.read(min(n, max(0, self.ext_footprint(p) - off)))


# --------------------------------------------------------------------------
# Light footprint-bounded classifier (the extracted .BINs over-read past the
# footprint via the indexed-size formula, which bleeds neighbours together -
# classify from PROT.DAT directly at the true entry start instead).
# --------------------------------------------------------------------------

SOUND_CLASSES = {"vab", "vab_stream", "vab_multi_bank", "seq"}


def classify(toc, p):
    head = toc.read_at(p, 0, 64)
    if len(head) < 16:
        return "tiny"
    if head.startswith(b"pochipochi"):
        return "pochi"
    if head[:4] == b"pQES":
        return "seq"
    if head[:4] == b"pBAV":
        return "vab"
    if head[4:8] == b"pBAV":
        return "vab_stream"  # [u32 chunk0][VABp...]
    w = struct.unpack_from("<4I", head)
    if w[0] == 0x10 and w[1] in (0, 8):
        return "tim"
    if w[0] == 0x80000002:
        return "tmd"
    if w[0] == 0x01059B84:
        return "field_pack"
    if w[0] == 0x02018B0C:
        return "effect_bundle"
    # Multi-bank VAB: [u32 0][u32 count][u32 sector_nums...] + pBAV in sec 0.
    if w[0] == 0 and 0 < w[1] <= 1024 and 0 < w[2] <= 0x4000:
        sec0 = toc.read_at(p, w[2] * 0x800, 8)
        if sec0[4:8] == b"pBAV":
            return "vab_multi_bank"
    h16 = struct.unpack_from("<8H", head)
    # Per-scene v12 fixup header: [N+4, 0x12, 0, 0x14, ?, N, 0, N+2] (u16s).
    if (
        h16[1] == 0x12
        and h16[2] == 0
        and h16[3] == 0x14
        and h16[6] == 0
        and h16[5] > 0
        and h16[0] == h16[5] + 4
        and h16[7] == h16[5] + 2
    ):
        return "scene_v12"
    if (w[0] & 0xFFFF8000) == 0x27BD8000:
        return "mips_code"  # addiu sp, sp, -X prologue
    if all(0x801C0000 <= x < 0x80200000 for x in struct.unpack_from("<8I", head)):
        return "overlay_ptrs"
    if all(b == 0 for b in head):
        return "zeros"
    printable = sum(1 for b in head if b == 0 or 0x20 <= b < 0x7F)
    if printable >= len(head) * 0.85:
        return "ascii_text"
    return "other"


def block_ranges(defines, n_ext, shift):
    """-> list of (name, ext_lo, ext_hi_exclusive) under `shift`
    (extraction entry = define + shift), clamped to the archive."""
    out = []
    for i, (idx, name) in enumerate(defines):
        lo = idx + shift
        hi = (defines[i + 1][0] + shift) if i + 1 < len(defines) else n_ext
        out.append((name, max(lo, 0), min(hi, n_ext)))
    return out


# --------------------------------------------------------------------------
# Evidence 1: loader-constant identities
# --------------------------------------------------------------------------

# (block name, define range, raw-TOC constants in retail code, provenance)
IDENTITY_ANCHORS = [
    (
        "battle_data",
        (865, 868),
        list(range(0x361, 0x365)),
        "PLAYER1..4 via FUN_800558FC(char+0x360) -> FUN_8003E8A8 "
        "(docs/reference/open-rev-eng-threads.md, live trace)",
    ),
    (
        "monster_se",
        (893, 893),
        [0x37D],
        "h:\\mpack\\monster.snd via FUN_8003E104 (li v0,0x37d; "
        "ghidra funcs/8003e104.txt)",
    ),
    (
        "bat_back_dat",
        (895, 896),
        [0x37F, 0x380],
        "summon.dat/readef.DAT via FUN_801F17F8 -> FUN_800558FC(.., 0x37f/0x380) "
        "(docs/formats/summon-readef.md, byte-verified RAM<->disc)",
    ),
    (
        "xxx_dat",
        (897, 899),
        [0x381, 0x382, 0x383],
        "overlay slot loaders FUN_8003EBE4/FUN_8003EC70: FUN_8003E8A8(param+0x381) "
        "(docs/formats/prot.md); param 2 = field overlay = extraction 0897 "
        "(crates/asset/data/static-overlays.toml)",
    ),
]

# Byte-pinned extraction-space content facts used to corroborate the
# arithmetic (offsets only; published in docs/).
PLAYER_FILE_OFFSETS = [0x36E8000, 0x3791000, 0x3828800, 0x3897800]
SUMMON_SLOT = 0x10800
SUMMON_SLOTS, READEF_SLOTS = 103, 78


def check_identities(toc, defines):
    dmap = dict(defines)
    print("== [1] Loader-constant identity anchors ==")
    ok = True
    for name, (dlo, dhi), raws, prov in IDENTITY_ANCHORS:
        match = dmap.get(dlo) == name and raws[0] == dlo and raws[-1] == dhi
        ok &= match
        print(
            "  %-13s defines %d..%d  retail constants 0x%X..0x%X (%d..%d)  %s"
            % (
                name,
                dlo,
                dhi,
                raws[0],
                raws[-1],
                raws[0],
                raws[-1],
                "IDENTICAL" if match else "MISMATCH",
            )
        )
        print("      %s" % prov)
    # Corroborate raw = extraction + 2 byte-arithmetic on the same TOC.
    p_ok = all(
        toc.ext_start(0x361 - 2 + i) == off
        for i, off in enumerate(PLAYER_FILE_OFFSETS)
    )
    s_ok = (
        toc.ext_footprint(893) == SUMMON_SLOTS * SUMMON_SLOT
        and toc.ext_footprint(894) == READEF_SLOTS * SUMMON_SLOT
    )
    m_cls = classify(toc, 0x37D - 2)
    print(
        "  raw->extraction-2 offset checks: player files at extraction 863..866 %s;"
        % ("PASS" if p_ok else "FAIL")
    )
    print(
        "    summon/readef slot footprints at extraction 893/894 %s; "
        "extraction 891 (raw 0x37D) classifies as %s"
        % ("PASS" if s_ok else "FAIL", m_cls)
    )
    return ok and p_ok and s_ok


# --------------------------------------------------------------------------
# Evidence 2: semantic expectations per shift
# --------------------------------------------------------------------------


def deep_contains(toc, p, needles, depth=0x10000):
    body = toc.read_at(p, 0, depth)
    return any(n in body for n in needles)


def semantic_scores(toc, defines, classes):
    """Per decidable block, per shift: fraction of entries matching the
    name's expectation (pochi fill counted as neutral/reserved)."""
    sound_blocks = [
        "sound_data",
        "sound_data2",
        "level_up",
        "monster_se",
        "music_01",
        "vab_01",
    ]
    rows = []
    for shift in SHIFTS:
        ranges = {n: (lo, hi) for n, lo, hi in block_ranges(defines, toc.n_ext, shift)}
        scores = {}
        for name in sound_blocks:
            lo, hi = ranges[name]
            ents = [classes[p] for p in range(lo, hi)]
            live = [c for c in ents if c != "pochi"]
            hit = sum(1 for c in live if c in SOUND_CLASSES)
            scores[name] = (hit, len(live))
        # move_program_no: a \DATA\MOV*.STR program/path table.
        lo, hi = ranges["move_program_no"]
        scores["move_program_no"] = (
            sum(1 for p in range(lo, hi) if deep_contains(toc, p, [b".STR;1"])),
            hi - lo,
        )
        # other_game: overlay banners "OTHER2" / "OTHER3" inside the block.
        lo, hi = ranges["other_game"]
        scores["other_game"] = (
            sum(
                1
                for p in range(lo, hi)
                if deep_contains(toc, p, [b"OTHER2 ", b"OTHER3 "], 0x40)
            ),
            hi - lo,
        )
        rows.append((shift, scores))

    names = sound_blocks + ["move_program_no", "other_game"]
    print("\n== [2] Semantic expectation match per shift ==")
    print("  (matched/decidable entries; pochi fill excluded as reserved)")
    print("  %-16s" % "block" + "".join("  shift %+d  " % s for s in SHIFTS))
    for name in names:
        cells = []
        for shift, scores in rows:
            hit, tot = scores[name]
            cells.append("%4d/%-4d " % (hit, tot))
        print("  %-16s" % name + "  ".join(cells))
    totals = []
    for shift, scores in rows:
        h = sum(v[0] for v in scores.values())
        t = sum(v[1] for v in scores.values())
        totals.append((shift, h, t))
    print(
        "  %-16s" % "TOTAL"
        + "  ".join("%4d/%-4d " % (h, t) for _, h, t in totals)
    )
    return totals


# --------------------------------------------------------------------------
# Evidence 3: scene-region v12 slot-position concentration
# --------------------------------------------------------------------------


def v12_concentration(toc, defines, classes):
    v12 = [p for p, c in classes.items() if c == "scene_v12"]
    # Scene-named region: first scene define (town01) .. last block before
    # battle_data.
    scene_lo = next(i for i, n in defines if n == "town01")
    scene_hi = next(i for i, n in defines if n == "battle_data")
    print(
        "\n== [3] Scene-region structural test: v12 table slot position =="
        "\n  %d v12 fixup tables detected; scene defines %d..%d"
        % (len(v12), scene_lo, scene_hi - 1)
    )
    results = []
    for shift in SHIFTS:
        starts = [i + shift for i, _ in defines if scene_lo <= i < scene_hi]
        offsets = {}
        in_scene = 0
        for p in v12:
            below = [s for s in starts if s <= p]
            if not below or p >= scene_hi + shift:
                continue
            in_scene += 1
            off = p - below[-1]
            offsets[off] = offsets.get(off, 0) + 1
        modal = max(offsets.items(), key=lambda kv: kv[1]) if offsets else (None, 0)
        results.append((shift, modal, in_scene, dict(sorted(offsets.items()))))
        print(
            "  shift %+d: modal slot %s in %d/%d scene-block v12s  histogram %s"
            % (shift, modal[0], modal[1], in_scene, dict(sorted(offsets.items())))
        )
    print(
        "  A correct alignment family puts the per-scene v12 at a CONSTANT"
        " slot; a wrong one\n  scatters it because scene block lengths vary."
        " (Slot constancy alone admits any shift\n  <= -1; the identity"
        " anchors in [1] pin the exact value.)"
    )
    return results


# --------------------------------------------------------------------------
# Evidence 4: class-transition alignment at tail-region block boundaries
# --------------------------------------------------------------------------


def boundary_transitions(toc, defines, classes):
    tail = [i for i, _ in defines if i >= 865]
    print(
        "\n== [4] Class transitions at non-scene block boundaries"
        " (defines >= 865) =="
    )
    out = []
    for shift in SHIFTS:
        n = 0
        for d in tail:
            p = d + shift
            if 1 <= p < toc.n_ext and classes[p] != classes[p - 1]:
                n += 1
        out.append((shift, n, len(tail)))
        print("  shift %+d: %d/%d boundaries land on a class change" % (shift, n, len(tail)))
    return out


# --------------------------------------------------------------------------


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument(
        "--extracted",
        type=Path,
        default=Path(__file__).resolve().parent.parent.parent / "extracted",
        help="extraction root containing CDNAME.TXT and PROT.DAT",
    )
    args = ap.parse_args()
    cdname = args.extracted / "CDNAME.TXT"
    prot = args.extracted / "PROT.DAT"
    if not cdname.is_file() or not prot.is_file():
        print("missing %s or %s - run the extractor first" % (cdname, prot))
        return 2

    defines = parse_cdname(cdname)
    toc = Toc(prot)
    print(
        "CDNAME defines: %d   extraction entries: %d   raw-TOC entries: %d"
        % (len(defines), toc.n_ext, toc.n_ext + 2)
    )
    classes = {p: classify(toc, p) for p in range(toc.n_ext)}

    ids_ok = check_identities(toc, defines)
    sem = semantic_scores(toc, defines, classes)
    v12 = v12_concentration(toc, defines, classes)
    boundary_transitions(toc, defines, classes)

    best_sem = max(sem, key=lambda t: t[1] / max(t[2], 1))
    const_shifts = [s for s, modal, n, hist in v12 if len(hist) == 1]
    print("\n== Verdict ==")
    print(
        "  identity anchors: %s; best semantic shift: %+d (%d/%d);"
        " v12-constant shifts: %s"
        % (
            "ALL IDENTICAL" if ids_ok else "MISMATCH (see [1])",
            best_sem[0],
            best_sem[1],
            best_sem[2],
            const_shifts,
        )
    )
    if ids_ok and best_sem[0] == -2 and -2 in const_shifts and 0 not in const_shifts:
        print(
            "  CONFIRMED: CDNAME #define numbers are raw in-RAM TOC indices"
            " (FUN_8003E8A8 space).\n  Extraction entry for define N is N-2;"
            " extraction filenames inherit names shifted by +2."
        )
    else:
        print("  NOT confirmed as uniform -2; inspect the section reports above.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
