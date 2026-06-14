#!/usr/bin/env python3
"""Linear MIPS32 (LE) disassembler for an as-loaded overlay .bin extracted by
`asset overlay extract`. Decodes one 4-byte word at a time so embedded data /
unknown encodings emit `.word` instead of halting the sweep (capstone's stock
linear sweep stops at the first non-instruction).

Usage:
  overlay_disasm.py <overlay.bin> <base_va_hex> [start_va_hex [n_insns]]

  No start_va  -> dump the whole overlay to stdout (pipe to a file + grep).
  start_va     -> dump n_insns (default 80) instructions from there.

The overlay .bin files are Sony code (gitignored under extracted/); this script
is host tooling and contains none of those bytes.

Note: a stale /tmp/dis.py on sys.path shadows stdlib `dis`, which capstone's
`import inspect` chain needs; importing dis/inspect first sidesteps that.
"""
import dis, inspect  # noqa: F401  (force the real stdlib modules in first)
import sys, struct, capstone


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        sys.exit(1)
    binpath = sys.argv[1]
    base = int(sys.argv[2], 16)
    data = open(binpath, "rb").read()
    md = capstone.Cs(capstone.CS_ARCH_MIPS,
                     capstone.CS_MODE_MIPS32 | capstone.CS_MODE_LITTLE_ENDIAN)

    def line(va, word):
        g = list(md.disasm(word, va))
        if g:
            return "%08x  %-8s %s" % (g[0].address, g[0].mnemonic, g[0].op_str)
        return "%08x  .word    0x%08x" % (va, struct.unpack('<I', word)[0])

    if len(sys.argv) >= 4:
        start = int(sys.argv[3], 16)
        n = int(sys.argv[4]) if len(sys.argv) >= 5 else 80
        off = start - base
        for i in range(n):
            print(line(start + i * 4, data[off + i * 4: off + i * 4 + 4]))
    else:
        out = []
        for i in range(len(data) // 4):
            out.append(line(base + i * 4, data[i * 4: i * 4 + 4]))
        sys.stdout.write("\n".join(out) + "\n")


if __name__ == "__main__":
    main()
