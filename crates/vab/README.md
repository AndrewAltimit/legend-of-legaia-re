# legaia-vab

Sony VAB instrument-bank parser and VAG sample extractor.

VAB ("VAGbank") is the PsyQ format that ships SPU-ADPCM samples bundled
with per-program tone metadata. Legaia's banks live inside `battle_data`
and `level_up` PROT blocks, plus the `vab_01` cluster (1072–1194). The
CDNAME label `vab_01` is misleading - verify by checking for the
`VABp` magic at offset 0.

## Header layout (Sony PsyQ docs, version 7)

```text
0x00 u32  magic   = 'pBAV'  (0x70424156 LE)
0x04 u32  version (typically 7)
0x08 u32  vab_id
0x0C u32  fsize           total bank size in bytes
0x10 u16  reserved
0x12 u16  ps              number of programs in use
0x14 u16  ts              total number of tones in use
0x16 u16  vs              number of VAG samples
0x18 u8   mvol            master volume
0x19 u8   pan
0x1A u8   attr1
0x1B u8   attr2
0x1C u32  reserved
```

After the 32-byte header:

```text
0x20             ProgAtr[128]    16 bytes each = 2048 bytes (always; not ps)
0x820            VagAtr[16][ps]  32 bytes per tone, 16 tones per program slot
                                 -> tones section size = 512 * ps
+(2048+512*ps)   u16 vag_table[256]
                   first entry is master shift (often 0 in v7)
                   entries 1..=vs hold cumulative VAG sizes / 8 (8-byte units)
+0x200 (after table)  VAG bodies (raw SPU ADPCM, 16-byte blocks)
```

`vag_table[i+1]` is the *size* of sample `i` in 8-byte units. Samples are
concatenated immediately after the table.

## VAG body (SPU ADPCM)

Stream of 16-byte blocks:

```text
byte 0:  (filter << 4) | shift     (filter in 0..=4)
byte 1:  flag                      (1 = loop end+jump, 2 = sustain, 4 = start)
bytes 2..16: 14 nibble pairs, low nibble first = 28 4-bit samples
```

The F0/F1 filter constants are shared with [`legaia-xa`] - the algorithm
is identical to XA-ADPCM, only the block packaging differs.

## CLI

```bash
vab list           <file>                           # tones + samples
vab extract        <file> <out_dir>                 # per-sample WAV
vab vag-sample-span <file> <sample_index>           # offsets + size
```

## See also

- [`docs/formats/vab.md`](../../docs/formats/vab.md)
- [`docs/subsystems/audio.md`](../../docs/subsystems/audio.md) - the
  PsyQ `libsnd`/`libspu` cluster in Legaia's binary, including the
  SsAPI sequencer and the SPU DMA transfer engine.
