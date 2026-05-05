# @category Legaia
# @runtime Jython
# Check status of remaining missing addresses
import os

prog = currentProgram
fm = prog.getFunctionManager()
af = prog.getAddressFactory()

ADDRS = [
    "801c63f0", "801c6884", "801c735c", "801c7574", "801c8ae8",
    "801c906c", "801c924c", "801c9394", "801c9608", "801c97dc",
    "801c9a00", "801c9e3c", "801ca278", "801cd194", "801cd1d8",
    "801cd21c", "801cd260", "801cd2a4", "801cd2e8", "801cd3b4",
    "801cd40c", "801cd4ec", "801ce850", "801ce8a0", "801ce8cc",
    "801ce8ec", "801cea3c", "801cea6c", "801cec94", "801cee80",
    "801cef54", "801cf00c", "801cf070", "801cf1b0", "801cf4ac",
    "801cf5d0", "801cf9f4", "801cfa48", "801cfbe4", "801cfe4c",
]

for s in ADDRS:
    addr = af.getAddress(s)
    func_at = fm.getFunctionAt(addr)
    func_in = fm.getFunctionContaining(addr)
    inst = currentProgram.getListing().getInstructionAt(addr)
    if func_at:
        print("{}: FN_AT={} size={}".format(s, func_at.getName(), func_at.getBody().getNumAddresses()))
    elif func_in:
        print("{}: FN_IN={} entry={}".format(s, func_in.getName(), func_in.getEntryPoint()))
    elif inst:
        print("{}: INST={}".format(s, inst.toString()))
    else:
        print("{}: nothing".format(s))
