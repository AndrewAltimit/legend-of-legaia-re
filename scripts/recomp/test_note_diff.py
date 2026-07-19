#!/usr/bin/env python3
"""Synthetic fixtures locking `note_diff.py`'s alignment + comparison rules.

Pure python, no disc data and no captures - the fixtures here are the only
note-trace-shaped data in the repo, exactly as `test_trace_diff.py` is for
the state-trace diff. Real captures are Sony-derived and never committed.

Run with::

    cd scripts/recomp && python3 -m unittest test_note_diff
"""

import json
import os
import tempfile
import unittest

import note_diff


def write_trace(path, notes, source="synthetic"):
    """Write a canonical note JSONL. `notes` are (addr, pitch, voice, vol)."""
    with open(path, "w") as fh:
        fh.write(json.dumps({"kind": "header", "source": source}) + "\n")
        for i, (addr, pitch, voice, vol) in enumerate(notes):
            fh.write(
                json.dumps(
                    {
                        "i": i,
                        "frame": i,
                        "ev": "on",
                        "v": voice,
                        "addr": addr,
                        "pitch": pitch,
                        "voll": vol,
                        "volr": 0,
                    }
                )
                + "\n"
            )


class NoteDiffTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()

    def path(self, name):
        return os.path.join(self.dir, name)

    # -- VAG normalisation -------------------------------------------------

    def test_addresses_normalise_to_dense_vag_ids_by_ascending_address(self):
        """Raw SPU addresses never match across sides; ascending order does."""
        p = self.path("a.jsonl")
        write_trace(p, [(0x9000, 1, 0, 10), (0x1000, 2, 1, 10), (0x5000, 3, 2, 10)])
        _, ons = note_diff.load(p)
        self.assertEqual([o["vag"] for o in ons], [2, 0, 1])

    def test_two_sides_with_different_base_addresses_still_match(self):
        a, b = self.path("a.jsonl"), self.path("b.jsonl")
        # Same tones, same order, wildly different allocator layouts.
        write_trace(a, [(0x1000, 60, 0, 10), (0x2000, 62, 1, 10)])
        write_trace(b, [(0x40000, 60, 0, 10), (0x51230, 62, 1, 10)])
        _, ona = note_diff.load(a)
        _, onb = note_diff.load(b)
        res = note_diff.compare(ona, onb, 0, 0)
        self.assertEqual(res["first"], {})

    # -- alignment ---------------------------------------------------------

    def test_auto_alignment_finds_the_lead_in_offset(self):
        a, b = self.path("a.jsonl"), self.path("b.jsonl")
        shared = [(0x1000, 60, 0, 10), (0x2000, 62, 1, 10), (0x3000, 64, 2, 10)]
        write_trace(a, shared)
        # B carries two extra lead-in notes before the shared run.
        write_trace(b, [(0x9000, 99, 5, 10), (0x9000, 98, 5, 10)] + shared)
        _, ona = note_diff.load(a)
        _, onb = note_diff.load(b)
        self.assertEqual(note_diff.best_offset(ona, onb, 16), 2)

    def test_explicit_offset_is_honoured_over_auto(self):
        a, b = self.path("a.jsonl"), self.path("b.jsonl")
        write_trace(a, [(0x1000, 60, 0, 10)])
        write_trace(b, [(0x1000, 60, 0, 10)])
        _, ona = note_diff.load(a)
        _, onb = note_diff.load(b)
        # Offsetting past the end leaves nothing to compare.
        res = note_diff.compare(ona, onb[5:], 0, 0)
        self.assertEqual(res["overlap"], 0)

    # -- comparison --------------------------------------------------------

    def test_a_missing_note_shears_the_sequence_from_that_point(self):
        """The signature of a dropped note: the sequences shear from there.

        Note what this does NOT catch. B is missing the 0x2000 tone entirely,
        so B's dense VAG ids renumber and B's 0x3000 takes the id A gave to
        0x2000 - the `vag` channel spuriously agrees. `pitch` is what actually
        exposes the drop, which is why the tool warns when the two sides'
        distinct-VAG counts differ.
        """
        a, b = self.path("a.jsonl"), self.path("b.jsonl")
        write_trace(
            a,
            [(0x1000, 60, 0, 10), (0x2000, 62, 1, 10), (0x3000, 64, 2, 10)],
        )
        # B drops the middle note entirely.
        write_trace(b, [(0x1000, 60, 0, 10), (0x3000, 64, 2, 10)])
        _, ona = note_diff.load(a)
        _, onb = note_diff.load(b)
        res = note_diff.compare(ona, onb, 0, 0)
        self.assertEqual(res["first"].get("pitch"), 1)
        self.assertNotIn("vag", res["first"])

    def test_distinct_vag_counts_are_exposed_for_the_comparability_guard(self):
        a, b = self.path("a.jsonl"), self.path("b.jsonl")
        write_trace(a, [(0x1000, 60, 0, 10), (0x2000, 62, 1, 10)])
        write_trace(b, [(0x1000, 60, 0, 10)])
        _, ona = note_diff.load(a)
        _, onb = note_diff.load(b)
        self.assertEqual(ona[0]["n_vags"], 2)
        self.assertEqual(onb[0]["n_vags"], 1)

    def test_pitch_tolerance_is_respected(self):
        a, b = self.path("a.jsonl"), self.path("b.jsonl")
        write_trace(a, [(0x1000, 1000, 0, 10)])
        write_trace(b, [(0x1000, 1004, 0, 10)])
        _, ona = note_diff.load(a)
        _, onb = note_diff.load(b)
        self.assertIn("pitch", note_diff.compare(ona, onb, 0, 0)["first"])
        self.assertNotIn("pitch", note_diff.compare(ona, onb, 8, 0)["first"])

    def test_volume_is_summed_across_both_channels(self):
        """A note keyed on at zero volume is the 'emitted but inaudible' case."""
        p = self.path("a.jsonl")
        write_trace(p, [(0x1000, 60, 0, -30)])
        _, ons = note_diff.load(p)
        self.assertEqual(ons[0]["vol"], 30)

    def test_identical_traces_report_no_divergence(self):
        a, b = self.path("a.jsonl"), self.path("b.jsonl")
        notes = [(0x1000, 60, 0, 10), (0x2000, 62, 1, 20)]
        write_trace(a, notes)
        write_trace(b, notes)
        _, ona = note_diff.load(a)
        _, onb = note_diff.load(b)
        self.assertEqual(note_diff.compare(ona, onb, 0, 0)["first"], {})

    def test_note_off_events_are_excluded_from_the_note_sequence(self):
        p = self.path("a.jsonl")
        with open(p, "w") as fh:
            fh.write(json.dumps({"kind": "header", "source": "s"}) + "\n")
            fh.write(
                json.dumps(
                    {"i": 0, "ev": "on", "v": 0, "addr": 16, "pitch": 1, "voll": 1}
                )
                + "\n"
            )
            fh.write(json.dumps({"i": 1, "ev": "off", "v": 0, "addr": 16}) + "\n")
        _, ons = note_diff.load(p)
        self.assertEqual(len(ons), 1)


if __name__ == "__main__":
    unittest.main()
