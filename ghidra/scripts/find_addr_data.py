# @category Legaia
# @runtime Jython
#
# Search the entire SCUS binary memory for any 4-byte word == specific address.
# Catches function pointer tables that wouldn't show in ref manager xrefs.

import struct

prog = currentProgram
mem = prog.getMemory()
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

TARGETS = [
    0x80020224,  # the (type_size, data_offset) walker - zero xrefs
    0x8001f05c,  # the dispatcher
    0x801c70f0,  # the in-RAM TOC base
]

for tgt in TARGETS:
    print("\n== searching for 0x{:08X} as 4-byte LE data ==".format(tgt))
    target_le = struct.pack("<I", tgt)
    found = []
    for block in mem.getBlocks():
        if not block.isInitialized():
            continue
        try:
            start = block.getStart().getOffset()
            size = block.getSize()
            # Read in chunks
            chunk = 0x10000
            for off in range(0, size, chunk):
                read_size = min(chunk, size - off)
                buf = bytearray(read_size)
                mem.getBytes(block.getStart().add(off), buf)
                # search
                idx = 0
                while True:
                    i = buf.find(target_le, idx)
                    if i < 0:
                        break
                    abs_addr = start + off + i
                    found.append(abs_addr)
                    idx = i + 1
        except Exception as e:
            print("  block error: {}".format(e))
    for a in found[:30]:
        addr = af.getAddress("{:x}".format(a))
        # is this in code (instructions) or data?
        insn = listing.getInstructionAt(addr)
        func = fm.getFunctionContaining(addr)
        fname = func.getName() if func else "<no func>"
        kind = "INSN" if insn else "data"
        print("  found at 0x{:08X}  [{}]  in {}".format(a, kind, fname))
    if len(found) > 30:
        print("  ... +{} more".format(len(found) - 30))
    print("  total: {}".format(len(found)))
