#!/usr/bin/env python3
"""Capture per-art CD-XA voice-shout cues from the recomp runtime.

The arts-voice selector ``FUN_8004C140(char_id, action_constant, flag)``
picks a channel from the art's candidate pool and fires the XA clip starter
``FUN_8003D53C(clip_slot, channel, dur)``. The starter stages the cue in a
cluster of SCUS-static globals (gp = ``0x8007B318``) before arming the
CdlSetfilter CdSync state machine ``FUN_8003D764`` - those globals are the
observation point, no breakpoint needed:

======================  =====================================================
``0x8007BBF0``          staged ``CdlLOC`` (BCD MSF) of the clip start, copied
                        verbatim from the clip-table slot (identifies the file)
``0x8007BC20``          gp+0x908 XA play state - ``2`` written per accepted cue
``0x8007BC30``          gp+0x918 ``dur`` argument (read span; clamped 0x2A30)
``0x8007BC40``          gp+0x928 CdSync SM state (0/1 at cue arm, climbs to the
                        7<->8 polling pair, 10 = done)
``0x8007BC6C``          gp+0x954 ``channel`` argument (the pool pick)
``0x8007BC88``          gp+0x970 start LBA (GetlocP-updated DURING playback -
                        only the cue-frame value is the clip start)
``0x8007BC8C``          gp+0x974 end LBA
``0x8007BBC1``          ``CdlSetfilter`` chan byte (written by SM state 3)
``0x8007BD62``          gp+0xa4a last-picked shout channel (the avoid-repeat
                        memory; only the FUN_8004C140 path updates it)
``0x8007BD24``          gp+0xa0c context ptr; byte ``ctx+0x243`` selects the
                        first-half candidate-table variant in FUN_8004C140
``0x801C6ED8``          34-slot clip table ``[CdlLOC][u32 byte_len]``;
                        slot ``i`` = file ``XA<i+1>.XA`` (zero len = empty)
======================  =====================================================

Provenance: ``ghidra/scripts/funcs/8003d53c.txt`` / ``8003d764.txt`` /
``8004c140.txt``; ``docs/subsystems/cutscene.md#xa-channel-selection``;
gp base pinned in ``docs/reference/memory-map.md``.

Mechanism: the per-frame snapshot ring (``set_snapshot`` /
``read_frame_ram``) records the two 128-byte regions covering the cluster
every frame; after the capture window elapses the ring is harvested and a
**cue edge** is any frame where the staged ``(msf, chan, dur)`` tuple
changes, or where the SM state restarts (drops to <= 1 from a higher state -
catches an identical cue re-fired). Each cue is resolved to its
``XA<n>.XA`` file through the clip table read once at startup.

Optionally (``--actor-slot N``) the fourth ring region snapshots that party
battle actor's ``+0x1D8..+0x258`` window (pointer table ``0x801C9370``, see
``docs/tooling/super-art-queue-capture.md``), so every cue line carries the
actor's action-queue bytes (``+0x1DF..``) on the cue frame - the art action
constants (``0x1B..0x32``) the cue interleaves with.

Every observed cue also carries ``variant243`` (the live ``ctx+0x243``
byte), which says which of FUN_8004C140's three first-half candidate-table
variants the fire went through.

Usage (instance already parked where the cues will fire, e.g. a battle)::

    python3 scripts/recomp/xa_cue_capture.py --port 4499 \
        --frames 900 --actor-slot 0 --label "vahn UDU somersault" \
        --out /tmp/scratch/cues.jsonl

Captured values are Sony-derived - keep outputs in scratch, never in git.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import probe  # noqa: E402

GP = 0x8007B318

CLIP_TABLE_VA = 0x801C6ED8
CLIP_TABLE_SLOTS = 34

# Snapshot region A: 0x8007BBC0..0x8007BC40.
REGION_A = 0x8007BBC0
FILTER_CHAN = 0x8007BBC1  # CdlSetfilter chan byte (region A +0x01)
STAGED_MSF = 0x8007BBF0  # CdlLOC BCD [mm ss ff 00]    (region A +0x30)
PLAY_STATE = GP + 0x908  # 0x8007BC20                  (region A +0x60)
DUR = GP + 0x918  # 0x8007BC30                         (region A +0x70)

# Snapshot region B: 0x8007BC40..0x8007BCC0.
REGION_B = 0x8007BC40
SM_STATE = GP + 0x928  # 0x8007BC40                    (region B +0x00)
CHANNEL = GP + 0x954  # 0x8007BC6C                     (region B +0x2C)
START_LBA = GP + 0x970  # 0x8007BC88                   (region B +0x48)
END_LBA = GP + 0x974  # 0x8007BC8C                     (region B +0x4C)

# Live (non-ring) reads.
CTX_PTR = GP + 0xA0C  # 0x8007BD24 -> ctx; ctx+0x243 = variant byte
LAST_PICK = GP + 0xA4A  # 0x8007BD62
ROSTER = 0x8007BD10  # per-slot party char id (1-based)
ACTOR_TABLE = 0x801C9370  # 8 x u32 battle-actor ptrs; slots 0..2 = party

ACTOR_WIN_OFF = 0x1D8  # snapshot actor+0x1D8..+0x258 (queue at +0x1DF)
CTX_WIN_OFF = 0x1D0  # snapshot ctx+0x1D0..+0x250 (variant byte at +0x243)
REGION_SIZE = 128


def bcd(b: int) -> int:
    return (b >> 4) * 10 + (b & 0x0F)


def msf_to_lba(msf: bytes) -> int:
    """BCD [mm ss ff] -> absolute LBA (150-sector lead-in subtracted)."""
    return bcd(msf[0]) * 4500 + bcd(msf[1]) * 75 + bcd(msf[2]) - 150


def parse_clip_table(raw: bytes) -> list[dict]:
    """Decode the 34-slot ``[CdlLOC][u32 byte_len]`` clip table. Returns
    non-empty slots as ``{"slot", "file", "lba", "byte_len"}``."""
    out = []
    for i in range(CLIP_TABLE_SLOTS):
        rec = raw[i * 8 : i * 8 + 8]
        if len(rec) < 8:
            break
        byte_len = int.from_bytes(rec[4:8], "little")
        if byte_len == 0:
            continue
        out.append(
            {
                "slot": i,
                "file": f"XA{i + 1}.XA",
                "lba": msf_to_lba(rec[0:3]),
                "byte_len": byte_len,
            }
        )
    return out


def slot_for_msf(table: list[dict], msf: bytes) -> dict | None:
    """The clip-table slot whose start LBA the staged CdlLOC matches. The
    starter copies the slot's CdlLOC verbatim, so this is an exact match
    (the ``clip 0x13 chan 2`` +0x10-sector skew happens after this copy,
    on the LBA word, not on the staged MSF)."""
    lba = msf_to_lba(msf)
    for rec in table:
        if rec["lba"] == lba:
            return rec
    return None


def decode_frame(
    frame: int,
    reg_a: bytes,
    reg_b: bytes,
    actor: bytes | None,
    ctx: bytes | None = None,
) -> dict:
    """Flatten one frame's ring regions into the named cue fields."""
    d = {
        "frame": frame,
        "msf": reg_a[STAGED_MSF - REGION_A : STAGED_MSF - REGION_A + 3],
        "play_state": int.from_bytes(
            reg_a[PLAY_STATE - REGION_A : PLAY_STATE - REGION_A + 4], "little"
        ),
        "dur": int.from_bytes(reg_a[DUR - REGION_A : DUR - REGION_A + 4], "little"),
        "filter_chan": reg_a[FILTER_CHAN - REGION_A],
        "sm": int.from_bytes(
            reg_b[SM_STATE - REGION_B : SM_STATE - REGION_B + 4], "little"
        ),
        "chan": int.from_bytes(reg_b[CHANNEL - REGION_B : CHANNEL - REGION_B + 4], "little"),
        "start_lba": int.from_bytes(
            reg_b[START_LBA - REGION_B : START_LBA - REGION_B + 4], "little"
        ),
        "end_lba": int.from_bytes(
            reg_b[END_LBA - REGION_B : END_LBA - REGION_B + 4], "little"
        ),
    }
    if actor is not None:
        d["queue"] = actor[0x1DF - ACTOR_WIN_OFF : 0x1DF - ACTOR_WIN_OFF + 0x14]
    if ctx is not None:
        d["variant243"] = ctx[0x243 - CTX_WIN_OFF]
    return d


