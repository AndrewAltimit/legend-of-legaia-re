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

## "Non-standard interleave" - what it is and isn't

The earliest extracted-XA tooling truncated each on-disc sector to 2048 bytes (Form 1 mode), which silently:

1. **Dropped 276 bytes per sector** of audio (Form 2 user data is 2324 B vs Form 1's 2048 B).
2. **Collapsed every channel of the multiplexed stream into a single shuffled byte sequence**, because the per-sector `(file_no, ch_no)` subheader was discarded.

The result was a stream where only ~10 % of 128-byte sound groups passed the standard XA validation rule `bytes 8..16 == bytes 0..8`. That's the "non-standard interleave" shorthand from earlier doc revisions - **not** a bespoke Legaia muxing scheme, just Form-1-truncation damage.

The fix lives in [`crates/xa/src/demux.rs`](../../crates/xa/src/demux.rs) (function [`demux_disc_range`](../../crates/xa/src/demux.rs)). It reads raw 2352-byte sectors, parses each subheader, filters to `AUDIO + FORM2`, and splits the audio data into one buffer per `(file_no, ch_no)` tuple. After that step the per-channel buffer is a clean concatenation of standard 128-byte sound groups that the 4-bit ADPCM decoder handles directly.

The `xa demux-disc` subcommand drives this end-to-end:

```bash
./target/release/xa demux-disc \
    "/path/to/Legend of Legaia (USA).bin" \
    --lba <ISO9660-LBA-of-XA1.XA> \
    --size <directory-entry-size> \
    --out extracted/xa_demux
```

One WAV lands per `(file_no, ch_no)` channel in `extracted/xa_demux/`.

## What the older `extracted/XA/*.XA` files contain

If the tree on disk has a populated `extracted/XA/` directory from a pre-fix extraction, those files are the Form-1-truncated bytes - usable for byte-stable hashing only, not for decoding. They should be deleted and re-extracted via `xa demux-disc` (or via the top-level `legaia-extract` once it integrates the new path).

## What's still open

- **Top-level extract integration.** `legaia-extract` doesn't yet call `xa demux-disc`. Cutscene audio works end-to-end via the standalone `xa` binary; the unified pipeline still emits the legacy Form-1 bytes.
- **8-bit ADPCM mode.** Detected (`coding_info` bit), unimplemented in the decoder. Most music in the corpus is 4-bit, so this hasn't blocked anything observed.
- **Per-cutscene file-no / ch-no map.** `demux-disc` emits one WAV per channel keyed by `(file_no, ch_no)`. The mapping from cutscene name → expected channel pair lives inside the cutscene-overlay's mode driver, which is [not yet captured](../tooling/overlay-capture.md). Until that's reversed, the WAV → cutscene assignment is manual.

## Provenance

| Subject | Source |
|---|---|
| Mode 2 / Form 2 sector layout | PSX BIOS docs + `legaia-iso::raw` |
| Subheader interpretation | [`crates/xa/src/demux.rs`](../../crates/xa/src/demux.rs) |
| 4-bit ADPCM filter coefficients | [`crates/xa/src/lib.rs`](../../crates/xa/src/lib.rs) |
| Form-1-truncation diagnosis | direct comparison: 90 % of 128-byte groups in `extracted/XA/*.XA` failed the `bytes 8..16 == bytes 0..8` invariant before the demuxer was added. |

## See also

- [VAB sound bank](vab.md) - the other SPU-ADPCM audio source.
- [`subsystems/cutscene.md`](../subsystems/cutscene.md) - the STR cutscene path that interleaves XA audio.
- [`subsystems/audio.md`](../subsystems/audio.md) - the PsyQ audio stack.
- [STR FMV table](str-fmv-table.md) - the in-RAM FMV file table.
