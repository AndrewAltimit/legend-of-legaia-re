#!/usr/bin/env python3
"""Decode the per-vsync SPU snapshot stream emitted by
`autorun_audio_trace.lua` into the AudioTraceFrame JSONL format the
engine-side audio-trace oracle consumes.

Input stream layout (matches the Lua probe):

    magic           = "LEGSPU01"        (8 bytes)
    frame_count     = u32 LE
    repeated frame_count times:
      vsync_index   = u32 LE
      spu_size      = u32 LE
      spu_bytes     = raw PCSX-Redux SPU sub-message (field-6 inner)

The SPU sub-message schema is sourced from PCSX-Redux's
`src/core/sstate.h` and `src/spu/types.h`:

    SPU.field 2  SPUPorts FixedBytes<0x200>  -- raw SPU register file
    SPU.field 6  Channels repeated × 24
        Channel.field 1  Chan::Data
            .field 7  start  Int32
            .field 9  loop   Int32
            .field 10 on     Bool
            .field 11 stop   Bool
            .field 21 raw_pitch Int32
        Channel.field 3  ADSRInfoEx
            .field 1  state  Int32  (0=Atk 1=Dcy 2=Sus 3=Rel 4=Stopped)

A voice is "audible" when `on || stop` - `on` means actively keyed,
`stop` means in the release tail (still producing samples until the
envelope reaches zero).

Master volume is read from the SPUPorts blob at offset 0x180/0x182
(MainVol_L / MainVol_R = registers 0x1F801D80/0x1F801D82, signed i16,
0x4000 = unity in libspu's representation). Reverb_Mode lives at 0x1AA.

Usage:
    extract_audio_trace_from_sstates.py STREAM.bin OUT.jsonl

Pair with `legaia-engine audio-trace --retail-jsonl OUT.jsonl ...` to
exercise the multi-frame retail-trace path through
`audio_trace_oracle::first_audio_trace_divergence_multi`.
"""
from __future__ import annotations

import json
import struct
import sys
from pathlib import Path
from typing import Iterator


MAGIC = b"LEGSPU01"

ADSR_STATE_STOPPED = 4  # PCSX-Redux ADSRState::Stopped

# PSX SPU register offsets within the SPUPorts blob (covers
# 0x1F801C00..0x1F801DFF). The MainVol regs are mednafen's "left/right
# master volume" - taken at face value as i16 LE.
SPU_REG_MAINVOL_L = 0x180
SPU_REG_MAINVOL_R = 0x182
SPU_REG_REVERB_MODE = 0x1AA  # u32 split across 0x1AA/0x1AE? mednafen
                              # treats it as 32-bit; lift the raw u16 from
                              # the canonical register and zero-extend.


def read_varint(buf: bytes, pos: int) -> tuple[int, int]:
    # Protobuf varints are up to 10 bytes (negative Int32 values are
    # sign-extended to 64 bits before encoding).
    v = 0
    shift = 0
    while pos < len(buf):
        b = buf[pos]
        pos += 1
        v |= (b & 0x7F) << shift
        if not (b & 0x80):
            return v, pos
        shift += 7
        if shift > 63:
            raise ValueError("varint too long")
    raise ValueError("truncated varint")


def iter_fields(buf: bytes) -> Iterator[tuple[int, int, bytes | int]]:
    """Walk a protobuf message; yield (field, wire_type, payload) tuples.
    For wire-type 2, payload is the bytes; for wire-type 0, payload is the int."""
    pos = 0
    while pos < len(buf):
        tag, pos = read_varint(buf, pos)
        field = tag >> 3
        wt = tag & 7
        if wt == 0:
            v, pos = read_varint(buf, pos)
            yield field, wt, v
        elif wt == 2:
            ln, pos = read_varint(buf, pos)
            yield field, wt, buf[pos:pos + ln]
            pos += ln
        elif wt == 5:
            yield field, wt, struct.unpack_from("<I", buf, pos)[0]
            pos += 4
        elif wt == 1:
            yield field, wt, struct.unpack_from("<Q", buf, pos)[0]
            pos += 8
        else:
            raise ValueError(f"unsupported wire type {wt} for field {field}")