def detect_cues(samples: list[dict], table: list[dict]) -> list[dict]:
    """Pure cue-edge detector over decoded frame samples.

    A cue fires only on frames where the play state (gp+0x908) reads ``2`` -
    the value FUN_8003D53C writes per accepted cue; the stop path
    (FUN_8003EE00, run at natural end) zeroes it on the same frame it zeroes
    the SM state, so end-of-playback resets don't read as cues. Within
    ``play_state == 2``, the cue edge is: play state freshly 2 (new cue after
    idle), the staged ``(msf, chan, dur)`` tuple changing (back-to-back
    cues), or the SM state restarting to <= 1 from higher (an identical cue
    re-fired mid-playback). The first sample is baseline only; a cue already
    mid-flight at capture start is not re-reported.
    """
    cues = []
    prev = None
    for s in samples:
        if prev is not None and s["play_state"] == 2:
            tup = (s["msf"], s["chan"], s["dur"])
            ptup = (prev["msf"], prev["chan"], prev["dur"])
            fresh = prev["play_state"] != 2
            restarted = s["sm"] <= 1 and prev["sm"] > 1
            if fresh or tup != ptup or restarted:
                rec = slot_for_msf(table, s["msf"])
                cue = {
                    "frame": s["frame"],
                    "clip_slot": rec["slot"] if rec else None,
                    "file": rec["file"] if rec else None,
                    "channel": s["chan"],
                    "dur": s["dur"],
                    "msf": s["msf"].hex(),
                    "start_lba": s["start_lba"],
                    "end_lba": s["end_lba"],
                }
                if "queue" in s:
                    cue["queue"] = s["queue"].hex()
                if "variant243" in s:
                    cue["variant243"] = s["variant243"]
                cues.append(cue)
        prev = s
    return cues


