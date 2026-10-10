# XA-ADPCM streams

The `XA/XA*.XA` files hold the game's streamed audio: cutscene voice lines and streamed music. They are standard CD-XA: each file is a run of Mode 2 Form 2 sectors, and each sector carries a small tag saying which **channel** it belongs to. Several channels are interleaved sector by sector in one file, so the drive can play one channel while skipping the others. Decoding is two steps - split the sectors by channel, then decode each channel's ADPCM sound groups to PCM.

Implementation: [`crates/xa`](../../crates/xa/README.md) - `demux.rs` (sector split), `lib.rs` (decoder), `encode.rs` (4-bit encoder for replacement audio).

## At a glance

| Property | Value | Confidence |
|---|---|---|
| Sector | 2352 bytes raw, Mode 2 Form 2 | Confirmed |
| Audio per sector | 18 sound groups x 128 bytes = 2304 bytes | Confirmed |
| Samples per sound group | 224 (4-bit: 8 units x 28) or 112 (8-bit: 4 units x 28) | Confirmed |
| Channel key | `(file_no, ch_no)` from the sector subheader | Confirmed |
| USA disc | 34 files, 316 channels, all 4-bit at 37.8 kHz | Confirmed |
| Channel modes | 16-channel mono voice files (`XA4`, `XA6`) and 8-channel stereo music (`XA5`, `XA7`, `XA8`, `XA9`) | Confirmed |

## Sector layout

```
one raw sector, 2352 bytes
+------+--------+-----------+-------------------------------------------+-----+-----+
| sync | header | subheader | 18 sound groups x 128 bytes               | pad | EDC |
|  12  |   4    |     8     |                 2304                      | 20  |  4  |
+------+--------+-----------+-------------------------------------------+-----+-----+
0x000  0x00C    0x010       0x018                                       0x918 0x92C

one sound group, 128 bytes (4-bit mode)
+--------------------------+----------------------------------------------------+
| 16 parameter bytes       | 28 lines x 4 bytes of sample nibbles               |
| (filter, range per unit) | 8 sound units x 28 samples                         |
+--------------------------+----------------------------------------------------+
0                          16                                                 128
```

| Offset | Size | Field | Meaning | Confidence |
|---|---|---|---|---|
| `0x000` | 12 | sync | `00`, ten `FF`, `00` | Confirmed |
| `0x00C` | 4 | header | `MM SS FF mode` | Confirmed |
| `0x010` | 8 | subheader | `file_no, ch_no, submode, coding_info`, stored twice | Confirmed |
| `0x018` | 2304 | audio | 18 sound groups | Confirmed |
| `0x918` | 20 | padding | Unused tail of the 2324-byte Form 2 payload | Confirmed |
| `0x92C` | 4 | EDC | Checksum | Confirmed |

Submode bits relevant for audio detection:

| bit | meaning |
|---|---|
| `0x04` | AUDIO |
| `0x20` | FORM2 |

Coding-info bits:

| bits | meaning |
|---|---|
| 0 (`0x01`) | stereo (vs mono) |
| 2..=3 | sample rate (`00` = 37.8 kHz, `01` = 18.9 kHz) |
| 4..=5 | bits/sample (`00` = 4-bit, `01` = 8-bit) |

## Sound-group decode (4-bit)

Each 128-byte sound group holds 8 sound units of 28 samples. The decode is bit-exact against an external lossless reference decode of a real cutscene track (every interleaved sample matches), so the layout below is confirmed, not inferred.

**Parameter bytes (0..16).** The redundant copy is interleaved *within each half*, not appended:

```text
byte:  0  1  2  3   4  5  6  7   8  9 10 11  12 13 14 15
unit: p0 p1 p2 p3  p0 p1 p2 p3  p4 p5 p6 p7  p4 p5 p6 p7
```

So unit `u`'s parameter byte is at `u + (if u < 4 { 0 } else { 4 })`. Each byte is `(filter << 4) | range`, filter ∈ 0..=3, range ∈ 0..=12. (Reading bytes 0..8 as eight sequential params - the "appended mirror" reading - mis-assigns the parameters of units 4..7 and is the classic CD-XA decode trap.)

**Sample nibbles (16..128).** 28 lines of 4 bytes. Unit `u` reads byte `u / 2` of each line, taking the **low** nibble when `u` is even and the **high** nibble when `u` is odd:

```text
line byte:  0           1           2           3
nibble:   lo=unit0     lo=unit2    lo=unit4    lo=unit6
          hi=unit1     hi=unit3    hi=unit5    hi=unit7
```

**Per-sample reconstruction.** With filter coefficients in 1/64 units (`k0 = {0, 0.9375, 1.796875, 1.53125}`, `k1 = {0, 0, -0.8125, -0.859375}`):

```text
shifted = (sign_extend(nibble, 4) << 12) >> range
value   = shifted + k0 * prev1 + k1 * prev2
output  = clip16(round_half_away_from_zero(value))
prev2   = prev1;  prev1 = value     // history is the UNCLAMPED, UNROUNDED value
```

The predictor history is the full-precision reconstructed `value`, **not** the rounded+clamped 16-bit output. Re-feeding the clamped output instead is audible only at high volume - the prediction drifts on loud sound-units and rails to the opposite extreme - which is exactly the symptom that bit-exact history feedback removes.

**Stereo de-interleave.** The LEFT channel is the even units (0,2,4,6) and the RIGHT channel is the odd units (1,3,5,7); output is L,R interleaved, pairing `(0,1),(2,3),(4,5),(6,7)`. Each channel keeps its own `(prev1, prev2)` history.

## Sound-group decode (8-bit)

