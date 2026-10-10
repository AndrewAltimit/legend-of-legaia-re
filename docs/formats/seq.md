# SEQ - PsyQ sequenced-music format

A SEQ is the game's sheet music: a single track of timed note, controller and tempo events, in a format Sony's PsyQ sound library derived from MIDI. It holds no audio. Every piece of in-game music is a SEQ paired with a [VAB](vab.md) sound bank that supplies the instruments. If you know Standard MIDI Files, most of this page is familiar, and the two places Legaia's SEQ differs are the two that break a MIDI-minded parser: the **header's version field is 32 bits wide**, and **meta events carry no length byte**.

For the map between each track's sound-test id, the scene it plays in and its OST title, see [`reference/music-tracks.md`](../reference/music-tracks.md). Parser: [`crates/seq`](../../crates/seq/README.md). Retail decoder: `FUN_80063CEC`; loader `FUN_80062410` ([`subsystems/audio.md`](../subsystems/audio.md)).

## At a glance

| Property | Value | Confidence |
|---|---|---|
| Magic | `pQES` (`70 51 45 53`) | Confirmed |
| Byte order | Big-endian header fields | Confirmed |
| Header length | 15 bytes on the disc (13 in the PsyQ documentation) | Confirmed |
| `version` | `1`, as a **u32** | Confirmed |
| Resolution | `ppqn = 480` in every retail SEQ | Confirmed |
| Header tempo | A 240 BPM placeholder (250000 µs per quarter note); the first body tempo event overrides it | Confirmed |
| Meta events | `FF 51 tt tt tt` (tempo) and `FF 2F` (end of track); **no length field** | Confirmed |
| Looping | Control change `0x63` with value 20 (start) / 30 (loop forever) | Confirmed |
| Channel events used | `0x9n`, `0xBn`, `0xCn`, `0xEn` only | Confirmed |

## Header

`crates/seq::parse_header` accepts two shapes. It reads `u32 BE` at `+4`; if that is `1` it takes the Legaia layout, otherwise the PsyQ-documented one. Every SEQ on the disc is the Legaia layout; the 13-byte shape is what synthetic test fixtures use.

### Legaia layout (15 bytes, `HEADER_LEN_LEGAIA`)

| Offset | Size | Field | Meaning | Confidence |
|---|---|---|---|---|
| `0x00` | u8 x 4 | magic | `pQES` | Confirmed |
| `0x04` | u32 BE | `version` | Always `1` | Confirmed |
| `0x08` | u16 BE | `resolution` | PPQN - ticks per quarter note | Confirmed |
| `0x0A` | u24 BE | initial tempo | Microseconds per quarter note | Confirmed |
| `0x0D` | u8 | time-signature numerator | e.g. 4 | Confirmed |
| `0x0E` | u8 | time-signature denominator | Power of 2 (`2` means /4, `3` means /8) | Confirmed |
| `0x0F` | - | event stream | | Confirmed |

### PsyQ-documented layout (13 bytes, `HEADER_LEN`)

| Offset | Size | Field |
|---|---|---|
| `0x00` | u8 x 4 | magic `pQES` |
| `0x04` | u16 BE | `version` |
| `0x06` | u16 BE | `resolution` |
| `0x08` | u24 BE | initial tempo |
| `0x0B` | u8 | time-signature numerator |
| `0x0C` | u8 | time-signature denominator |
| `0x0D` | - | event stream |

The SsAPI loader (`FUN_80062410`) verifies `version`; a file with `version != 1` emits `s_This_is_an_old_SEQ_Data_Format_*`.

## SEQ meta events against Standard MIDI

```
                 Standard MIDI File              Legaia SEQ
set tempo        FF 51 03 tt tt tt               FF 51 tt tt tt
end of track     FF 2F 00                        FF 2F
next meta        FF 51 03 tt tt tt               51 tt tt tt        (FF is a running status)
length field     variable-length, every meta     none; fixed size per meta type
```

A parser that expects the `03` reads the first tempo byte as a length, swallows the notes that follow as payload, and drops the tempo change. The track then plays at the 240 BPM header placeholder, about 3x too fast.

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
`velocity == 0`). A disc-wide sweep of every SEQ-bearing PROT entry
(`engine-audio/tests/real_seq_expressive_events.rs`) confirms this set:
pitch bend (`0xE0`) is present (thousands of events across a handful of
music banks) and is acted on by the sequencer; channel/poly aftertouch
(`0xD0` / `0xA0`) never appear.

### Variable-length quantity (VLQ)

A VLQ is a big-endian sequence of 7-bit groups; the high bit of each
byte is `1` for "more bytes follow", `0` for the final group. Maximum
4 bytes per delta. SEQ uses VLQ for delta-times only - meta events
carry **no** length field (see below). See `legaia_seq::read_vlq`.

### Meta events

**PSX SEQ meta events have no MIDI variable-length `length` field.** The
SsAPI sequencer reads a meta-type byte and then a *fixed* number of payload
bytes determined by the type. The two meta types that appear in retail data:

| Kind | Bytes after type | Meaning |
| ---- | ---------------- | ------- |
| `0x51` | 3 | Set Tempo (u24 BE microseconds per quarter note). The 3 tempo bytes follow `0x51` **directly** - there is no `0x03` length prefix. A Standard MIDI File would write `FF 51 03 tt tt tt`; PSX SEQ writes `FF 51 tt tt tt`. |
| `0x2F` | 0 | End-of-Track. Two bytes total (`FF 2F`), no `0x00` payload. Required; terminates parsing. |