def capture(
    client: probe.RecompClient,
    frames: int,
    actor_slot: int | None,
    start_frame: int | None = None,
    arm: bool = True,
    stride: int = 1,
) -> tuple[list[dict], dict]:
    """Arm the ring, ride out ``frames`` guest frames, harvest, detect.

    With ``start_frame`` (and ``arm=False``) an already-armed ring is
    harvested over a past window instead - the pattern for interactive
    driving: arm once (``--arm-only``), play the turn, then harvest the
    frames it covered (the ring holds 36000 frames).

    ``stride`` samples every Nth ring frame. Safe for cue *detection*
    because the staged globals persist for a clip's whole playback (the
    shortest arts clip reads >= 100 sectors ~ 100+ frames) - only the
    reported cue frame quantises to the stride.
    """
    table = parse_clip_table(client.read_ram(CLIP_TABLE_VA, CLIP_TABLE_SLOTS * 8))

    actor_va = None
    if actor_slot is not None:
        actor_va = client.read_u32(ACTOR_TABLE + actor_slot * 4)
        if not 0x80000000 <= actor_va < 0x80200000:
            raise SystemExit(
                f"actor slot {actor_slot}: table entry 0x{actor_va:08X} is not "
                "a RAM pointer - not in a battle?"
            )

    ctx_va = client.read_u32(CTX_PTR)
    if not 0x80000000 <= ctx_va < 0x80200000:
        ctx_va = None
    roster = list(client.read_ram(ROSTER, 3))

    if arm:
        client.set_snapshot(0, REGION_A, REGION_SIZE)
        client.set_snapshot(1, REGION_B, REGION_SIZE)
        if ctx_va is not None:
            client.set_snapshot(2, ctx_va + CTX_WIN_OFF, REGION_SIZE)
        if actor_va is not None:
            client.set_snapshot(3, actor_va + ACTOR_WIN_OFF, REGION_SIZE)

    f0 = start_frame if start_frame is not None else client.frame() + 2
    target = f0 + frames
    while client.frame() < target:
        time.sleep(0.25)

    samples = []
    for f in range(f0, target, max(1, stride)):
        reg_a = client.read_frame_ram(REGION_A, REGION_SIZE, f)
        reg_b = client.read_frame_ram(REGION_B, REGION_SIZE, f)
        actor = (
            client.read_frame_ram(actor_va + ACTOR_WIN_OFF, REGION_SIZE, f)
            if actor_va is not None
            else None
        )
        ctx = (
            client.read_frame_ram(ctx_va + CTX_WIN_OFF, REGION_SIZE, f)
            if ctx_va is not None
            else None
        )
        samples.append(decode_frame(f, reg_a, reg_b, actor, ctx))

    meta = {
        "kind": "header",
        "scene": client.scene_name(),
        "mode": client.game_mode(),
        "frames": [f0, target],
        "roster": roster,
        "last_pick": client.read_ram(LAST_PICK, 1)[0],
        "actor_slot": actor_slot,
        "clip_table_slots": len(table),
    }
    return detect_cues(samples, table), meta


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument(
        "--port", type=int, default=int(os.environ.get("LEGAIA_RECOMP_PORT", "4370"))
    )
    ap.add_argument("--frames", type=int, default=900, help="guest frames to record")
    ap.add_argument(
        "--actor-slot",
        type=int,
        help="party battle-actor slot (0..2) whose action queue to snapshot",
    )
    ap.add_argument("--label", default="", help="free-form tag copied to the header")
    ap.add_argument("--out", default="-", help="JSONL out (- = stdout)")
    ap.add_argument(
        "--arm-only",
        action="store_true",
        help="configure the ring regions and exit (then drive the game, then "
        "harvest with --start-frame/--no-arm)",
    )
    ap.add_argument(
        "--start-frame",
        type=int,
        help="harvest from this (possibly past) frame instead of from now",
    )
    ap.add_argument(
        "--no-arm",
        action="store_true",
        help="don't reconfigure the ring regions (they were armed earlier)",
    )
    ap.add_argument(
        "--stride",
        type=int,
        default=1,
        help="sample every Nth ring frame (cue-safe; see capture())",
    )
    args = ap.parse_args(argv)

    client = probe.RecompClient(host=args.host, port=args.port)
    if args.arm_only:
        client.set_snapshot(0, REGION_A, REGION_SIZE)
        client.set_snapshot(1, REGION_B, REGION_SIZE)
        ctx_va = client.read_u32(CTX_PTR)
        if 0x80000000 <= ctx_va < 0x80200000:
            client.set_snapshot(2, ctx_va + CTX_WIN_OFF, REGION_SIZE)
        if args.actor_slot is not None:
            actor_va = client.read_u32(ACTOR_TABLE + args.actor_slot * 4)
            client.set_snapshot(3, actor_va + ACTOR_WIN_OFF, REGION_SIZE)
        print(f"armed at frame {client.frame()}")
        return 0
    cues, meta = capture(
        client,
        args.frames,
        args.actor_slot,
        start_frame=args.start_frame,
        arm=not args.no_arm,
        stride=args.stride,
    )
    meta["label"] = args.label

    out = sys.stdout if args.out == "-" else open(args.out, "a")
    try:
        out.write(json.dumps(meta) + "\n")
        for c in cues:
            out.write(json.dumps(c) + "\n")
    finally:
        if out is not sys.stdout:
            out.close()

    for c in cues:
        sys.stderr.write(
            f"cue @f{c['frame']}: {c['file']} chan {c['channel']} "
            f"dur 0x{c['dur']:X}"
            + (f" queue {c['queue']}" if "queue" in c else "")
            + "\n"
        )
    if not cues:
        sys.stderr.write("no cues in window\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
