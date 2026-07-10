# Cutscene

Pre-rendered cutscene playback combines PSX STR video (MDEC hardware decoder) with the
XA-ADPCM audio interleaved in the same CD-XA sectors. The engine drives it through game modes 26 and 27
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
handler lives in a dedicated overlay (game modes 26/27). Its **source is now pinned: PROT 0970**
(`cutscene_str`, slot-A base `0x801CE818`), identified statically from the disc by its leading
`MV*.STR` movie paths + the MDEC decoder strings (`MDEC_in_sync` / `MDEC_out_sync` /
`MDEC_rest:bad option`); see [`static-overlay-pipeline.md`](../tooling/static-overlay-pipeline.md).
So the overlay code is now Ghidra-importable straight from the disc (`asset overlay ghidra`) -
the master-dispatch + mode-26/27 handler can be decompiled without a live capture; the per-scene
`fmv_id` assignment is disc-sourced too (literal operands in the scene MAN scripts - see
"per-scene trigger assignment" below). The two
script-cutscene captures (`overlay_cutscene_dialogue.bin`, `overlay_cutscene_mapview.bin`) in the
Ghidra project are a different overlay - they cover the actor-scripted dialogue sequences (op*/ed*
CDNAME labels) and share the town field-VM overlay binary, not the FMV decoder. Capture pipeline:
`ghidra/scripts/dump_str_fmv_overlay.py` (instructions inside the script).

## STR sector format

STR video is carried in 2048-byte Mode 2 Form 1 sectors. Each sector's user-data area starts with
a 32-byte sector header; the remaining 2016 bytes are the demuxed-frame payload. Concatenating the
`[0x20..2048]` payload of every sector of a frame (in arrival order) reconstructs that frame's
demuxed bitstream, which begins with the Iki frame header (see below).

```text
Offset  Bytes  Field
0x000   2      magic            - 0x0160 = video sector; any other value = non-video, skip silently
0x002   2      type             - 0x8001
0x004   2      chunk_number     - 0-indexed position of this sector within the frame
0x006   2      chunks_per_frame - total sectors needed to complete this frame
0x008   4      frame_number     - sequential, wraps at 0xFFFF
0x00C   4      frame_size_bytes - total demuxed bytes across all chunks for this frame
0x010   2      width            - frame width in pixels (multiple of 16)
0x012   2      height           - frame height in pixels (multiple of 16)
0x014   12     replicated frame-header copy + zero padding (not used by the decoder)
0x020   2016   demux payload chunk
```

Multi-chunk frames: `StrFrameAssembler` accumulates sector payloads in arrival order. When
`chunk_number + 1 == chunks_per_frame` the demuxed frame is returned, truncated to
`frame_size_bytes`. Non-video sectors (magic ≠ 0x0160) are skipped silently.

Implementation: `crates/mdec/src/str_sector.rs` (`StrFrameAssembler`).

## MDEC decoder (Iki bitstream)

`MdecDecoder::decode_frame(frame)` converts a complete demuxed frame into an RGBA8 pixel buffer.
Legaia's movies use the PSX **"Iki"** bitstream variant, **not** the common STRv2 layout. The
distinguishing trait: the per-block DC and quantization scale are **not** in the entropy bitstream
- they live in a separate LZSS-compressed lookup table right after the frame header, and the
bitstream carries only AC coefficients. (STRv2 would put a per-frame qscale in the header and each
block's DC inside the bitstream; Legaia overwrites STRv2's header qscale/version fields with the
frame width/height, which is what a strict STRv2 parser rejects.) Clean-room port; sources:
PSX-SPX BS-compression pages + jPSXdec's `PlayStation1_STR_format.txt` (format docs only).

### 1. Frame header (10 bytes)

```text
Offset  Bytes  Field
0x000   2      mdec_code_count
0x002   2      0x3800 magic
0x004   2      width
0x006   2      height
0x008   2      lzss_size   - byte length of the compressed qscale/DC table that follows
```

### 2. LZSS qscale/DC table

The `lzss_size` bytes after the header decompress to a `block_count * 2`-byte table. The LZSS scheme:
one control byte whose 8 bits are tested LSB-first; a `0` bit copies one literal byte, a `1` bit is a
back-reference - a length byte (`+3`, range 3..=258) then a 1- or 2-byte offset (high bit of the
first byte selects the 2-byte form; offset is `+1`, relative to the current output position;
overlapping copies allowed). For block `i` the packed word is
`(table[i] << 8) | table[i + block_count]`: top 6 bits = quant scale, low 10 bits = signed DC.

### 3. AC bitstream

Read as **16-bit little-endian words, MSB-first within each word**, beginning immediately after the
compressed table. Per block: AC run/level codes from the PSX VLC table (`AC_CODES`), terminated by
the End-of-Block code `10`. The escape code `000001` is followed by a 16-bit raw MDEC value
(`run << 10 | signed-10-bit level`). A block that fills all 63 AC positions is *still* terminated by
an explicit EOB, so the decode loop always reads the next code rather than stopping when the
coefficient index saturates.

### 4. Dequantize + IDCT

DC: `coef[0] = DC * Q_MAT[0]`. AC: `coef[zigzag[i]] = (level * Q_MAT[i] * qscale + 4) >> 3`
(arithmetic shift = floor, matching PSX rounding; not range-clamped - escape codes carry large
levels). Two-pass separable 8×8 IDCT using `IDCT_C[k][n]` (pre-scaled by 2048); the row pass keeps
full `i64` precision and the single `>> 24` after the column pass normalises a DC-only block to
`coef[0] / 8`.

### 5. Macroblock layout

Each macroblock decodes 6 × 8×8 blocks in the order **Cr, Cb, Y0 (top-left), Y1 (top-right),
Y2 (bottom-left), Y3 (bottom-right)**. Macroblocks are laid out **column-major**: down each 16-pixel
column top-to-bottom, then the next column to the right.

### 6. 4:2:0 upsampling + BT.601 colour conversion

Each Cb/Cr sample covers a 2×2 luma region. PSX MDEC outputs signed (zero-centred) samples, so the
luma is offset by `+128` on the final RGB. Fixed-point BT.601 YCbCr → RGBA8:

```
R = (Y+128) + ((91881 * Cr) >> 16)
G = (Y+128) - ((22554 * Cb + 46802 * Cr) >> 16)
B = (Y+128) + ((116130 * Cb) >> 16)
A = 255
```

Output is a `width × height` RGBA8 buffer in row-major order.

Implementation: `crates/mdec/src/lib.rs` (`MdecDecoder`, `AC_CODES`, `iki_lzss_decompress`,
`IDCT_C`, `Q_MAT`). The disc-gated `str_mdec_decode_is_pixel_stable` test pins a decoded-frame
fingerprint as a regression guard.

## XA audio

XA-ADPCM audio is carried on Mode 2 Form 2 sectors with `submode & 0x24 == 0x24`. The demuxer
splits them by `(file_no, ch_no)` into per-channel streams. Each 128-byte sound group holds 8
sound units of 28 4-bit ADPCM samples; for stereo the LEFT channel is the even units (0,2,4,6)
and the RIGHT channel is the odd units (1,3,5,7), output L,R interleaved. The decode is bit-exact
against an external lossless reference decode of a real cutscene track.

See [`formats/xa.md`](../formats/xa.md) for the full sector layout, coding-info bit definitions,
filter coefficients, the per-sound-group decode (parameter/nibble layout + full-precision
predictor), and the demuxer invocation.

### Interleaved cutscene audio (A/V sync)

The six `MOV/MV*.STR` movies **interleave** their audio with the video at the sector level: the
video sectors (Mode 2 Form 1, magic `0x0160`) and one XA audio track (Mode 2 Form 2, all on
file/channel `(1, 0)`, stereo 37.8 kHz 4-bit) share the same LBA range. The cutscene's audio
therefore needs no name-based pairing - it is pulled from the same sector stream as the video, so
the two are aligned by construction.

The Form-1 extract written to `extracted/MOV/*.STR` keeps the video sectors intact but truncates
each Form-2 audio sector from 2324 to 2048 bytes, corrupting the audio. Faithful playback therefore
reads the raw 2352-byte sectors **straight off the disc image**:
[`legaia_engine_shell::cutscene_av::decode_str_av_from_disc`](../../crates/engine-shell/src/cutscene_av.rs)
makes one pass over the sectors, routing Form-2 audio to a per-`(file_no, ch_no)` buffer (à la
[`legaia_xa::demux`]) and the rest to the [`StrFrameAssembler`], then decodes the dominant audio
channel to PCM and the video to RGBA frames.

The decoded PCM is staged into the engine's audio output ([`AudioOut::play_xa`]) and the video clock
is driven off the audio cursor ([`AudioOut::xa_cursor_secs`]): the visible frame is
`audio_position / frame_period` ([`cutscene_av::due_video_frame`]), so the picture stays locked to
the soundtrack instead of free-running on a separate wall-clock timer (which drifts against the
hardware audio rate). When no audio track is present the same function falls back to a wall-clock
position, preserving the video-only pacing.

