"""Register walk shared by the host-side reference scans.

`find-gp-relative-refs.py` and `find-address-word-refs.py` both follow the
register a `lui` loads until an instruction completes the address. What
"follow" means is the whole of their precision, so it lives once here:

  * a register leaves the walk the moment anything else writes it;
  * `addu rB, rA, $zero` / `or rB, rA, $zero` copies `rA` into `rB`;
  * `addu rB, rA, rX` with a runtime `rX` makes `rB` the high half plus an
    index - the array form `lui at,hi; addu at,at,v0; lw v1,lo(at)`, whose
    `hi + lo` is the array's base, not the element read;
  * a `jal` / `jalr` delay slot still sees the register (retail often
    completes a pair there), but the caller-saved registers do not survive
    the call;
  * a branch forks the walk - fall-through and target both carry the state
    out of the delay slot - and `j` / `b` continue at their target only,
    because the word after them is reached from somewhere else (typically
    the other arm of an `if`, whose own state this walk does not have);
    `jr ra` ends the path.

Before this module both scans walked straight down and stopped only when
the register was re-loaded by another `lui`, so a `lui v0,hi; lw v0,x(v0);
... lw v1,lo(v0)` sequence paired the high half with a pointer the first
load returned, and a pair split across a branch - `bnez v0,L; lui v0,hi`
with `L: lw a1,lo(v0)` - was found only when a stale `lui` above happened to
line up. See
docs/tooling/address-reference-scan.md.
"""

from __future__ import annotations

def writes_reg(word: int) -> int | None:
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


def propagate(word: int, regs: dict) -> bool:
    """Carry a `lui`-derived register through a copy or one indexing `addu`.

    `regs` maps a register to the index register already summed into it, or
    None for a plain one. `addu rB, rA, $zero` / `or rB, rA, $zero` copies
    `rA`'s state into `rB`; `addu rB, rA, rX` with `rA` plain and `rX`
    untracked makes `rB` the high half **plus an index** - the `lui at,hi;
    addu at,at,v0; lw v1,lo(at)` array access, whose `lo` completes the
    array's *base*, not the datum. Returns True when the instruction was
    consumed this way (the caller must then not treat it as an ordinary
    redefinition).
    """
    if word >> 26 != 0x00 or (word & 0x3F) not in (0x21, 0x25):
        return False
    rs, rt, rd = (word >> 21) & 0x1F, (word >> 16) & 0x1F, (word >> 11) & 0x1F
    if rd == 0:
        return False
    if (rs in regs) == (rt in regs):
        return False
    src, other = (rs, rt) if rs in regs else (rt, rs)
    if other == 0:
        state = regs[src]
    elif (word & 0x3F) == 0x21 and regs[src] is None:
        state = other
    else:
        return False
    regs[rd] = state
    return True


# Registers a call may clobber under the MIPS o32 convention: at, v0-v1,
# a0-a3, t0-t9, ra.
CALLER_SAVED = frozenset(list(range(1, 16)) + [24, 25, 31])


def _branch(word: int, at: int, base: int | None):
    """Classify a control transfer at file offset `at`.

    Returns `(kind, target_offset)`: kind is `"call"`, `"return"`, `"jump"`
    (unconditional, target only), `"branch"` (conditional: both ways) or
    `None`. A `j` target needs the image's load base; without one the path
    simply ends there.
    """
    op = word >> 26
    rs, rt = (word >> 21) & 0x1F, (word >> 16) & 0x1F
    simm = (word & 0xFFFF) - (0x10000 if word & 0x8000 else 0)
    rel = at + 4 + (simm << 2)
    if op == 0x03 or (op == 0x00 and (word & 0x3F) == 0x09):
        return "call", None
    if op == 0x00 and (word & 0x3F) == 0x08:
        return ("return", None) if rs == 31 else (None, None)
    if op == 0x02:
        if base is None:
            return "return", None
        va = ((base + at + 4) & 0xF0000000) | ((word & 0x03FFFFFF) << 2)
        return "jump", va - base
    if op == 0x04 and rs == 0 and rt == 0:
        return "jump", rel
    if op == 0x01 and rs == 0 and rt == 0x01:  # bgez zero: `b`
        return "jump", rel
    if op in (0x04, 0x05, 0x06, 0x07) or (op == 0x01 and rt in (0x00, 0x01, 0x10, 0x11)):
        return "branch", rel
    return None, None


def walk(word_at, lui_off: int, window: int, base: int | None = None):
    """Follow the register a `lui` at `lui_off` loads, yielding every word
    the value can reach as `(offset, word, state)`.

    `word_at(off)` returns the word at a file offset or None. `state` maps a
    register to `(value, index_reg)`: `value` the address it holds (the high
    half, or the high half plus an `addiu`/`ori`), `index_reg` the runtime
    index summed into it (`propagate`) or None. It is the state the word
    *sees*; a caller tests the word's use of it before the walk applies the
    word. `window` bounds every path in instructions.
    """
    lui = word_at(lui_off)
    reg = (lui >> 16) & 0x1F
    if reg == 0:
        return
    start = {reg: (((lui & 0xFFFF) << 16) & 0xFFFFFFFF, None)}
    stack = [(lui_off + 4, start, window)]
    # A `lui` in a delay slot - retail hoists one into `bnez v0,L` - runs
    # before the jump lands, so the walk starts where the jump goes.
    prev = word_at(lui_off - 4) if lui_off >= 4 else None
    kind, target = _branch(prev, lui_off - 4, base) if prev is not None else (None, None)
    if kind == "return":
        return
    if kind == "call":
        start = {r: v for r, v in start.items() if r not in CALLER_SAVED}
        stack = [(lui_off + 4, start, window)] if start else []
    elif kind == "jump":
        stack = [(target, start, window)]
    elif kind == "branch":
        stack.append((target, dict(start), window))
    visited = set()
    while stack:
        at, state, budget = stack.pop()
        pending = None
        while budget > 0 and state:
            if at in visited and pending is None:
                break
            word = word_at(at)
            if word is None:
                break
            visited.add(at)
            yield at, word, state
            state = _apply(word, state)
            budget -= 1
            here = _branch(word, at, base)
            if pending is not None:
                kind, target = pending
                pending = None
                if kind == "return":
                    break
                if kind == "call":
                    state = {r: v for r, v in state.items() if r not in CALLER_SAVED}
                elif kind == "jump":
                    at = target
                    continue
                elif kind == "branch" and target is not None:
                    stack.append((target, dict(state), budget))
            elif here[0] is not None:
                pending = here
            at += 4


def _apply(word: int, state: dict) -> dict:
    """The state after `word` runs."""
    op = word >> 26
    rs, rt = (word >> 21) & 0x1F, (word >> 16) & 0x1F
    imm = word & 0xFFFF
    simm = imm - 0x10000 if imm & 0x8000 else imm
    state = dict(state)
    if op in (0x09, 0x0D) and rs in state:
        value, index = state[rs]
        if op == 0x0D and index is not None:
            state.pop(rt, None)
        else:
            value = (value | imm) if op == 0x0D else (value + simm)
            state[rt] = (value & 0xFFFFFFFF, index)
        return state
    idx = {r: v[1] for r, v in state.items()}
    if propagate(word, idx):
        rd = (word >> 11) & 0x1F
        src = rs if rs in state else rt
        state[rd] = (state[src][0], idx[rd])
        return state
    dest = writes_reg(word)
    if dest is not None:
        state.pop(dest, None)
    return state
