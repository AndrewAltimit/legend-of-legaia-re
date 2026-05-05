# @category Legaia
# @runtime Jython
#
# Inventory pass for the 0896_bat_back_dat overlay (loaded at RAM 0x801C5818
# in town mode). Output: function table sorted by size and outgoing-call count
# + Shift-JIS / ASCII strings within the first 8 KB.

import os

OUT = "/scripts/funcs/overlay_0896_survey.txt"

prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
mem = prog.getMemory()

functions = []
for f in fm.getFunctions(True):
    body = f.getBody()
    size = body.getNumAddresses()
    in_refs = sum(1 for _ in prog.getReferenceManager().getReferencesTo(f.getEntryPoint()))
    out_calls = 0
    for blk in f.getBody():
        pass
    insns = list(listing.getInstructions(f.getBody(), True))
    out_calls = sum(1 for i in insns if i.getMnemonicString() in ("jal", "JAL"))
    functions.append((str(f.getEntryPoint()), f.getName(), size, len(insns), out_calls, in_refs))

with open(OUT, "w") as fh:
    fh.write("== 0896_bat_back_dat overlay survey ==\n")
    fh.write("Loaded at RAM 0x801C5818 in town mode (delta 0x5818 from town save base).\n")
    fh.write("File size 325632 bytes; find-overlay rank 6 / score 3.89.\n\n")

    fh.write("--- TOP 25 FUNCTIONS BY SIZE ---\n")
    fh.write("addr        name                          size  insns  out  in\n")
    for entry in sorted(functions, key=lambda x: -x[2])[:25]:
        fh.write("{:10}  {:<28}  {:>5}  {:>5}  {:>3}  {:>3}\n".format(*entry))

    fh.write("\n--- TOP 25 FUNCTIONS BY OUT-CALLS ---\n")
    fh.write("addr        name                          size  insns  out  in\n")
    for entry in sorted(functions, key=lambda x: -x[4])[:25]:
        fh.write("{:10}  {:<28}  {:>5}  {:>5}  {:>3}  {:>3}\n".format(*entry))

    fh.write("\n--- TOP 25 FUNCTIONS BY IN-REFS ---\n")
    fh.write("addr        name                          size  insns  out  in\n")
    for entry in sorted(functions, key=lambda x: -x[5])[:25]:
        fh.write("{:10}  {:<28}  {:>5}  {:>5}  {:>3}  {:>3}\n".format(*entry))

    fh.write("\n--- TOTAL FUNCTION COUNT: {} ---\n".format(len(functions)))

    # Bytes 0..0x2000 string scan.
    fh.write("\n--- ASCII / SHIFT-JIS STRING SAMPLES (first 0x2000 bytes) ---\n")
    base = af.getAddress("0x801C5818")
    buf = bytearray(0x2000)
    for i in range(0x2000):
        try:
            buf[i] = mem.getByte(base.add(i)) & 0xFF
        except:
            buf[i] = 0

    # ASCII runs >= 4 chars
    cur = []
    runs = []
    for i, b in enumerate(buf):
        if 0x20 <= b < 0x7F:
            cur.append(chr(b))
        else:
            if len(cur) >= 4:
                runs.append((i - len(cur), "".join(cur)))
            cur = []
    if len(cur) >= 4:
        runs.append((len(buf) - len(cur), "".join(cur)))
    fh.write("ASCII runs (offset, text):\n")
    for off, s in runs[:80]:
        fh.write("  +{:04x}: {}\n".format(off, s))

    fh.write("\nFirst 256 bytes (hex):\n")
    for row in range(16):
        fh.write("  +{:04x}:".format(row * 16))
        for col in range(16):
            fh.write(" {:02x}".format(buf[row * 16 + col]))
        fh.write("\n")

print("wrote " + OUT)
