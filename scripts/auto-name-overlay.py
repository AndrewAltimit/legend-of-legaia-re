#!/usr/bin/env python3
"""
End-to-end: save state -> named overlay binary + stub Ghidra dump script.

Cuts the per-scene reverse cycle from "extract slice -> manually identify
which overlay -> hand-roll a dump_<label>.py with the right TARGETS" to a
single command.

What it does:
  1. Detects save-state format (mednafen MDFNSVST gz / Duckstation DUCCS
     zstd) from the magic bytes. PCSX-Redux .sstate files are gzip-wrapped
     but use a different binary layout - extract via the in-emulator Lua
     dump path instead (see scripts/pcsx-redux/autorun_dump_full_ram.lua).
  2. Extracts the overlay window 0x801C0000..0x80200000 (256 KiB) using
     the same anchor-string approach as scripts/extract-mednafen-overlay.py.
  3. Walks the slice for MIPS `addiu $sp, $sp, -N` prologues to enumerate
     function entry-point candidates.
  4. Cross-references against an anchor-function fingerprint table
     (curated from docs/tooling/overlay-capture.md) to pick a label.
  5. Emits:
        <out-dir>/overlay_<label>.bin                  (raw slice)
        ghidra/scripts/dump_<label>_overlay.py         (stub; preserved
                                                        if it already
                                                        exists unless
                                                        --force)
     The stub TARGETS list is seeded from the top-N candidates by
     estimated size (prologue-to-prologue distance).

Usage:
    scripts/auto-name-overlay.py SAVE
    scripts/auto-name-overlay.py SAVE --label custom_name
    scripts/auto-name-overlay.py SAVE --out-dir /tmp --force

Pass --label to override auto-detection (useful when you know which
overlay this save state captures and the fingerprint table doesn't).

See docs/tooling/overlay-capture.md for the broader workflow + per-overlay
context.
"""

import argparse
import gzip
import hashlib
import os
import struct
import subprocess
import sys
import tempfile
from typing import Dict, List, Optional, Tuple

PSX_RAM_SIZE = 2 * 1024 * 1024
PSX_RAM_KSEG0 = 0x80000000
SCUS_LOAD_ADDR = 0x80010000
PSX_EXE_HEADER = 0x800

OVERLAY_START = 0x801C0000
OVERLAY_END = 0x80200000  # exclusive; 256 KiB window

# Magic bytes identifying each container shape.
GZIP_MAGIC = b"\x1f\x8b"
ZSTD_MAGIC = b"\x28\xb5\x2f\xfd"
DUCCS_MAGIC = b"DUCCS"
MDFN_MAGIC = b"MDFNSVST"

# Anchor strings in SCUS_942.54 used to locate main RAM in the
# decompressed save state.
ANCHORS = [
    b"---- FIELD PROGRAM -----%d",
    b"PSX TEST PROGRAM",
    b"enter main loop",
    b"main free mem%d",
    b"h:\\prot\\cdname.dat",
]

