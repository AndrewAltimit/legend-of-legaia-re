#!/usr/bin/env python3
"""Find every reference to a global reached through `$gp`, through a
`lui`+load/store pair, or through a materialised base plus a displacement.

`find-address-word-refs.py` answers "who references this address?" in five
forms - literal word, `lui`+`addiu`/`ori`, `jal`, `j`, PC-relative branch.
Three forms are missing from that list, and a global in the small-data band
or in the scratchpad is invisible in all of them:

  * **`disp($gp)`** - the whole point of the small-data pointer is that the
    address never appears in the instruction stream. `sw a0,0x678(gp)` and
    `lw v0,0x678(gp)` carry the displacement only; no scan for the absolute
    address can see either.
  * **`lui rX, hi` + `lw rY, lo(rX)`** - the direct-global form, where the
    low half rides the *load* rather than a separate `addiu`. This is the
    commonest way retail touches a named global, and the sibling tool skips
    it: its pair scan accepts only `addiu` (op `0x09`) and `ori` (op `0x0D`)
    as the second half.
  * **`lui rX, hi` + `ori/addiu rX` + `<mem> rY, disp(rX)`** - the same thing
    with the low half *split* between the register and the operand, so no
    instruction carries either the address or its low half. Every scratchpad
    access retail makes has this shape: `lui a0,0x1f80; ori a0,a0,0x314;
    sb v0,0xd4(a0)` writes `0x1F8003E8`. `--no-base-disp` turns it off.

The blind spots compose, and each has already hidden a real answer behind a
five-form negative. `gp[0x678]` (the battle sound bank's record table) has
one gp-relative writer and seven `lui`+`lw` readers, and a five-form sweep of
its absolute address `0x8007B990` reports "no word, no jump, no branch, no
materialisation pair - in any image". `0x1F8003E8` (the camera zone loader's
scratchpad window) has four readers and six writers, and reports the same.
So does `0x800846DC`, the field run-button mask, whose one writer and one
reader both reach it as `0x80084140 + disp`.

    scripts/ghidra-analysis/find-gp-relative-refs.py 0x678
    scripts/ghidra-analysis/find-gp-relative-refs.py --va 0x8007b990
    scripts/ghidra-analysis/find-gp-relative-refs.py 0x678 --dumps
    scripts/ghidra-analysis/find-gp-relative-refs.py 0x5b8 --prot
    scripts/ghidra-analysis/find-gp-relative-refs.py --va 0x1f8003e8
    scripts/ghidra-analysis/find-gp-relative-refs.py --find-gp

## What `$gp` is, and why it must be recovered rather than assumed

`$gp` is set once by the runtime's own `lui gp` / `addiu gp` pair and never
reloaded, so every displacement in the image is relative to one constant.
`--find-gp` recovers it by decoding that pair out of `SCUS_942.54` (retail
Legaia: `0x8007B318`, from `lui gp,0x8008; addiu gp,gp,-0x4ce8` at
`0x80026CA8`). The default below is that value; pass `--gp` for another
build. A displacement scan needs no `gp` at all - only the `--va` form and
the `lui` pair form consume it.

## What a hit means per image

Same three cases the sibling tool keeps apart, for the same reason:

  * **SCUS** - one fixed base, so a file offset maps to exactly one VA.
  * **Overlay images** - the committed base map
    (`crates/asset/data/static-overlays.toml`). Slot-A overlays share a
    base, so the *image* is as much of the answer as the offset is.
  * **Other PROT entries** (`--prot`) - streamed data with no load base, so
    hits are reported by file offset only. Unlike an absolute-word scan, a
    gp-relative *displacement* scan is still meaningful there: the encoding
    carries no address, so it is base-independent by construction.

A raw byte scan over data will produce coincidences. Read the disassembly at
each hit (`disasm-overlay-fn.py`) before calling it a reference - the
`code` column is a hint, not a verdict.
"""

from __future__ import annotations

import argparse
import glob
import re
import struct
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
OVERLAY_MAP = REPO / "crates" / "asset" / "data" / "static-overlays.toml"
OVERLAY_DIR = REPO / "extracted" / "overlays"
PROT_DIR = REPO / "extracted" / "PROT"
SCUS = REPO / "extracted" / "SCUS_942.54"
FUNCS_DIR = REPO / "ghidra" / "scripts" / "funcs"

