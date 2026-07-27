"""Unit tests for xa_cue_capture's pure decode/detect layer.

Synthetic fixtures only - the byte patterns model the observed global
lifecycle (arm -> poll -> done -> stop), no Sony data.
"""

import unittest

import xa_cue_capture as xc


def region_a(msf=b"\x00\x00\x00", play_state=0, dur=0, filter_chan=0):
    buf = bytearray(128)
    buf[xc.FILTER_CHAN - xc.REGION_A] = filter_chan
    buf[xc.STAGED_MSF - xc.REGION_A : xc.STAGED_MSF - xc.REGION_A + 3] = msf
    buf[xc.PLAY_STATE - xc.REGION_A : xc.PLAY_STATE - xc.REGION_A + 4] = (
        play_state.to_bytes(4, "little")
    )
    buf[xc.DUR - xc.REGION_A : xc.DUR - xc.REGION_A + 4] = dur.to_bytes(4, "little")
    return bytes(buf)


def region_b(sm=0, chan=0, start=0, end=0):
    buf = bytearray(128)
    buf[xc.SM_STATE - xc.REGION_B : xc.SM_STATE - xc.REGION_B + 4] = sm.to_bytes(
        4, "little"
    )
    buf[xc.CHANNEL - xc.REGION_B : xc.CHANNEL - xc.REGION_B + 4] = chan.to_bytes(
        4, "little"
    )
    buf[xc.START_LBA - xc.REGION_B : xc.START_LBA - xc.REGION_B + 4] = start.to_bytes(
        4, "little"
    )
    buf[xc.END_LBA - xc.REGION_B : xc.END_LBA - xc.REGION_B + 4] = end.to_bytes(
        4, "little"
    )
    return bytes(buf)


def frame(f, msf=b"\x00\x00\x00", ps=0, dur=0, sm=0, chan=0, start=0, end=0):
    return xc.decode_frame(f, region_a(msf, ps, dur), region_b(sm, chan, start, end), None)


# A two-slot clip table: slot 0 at MSF 00:02:00 (LBA 0), slot 1 at
# BCD 13:28:63 (the shape a real staged CdlLOC has).
def clip_table_raw():
    buf = bytearray(xc.CLIP_TABLE_SLOTS * 8)
    buf[0:3] = b"\x00\x02\x00"
    buf[4:8] = (4096).to_bytes(4, "little")
    buf[8:11] = b"\x13\x28\x63"
    buf[12:16] = (2048).to_bytes(4, "little")
    return bytes(buf)


class MsfTests(unittest.TestCase):
    def test_bcd(self):
        self.assertEqual(xc.bcd(0x00), 0)
        self.assertEqual(xc.bcd(0x59), 59)
        self.assertEqual(xc.bcd(0x13), 13)

    def test_msf_to_lba(self):
        # 00:02:00 is LBA 0 (the 150-sector lead-in).
        self.assertEqual(xc.msf_to_lba(b"\x00\x02\x00"), 0)
        # BCD 13:28:63 = 13*4500 + 28*75 + 63 - 150 = 60513.
        self.assertEqual(xc.msf_to_lba(b"\x13\x28\x63"), 60513)


class ClipTableTests(unittest.TestCase):
    def test_parse_skips_empty_slots(self):
        table = xc.parse_clip_table(clip_table_raw())
        self.assertEqual(len(table), 2)
        self.assertEqual(table[0]["file"], "XA1.XA")
        self.assertEqual(table[0]["lba"], 0)
        self.assertEqual(table[1]["file"], "XA2.XA")
        self.assertEqual(table[1]["lba"], 60513)

    def test_slot_for_msf_exact_match(self):
        table = xc.parse_clip_table(clip_table_raw())
        self.assertEqual(xc.slot_for_msf(table, b"\x13\x28\x63")["slot"], 1)
        self.assertIsNone(xc.slot_for_msf(table, b"\x13\x28\x62"))


