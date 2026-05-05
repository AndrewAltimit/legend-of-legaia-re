# @category Legaia
# @runtime Jython
#
# Read mode-name strings and per-mode handler function entry points from the
# table at 0x8007078C. Resolve string pointers; check whether the handler
# entries are real functions; flag mode 6's RAM-resident handler (script VM
# candidate).

prog = currentProgram
af = prog.getAddressFactory()
listing = prog.getListing()
fm = prog.getFunctionManager()
mem = prog.getMemory()

TABLE_BASE = 0x8007078C
ENTRY_SIZE = 0x18
N = 28  # actual number of modes (after this is a different table)

def read_str(ram_addr, max_len=64):
    out = []
    a = af.getAddress("{:x}".format(ram_addr))
    for j in range(max_len):
        try:
            b = mem.getByte(a.add(j)) & 0xFF
        except:
            return None
        if b == 0:
            return "".join(out)
        if b < 0x20 or b > 0x7E:
            return None
        out.append(chr(b))
    return "".join(out) + "..."

print("=== Mode table (canonical interpretation) ===")
print("idx  name_ptr   mode_name                       next_mode  handler_ptr  handler_fn          param")
print("-" * 110)
for i in range(N):
    a = af.getAddress("{:x}".format(TABLE_BASE + i * ENTRY_SIZE))
    bs = bytearray(ENTRY_SIZE)
    for j in range(ENTRY_SIZE):
        bs[j] = mem.getByte(a.add(j)) & 0xFF
    def u16(off): return bs[off] | (bs[off+1] << 8)
    def u32(off): return bs[off] | (bs[off+1]<<8) | (bs[off+2]<<16) | (bs[off+3]<<24)
    name_ptr = u32(0x00)
    next_mode = u16(0x0a)
    if next_mode > 0x7FFF:
        next_mode -= 0x10000
    handler_ptr = u32(0x10)
    param = u32(0x14)
    name = read_str(name_ptr) or "(non-string)"
    handler_a = af.getAddress("{:x}".format(handler_ptr))
    func = fm.getFunctionContaining(handler_a) or fm.getFunctionAt(handler_a)
    fname = func.getName() if func else "(not in any known function)"
    if handler_ptr >= 0x801C0000 and handler_ptr < 0x80200000:
        fname = "*** RAM (loaded module) ***"
    print(" {:2d}  0x{:08X}  {:30s}  {:9d}  0x{:08X}  {:18s}  0x{:x}".format(
        i, name_ptr, name[:30], next_mode, handler_ptr, fname, param))

print()
print("=== Decompile the dispatcher candidate (FUN_8001dcf8) ===")
print("(This function loads the table base and is large enough to be the per-frame mode driver)")
addr = af.getAddress("8001dcf8")
func = fm.getFunctionAt(addr)
print("Name: {}, body size: {}".format(func.getName(), func.getBody().getNumAddresses()))

# Decompile
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor
decomp = DecompInterface()
opts = DecompileOptions()
decomp.setOptions(opts)
decomp.openProgram(prog)
res = decomp.decompileFunction(func, 60, ConsoleTaskMonitor())
if res.decompileCompleted():
    out = res.getDecompiledFunction().getC()
    # Print first ~80 lines
    for line in out.split("\n")[:80]:
        print(line)
else:
    print("decompile failed:", res.getErrorMessage())

print("\ndone")
