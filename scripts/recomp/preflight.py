#!/usr/bin/env python3
"""Preflight checks for recomp savestate resume.

A savestate load that lands at the boot entry looks like an engine/retail
divergence but is usually a harness fault. This module separates the three
causes *before* a capture runs, so a stale build never gets mistaken for a
finding:

1. **Runtime form.** ``boot_state.c``'s ``apply_section`` ships in two forms.
   The self-wiping form forces ``cpu->pc = entry_pc``; that entry is the
   game's BSS-clear routine, which zeroes the game-mode word, so every load
   restores RAM and then wipes the state that gives it meaning. The working
   form is ``cpu->pc = c->pc ? c->pc : entry_pc``. Reapply with
   ``apply_boot_state_fix.py``.
2. **Build staleness.** A fixed source with an older binary behaves exactly
   like the self-wiping form. Compare mtimes, not source alone.
3. **Stale snapshot.** A slot captured before resume-PC capture existed
   carries ``pc == 0`` in the file itself and falls back to ``entry_pc`` even
   on a correct build. That slot needs recapturing; it is *not* evidence of a
   broken runtime, and conflating the two sends you fixing the wrong thing.

Cause 3 is read straight out of the ``.pst`` byte stream, so it is settled
without launching anything.

Run standalone:

    python3 scripts/recomp/preflight.py --slot 4
    python3 scripts/recomp/preflight.py            # runtime checks + all slots
"""

from __future__ import annotations

import argparse
import glob
import os
import re
import struct
import sys

# .pst layout, confirmed against real slot files and runtime/include/boot_state.h.
# Header: 9 x u32 packed (magic, version, bios_checksum, entry_pc, codegen_hash,
# abi_tag, codegen_ver, section_count, reserved). Then each section is
# u32 tag, u32 pad, u64 len, payload[len] -- written by three fwrites, so the
# stream is packed regardless of struct alignment.
PST_HEADER_SIZE = 36
PST_MAGIC = 0x50535842  # "PSXB"
PST_VERSION = 2
BS_SEC_CPU = 0x01
# CpuRegs = gpr[32], pc, hi, lo, cop0[32], gte_data[32], gte_ctrl[32].
CPU_SECTION_LEN = (32 + 3 + 32 + 32 + 32) * 4  # 524
CPU_PC_OFFSET = 32 * 4  # pc follows gpr[32] inside the payload

RUNTIME_BINARY = os.path.join("build-dbg", "Legend_of_Legaia_Recompiled")

# The two shipped forms of the resume-PC assignment in apply_section.
_FORM_FIXED = re.compile(r"cpu->pc\s*=\s*c->pc\s*\?\s*c->pc\s*:\s*entry_pc")
_FORM_SELF_WIPING = re.compile(r"cpu->pc\s*=\s*entry_pc\s*;")

PATCH_NAME = "scripts/recomp/apply_boot_state_fix.py"


class PreflightError(RuntimeError):
    """A preflight check failed in a way that invalidates a capture."""


# -- locating things -------------------------------------------------------


def boot_state_source(recomp_dir: str) -> str | None:
    """Path to ``boot_state.c``. ``recomp_dir`` may be the build workspace
    (which symlinks the runtime in as ``psxrecomp/``) or the runtime checkout
    itself."""
    for rel in (
        os.path.join("psxrecomp", "runtime", "src", "boot_state.c"),
        os.path.join("runtime", "src", "boot_state.c"),
    ):
        p = os.path.join(recomp_dir, rel)
        if os.path.exists(p):
            return p
    return None


def slot_state_path(recomp_dir: str, slot: int, entry_pc: int | None = None) -> str | None:
    """Locate ``state_<entry_pc>_slot<NN>.pst``. The entry PC is part of the
    name (slots from different games share a directory), so when it is not
    supplied the slot is matched by glob."""
    base = os.path.join(recomp_dir, "build-dbg")
    if entry_pc is not None:
        p = os.path.join(base, "state_%08X_slot%02d.pst" % (entry_pc, slot))
        return p if os.path.exists(p) else None
    hits = sorted(glob.glob(os.path.join(base, "state_*_slot%02d.pst" % slot)))
    return hits[0] if hits else None


