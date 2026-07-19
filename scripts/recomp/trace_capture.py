#!/usr/bin/env python3
"""Frame-tagged state-trace capture from the Legend of Legaia static recomp.

Emits the canonical differential-oracle JSONL shape (one object per line;
every field except ``frame`` optional per line):

    {"frame": N, "scene": "town01", "mode": 3,
     "cam": {"pitch":u16,"yaw":u16,"roll":u16,"h":u16,
             "eye":[x,y,z], "focus":[x,y,z]},
     "player": {"x":int,"z":int,"heading":int},
     "actors": [{"i":int,"x":int,"z":int,"heading":int}, ...]}

The same shape is emitted by ``legaia-engine sim-trace`` (retail units on
both sides), so the two diff 1:1 through ``trace_diff.py``.

Two capture engines:

  * ``ring`` (frame-exact): configures per-frame RAM snapshot regions
    (``set_snapshot``, 4 slots x 128 bytes) and reads them back per frame
    from the runtime's 36000-frame ring (``read_frame_ram``). Every sample
    is taken at the same frame boundary. Only possible when the requested
    maps fit 4 regions - the built-in ``camera`` map alone uses all 4.
  * ``poll`` (best-effort): live ``read_ram`` loop tagged with the current
    frame number. Frames are skipped when the loop can't keep up, and a
    sample can straddle a frame boundary. Required for maps the region
    budget can't cover (camera+player together, per-actor tables).

``pause`` / ``step`` / ``run_to_frame`` are removed from the runtime's
debug server, so there is no synchronous stepping - the ring engine is the
frame-exact primitive. Default engine is ``auto`` (ring when it fits).

Address map (pinned in docs/reference/memory-map.md +
docs/subsystems/field-locomotion.md): camera rotation trio u16
@0x8007B790/92/94, projection H u16 @0x8007B6F4, eye trio i32 @0x800840B8,
focus trio i32 @0x80089118, player ptr @0x8007C364 (+0x14 X, +0x18 Z,
+0x26 heading u16, 4096 = 360 deg), scene name @0x8007050C, mode
@0x8007B83C.

Captured traces are Sony-derived (game RAM values) - keep them untracked.
"""

from __future__ import annotations

import argparse
import json
import struct
import sys
import time

import probe

CAM_ROT_ADDR = 0x8007B790  # u16 pitch / yaw / roll
CAM_H_ADDR = 0x8007B6F4  # u16 GTE projection H
CAM_EYE_ADDR = 0x800840B8  # i32 x/y/z
CAM_FOCUS_ADDR = 0x80089118  # i32 x/y/z
PLAYER_PTR_ADDR = 0x8007C364
PLAYER_X_OFF = 0x14
PLAYER_Z_OFF = 0x18
PLAYER_HEADING_OFF = 0x26
ANGLE_MASK = 0xFFF  # 12-bit PSX angle space (4096 = 360 deg)


class Span:
    """One contiguous RAM span a map needs, with a decoder into the line."""

    def __init__(self, addr: int, size: int, decode):
        self.addr = addr
        self.size = size
        self.decode = decode  # fn(bytes, line_dict) -> None


def _u16(b, off=0):
    return struct.unpack_from("<H", b, off)[0]


def _i16(b, off=0):
    return struct.unpack_from("<h", b, off)[0]


def _i32x3(b, off=0):
    return list(struct.unpack_from("<3i", b, off))


def _cam(line):
    return line.setdefault("cam", {})


def build_camera_spans() -> list[Span]:
    return [
        Span(
            CAM_ROT_ADDR,
            6,
            lambda b, ln: _cam(ln).update(
                pitch=_u16(b, 0) & ANGLE_MASK,
                yaw=_u16(b, 2) & ANGLE_MASK,
                roll=_u16(b, 4) & ANGLE_MASK,
            ),
        ),
        Span(CAM_H_ADDR, 2, lambda b, ln: _cam(ln).update(h=_u16(b))),
        Span(CAM_EYE_ADDR, 12, lambda b, ln: _cam(ln).update(eye=_i32x3(b))),
        Span(CAM_FOCUS_ADDR, 12, lambda b, ln: _cam(ln).update(focus=_i32x3(b))),
    ]