# Retail Legaia `$gp`, from the `lui gp,0x8008; addiu gp,gp,-0x4ce8` pair at
# 0x80026CA8 in SCUS_942.54. `--find-gp` re-derives it from the bytes.
DEFAULT_GP = 0x8007B318

GP_REG = 28

# I-type memory opcodes, by primary opcode. `lwc2`/`swc2` are the GTE
# transfer pair - a gp-relative GTE load is rare but legal.
MEM_OPS = {
    0x20: "lb",
    0x21: "lh",
    0x22: "lwl",
    0x23: "lw",
    0x24: "lbu",
    0x25: "lhu",
    0x26: "lwr",
    0x28: "sb",
    0x29: "sh",
    0x2A: "swl",
    0x2B: "sw",
    0x2E: "swr",
    0x32: "lwc2",
    0x3A: "swc2",
}

# Address-forming opcodes: `addiu rt, gp, disp` / `ori rt, gp, disp` hand the
# address itself to a later load, one hop away from the displacement.
ADDR_OPS = {0x09: "addiu", 0x0D: "ori"}

REG_NAMES = (
    "zero at v0 v1 a0 a1 a2 a3 t0 t1 t2 t3 t4 t5 t6 t7 "
    "s0 s1 s2 s3 s4 s5 s6 s7 t8 t9 k0 k1 gp sp s8 ra"
).split()

# How many instructions after a `lui` its load/store partner may sit.
LUI_PAIR_WINDOW = 8
# How far the base-plus-displacement walk carries a materialised register.
BASE_DISP_WINDOW = 24
# How far either side of a hit to tally code markers.
CODE_WINDOW = 0x200


class Image:
    """One scanned blob: bytes, an identity, and possibly a load base."""

    def __init__(self, name: str, data: bytes, base: int | None, kind: str):
        self.name = name
        self.data = data
        self.base = base
        self.kind = kind

    def word(self, off: int) -> int | None:
        if off < 0 or off + 4 > len(self.data):
            return None
        return struct.unpack_from("<I", self.data, off)[0]

    def where(self, off: int) -> str:
        if self.base is None:
            return f"{self.name} +0x{off:06x}"
        return f"{self.name} +0x{off:06x} (VA 0x{self.base + off:08x})"

    def code_markers(self, off: int) -> int:
        """`jr ra` / `addiu sp, sp, N` words within +-CODE_WINDOW."""
        lo = max(0, (off - CODE_WINDOW) & ~3)
        hi = min(len(self.data) - 3, off + CODE_WINDOW)
        n = 0
        for at in range(lo, hi, 4):
            word = self.word(at)
            if word == 0x03E00008 or (word is not None and (word >> 16) == 0x27BD):
                n += 1
        return n


def load_scus() -> Image | None:
    """`SCUS_942.54` at its PS-X EXE load base, header trimmed."""
    if not SCUS.exists():
        return None
    raw = SCUS.read_bytes()
    if raw[:8] != b"PS-X EXE":
        sys.exit(f"{SCUS} is not a PS-X EXE")
    t_addr, t_size = struct.unpack_from("<II", raw, 0x18)
    return Image("SCUS_942.54", raw[0x800 : 0x800 + t_size], t_addr, "exe")


def load_overlays() -> list[Image]:
    """Every extracted overlay image the committed map gives a base."""
    if not OVERLAY_MAP.exists():
        return []
    rows = re.findall(
        r'prot_index = (\d+)\s*\nlabel = "([^"]+)"\s*\nbase_va = (0x[0-9A-Fa-f]+)',
        OVERLAY_MAP.read_text(),
    )
    out = []
    for prot, label, base in rows:
        path = OVERLAY_DIR / f"overlay_{label}_{int(prot):04d}.bin"
        if path.exists():
            out.append(
                Image(f"{int(prot):04d}/{label}", path.read_bytes(), int(base, 16), "overlay")
            )
    return out


def load_prot_entries(skip: set[str]) -> list[Image]:
    """Every other extracted PROT entry, base-less."""
    out = []
    for path in sorted(glob.glob(str(PROT_DIR / "*.BIN"))):
        stem = Path(path).name
        if stem[:4] in skip:
            continue
        out.append(Image(stem, Path(path).read_bytes(), None, "prot"))
    return out