def known_slots(recomp_dir: str) -> list[int]:
    base = os.path.join(recomp_dir, "build-dbg")
    slots = []
    for p in glob.glob(os.path.join(base, "state_*_slot*.pst")):
        m = re.search(r"_slot(\d+)\.pst$", p)
        if m:
            slots.append(int(m.group(1)))
    return sorted(set(slots))


# -- the three checks ------------------------------------------------------


def runtime_form(recomp_dir: str) -> str:
    """``"fixed"``, ``"self-wiping"``, or ``"unknown"`` -- which form of the
    resume-PC assignment the *source* carries."""
    src = boot_state_source(recomp_dir)
    if src is None:
        return "unknown"
    with open(src, "r", errors="replace") as f:
        text = f.read()
    # Check the fixed form first: it is a strict superset of the self-wiping
    # pattern's tail, so order matters.
    if _FORM_FIXED.search(text):
        return "fixed"
    if _FORM_SELF_WIPING.search(text):
        return "self-wiping"
    return "unknown"


def build_is_stale(recomp_dir: str) -> bool | None:
    """True when the runtime binary predates ``boot_state.c``. None when
    either path is missing (nothing can be concluded)."""
    src = boot_state_source(recomp_dir)
    binary = os.path.join(recomp_dir, RUNTIME_BINARY)
    if src is None or not os.path.exists(binary):
        return None
    return os.path.getmtime(binary) < os.path.getmtime(src)


def slot_resume_pc(path: str) -> int:
    """Resume PC recorded in a ``.pst``. Raises PreflightError if the file is
    not a v2 snapshot whose first section is a well-formed CPU section --
    a wrong offset must fail loudly rather than return a plausible integer."""
    with open(path, "rb") as f:
        blob = f.read(PST_HEADER_SIZE + 16 + CPU_SECTION_LEN)
    if len(blob) < PST_HEADER_SIZE + 16 + CPU_SECTION_LEN:
        raise PreflightError(f"{path}: truncated, not a complete snapshot")
    magic, version = struct.unpack_from("<II", blob, 0)
    if magic != PST_MAGIC:
        raise PreflightError(f"{path}: bad magic 0x{magic:08X} (expected PSXB)")
    if version != PST_VERSION:
        raise PreflightError(f"{path}: version {version}, expected {PST_VERSION}")
    tag, _pad = struct.unpack_from("<II", blob, PST_HEADER_SIZE)
    (seclen,) = struct.unpack_from("<Q", blob, PST_HEADER_SIZE + 8)
    if tag != BS_SEC_CPU or seclen != CPU_SECTION_LEN:
        raise PreflightError(
            f"{path}: first section is tag {tag} len {seclen}, "
            f"expected CPU tag {BS_SEC_CPU} len {CPU_SECTION_LEN}"
        )
    (pc,) = struct.unpack_from("<I", blob, PST_HEADER_SIZE + 16 + CPU_PC_OFFSET)
    return pc


# -- diagnosis -------------------------------------------------------------


def check_runtime(recomp_dir: str) -> list[str]:
    """Problems with the *build*. An empty list means the runtime honours a
    saved resume PC."""
    problems = []
    form = runtime_form(recomp_dir)
    if form == "self-wiping":
        problems.append(
            "runtime has the self-wiping boot_state form (cpu->pc = entry_pc): "
            "every savestate load re-runs the BSS-clear boot routine and zeroes "
            f"the game-mode word. Reapply with {PATCH_NAME} and rebuild "
            "(make psx-runtime)."
        )
    elif form == "unknown":
        problems.append(
            "could not find the resume-PC assignment in boot_state.c - the "
            "runtime may have been restructured; re-derive the fix before "
            "trusting any savestate load."
        )
    stale = build_is_stale(recomp_dir)
    if stale:
        problems.append(
            "the runtime binary is OLDER than boot_state.c - the running build "
            "does not contain the current source. Rebuild (make psx-runtime); "
            "note the executable name is a file target, so `make "
            "Legend_of_Legaia_Recompiled` is a silent no-op."
        )
    elif stale is None:
        problems.append(
            "could not compare binary and source mtimes (one is missing); "
            "build freshness is unverified."
        )
    return problems


