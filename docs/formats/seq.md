# SEQ - PsyQ sequenced-music format

PsyQ's `SsSeqOpen` / `SsSeqPlay` accepts a 13-byte-header SEQ file: a thin
MIDI variant that names a single track with delta-time + event records.
Legaia uses it for in-game music, paired with a [VAB](vab.md) sound bank
that holds the instrument samples.

The format is a publicly-documented PsyQ SDK shape (header layout +
event encoding). This page describes byte order, the meaning of every
header field, and how Sony's SsAPI sequencer consumes the event stream
- **no Sony bytes appear here**.

## Header

Two header shapes coexist in the wild:

### Standard PsyQ shape (13 bytes)

```text
+0x00  u8[4]   magic   "pQES"  (0x70 0x51 0x45 0x53)
+0x04  u16 BE  version          (typically 1)
+0x06  u16 BE  resolution       PPQN - ticks per quarter note
+0x08  u24 BE  initial tempo    microseconds per quarter note
+0x0B  u8      time-sig num     (e.g. 4)
+0x0C  u8      time-sig denom   power of 2 (2 means /4, 3 means /8)
+0x0D  ...     event stream
```

### Legaia variant (15 bytes)

Every retail Legaia SEQ examined uses a **u32 BE version** field rather
than the u16 BE PsyQ-doc form, with two reserved zero bytes between the
magic and the version word. The remaining fields shift by two bytes:

```text
+0x00  u8[4]   magic   "pQES"
+0x04  u32 BE  version          (always 1 in retail)
+0x08  u16 BE  resolution       PPQN
+0x0A  u24 BE  initial tempo    microseconds per quarter note
+0x0D  u8      time-sig num
+0x0E  u8      time-sig denom   power of 2
+0x0F  ...     event stream
```

`crates/seq::parse_header` accepts both shapes - it probes
`u32 BE at +4..+8 == 1` and dispatches accordingly. `HEADER_LEN` is the
standard length; `HEADER_LEN_LEGAIA` is the variant length.

`version` is verified by the SsAPI loader (`FUN_80062410` in SCUS, see
[`subsystems/audio.md`](../subsystems/audio.md)). Files with `version != 1`
emit `s_This_is_an_old_SEQ_Data_Format_*`.

## Event stream

Each event is a *delta-time* (variable-length integer) followed by a
status byte and zero or more data bytes. Running status applies: if the
first byte of an event is `< 0x80`, reuse the previous status byte and
treat that byte as data.

| Status range | Event              | Data bytes |
| ------------ | ------------------ | ---------- |
| `0x80..=0x8F`| Note Off           | 2 (key, velocity) |
| `0x90..=0x9F`| Note On            | 2 (key, velocity) - `velocity == 0` ≡ NoteOff |
| `0xA0..=0xAF`| Poly Aftertouch    | 2 |
| `0xB0..=0xBF`| Control Change     | 2 (controller, value) |
| `0xC0..=0xCF`| Program Change     | 1 (program) |
| `0xD0..=0xDF`| Channel Aftertouch | 1 |
| `0xE0..=0xEF`| Pitch Bend         | 2 (LSB, MSB; both 7-bit) |
| `0xFF NN`    | Meta event         | fixed length per type (see below) |

Channel index is the low nibble of the status byte (`0..=15`). Retail data
only uses `0x90` / `0xB0` / `0xC0` / `0xE0` (Note Off is `0x90` with
`velocity == 0`).

### Variable-length quantity (VLQ)

A VLQ is a big-endian sequence of 7-bit groups; the high bit of each
byte is `1` for "more bytes follow", `0` for the final group. Maximum
4 bytes per delta. SEQ uses VLQ both for delta-times and meta-event
length fields. See `legaia_seq::read_vlq`.

### Meta events

**PSX SEQ meta events have no MIDI variable-length `length` field.** This is
the one place the format diverges sharply from a Standard MIDI File: the
SsAPI sequencer reads a meta-type byte and then a *fixed* number of payload
bytes determined by the type. The two meta types that appear in retail data:

| Kind | Bytes after type | Meaning |
| ---- | ---------------- | ------- |
| `0x51` | 3 | Set Tempo (u24 BE microseconds per quarter note). The 3 tempo bytes follow `0x51` **directly** - there is no `0x03` length prefix. A Standard MIDI File would write `FF 51 03 tt tt tt`; PSX SEQ writes `FF 51 tt tt tt`. |
| `0x2F` | 0 | End-of-Track. Two bytes total (`FF 2F`), no `0x00` payload. Required; terminates parsing. |

Any other meta type has an undefined fixed length, so the parser cannot
safely skip it and stops the track there (the reference SsAPI reader behaves
the same way).

> **The tempo gotcha.** Reading a phantom MIDI length byte mis-decodes every
> tempo event: `0x51` would consume the first tempo byte as a "length", then
> swallow the following note events as a bogus payload, and the override would
> be dropped. Retail tracks ship a **240 BPM (250000 µs/qn) init-placeholder**
> header tempo that the *first body* `0xFF 0x51` event immediately overrides
> to the real musical tempo (e.g. `FF 51 0B 71 B0` = 750000 µs/qn = 80 BPM).
> Dropping that override pins playback at the 240 BPM placeholder - a constant
> ~3x-too-fast rate. Every retail SEQ has `ppqn = 480`.

### Loop markers

PSX SEQ encodes looping through NRPN-style control changes on `0xB0`:

| Controller | Value | Meaning |
| ---------- | ----- | ------- |
| `0x63` (99) | 20 | Loop Start - remembers the current position |
| `0x63` (99) | 30 | Loop Forever - jump back to the last Loop Start |

88 of 92 retail SEQ tracks carry these markers.

The parser surfaces them as ordinary `ControlChange` events (the bytes really
are a CC), and the engine `Sequencer` interprets them at playback time: a Loop
Start fires recording the position immediately after the marker, and a later
Loop Forever - or an end-of-track that follows a Loop Start - rewinds there
rather than to event 0. The rewind lands on the event *after* the marker, so it
neither re-fires the marker nor re-applies its delta, and the integer
sample-clock is reset so the looped body re-fires on the same sample offset
every pass. `Sequencer::set_loop_to` remains an external fallback for the four
tracks that carry no markers.

## Tempo math

`tempo` is microseconds per quarter note; `ppqn` is ticks per quarter
note (always 480 in retail data). Per-tick duration is `tempo / ppqn`
microseconds, and the runtime accumulates real-world time against this rate.
A mid-stream `SetTempo` overrides for **future** events only - events that
already fired at the previous tempo are unaffected.

`legaia_seq::us_per_tick(tempo, ppqn)` returns the per-tick duration as
`f64` for inspection. The engine playback clock (`Sequencer`) does **not**
use this float: it accumulates time as an exact integer in units of
`sample × ppqn × 1_000_000` and fires an event of delta `d` ticks once the
accumulator reaches `d × tempo_us × 44100`, which keeps every term integer
and the timebase free of long-track drift.

## Where the data lives

SEQ payloads are loaded by the PsyQ libsnd `SsSeqOpen` family - see
[`subsystems/audio.md`](../subsystems/audio.md) → "Public SEQ API". On-disc,
SEQ data lives inside the same scene-VAB-prefixed streaming containers
described in [scene-bundles.md](scene-bundles.md). The `_DAT_8007BAC8`
slot the field VM writes (opcode `0x35`) is consumed by `FUN_800243F0`,
which resolves a SEQ payload through the [CDNAME](cdname.md) per-scene
block and hands it to `FUN_80062340` (`SsSeqOpen`) for playback.

## Tooling

`crates/seq` (binary `seq`) parses SEQ files end-to-end:

```
seq info    <PATH>    # header summary + event-type histogram
seq events  <PATH>    # disassemble every event in source order
seq json    <PATH>    # full parse as JSON
```

Playback is the engine side: `legaia_engine_audio::Sequencer` consumes
a parsed `Seq` + a loaded `VabBank` and drives the clean-room SPU
model. See `docs/subsystems/audio.md` → "Engine-audio model".

## See also

- [VAB sound bank](vab.md) - the instrument bank these sequences play against.
- [Sound-driver outputs](sound-driver.md) - the related `.dpk`/`.spk`/`.MAP` driver formats.
- [`subsystems/audio.md`](../subsystems/audio.md) - the PsyQ libsnd/libspu stack and sequencer.
