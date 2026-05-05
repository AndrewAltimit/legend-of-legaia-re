# @category Legaia
# @runtime Jython
#
# Dump ASCII strings at given addresses (typically debug printf format strings).

from ghidra.program.model.address import AddressFactory

TARGETS = [
    "801cf5ac", "801cf5bc",
]

prog = currentProgram
mem = prog.getMemory()
af = prog.getAddressFactory()


def read_string(addr_str, max_len=128):
    addr = af.getAddress(addr_str)
    if addr is None:
        return None
    out = []
    cur = addr
    for _ in range(max_len):
        try:
            b = mem.getByte(cur) & 0xFF
        except Exception:
            return None
        if b == 0:
            break
        if 0x20 <= b < 0x7F:
            out.append(chr(b))
        else:
            out.append("\\x{:02x}".format(b))
        cur = cur.add(1)
    return "".join(out)


for t in TARGETS:
    s = read_string(t)
    print("{}: {!r}".format(t, s))