class DetectTests(unittest.TestCase):
    def setUp(self):
        self.table = xc.parse_clip_table(clip_table_raw())

    def test_fresh_cue_on_play_state_edge(self):
        frames = [
            frame(10),
            frame(11, msf=b"\x13\x28\x63", ps=2, dur=75, sm=1, chan=14, start=60513, end=60702),
            frame(12, msf=b"\x13\x28\x63", ps=2, dur=75, sm=8, chan=14, start=60514, end=60702),
        ]
        cues = xc.detect_cues(frames, self.table)
        self.assertEqual(len(cues), 1)
        self.assertEqual(cues[0]["frame"], 11)
        self.assertEqual(cues[0]["file"], "XA2.XA")
        self.assertEqual(cues[0]["channel"], 14)
        self.assertEqual(cues[0]["dur"], 75)

    def test_end_of_playback_is_not_a_cue(self):
        # The stop path zeroes play_state and sm on the same frame - the
        # false-duplicate shape the ps==2 gate exists for.
        frames = [
            frame(10, msf=b"\x13\x28\x63", ps=2, dur=75, sm=8, chan=14, start=60700, end=60702),
            frame(11, msf=b"\x13\x28\x63", ps=2, dur=75, sm=10, chan=14, start=60703, end=60702),
            frame(12, msf=b"\x13\x28\x63", ps=0, dur=75, sm=0, chan=14, start=60703, end=1000000),
        ]
        self.assertEqual(xc.detect_cues(frames, self.table), [])

    def test_back_to_back_cue_via_tuple_change(self):
        # A second cue fired while play_state never left 2.
        frames = [
            frame(10, msf=b"\x13\x28\x63", ps=2, dur=75, sm=8, chan=14, start=60600, end=60702),
            frame(11, msf=b"\x00\x02\x00", ps=2, dur=357, sm=1, chan=2, start=0, end=894),
        ]
        cues = xc.detect_cues(frames, self.table)
        self.assertEqual(len(cues), 1)
        self.assertEqual(cues[0]["file"], "XA1.XA")
        self.assertEqual(cues[0]["channel"], 2)

    def test_identical_refire_via_sm_restart(self):
        # Same tuple, but the SM restarted while play_state stayed 2 -
        # a genuine re-fire (e.g. a looped stream).
        frames = [
            frame(10, msf=b"\x00\x02\x00", ps=2, dur=357, sm=8, chan=2, start=800, end=894),
            frame(11, msf=b"\x00\x02\x00", ps=2, dur=357, sm=1, chan=2, start=0, end=894),
        ]
        cues = xc.detect_cues(frames, self.table)
        self.assertEqual(len(cues), 1)
        self.assertEqual(cues[0]["frame"], 11)

    def test_mid_flight_cue_at_capture_start_not_reported(self):
        frames = [
            frame(10, msf=b"\x13\x28\x63", ps=2, dur=75, sm=8, chan=14, start=60600, end=60702),
            frame(11, msf=b"\x13\x28\x63", ps=2, dur=75, sm=7, chan=14, start=60602, end=60702),
        ]
        self.assertEqual(xc.detect_cues(frames, self.table), [])

    def test_queue_bytes_ride_along(self):
        actor = bytearray(128)
        actor[0x1DF - xc.ACTOR_WIN_OFF : 0x1DF - xc.ACTOR_WIN_OFF + 4] = bytes(
            [0x0F, 0x0E, 0x19, 0x27]
        )
        f1 = xc.decode_frame(10, region_a(), region_b(), bytes(actor))
        f2 = xc.decode_frame(
            11,
            region_a(b"\x13\x28\x63", 2, 75),
            region_b(1, 14, 60513, 60702),
            bytes(actor),
        )
        cues = xc.detect_cues([f1, f2], self.table)
        self.assertEqual(len(cues), 1)
        self.assertTrue(cues[0]["queue"].startswith("0f0e1927"))


if __name__ == "__main__":
    unittest.main()
