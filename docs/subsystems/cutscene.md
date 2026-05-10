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

## STR/MDEC FMV overlay residency

The retail `StrInit` / `StrMode` handlers live in a dedicated overlay distinct from the dialogue overlay. The overlay loads at `0x801C0000+` and occupies roughly `0x801CAD90..0x801F1200` (~156 KB of mixed code + data + sparse zero-padding) when an FMV is active.

Pinned data structures inside the residency window (captured from a save state during FMV playback):

| Address | Size | Stride | Contents |
|---|---:|---:|---|
| `0x801CAE40` | 144 B | 24 B × 6 | Compact MV-file table: `MV1.STR;1` .. `MV6.STR;1` |
| `0x801CCA80` | 336 B | 56 B × 6 | ISO9660-shape directory record copies of the same six files |
| `0x801CE810` | ~150 B | variable | Path-string table (\\DATA\\MOV.STR;1, \\DATA\\MOV15.STR;1, \\MOV\\MV1A.STR;1, \\MOV\\MV6..MV1.STR;1) |
| `0x801CE8AC` | ~50 B | variable | CDNAME labels for mid-game FMV-bearing field scenes |

### Compact MV-file table layout

Each entry in the compact table at `0x801CAE40` is 24 bytes:

```text
+0x00 char[12]  filename (libcd-shaped, e.g. "MV1.STR;1\0")
+0x0C u32       reserved (zero across the captured corpus)
+0x10 u32       BCD MSF (byte 0=BCD minute, 1=BCD second, 2=BCD frame, 3=zero)
+0x14 u32       file size in bytes (LE)
```

`bcd_msf` matches the libcd `CdlLOC` representation passed to `CdControl(CdlSetloc, ...)`. Convert to LBA with the standard CD identity: `LBA = ((M * 60) + S) * 75 + F - 150`.

The Rust parser is `legaia_asset::str_fmv_table::parse_entries`; the residency check + pinned addresses are in `legaia_engine_core::capture_observations::str_fmv_overlay`.

### Mid-game FMV-bearing field scenes

The FMV overlay's data section carries the CDNAME labels of seven field scenes - distinct from the `op*` / `ed*` engine cutscene scenes:

```text
town0b  map01  chitei2  map02  jou  uru2  town0e
```

These scenes have FMV trigger points in their field-VM scripts. The exact `MV*.STR` each plays is encoded in the per-scene script as the operand of the field-VM FMV-trigger op (decoded below). The heuristic in [`cutscene_str_for`](../../crates/engine-core/src/scene.rs) covers the `op*` / `ed*` scenes in CDNAME order; `FMV_TRIGGER_FIELD_SCENES` (sibling constant) lists the mid-game scenes; and the per-scene MV index resolves through [`cutscene::fmv_index_to_str_filename`](../../crates/engine-core/src/cutscene.rs) once the scene's bytecode is disassembled.

## Field-VM FMV-trigger op

The field VM triggers an FMV via a 7-byte instruction sub-dispatched off opcode `0x4C`:

```text
0x4C  0xE2  lo  hi  _  _  _      ; PC advances by 7
            ^^^^^^
            i16 LE  fmv_id (sign-extended through FUN_8003CE9C)
```

Outer opcode `0x4C` enters the field-VM dispatcher's high-nibble re-dispatch at `FUN_801E0C3C` (JT base `0x801CEE60`). The high nibble of byte 1 selects the secondary handler; the low nibble selects the inner sub-op. For byte 1 = `0xE2`:

| Step | Address | What it does |
|---|---|---|
| Outer dispatch  | `0x801DE94C..0x801DE980` | `andi 0x7F`, subtract `0x21`, jump through 47-entry JT at `0x801CECC0`. PC += 1. |
| Outer JT entry  | `0x801CED6C` | Outer op `0x4C` → handler `0x801E0C3C`. |
| High-nibble JT  | `0x801CEE70` | `byte1 >> 4 == 0xE` → handler `0x801E3040`. |
| Sub-op JT       | `0x801CF010` | `byte1 & 0xF == 0x2` → handler `0x801E30E4`. |
| FMV handler     | `0x801E30E4` | `_DAT_8007BA78 = (s16)bytecode[2..3]`; `_DAT_8007B83C = 0x1A` (next game mode = 26 = `StrInit`). PC += 6. |

The two globals it writes are the only side-effects:

- **`_DAT_8007BA78`** - FMV index. Read by the str_fmv overlay to select a 64-byte runtime FMV-state struct from the table at `0x801D0A6C` (populated from the compact MV-file table at `0x801CAE40`). On retail USA, indices `0..=5` map to `MV1.STR..MV6.STR` in order.
- **`_DAT_8007B83C`** - next-game-mode global. Setting it to `0x1A` (decimal 26) kicks the main mode dispatcher (`FUN_80017714`) into `StrInit` on the next frame, which loads the str_fmv overlay and reads `_DAT_8007BA78` to pick the file.