# Anchor functions: (entry_address -> overlay label).
#
# Source: docs/tooling/overlay-capture.md "Capture status" table +
# the per-overlay dump script comments. Each entry is a function we know
# only exists in (or is canonical for) the named overlay binary. When
# multiple labels share an anchor, the match-counter wins by majority and
# warns on a tie.
#
# Add a new anchor whenever you confirm a function is exclusive to a
# specific overlay (e.g. through dump-script body comments that say
# "incoming=N" + an overlay-specific subsystem). The detector tolerates
# noise: false positives just lower the winning label's relative score.
ANCHOR_FUNCTIONS: Dict[int, str] = {
    # World map overlay (0x80108EA4 capture)
    0x801E76D4: "world_map",       # world_map_controller
    0x801EAD98: "world_map",       # dev menu renderer
    0x801E5B4C: "world_map",
    0x801E9F64: "world_map",
    0x801E3E00: "world_map",

    # Battle action overlay (0898)
    0x801E295C: "battle_action",   # per-actor state machine
    0x801D0748: "battle_action",   # battle main dispatcher

    # Save UI overlay
    0x801DC6B4: "save_ui",         # save-screen state machine
    0x801E4F40: "save_ui",         # PTR_FUN handler table region

    # Field / town / dialog overlay (0897)
    0x801DE840: "field",           # field/event VM (largest in corpus)
    0x801ED710: "field",           # MES renderer
    0x801D6704: "field",           # MAIN INIT
    0x801F5748: "field",           # inventory hub

    # Menu / pause-screens overlay (0896)
    0x801CF650: "menu",            # equipment stat aggregator

    # Shop / inn overlay - no single canonical anchor pinned in the
    # docs yet. Pass --label shop when capturing one until a
    # shop-exclusive function is identified.

    # Minigame hub overlay variants - all share a base; anchors below
    # are the per-variant primary entry points.
    0x801D63B0: "fishing",
    0x801D2CC0: "slot_machine",
    0x801D5ED0: "baka_fighter",
    0x801D2F38: "dance",

    # Muscle Dome / Baka card battle (distinct family from minigame hub)
    0x801D8DE8: "muscle_dome",     # round dispatcher
    0x801D5854: "muscle_dome",     # game SM
    0x801D388C: "muscle_dome",     # card resolution

    # Cutscene overlay
    # No single anchor pinned in the docs; left for future captures.
}


def parse_addr(s: str) -> int:
    return int(s, 16) if s.lower().startswith("0x") else int(s)


# ----------------------------------------------------------------------
# Save-state decompression


def decompress_save_state(path: str) -> Tuple[bytes, str]:
    """Detect format from magic bytes and return decompressed body + label.

    Recognises:
        gzip + MDFNSVST   -> "mednafen"     (mednafen .mc[0-9])
        DUCCS + zstd      -> "duckstation"  (Duckstation .sav)
    """
    with open(path, "rb") as fh:
        head = fh.read(16)

    if head.startswith(GZIP_MAGIC):
        with gzip.open(path, "rb") as fh:
            body = fh.read()
        if body.startswith(MDFN_MAGIC):
            return body, "mednafen"
        raise ValueError(
            f"{path}: gzip-wrapped but MDFNSVST magic not at offset 0 "
            f"(found {body[:8]!r}). Mednafen + PCSX-Redux both wrap with "
            "gzip, but their inner layouts differ; only mednafen is "
            "supported. Capture from PCSX-Redux via "
            "scripts/pcsx-redux/autorun_dump_full_ram.lua instead.")

    if head.startswith(DUCCS_MAGIC):
        # Duckstation: ASCII header up to a fixed offset, then zstd stream.
        with open(path, "rb") as fh:
            full = fh.read()
        zstd_off = full.find(ZSTD_MAGIC)
        if zstd_off < 0:
            raise ValueError(f"{path}: DUCCS header but no zstd magic found")
        compressed = full[zstd_off:]
        return _zstd_decompress(compressed), "duckstation"

    raise ValueError(
        f"{path}: unrecognised save-state format "
        f"(first 16 bytes: {head!r}). Expected gzip+MDFNSVST or DUCCS+zstd.")


def _zstd_decompress(compressed: bytes) -> bytes:
    """Decompress via the system `zstd` binary (avoids a python-zstd dep)."""
    with tempfile.NamedTemporaryFile(suffix=".bin", delete=False) as tf:
        tf.write(compressed)
        tmp_in = tf.name
    tmp_out = tmp_in + ".dec"
    try:
        subprocess.run(
            ["zstd", "-d", tmp_in, "-o", tmp_out, "--force", "-q"],
            check=True, capture_output=True)
        with open(tmp_out, "rb") as fh:
            return fh.read()
    finally:
        for p in (tmp_in, tmp_out):
            try:
                os.unlink(p)
            except OSError:
                pass


# ----------------------------------------------------------------------
# Slice extraction (anchor-find + slice). Mirrors the logic in
# extract-mednafen-overlay.py / extract-duckstation-overlay.py. Kept inline
# so this script is self-contained; if a third call site lands later, lift
# all three into a shared scripts/_overlay_extract.py module.