def parse_channel(channel_bytes: bytes) -> dict:
    """Parse one PCSX-Redux Channel sub-message; return a dict shaped to
    feed VoiceTraceFrame fields."""
    data_payload: bytes | None = None
    adsr_ex_payload: bytes | None = None
    for field, wt, payload in iter_fields(channel_bytes):
        if wt != 2:
            continue
        if field == 1:
            data_payload = payload
        elif field == 3:
            adsr_ex_payload = payload

    on = False
    stop = False
    start_addr = 0
    loop_addr = 0
    raw_pitch = 0
    if data_payload is not None:
        for field, wt, payload in iter_fields(data_payload):
            if wt != 0:
                continue
            if field == 7:
                start_addr = payload
            elif field == 9:
                loop_addr = payload
            elif field == 10:
                on = bool(payload)
            elif field == 11:
                stop = bool(payload)
            elif field == 21:
                raw_pitch = payload

    state = ADSR_STATE_STOPPED
    if adsr_ex_payload is not None:
        for field, wt, payload in iter_fields(adsr_ex_payload):
            if wt == 0 and field == 1:
                state = payload
                break

    # "Audible" criterion: PCSX-Redux sets `on` while the voice is keyed
    # and not yet in the release tail; `stop` flips on at KOFF and stays
    # set while the envelope decays. ADSRInfoEx.state is the *configured*
    # envelope shape for the next attack and stays at Sustain for unused
    # voices, so it's not a reliable audibility signal - `on || stop` is
    # the correct match against mednafen PsxSpu's `voice_state.active`.
    _ = state  # state retained from the schema walk for future use
    active = on or stop

    voice = {
        "active": active,
    }
    if start_addr:
        voice["start_addr"] = start_addr
    if loop_addr:
        voice["loop_addr"] = loop_addr
    if raw_pitch:
        # raw_pitch is the 14-bit PSX pitch register; clamp to u16 for the
        # JSON envelope.
        voice["pitch"] = raw_pitch & 0xFFFF
    return voice


def parse_spu_section(spu_bytes: bytes) -> dict:
    """Walk one SPU sub-message; return a partial AudioTraceFrame dict
    (without `frame`, which the caller assigns)."""
    voices: list[dict] = []
    ports: bytes | None = None
    for field, wt, payload in iter_fields(spu_bytes):
        if wt != 2:
            continue
        if field == 2:
            ports = payload
        elif field == 6:
            voices.append(parse_channel(payload))

    # PCSX-Redux's SPU should always have 24 channels but pad if missing.
    while len(voices) < 24:
        voices.append({"active": False})

    active_mask = 0
    for i, v in enumerate(voices):
        if v.get("active"):
            active_mask |= 1 << i

    master_volume: tuple[int, int] | None = None
    reverb_mode: int | None = None
    if ports and len(ports) >= 0x200:
        ml, mr = struct.unpack_from("<hh", ports, SPU_REG_MAINVOL_L)
        master_volume = (ml, mr)
        rm = struct.unpack_from("<H", ports, SPU_REG_REVERB_MODE)[0]
        reverb_mode = rm  # u16 zero-extended; mednafen sometimes reports
                          # the 4-byte block - see audio_trace_oracle.

    out: dict = {
        "active_voice_mask": active_mask,
        "voices": voices,
    }
    if master_volume is not None:
        out["master_volume"] = master_volume
    if reverb_mode is not None:
        out["reverb_mode"] = reverb_mode
    return out


def main() -> int:
    if len(sys.argv) < 3:
        raise SystemExit(
            "usage: extract_audio_trace_from_sstates.py STREAM.bin OUT.jsonl")
    in_path = Path(sys.argv[1])
    out_path = Path(sys.argv[2])

    blob = in_path.read_bytes()
    if not blob.startswith(MAGIC):
        raise SystemExit(f"bad magic; got {blob[:8]!r}, expected {MAGIC!r}")
    pos = 8
    frame_count = struct.unpack_from("<I", blob, pos)[0]
    pos += 4

    frames: list[dict] = []
    while pos < len(blob):
        if pos + 8 > len(blob):
            break
        vsync_idx, spu_size = struct.unpack_from("<II", blob, pos)
        pos += 8
        if pos + spu_size > len(blob):
            raise SystemExit(
                f"truncated SPU section at vsync {vsync_idx}: need "
                f"{spu_size} bytes, have {len(blob) - pos}")
        spu_bytes = blob[pos:pos + spu_size]
        pos += spu_size

        frame_dict = parse_spu_section(spu_bytes)
        frame_dict["frame"] = vsync_idx
        # The retail-multi-frame path leaves sequencer fields unset; the
        # SPU section doesn't carry CPU-side SsAPI workspace state.
        frames.append(frame_dict)

    if len(frames) != frame_count:
        print(
            f"warning: header frame_count={frame_count} but parsed "
            f"{len(frames)} frames (using parsed count)",
            file=sys.stderr,
        )

    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("w") as fh:
        for f in frames:
            fh.write(json.dumps(f, separators=(",", ":")))
            fh.write("\n")

    print(
        f"wrote {len(frames)} frames -> {out_path} "
        f"(active_voice_mask histogram: {summarise_masks(frames)})"
    )
    return 0


def summarise_masks(frames: list[dict]) -> str:
    """Compact summary: most common active masks across the trace."""
    if not frames:
        return "(empty)"
    counts: dict[int, int] = {}
    for f in frames:
        m = f.get("active_voice_mask", 0)
        counts[m] = counts.get(m, 0) + 1
    top = sorted(counts.items(), key=lambda kv: -kv[1])[:3]
    return ", ".join(f"0b{m:024b}={n}f" for m, n in top)


if __name__ == "__main__":
    sys.exit(main())
