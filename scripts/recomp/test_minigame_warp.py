#!/usr/bin/env python3
"""Synthetic-fixture tests for minigame_warp (no server, no game data).

Locks the byte-level shape of the op-0x3E door-warp replay: which globals
are written, the mode word coming last, the player-flag clear, and the
settle-poll semantics.

    cd scripts/recomp && python3 -m unittest test_minigame_warp
"""

import unittest

import minigame_warp as mw


class FakeClient:
    """Collects write_ram calls; serves reads from a fixture RAM dict."""

    def __init__(self, ram_u32=None, modes=None):
        self.ram_u32 = dict(ram_u32 or {})
        self.writes = []  # (addr, byte) in call order
        self._modes = list(modes or [])

    def call(self, cmd, **params):
        assert cmd == "write_ram", cmd
        self.writes.append((int(params["addr"], 16), int(params["val"], 16)))
        return {"ok": True}

    def read_u32(self, addr):
        return self.ram_u32.get(addr, 0)

    def game_mode(self):
        return self._modes.pop(0) if len(self._modes) > 1 else self._modes[0]


def bytes_written(writes, addr, n):
    """Reassemble the little-endian value written at addr..addr+n-1."""
    by_addr = dict(writes)
    return sum(by_addr[addr + i] << (8 * i) for i in range(n))


class WarpWrites(unittest.TestCase):
    def test_dome_globals(self):
        w = mw.warp_byte_writes(5)
        self.assertEqual(bytes_written(w, mw.SUB_ID_ADDR, 4), 5)
        self.assertEqual(bytes_written(w, mw.WINNINGS_ADDR, 4), 0)
        self.assertEqual(bytes_written(w, mw.AUX_ADDR, 4), 0)
        self.assertEqual(bytes_written(w, mw.MODE_WORD_ADDR, 2), 0x18)

    def test_mode_word_is_last(self):
        w = mw.warp_byte_writes(5)
        mode_positions = [i for i, (a, _) in enumerate(w)
                          if mw.MODE_WORD_ADDR <= a < mw.MODE_WORD_ADDR + 2]
        self.assertEqual(mode_positions, [len(w) - 2, len(w) - 1])

    def test_sub_id_parametrises(self):
        w = mw.warp_byte_writes(3)
        self.assertEqual(bytes_written(w, mw.SUB_ID_ADDR, 4), 3)


class PerformWarp(unittest.TestCase):
    def test_clears_player_flag_bit(self):
        player = 0x80083794
        c = FakeClient(ram_u32={
            mw.PLAYER_PTR_ADDR: player,
            player + 0x10: 0x09820880 | mw.PLAYER_FLAG_BIT,
        })
        mw.perform_warp(c, 5)
        self.assertEqual(bytes_written(c.writes, player + 0x10, 4), 0x09820880)

    def test_skips_flag_write_when_clear(self):
        player = 0x80083794
        c = FakeClient(ram_u32={
            mw.PLAYER_PTR_ADDR: player,
            player + 0x10: 0x09820880,  # bit already clear
        })
        mw.perform_warp(c, 5)
        touched = {a for a, _ in c.writes}
        self.assertNotIn(player + 0x10, touched)

    def test_skips_flag_chase_on_bad_pointer(self):
        c = FakeClient(ram_u32={mw.PLAYER_PTR_ADDR: 0})
        mw.perform_warp(c, 5)  # must not chase a null pointer
        self.assertEqual(bytes_written(c.writes, mw.SUB_ID_ADDR, 4), 5)


class WaitSettle(unittest.TestCase):
    def test_returns_observed_chain(self):
        # 0x18 -> 0x19 -> 0x14 -> 0x15 then stable (the dome chain).
        c = FakeClient(modes=[0x18, 0x18, 0x19, 0x19, 0x14, 0x15,
                              0x15, 0x15, 0x15, 0x15])
        chain = mw.wait_settle(c, timeout=30.0, stable_samples=3,
                               sleep=lambda _t: None)
        self.assertEqual(chain, [0x18, 0x19, 0x14, 0x15])

    def test_does_not_settle_on_other_run_mode(self):
        # A chain parked in 0x19 (OTHER MODE run) must keep polling, not
        # report 0x19 as settled.
        c = FakeClient(modes=[0x18] + [0x19] * 8 + [0x15] * 6)
        chain = mw.wait_settle(c, timeout=30.0, stable_samples=3,
                               sleep=lambda _t: None)
        self.assertEqual(chain[-1], 0x15)


if __name__ == "__main__":
    unittest.main()