def build_player_spans(player_base: int) -> list[Span]:
    """Player record span. ``player_base`` is the resolved actor pointer
    (read live from 0x8007C364 once - the pointer is stable within a scene)."""

    def dec(b, ln):
        ln["player"] = {
            "x": _i16(b, 0),
            "z": _i16(b, PLAYER_Z_OFF - PLAYER_X_OFF),
            "heading": _u16(b, PLAYER_HEADING_OFF - PLAYER_X_OFF) & ANGLE_MASK,
        }

    size = PLAYER_HEADING_OFF - PLAYER_X_OFF + 2
    return [Span(player_base + PLAYER_X_OFF, size, dec)]


def build_scene_spans() -> list[Span]:
    def dec_scene(b, ln):
        ln["scene"] = b.replace(b"\x00", b" ").decode("ascii", "replace").strip()

    return [
        Span(probe.SCENE_NAME_ADDR, 8, dec_scene),
        Span(probe.GAME_MODE_ADDR, 2, lambda b, ln: ln.update(mode=_u16(b))),
    ]


class ActorMap:
    """Configurable per-actor sweep so lanes can trace NPC positions and
    headings. Spec string: comma-separated ``key=value``:

      base=0x800843xx   required - table base address
      kind=records|ptrs records: entries at base + i*stride;
                        ptrs: u32 pointer table at base + i*4 (null skipped)
      stride=0x188      records only - entry stride
      count=8           max entries to scan
      term=0x0          optional - stop when the entry's first u32 == term
      x=0x14 z=0x18 h=0x26   field offsets within an entry (defaults =
                        the player-record layout)

    Poll-engine only (the sweep needs pointer chases / spans past the
    128-byte region limit)."""

    def __init__(self, spec: str):
        kv = {}
        for part in spec.split(","):
            if not part.strip():
                continue
            k, _, v = part.partition("=")
            kv[k.strip()] = v.strip()
        self.base = int(kv["base"], 0)
        self.kind = kv.get("kind", "records")
        if self.kind not in ("records", "ptrs"):
            raise ValueError(f"actor kind must be records|ptrs, got {self.kind!r}")
        self.stride = int(kv.get("stride", "0"), 0)
        if self.kind == "records" and self.stride <= 0:
            raise ValueError("records actor map needs stride=")
        self.count = int(kv.get("count", "8"), 0)
        self.term = int(kv["term"], 0) if "term" in kv else None
        self.x_off = int(kv.get("x", hex(PLAYER_X_OFF)), 0)
        self.z_off = int(kv.get("z", hex(PLAYER_Z_OFF)), 0)
        self.h_off = int(kv.get("h", hex(PLAYER_HEADING_OFF)), 0)

    def sample(self, client: probe.RecompClient) -> list[dict]:
        out = []
        entry_span = max(self.x_off, self.z_off, self.h_off) + 2
        if self.kind == "records":
            blob = client.read_ram(self.base, self.count * self.stride)
            for i in range(self.count):
                ent = blob[i * self.stride : i * self.stride + self.stride]
                if self.term is not None and struct.unpack_from("<I", ent)[0] == self.term:
                    break
                out.append(self._decode(i, ent))
        else:
            ptrs = struct.unpack(
                f"<{self.count}I", client.read_ram(self.base, self.count * 4)
            )
            for i, p in enumerate(ptrs):
                if self.term is not None and p == self.term:
                    break
                if not (0x80000000 <= p < 0x80200000):
                    continue
                ent = client.read_ram(p, entry_span)
                out.append(self._decode(i, ent))
        return out

    def _decode(self, i: int, ent: bytes) -> dict:
        return {
            "i": i,
            "x": _i16(ent, self.x_off),
            "z": _i16(ent, self.z_off),
            "heading": _u16(ent, self.h_off) & ANGLE_MASK,
        }