Any other meta type has an undefined fixed length, so the parser cannot
safely skip it and stops the track there (the reference SsAPI reader behaves
the same way).

> **The tempo trap.** Retail tracks ship a **240 BPM (250000 µs/qn)
> placeholder** header tempo that the *first body* `0xFF 0x51` event overrides
> to the real musical tempo (e.g. `FF 51 0B 71 B0` = 750000 µs/qn = 80 BPM).
> Reading a phantom MIDI length byte drops that override and pins playback at
> the placeholder.

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

### ProgramChange to an unused VAB slot

A `0xCn` ProgramChange names a `ProgAtr` slot in the paired [VAB](vab.md).
Retail resolves the slot to a tone page by its rank among the used slots, so a
change to an **unused** slot aliases onto the next used slot's page. The alias
is silent for the score: a note-on searches only the slot's own `ProgAtr.tones`
rows, and an unused slot's count is zero, so its notes key no voice. The full
mechanism (`FUN_80068D94`, `FUN_80068B98`, `FUN_80066308`, `FUN_80068568`) is on
[`vab.md`](vab.md#program-slots-vs-packed-tone-pages).

The retail corpus exercises it. A disc sweep of every in-container `[VAB][SEQ]`
pair (`engine-audio/tests/real_seq_program_change_coverage.rs`) finds eight such
ProgramChanges across four entries:

| Entry | Program | Retail | Port (`engine-audio::vab_bind`) |
|---|---|---|---|
| PROT 868 | 5 | Aliases to the next used page; notes follow and key no voice | Same alias, same silence |
| PROT 996 | 19 | As above (breakpoint census: the note reaches no voice allocation) | Same |
| PROT 994 | 42 | Alias index runs past the tone region and reads garbage | Left an empty page |
| PROT 988 | 127 | No notes follow the change | No effect |

## Stream termination and truncation

A well-formed stream ends on its own `FF 2F` marker. Retail always writes a
trailing `00` after it, which the parser never reads - it stops at the
marker - so the byte is invisible to decoding but present on disc.

Two things the parser cannot size stop it early: a meta type other than
`0x51` / `0x2F`, and a system-common / SysEx status byte (`0xF0..=0xFE`).
In both cases it appends a **synthetic** `EndOfTrack` so the event list
stays well-formed for consumers, and the remaining bytes are never decoded.

That synthetic marker is indistinguishable from a real one by inspecting
the events alone, which makes a half-decoded track look complete and simply
stop playing partway through. `Seq::termination` (and the `Seq::is_complete`
shorthand) is the only way to tell them apart, and anything that cares about
getting a whole track - a player, a note-level parity oracle, a corpus sweep
- must check it.

Every stream in the disc's SEQ-bearing PROT entries is clean.
`engine-audio/tests/real_seq_stream_integrity.rs` pins the count of non-clean
streams at zero, so a parser change that starts truncating tracks fails loudly.

### A meta is a running status

The retail decoder `FUN_80063CEC` latches `0xFF` into the channel's
running-status byte when it reads a meta (`sb v0,0x16(s3)` at `0x80063EE4`),
and a data byte under that latch is dispatched as the next meta's **kind**
(`0x80063F44` -> `0x80064014`). So `FF 51 t t t <delta> 51 t t t` is two
tempo events, the second with no `FF`, and a channel event after a meta needs
its own status byte.

One track depends on it: PROT 1045 (sound test 57, the "sorrowful event"
requiem) closes on a ritardando written that way - `FF 51 0E C4 3E` (62 BPM),
then `51 0F 42 40` (60 BPM). A parser that keeps the previous *channel*
status across a meta reads `51 0F 42` as a note, falls one byte out of phase,
reads the closing volume fade (`B5 07 nn` / `B6 07 nn`) as long deltas and
notes, walks through the real `FF 2F 00`, and halts on a `0xF4` eleven bytes
past it - the track's last bars garbled and its loop never reached. The stream
itself is valid. `engine-audio/tests/real_seq_meta_running_status.rs` pins the
rule and the track.

## Tempo math

`tempo` is microseconds per quarter note; `ppqn` is ticks per quarter
note (always 480 in retail data). Per-tick duration is `tempo / ppqn`
microseconds, and the runtime accumulates real-world time against this rate.
A mid-stream `SetTempo` overrides for **future** events only - events that
already fired at the previous tempo are unaffected.

The three tempo bytes are read raw, with **no** MIDI variable-length
`length` field between `FF 51` and the payload. The evidence is arithmetic
rather than structural: read that way, retail tempo events land on exact
round BPM values (65, 70, 80, 128, 130, 140, 150, 160, 170) across the
corpus. Consuming a phantom length byte first would shift every payload one
byte and scatter those into nonsense, so the reading is self-verifying -
`real_seq_stream_integrity.rs` asserts the roundness precisely to keep that
falsification standing.

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
a parsed `Seq` + a loaded `VabBank` and drives the from-scratch SPU
model. See `docs/subsystems/audio.md` → "Engine-audio model".

## See also

- [VAB sound bank](vab.md) - the instrument bank these sequences play against.
- [Sound-driver outputs](sound-driver.md) - the related `.dpk`/`.spk`/`.MAP` driver formats.
- [`subsystems/audio.md`](../subsystems/audio.md) - the PsyQ libsnd/libspu stack and sequencer.