**Open item:** the mapping from cutscene *name* to the expected `(file_no, ch_no)` channel pair is
still overlay-resident (in the not-yet-captured cutscene overlay) - this is only needed for
selecting a cutscene by name from a separate multi-channel container; the in-file interleaving above
needs no such map. The 8-bit-ADPCM coding mode is now decoded too (the cutscene audio path maps each
channel's `coding_info` width); no 8-bit audio appears in the movie corpus, so it stays an untested
fallback rather than a verified path.

## Playback loop (`play-str`)

`legaia-engine play-str <file>` demonstrates end-to-end decoding. It has two modes:

- **`play-str <file>`** (no disc): plays a raw filesystem STR file (2048-byte Form-1 sectors, the
  `legaia-extract` shape) as **video only** - the extract truncates the interleaved audio.
- **`play-str MOV/MV1.STR --disc <bin>`**: resolves the movie inside the disc image and plays it
  **with its interleaved XA audio** in sync (raw 2352-byte sectors; see "Interleaved cutscene
  audio" above).

The loop:

1. Decode video frames + (disc mode) the audio track up front
   (`cutscene_av::decode_str_av_from_disc` / `decode_str_video_only`).
2. Stage the decoded audio into `AudioOut` on the first redraw so the audio cursor and the picture
   start together.
3. On `RedrawRequested`: show the frame due at the current playback position
   (`cutscene_av::due_video_frame`) and render it with `RenderTarget::Texture`. With audio the
   position is the **audio cursor** (`AudioOut::xa_cursor_secs`); without audio it is wall-clock
   (`elapsed`). Either way the movie plays at its real rate, not the display refresh rate.

### Frame-rate detection

PSX STR files carry no frame-rate field; the rate is implied by how many CD
sectors elapse per frame at the 2x delivery rate (150 sectors/s). In the raw
2048-byte-per-sector files this codebase works with, the on-disc sector order is
preserved 1:1 (audio sectors appear as skipped chunks), so the mean
sectors-per-frame recovers the authored rate directly:

```text
fps = 150 / (total_sectors / video_frame_count)
```

`legaia_mdec::str_sector::analyze_str_timing` computes this and
`StrTiming::frame_period` returns the per-frame hold duration (falling back to
the canonical 15 fps for a degenerate stream). All six Legaia movies measure
**exactly 10 sectors/frame → 15.00 fps** (`MV1` = 1345 frames = 89.7 s). The
windowed in-flow cutscene driver and `play-str` both pace to this clock when no
audio track is playing; frames are held when the host runs faster and dropped if
it falls behind. When the interleaved XA audio is playing (disc-sourced
playback), the audio cursor is the master clock instead - see "Interleaved
cutscene audio (A/V sync)" above.

For PROT-scene routing (`play --scene cutsceneN`), the mapping from CDNAME scene label to STR
entry needs the cutscene overlay trace (see "Open items" below).

## CLI reference

```bash
# Report frame inventory + detected frame rate of a raw STR file
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

The retail mapping is decoded straight from the disc - the `fmv_dispatch`
table (`legaia_asset::fmv_dispatch`) plus the MAN-carried per-scene
triggers (`man_field_scripts::scene_fmv_triggers`); the TOML layer
remains as an engine-side override surface.

## STR/MDEC FMV overlay residency

The retail `StrInit` / `StrMode` handlers live in a dedicated overlay distinct from the dialogue overlay - **PROT 0970** (`cutscene_str`), a slot-A overlay at base `0x801CE818` (pinned statically; see [`static-overlay-pipeline.md`](../tooling/static-overlay-pipeline.md)). The residency window below is from a save state during FMV playback; the addresses match the disc entry loaded at that base.

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

These labels are the scenes the overlay itself references (e.g. for the post-playback scene restore) - **not** the trigger-op scene list: the disc-walked trigger table (below) finds the `0x4C 0xE2` ops in a different scene set, with only `chitei2` overlapping. The heuristic in [`cutscene_str_for`](../../crates/engine-core/src/scene.rs) covers the `op*` / `ed*` scenes in CDNAME order; `FMV_TRIGGER_FIELD_SCENES` (sibling constant) records this overlay label list; the per-scene MV index resolves through [`cutscene::fmv_index_to_str_filename`](../../crates/engine-core/src/cutscene.rs).

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

- **`_DAT_8007BA78`** - FMV index. Read by the str_fmv overlay to select a 64-byte dispatch-table slot from `0x801D0A6C`. On retail USA the table has 12 slots (`fmv_id ∈ 0..=11`); the field VM has been observed writing values up to `fmv_id = 8` via the per-STR FMV trigger corpus. The mapping (`fmv_id → STR file + frame range`) is **not** a 1:1 walk over `MV1.STR..MV6.STR` - it skips `MV2.STR` and `MV5.STR` (disc-resident but unreferenced) and slots 5..=11 point at cut paths. The table is static overlay data, decoded from the disc by `legaia_asset::fmv_dispatch`; see [`str-fmv-table.md`](../formats/str-fmv-table.md#fmv-dispatch-table-0x801d0a6c-12--64-b) for the mapping.
- **`_DAT_8007B83C`** - next-game-mode global. Setting it to `0x1A` (decimal 26) kicks the main mode dispatcher (`FUN_80017714`) into `StrInit` on the next frame, which loads the str_fmv overlay and reads `_DAT_8007BA78` to pick the file.

The field-VM port handles this op as `op4c_n_e_sub2_fmv_trigger(fmv_id: i16)` in [`legaia_engine_vm::field`](../../crates/engine-vm/src/field.rs) and the world's [`FieldHostImpl`](../../crates/engine-core/src/world.rs) records the request as `World::pending_fmv_trigger` plus a `FieldEvent::FmvTrigger { fmv_id }`.

The world drives the Field → Cutscene → Field flow itself, mirroring the retail next-game-mode dispatch: the **next** `World::tick` consumes `pending_fmv_trigger` at the top of the frame (one frame after the op fires, exactly as `FUN_80017714` reads the next-game-mode global a frame late), and if the id resolves to a playable slot (`cutscene::fmv_index_to_str_filename` is `Some`) it flips `World::mode` into `SceneMode::Cutscene` and records the active FMV (`World::active_fmv()`). While the FMV plays the world **suspends the field VM** (the STR overlay owns the frame in retail); the host polls `World::active_fmv_str_filename()`, plays the resolved `MV*.STR`, and calls `World::finish_cutscene()` when playback ends,
which returns to the field with the field-VM program counter already past the op. A `fmv_id` whose runtime slot points at a cut/missing path is drained as a no-op (no mode flip), matching the engine's "treat a cut slot as a no-op" rule. The `legaia-engine play` loop runs this flow headlessly, decoding the resolved STR via MDEC to report its frame count. The windowed `play-window` host plays it **in the engine window**: when a tick flips the world into `SceneMode::Cutscene`, it resolves the `MV*.STR` and decodes it (shared `cutscene_av` module with `play-str`), suspends world ticks, and shows the video one frame per redraw; once the frames drain it calls `finish_cutscene()` and resumes the field.
When booting from a **disc image** the movie is read straight from the ISO with its interleaved XA audio (the scene BGM sequencer is paused for the duration and the video is paced off the audio cursor); when booting from an **extracted root** it plays video only (the extract truncates the audio). A `fmv_id` whose slot points at a missing path drains as a no-op.

The trailing 3 bytes of the instruction are reserved by the dispatcher's PC math (the handler's `addiu s8, s8, 6` is fixed, but only bytes `+1..+3` are read). Disassemblers should leave them as opaque padding.

### Static FMV-trigger sites - exhaustive

A backward sweep of every Ghidra dump in the corpus surfaces **three** writers of `_DAT_8007B83C = 0x1A` in retail. The first two are codified in [`legaia_engine_vm::cutscene_trigger`](../../crates/engine-vm/src/cutscene_trigger.rs) as `FMV_TRIGGER_SITES`; the third was pinned later via a PCSX-Redux watchpoint on the title-attract countdown.

| Site | Function | Mode-write addr | FMV-id source | Trigger condition |
|---|---|---|---|---|
| `field_vm_op_4c_e2` | `FUN_801DE840` | `0x801E3104` | `decode_u16_be(pc+1)` from field-VM bytecode | Field-VM bytecode hits `0x4C 0xE2 lo hi`; reached via JT chain `0x801CEE60` (high nibble 0xE) → `0x801CF008` (low nibble 0x2). |
| `title_attract_loop` (`FUN_801DE234` label) | `FUN_801DD35C` (label `FUN_801DE234`) | `0x801E0F50` | Hardcoded `0` (= `MV1.STR`, intro) | Title-screen idle countdown `DAT_801ef16c` underflows. |
| `title_tick_inline` | `FUN_801DD35C` | `0x801DDCF0` | Inline: `sh zero, -0x4588(v0)` zeroes `_DAT_8007BA78` at `0x801DDCE8` immediately before (= `MV1.STR`). | Inline fall-through past the decrement instruction at `0x801DDCCC` (`bgez v0, 0x801DFC3C` not taken). PC-verified via the live capture in [`subsystems/boot.md` § Tick function](boot.md#tick-function). |

Both title-side sites live in the same outer function `FUN_801DD35C` (the per-frame title-overlay tick); `FUN_801DE234` is a Ghidra-promoted label inside its body. The `0x801DDCF0` site is the one the watchpoint pins in practice - every per-frame decrement passes through `0x801DDCCC` and the underflow path immediately writes the mode-byte before any sub-call.

**`FUN_801E30E4` has zero static callers.** It is a label inside `FUN_801DE840`, not a callable subroutine - Ghidra promotes it to a `FUN_` symbol because the JT at `0x801CF008[2]` resolves to that address. The actual control flow is `outer 0x4C dispatcher → 0x801E0C3C → 0x801E3040 → jump-table indirect to 0x801E30E4`.

### The per-scene trigger assignment is disc-sourced (the "runtime-reconstructed" reading is falsified)

A raw bytewise PROT scan can't see the trigger ops because the scene scripts live **LZS-compressed** inside each scene's MAN. Decompressing every scene MAN and walking its partition-1 scripts with the field-VM disassembler (`man_field_scripts::scene_fmv_triggers`, the `0x3F`-destination walk's sibling) recovers the full assignment statically - `town01 → 1`, `garmel → 2`, `deroa / chitei2 → 3`, `dohaty → 4`, `town0d → 6`, `uru → 7`, `jouine → 8`; one op per scene, no other scene MAN carries one.
Pinned by the disc-gated `scene_fmv_triggers_disc` test; full table in [`str-fmv-table.md` § Per-scene trigger assignment](../formats/str-fmv-table.md#per-scene-trigger-assignment-disc-sourced). The earlier conclusion that the trigger bytecode is "reconstructed at scene-load from the field-pack preamble's runtime-projected slot" is **falsified** - the ops were simply compressed on disc.

The overlay's seven-label list above is therefore *not* the trigger-scene set (only `chitei2` appears in both). Outside the MAN-carried scripts, a raw sweep also finds in-range `4C E2` byte candidates in `taiku` (`fmv_id 5`, the cut `MOV15.STR` slot) and `opmap01` / `koin1b` (`fmv_id 7`) in uncompressed regions of non-MAN scene structures - uncontextualized byte matches (the same sweep "finds" triggers inside VAB sample data), kept as candidates rather than pins.

### Per-STR FMV trigger corpus

The current corpus carries nine save states captured RIGHT before each FMV begins playing, one per `_DAT_8007BA78` value (`fmv_id ∈ 0..=8`). They pin the trigger-side state across the full retail range:

- `_DAT_8007BA78 = expected_fmv_id` (s16 LE) for each of nine saves
- `_DAT_8007B83C = 0x1A` (StrInit) for every save
- `_DAT_8007BAC8 = 2000` (BGM ID) for every save
- Active scene = `map01` for every save (one of the seven mid-game FMV-trigger field scenes)
- `recover_base()` = `0x80139530` (`map01`'s field-pack base) for every save

The `0x4C 0xE2 lo hi` byte sequence does NOT appear in the field-pack RAM region for any save - the corpus was generated by **debug-menu-driven** trigger paths, NOT by stepping the field VM through a per-scene FMV trigger op. So the corpus pins the `(fmv_id, game_mode)` tuple across the full `0..=8` range but does not disambiguate which fmv_id each of the seven mid-game scenes' field-VM bytecode writes at runtime - that gap is still gated on intra-transition field-pack projection capture.

The debug-menu mechanism itself is pinned: the two direct `_DAT_8007BA78` store sites (the `4C E2` handler `0x801E3104` and the title-attract tick `0x801DDCE8/CF0`) are **corpus-exhaustive** (raw-byte scan of all 1235 PROT entries, every addressing form) - the dev menu writes the global through its register-pointer editor (`FUN_801DBD04` family, field overlay 0897), invisible to static addressing-form scans. There is **no per-FMV "event record"** carrying post-play teleport/flags; the debug "jump to beat" behaviour is the MAP CHANGE warp appliers (`FUN_801EE094`/`FUN_801EE328`) plus the EVENT FLAG editor, see [`functions.md`](../reference/functions.md).

The corpus is codified at `legaia_engine_core::capture_observations::cutscene_trigger_corpus` and exercised by the disc-gated test `crates/mednafen/tests/real_saves.rs::cutscene_trigger_corpus_pins_fmv_id_across_nine_saves`.

## In-engine 3D opening (the five-scene New-Game chain)

Not every cutscene is an STR FMV. The New Game opening - the "Genesis tree" prologue with the *"…the Seru."* narration - is an **in-engine 3D cutscene chain**, field scenes running in master game-mode `0x03` (field RUN), not a `MOV/MVn.STR` video. (`MV1.STR` is the title-attract movie; the opening 3D sequence is engine-rendered - see [`boot.md`](boot.md#the-opening-scene-chain--the-fun_801d1344-intro-skip).)

### The five-scene chain

NEW GAME boots **`opdeene`** (CDNAME/PROT #748, the creation-myth crawl) and the opening then chains through **five scenes with ZERO input** (pinned by a PCSX-Redux cold-boot pixel capture; disc-gated end-to-end oracle `crates/engine-core/tests/opening_full_chain_e2e.rs`):

| Scene | Content | Opening record + how it spawns |
|---|---|---|
| `opdeene` | Creation-myth crawl (14 + 8 pages) over the Genesis-tree vignettes | timeline P2[18], spawned by op `0x44` (`44 23`) in the P1[0] entry system script; ends with a `0x3F` SceneChange to `opstati` |
| `opstati` | Seru-intro crawls (3 + 6 pages) | P2[0], op `0x44` (`44 21`); chains to `opurud` |
| `opurud` | Mist-story crawls (4 + 3 + 5 pages) | P2[9], op `0x44` (`44 32`); chains to `map01` |
| `map01` | World-map fly-in: static "twilight of humanity" title card + a 5-page crawl over an aerial approach of Rim Elm | P2[38], spawned by the **walk-on tile trigger** at the arrival tile; scene-changes into `town01` at tile `(0x1D, 0x5B)` |
| `town01` | Establishing pan → **name entry** → Vahn's walk-out (the walk-out is post-confirm) | P2[3], walk-on tile trigger; C1 gate lists flag `0x225` (one-shot) |

A confirm press at any time after `opdeene`'s timeline arms `GFLAG 26` (near the record's top) fires the `FUN_801D1344` `town01` scene-change packet - that packet is the **intro SKIP**, not a required hand-off gate (the earlier "confirm-to-continue after the prologue" reading is superseded; the natural chain needs no input). See [`boot.md`](boot.md#the-opening-scene-chain--the-fun_801d1344-intro-skip).

### Record spawn mechanisms (live-probe-pinned)

An exec-breakpoint on the record dispatcher `FUN_8003BDE0` across the whole opening fires **exactly five times** - one opening record per scene, via two mechanisms:

- **Field-VM op `0x44` SPAWN_RECORD** (`opdeene` / `opstati` / `opurud`). The scene's P1[0] entry system script runs `[44, global_index]`; the dispatcher (`FUN_801DE840` case `0x44`, call site ra `0x801DF098`) hands it to `FUN_8003BDE0` with the gate forced to 1. The operand is a **GLOBAL** record index, re-based into partition 2 by subtracting the partition-0/1 record counts (`- N0 - N1`).
  The old "COUNTER" reading of op `0x44` is superseded - see [`script-vm.md`](script-vm.md#0x44-0x4f-record-spawn--camera--render--state--move-block). Engine: `legaia_engine_vm` decodes it as `SpawnRecord` and the host installs the record ([`FieldHost::op44_spawn_scene_record`](../../crates/engine-vm/src/field/host.rs) → [`World::install_spawned_record`](../../crates/engine-core/src/world/narration.rs)).
- **Walk-on tile trigger** (`map01` / `town01`). The per-frame tile trigger `FUN_801D1EC4` → `FUN_801D5630(1, x, z)` → `FUN_8003BDE0(x, z, rec[2], rec[3])` (ra `0x801D218C`): kind-1 records `[tile_x][tile_z][p2_record][gate]` in the scene `.MAP`'s `+0x10000` trigger block (and its `+0x12000` fallback window - see [`field-locomotion.md`](field-locomotion.md#trigger-block-0x10000---four-kind-sub-tables)).
  The scene-entry SEAT lands *on* the trigger tile, and the stale last-tile compare fires the same tick - so an arrival spawns the opening record immediately. `gate = 1` spawns the P2 record; `gate = 0` records are object-binds consumed at scene init (`FUN_8003A55C`) and never spawn.
  Engine: [`field_regions::TileTrigger` / `parse_tile_triggers` / `lookup_tile_trigger`](../../crates/engine-core/src/field_regions.rs), [`Scene::field_tile_triggers`](../../crates/engine-core/src/scene/scene_ty.rs), `SceneHost`'s `spawn_arrival_trigger_record`.

Before spawning, `FUN_8003BDE0` checks the P2 record's **C1/C2 story-flag gates** against the bitmap at `DAT_80085758` (`bit = byte[flag >> 3] & (0x80 >> (flag & 7))`): **C1 blocks the spawn if ANY listed flag is set** - the one-shot mechanism (`town01` P2[3] lists `0x225`, set once the opening has played) - and **C2 requires ALL listed flags set**. Engine mirror: [`World::p2_record_gates_pass`](../../crates/engine-core/src/world/narration.rs) over [`man_field_scripts::partition2_record_gates`](../../crates/engine-core/src/man_field_scripts/partitions.rs).

### The `opdeene` timeline record

`opdeene`'s timeline record (partition 2, record 18; record start at MAN offset `0xA47`) is a field-VM script that interleaves:

- camera staging - op `0x45` `Camera Configure` (a 23-byte payload block) and op `0x46` `RenderCfg`;
- actors - op `0x23` `MoveTo` and op `0x34` `Effect` spawns;
- the intro-skip arm - op `0x2E` `GFLAG_SET 26` (`2E 1A` at `0xA5E`);
- **inline narration text** (below);
- the terminal `0x3F` SceneChange to `opstati` (the natural chain hand-off).

### Inline narration format

The on-screen narration is carried as **inline ASCII text pages embedded in the timeline script**, not as a `MES` text id. A narration **block** is introduced by a field-VM op `0x4C` in its outer-nibble-8 form with the cross-context extended target `0xF8`:

```text
0xCC 0xF8 0x80 N        ; op (0xCC = 0x80|0x4C extended), N = page count
1F <ascii…> 00          ; page 1
1F <ascii…> 00          ; page 2
…                       ; N pages total
```

Each page is framed `0x1F <printable ASCII> 0x00` - `0x1F` (ASCII Unit Separator) starts a page, `0x00` terminates it, the body is plain 7-bit ASCII. The page count `N` in the introducing op equals the number of `0x1F`-framed pages that follow, which both validates the parse and gives a consumer the cadence for revealing subtitles.

A sibling **static title-card op** `[0xCC 0xF8 0x89 b1 b2]` carries the same `0x1F`/`0x00` page framing (after an optional short placement word) but presents differently: the pages show **simultaneously**, centered, while the parent script **continues**; a later card block whose pages are blank clears the card. `map01`'s fly-in uses it for the "twilight of humanity" title card + its blank-page clear. The parser distinguishes the two as [`NarrationKind::Crawl`](../../crates/asset/src/cutscene_text.rs) (`op0 = 0x80`) vs [`NarrationKind::Card`](../../crates/asset/src/cutscene_text.rs) (`op0 = 0x89`); the engine surfaces the card via `World::cutscene_card`.

`opdeene`'s timeline carries two crawl blocks: a 14-page creation prologue and an 8-page Seru-history block (22 pages total). The clean-room parser is [`legaia_asset::cutscene_text`](../../crates/asset/src/cutscene_text.rs) (`parse_narration` / `narration_pages`); it locates the introducing op and the page framing structurally and decodes the runtime disc bytes (no narration text is baked into the repo). Inspect it with:

```bash
legaia-engine man-scripts --scene opdeene --disc "<disc>.bin" \
  --narration --disasm-partition 2
```

The disc-gated test `crates/engine-core/tests/opdeene_narration.rs` ground-truths the structure (two blocks, 14 + 8 pages, every page non-empty ASCII, declared count matches decoded) without committing the text.

### Narration playback - the crawl roller (`FUN_80037174`)

The opening narration is a **bottom-up scrolling crawl**, not a one-caption-at-a-time presenter. The `[CC F8 80 N]` op routes to an on-screen-text actor whose handler is `FUN_80037174` (SCUS-static):

- one roller actor owns all `N` pages of a block, spawned as a **child context**: the PARENT timeline **keeps executing** while the pages scroll, so the camera cuts / fades / `WaitFrames` authored between the crawl blocks play **under** the scrolling text (a cold-boot capture of `opdeene` crawl-1 shows the eye cut from an establishing shot through the Genesis-grove foliage to the villager-tableau while the creation crawl scrolls continuously - the probe is [`scripts/pcsx-redux/autorun_crawl1_capture.lua`](../../scripts/pcsx-redux/autorun_crawl1_capture.lua)). The parent only blocks before a **new** crawl block (so two rollers never stack) and before the record's terminal SceneChange (so the final pages finish);
- each line is drawn centered with **all glyphs at once** (no typewriter), scrolling upward inside a clipped window; several lines are visible concurrently.

The geometry and speed are **pixel-capture-pinned** (PCSX-Redux cold boot, per-frame text-band tracking): 0.5 px/frame everywhere except `opurud` (1.0 px/frame); `opdeene` runs the tall window (enter ~y188, exit ~y64, 18 px line spacing, up to 8 lines visible); `opstati` / `map01` enter ~y203 and exit at y128 with 16 px spacing; `opurud` enters ~y187 and exits at y128.

The engine's [`CutsceneNarration`](../../crates/engine-core/src/cutscene_narration.rs) is that roller as a state machine, with per-scene [`RollerParams::for_scene`](../../crates/engine-core/src/cutscene_narration.rs) carrying the capture-pinned values. The timeline stepper installs each block's pages when its PC reaches the block's op ([`NarrationSite`](../../crates/engine-core/src/cutscene_timeline.rs)) and, mirroring the child-context spawn, lets the timeline **continue** (non-blocking) so the between-block camera cuts play under the crawl. It holds only for the **last** block (so the terminal SceneChange waits for the final pages) and when a new block would open over a still-scrolling one. `World::tick` advances the roller independent of the timeline; the host renders `visible_lines()`.

The `RollerParams` px/frame values are pinned against retail's **~60 Hz** field frames, but the engine sim ticks at **100 Hz** ([`redraw`](../../crates/engine-shell/src/bin/legaia-engine/window/event_handler/redraw.rs) `advance_tick(100)`). Advancing the roller once per sim tick would scroll it 1.67× too fast and drain the crawl ~6 s early, opening the inter-crawl gap. `World::tick` therefore drives the roller off a **60 fps sub-clock** (`field_frame_accum += 60; step = accum >= 100`, ~0.6 roller-frames per sim tick), so the crawl duration matches retail wall-time.

The residual dead-air between the two crawls (engine ~5 s vs retail ~3 s) is the ~800-frame small-`WaitFrames` camera choreography (`pc 0x2d9..0x88f`) that the engine runs **sequentially** before block 2 - a field-VM step-parallelism thread, not a roller-speed one.

#### Roller op operands (Ghidra-traced)

The spawner and the crawl-geometry config are two distinct sub-ops of field-VM op `0x4C` (`MENU_CTRL`), dispatched by `switch(op0 >> 4)` then `switch(op0 & 0xF)` inside `FUN_801DE840` (`see ghidra/scripts/funcs/overlay_0897_801e0c3c.txt`, cases `0x4C` nibble-8 sub-0 and nibble-E sub-8). Both take the cross-context target `0xF8` (the player / camera-anchor actor).

- **Spawn - `CC F8 80 N`** (op `0x4C`, op0 `0x80`: outer nibble `8`, sub `0`). Operand `N` is the **page count**. The op allocates a child actor from the template `DAT_801F28A0` (via `FUN_80020DE0`, which copies template `+0x8 = FUN_80037174` into the actor's handler word `+0xC`), points the child's script pointer `+0x90` at the operand's `N` byte, and leaves the following bytes framed `[N][page0][page1]…[page(N-1)]` (each page `0x1F <ascii> 0x00`). The parent then measures each of the `N` pages (`FUN_8003CA38`) to advance its own PC past the whole block and **continues** (non-blocking). The roller reads `N` back as the first byte at `+0x90`.

- **Geometry config - `CC F8 E8 …`** (op `0x4C`, op0 `0xE8`: outer nibble `0xE`, sub `8`; 10-byte op). It fetches four signed-16 LE words at operand `+1/+3/+5/+7` (`word0..word3`); `word3` selects the sub-mode:
  - `word3 == 0` seeds the three crawl-geometry globals at `_DAT_801C6EA4` (each word defaults if written as `0`): `+0x4C = word0` (default `0x40`), `+0x4E = word1` (default `0x08`), `+0x50 = word2` (default `0x04`).
  - `word3 == 1` finds the live roller by handler `FUN_80037174` (`FUN_8003CF04`) and either **pauses** it (`word0 == 0` sets actor `+0x10 |= 0x80000`) or writes the stop trigger `_DAT_801C6EA4 +0x52 = word0`.
  - `word3 == 2` **resumes** the roller (clears `+0x10 & ~0x80000`); `word3 == 3` unlinks the child and raises the terminal-kill flag (`+0x10 |= 8`).

  The task-name "`4C 88`" is a **different** op (op0 `0x88`, nibble-8 sub-8): it writes `_DAT_80084628/80084624/8008462C`, not the crawl geometry. The crawl-geometry seed op is specifically the nibble-E sub-8 (`0xE8`) form.

**Seed meaning** (from the roller's reads, `see ghidra/scripts/funcs/80037174.txt`):

| offset | role | default |
|---|---|---|
| `+0x4C` | window **top Y** (line base). Line `i` draws at `Y = (+0x4C) - subscroll + 16*i`. | `0x40` (64) |
| `+0x4E` | **visible line count** (window height in 16 px lines). Bottom clip `Y = (+0x4C) + 16*(+0x4E)`, clamped `<= 232`; also the length of the roller's per-line state array `actor+0x80…`. | `0x08` (8) |
| `+0x50` | scroll-cadence **divisor**. A per-actor accumulator advances by the scratchpad speed byte `DAT_1F800393`/frame; on reaching `+0x50` the 1 px sub-scroll `actor+0x9E` (0..15) steps and the accumulator resets, so `px/frame = DAT_1F800393 / (+0x50)`. | `0x04` (4) |
| `+0x52` | **stop-after-N-lines** trigger. When the lines-scrolled counter `actor+0x6A` reaches `+0x52`, the roller pauses (`actor+0x10 |= 0x80000`) and `+0x52` is cleared. Written only by the `word3 == 1` sub-mode. | (unset) |

A prior model - "one caption per page, 120 frames each, killing its predecessor, drawn at `Y = 180` / mid-screen" - described the separate **`4C E1` single-balloon op** (spawner `FUN_8003C764`, handler `FUN_801DA7F0`, dispatcher case at `0x801E30B8`/`C8`). That op is real but it is **not the crawl**.
The *"It was the Seru."* caption appears between `opdeene`'s two crawls, as a centered line over the villager-tableau shot (between the creation crawl's last page and the Seru-history crawl's first). It is **not a text balloon at all** and **not any live-rendered font string**: it is a **pre-rendered image**. The caption is a baked **112×32 4bpp TIM** (two CLUT palettes - the fade steps) in the `opdeene` geometry pack **PROT entry 0749** at LZS-decoded offset `0x01EC30`, VRAM `fb=(384,0)`, sitting among that pack's scene textures (the cloth grades, the Genesis-tree flame, the foliage; `tim-scan extracted/PROT/0749_opdeene.BIN`). The scene renderer draws it as a screen-space textured quad; there is no font string to source.

**Clean-room port.** The engine blits that scene texture rather than rendering text. On entering `opdeene`, [`cutscene_caption::decode_opdeene_caption`](../../crates/engine-core/src/cutscene_caption.rs) locates the 112×32 4bpp TIM in PROT 0749's LZS sections and decodes it to RGBA (its background palette entry is `0x0000`, so [`legaia_tim::decode_rgba8`] gives it alpha 0 - only the glyphs are opaque), stored on `World::cutscene_caption`.

[`World::tick`](../../crates/engine-core/src/world/frame_tick.rs) fades `cutscene_caption_alpha` in while the caption is target-visible - after the first crawl block scrolls out (`cutscene_narration_seq == 1` and narration inactive) - and back out; the host uploads the image once as a sprite atlas and emits one centered, alpha-tinted `SpriteDraw`. The caption is bounded to a retail-like ~2 s beat (`CAPTION_HOLD_FRAMES`) so the engine's currently-longer inter-crawl timeline gap doesn't leave it frozen; it also fades on the second crawl opening, whichever comes first. Disc-gated oracle: `crates/engine-core/tests/opdeene_caption_playback.rs`.

How the image origin was pinned (cold-boot probes in `scripts/pcsx-redux/`): a **text-path census** (`autorun_text_census.lua`) over the whole `opdeene` leg shows the only text renderers that fire are the crawl roller `FUN_80037174` and the MES glyph renderer `FUN_80036888`, both rendering **only** the 22 ASCII crawl pages (resident at `0x80109D89` / `0x8010A581`); the balloon spawner `FUN_8003C764`, text-actor register `FUN_8003541C`, single-line `FUN_8003CC98` and dialog-glyph emitter `FUN_8003C1F8` fire **zero** times.

A **blit census** (`autorun_seru_blit_probe.lua`) then confirms the image-blit `FUN_8002BDC4` and icon drawer `FUN_8002C488` also fire **zero** times, and MES rendering fires **not at all** during the caption window (it lands in the gap between crawl blocks). A **full-RAM dump** during display finds the string in **no** encoding (ASCII, 2-byte-glyph, or interleaved) - consistent with the pixels living only in VRAM, blitted from the baked TIM.
Note also that `FUN_8003CF04` is a list **finder** (walks `0x8007C34C` matching `node[+0xC] == handler && !(node[+0x10] & 8)`), not a kill function; the balloon's predecessor-kill lives in `FUN_801DA7F0`'s own first lines.

The narration does **not** gate the `town01` hand-off: the roller is timer-driven, and the `FUN_801D1344` packet is the intro **skip** - it fires mid-narration too, once `GFLAG 26` is armed ([`World::take_prologue_handoff`](../../crates/engine-core/src/world/narration.rs) tears down the playing narration / card / timeline wholesale). The disc-gated test `crates/engine-core/tests/opdeene_narration_playback.rs` cold-boots `opdeene` and drives the crawl blocks to completion; `opening_full_chain_e2e.rs` asserts the block cadence across all five scenes.

### Timeline execution model (Ghidra-traced)

The cutscene timeline runs on the **same field/event VM** (`FUN_801DE840`) as every other field script - there is no dedicated cutscene executor. The pieces:

- **Record header.** Partition-2 records are **named records**, *not* the partition-1 `[u8 N][N*2 locals][4-byte header]` shape. Layout: `[u8 name_len][name_len*2 SJIS name][u8 C0][C0 bytes][u8 C1][C1*u16][u8 C2][C2*u16]<script>`. The name length is in characters; the three condition-list gates are story-flag predicates the dispatcher tests before running the record (block 1 = OR gate, block 2 = AND gate; block 0 is skipped here). The script entry offset is `1 + name_len*2 + (1+C0) + (1+C1*2) + (1+C2*2)`. For `opdeene`'s record 18 (`name_len=6` "Opening", all blocks empty) that is `0x10` - the `0x34` EFFECT op (white fade-in) that opens the prologue, immediately followed by `GFLAG_SET 26` at `+0x17`. Decoder:
  [`man_field_scripts::partition_record_span`](../../crates/engine-core/src/man_field_scripts.rs) (`FUN_8003BDE0`).
- **Dispatch.** `FUN_8003BDE0` resolves a partition record by index, walks the header, and **spawns a VM context** (`ctx[+0x90]` = record base, `ctx[+0x9e]` = entry PC, `ctx[+0x10] |= 0x100` "run me"); the per-frame runner `FUN_80039B7C` then loops `FUN_801DE840` on it until a yield. The index comes from the two caller families [above](#record-spawn-mechanisms-live-probe-pinned) - an entry-script op `0x44` or a walk-on tile trigger (`FUN_801D1EC4`) - not a sequential partition walk.
- **Cross-context target `0xF8`.** Nearly every op in the timeline carries the extended-target byte `0xF8` (`A3 F8 …` = MoveTo, `CC F8 …` = MenuCtrl). `FUN_8003C83C(0xF8)` resolves to `_DAT_8007C364` - the **player / camera-anchor actor** - so the timeline drives the camera/lead actor.
- **Narration op.** `CC F8 80 N` (op `0x4C`, outer-nibble 8, sub-0) **spawns the roller child** - the on-screen-text actor whose handler is `FUN_80037174` - over the `N` inline pages; the parent timeline **keeps running** so the between-block camera choreography plays under the scroll (see [Narration playback](#narration-playback---the-crawl-roller-fun_80037174)). The single-line balloon path (`4C E1`, spawner `FUN_8003C764` → handler `FUN_801DA7F0`: centered `X = (320 − width)/2`, `Y = 180`, 120-frame timer, kills its predecessor) is a **different op** - not the opening crawl.
- **Camera Configure op `0x45`.** The CONFIGURE sub-path (`op0 & 0xC0 == 0`) reads a big-endian 10-bit field mask `(op0<<8)|op1`; bit `(9−i)` selects param `i`, each a signed-16 LE word written into the camera staging struct at `0x801C6EA8 + 0x02 + i*4`, followed by the commit `FUN_801DE084(struct, apply_trigger)`. The commit (`overlay_cutscene_dialogue_801de084.txt`) maps every param to a camera global:

  | param | struct off | global | role |
  |---|---|---|---|
  | 0 | `+0x02` | `_DAT_8007b790` | **pitch** (GTE `RotMatrixX` angle) |
  | 1 | `+0x06` | `_DAT_8007b792` | **yaw** (GTE `RotMatrixY` angle / heading) |
  | 2 | `+0x0a` | `_DAT_8007b794` | **roll** (GTE `RotMatrixZ` angle; zeroed in the field-camera build path) |
  | 3 / 4 / 5 | `+0x0e/12/16` | `_DAT_800840b8/bc/c0` | **eye-space translation trio** (post-rotation `(dx, dy, depth)`; the analog of the battle camera's `(0, 1280, 7680)` - slot 5 is the eye-back depth) |
  | 6 / 7 / 8 | `+0x1a/1e/22` | `_DAT_80089118/1c/20` | **camera focus** = the GTE translation `(-X, +Y, -Z)` |
  | 9 | `+0x26` | `_DAT_8007b6f4` | **GTE H** projection register (focal length / zoom) via `func_0x8003d254` = `setCopControlWord(2, …)` |

  The focus trio is the high-confidence pin: three independent consumers store the *negated* world focus there - the follow-cam `FUN_801DBE9C` sets `_DAT_80089118 = -(anchor+0x14)` (−X) / `_DAT_80089120 = -(anchor+0x18)` (−Z); the culling test `FUN_80021DF4` reads `-_DAT_80089118` as the world focus X; the smooth-scroll in `overlay_0896_801ca998` targets `tile*-0x80 - _DAT_80089118`. So the world focus is `(-param6, param7, -param8)` (Y is stored un-negated, per the camera-param builder `FUN_801DAB90`).

  Each focus slot is applied **independently** on its own presence, mirroring the apply handler `FUN_801DE084` writing each focus global only when its slot bit is set (an absent slot leaves its global at the prior beat's value). `opdeene`'s opening beats supply focus X/Z (slots 6/8) but **never slot 7** (focus Y) - so a beat still pans horizontally, and Y holds.
  Both engine-side consumers apply per-axis: `engine-core`'s `Camera::route_camera_events` (an earlier all-or-nothing `(slot6, slot7, slot8)` gate never retargeted these beats, freezing the shot) and the shell's `cutscene_view`, which falls the absent focus Y back to retail's `0` (the vertical framing rides the eye-space Y offset in the translation trio, not the focus Y - so keying it on the lead actor's field cold-spawn `Y=0`, or on the scene-AABB centre, is unnecessary).

  **The full transform is `screen = H · (R·(v − focus) + tr_eye) / Ze`; the eye-back depth is `tr_eye.z` (slot 5), not a missing scalar.** The once-per-frame view builder `FUN_800172c0` assembles it: build `R` from the angle globals (`FUN_80026988`), left-multiply the constant base matrix `DAT_8007BF10` (a uniform `24576·I` = **6× world scale**), copy the eye-space translation trio `_DAT_800840B8/BC/C0` into the view struct's `.t`, then MVMVA the negated focus `(_DAT_80089118/1C/20)` through `R` and add `.t` - giving the uploaded GTE translation `TR = R·(−focus) + tr_eye`, so every world vertex maps to `R·(v − focus) + tr_eye`.
  The camera-rotation build is pinned: `FUN_8001CF50` composes `R` by rotating about each axis with the angle globals - `RotMatrixX(pitch=_DAT_8007B790)` at `0x800461A4`, `RotMatrixY(yaw=_DAT_8007B792)` at `0x8004629C`, `RotMatrixZ(roll=_DAT_8007B794)` at `0x8004638C` (each masks the angle to 12 bits and indexes the shared sin/cos LUT at `0x80070A2C`, `4096 = 360°`, `+0x800` = the quarter-wave cosine offset; composed via GTE `mvmva`).
  **So param 0 is the camera PITCH, not a "rot/zoom" word** - the zoom is H (a separate projection register). The eye sits *behind* the focus by `tr_eye` (in the 6×-scaled space); it is NOT at the focus.
  The per-frame *interpolation* is also pinned: `FUN_801DB510` eases the focus globals, the eye-space translation trio, and the typed `0x801F2798` param table toward their control-block targets every frame with an exponential right-shift lerp (`srav` by `_DAT_8007B60B>>4`), so Camera Configure beats blend rather than snap.
  Confirmed against the `new_game_cutscene_intro_a` save state: focus `(8640, 0, 10304)` (mode byte `0x10` = anchor-follow), pitch `180` (≈15.8°), yaw `-2967`, roll `0`, H `792`, `tr_eye = _DAT_800840B8 = (260, 1293, 17145)`; the focus projects to screen `(792·260/17145 + 160, 792·1293/17145 + 120) = (172, 180)`, matching the party position in that frame's framebuffer.
  The captured RAM is the interpolated tween between two op-`0x45` keyframes (`opdeene` beat 0 `tr_eye = (−740, 512, 16384)`, focus `(10816, ?, 12224)`; a later beat `tr_eye = (118, 2241, 20795)`, focus `(5824, ?, 1984)`) - every axis of the capture sits between them. Note (don't re-walk): the GTE rotation matrix read straight from a save state is the last-rendered object's composed transform (row norms ≈ 6.0 = the base-matrix world scale), so recover `R` from the angle globals - but that `6.0` **is** the camera world scale, folded into `R` via `DAT_8007BF10`.

### Timeline execution (engine port)

The engine **executes** this timeline as a spawned field-VM context. On entering `opdeene` live, [`World::load_cutscene_timeline_from_man`](../../crates/engine-core/src/world/narration.rs) locates the partition-2 record that issues `GFLAG_SET 26` (via [`man_field_scripts::walk_partition_gflag_sites`](../../crates/engine-core/src/man_field_scripts.rs)), resolves its named-record span, and installs a [`CutsceneTimeline`](../../crates/engine-core/src/cutscene_timeline.rs) - a second `FieldCtx` separate from the scene-entry system script on `World::field_ctx`, seeded on the system channel (`script_id = 0xFB`) so cross-context (`0x80`-bit) ops keep running after the record's first yield sets the context halt bit.
The `opstati` / `opurud` legs install theirs through the faithful op-`0x44` spawn instead ([`World::install_spawned_record`](../../crates/engine-core/src/world/narration.rs)); `map01` / `town01` through the walk-on tile trigger ([above](#record-spawn-mechanisms-live-probe-pinned)).

Only **cutscene-class** records (the opening chain, and gated walk-on beat records via `install_gated_p2_record`) install as this modal timeline (camera seize + locomotion lock). An ordinary scene's mid-play op-`0x44` spawn installs as a **concurrent helper context** instead - `World::helper_contexts` (bounded table mirroring retail's small fixed context set), installed by [`World::install_spawned_helper_record`](../../crates/engine-core/src/world/narration.rs) and stepped by `step_helper_contexts` through the same run-until-yield slice (`run_spawned_record_slice`) - without seizing the camera, locking locomotion, or reading as `cutscene_timeline_active()`. Pending spawns queue (FIFO) rather than dropping while another record executes.

[`World::step_cutscene_timeline`](../../crates/engine-core/src/world/narration.rs) runs that context through the same `legaia_engine_vm::field::step` each frame, run-until-yield (mirroring retail's per-frame dispatch), bounded by a per-frame step budget and a frame cap. The Camera Configure (`0x45`) and `MoveTo` (`0x23`) ops emit the same [`FieldEvent`](../../crates/engine-core/src/field_events.rs)s the runtime [`Camera`](../../crates/engine-core/src/camera.rs) folds in; the `GFLAG_SET 26` near the record's top arms the **intro skip** through the same host path the main field VM uses; and the record's terminal `0x3F` SceneChange chains the next opening leg - all **by execution**, not by a static MAN-walk derivation.
The static arm ([`World::arm_prologue_handoff_from_man`](../../crates/engine-core/src/world/narration.rs)) remains as a fallback for a scene whose timeline record can't be resolved, and a safety net arms it if execution can't reach the arming op within the frame cap, so the prologue can never stall.

Two overlay-variant pins from the live opening run:

- **Op `0x4C` nibble-4 sub-9 (`4C 49`) never jumps in the cutscene-dialogue overlay.** Its case 9 (`overlay_cutscene_dialogue_801de840.txt`, around the `_DAT_1f800394 & 0x1000000` test) selects a **write variant**: bit 25 → Delta (write/ramp target slot + the delta global), bit 24 → **player-relative write** (`+0x4A = value + player_anchor[+0x16]`), else Default - always advancing 6 bytes. The field-overlay-0897 dump's absolute-jump arm does **not** apply to the opening path (live probe: `opurud`'s entry script reaches its `44 32` at `+0x7A` with bit 24 set, which an abs-jump arm would have made unreachable). Engine: [`Sub9State::PlayerRelative`](../../crates/engine-vm/src/field/types.rs) replaces the earlier `AbsJump`.
- **`4C 9F` (nibble-9 sub-F register-callback, `LAB_801DA930` via `0x8003CF40`) never fires during the opening** (live probe: zero exec hits on the callback). `FUN_8003CF40` only sets `node[+0x10] |= 8` on an already-live actor whose entry equals the callback - inert when none is live. The engine's host hook ([`FieldHost::op4c_n9_sub_f_register_callback`](../../crates/engine-vm/src/field/host.rs)) reports "already satisfied" during the opening chain so the entry script proceeds to its op-`0x44` spawn.

Two single-shared-VM accommodations, **approximate by design**:

- **Narration blocks spawn the roller and let the timeline continue.** The inline page bytes are data, not opcodes, so the stepper never walks the VM into them: [`World::install_cutscene_timeline_record`](../../crates/engine-core/src/world/narration.rs) parses each block into a [`NarrationSite`](../../crates/engine-core/src/cutscene_timeline.rs) (`op_offset` + [`byte_span`](../../crates/asset/src/cutscene_text.rs) end + pages + kind).
  When the PC reaches a crawl site, the stepper installs the pages on the roller presenter and, mirroring retail's child-context spawn, **advances the PC past the block** so the between-block camera cuts play under the scroll - non-blocking. Two exceptions still hold the timeline (`CutsceneTimeline::narration_pc`): the **last** crawl block of a scene blocks until its pages scroll out (so the record's terminal SceneChange doesn't cut them off), and a block reached while a prior roller is still scrolling holds (`narration_pending_open`) so two rollers never stack. A card site installs `World::cutscene_card` (blank pages clear it) and the parent continues, per the retail card semantics.
  (The earlier NOP-fill of the narration span, the scene-entry page install, and the per-page confirm-skip are gone; the earlier "park the whole crawl" model is superseded - it serialized the camera cuts after the text instead of playing them under it.)
- **Camera params (per-slot merge).** The op-`0x45` events flow to the `Camera` controller and the host **merges** each beat's masked slots into a persistent `World::camera_state.params` set.
  This mirrors retail's `FUN_801DE084`, which writes each masked param into a persistent camera struct slot (`0x801C6EA8 + 0x02 + i*4`) - a beat that omits a slot keeps its prior value.
  It matters: one of opdeene's nine op-`0x45` beats sets **only slot 9 (H)** (`[(9, 792)]`), so a wholesale replace would drop that shot's focus / pitch / eye-depth and snap the camera to `cutscene_view`'s fall-back framing (lead-actor focus + default depth); the per-slot merge keeps the staged shot and only tweaks the focal length.
  The set is cleared on scene entry so cutscene shots don't leak across scenes.
- **Camera model.** The native `play-window` renders the cutscene with the **exact retail GTE model** whenever a cutscene timeline is installed: the shell's `compute_scene_camera` cutscene branch builds `psx_camera_mvp(pitch, yaw, H, tr_eye, focus)` (the same `screen = H·(R·(v − focus) + tr_eye)/Ze` builder the field follow camera uses; `FUN_800172c0`), composed with `FIELD_WORLD_FLIP` exactly like `field_follow_camera_mvp` (the internal Y-flip and the world flip cancel, so the raw retail Y-down `focus` and native-`1×` geometry pass through unchanged).
  `SceneHost`'s `cutscene_view` decodes the pinned params: **focus** `(-param6, param7, -param8)` (Y defaults to retail's `0`), **pitch/yaw** from params 0/1 (`4096` = full turn), **H** straight from param 9, and **tr_eye** = the eye-space translation trio (params 3/4/5, `0x800840B8`) - the eye-back depth is `param5`. There is **no eye-distance heuristic**: the depth is a real decoded param.
  Because retail folds a `6×` world scale into `R` (base matrix `DAT_8007BF10`) while the engine renders geometry at native `1×`, `tr_eye` is divided by `6` - the perspective divide makes `6×`-geometry-at-`z` and `1×`-geometry-at-`z/6` project to identical pixels (the same `depth/6` trick `field_follow_camera_mvp`'s `FIELD_CAM_DEPTH = 1200 = 7200/6` uses). `opdeene` supplies all three offset slots per beat.
  The shot re-targets each time the timeline executes a new Camera Configure op; rather than
  cutting, `play-window` eases the rendered `(focus, pitch, yaw, H, tr_eye)` toward each new beat
  through [`window::CutsceneCameraInterp`](../../crates/engine-render/src/window.rs) (per-frame
  ease, angles along the shortest arc, reset to snap when the timeline first installs) - mirroring
  retail's own per-frame `FUN_801DB510` exponential ease. The ease **rate is the beat's
  `apply_trigger`**: a Configure with `apply == 0` commits the camera targets immediately (a hard
  cut), while `apply > 0` stages them and lets the ease glide the eye toward them over roughly
  `apply` frames (`t ≈ 4/apply`, clamped) - the same snap-vs-tween split `FUN_801DE084`'s
  `apply_trigger` selects. opdeene mixes both: the entry shot snaps (`apply 0`), but the
  mid-prologue forest dolly is `apply 840`, paired with a `760`-frame `WaitFrames`, so the camera
  glides continuously *while the narration crawl scrolls* rather than snapping to a still hold. The
  ease is stepped in **sim-tick time** (once per world tick that elapsed, not once per rendered
  frame), so an `apply`-paced glide spans its authored sim-frame count even across a long
  `WaitFrames` where few ticks advance but many redraws fire - without that, the dolly converged in
  a fraction of a wall-clock second and then froze into a dead static hold.
  The framing is pinned by the disc-free regression tests `cutscene_framing_tests` (focus → `(172, 180)`; a `133`-unit character subtends the retail ~1/6-frame height, upright). The legacy orbit-radius framing [`window::cutscene_camera_mvp`](../../crates/engine-render/src/window.rs) is retained only as a unit-tested reference, no longer wired into a render path.

The same machinery drives the **`town01` opening** (a sibling partition-2 record, `P2[3]`). It installs two ways: the **natural chain arrival** from the `map01` fly-in fires the walk-on tile trigger at the entry tile `(0x1D, 0x5B)` (C1 gate `0x225` makes it one-shot), and the **intro skip** ([`World::take_prologue_handoff`](../../crates/engine-core/src/world/narration.rs)) sets `entering_town01_opening` so the `town01` field entry installs the record via [`World::install_town01_opening_timeline`](../../crates/engine-core/src/world/narration.rs) - which honors the record's C1/C2 header gates, so both routes share the retail one-shot.
The one-shot writes itself: the record's opening `52 25` script bytes SET its own C1 gate flag `0x225` (549) when the timeline executes (disc-gated `organic_beat_records_disc.rs`), the same self-latch shape as the rikuroa post-victory record.
Two differences from the opdeene prologue:

- **It does not chain onward.** opdeene's timeline carries `arms_prologue_handoff` (its `GFLAG_SET 26` / the frame-cap safety net arms the skip) and ends in a `0x3F` SceneChange; `town01` is the destination, so its opening's completion just drops the timeline (reverting the cutscene camera to normal field gameplay) and un-parks the townsfolk the establishing shot hid.
- **It opens name entry at op `0x49`.** `step_cutscene_timeline` steps past the conditional-wait parks the engine doesn't model - `0x4C` nibble-C `script_alloc` / globals-gate and `0x2D` / `0x30` flag-tests, all handshakes a spawned sub-context would satisfy - by their encoded width, while keeping `0x4A` WAIT_FRAMES (a timed wait that plays out over frames) and `0x49` STATE_RESUME parking. The retail order is establishing pan → **name entry** → **Vahn's walk-out** (the walk-out is post-confirm): the pinned op `0x49` at body `0x02c6` opens the *"Select your name."* overlay through the op-49 host hooks (`op49_invoke_setup` → [`World::open_name_entry`](../../crates/engine-core/src/world/narration.rs); `op49_state` reports Armed while the overlay is up, Done once a name commits).
  The timeline is frozen while name entry is open (the STATE_RESUME suspend) and resumes - playing the walk-out - when the player names the lead. See [`boot.md`](boot.md#name-entry-overlay).

Disc-gated coverage: `crates/engine-core/tests/opening_full_chain_e2e.rs` drives the whole zero-input chain (`opdeene` → `opstati` → `opurud` → `map01` → `town01` name entry, asserting each hand-off + the narration-block cadence) and the confirm-skip path;
`opdeene_timeline_execution.rs` cold-boots `opdeene`, asserts the timeline installs with the skip bit clear, ticks until it arms by execution, and follows the terminal SceneChange;
`town01_opening_name_entry_wiring.rs` drives the `town01` opening end to end (install → camera/wait beats → name entry opens at op `0x49` → freeze → commit → resume → drop); `town01_opening_timeline_trace.rs` pins the op-`0x49` site.
The CI synthetic `cutscene_timeline_synthetic.rs` exercises both paths (GFLAG-by-execution + safety net + idempotent completion; op-`0x49` name-entry open / freeze / resume) without disc data.

### Per-actor channels - the vignette actors

The "characters doing things" during the narration are **per-actor script channels**.
Retail spawns one script context per MAN partition-1 placement record at scene entry
(`FUN_8003A1E4`, called per record `1..N1` by the scene setup `FUN_8003AEB0`): the record
base becomes the context's bytecode buffer (`actor[+0x90]`), its first opcode the entry PC
(`actor[+0x9E]`), and its script id (`actor[+0x50]`) is `partition-0 count + placement index` -
the id space the cross-context (`0x80`-bit) ops resolve through `FUN_8003C83C`.
The opdeene timeline drives them: after the camera-configure opening it **halt-acquires**
channels `0x05..0x0F` (a sweep of `4C 85` freezes = op `0x4C` n8 sub-5 against each target),
then pokes them beat by beat - a `4C 45` (n4 sub-5) parameter write, a `4B` ANIMATE cue, an
`A3`/`23` MoveTo. Each poked channel's own placement script responds by playing its animation /
walking to its mark, then signalling completion via a context flag the timeline waits on
(`B3 <id> <bit>` = cross-context `CFLAG_TST`).

The engine mirrors this in
[`legaia_engine_core::field_channels`](../../crates/engine-core/src/field_channels.rs):
[`spawn_channels`](../../crates/engine-core/src/field_channels.rs) builds one
[`FieldChannel`](../../crates/engine-core/src/field_channels.rs) per placement (with the retail
script-id rule), spawned alongside a cutscene timeline in
[`World::install_cutscene_timeline_record`](../../crates/engine-core/src/world.rs).
[`World::step_field_channels`](../../crates/engine-core/src/world.rs) runs each live channel one
frame-slice per tick (mirroring `FUN_80039B7C`'s per-actor loop: ops until a yield, a park, or a
`0x21` NOP - the retail frame-pacing point, which is why placement idle loops are
`21 21 26 FE FF`), and the timeline's cross-context pokes run against the resolved channel context
(the acquirer clears the target's halt bit - the poke from the owner is the resume signal).
Scripted moves write through to `World::field_npc_positions` so the field render + interact probes
follow, and `0x4B` ANIMATE cues land in `World::field_npc_anim_cues` keyed by placement.
The play-window render drains those cues each frame and **re-targets the NPC's clip player** to
the cued bundle record (`record = anim id - 1`, the same rule as the placement anim byte), so the
vignette actors perform their scripted beats instead of looping the placement clip. Simplified:
the cue's per-keyframe parameter words are not modelled - the cued record plays as a loop until
the next cue.
Channels are cutscene-scoped: they drop when the timeline completes, so normal field NPC behaviour
(the decoded-waypoint motion substitute) is untouched outside cutscenes.
Disc-gated `crates/engine-core/tests/opdeene_field_channels.rs` cold-boots `opdeene`, asserts 13
channels spawn with the right ids, and observes them execute + raise animate cues + take timeline
pokes.

**Placement-default idle clips (the vignette-liveness source).** Even a halt-acquired channel keeps
playing its clip: retail's per-actor animation tick (`FUN_8003BC08 → FUN_80021DF4`) advances each
actor's keyframe interpolation every frame *independent of the parked script PC*, so a vignette actor
cued with its placement anim byte animates through the whole narration crawl. The play-window render
mirrors this - it builds a looping [`FieldClipPlayer`](../../crates/engine-core/src/field_anim.rs)
from each on-screen placement's default anim id (`record = anim id - 1`) and ticks it every frame,
gated only on Field mode, not on the channel's halt state or the timeline park. The clip source is
the **per-scene ANM bundle** (`player_anm::find_in_entry`, the type-`0x05` section of the scene's
first PROT slot). That lookup is seeded with a descriptor count and the count is **not uniform**:
`town01`'s bundle surfaces at count `3`, but the opening prologue scenes stash theirs deeper -
`opdeene` (PROT entry 749), `opstati` (754), and `opurud` (764) only surface at count `≥ 5`. The
render path searches counts `[3, 5, 6, 7]` and takes the first bundle any entry yields; hardcoding
`3` resolved *no* bundle for the three prologue scenes, so their vignette actors got no clip player
and rendered as a **frozen tableau** under the crawl (the "3D isn't playing while the text scrolls"
gap). Disc-gated `opening_scene_anm_bundle.rs` pins the invariant (`town01` at 3, prologue scenes
need `≥ 5`).

**Channel-completion handshake (`CFLAG_TST` / halt-acquire state-resume).** A cross-context
`CFLAG_TST` (`B3 <id> <bit>` = op `0x33` with the `0x80` bit, targeting a spawned channel's
`ctx[+0x50]` id and testing `ctx.flags & (1 << bit)`) is the beat-completion wait: retail's
`4C 85` acquire freezes the channel, a poke drives its beat, `B2 <id> 0A` resumes it, and the
timeline **halts** at the `B3` until the channel raises its completion bit (its own placement
script runs `0x31 CFLAG_SET` when the move/anim finishes). `step_cutscene_timeline` models that
handshake: on a failing cross-context `0x33` it **PARKS** ([`CutsceneTimeline::channel_wait`]) -
holding the PC on the flag-test op and, each subsequent tick, re-testing the awaited channel's
bit, resuming past the op only once it is set (`step_field_channels` steps the real channel scripts
each tick, so a channel whose beat completes raises the bit and the park clears). The park is
bounded by `CHANNEL_WAIT_PARK_TIMEOUT`: a channel our port cannot advance to its flag-set falls
back to the by-width step-past (the prior behaviour) so the prologue never stalls. Bit 10 (`0x400`,
the halt/busy bit the acquire sweep toggles) is a suspension *verify* (`B3 <id> 0A`), not a
completion wait, so it keeps the width step-past; the local/global flag-tests `0x2D`/`0x30` and a
bare (non-cross-context) `0x33` step past too. Real timed `0x4A` WAIT_FRAMES and the `0x49`
STATE_RESUME name-entry suspend are still honoured. Unit-covered by
`cutscene_timeline_parks_on_channel_wait_until_flag_set` (parks while the flag is clear, resumes
the tick it is set) + the timeout fallback.

**Player-channel (`0xF8`) ExecMove / halt-acquire completion.** Door-cutscene records drive the
**player** through the same handshake: `A2 F8 <move_id>` (ExecMove) pokes a move-table clip onto
the player object, then `C3 F8 <sub> …` (op `0x43` sub-0/1/A/B halt-acquire) halts the caller and
state-resumes it at the operand s16 once the move completes - a resume PC pointing **backward**
into the poke loop (jou's castle-door record `P2[5]`: `C3 F8 00 5E E2 50` at `+0x60` resumes at
`+0x50`; the record's terminal `0x3F` to `jouina` sits at `+0xD0`). Retail resolves `0xF8` to the
live player object (`_DAT_8007C364`, `FUN_8003C83C`); the engine spawns no player channel, so
[`field_channels::resolve_target`](../../crates/engine-core/src/field_channels.rs) keeps its
`None`-for-`0xF8` contract and `run_spawned_record_slice` models the two ops directly: the
ExecMove emits the same `ExecMove` field event and arms a short in-flight countdown
([`CutsceneTimeline::player_move_frames`](../../crates/engine-core/src/cutscene_timeline.rs),
standing in for the playout since engine player pokes complete synchronously), and the
halt-acquire **parks** at the op ([`CutsceneTimeline::player_wait`]) until the countdown drains,
then steps **past** it by encoded width - the completion side of the handshake - so the record
flows on to its trailing scene change instead of taking the backward yield into a spin. A
halt-acquire with no move in flight completes at once; the op-`0x38` halt-acquire variant resumes
forward at its post-instruction PC, so its plain yield already reads as completion. Unit-covered
by `cutscene_timeline_player_channel_door_reaches_scene_change`; the disc-gated
`chapter1_hub_depth_oracle` drives the jou castle door through this path to
`SceneEntered("jouina")`.

### Colour fade (op `0x34` sub-0)

The prologue opens on a white flash: the timeline's first op is `34 05 FF FF FF 00 00` = op
`0x34` sub-0 (effect-global colour + intensity, `FUN_801E1FB0`). Retail clears the active fade
when the colour is all-zero, else schedules a fade of that colour with a direction from `op0 & 1`.
The engine ports the *setup* to [`fade::ColorFade`](../../crates/engine-core/src/fade.rs) (a
colour + a coverage ramp; `op0 & 1` selects a reveal / fade-from-colour vs a fade-to-colour),
driven by
[`FieldHostImpl::op34_sub0_color_intensity_setup`](../../crates/engine-core/src/world/vm_hosts.rs)
into `World::color_fade`, stepped per `World::tick` and drawn by `play-window` as a full-screen
semi-transparent wash while active. **Approximate by design:** the retail fade actor's per-frame
*draw* handler is not dumped, so the coverage curve + PSX blend mode aren't pinned - the render is
a 50%-average (ABR 0) wash that lifts as the ramp completes, pending that dump.

### Full-scene sepia grade (the gold prologue look)

The whole prologue-cutscene leg of the opening renders in a **persistent warm gold/amber
monochrome** - every 3D surface (terrain, foliage, the vignette actors) is tinted gold while the
white narration text stays white. It is distinct from the transient colour fade above. The
cold-boot pixel capture pins its scope: the grade **persists across `opdeene` / `opstati` /
`opurud`** and drops for the full-colour `map01` fly-in and `town01`.

**Retail mechanism (traced two ways).** The grade is a GTE render-time effect, not baked
geometry and not a `MES`/texture change:
- The TMD renderer `FUN_8002735C` runs the GTE **DPCS** depth-cue per primitive:
  `out = base + IR0·(far − base)`, where the **far colour** (GTE control regs 21/22/23 = RFC/GFC/BFC)
  comes from each render node's `+0x74` and `IR0` from `+0x78`. Setting a gold far colour with a
  non-zero IR0 pulls every object uniformly toward gold - the exact tool for a scene-wide grade.
- The GTE **back/ambient colour** `DAT_8007B788` ("light_back_color") is `0x00202020` (dim,
  R=G=B=32) in `opdeene` vs `0x00FFFFFF` (white) in `town01`, staged into GTE cr13-15 by
  `FUN_80043390` - the darkening half of the look. (Byte-exact across save states.)

The `opdeene` MAN itself carries **no** colour op (no op `0x4C 0x8A` ambient, no `0x4C 0x81` far
colour); it drives op `0x46` depth-fog and op `0x4C 0x12` fade-to-black only. So the gold is set by
the **cutscene-host overlay during the narration beats**, not the field script. Measured off the
retail cutscene framebuffer, the grade collapses all hues to amber: average RGB `(61, 55, 15)`,
`G/R ≈ 0.90`, `B/R ≈ 0.24`, zero surviving green/blue.

**Engine port.** Rather than replicate the per-object GTE far-colour plumbing, the engine
reproduces the measured *look* with a single luminance→gold tone-map:
[`fade::ColorGrade`](../../crates/engine-core/src/fade.rs) holds the gold direction + strength
([`ColorGrade::PROLOGUE_SEPIA`](../../crates/engine-core/src/fade.rs)), and
[`World::scene_color_grade`](../../crates/engine-core/src/world/narration.rs) returns it while
the active scene is one of the prologue cutscene legs (`opdeene` / `opstati` / `opurud`) and
`None` for every other scene (including `map01` / `town01`). `play-window` stages
it into the renderer each frame ([`Renderer::set_color_grade`](../../crates/engine-render/src/renderer.rs));
the field mesh shaders' `apply_grade` maps each shaded pixel to `luminance · gold` cross-faded by
`strength` (the text/UI overlays use separate shaders, so the narration stays white). The gold
coefficients are stored in **linear** space (the shader multiplies before the sRGB framebuffer
encode, gamma ≈ 2.0), i.e. the display targets squared: `(1.0, 0.90², 0.24²)`. Verified
pixel-aligned against a pure-diagnostic grade - the encoded output lands `G/R ≈ 0.90`, `B/R ≈ 0.24`.
`scene_color_grade_only_on_the_prologue_cutscene` (engine-core) guards the scene gate.

## Open items

- **FMV dispatch table - decoded from disc.** The play loop `FUN_801CF098` (1236 B) is reached from the selector at `0x801CECA0` (`_DAT_8007BA78 << 6 + 0x801D0A6C`), and that dispatch table is **static overlay data** now decoded straight from the disc (`legaia_asset::fmv_dispatch`): each `fmv_id`'s movie + frame range, used to seek to the right segment (`cutscene_av::fmv_segment_window`). The STR overlay (PROT 0970) is Ghidra-importable at its base, so the master-dispatch is a static decompile, no capture. Still finer-grained: the XA channel selector + the MDEC frame-demux state machine.
- **XA channel map.** `(file_no, ch_no)` → cutscene-name association is inside the STR/MDEC overlay. The MV-file table doesn't carry XA channel info directly; the channel selector is presumably driven by `\DATA\MOV.STR;1` (which appears to be a multi-channel container distinct from the per-cutscene `\MOV\MVn.STR;1` files).
- **MOV15.STR + MV1A.STR.** Two extra path strings (`\DATA\MOV15.STR;1` and `\MOV\MV1A.STR;1`) appear alongside the six numbered MVs. These are dev / debug branches: `MOV15` is the 15-FPS test file (referenced by the `psx.cdspeedup` / 15 fps debug paths), and `MV1A` is an alternate / cut version of MV1. Neither ships in the released disc layout.
- **8-bit ADPCM.** `coding_info` width detection drives a real 8-bit group decoder (`BitsPerSample::Eight`: 4 units/group, full-byte samples). No 8-bit audio has been observed in the corpus, so the path is covered by synthetic unit tests rather than a bit-exact reference.

## Provenance

| Subject | Source |
|---|---|
| STR sector header layout | `crates/mdec/src/str_sector.rs`; PSX-SPX §STR Video Files |
| Iki AC VLC table + LZSS qscale/DC table | `crates/mdec/src/lib.rs`; PSX-SPX BS-compression pages + jPSXdec `PlayStation1_STR_format.txt` (format docs) |
| IDCT + dequantize formula | `crates/mdec/src/lib.rs`; PSX-SPX §MDEC |
| BT.601 coefficients | `crates/mdec/src/lib.rs` |
| XA sector layout + demux | `crates/xa/src/demux.rs`; [`formats/xa.md`](../formats/xa.md) |
| Interleaved STR A/V decode + sync clock | `crates/engine-shell/src/cutscene_av.rs` |
| Audio-cursor playback clock | `crates/engine-audio/src/lib.rs` (`AudioOut::xa_cursor_secs`) |
| Game modes 26 / 27 | `crates/engine-core/src/mode.rs` |
| `play-str` frame loop | `crates/engine-shell/src/bin/legaia-engine.rs` (`cmd_play_str` / `StrPlayerApp`) |

## See also

**Reference** -
[STR FMV table](../formats/str-fmv-table.md) ·
[XA audio](../formats/xa.md) ·
[Audio stack](audio.md) ·
[Field/event VM](script-vm.md)