def capture_ring(client, spans, frames, static_fields, out):
    """Frame-exact capture through the per-frame snapshot ring."""
    if len(spans) > 4:
        raise SystemExit(
            f"ring engine: {len(spans)} regions needed but the runtime has 4 "
            "snapshot slots - use --engine poll or drop a map"
        )
    for slot, sp in enumerate(spans):
        if sp.size > 128:
            raise SystemExit(f"ring engine: span @0x{sp.addr:08X} is {sp.size} B > 128")
        client.set_snapshot(slot, sp.addr, sp.size)
    # Regions record from the next frame boundary; skip 2 to be safe.
    f0 = client.frame() + 2
    target = f0 + frames
    while client.frame() < target:
        time.sleep(0.05)
    for f in range(f0, target):
        line = {"frame": f}
        line.update(static_fields)
        for sp in spans:
            sp.decode(client.read_frame_ram(sp.addr, sp.size, f), line)
        out.write(json.dumps(line) + "\n")
    return frames


def capture_poll(client, spans, actor_map, frames, static_fields, out):
    """Best-effort live polling tagged with the frame counter. Skipped
    frames are simply absent from the output (the diff tool aligns on
    frame numbers, not line indices)."""
    captured = 0
    last = -1
    while captured < frames:
        f = client.frame()
        if f == last:
            time.sleep(0.002)
            continue
        last = f
        line = {"frame": f}
        line.update(static_fields)
        for sp in spans:
            sp.decode(client.read_ram(sp.addr, sp.size), line)
        if actor_map is not None:
            line["actors"] = actor_map.sample(client)
        out.write(json.dumps(line) + "\n")
        captured += 1
    return captured


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--host", default=probe.DEFAULT_HOST)
    ap.add_argument("--port", type=int, default=probe.DEFAULT_PORT)
    ap.add_argument("--frames", type=int, default=100, help="samples to capture")
    ap.add_argument(
        "--map",
        action="append",
        choices=["camera", "player", "scene"],
        help="built-in address map (repeatable); default: camera",
    )
    ap.add_argument("--actors", help="per-actor sweep spec (see ActorMap docstring)")
    ap.add_argument(
        "--engine",
        choices=["auto", "ring", "poll"],
        default="auto",
        help="ring = frame-exact snapshot ring (<= 4 regions); poll = live loop",
    )
    ap.add_argument("--savestate", type=int, help="load this savestate slot first")
    ap.add_argument("--expect-scene", help="verify scene after savestate load")
    ap.add_argument("--expect-mode", help="verify mode after savestate load (hex ok)")
    ap.add_argument("--out", default="-", help="output JSONL path (default stdout)")
    args = ap.parse_args(argv)

    maps = args.map or ["camera"]
    client = probe.RecompClient(args.host, args.port)
    client.connect()

    if args.savestate is not None:
        expect_mode = int(args.expect_mode, 0) if args.expect_mode else None
        scene, mode = client.load_savestate(
            args.savestate, expect_scene=args.expect_scene, expect_mode=expect_mode
        )
        print(
            f"savestate slot {args.savestate} verified: scene={scene!r} mode=0x{mode:X}",
            file=sys.stderr,
        )

    spans: list[Span] = []
    if "camera" in maps:
        spans += build_camera_spans()
    if "player" in maps:
        pptr = client.read_u32(PLAYER_PTR_ADDR)
        if not (0x80000000 <= pptr < 0x80200000):
            raise SystemExit(f"player ptr @0x{PLAYER_PTR_ADDR:08X} = 0x{pptr:08X} not in RAM")
        spans += build_player_spans(pptr)
    if "scene" in maps:
        spans += build_scene_spans()

    actor_map = ActorMap(args.actors) if args.actors else None

    engine = args.engine
    if engine == "auto":
        engine = "ring" if (len(spans) <= 4 and actor_map is None) else "poll"
    if engine == "ring" and actor_map is not None:
        raise SystemExit("actor sweeps need --engine poll (pointer chases)")

    # When the scene map isn't part of the capture, stamp scene/mode once
    # from a live read so every line still carries them (they change only
    # on scene transitions; a transition mid-capture shows up in the data).
    static_fields = {}
    if "scene" not in maps:
        static_fields["scene"] = client.scene_name()
        static_fields["mode"] = client.game_mode()

    out = sys.stdout if args.out == "-" else open(args.out, "w")
    try:
        if engine == "ring":
            n = capture_ring(client, spans, args.frames, static_fields, out)
        else:
            n = capture_poll(client, spans, actor_map, args.frames, static_fields, out)
    finally:
        if out is not sys.stdout:
            out.close()
        client.close()
    print(f"captured {n} frames via {engine} engine", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