def recover_gp(scus: Image) -> int | None:
    """Decode the runtime's own `lui gp, hi` / `addiu gp, gp, lo` pair.

    `$gp` is written once and never reloaded, so the first such pair in the
    image is the build's small-data pointer.
    """
    for off in range(0, len(scus.data) - 7, 4):
        word = scus.word(off)
        nxt = scus.word(off + 4)
        if word is None or nxt is None:
            continue
        if (word >> 26) != 0x0F or ((word >> 16) & 0x1F) != GP_REG:
            continue
        if (nxt >> 26) != 0x09:
            continue
        if ((nxt >> 21) & 0x1F) != GP_REG or ((nxt >> 16) & 0x1F) != GP_REG:
            continue
        lo = nxt & 0xFFFF
        if lo & 0x8000:
            lo -= 0x10000
        return (((word & 0xFFFF) << 16) + lo) & 0xFFFFFFFF
    return None


def scan_gp_relative(image: Image, disps: set[int]) -> list[tuple[int, str]]:
    """`<mem> rt, disp(gp)` and `addiu/ori rt, gp, disp` sites."""
    out = []
    for off in range(0, len(image.data) - 3, 4):
        word = struct.unpack_from("<I", image.data, off)[0]
        op = word >> 26
        if op not in MEM_OPS and op not in ADDR_OPS:
            continue
        if ((word >> 21) & 0x1F) != GP_REG:
            continue
        imm = word & 0xFFFF
        if imm & 0x8000:
            imm -= 0x10000
        if imm not in disps:
            continue
        rt = REG_NAMES[(word >> 16) & 0x1F]
        if op in MEM_OPS:
            out.append((off, f"{MEM_OPS[op]} {rt},0x{imm:x}(gp)"))
        else:
            out.append((off, f"{ADDR_OPS[op]} {rt},gp,0x{imm:x}"))
    return out


def scan_lui_mem(image: Image, target: int) -> list[tuple[int, int, str]]:
    """`lui rX, hi` + `<mem> rY, lo(rX)` - the direct-global form.

    Returns `(lui_offset, memop_offset, text)`. The high half carries the
    assembler's sign correction, exactly as for a `lui`+`addiu` pair.
    """
    lo = target & 0xFFFF
    hi = ((target >> 16) + (1 if lo >= 0x8000 else 0)) & 0xFFFF
    signed_lo = lo - 0x10000 if lo >= 0x8000 else lo
    out = []
    for off in range(0, len(image.data) - 3, 4):
        word = struct.unpack_from("<I", image.data, off)[0]
        if (word >> 26) != 0x0F or (word & 0xFFFF) != hi:
            continue
        reg = (word >> 16) & 0x1F
        for k in range(1, LUI_PAIR_WINDOW + 1):
            at = off + 4 * k
            nxt = image.word(at)
            if nxt is None:
                break
            op = nxt >> 26
            if op == 0x0F and ((nxt >> 16) & 0x1F) == reg:
                break  # the register is reloaded; the pair cannot span this
            if ((nxt >> 21) & 0x1F) != reg or (nxt & 0xFFFF) != lo:
                continue
            if op in MEM_OPS:
                rt = REG_NAMES[(nxt >> 16) & 0x1F]
                base = REG_NAMES[reg]
                out.append((off, at, f"{MEM_OPS[op]} {rt},{signed_lo:#x}({base})"))
                break
    return out


def _writes_reg(word: int) -> int | None:
    """Which GPR an instruction clobbers, for the register walk below.

    Only the forms that matter to a base-register walk are decoded exactly;
    anything else that plausibly writes a register returns its destination so
    the walk drops the base rather than trusting a stale value.
    """
    op = word >> 26
    rt = (word >> 16) & 0x1F
    if op == 0x00:  # SPECIAL
        funct = word & 0x3F
        if funct in (0x08, 0x0C, 0x0D):  # jr, syscall, break
            return None
        if funct in (0x18, 0x19, 0x1A, 0x1B, 0x11, 0x13):  # mult/div, mthi/mtlo
            return None
        return (word >> 11) & 0x1F  # rd
    if op == 0x01:  # REGIMM - the `*al` forms link
        return 31 if (rt & 0x1E) == 0x10 else None
    if op == 0x03:  # jal
        return 31
    if 0x08 <= op <= 0x0F:  # addi/addiu/slti/sltiu/andi/ori/xori/lui
        return rt
    if op in (0x10, 0x11, 0x12, 0x13):  # coprocessor: mfc/cfc write rt
        return rt if ((word >> 21) & 0x1F) in (0x00, 0x02) else None
    if 0x20 <= op <= 0x26:  # loads
        return rt
    return None