def find_ram_offset(state: bytes, scus: bytes) -> Optional[Tuple[int, bytes]]:
    """Locate the start of main RAM inside the decompressed state.

    Returns (offset, used_anchor) or None. Verifies via a second anchor
    when one is available; warns to stderr if the second anchor disagrees
    (would indicate non-contiguous RAM in the state file - the slice
    would be wrong).
    """
    found = None
    for anchor in ANCHORS:
        scus_off = scus.find(anchor)
        if scus_off < PSX_EXE_HEADER:
            continue
        state_off = state.find(anchor)
        if state_off < 0:
            continue
        ram_addr = SCUS_LOAD_ADDR + (scus_off - PSX_EXE_HEADER)
        phys = ram_addr - PSX_RAM_KSEG0
        offset = state_off - phys
        if found is None:
            found = (offset, anchor)
        elif offset != found[0]:
            print(f"warning: anchor {anchor!r} disagrees with first anchor "
                  f"(offset 0x{offset:X} vs 0x{found[0]:X}); main RAM may "
                  "be non-contiguous in the state file", file=sys.stderr)
    return found


def extract_overlay_slice(state: bytes, scus: bytes,
                          start: int, end: int) -> bytes:
    found = find_ram_offset(state, scus)
    if found is None:
        raise ValueError("no anchor string from SCUS_942.54 found in save state")
    ram_offset, _anchor = found

    if start < PSX_RAM_KSEG0 or end > PSX_RAM_KSEG0 + PSX_RAM_SIZE:
        raise ValueError(
            f"slice [0x{start:08X}..0x{end:08X}) outside main RAM")
    slice_start = ram_offset + (start - PSX_RAM_KSEG0)
    slice_end = ram_offset + (end - PSX_RAM_KSEG0)
    sliced = state[slice_start:slice_end]
    if len(sliced) != end - start:
        raise ValueError(
            f"short read ({len(sliced)} of {end - start} bytes); "
            f"main RAM may be non-contiguous in the state file")
    return sliced


# ----------------------------------------------------------------------
# Function-prologue scan
#
# MIPS `addiu $sp, $sp, -N` encoding:
#     opcode=001001 rs=11101 rt=11101 imm=imm16
#   = 0010 0111 1011 1101 IIII IIII IIII IIII
# Little-endian byte sequence: [imm_lo, imm_hi, 0xBD, 0x27]
# For -N where 0 < N <= 4096, the high byte of the two's-complement
# imm16 is in {0xFF, 0xFE, 0xFD, 0xFC} (covers N up to 1024 cleanly,
# 1024..4096 with a cap on common stack frames).


def scan_sp_prologues(blob: bytes, base_addr: int,
                      max_stack: int = 4096) -> List[int]:
    """Return sorted list of PSX virtual addresses that look like SP prologues."""
    high_byte_set = {0xFF, 0xFE, 0xFD, 0xFC}  # covers stacks <= 1024
    if max_stack > 1024:
        # Extend to cover larger stacks (e.g. world_map dispatcher's 19 KB
        # function with bigger frame). 0xF8 covers up to -2048; 0xF0 to -4096.
        high_byte_set |= {0xFB, 0xFA, 0xF9, 0xF8}
        if max_stack > 2048:
            high_byte_set |= {0xF7, 0xF6, 0xF5, 0xF4, 0xF3, 0xF2, 0xF1, 0xF0}

    out: List[int] = []
    n = len(blob)
    for i in range(0, n - 3, 4):
        # Little-endian: blob[i+3]=0x27, blob[i+2]=0xBD,
        # blob[i+1]=imm_hi, blob[i+0]=imm_lo
        if blob[i + 3] == 0x27 and blob[i + 2] == 0xBD and blob[i + 1] in high_byte_set:
            out.append(base_addr + i)
    return out


def estimate_func_sizes(prologues: List[int],
                        blob_end_addr: int) -> List[Tuple[int, int]]:
    """Pair each prologue with the gap to the next prologue (size estimate)."""
    out = []
    for i, addr in enumerate(prologues):
        next_addr = prologues[i + 1] if i + 1 < len(prologues) else blob_end_addr
        out.append((addr, next_addr - addr))
    return out


