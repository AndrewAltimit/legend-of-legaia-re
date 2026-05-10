# legaia-mdec

PSX MDEC clean-room decoder and PSX STR video-sector parser.

## Scope

- `MdecDecoder` - decodes a complete BS v2 bitstream payload into RGBA8 pixels.
- `str_sector::StrFrameAssembler` - collects 2048-byte Mode 2 Form 1 sector data areas and returns assembled BS payloads when a full frame is ready.
- `mdec` CLI - `decode-frame` (raw BS → PPM), `scan-str` (report frame inventory), `decode-str` (batch decode to PPMs).

## Algorithm

Clean-room port of the PSX MDEC hardware; source: PSX-SPX §MDEC Decompression.

1. 4-byte BS header: `u16 n_words, u16 qs` (per-frame quantization scale).
2. Macroblocks in raster order, each 16×16 px = 6 × 8×8 blocks: Cr, Cb, Y0, Y1, Y2, Y3.
3. Per block: MPEG-1 luma/chroma DC VLC (delta-coded) + MPEG-1 AC VLC (run/level, escape, EOB) → de-zigzag → dequantize (Q_MAT × qs / 8) → 8×8 IDCT (precomputed cosine table, `IDCT_C`).
4. 4:2:0 upsampling (each Cb/Cr sample covers 2×2 luma), BT.601 YCbCr → RGBA8.

## STR sector format

```text
Offset  Bytes  Field
0x000   2      magic (0x0160 = video)
0x002   2      chunk_number
0x004   2      chunks_per_frame
0x006   2      frame_number
0x008   4      bs_data_size
0x00C   2      width
0x00E   2      height
0x010   2      bs_version (2)
0x012   2      quantize_scale
0x014   2028   bs_data payload
```

## Usage

```bash
# Decode a raw BS payload file to PPM
mdec decode-frame cutscene.bs --width 320 --height 240 --out frame.ppm

# Scan a raw STR file (2048-byte sectors)
mdec scan-str cutscene.str

# Decode all frames from a STR file
mdec decode-str cutscene.str --out-dir frames/

# Play STR video in a window
legaia-engine play-str cutscene.str
```

## Integration

Engine-shell's `play-str` subcommand pre-decodes all frames, then renders each
as `RenderTarget::Texture` via `engine-render`. Audio sync with the XA track is
deferred (XA demux already exists in `legaia-xa`); the retail Legaia STR-to-PROT
mapping is not yet traced.
