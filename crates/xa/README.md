# legaia-xa

PSX CD-XA ADPCM decoder and WAV exporter.

Decodes raw 128-byte sound groups (the format Legaia ships in
`extracted/XA/*.XA`, with CD-XA Mode2/Form2 sector subheaders stripped)
into 16-bit PCM, then writes a standard WAV file.

## Sound group layout (128 bytes, 4-bit mode)

- bytes 0..16 — 8 sound-unit parameters, each repeated twice for error
  detection: `[su0..su7, su0..su7]`. Each parameter byte =
  `(filter << 4) | range`, with filter ∈ 0..=3 and range ∈ 0..=12.
- bytes 16..128 — 28 lines × 4 bytes per line of sample nibbles.
  Within a line, byte k holds:
    * low nibble  = sound unit `k` sample
    * high nibble = sound unit `k+4` sample

8-bit ADPCM (4 sound units of 28 samples each, 1 byte per sample) is
less common for music and not yet implemented.

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

## Caveat: Legaia's `.XA` files are non-standard

Files in `extracted/XA/` are aligned to 128-byte sound groups, but only
~10% of groups pass standard XA validation (`bytes 8..16` must mirror
`bytes 0..8`, all filter nibbles ≤ 3). The remaining ~90% appear to be
either CD-XA subheader/padding interleaved between audio frames or a
Legaia-specific muxing scheme that hasn't been reverse-engineered yet.
The decoder itself is spec-correct; it's the muxing that's the open
problem.

## CLI

```bash
xa info        <file>
xa convert     <file> <output.wav>
xa convert-dir <dir>  <out_dir>
```

## See also

- [`docs/formats/xa.md`](../../docs/formats/xa.md)
- [`docs/subsystems/audio.md`](../../docs/subsystems/audio.md)