def scan_base_disp(image: Image, target: int) -> list[tuple[int, str]]:
    """`lui rX, hi` [`ori`/`addiu` rX] + `<mem> rY, disp(rX)` reaching `target`.

    The generalisation of the `lui`+load pair: the low half of the address is
    split between the register-forming instructions and the memory operand's
    own displacement, so neither half equals the target's low half. Retail
    forms every scratchpad access this way - `lui a0,0x1f80; ori a0,a0,0x314;
    sb v0,0xd4(a0)` writes `0x1F8003E8` while carrying neither `0x3e8` nor
    the whole address anywhere in the instruction stream.

    A register that *holds* the target is reported too, which closes the
    sibling tool's documented "split materialisation" gap - an address built
    in more than two steps is not a `lui`+`addiu` pair and that scan walks
    past it.

    Returns `(memop_offset, text)`. The walk is linear and stops a register at
    its next writer, so a hit inside a branch shadow is a candidate to read,
    not a proof; `code` and the disassembly settle it.
    """
    out = []
    seen = set()
    for off in range(0, len(image.data) - 3, 4):
        word = struct.unpack_from("<I", image.data, off)[0]
        if (word >> 26) != 0x0F:
            continue
        reg = (word >> 16) & 0x1F
        if reg == 0:
            continue
        regs = {reg: ((word & 0xFFFF) << 16) & 0xFFFFFFFF}
        for k in range(1, BASE_DISP_WINDOW + 1):
            at = off + 4 * k
            nxt = image.word(at)
            if nxt is None:
                break
            op = nxt >> 26
            rs = (nxt >> 21) & 0x1F
            rt = (nxt >> 16) & 0x1F
            imm = nxt & 0xFFFF
            simm = imm - 0x10000 if imm & 0x8000 else imm
            if op in MEM_OPS and rs in regs:
                if (regs[rs] + simm) & 0xFFFFFFFF == target and at not in seen:
                    seen.add(at)
                    out.append(
                        (
                            at,
                            "%s %s,%#x(%s)  [base 0x%08x from lui @ +0x%x]"
                            % (
                                MEM_OPS[op],
                                REG_NAMES[rt],
                                simm,
                                REG_NAMES[rs],
                                regs[rs],
                                off,
                            ),
                        )
                    )
            if op in ADDR_OPS and rs in regs:
                val = ((regs[rs] | imm) if op == 0x0D else (regs[rs] + simm)) & 0xFFFFFFFF
                regs[rt] = val
                # The address itself in a register: the two-step form the
                # sibling tool already reports, and the multi-step form it
                # explicitly cannot ("split materialisation").
                if val == target and at not in seen:
                    seen.add(at)
                    out.append(
                        (
                            at,
                            "%s %s,%s,%#x  [= 0x%08x, from lui @ +0x%x]"
                            % (
                                ADDR_OPS[op],
                                REG_NAMES[rt],
                                REG_NAMES[rs],
                                imm,
                                val,
                                off,
                            ),
                        )
                    )
                continue
            dest = _writes_reg(nxt)
            if dest is not None:
                regs.pop(dest, None)
            if not regs:
                break
    return out


def scan_dumps(disps: set[int]) -> list[tuple[str, str]]:
    """`grep -E '[-]?0x<disp>\\(gp\\)'` over the committed dump corpus.

    A negative displacement prints the way the disassembler renders it
    (`-0x4730(at)` style), so both signs are matched.
    """
    out = []
    if not FUNCS_DIR.is_dir() or not disps:
        return out
    forms = sorted({("-" if d < 0 else "") + f"0x{abs(d):x}" for d in disps})
    pattern = re.compile("(?:%s)\\(gp\\)" % "|".join(re.escape(f) for f in forms))
    for path in sorted(FUNCS_DIR.glob("*.txt")):
        try:
            text = path.read_text(errors="replace")
        except OSError:
            continue
        for line in text.splitlines():
            if pattern.search(line):
                out.append((path.name, line.strip()))
    return out