# ----------------------------------------------------------------------
# Label fingerprinting


def fingerprint(prologues: List[int]) -> Tuple[Optional[str], Dict[str, int]]:
    """Pick a label by counting anchor-function matches per label.

    Returns (winning_label_or_None, score_per_label).
    """
    p_set = set(prologues)
    scores: Dict[str, int] = {}
    for anchor_addr, label in ANCHOR_FUNCTIONS.items():
        if anchor_addr in p_set:
            scores[label] = scores.get(label, 0) + 1
    if not scores:
        return None, {}
    # Winning label: highest score; tie-break alphabetically (deterministic).
    max_score = max(scores.values())
    winners = sorted([l for l, s in scores.items() if s == max_score])
    if len(winners) > 1:
        return "_or_".join(winners), scores
    return winners[0], scores


# ----------------------------------------------------------------------
# Stub generator


STUB_TEMPLATE = '''# @category Legaia
# @runtime Jython
#
# Auto-generated stub by scripts/auto-name-overlay.py.
#
# Capture: {capture_summary}
#
# Source save state: {save_path}
# Detected label   : {label}
# Anchor scores    : {scores}
#
# This script dumps the top-{n} largest function entry-points in
# overlay_{label}.bin (estimated by SP-prologue spacing). Refine the
# TARGETS list after import - Ghidra's accurate function bounds and
# decompiled bodies will let you swap addresses for higher-signal ones.
#
# Run:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \\
#       /projects legaia -process overlay_{label}.bin -noanalysis \\
#       -postScript /scripts/dump_{label}_overlay.py
#
# Output filenames: ghidra/scripts/funcs/overlay_{label}_<addr>.txt

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
{targets_block}]

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
prog_name = prog.getName()
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
mem = prog.getMemory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
opts = DecompileOptions()
decomp.setOptions(opts)
decomp.openProgram(prog)


def out_path_for(addr_str):
    if prog_name.startswith("SCUS"):
        return os.path.join(OUT_DIR, addr_str + ".txt")
    label = prog_name.replace(".bin", "").replace(".", "_")
    return os.path.join(OUT_DIR, label + "_" + addr_str + ".txt")


def in_program(addr):
    return mem.getBlock(addr) is not None


def dump(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        print("[skip] {{}} not an address".format(addr_str))
        return
    if not in_program(addr):
        return
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is None:
        print("[skip] no function at {{}} in {{}}".format(addr_str, prog_name))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = out_path_for(addr_str)
    fh = open(out_path, "w")
    try:
        fh.write("== {{}} {{}} (entry={{}}) [{{}}] ==\\n".format(
            func.getName(), addr_str, func.getEntryPoint(), prog_name))
        fh.write("size={{}} bytes, {{}} instructions\\n\\n".format(
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\\n")
        for ins in instrs:
            fh.write("{{}}  {{}}\\n".format(ins.getAddress(), ins.toString()))
        fh.write("\\n--- DECOMPILED ---\\n")
        try:
            res = decomp.decompileFunction(func, 60, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: {{}})\\n".format(res.getErrorMessage()))
        except Exception as e:
            fh.write("(decompile exception: {{}})\\n".format(e))
    finally:
        fh.close()
    print("wrote {{}}".format(out_path))


for t in TARGETS:
    dump(t)

print("done [{{}}]".format(prog_name))
'''


def render_targets_block(sized: List[Tuple[int, int]]) -> str:
    lines = []
    for addr, size in sized:
        lines.append(f'    "{addr:08x}",  # ~{size} bytes')
    return "\n".join(lines) + "\n"


# ----------------------------------------------------------------------
# Driver


