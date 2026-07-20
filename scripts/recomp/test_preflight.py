#!/usr/bin/env python3
"""Unit tests for the savestate-resume preflight.

Pure-python, synthetic fixtures only (no game data, no Sony bytes). The .pst
fixtures are built from the documented section layout, so a decode regression
fails here rather than silently returning a plausible integer. Run with:

    python3 -m unittest scripts/recomp/test_preflight.py
    (or: cd scripts/recomp && python3 -m unittest test_preflight)
"""

import os
import struct
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import preflight  # noqa: E402

FIXED_SRC = """
static int apply_section(uint32_t tag, const uint8_t* p, uint32_t len) {
    /* fall back to entry_pc only for legacy snapshots (c->pc == 0). */
    cpu->pc = c->pc ? c->pc : entry_pc;
    cpu->hi = c->hi;
}
"""

SELF_WIPING_SRC = """
static int apply_section(uint32_t tag, const uint8_t* p, uint32_t len) {
    cpu->pc = entry_pc;   /* always enter at the game entry, never a mid-PC */
    cpu->hi = c->hi;
}
"""

RESTRUCTURED_SRC = """
static int apply_section(uint32_t tag, const uint8_t* p, uint32_t len) {
    cpu_set_pc(cpu, resolve_resume(c, entry_pc));
}
"""


