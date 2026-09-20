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
            .field 1  state         Int32  (0=Atk 1=Dcy 2=Sus 3=Rel 4=Stopped)
            .field 11 EnvelopeVol   Int32  (0..0x7FFF, the live envelope)

A voice is "audible" when its ADSR **envelope level** is non-zero. The
envelope is what scales the voice's output, so it is the one field that
answers the question, and it is the same line the mednafen side draws
(`legaia_mednafen::SpuVoiceState::is_active`) and the same line the engine
side draws (`Phase::Off`). Over a town-scene capture the three candidates
that do NOT use it all over-count badly: `on || stop` reports 18-23 of 24
voices per frame and `state != Stopped` 13-24, against 3-19 for the
envelope - and `stop` in particular stays set after the release tail has
run to zero, so a finished voice keeps reading as audible.

The SPUPorts blob is the PSX register window `0x1F801C00..0x1F801DFF`
verbatim, so a register's byte offset into it is `addr - 0x1F801C00`. That
mapping is corroborated three ways in a real capture: `0x180` holds `0x3FFF`
(the master volume the mednafen side reports independently), `0x188..0x18E`
carry the sparse transient writes a key-on / key-off register has and nothing
else does, and `0x1A2` (`mBASE`) resolves to the Studio C work-area size.

Global registers lifted here, each by its hardware address:

  0x180/0x182  MainVol L/R        master volume (signed i16)
  0x184/0x186  vLOUT/vROUT        reverb output depth (SpuSetReverbDepth)
  0x198/0x19A  EON                per-voice reverb-enable mask (24 bits)
  0x1A2        mBASE              reverb work-area base, in 8-byte units
  0x1AA        SPUCNT             SPU control (bit 15 enable, 14 unmute,
                                  7 reverb master, 0 CD audio)

**`0x1AA` is SPUCNT, not a reverb register.** This extractor used to publish it
as `reverb_mode`, which put the SPU control word into a field the oracle read
as a reverb-routing mask: `0xC081` decoded as "retail routes voices 0, 7, 14
and 15" when it is really "SPU enabled, unmuted, reverb master on, CD audio
on". The real `EON` register two words earlier reads `0x00FFFFFF` on every
frame of the same capture - retail routes **all 24 voices** - which is what the
mednafen save-state corpus says as well.

Per-voice registers live at `n * 0x10` within the same blob: `+0`/`+2` volume
L/R, `+4` pitch, `+6` start address (8-byte units), `+8`/`+0xA` the two ADSR
config words. The per-voice **envelope level** is not mirrored there (it reads
zero for every voice in a capture), so it comes from the protobuf
`ADSRInfoEx.EnvelopeVol` instead.

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

# ADSRInfoEx field carrying the live envelope level (0..0x7FFF). Pinned by
# range over a capture: it is the only varint field in that sub-message
# whose values span the envelope range and move on nearly every voice.
ADSR_EX_ENVELOPE_VOL = 11

# PSX SPU register offsets within the SPUPorts blob (covers
# 0x1F801C00..0x1F801DFF). The MainVol regs are mednafen's "left/right
# master volume" - taken at face value as i16 LE.
SPU_REG_MAINVOL_L = 0x180
SPU_REG_MAINVOL_R = 0x182
SPU_REG_REVERB_OUT_L = 0x184
SPU_REG_REVERB_OUT_R = 0x186
SPU_REG_EON_LO = 0x198
SPU_REG_EON_HI = 0x19A
SPU_REG_REVERB_MBASE = 0x1A2
SPU_REG_SPUCNT = 0x1AA

# Per-voice register block stride and the offsets within it.
SPU_VOICE_STRIDE = 0x10
SPU_VOICE_VOL_L = 0x00
SPU_VOICE_VOL_R = 0x02
SPU_VOICE_ADSR1 = 0x08
SPU_VOICE_ADSR2 = 0x0A


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


