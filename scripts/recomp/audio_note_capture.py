#!/usr/bin/env python3
"""Capture a note-level BGM timeline from the recomp's SPU.

The retail sound driver's output, stripped of the mixer, is a stream of
*note events*: key-on with an ADPCM start address, a pitch, per-voice
volumes and an ADSR envelope; key-off; and the sample-loop edges the
hardware reports back. That stream is what the engine's own sequencer is
diffed against (``note_diff.py``) - it isolates "which notes were asked
for" from "how they were mixed", which is the split that matters when
notes go missing.

Two sources, both from the recomp runtime's always-on rings:

``--source events`` (default)
    The semantic key-on ring in ``runtime/src/spu.c``. Each entry already
    snapshots the voice registers at the moment of the event, and carries
    the real vblank frame stamp. Also reports ``END_LOOP`` / ``END_STOP``,
    which is how sample loop-point handling becomes observable.

``--source regs``
    Replays raw ``AUDIO_EV_REG_WRITE`` events from the audio trace ring
    through a shadow register file. Independent of the SPU model's own
    bookkeeping, so it cross-checks the semantic ring; it also preserves
    the *order* in which the driver programmed the voice registers, which
    the semantic ring flattens away.

**The headless trap this tool guards against.** ``spu_render()`` is driven
by the host audio pump, so a runtime started with ``--headless`` (no SDL
audio device) never clocks the SPU at all: ``render_frames`` stays 0, every
voice sits frozen at ``env_level == 0``, and no envelope ever decays. The
retail driver picks a free voice by polling CURVOL for ``env_level == 0``,
so against a frozen SPU it believes all 24 voices are free forever and
allocates voice 0 over and over. A capture taken that way looks plausible
and is entirely an artifact. This script therefore refuses to run unless
``spu_status.render_frames`` is advancing, unless ``--allow-unclocked`` is
passed explicitly.

To get a clocked instance without an audio device or a desktop::

    SDL_AUDIODRIVER=dummy xvfb-run -a \
        ./build-dbg/Legend_of_Legaia_Recompiled --debug-port 4472 \
        --no-launcher --bios SCPH1001.BIN --game game.toml

Wall-clock speed does not matter: what matters is that SPU frames advance
at exactly 735 per guest frame (44100/60), which puts the sequencer and the
envelopes in the same time base as retail even when the host runs slow.

Usage::

    python3 scripts/recomp/audio_note_capture.py --port 4472 \
        --seconds 30 --out /tmp/scratch/recomp_notes.jsonl --summary
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import probe  # noqa: E402

# -- SPU register map (see runtime/src/spu.c) ------------------------------

VOICE_BASE = 0x1F801C00
NUM_VOICES = 24
VOICE_STRIDE = 0x10

R_VOLL, R_VOLR, R_PITCH, R_START = 0x0, 0x2, 0x4, 0x6
R_ADSR1, R_ADSR2, R_ADSRVOL, R_REPEAT = 0x8, 0xA, 0xC, 0xE

KEY_ON_LO, KEY_ON_HI = 0x1F801D88, 0x1F801D8A
KEY_OFF_LO, KEY_OFF_HI = 0x1F801D8C, 0x1F801D8E
MAIN_VOL_L, MAIN_VOL_R = 0x1F801D80, 0x1F801D82

SPU_EVENTS_MAX = 4096  # handle_spu_events clamp
AUDIO_EVENTS_MAX = 8192  # handle_audio_events clamp

EV_NAME = {
    "KEYON": "on",
    "KEYOFF": "off",
    "END_LOOP": "end_loop",
    "END_STOP": "end_stop",
}


def _s16(value: int) -> int:
    value &= 0xFFFF
    return value - 0x10000 if value & 0x8000 else value


def _hexish(value) -> int:
    """The wire quotes hex fields; accept either form."""
    if isinstance(value, int):
        return value
    return int(value, 0)


# -- source: semantic key-on ring -----------------------------------------


def capture_events(
    client: probe.RecompClient, seconds: float, interval: float, backlog: bool
) -> tuple[list[dict], dict]:
    seen: dict[int, dict] = {}
    gaps = 0
    prev_hi = None
    start_seq = None

    if not backlog:
        first = client.call("spu_events", count=1)
        evs = first.get("events") or []
        start_seq = evs[-1]["seq"] if evs else -1

    deadline = time.monotonic() + seconds
    while True:
        r = client.call("spu_events", count=SPU_EVENTS_MAX)
        evs = r.get("events") or []
        if evs:
            if prev_hi is not None and evs[0]["seq"] > prev_hi + 1:
                gaps += evs[0]["seq"] - (prev_hi + 1)
            prev_hi = evs[-1]["seq"]
            for e in evs:
                seen[e["seq"]] = e
        if time.monotonic() >= deadline:
            break
        time.sleep(interval)

    notes = []
    for i, key in enumerate(sorted(seen)):
        e = seen[key]
        if start_seq is not None and e["seq"] <= start_seq:
            continue
        kind = EV_NAME.get(e["kind"])
        if kind is None:
            continue
        note = {
            "i": len(notes),
            "seq": e["seq"],
            "frame": e["frame"],
            "ev": kind,
            "v": e["v"],
            "addr": _hexish(e["addr"]),
        }
        if kind == "on":
            note.update(
                pitch=_hexish(e["pitch"]),
                voll=_s16(_hexish(e["vol_l"])),
                volr=_s16(_hexish(e["vol_r"])),
                adsr1=_hexish(e["adsr_lo"]),
                adsr2=_hexish(e["adsr_hi"]),
            )
        notes.append(note)
    return notes, {"raw_events": len(seen), "ring_gap_events": gaps}


# -- source: raw register writes ------------------------------------------


def voice_of(addr: int):
    if VOICE_BASE <= addr < VOICE_BASE + NUM_VOICES * VOICE_STRIDE:
        rel = addr - VOICE_BASE
        return rel // VOICE_STRIDE, rel % VOICE_STRIDE
    return None


class ShadowSpu:
    """Replays register writes so a bare KEYON bit resolves to a full note.

    A key-on carries no payload; the note's identity lives in the voice
    registers programmed before it. Holding that shadow state is what turns
    a KEYON mask into a described note.
    """

    def __init__(self) -> None:
        self.regs = [dict() for _ in range(NUM_VOICES)]
        self.main_vol = [0, 0]
        self.notes: list[dict] = []

    def apply(self, addr: int, value: int, seq: int, frame) -> None:
        value &= 0xFFFF
        split = voice_of(addr)
        if split is not None:
            self.regs[split[0]][split[1]] = value
            return
        if addr == MAIN_VOL_L:
            self.main_vol[0] = _s16(value)
        elif addr == MAIN_VOL_R:
            self.main_vol[1] = _s16(value)
        elif addr in (KEY_ON_LO, KEY_ON_HI):
            self._key(value, 0 if addr == KEY_ON_LO else 16, "on", seq, frame)
        elif addr in (KEY_OFF_LO, KEY_OFF_HI):
            self._key(value, 0 if addr == KEY_OFF_LO else 16, "off", seq, frame)

    def _key(self, mask, base, kind, seq, frame) -> None:
        for bit in range(16):
            if not mask & (1 << bit):
                continue
            v = base + bit
            if v >= NUM_VOICES:
                continue
            r = self.regs[v]
            note = {
                "i": len(self.notes),
                "seq": seq,
                "frame": frame,
                "ev": kind,
                "v": v,
                "addr": r.get(R_START, 0) * 8,
            }
            if kind == "on":
                note.update(
                    repeat=r.get(R_REPEAT, 0) * 8,
                    pitch=r.get(R_PITCH, 0),
                    voll=_s16(r.get(R_VOLL, 0)),
                    volr=_s16(r.get(R_VOLR, 0)),
                    adsr1=r.get(R_ADSR1, 0),
                    adsr2=r.get(R_ADSR2, 0),
                )
            self.notes.append(note)


def capture_regs(
    client: probe.RecompClient, seconds: float, interval: float, backlog: bool
) -> tuple[list[dict], dict]:
    seen: dict[int, dict] = {}
    gaps = 0
    prev_hi = None
    start_seq = None
    if not backlog:
        first = client.call("audio_events", count=1)
        evs = first.get("events") or []
        start_seq = evs[-1]["seq"] if evs else -1

    marks: list[tuple[int, int]] = []
    deadline = time.monotonic() + seconds
    while True:
        try:
            frame = client.frame()
        except Exception:
            frame = None
        r = client.call("audio_events", count=AUDIO_EVENTS_MAX)
        evs = r.get("events") or []
        if evs:
            if prev_hi is not None and evs[0]["seq"] > prev_hi + 1:
                gaps += evs[0]["seq"] - (prev_hi + 1)
            prev_hi = evs[-1]["seq"]
            for e in evs:
                seen[e["seq"]] = e
            if frame is not None:
                marks.append((evs[-1]["seq"], frame))
        if time.monotonic() >= deadline:
            break
        time.sleep(interval)

    def frame_for(seq):
        last = None
        for hi, fr in marks:
            if seq <= hi:
                return fr
            last = fr
        return last

    spu = ShadowSpu()
    for key in sorted(seen):
        e = seen[key]
        if start_seq is not None and e["seq"] <= start_seq:
            continue
        if e.get("kind") != "REG":
            continue
        spu.apply(int(e["a"], 16), int(e["b"], 16), e["seq"], frame_for(e["seq"]))
    return spu.notes, {"raw_events": len(seen), "ring_gap_events": gaps}


# -- reporting ------------------------------------------------------------


def summarize(notes: list[dict]) -> str:
    by_kind = collections.Counter(n["ev"] for n in notes)
    ons = [n for n in notes if n["ev"] == "on"]
    per_voice = collections.Counter(n["v"] for n in ons)
    per_addr = collections.Counter(n["addr"] for n in ons)
    lines = [
        "events        : " + ", ".join(f"{k}={v}" for k, v in sorted(by_kind.items())),
        f"voices used   : {len(per_voice)} of {NUM_VOICES}",
        f"distinct VAGs : {len(per_addr)} (by SPU start address)",
    ]
    if ons:
        lines.append(f"frame range   : {ons[0]['frame']} -> {ons[-1]['frame']}")
    lines.append("  top VAGs by note count:")
    for addr, n in per_addr.most_common(12):
        lines.append(f"    0x{addr:06X}  x{n}")
    lines.append("  notes per voice:")
    lines.append(
        "    " + " ".join(f"v{v}={per_voice.get(v, 0)}" for v in range(NUM_VOICES))
    )
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument(
        "--port", type=int, default=int(os.environ.get("LEGAIA_RECOMP_PORT", "4471"))
    )
    ap.add_argument("--source", choices=("events", "regs"), default="events")
    ap.add_argument("--seconds", type=float, default=20.0)
    ap.add_argument("--interval", type=float, default=1.0)
    ap.add_argument(
        "--include-backlog",
        action="store_true",
        help="also decode events already in the ring at capture start",
    )
    ap.add_argument(
        "--allow-unclocked",
        action="store_true",
        help="skip the render_frames guard (the capture will be an artifact)",
    )
    ap.add_argument("--out", default="-")
    ap.add_argument("--summary", action="store_true")
    args = ap.parse_args(argv)

    client = probe.RecompClient(host=args.host, port=args.port)

    # The headless trap: an unclocked SPU makes every voice look free.
    r0 = client.call("spu_status")["render_frames"]
    time.sleep(0.5)
    r1 = client.call("spu_status")["render_frames"]
    if r1 <= r0 and not args.allow_unclocked:
        sys.stderr.write(
            "REFUSING: spu_status.render_frames is not advancing "
            f"({r0} -> {r1}). The SPU is not being clocked, so every voice "
            "reads env_level==0 and the guest driver believes all 24 voices "
            "are free - any capture is an artifact. Start the runtime with an "
            "audio pump (SDL_AUDIODRIVER=dummy + xvfb-run, no --headless), or "
            "pass --allow-unclocked if you truly want the raw command stream.\n"
        )
        return 2

    try:
        scene, mode = client.scene_name(), client.game_mode()
    except Exception:
        scene, mode = None, None

    fn = capture_events if args.source == "events" else capture_regs
    notes, meta = fn(client, args.seconds, args.interval, args.include_backlog)

    header = {
        "kind": "header",
        "source": "recomp",
        "ring": args.source,
        "scene": scene,
        "mode": mode,
        "seconds": args.seconds,
        "spu_clocked": True,
        **meta,
    }
    out = sys.stdout if args.out == "-" else open(args.out, "w")
    try:
        out.write(json.dumps(header) + "\n")
        for n in notes:
            out.write(json.dumps(n) + "\n")
    finally:
        if out is not sys.stdout:
            out.close()

    if args.summary:
        sys.stderr.write(summarize(notes) + "\n")
    if meta["ring_gap_events"]:
        sys.stderr.write(
            f"WARNING: {meta['ring_gap_events']} events lost to ring wrap; "
            "lower --interval\n"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
