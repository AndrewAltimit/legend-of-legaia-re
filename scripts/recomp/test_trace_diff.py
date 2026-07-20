#!/usr/bin/env python3
"""Unit tests for trace_diff alignment + channel comparison.

Pure-python, synthetic fixtures only (no game data). Run with:

    python3 -m unittest scripts/recomp/test_trace_diff.py
    (or: cd scripts/recomp && python3 -m unittest test_trace_diff)
"""

import io
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import trace_diff  # noqa: E402


def synth_trace(start_frame, n, cam_change_at, yaw_fn):
    """Build {frame: record} with a camera cut at ``cam_change_at`` (index
    into the trace) and per-frame yaw from ``yaw_fn(i)``."""
    frames = {}
    for i in range(n):
        f = start_frame + i
        pitch = 32 if i < cam_change_at else 96
        frames[f] = {
            "frame": f,
            "scene": "synth",
            "mode": 3,
            "cam": {
                "pitch": pitch,
                "yaw": yaw_fn(i) % 4096,
                "roll": 0,
                "h": 256,
                "eye": [0, 1280, 7920],
                "focus": [0, 0, 0],
            },
            "player": {"x": 100 + i, "z": 200, "heading": 0},
        }
    return frames


class Args:
    tol_angle = 0.0
    tol_pos = 0.0
    tol_h = 0.0


class AutoAlignTest(unittest.TestCase):
    def test_auto_offset_from_first_camera_change(self):
        # Same content, different frame-counter origins: A cuts at frame
        # 1010, B (origin 5000, cut index 10) at 5010 -> offset +4000.
        a = synth_trace(1000, 40, cam_change_at=10, yaw_fn=lambda i: 3000 - 2 * i)
        b = synth_trace(5000, 40, cam_change_at=10, yaw_fn=lambda i: 3000 - 2 * i)
        self.assertEqual(trace_diff.auto_offset(a, b), 4000)

    def test_auto_offset_none_without_camera_change(self):
        a = synth_trace(0, 10, cam_change_at=99, yaw_fn=lambda i: 0)
        b = synth_trace(0, 10, cam_change_at=99, yaw_fn=lambda i: 0)
        self.assertIsNone(trace_diff.auto_offset(a, b))

    def test_aligned_identical_traces_report_no_divergence(self):
        a = synth_trace(1000, 40, cam_change_at=10, yaw_fn=lambda i: 3000 - 2 * i)
        b = synth_trace(5000, 40, cam_change_at=10, yaw_fn=lambda i: 3000 - 2 * i)
        out = io.StringIO()
        divergent = trace_diff.diff_traces(a, b, 4000, Args(), out=out)
        self.assertEqual(divergent, 0, out.getvalue())

    def test_misaligned_traces_diverge(self):
        a = synth_trace(1000, 40, cam_change_at=10, yaw_fn=lambda i: 3000 - 2 * i)
        b = synth_trace(5000, 40, cam_change_at=10, yaw_fn=lambda i: 3000 - 2 * i)
        out = io.StringIO()
        divergent = trace_diff.diff_traces(a, b, 3999, Args(), out=out)
        self.assertGreater(divergent, 0)


class ChannelCompareTest(unittest.TestCase):
    def test_first_divergence_frame_and_exit_signal(self):
        a = synth_trace(0, 30, cam_change_at=5, yaw_fn=lambda i: 100)
        # B matches A until index 20, then yaw walks off.
        b = synth_trace(0, 30, cam_change_at=5, yaw_fn=lambda i: 100 + max(0, i - 19) * 8)
        out = io.StringIO()
        divergent = trace_diff.diff_traces(a, b, 0, Args(), out=out)
        report = out.getvalue()
        self.assertEqual(divergent, 1)
        self.assertIn("DIVERGE cam.yaw at B frame 20", report)
        # Context window shows frames around the divergence.
        self.assertIn("B frame 15", report)
        self.assertIn("B frame 25", report)

    def test_angle_wraparound_distance(self):
        # 4090 vs 6 is 12 apart across the wrap, not 4084.
        self.assertEqual(trace_diff.channel_distance("cam.yaw", 4090, 6), 12)
        self.assertEqual(trace_diff.channel_distance("player.heading", 0, 4095), 1)
        # Position channels use plain absolute distance.
        self.assertEqual(trace_diff.channel_distance("player.x", 10, -10), 20)

    def test_tolerance_suppresses_small_angle_noise(self):
        a = synth_trace(0, 20, cam_change_at=3, yaw_fn=lambda i: 100)
        b = synth_trace(0, 20, cam_change_at=3, yaw_fn=lambda i: 101)  # 1 unit off

        class Tol(Args):
            tol_angle = 2.0

        out = io.StringIO()
        self.assertEqual(trace_diff.diff_traces(a, b, 0, Tol(), out=out), 0)
        out = io.StringIO()
        self.assertEqual(trace_diff.diff_traces(a, b, 0, Args(), out=out), 1)

    def test_scene_and_mode_compare_exactly(self):
        a = synth_trace(0, 10, cam_change_at=2, yaw_fn=lambda i: 0)
        b = synth_trace(0, 10, cam_change_at=2, yaw_fn=lambda i: 0)
        for f in list(b)[5:]:
            b[f] = dict(b[f], scene="other")
        out = io.StringIO()
        divergent = trace_diff.diff_traces(a, b, 0, Args(), out=out)
        self.assertEqual(divergent, 1)
        self.assertIn("DIVERGE scene at B frame 5", out.getvalue())

    def test_angle_distance_reduces_before_measuring(self):
        # A capture that forwards retail's raw u16 angle word (rather than
        # masking to 12 bits) must still measure correctly. Before the
        # reduction, ``ANGLE_MOD - d`` went negative and min() picked it, so
        # the channel silently reported OK for every frame.
        d_same = trace_diff.channel_distance("cam.yaw", 65081, 65081 & 0xFFF)
        self.assertEqual(d_same, 0)
        # Genuinely different angles must still register a real distance.
        d_diff = trace_diff.channel_distance("cam.yaw", 65081, 3000)
        self.assertGreater(d_diff, 0)
        self.assertLessEqual(d_diff, trace_diff.ANGLE_MOD // 2)
        # No angle distance may ever be negative, whatever the input.
        for a, b in ((0, 0), (65535, 1), (4096, 0), (100000, 7)):
            self.assertGreaterEqual(trace_diff.channel_distance("cam.yaw", a, b), 0)

    def test_absent_fields_are_skipped_not_divergent(self):
        # A has player, B doesn't: player.* is A-only, reported as skipped.
        a = synth_trace(0, 10, cam_change_at=2, yaw_fn=lambda i: 0)
        b = synth_trace(0, 10, cam_change_at=2, yaw_fn=lambda i: 0)
        for f in b:
            b[f] = {k: v for k, v in b[f].items() if k != "player"}
        out = io.StringIO()
        divergent = trace_diff.diff_traces(a, b, 0, Args(), out=out)
        self.assertEqual(divergent, 0)
        self.assertIn("only in trace A", out.getvalue())


if __name__ == "__main__":
    unittest.main()
