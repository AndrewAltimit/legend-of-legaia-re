#!/usr/bin/env python3
"""
Disassemble a MIPS function out of a raw PSX RAM dump or SCUS_942.54
binary, walking instructions until the first `jr ra; nop` exit pair.

Useful as a no-Ghidra shortcut for first-pass reads of overlay-resident
functions. Capstone handles regular MIPS R3000 instructions; PSX GTE
(COP2) ops are annotated from a small lookup table since capstone bails
on COP2 dispatch.

Typical usage:

  # Eight overlay leaves dispatched by FUN_80043390's overlay path.
  scripts/ghidra-analysis/disasm-overlay-fn.py /tmp/overlay_world_map_top_ext.bin \\
      --base 0x801C0000 --addr 0x801F7644

  # SCUS sibling renderer for slot 12 (alpha row 0).
  scripts/ghidra-analysis/disasm-overlay-fn.py extracted/SCUS_942.54 \\
      --base 0x80010000 --header 0x800 --addr 0x80043658

  # Dump all eight overlay leaves in one go, paired with their SCUS
  # siblings, into /tmp/leaves/.
  scripts/ghidra-analysis/disasm-overlay-fn.py /tmp/overlay_world_map_top_ext.bin \\
      --base 0x801C0000 --batch leaves --out-dir /tmp/leaves
"""

import argparse
import os
import sys
from typing import Optional

try:
    from capstone import Cs, CS_ARCH_MIPS, CS_MODE_MIPS32, CS_MODE_LITTLE_ENDIAN
except ImportError:
    print("error: capstone not installed (`pip install capstone`)", file=sys.stderr)
    sys.exit(1)


# PSX GTE (COP2) instruction annotation. Capstone returns a bare "cop2" with
# the raw immediate and doesn't name the GTE function, so we annotate from the
# shared table in scripts/ghidra-analysis/mips_gte.py.
from mips_gte import annotate_cop2  # noqa: E402,F401


def virt_to_file(addr: int, base: int, header: int) -> int:
    """Translate a PSX virtual address to a file offset in the input.

    The PSX-EXE on disc has an 0x800-byte header before the loaded body,
    so SCUS_942.54 needs --header 0x800 to put 0x80010000 at file 0x800.
    Raw RAM dumps (overlay extracts) typically have no header.
    """
    return header + (addr - base)


def find_function_end(data: bytes, file_off: int, fn_start_addr: int,
                      base: int, header: int) -> int:
    """Walk forward from `file_off` until a function exit.

    Recognised exits:
      * `jr ra` with a delay slot. Returns one past the delay slot.
      * Unconditional `j <target>` whose target lands more than 0x1000
        bytes from the current function start AND outside the range
        we've already walked. The classic Legaia overlay-leaf tail call
        `j 0x80043580` (back to SCUS continuation) matches this.

    Bounded to avoid runaway when a function lacks any recognisable exit.
    """
    MAX_INSTRS = 8192
    JR_RA = b"\x08\x00\xe0\x03"
    for i in range(MAX_INSTRS):
        off = file_off + i * 4
        if off + 8 > len(data):
            return min(off, len(data))
        word = data[off:off + 4]
        # jr $ra (any delay slot).
        if word == JR_RA:
            return off + 8
        # Unconditional `j <target26>`. Opcode 0x02 in the top 6 bits.
        raw = int.from_bytes(word, "little", signed=False)
        if (raw >> 26) == 0x02:
            cur_pc = fn_start_addr + i * 4
            target = ((cur_pc + 4) & 0xF000_0000) | ((raw & 0x03FF_FFFF) << 2)
            # A `j` is a function exit when its target sits outside the
            # range we've walked so far - either backward past the function
            # entry, or forward past a small lookahead (a forward `j`
            # inside that lookahead might be a tail-of-loop into a basic
            # block we haven't reached yet, so don't terminate on it).
            lookahead = 0x100  # 64 instructions
            if target < fn_start_addr or target > cur_pc + lookahead:
                return off + 8  # consume the delay slot too
    # No exit within bound. Return file_off + cap so the caller at least
    # dumps what we walked.
    return file_off + MAX_INSTRS * 4