def parse_channel(channel_bytes: bytes, idx: int, ports: bytes | None,
                  eon: int | None) -> dict:
    """Parse one PCSX-Redux Channel sub-message; return a dict shaped to
    feed VoiceTraceFrame fields.

    `ports` supplies the per-voice register block (volume + ADSR config),
    which the Channel sub-message does not carry, and `eon` the voice's
    reverb-send bit."""
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
    env_vol = 0
    if adsr_ex_payload is not None:
        for field, wt, payload in iter_fields(adsr_ex_payload):
            if wt != 0:
                continue
            if field == 1:
                state = payload
            elif field == ADSR_EX_ENVELOPE_VOL:
                env_vol = payload

    # "Audible" criterion: the live ADSR envelope level. `on` / `stop` /
    # `state` are all key-state words - `stop` stays set once the release
    # tail has drained, `state` sits at a configured value for voices
    # nothing is driving - so each of them counts finished voices as
    # audible. Only the envelope goes to zero when the voice does.
    _ = (state, on, stop)  # retained from the schema walk for future use
    active = env_vol > 0

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
    # The envelope level is the one field that says how loudly the voice is
    # actually sounding, so it travels with `active` rather than deciding it
    # and then being discarded.
    voice["env_level"] = env_vol & 0xFFFF
    if ports is not None and len(ports) >= (idx + 1) * SPU_VOICE_STRIDE:
        base = idx * SPU_VOICE_STRIDE
        vl, vr = struct.unpack_from("<hh", ports, base + SPU_VOICE_VOL_L)
        a1 = struct.unpack_from("<H", ports, base + SPU_VOICE_ADSR1)[0]
        a2 = struct.unpack_from("<H", ports, base + SPU_VOICE_ADSR2)[0]
        voice["vol_left"] = vl
        voice["vol_right"] = vr
        voice["adsr_control"] = a1 | (a2 << 16)
    if eon is not None:
        voice["reverb_send"] = bool(eon & (1 << idx))
    return voice


def parse_spu_section(spu_bytes: bytes) -> dict:
    """Walk one SPU sub-message; return a partial AudioTraceFrame dict
    (without `frame`, which the caller assigns)."""
    channel_payloads: list[bytes] = []
    ports: bytes | None = None
    for field, wt, payload in iter_fields(spu_bytes):
        if wt != 2:
            continue
        if field == 2:
            ports = payload
        elif field == 6:
            channel_payloads.append(payload)

    have_ports = ports is not None and len(ports) >= 0x200
    eon: int | None = None
    if have_ports:
        lo = struct.unpack_from("<H", ports, SPU_REG_EON_LO)[0]
        hi = struct.unpack_from("<H", ports, SPU_REG_EON_HI)[0]
        eon = (lo | (hi << 16)) & 0x00FFFFFF

    voices = [
        parse_channel(p, i, ports if have_ports else None, eon)
        for i, p in enumerate(channel_payloads)
    ]
    # PCSX-Redux's SPU should always have 24 channels but pad if missing.
    while len(voices) < 24:
        voices.append({"active": False})

    active_mask = 0
    for i, v in enumerate(voices):
        if v.get("active"):
            active_mask |= 1 << i

    out: dict = {
        "active_voice_mask": active_mask,
        "voices": voices,
    }
    if have_ports:
        ml, mr = struct.unpack_from("<hh", ports, SPU_REG_MAINVOL_L)
        out["master_volume"] = (ml, mr)
        rl, rr = struct.unpack_from("<hh", ports, SPU_REG_REVERB_OUT_L)
        out["reverb_depth"] = (rl, rr)
        out["spu_control"] = struct.unpack_from("<H", ports, SPU_REG_SPUCNT)[0]
        out["reverb_work_area"] = (
            struct.unpack_from("<H", ports, SPU_REG_REVERB_MBASE)[0] * 8
        )
    if eon is not None:
        out["reverb_eon"] = eon
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