The 8-bit mode (subheader `coding_info` bits 4..5 = `01`) uses the **same 128-byte group** but packs **4 sound units of 28 full-byte samples** instead of 8 nibble units:

- **Parameter bytes (0..16).** The 4 unit params live at bytes 0..4, mirrored three more times at 4..8 / 8..12 / 12..16; the live copy is bytes 0..4. Same `(filter << 4) | range` encoding.
- **Sample bytes (16..128).** 28 lines of 4 bytes; unit `u` reads byte `u` of each line (one full 8-bit sample per byte, no nibble split).
- **Reconstruction.** Identical filter math, but the sample sits in the top byte of the 16-bit word before the gain shift: `shifted = (sign_extend(byte, 8) << 8) >> range` (8 bits of headroom vs the 4-bit path's 12). The 8-bit byte is signed (`0x80` = -128).
- **Stereo de-interleave.** LEFT = even units (0,2), RIGHT = odd units (1,3), pairing `(0,1),(2,3)`.

This yields 112 samples/group (4 × 28) vs the 4-bit path's 224. Select it with `legaia_xa::DecodeOptions { bits: BitsPerSample::Eight, .. }` (the demux path maps each channel's reported width automatically; the CLI exposes `--bits 8`). The whole NA corpus is 4-bit, so 4-bit is the default and the 8-bit path is exercised by synthetic unit tests (silence, full-byte sign-extension, the stereo split).

## Demuxing

[`demux_disc_range`](../../crates/xa/src/demux.rs) reads raw 2352-byte sectors, parses each subheader, keeps the `AUDIO + FORM2` ones, and appends each sector's audio to one buffer per `(file_no, ch_no)`. Each buffer is then a clean concatenation of standard 128-byte sound groups.

The `xa demux-disc-all` subcommand drives this across the whole disc. It walks
the ISO9660 tree, finds every `*.XA`, and demuxes each at the sample rate and
channel mode read from its own subheaders (no guessed global rate):

```bash
./target/release/xa demux-disc-all \
    "/path/to/Legend of Legaia (USA).bin" \
    --out extracted/xa_demux
```

One WAV lands per `(file_no, ch_no)` channel under `extracted/xa_demux/`, named
`<xa-stem>_fileN_chM.wav`. The single-file `xa demux-disc --lba --size` variant
targets one entry by directory offset. `legaia-extract` runs the demux
automatically and writes the WAVs to `extracted/XA_WAV/`.

Pacing is data-driven per channel. The decoder handles 4-bit and 8-bit widths
as each channel's `coding_info` reports them; any other width is skipped with a
warning rather than mis-decoded.

## "Non-standard interleave" - what it is and isn't

Legaia's XA files have no bespoke muxing scheme. The appearance of one comes from reading the sectors as Form 1, which keeps 2048 bytes of each and so:

1. **drops 276 bytes of audio per sector** (Form 2 payload is 2324 bytes), and
2. **collapses every channel into one shuffled byte sequence**, because the per-sector `(file_no, ch_no)` subheader is discarded.

In that stream only about 10 % of 128-byte sound groups pass the standard XA validation rule `bytes 8..16 == bytes 0..8`, and a stereo track read as mono plays at 2x speed.

The `extracted/XA/*.XA` files copied by the disc-extract step are exactly those Form-1-truncated bytes. They are usable for byte-stable hashing only, not for decoding.

## What's still open

- **8-bit ADPCM mode is decoded but unexercised** (see "Sound-group decode (8-bit)" above). The NA corpus is **entirely 4-bit, 37.8 kHz** (`demux-disc-all` reports `bits_per_sample = 4` for all 316 channels across 34 `*.XA` files), so nothing on the NA disc exercises it; the path is covered by synthetic unit tests and is wired through the demux/CLI/cutscene consumers (which map each channel's reported width). A disc that uses 8-bit would decode without code changes. The 8-bit path is **not** verified bit-exact against a real 8-bit reference, since the NA corpus has no 8-bit source.
- **Which event plays which channel is not part of this format.** `demux-disc` emits one WAV per channel keyed by `(file_no, ch_no)`. The clip → channel routing is game logic: the voice-cue dispatchers and SCUS cue tables in [`audio.md`](../subsystems/audio.md#cd-xa-voice-clip-dispatchers-and-static-cue-census) and the movie channel selection in [`cutscene.md`](../subsystems/cutscene.md#xa-channel-selection). The extracted WAV filenames carry no scene labels.

## Provenance

| Subject | Source |
|---|---|
| Mode 2 / Form 2 sector layout | PSX BIOS docs + `legaia-iso::raw` |
| Subheader interpretation | [`crates/xa/src/demux.rs`](../../crates/xa/src/demux.rs) |
| 4-bit ADPCM filter coefficients | [`crates/xa/src/lib.rs`](../../crates/xa/src/lib.rs) |
| Sound-group decode (param + nibble layout, predictor) | bit-exact, sample-for-sample, against an external lossless reference decode of a real cutscene track; pinned by the disc-gated `xa_pcm_matches_reference` oracle in [`crates/xa/tests/pcm_reference.rs`](../../crates/xa/tests/pcm_reference.rs). |
| Form-1-truncation diagnosis | direct comparison: 90 % of 128-byte groups in the truncated `extracted/XA/*.XA` bytes fail the `bytes 8..16 == bytes 0..8` invariant; the demuxed channels pass. |

## See also

- [VAB sound bank](vab.md) - the other SPU-ADPCM audio source.
- [`subsystems/cutscene.md`](../subsystems/cutscene.md) - the STR cutscene path that interleaves XA audio.
- [`subsystems/audio.md`](../subsystems/audio.md) - the PsyQ audio stack.
- [STR FMV table](str-fmv-table.md) - the in-RAM FMV file table.
