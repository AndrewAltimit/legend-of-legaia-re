# legaia-xa

PSX CD-XA ADPCM decoder and WAV exporter.

Decodes raw 128-byte sound groups (the format Legaia ships in
`extracted/XA/*.XA`, with CD-XA Mode2/Form2 sector subheaders stripped)
into 16-bit PCM, then writes a standard WAV file.

## Sound group layout (128 bytes, 4-bit mode)

- bytes 0..16 - 8 sound-unit parameters, each repeated twice for error
  detection: `[su0..su7, su0..su7]`. Each parameter byte =
  `(filter << 4) | range`, with filter ∈ 0..=3 and range ∈ 0..=12.
- bytes 16..128 - 28 lines × 4 bytes per line of sample nibbles.
  Within a line, byte k holds:
    * low nibble  = sound unit `k` sample
    * high nibble = sound unit `k+4` sample

8-bit ADPCM (4 sound units of 28 samples each, one full byte per sample;
params at bytes 0..4 mirrored four times) is also decoded - select it with
`DecodeOptions { bits: BitsPerSample::Eight, .. }` or `--bits 8` on the CLI.
The NA Legaia corpus is entirely 4-bit, so 4-bit stays the default.

## Filter coefficients (1/64 units)

| filter | f0  | f1   |
|--------|-----|------|
| 0      |   0 |    0 |
| 1      |  60 |    0 |
| 2      | 115 |  -52 |
| 3      |  98 |  -55 |

XA defines four filters; the SPU adds a fifth. `legaia-vab` shares these
constants.

## Decode formula (per sample)

```text
shifted = sign_extend(nibble, 4) << 12 >> range
pred    = (prev1 * f0 + prev2 * f1 + 32) >> 6
output  = clip(shifted + pred)
prev2   = prev1; prev1 = output
```

## On-disc layout: CD-XA Mode 2 Form 2 with multi-channel mux

Legaia's `.XA` files are standard CD-XA Mode 2 Form 2 streams that
multiplex up to 8 audio channels per file at the sector level. The
existing `extracted/XA/*.XA` files came out of `legaia-extract`
reading sectors as Form 1 (2048 user-data bytes per sector), which
silently dropped 276 bytes of audio per sector AND collapsed every
channel of the stream into a single shuffled byte sequence.

The fix lives in `legaia_xa::demux`: read raw 2352-byte sectors,
parse each subheader, filter to `AUDIO + FORM2`, group by
`(file_no, ch_no)`. Each resulting per-channel buffer is a clean
concatenation of 128-byte sound groups that the 4-bit XA decoder
handles directly.

The decoder's validity predicate accepts any sound group with all
filter nibbles 0..=3. Legaia's encoder writes distinct parameter
values into bytes 8..16 (possibly a per-half adaptive parameter
set), but the standard decoder using only bytes 0..8 produces
smoother output empirically than the 16-distinct-param hypothesis.
Insisting on the bytes-0..8 == bytes-8..16 redundancy mirror would
skip ~90% of Legaia's audio.

## CLI

**Prefer the demux path.** `demux-disc-all` reads raw 2352-byte sectors straight
off your `.bin` and recovers each track's true sample rate and channel mode from
the CD-XA subheaders. `convert` / `convert-dir` operate on subheader-stripped
Form-1 dumps, where that metadata is already gone and the rate has to be guessed.

```bash
# Demux every *.XA on the disc: one WAV per (file_no, ch_no), each at its own
# real sample rate. This is the correct-pacing path - no --sample-rate guess.
xa demux-disc-all "/path/to/Legend of Legaia (USA).bin" --out extracted/xa_demux

# Demux one stream by disc location. Read --lba / --size out of the ISO9660
# directory entry (`disc-extract list`); on the NA disc XA1.XA is at LBA 59449.
xa demux-disc "/path/to/Legend of Legaia (USA).bin" \
    --lba 59449 --size <size-from-listing> --out extracted/xa_demux

# Form-1 dump paths: the rate is an assumption you supply
xa info extracted/XA/XA1.XA --sample-rate 37800 --channels stereo
xa convert extracted/XA/XA1.XA -o out.wav --sample-rate 37800
xa convert-dir extracted/XA -o wav_out --sample-rate 37800
```

`demux-disc-all` is the production audio path: it walks the disc's
ISO9660 tree, finds every `*.XA`, and writes one WAV per
`(file_no, ch_no)` channel, each decoded at its true per-sector sample
rate / channel mode (no guessed global rate). On the NA disc that's 34
files / 316 channels, all 4-bit 37.8 kHz, with channel mode varying per
file (16-channel mono voice vs 8-channel stereo music). `demux-disc`
targets a single entry by `--lba` / `--size` from its ISO9660 directory
record. The decoder handles both 4-bit and 8-bit widths (the demux path
maps each channel's `coding_info` width automatically); any other reported
width is skipped with a warning.

## See also

- [`docs/formats/xa.md`](../../docs/formats/xa.md)
- [`docs/subsystems/audio.md`](../../docs/subsystems/audio.md)
