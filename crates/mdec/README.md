# legaia-mdec

PSX MDEC clean-room decoder (Iki bitstream variant) and PSX STR video-sector parser.

## Scope

- `MdecDecoder` - decodes a complete demuxed Iki frame (header + LZSS qscale/DC table + AC bitstream) into RGBA8 pixels.
- `str_sector::StrFrameAssembler` - collects 2048-byte Mode 2 Form 1 sector data areas (32-byte sector header) and returns the assembled demuxed frame when a full frame is ready.
- `str_sector::analyze_str_timing` - recovers the playback frame rate from the sector stride at the 2x CD rate (PSX STR carries no fps field). All six Legaia `MV*.STR` measure 10 sectors/frame → 15 fps; `play-str` and the in-flow cutscene driver pace to this.
- `st_ring::StRing` - the retail `St` streaming-library sector ring: the same demux job as `StrFrameAssembler`, but with the fixed-slot ring bookkeeping (back-pressure, seek-to-start-frame, end-frame latch, wrap handling) a real-time player needs. See [`docs/subsystems/cutscene.md`](../../docs/subsystems/cutscene.md#engine-port---legaia_mdecst_ring).
- `str_player::StrPlayer` - the retail play loop over the ring: the FMV dispatch slot's frame window, the frame pump, the MDEC output-control word and the slice cursor. It is what makes a *segment* of a multi-cutscene movie playable (`MV3.STR` carries four). See [`docs/subsystems/cutscene.md`](../../docs/subsystems/cutscene.md#engine-port---legaia_mdecstr_player).
- `strv2_table` - unpacker for the STRv2/v3 VLC lookup table the play loop expands into `0x801E0A00`. No retail movie uses that decoder path, so nothing here consumes the table yet.
- `mdec` CLI - `decode-frame` (raw frame → image), `scan-str` (frame inventory + detected fps), `decode-str` (batch decode, with an optional `--start-frame`/`--end-frame` segment window), `strv2-table` (unpack + report the VLC table).

## Algorithm

Legaia movies use the PSX **"Iki"** bitstream, not STRv2 - the per-block DC and quant scale live in
an LZSS-compressed table after the header, and the entropy bitstream carries only AC coefficients.
Clean-room port; sources: PSX-SPX BS-compression pages + jPSXdec's `PlayStation1_STR_format.txt`
(format docs only). See [`docs/subsystems/cutscene.md`](../../docs/subsystems/cutscene.md#mdec-decoder-iki-bitstream).

1. 10-byte frame header: `u16 mdec_code_count, u16 0x3800, u16 width, u16 height, u16 lzss_size`.
2. LZSS-decompress `lzss_size` bytes → `block_count*2` table; per block `(hi<<8)|lo` = top-6 qscale + low-10 signed DC.
3. AC bitstream (16-bit LE words, MSB-first) after the table: PSX run/level VLC + `000001` escape (16-bit raw run/level), EOB `10`. A full block is still EOB-terminated.
4. Dequantize (DC `*Q_MAT[0]`; AC `(level*Q_MAT[i]*qscale+4)>>3`), 8×8 IDCT (`IDCT_C`), 4:2:0 upsample, BT.601 YCbCr→RGBA8 (luma `+128`). Macroblocks are column-major; blocks Cr, Cb, Y0..Y3.

## STR sector format

```text
Offset  Bytes  Field
0x000   2      magic (0x0160 = video)
0x002   2      type (0x8001)
0x004   2      chunk_number
0x006   2      chunks_per_frame
0x008   4      frame_number
0x00C   4      frame_size_bytes
0x010   2      width
0x012   2      height
0x014   12     replicated frame-header copy + padding (unused)
0x020   2016   demux payload chunk
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

Engine-shell's `play-str` subcommand and in-flow cutscene driver decode frames
and render each via `engine-render`. When played straight off the disc the
interleaved XA audio is demuxed alongside and the video is paced off the audio
cursor (see `engine-shell::cutscene_av` and `docs/subsystems/cutscene.md`).