def check_slot(recomp_dir: str, slot: int, entry_pc: int | None = None) -> list[str]:
    """Problems with a *slot*, independent of the build."""
    path = slot_state_path(recomp_dir, slot, entry_pc)
    if path is None:
        return [f"slot {slot}: no snapshot file found"]
    pc = slot_resume_pc(path)
    if pc == 0:
        return [
            f"slot {slot}: stale snapshot (resume pc == 0 in the file), captured "
            "before resume-PC capture existed. It falls back to entry_pc and "
            "self-wipes even on a correct build - recapture it. This is NOT a "
            "runtime fault."
        ]
    return []


def diagnose(recomp_dir: str, slot: int | None = None,
             entry_pc: int | None = None) -> list[str]:
    """All preflight problems, runtime first then slot. Empty == good to fly."""
    problems = check_runtime(recomp_dir)
    if slot is not None:
        problems += check_slot(recomp_dir, slot, entry_pc)
    return problems


def assert_ok(recomp_dir: str, slot: int | None = None,
              entry_pc: int | None = None) -> None:
    problems = diagnose(recomp_dir, slot, entry_pc)
    if problems:
        raise PreflightError(
            "recomp preflight failed:\n  - " + "\n  - ".join(problems)
        )


# -- CLI -------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--recomp-dir", help="recomp workspace (default $LEGAIA_RECOMP_DIR)")
    ap.add_argument("--slot", type=int, help="also check this slot (default: all found)")
    ap.add_argument("--entry-pc", help="entry PC in the slot filename, e.g. 0x80026C28")
    args = ap.parse_args(argv)

    recomp = args.recomp_dir or os.environ.get("LEGAIA_RECOMP_DIR")
    if not recomp:
        print("recomp workspace not configured: set LEGAIA_RECOMP_DIR or pass "
              "--recomp-dir", file=sys.stderr)
        return 2
    recomp = os.path.expanduser(recomp)
    entry_pc = int(args.entry_pc, 0) if args.entry_pc else None

    print(f"runtime form : {runtime_form(recomp)}")
    stale = build_is_stale(recomp)
    print(f"build stale  : {'unknown' if stale is None else stale}")

    runtime_problems = check_runtime(recomp)
    slots = [args.slot] if args.slot is not None else known_slots(recomp)
    slot_problems = []
    for s in slots:
        path = slot_state_path(recomp, s, entry_pc)
        if path is None:
            print(f"slot {s:>2}      : no snapshot file")
            slot_problems.append(f"slot {s}: no snapshot file found")
            continue
        try:
            pc = slot_resume_pc(path)
        except PreflightError as e:
            print(f"slot {s:>2}      : UNREADABLE ({e})")
            slot_problems.append(str(e))
            continue
        verdict = "STALE - recapture" if pc == 0 else "ok"
        print(f"slot {s:>2}      : resume pc 0x{pc:08X}  {verdict}")
        if pc == 0:
            slot_problems += check_slot(recomp, s, entry_pc)

    problems = runtime_problems + slot_problems
    sys.stdout.flush()  # keep the report above the problem list when piped
    if runtime_problems:
        print("\nRUNTIME PROBLEMS (every slot will land at the boot entry):",
              file=sys.stderr)
        for p in runtime_problems:
            print(f"  - {p}", file=sys.stderr)
    if slot_problems:
        print("\nSLOT PROBLEMS (specific snapshots, not the build):", file=sys.stderr)
        for p in slot_problems:
            print(f"  - {p}", file=sys.stderr)
    if not problems:
        print("\npreflight OK")
    return 1 if problems else 0


if __name__ == "__main__":
    raise SystemExit(main())
