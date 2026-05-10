# Cutscene

Pre-rendered cutscene playback combines PSX STR video (MDEC hardware decoder) with multiplexed
XA-ADPCM audio from the `XA*.XA` files on disc. The engine drives it through game modes 26 and 27
(`StrInit` / `StrMode`), which map to `SceneMode::Cutscene` in the clean-room port.

## Game modes

| Index | Name | param | next |
|---|---|---|---|
| 26 | `STR` (`StrInit`) | `0x80A` | - |
| 27 | `STR MODE` (`StrMode`) | `0x000` | `ConfigInit` |

`StrInit` (index 26) bootstraps the cutscene: opens the STR stream, initialises the MDEC hardware
(or the clean-room decoder), starts the XA channel. `StrMode` (index 27) runs the per-frame loop:
reads the next batch of sectors, decodes a frame, blits it full-screen, and advances the audio
position. When the stream ends, the mode chain transitions to `ConfigInit` (index 1).

Both modes share `SceneMode::Cutscene` and see the same world state. The retail STR/MDEC FMV
handler lives in a dedicated overlay (game modes 26/27) that has not yet been captured. Two
script-cutscene captures (`overlay_cutscene_dialogue.bin`, `overlay_cutscene_mapview.bin`) exist in
the Ghidra project; these cover the actor-scripted dialogue sequences (op*/ed* CDNAME labels) and
share the town field-VM overlay binary, not the FMV decoder. Capture pipeline:
`ghidra/scripts/dump_str_fmv_overlay.py` (instructions inside the script).

## STR sector format

STR video is carried in 2048-byte Mode 2 Form 1 sectors. Each sector's user-data area starts with
a 20-byte header, followed by 2028 bytes of compressed bitstream payload:

```text
Offset  Bytes  Field
0x000   2      magic            - 0x0160 = video sector; any other value = non-video, skip silently
0x002   2      chunk_number     - 0-indexed position of this sector within the frame
0x004   2      chunks_per_frame - total sectors needed to complete this frame
0x006   2      frame_number     - sequential, 0-based, wraps at 0xFFFF
0x008   4      bs_data_size     - total bitstream bytes across all chunks for this frame
0x00C   2      width            - frame width in pixels (multiple of 16)
0x00E   2      height           - frame height in pixels (multiple of 16)
0x010   2      bs_version       - 2 (BS v2 only in Legaia)
0x012   2      quantize_scale   - per-frame quantization scale, 0..63
0x014   2028   bs_data          - compressed bitstream payload chunk
```

Multi-chunk frames: `StrFrameAssembler` accumulates sector payloads in arrival order. When
`chunk_number + 1 == chunks_per_frame` the full bitstream is returned truncated to `bs_data_size`.
Non-video sectors (magic ≠ 0x0160) are skipped silently.

Implementation: `crates/mdec/src/str_sector.rs` (`StrFrameAssembler`).

## MDEC decoder

`MdecDecoder::decode_frame(bs)` converts a complete BS v2 bitstream into an RGBA8 pixel buffer.
Clean-room port; source: PSX-SPX §MDEC Decompression.

### 1. Bitstream header

4 bytes preceding the macroblock data: `u16 n_words` (number of 32-bit words) + `u16 qs`
(per-frame quantization scale, 0–63, also embedded in the STR sector header above).

### 2. VLC decoding

Each 8×8 block decodes its DC coefficient first, then AC coefficients until an EOB token.

**DC coefficient** - luma and chroma use separate VLC tables (PSX-SPX Tables B.12 / B.13). The
table gives a size in bits; that many sign-extended bits follow as the delta value. DC is
delta-coded: each block's DC is the previous block's DC plus the delta, per-component
(Cr/Cb/Y0-Y3 each have independent running state).

**AC coefficients** - MPEG-1 Table B.14 (run/level pairs). Each entry gives `(run, level)`: skip
`run` zero coefficients, then insert `level`. Escape sequences carry a 6-bit run + 8-bit signed
level directly. The EOB code (`run == 64`) ends the block.

After VLC, coefficients are arranged in JPEG zigzag scan order.

### 3. Dequantize

```
coef[i] = clamp( (coef[i] * qs * Q_MAT[i] + 4) / 8, -2048, 2047 )
```

`Q_MAT` is the standard PSX quantization matrix (DC position always gets `qs = 2` applied
independently before the formula).

### 4. 8×8 IDCT

Two-pass separable 2D IDCT using a precomputed cosine table `IDCT_C[k][n]` (values pre-scaled by
2048). Row IDCT followed by column IDCT; output clamped to `[-128, 127]`.

### 5. Macroblock layout

Macroblocks are 16×16 pixels in raster order. Each macroblock decodes 6 × 8×8 blocks in this
order: **Cr, Cb, Y0 (top-left), Y1 (top-right), Y2 (bottom-left), Y3 (bottom-right)**.

### 6. 4:2:0 upsampling + BT.601 colour conversion

Each Cb/Cr sample covers a 2×2 luma region. Chroma values are center-biased (nominal ~128,
subtracted before the matrix). Fixed-point BT.601 YCbCr → RGBA8:

```
R = Y + ((91881 * Cr) >> 16)
G = Y - ((22554 * Cb + 46802 * Cr) >> 16)
B = Y + ((116130 * Cb) >> 16)
A = 255
```

Output is a `width × height` RGBA8 buffer in row-major order.

