# @category Legaia
# @runtime Jython
#
# Build a static map of every call site that passes a constant PROT index
# to the LBA resolver chain (FUN_8003e8a8 + close relatives that read the
# same in-RAM TOC at 0x801C70F0).
#
# This surfaces:
#   * which PROT entries have ANY static caller in SCUS_942.54 (vs purely
#     overlay-loaded entries),
#   * co-load groups: functions that resolve >1 constant index form a
#     bundle (e.g. an asset chain loaded together for a scene),
#   * the per-index "calling context" (caller function name).
#
# Output format: CSV-ish lines on stdout, one row per (caller, index):
#   prot_index,caller_addr,caller_name,call_site,resolver
#
# Methodology:
#   For each call site to a target function, walk back up to N instructions
#   looking for the most recent write to register a0 (MIPS arg 0). If that
#   write is an ADDIU/ORI with an immediate, OR a sequence of LUI+ORI/ADDIU
#   that produces a 16-bit value, record the value as the constant arg.
#
# Targets - anything that takes a PROT index and reads the TOC at 0x801C70F0:
#   FUN_8003e8a8 (LBA resolver, returns byte length)
#   FUN_8003eb98 (LBA resolver variant; takes (idx, ?, flag))
#   FUN_8003e360 (also references the TOC)
#
# False-positive notes:
#   * a0 might come from a function arg (no constant) - skipped.
#   * a0 might be set via a register move (unimplemented MIPS pipeline tracking).
#     Future improvement: track register aliasing to follow `move a0, sX`.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
ref_mgr = prog.getReferenceManager()
listing = prog.getListing()

TARGETS = [
    ("FUN_8003e8a8", 0x8003e8a8),
    ("FUN_8003eb98", 0x8003eb98),
    ("FUN_8003e360", 0x8003e360),
]

# How far back to walk per call site looking for $a0's source.
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
    """Walk back from call_addr looking for the value of `target_reg` at the
    call. Handles:
      * li/addiu/ori with immediate
      * clear (pseudo-op -> 0)
      * move src,dst (renames target_reg to src)
      * lui+ori/addiu pairs (lui sets upper 16, addiu/ori the lower)
    Returns the constant or None if can't resolve.
    """
    cur = listing.getInstructionBefore(call_addr)
    steps = 0
    upper = None  # tracked LUI value for current target_reg
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
            v = imm_at(cur, 1)
            return v
        if mnem == "lui":
            v = imm_at(cur, 1)
            if v is not None:
                # Combined value when paired with a later ori/addiu we already
                # processed below. But we walk backward, so LUI is older than
                # any addiu we saw. If we already merged with an addiu, we'd
                # have returned. So if we reach LUI alone, it's just the high.
                if upper is not None:
                    return ((v & 0xFFFF) << 16) | upper
                # Pure LUI without low half = address-page form, unlikely a
                # PROT index. Return high << 16 anyway.
                return (v & 0xFFFF) << 16
            return None
        if mnem == "ori" or mnem == "addiu":
            # Two forms: `addiu dest, src, imm` or `addiu dest, imm` (li alias).
            ops = cur.getNumOperands()
            imm = imm_at(cur, ops - 1)
            if imm is None:
                return None
            if ops == 2:
                # `addiu dest, imm` (no source) - effectively li.
                return imm
            src = reg_name(cur, 1)
            if src is None:
                return None
            if src == "zero":
                return imm
            # Need to find the value of `src`. Common pattern is preceding LUI.
            # Track `upper` if we encounter an LUI on `dest` later (older).
            # Actually we process backward, so we may not yet have seen LUI.
            # Continue walking with target_reg = src and accumulate the imm.
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
        # Any other write to target_reg: stop, can't resolve.
        return None
    return None


def find_const_arg(call_addr):
    return trace_register_const(call_addr, "a0"), None


print("prot_index,caller_addr,caller_name,call_site,resolver")

for tname, taddr in TARGETS:
    addr = af.getAddress("{:x}".format(taddr))
    refs = list(ref_mgr.getReferencesTo(addr))
    for r in refs:
        site = r.getFromAddress()
        func = fm.getFunctionContaining(site)
        if func is None:
            continue
        # Only count actual calls, skip data refs.
        rt_name = str(r.getReferenceType()).lower()
        if "call" not in rt_name and "unconditional_call" not in rt_name:
            # Some MIPS branch targets are typed differently; let through.
            instr = listing.getInstructionAt(site)
            if instr is None:
                continue
            mnem = instr.getMnemonicString().lower()
            if mnem not in ("jal", "jalr"):
                continue
        const, set_at = find_const_arg(site)
        if const is None:
            continue
        # Only print plausibly-real PROT indices (PROT.DAT has ~1234 entries).
        if const > 0x500:
            continue
        print("0x{:X},{},{},{},{}".format(
            const,
            func.getEntryPoint(),
            func.getName(),
            site,
            tname,
        ))