def make_pst(path, pc, *, magic=preflight.PST_MAGIC, version=preflight.PST_VERSION,
             tag=preflight.BS_SEC_CPU, seclen=preflight.CPU_SECTION_LEN,
             entry_pc=0x80026C28, truncate=False):
    """Build a synthetic snapshot: 9x u32 header, then tag/pad/len + CpuRegs."""
    hdr = struct.pack(
        "<IIIIiIIII", magic, version, 0xDEADBEEF, entry_pc, 0, 0, 0, 14, 0
    )
    sec = struct.pack("<IIQ", tag, 0, seclen)
    payload = bytearray(preflight.CPU_SECTION_LEN)
    struct.pack_into("<I", payload, preflight.CPU_PC_OFFSET, pc)
    blob = hdr + sec + bytes(payload)
    if truncate:
        blob = blob[: len(blob) // 2]
    with open(path, "wb") as f:
        f.write(blob)
    return path


class TestSlotDecode(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = self.tmp.name
        self.addCleanup(self.tmp.cleanup)

    def test_reads_resume_pc(self):
        p = make_pst(os.path.join(self.dir, "a.pst"), 0x80045690)
        self.assertEqual(preflight.slot_resume_pc(p), 0x80045690)

    def test_reads_zero_pc(self):
        p = make_pst(os.path.join(self.dir, "b.pst"), 0)
        self.assertEqual(preflight.slot_resume_pc(p), 0)

    def test_section_len_matches_cpuregs(self):
        # gpr[32] + pc/hi/lo + cop0[32] + gte_data[32] + gte_ctrl[32]
        self.assertEqual(preflight.CPU_SECTION_LEN, 524)

    def test_bad_magic_raises(self):
        p = make_pst(os.path.join(self.dir, "c.pst"), 0x1234, magic=0x11111111)
        with self.assertRaises(preflight.PreflightError):
            preflight.slot_resume_pc(p)

    def test_wrong_version_raises(self):
        p = make_pst(os.path.join(self.dir, "d.pst"), 0x1234, version=1)
        with self.assertRaises(preflight.PreflightError):
            preflight.slot_resume_pc(p)

    def test_wrong_first_section_raises(self):
        # A layout change that moved CPU out of first place must fail loudly,
        # not decode whatever happens to sit at the old offset.
        p = make_pst(os.path.join(self.dir, "e.pst"), 0x1234, tag=0x02)  # BS_SEC_RAM
        with self.assertRaises(preflight.PreflightError):
            preflight.slot_resume_pc(p)

    def test_wrong_section_len_raises(self):
        p = make_pst(os.path.join(self.dir, "f.pst"), 0x1234, seclen=600)
        with self.assertRaises(preflight.PreflightError):
            preflight.slot_resume_pc(p)

    def test_truncated_raises(self):
        p = make_pst(os.path.join(self.dir, "g.pst"), 0x1234, truncate=True)
        with self.assertRaises(preflight.PreflightError):
            preflight.slot_resume_pc(p)


class TestWorkspace(unittest.TestCase):
    """Checks over a synthetic workspace laid out like the real one."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = self.tmp.name
        self.addCleanup(self.tmp.cleanup)
        self.src = os.path.join(self.root, "runtime", "src")
        os.makedirs(self.src)
        os.makedirs(os.path.join(self.root, "build-dbg"))

    def write_src(self, text):
        p = os.path.join(self.src, "boot_state.c")
        with open(p, "w") as f:
            f.write(text)
        return p

    def write_binary(self, *, newer_than_src):
        p = os.path.join(self.root, preflight.RUNTIME_BINARY)
        with open(p, "wb") as f:
            f.write(b"\x7fELF")
        src = preflight.boot_state_source(self.root)
        st = os.path.getmtime(src)
        os.utime(p, (st + 10, st + 10) if newer_than_src else (st - 10, st - 10))
        return p

    def add_slot(self, slot, pc, entry_pc=0x80026C28):
        return make_pst(
            os.path.join(self.root, "build-dbg",
                         "state_%08X_slot%02d.pst" % (entry_pc, slot)),
            pc, entry_pc=entry_pc,
        )

    # -- form detection ---------------------------------------------------

    def test_detects_fixed_form(self):
        self.write_src(FIXED_SRC)
        self.assertEqual(preflight.runtime_form(self.root), "fixed")

    def test_detects_self_wiping_form(self):
        self.write_src(SELF_WIPING_SRC)
        self.assertEqual(preflight.runtime_form(self.root), "self-wiping")

    def test_restructured_source_is_unknown(self):
        self.write_src(RESTRUCTURED_SRC)
        self.assertEqual(preflight.runtime_form(self.root), "unknown")

    def test_missing_source_is_unknown(self):
        self.assertEqual(preflight.runtime_form(self.root), "unknown")

    # -- staleness --------------------------------------------------------

    def test_fresh_build_not_stale(self):
        self.write_src(FIXED_SRC)
        self.write_binary(newer_than_src=True)
        self.assertIs(preflight.build_is_stale(self.root), False)

    def test_old_binary_is_stale(self):
        self.write_src(FIXED_SRC)
        self.write_binary(newer_than_src=False)
        self.assertIs(preflight.build_is_stale(self.root), True)

    def test_missing_binary_is_unknown(self):
        self.write_src(FIXED_SRC)
        self.assertIsNone(preflight.build_is_stale(self.root))

    # -- the distinction this module exists for ---------------------------

    def test_good_runtime_is_clean(self):
        self.write_src(FIXED_SRC)
        self.write_binary(newer_than_src=True)
        self.assertEqual(preflight.check_runtime(self.root), [])

    def test_self_wiping_runtime_flagged(self):
        self.write_src(SELF_WIPING_SRC)
        self.write_binary(newer_than_src=True)
        problems = preflight.check_runtime(self.root)
        self.assertTrue(any("self-wiping" in p for p in problems))

    def test_stale_slot_is_not_a_runtime_problem(self):
        """A pc==0 slot on a correct build is a slot fault only - the runtime
        check must stay clean so the two never get conflated."""
        self.write_src(FIXED_SRC)
        self.write_binary(newer_than_src=True)
        self.add_slot(5, 0)
        self.assertEqual(preflight.check_runtime(self.root), [])
        problems = preflight.check_slot(self.root, 5)
        self.assertTrue(any("stale snapshot" in p for p in problems))
        self.assertTrue(any("NOT a runtime fault" in p for p in problems))

    def test_live_slot_on_good_build_is_clean(self):
        self.write_src(FIXED_SRC)
        self.write_binary(newer_than_src=True)
        self.add_slot(4, 0x80045690)
        self.assertEqual(preflight.diagnose(self.root, 4), [])

    def test_missing_slot_reported(self):
        self.write_src(FIXED_SRC)
        self.write_binary(newer_than_src=True)
        problems = preflight.check_slot(self.root, 9)
        self.assertTrue(any("no snapshot file" in p for p in problems))

    def test_known_slots_enumerated(self):
        self.write_src(FIXED_SRC)
        self.add_slot(2, 0x8005FCF4)
        self.add_slot(4, 0x80045690)
        self.add_slot(5, 0)
        self.assertEqual(preflight.known_slots(self.root), [2, 4, 5])

    def test_assert_ok_raises_on_bad_runtime(self):
        self.write_src(SELF_WIPING_SRC)
        self.write_binary(newer_than_src=True)
        with self.assertRaises(preflight.PreflightError):
            preflight.assert_ok(self.root)

    def test_source_found_via_psxrecomp_symlink_layout(self):
        """The build workspace reaches the runtime through psxrecomp/."""
        root2 = tempfile.mkdtemp(dir=self.root)
        nested = os.path.join(root2, "psxrecomp", "runtime", "src")
        os.makedirs(nested)
        with open(os.path.join(nested, "boot_state.c"), "w") as f:
            f.write(FIXED_SRC)
        self.assertEqual(preflight.runtime_form(root2), "fixed")


if __name__ == "__main__":
    unittest.main()


class TestApplier(unittest.TestCase):
    """The reapplication path, on synthetic sources only."""

    def setUp(self):
        sys.path.insert(0, str(Path(__file__).resolve().parent))
        global apply_boot_state_fix
        import apply_boot_state_fix  # noqa: F401
        self.mod = apply_boot_state_fix
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.src = os.path.join(self.tmp.name, "boot_state.c")

    def write(self, text):
        with open(self.src, "w") as f:
            f.write(text)

    def read(self):
        with open(self.src) as f:
            return f.read()

    def test_applies_to_stock(self):
        self.write(SELF_WIPING_SRC)
        self.assertEqual(self.mod.apply_fix(self.src), "applied")
        self.assertIn("c->pc ? c->pc : entry_pc", self.read())

    def test_apply_is_idempotent(self):
        self.write(SELF_WIPING_SRC)
        self.mod.apply_fix(self.src)
        once = self.read()
        self.assertEqual(self.mod.apply_fix(self.src), "already-applied")
        self.assertEqual(self.read(), once)

    def test_preserves_indentation(self):
        self.write("void f() {\n\t\tcpu->pc = entry_pc;   /* x */\n}\n")
        self.mod.apply_fix(self.src)
        line = [ln for ln in self.read().split("\n")
                if "c->pc ?" in ln][0]
        self.assertTrue(line.startswith("\t\t"))

    def test_round_trips_to_stock(self):
        self.write(SELF_WIPING_SRC)
        self.mod.apply_fix(self.src)
        self.mod.revert_fix(self.src)
        self.assertEqual(self.read().strip(), SELF_WIPING_SRC.strip())

    def test_revert_on_stock_is_noop(self):
        self.write(SELF_WIPING_SRC)
        self.assertEqual(self.mod.revert_fix(self.src), "already-stock")
        self.assertEqual(self.read(), SELF_WIPING_SRC)

    def test_refuses_when_anchor_missing(self):
        self.write(RESTRUCTURED_SRC)
        with self.assertRaises(SystemExit):
            self.mod.apply_fix(self.src)

    def test_refuses_when_anchor_ambiguous(self):
        self.write(SELF_WIPING_SRC + "\nvoid g() {\n    cpu->pc = entry_pc;\n}\n")
        with self.assertRaises(SystemExit):
            self.mod.apply_fix(self.src)

    def test_anchor_must_start_the_line(self):
        """The anchor is line-anchored, so an assignment buried mid-line (a
        brace-wrapped one-liner, or a mention inside a comment) is not counted
        as a second candidate."""
        self.write(SELF_WIPING_SRC + "\nvoid g() { cpu->pc = entry_pc; }\n")
        self.assertEqual(self.mod.apply_fix(self.src), "applied")

    def test_revert_leaves_foreign_comment_alone(self):
        self.write("void f() {\n"
                   "    /* somebody else's note */\n"
                   "    cpu->pc = c->pc ? c->pc : entry_pc;\n}\n")
        self.mod.revert_fix(self.src)
        self.assertIn("somebody else's note", self.read())
        self.assertIn("cpu->pc = entry_pc;", self.read())