def scan_dumps_abs(target: int) -> list[tuple[str, str]]:
    """Grep the dump corpus for the target's bare hex, either case.

    A `lui`/`ori` base plus a displacement never prints the whole address in
    the disassembly, but Ghidra's decompiler folds it back into a `DAT_`
    symbol - so the C half of a dump names the address the assembly does not.
    """
    out = []
    if not FUNCS_DIR.is_dir():
        return out
    pattern = re.compile("%08x" % target, re.IGNORECASE)
    for path in sorted(FUNCS_DIR.glob("*.txt")):
        try:
            text = path.read_text(errors="replace")
        except OSError:
            continue
        for line in text.splitlines():
            if pattern.search(line):
                out.append((path.name, line.strip()))
    return out


def main() -> int:
    ap = argparse.ArgumentParser(
        description=(
            "Find references to a global reached through $gp, a lui+load pair, "
            "or a materialised base plus a displacement."
        )
    )
    ap.add_argument("disps", nargs="*", help="gp displacements, e.g. 0x678")
    ap.add_argument("--va", action="append", default=[], help="absolute VA instead of a displacement")
    ap.add_argument("--gp", default=hex(DEFAULT_GP), help=f"$gp value (default {DEFAULT_GP:#x})")
    ap.add_argument("--find-gp", action="store_true", help="recover $gp from SCUS and exit")
    ap.add_argument("--prot", action="store_true", help="also sweep every extracted PROT entry")
    ap.add_argument("--dumps", action="store_true", help="also grep ghidra/scripts/funcs/*.txt")
    ap.add_argument("--no-lui", action="store_true", help="skip the absolute lui+load pair scan")
    ap.add_argument(
        "--no-base-disp",
        action="store_true",
        help="skip the materialised-base + displacement walk",
    )
    args = ap.parse_args()

    scus = load_scus()
    if scus is None:
        print(f"error: {SCUS} not found (extract the disc first)", file=sys.stderr)
        return 2

    if args.find_gp:
        found = recover_gp(scus)
        if found is None:
            print("no `lui gp` / `addiu gp,gp` pair found", file=sys.stderr)
            return 1
        print(f"gp = 0x{found:08x}")
        return 0

    gp = int(args.gp, 16)
    # A target is `(label, absolute VA, gp displacement or None)`. Only an
    # address inside the small-data window has a meaningful displacement, and
    # a scratchpad address never does - `--va 0x1f8003e8` is an absolute-form
    # query, not a claim that `$gp` reaches it.
    targets = []
    for d in args.disps:
        disp = int(d, 16)
        targets.append((f"gp+0x{disp:x}", (gp + disp) & 0xFFFFFFFF, disp))
    for v in args.va:
        va = int(v, 16)
        disp = va - gp
        disp = disp if -0x8000 <= disp < 0x8000 else None
        label = f"gp+0x{disp:x}" if disp is not None else "abs"
        targets.append((label, va & 0xFFFFFFFF, disp))
    if not targets:
        ap.error("give at least one displacement, or --va, or --find-gp")

    images = [scus] + load_overlays()
    if args.prot:
        images += load_prot_entries({im.name.split("/")[0] for im in images[1:]})

    total = 0
    for label, target, disp in targets:
        print(f"\n=== {label} = 0x{target:08x} " + "=" * 34)
        hits = 0
        for image in images:
            for off, text in scan_gp_relative(image, {disp} if disp is not None else set()):
                marks = image.code_markers(off)
                print(f"  GP   {image.where(off)}  {text}   code={marks}")
                hits += 1
            if not args.no_lui:
                pair_sites = set()
                for _, at, text in scan_lui_mem(image, target):
                    pair_sites.add(at)
                    marks = image.code_markers(at)
                    print(f"  LUI  {image.where(at)}  {text}   code={marks}")
                    hits += 1
                if not args.no_base_disp:
                    for at, text in scan_base_disp(image, target):
                        if at in pair_sites:
                            continue
                        marks = image.code_markers(at)
                        print(f"  BASE {image.where(at)}  {text}   code={marks}")
                        hits += 1
        if args.dumps:
            for name, line in scan_dumps({disp} if disp is not None else set()):
                print(f"  DUMP {name}: {line}")
                hits += 1
            for name, line in scan_dumps_abs(target):
                print(f"  DUMP {name}: {line}")
                hits += 1
        if hits == 0:
            forms = ["no gp-relative access"]
            if not args.no_lui:
                forms.append("no lui+load pair")
                if not args.no_base_disp:
                    forms.append("no base+displacement")
            print("  (%s - in any image)" % ", ".join(forms))
        total += hits

    print(f"\n# {len(images)} images scanned, {total} hits")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