The field-VM port handles this op as `op4c_n_e_sub2_fmv_trigger(fmv_id: i16)` in [`legaia_engine_vm::field`](../../crates/engine-vm/src/field.rs) and the world's [`FieldHostImpl`](../../crates/engine-core/src/world.rs) records the request as `World::pending_fmv_trigger` plus a `FieldEvent::FmvTrigger { fmv_id }`. Engines drain those after `World::tick` and either play the resolved STR file (use [`cutscene::fmv_index_to_str_filename`](../../crates/engine-core/src/cutscene.rs) for the retail mapping) or skip the FMV - the field VM doesn't require any host-side response.

The trailing 3 bytes of the instruction are reserved by the dispatcher's PC math (the handler's `addiu s8, s8, 6` is fixed, but only bytes `+1..+3` are read). Disassemblers should leave them as opaque padding.

### Static FMV-trigger sites — exhaustive

A backward sweep of every Ghidra dump in the corpus surfaces only **two** writers of `_DAT_8007B83C = 0x1A` in retail. Both are codified in [`legaia_engine_vm::cutscene_trigger`](../../crates/engine-vm/src/cutscene_trigger.rs) as `FMV_TRIGGER_SITES`.

| Site | Function | Mode-write addr | FMV-id source | Trigger condition |
|---|---|---|---|---|
| `field_vm_op_4c_e2` | `FUN_801DE840` | `0x801E3104` | `decode_u16_be(pc+1)` from field-VM bytecode | Field-VM bytecode hits `0x4C 0xE2 lo hi`; reached via JT chain `0x801CEE60` (high nibble 0xE) → `0x801CF008` (low nibble 0x2). |
| `title_attract_loop` | `FUN_801DE234` | `0x801E0F50` | Hardcoded `0` (= `MV1.STR`, intro) | Title-screen idle countdown `DAT_801ef16c` underflows. |

**`FUN_801E30E4` has zero static callers.** It is a label inside `FUN_801DE840`, not a callable subroutine — Ghidra promotes it to a `FUN_` symbol because the JT at `0x801CF008[2]` resolves to that address. The actual control flow is `outer 0x4C dispatcher → 0x801E0C3C → 0x801E3040 → jump-table indirect to 0x801E30E4`.

### Why the seven mid-game scenes don't surface in a bytewise PROT scan

`town0b`, `map01`, `chitei2`, `map02`, `jou`, `uru2`, `town0e` all kick off mid-game STR FMVs. Since `field_vm_op_4c_e2` is the only field-VM-driven trigger and the title-screen attract path is hardcoded to `fmv_id = 0`, those seven scenes **must** trigger via the same `0x4C 0xE2` op. The byte sequence is not statically present in their on-disc PROT entries, however — the disassembler's `--bytewise` mode (which scans every byte of every PROT entry) surfaces only one in-range trigger across the corpus (`PROT[371] taiku`, `fmv_id=5`).

The conclusion is that the seven scenes' field-VM bytecode is **reconstructed at scene-load time** from the field-pack preamble's runtime-projected slot. The lift is therefore blocked on the same intra-transition byte-level capture that gates the [field-pack runtime projection](../formats/field-pack.md) — once the loader's preamble → runtime-RAM-cell projection is mapped, the `0x4C 0xE2` op-byte sequence becomes scannable in RAM and the per-scene MV index falls out of a single-frame disassembly.

## Open items

- **Function-by-function overlay decompilation.** `ghidra/scripts/dump_str_fmv_overlay.py` ships a `TARGETS` list re-ranked by xref count from `inventory_overlay.py` against the captured `overlay_str_fmv.bin` slice. The 27 entry points cluster around `FUN_801CF098` (the 1236-byte main play loop) - inbound xrefs from `0x801CECA0` confirm the FMV-state struct selector reads `_DAT_8007BA78 << 6 + 0x801D0A6C` to pick the entry passed in. Per-function sub-asset decode (XA channel selector, MDEC frame demux state machine) still pending.
- **XA channel map.** `(file_no, ch_no)` → cutscene-name association is inside the STR/MDEC overlay. The MV-file table doesn't carry XA channel info directly; the channel selector is presumably driven by `\DATA\MOV.STR;1` (which appears to be a multi-channel container distinct from the per-cutscene `\MOV\MVn.STR;1` files).
- **MOV15.STR + MV1A.STR.** Two extra path strings (`\DATA\MOV15.STR;1` and `\MOV\MV1A.STR;1`) appear alongside the six numbered MVs. These are dev / debug branches: `MOV15` is the 15-FPS test file (referenced by the `psx.cdspeedup` / 15 fps debug paths), and `MV1A` is an alternate / cut version of MV1. Neither ships in the released disc layout.
- **8-bit ADPCM.** `coding_info` bit detection is implemented; the decoder emits silence for 8-bit groups. No 8-bit audio has been observed in the corpus so far.

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