Implementation: `crates/mdec/src/lib.rs` (`MdecDecoder`, VLC tables, `IDCT_C`, `Q_MAT`).

## XA audio

XA-ADPCM audio is carried on Mode 2 Form 2 sectors with `submode & 0x24 == 0x24`. The demuxer
splits them by `(file_no, ch_no)` into per-channel streams. Each 128-byte sound group holds 8
sound units of 28 4-bit ADPCM samples; stereo interleaves as SU0 = L, SU1 = R, ….

See [`formats/xa.md`](../formats/xa.md) for the full sector layout, coding-info bit definitions,
filter coefficients, and the demuxer invocation.

**Open item:** the mapping from cutscene name to the expected `(file_no, ch_no)` channel pair is
overlay-resident (in the not-yet-captured cutscene overlay). Until that's reversed, WAV → cutscene
assignment is manual.

## Playback loop (`play-str`)

`legaia-engine play-str <file>` demonstrates end-to-end decoding:

1. Read the raw file in 2048-byte sectors.
2. Feed each sector to `StrFrameAssembler::push_sector()`.
3. On complete frame: `MdecDecoder::new(w, h).decode_frame(&bs)` → RGBA8 buffer.
4. Pre-decode all frames into `Vec<VideoFrame>`, then enter the winit event loop.
5. On `RedrawRequested`: upload the next frame's RGBA as a GPU texture via
   `renderer.upload_texture()` and render with `RenderTarget::Texture`.

Audio sync with the XA track is deferred; XA demux infrastructure exists in `crates/xa`.

For PROT-scene routing (`play --scene cutsceneN`), the mapping from CDNAME scene label to STR
entry needs the cutscene overlay trace (see "Open items" below).

## CLI reference

```bash
# Report frame inventory of a raw STR file
mdec scan-str cutscene.str

# Decode all frames to PPM images
mdec decode-str cutscene.str --out-dir frames/

# Play STR video in a window
legaia-engine play-str cutscene.str
```

## CDNAME → STR override map

Engines can override the hard-coded `cutscene_str_for` heuristic by passing
a TOML config to `play` / `play-window`:

```toml
# legaia-cutscene-map.toml
[scenes]
opdeene  = "MOV/MV1.STR"
opstati  = "MOV/MV2.STR"
opkorout = "MOV/MV3.STR"
opurud   = "MOV/MV4.STR"
opmap01  = "MOV/MV5.STR"
edteien  = "MOV/MV6.STR"
```

```bash
# Generate a starter file pre-seeded with the heuristic mapping
legaia-engine config dump-cutscene-map --out legaia-cutscene-map.toml

# Run with the override
legaia-engine play --scene opdeene --cutscene-map legaia-cutscene-map.toml
legaia-engine play-window --scene opdeene --cutscene-map legaia-cutscene-map.toml
```

The map layers on top of the heuristic: explicit entries win, missing keys
fall through to `cutscene_str_for`. API:
[`CutsceneMap::from_toml_path`](../../crates/engine-core/src/scene.rs) /
`from_toml_str` / `to_toml_string`.

The retail mapping table itself still requires the STR/MDEC overlay
capture; the TOML interface lets engines distribute the recovered map
once that lands without a code change.

## Open items

- **STR/MDEC FMV overlay capture.** The retail `StrInit` / `StrMode` handlers are in a dedicated
  overlay distinct from the dialogue overlay. Save state during a pre-rendered FMV video (opening
  or ending movie) and run `scripts/analyze-overlay.sh --label str_fmv`; then run
  `ghidra/scripts/dump_str_fmv_overlay.py` after import.
  Unblocks: XA channel mapping, PROT-to-STR entry table, `play --scene cutsceneN`.
- **XA channel map.** `(file_no, ch_no)` → cutscene-name association is inside the STR/MDEC
  overlay (not the dialogue overlay). Until reversed, WAV→cutscene assignment is manual.
- **8-bit ADPCM.** `coding_info` bit detection is implemented; the decoder emits silence for
  8-bit groups. No 8-bit audio has been observed in the corpus so far.
- **CDNAME scene label patterns.** In-engine cutscene scenes prefixed with `op`/`ed` use the town
  field-VM overlay (same binary as `overlay_cutscene_dialogue.bin`) via actor scripting; they are
  distinct from FMV (`MOV/MV*.STR`). See `is_cutscene_label()` in `engine-core/src/scene.rs`. The
  mapping from `op*`/`ed*` CDNAME labels to `MV*.STR` files is overlay-resident (blocked on
  STR/MDEC overlay capture).

## Provenance

| Subject | Source |
|---|---|
| STR sector header layout | `crates/mdec/src/str_sector.rs`; PSX-SPX §STR Video Files |
| BS v2 VLC tables (DC/AC) | `crates/mdec/src/lib.rs`; PSX-SPX Tables B.12–B.14 |
| IDCT + dequantize formula | `crates/mdec/src/lib.rs`; PSX-SPX §MDEC |
| BT.601 coefficients | `crates/mdec/src/lib.rs` |
| XA sector layout + demux | `crates/xa/src/demux.rs`; [`formats/xa.md`](../formats/xa.md) |
| Game modes 26 / 27 | `crates/engine-core/src/mode.rs` lines 101–104, 322–332 |
| `play-str` frame loop | `crates/engine-shell/src/bin/legaia-engine.rs` lines 827–876 |