def disasm_function(data: bytes, addr: int, base: int, header: int,
                    max_size: Optional[int] = None) -> str:
    """Disassemble one function. Returns the full output as a string."""
    file_off = virt_to_file(addr, base, header)
    if file_off < 0 or file_off >= len(data):
        return f"; ERROR: address 0x{addr:08X} outside input data " \
               f"(file_off 0x{file_off:X}, data size 0x{len(data):X})\n"
    end_off = find_function_end(data, file_off, addr, base, header)
    if max_size is not None:
        end_off = min(end_off, file_off + max_size)
    body = data[file_off:end_off]

    md = Cs(CS_ARCH_MIPS, CS_MODE_MIPS32 | CS_MODE_LITTLE_ENDIAN)
    md.detail = True
    md.skipdata = True

    lines = []
    lines.append(f"; function @ 0x{addr:08X}  ({end_off - file_off} bytes, "
                 f"{(end_off - file_off) // 4} instructions)")
    lines.append(f"; input: {len(body)} bytes from file offset 0x{file_off:X}")
    lines.append("")

    cur_addr = addr
    consumed = 0
    for ins in md.disasm(body, addr):
        # Manually annotate COP2.
        raw_word = int.from_bytes(
            body[consumed:consumed + 4], "little", signed=False) if consumed + 4 <= len(body) else 0
        ann = ""
        if ins.mnemonic.startswith("cop2") or (raw_word >> 26) == 0x12:
            gte = annotate_cop2(raw_word)
            if gte:
                ann = f"  ; GTE.{gte}"
        lines.append(f"  {ins.address:08X}  {ins.mnemonic:<8} {ins.op_str}{ann}")
        cur_addr = ins.address + ins.size
        consumed += ins.size
        if cur_addr >= addr + len(body):
            break

    return "\n".join(lines) + "\n"


# Curated batches so the same script can drive Track A's leaf inventory
# without callers having to remember addresses.
BATCHES = {
    # Eight overlay-resident high-mode renderers swapped in by the
    # world-map top-view via FUN_80043390's overlay path. Addresses come
    # from `mednafen-state prim-dispatch-table --overlay-targets-only`
    # against a world-map-loaded save.
    "leaves": [
        ("overlay_leaf_12", 0x801F7644),
        ("overlay_leaf_13", 0x801F7838),
        ("overlay_leaf_16", 0x801F7AA4),
        ("overlay_leaf_17", 0x801F7CCC),
        ("overlay_leaf_14", 0x801F7F78),
        ("overlay_leaf_15", 0x801F8198),
        ("overlay_leaf_18", 0x801F8454),
        ("overlay_leaf_19", 0x801F8690),
    ],
    # SCUS-resident sibling renderers in slots 12..19 of alpha row 0
    # (`SCUS_TABLE_BASE = 0x8007657C`). Read off the dispatch-table
    # printout.
    "scus-siblings": [
        ("scus_slot12", 0x80043658),
        ("scus_slot13", 0x80043768),
        ("scus_slot14", 0x80043B58),
        ("scus_slot15", 0x80043C6C),
        ("scus_slot16", 0x800438B8),
        ("scus_slot17", 0x800439E4),
        ("scus_slot18", 0x80043DD4),
        ("scus_slot19", 0x80043F10),
    ],
}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("input", help="Raw RAM dump or SCUS_942.54 binary")
    ap.add_argument("--base", type=lambda s: int(s, 0), required=True,
                    help="PSX virtual address of input byte 0 (after --header)")
    ap.add_argument("--header", type=lambda s: int(s, 0), default=0,
                    help="File header to skip before --base (SCUS_942.54: 0x800)")
    ap.add_argument("--addr", type=lambda s: int(s, 0),
                    help="PSX virtual address of function to disassemble")
    ap.add_argument("--max-size", type=lambda s: int(s, 0), default=None,
                    help="Cap disassembly at this many bytes (debugging)")
    ap.add_argument("--batch", choices=sorted(BATCHES.keys()),
                    help="Disassemble a curated set instead of a single --addr")
    ap.add_argument("--out-dir", default=None,
                    help="With --batch: write each function to <out-dir>/<name>.txt")
    args = ap.parse_args()

    with open(args.input, "rb") as fh:
        data = fh.read()

    if args.batch is not None:
        targets = BATCHES[args.batch]
        out_dir = args.out_dir or f"/tmp/{args.batch}"
        os.makedirs(out_dir, exist_ok=True)
        for name, addr in targets:
            text = disasm_function(data, addr, args.base, args.header, args.max_size)
            out_path = os.path.join(out_dir, f"{name}_0x{addr:08X}.txt")
            with open(out_path, "w") as fh:
                fh.write(f"; source: {args.input}\n")
                fh.write(f"; base 0x{args.base:08X} header 0x{args.header:X}\n")
                fh.write(text)
            print(f"wrote {out_path}")
        return 0

    if args.addr is None:
        print("error: pass --addr or --batch", file=sys.stderr)
        return 2

    sys.stdout.write(disasm_function(data, args.addr, args.base, args.header, args.max_size))
    return 0


if __name__ == "__main__":
    sys.exit(main())
