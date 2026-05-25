# XA-ADPCM streams

CD-XA Mode 2 Form 2 audio sectors carrying the cutscene voice / BGM tracks. On-disc filenames are `XA1.XA` / `XA2.XA` / etc. (one or more per release).

## Sector layout

Each on-disc XA sector is the standard PSX 2352-byte raw layout:

```text
+0x000  12 B  sync (00 + 10x FF + 00)
+0x00C   4 B  header (MM SS FF mode)
+0x010   8 B  CD-XA subheader (4 fields + duplicated copy):
                  file_no, ch_no, submode, coding_info
+0x018  2304 B  user data (18 sound groups × 128 B audio)
              - 0x14 trailing bytes are padding
+0x92C   4 B  EDC
```

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

The 18 sound groups inside the user data are 128-byte CD-XA ADPCM blocks; see [the lib doc-comment](../../crates/xa/src/lib.rs) for the per-block layout (8 sound units, 28 lines × 4 bytes).

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

## "Non-standard interleave" - what it is and isn't

The earliest extracted-XA tooling truncated each on-disc sector to 2048 bytes (Form 1 mode), which silently:

1. **Dropped 276 bytes per sector** of audio (Form 2 user data is 2324 B vs Form 1's 2048 B).
2. **Collapsed every channel of the multiplexed stream into a single shuffled byte sequence**, because the per-sector `(file_no, ch_no)` subheader was discarded.

The result was a stream where only ~10 % of 128-byte sound groups passed the standard XA validation rule `bytes 8..16 == bytes 0..8`. That's the "non-standard interleave" shorthand from earlier doc revisions - **not** a bespoke Legaia muxing scheme, just Form-1-truncation damage.

The fix lives in [`crates/xa/src/demux.rs`](../../crates/xa/src/demux.rs) (function [`demux_disc_range`](../../crates/xa/src/demux.rs)). It reads raw 2352-byte sectors, parses each subheader, filters to `AUDIO + FORM2`, and splits the audio data into one buffer per `(file_no, ch_no)` tuple. After that step the per-channel buffer is a clean concatenation of standard 128-byte sound groups that the 4-bit ADPCM decoder handles directly.

The `xa demux-disc-all` subcommand drives this across the whole disc - it walks
the ISO9660 tree, finds every `*.XA`, and demuxes each at its own per-sector
sample rate / channel mode read from the subheaders (no guessed global rate):

```bash
./target/release/xa demux-disc-all \
    "/path/to/Legend of Legaia (USA).bin" \
    --out extracted/xa_demux
```

One WAV lands per `(file_no, ch_no)` channel under `extracted/xa_demux/`, named
`<xa-stem>_fileN_chM.wav`. The single-file `xa demux-disc --lba --size` variant
remains for targeting one entry by directory offset.

Pacing is therefore **data-driven per track** - the whole point. A track that
varies channel mode (the NA disc has 16-channel mono voice files like `XA4`/`XA6`
alongside 8-channel stereo music like `XA5`/`XA7`/`XA8`/`XA9`) decodes each
channel at its real width: the Form-1 `convert` path read a stereo track as mono
and played it at 2× speed; the demux path reads `coding_info` and gets it right.
Channels reporting a non-4-bit width are skipped with a warning rather than
mis-decoded.

## What the older `extracted/XA/*.XA` files contain

If the tree on disk has a populated `extracted/XA/` directory from a pre-fix extraction, those files are the Form-1-truncated bytes - usable for byte-stable hashing only, not for decoding. They should be deleted and re-extracted via `xa demux-disc-all` (or via the top-level `legaia-extract` once it integrates the new path).

## What's still open

- **Top-level extract integration.** `legaia-extract` doesn't yet call `xa demux-disc-all`. Cutscene audio works end-to-end via the standalone `xa` binary; the unified pipeline still emits the legacy Form-1 bytes.
- **8-bit ADPCM mode.** The NA corpus is **entirely 4-bit, 37.8 kHz** (`demux-disc-all` reports `bits_per_sample = 4` for all 316 channels across 34 `*.XA` files), so the unimplemented 8-bit group decoder doesn't block anything. The demuxer surfaces `bits_per_sample` and the CLI skips-and-warns on any non-4-bit channel rather than mis-decoding it - if a JP/EU build turns out to use 8-bit, that warning is where to start.
- **Per-cutscene file-no / ch-no map.** `demux-disc` emits one WAV per channel keyed by `(file_no, ch_no)`. The mapping from cutscene name → expected channel pair lives inside the cutscene-overlay's mode driver, which is [not yet captured](../tooling/overlay-capture.md). Until that's reversed, the WAV → cutscene assignment is manual.

## Provenance

| Subject | Source |
|---|---|
| Mode 2 / Form 2 sector layout | PSX BIOS docs + `legaia-iso::raw` |
| Subheader interpretation | [`crates/xa/src/demux.rs`](../../crates/xa/src/demux.rs) |
| 4-bit ADPCM filter coefficients | [`crates/xa/src/lib.rs`](../../crates/xa/src/lib.rs) |
| Sound-group decode (param + nibble layout, predictor) | bit-exact, sample-for-sample, against an external lossless reference decode of a real cutscene track; pinned by the disc-gated `xa_pcm_matches_reference` oracle in [`crates/xa/tests/pcm_reference.rs`](../../crates/xa/tests/pcm_reference.rs). |
| Form-1-truncation diagnosis | direct comparison: 90 % of 128-byte groups in `extracted/XA/*.XA` failed the `bytes 8..16 == bytes 0..8` invariant before the demuxer was added. |

## See also

- [VAB sound bank](vab.md) - the other SPU-ADPCM audio source.
- [`subsystems/cutscene.md`](../subsystems/cutscene.md) - the STR cutscene path that interleaves XA audio.
- [`subsystems/audio.md`](../subsystems/audio.md) - the PsyQ audio stack.
- [STR FMV table](str-fmv-table.md) - the in-RAM FMV file table.
