# @category Legaia
# @runtime Jython
#
# Scan an OVERLAY program (imported as Raw Binary at base 0x801C0000) for
# every call site that hits a SCUS-side asset loader. Const-tracks the
# PROT index (or string-arg) and emits a CSV of:
#
#   loader,prot_index_or_string,caller_func,call_site
#
# Run with: -process overlay.bin (after importing the overlay dump via
# ghidra/scripts/import_overlay.sh).
#
# Loader functions live at fixed SCUS addresses, so even though the
# overlay is a separate Ghidra program, calls into them are direct `jal`
# instructions to the canonical SCUS addresses. Ghidra's xref manager
# won't show them as outgoing edges (the destination isn't in this
# program's address space), so we walk every instruction and detect the
# jal target manually.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

# (name, address) of every SCUS asset loader we want to detect.
# Index args: FUN_8003e8a8 + FUN_8003eb98 take (idx, ...).
# String args: FUN_8003e6bc takes (path_string, ...).
# Both paths feed the same in-RAM TOC at 0x801C70F0.
LOADERS = {
    0x8003E8A8: ("FUN_8003e8a8", "index"),
    0x8003EB98: ("FUN_8003eb98", "index"),
    0x8003E6BC: ("FUN_8003e6bc", "string"),
    0x8003E800: ("FUN_8003e800", "buffer"),  # downstream of e8a8
    0x800520F0: ("FUN_800520f0", "scene"),  # battle scene loader
    0x8001F7C0: ("FUN_8001f7c0", "field"),  # field/town loader
    0x8001E890: ("FUN_8001e890", "player"),  # player.lzs loader
    0x8001ED60: ("FUN_8001ed60", "player2"),
    # Asset-type orchestrators (downstream of the field loader).
    0x8002541C: ("FUN_8002541c", "asset_type"),  # streaming driver
    0x800255B8: ("FUN_800255b8", "asset_type"),  # per-asset-type field loader
    0x80020224: ("FUN_80020224", "buffer"),       # descriptor-pair walker
    0x8001F05C: ("FUN_8001f05c", "type_size"),    # asset-type dispatcher
}

LOOKBACK = 24


def reg_name(instr, op_idx):
    try:
        r = instr.getRegister(op_idx)
        return r.getName() if r is not None else None
    except Exception:
        return None


def imm_at(instr, op_idx):
    try:
        s = instr.getScalar(op_idx)
        return s.getUnsignedValue() & 0xFFFFFFFF if s is not None else None
    except Exception:
        return None


def trace_register_const(call_addr, target_reg):
    """Walk back looking for the value of `target_reg` at the call. Handles
    li/addiu/ori/lui+addiu/move chains. Returns int or None."""
    cur = listing.getInstructionBefore(call_addr)
    steps = 0
    upper = None
    while cur is not None and steps < LOOKBACK:
        steps += 1
        mnem = cur.getMnemonicString().lower()
        dest = reg_name(cur, 0)
        if dest != target_reg:
            cur = cur.getPrevious()
            continue
        if mnem == "clear":
            return 0
        if mnem == "li":
            return imm_at(cur, 1)
        if mnem == "lui":
            v = imm_at(cur, 1)
            if v is None:
                return None
            if upper is not None:
                return ((v & 0xFFFF) << 16) | upper
            return (v & 0xFFFF) << 16
        if mnem == "ori" or mnem == "addiu":
            ops = cur.getNumOperands()
            imm = imm_at(cur, ops - 1)
            if imm is None:
                return None
            if ops == 2:
                return imm
            src = reg_name(cur, 1)
            if src is None:
                return None
            if src == "zero":
                return imm
            target_reg = src
            upper = imm
            cur = cur.getPrevious()
            continue
        if mnem == "move":
            src = reg_name(cur, 1)
            if src is None:
                return None
            target_reg = src
            cur = cur.getPrevious()
            continue
        return None
    return None


def addr_to_string(addr_int):
    """If `addr_int` looks like a pointer to an ASCII string somewhere in
    SCUS or this overlay, return the string. Otherwise None.
    Best-effort: we can only see strings within THIS program's memory."""
    a = af.getAddress("{:x}".format(addr_int))
    if a is None:
        return None
    data = listing.getDataAt(a)
    if data is None:
        return None
    try:
        v = data.getValue()
        if v is None:
            return None
        s = str(v)
        # Filter to plausible asset-path strings.
        if "\\" in s or "/" in s or s.lower().endswith((".dat", ".lzs", ".pac")):
            return s
    except Exception:
        return None
    return None


print("loader,arg_kind,arg_value,caller_func,caller_addr,call_site")

# Iterate every instruction across the whole program (true = forward).
ins_iter = listing.getInstructions(True)
for ins in ins_iter:
        mnem = ins.getMnemonicString().lower()
        if mnem not in ("jal", "jalr"):
            continue
        # `jal target` -> operand 0 is an Address (not a Scalar) for direct
        # jal. Try multiple extraction paths.
        target = None
        try:
            objs = ins.getOpObjects(0)
            for o in objs:
                # Address objects have getOffset()
                if hasattr(o, "getOffset"):
                    target = o.getOffset() & 0xFFFFFFFF
                    break
        except Exception:
            pass
        if target is None:
            # Fall back to scalar (jalr / pseudo-ops)
            target = imm_at(ins, 0)
        if target is None:
            try:
                refs = ins.getReferencesFrom()
                if len(refs) > 0:
                    target = refs[0].getToAddress().getOffset() & 0xFFFFFFFF
            except Exception:
                continue
        if target is None or target not in LOADERS:
            continue
        loader_name, kind = LOADERS[target]
        # Trace $a0 for the call.
        a0_val = trace_register_const(ins.getAddress(), "a0")
        if a0_val is None:
            arg_str = "?"
        elif kind == "string":
            s = addr_to_string(a0_val)
            arg_str = repr(s) if s else "0x{:X}".format(a0_val)
        else:
            arg_str = "0x{:X}".format(a0_val) if a0_val < 0x500 else "0x{:X}(?)".format(a0_val)
        # Containing function (may be None if instruction is outside any).
        cfunc = fm.getFunctionContaining(ins.getAddress())
        cname = cfunc.getName() if cfunc else "<no func>"
        centry = str(cfunc.getEntryPoint()) if cfunc else "?"
        print("{},{},{},{},{},{}".format(
            loader_name,
            kind,
            arg_str,
            cname,
            centry,
            ins.getAddress(),
        ))
