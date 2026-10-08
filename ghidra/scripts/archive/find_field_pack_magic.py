# @category Legaia
# @runtime Jython
#
# Search for code that materialises the field-pack magic word
# 0x01059B84 - either as a 32-bit immediate (LUI+ORI/ADDIU pair) or as
# byte-by-byte loads of the four-byte sequence 84 9B 05 01.
#
# Run on SCUS_942.54 first, then on captured overlays. Helps locate the
# runtime parser/loader for field-pack PROT entries.

from ghidra.util.task import ConsoleTaskMonitor

MAGIC_WORD = 0x01059B84
HI = 0x0105
LO = 0x9B84

prog = currentProgram
mem = prog.getMemory()
listing = prog.getListing()

print("[field-pack] scanning {} for magic 0x{:08X}".format(prog.getName(), MAGIC_WORD))


def scan_immediates():
    # Walk every LUI instruction and look for an ADDIU / ORI partner that
    # combines to MAGIC_WORD.
    hits = []
    func_iter = prog.getFunctionManager().getFunctions(True)
    for f in func_iter:
        # per-function register tracker
        lui_targets = {}  # reg -> hi
        body = f.getBody()
        for ins in listing.getInstructions(body, True):
            mnem = ins.getMnemonicString().lower()
            if mnem == "lui":
                # Operand 0 = dst register, operand 1 = imm.
                ops = ins.getNumOperands()
                if ops >= 2:
                    try:
                        dst = ins.getDefaultOperandRepresentation(0)
                        imm = ins.getScalar(1)
                        if imm is not None:
                            lui_targets[dst] = imm.getUnsignedValue() & 0xFFFF
                    except Exception:
                        pass
            elif mnem in ("addiu", "ori"):
                try:
                    src = ins.getDefaultOperandRepresentation(1)
                    imm = ins.getScalar(2)
                    if imm is not None and src in lui_targets:
                        combined = (lui_targets[src] << 16) | (imm.getUnsignedValue() & 0xFFFF)
                        # The ADDIU sign-extends, so values that fit ADDIU need an off-by-one.
                        if combined == MAGIC_WORD or (
                            mnem == "addiu" and combined - 0x10000 == MAGIC_WORD
                        ):
                            hits.append((f.getName(), ins.getAddress(), str(ins)))
                except Exception:
                    pass
    return hits


def scan_byte_pattern():
    # Search the .data and .rodata regions for the exact 4-byte LE pattern.
    hits = []
    pattern = bytes(bytearray([0x84, 0x9B, 0x05, 0x01]))
    blocks = mem.getBlocks()
    for blk in blocks:
        if not blk.isInitialized():
            continue
        try:
            start = blk.getStart()
            size = int(blk.getSize())
            buf = bytearray(size)
            mem.getBytes(start, buf)
            data = bytes(buf)
            i = 0
            while True:
                j = data.find(pattern, i)
                if j < 0:
                    break
                addr = start.add(j)
                hits.append((blk.getName(), addr))
                i = j + 1
        except Exception as e:
            print("[field-pack] block {} read failed: {}".format(blk.getName(), e))
    return hits


imm_hits = scan_immediates()
byte_hits = scan_byte_pattern()

print("[field-pack] immediate-pair hits: {}".format(len(imm_hits)))
for name, addr, ins in imm_hits[:20]:
    print("  {} @ {} : {}".format(name, addr, ins))
print("[field-pack] byte-pattern hits: {}".format(len(byte_hits)))
for blk, addr in byte_hits[:20]:
    print("  {} @ {}".format(blk, addr))

print("[field-pack] done")