def main() -> int:
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))

    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("save", help="save state path (.mc[0-9] / .sstate / .sav)")
    ap.add_argument("--label",
                    help="override auto-detected overlay label")
    ap.add_argument("--scus", default=os.path.join(repo_root, "extracted", "SCUS_942.54"),
                    help="path to extracted SCUS_942.54 (default: extracted/SCUS_942.54)")
    ap.add_argument("--out-dir", default="/tmp",
                    help="where to write overlay_<label>.bin (default: /tmp)")
    ap.add_argument("--ghidra-scripts",
                    default=os.path.join(repo_root, "ghidra", "scripts"),
                    help="where to write dump_<label>_overlay.py "
                         "(default: ghidra/scripts/)")
    ap.add_argument("--start", type=parse_addr, default=OVERLAY_START)
    ap.add_argument("--end", type=parse_addr, default=OVERLAY_END)
    ap.add_argument("--top-n", type=int, default=20,
                    help="number of largest functions to seed in TARGETS "
                         "(default: 20)")
    ap.add_argument("--force", action="store_true",
                    help="overwrite existing dump_<label>_overlay.py")
    args = ap.parse_args()

    if not os.path.isfile(args.save):
        print(f"error: save state not found: {args.save}", file=sys.stderr)
        return 1
    if not os.path.isfile(args.scus):
        print(f"error: SCUS_942.54 not found at {args.scus}", file=sys.stderr)
        print("       pass --scus PATH to override", file=sys.stderr)
        return 1

    print(f"[info] reading {args.save}")
    state, fmt = decompress_save_state(args.save)
    print(f"[info] format: {fmt}; {len(state):,} bytes decompressed")

    scus = open(args.scus, "rb").read()
    sliced = extract_overlay_slice(state, scus, args.start, args.end)
    print(f"[info] sliced {len(sliced):,} bytes "
          f"(0x{args.start:08X}..0x{args.end:08X})")

    prologues = scan_sp_prologues(sliced, args.start)
    print(f"[info] {len(prologues)} SP-prologue candidates")

    if args.label:
        label = args.label
        scores: Dict[str, int] = {}
        print(f"[info] using user-supplied label: {label}")
    else:
        label, scores = fingerprint(prologues)
        if label is None:
            sha8 = hashlib.sha256(sliced).hexdigest()[:8]
            label = f"unknown_{sha8}"
            print(f"[warn] no anchor functions matched; "
                  f"falling back to label: {label}")
        else:
            score_str = ", ".join(f"{l}={s}" for l, s in
                                  sorted(scores.items(), key=lambda kv: (-kv[1], kv[0])))
            print(f"[info] auto-detected label: {label}  ({score_str})")

    # Emit the binary slice.
    bin_path = os.path.join(args.out_dir, f"overlay_{label}.bin")
    with open(bin_path, "wb") as fh:
        fh.write(sliced)
    print(f"[ok]   {bin_path}")

    # Emit the stub dump script if not present (or if --force).
    stub_path = os.path.join(args.ghidra_scripts, f"dump_{label}_overlay.py")
    if os.path.exists(stub_path) and not args.force:
        print(f"[skip] {stub_path} already exists "
              "(pass --force to overwrite)")
    else:
        sized = sorted(
            estimate_func_sizes(prologues, args.start + len(sliced)),
            key=lambda t: -t[1])[:args.top_n]
        capture_summary = (
            f"{len(prologues)} prologues; top-{args.top_n} by size; "
            f"window 0x{args.start:08X}..0x{args.end:08X}; "
            f"sha256[:8]={hashlib.sha256(sliced).hexdigest()[:8]}")
        score_repr = scores or "(label set by --label)"
        # Use basename only - the stub may get committed and we don't want
        # to bake the user's home-directory layout into the header comment.
        body = STUB_TEMPLATE.format(
            label=label,
            n=args.top_n,
            save_path=os.path.basename(args.save),
            scores=score_repr,
            capture_summary=capture_summary,
            targets_block=render_targets_block(sized))
        with open(stub_path, "w") as fh:
            fh.write(body)
        print(f"[ok]   {stub_path}")

    print()
    print("Next:")
    print(f"  scripts/import-overlay-named.sh {bin_path} {label}")
    print(f"  docker compose exec ghidra /ghidra/support/analyzeHeadless "
          f"/projects legaia \\")
    print(f"      -process overlay_{label}.bin -noanalysis "
          f"-postScript /scripts/dump_{label}_overlay.py")
    return 0


if __name__ == "__main__":
    sys.exit(main())
