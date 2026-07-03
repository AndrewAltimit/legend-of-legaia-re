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

The corpus is codified at `legaia_engine_core::capture_observations::cutscene_trigger_corpus` and exercised by the disc-gated test `crates/mednafen/tests/real_saves.rs::cutscene_trigger_corpus_pins_fmv_id_across_nine_saves`.

## In-engine 3D cutscene (`opdeene` opening prologue)

Not every cutscene is an STR FMV. The New Game opening - the "Genesis tree" prologue with the *"…the Seru."* narration - is an **in-engine 3D cutscene scene** (`opdeene`, CDNAME/PROT #748), a field scene running in master game-mode `0x03` (field RUN), not a `MOV/MVn.STR` video. (`MV1.STR` is the title-attract movie; the opening 3D sequence is engine-rendered - see [`boot.md`](boot.md#opdeene--town01-handoff-scene-change-packet).)

The cutscene plays out of the scene MAN's **cutscene-timeline partition** (partition 2). Its closing record (record 18; record start at MAN offset `0xA47`) is a field-VM script that interleaves:

- camera staging - op `0x45` `Camera Configure` (a 23-byte payload block) and op `0x46` `RenderCfg`;
- actors - op `0x23` `MoveTo` and op `0x34` `Effect` spawns;
- the `town01` hand-off arm - op `0x2E` `GFLAG_SET 26` (`2E 1A` at `0xA5E`);
- **inline narration text** (below).

### Inline narration format

The on-screen narration is carried as **inline ASCII text pages embedded in the timeline script**, not as a `MES` text id. A narration **block** is introduced by a field-VM op `0x4C` in its outer-nibble-8 form with the cross-context extended target `0xF8`:

```text
0xCC 0xF8 0x80 N        ; op (0xCC = 0x80|0x4C extended), N = page count
1F <ascii…> 00          ; page 1
1F <ascii…> 00          ; page 2
…                       ; N pages total
```

Each page is framed `0x1F <printable ASCII> 0x00` - `0x1F` (ASCII Unit Separator) starts a page, `0x00` terminates it, the body is plain 7-bit ASCII. The page count `N` in the introducing op equals the number of `0x1F`-framed pages that follow, which both validates the parse and gives a consumer the cadence for revealing subtitles.

`opdeene`'s timeline carries two blocks: a 14-page creation prologue and an 8-page Seru-history block (22 pages total). The clean-room parser is [`legaia_asset::cutscene_text`](../../crates/asset/src/cutscene_text.rs) (`parse_narration` / `narration_pages`); it locates the introducing op and the page framing structurally and decodes the runtime disc bytes (no narration text is baked into the repo). Inspect it with:

```bash
legaia-engine man-scripts --scene opdeene --disc "<disc>.bin" \
  --narration --disasm-partition 2
```

The disc-gated test `crates/engine-core/tests/opdeene_narration.rs` ground-truths the structure (two blocks, 14 + 8 pages, every page non-empty ASCII, declared count matches decoded) without committing the text.

### Narration playback

Entering `opdeene` live installs the decoded pages on the world ([`World::open_cutscene_narration`](../../crates/engine-core/src/world.rs); the host gathers them via [`man_field_scripts::collect_partition_narration`](../../crates/engine-core/src/man_field_scripts.rs) over partition 2). The presenter [`CutsceneNarration`](../../crates/engine-core/src/cutscene_narration.rs) walks them one page at a time: `World::tick` advances a per-page timer (auto-advancing the subtitle, default `DEFAULT_PAGE_FRAMES` ≈ 2.5 s/page), and a confirm press skips to the next page. The host renders the active page centered near the bottom of the screen ([`cutscene_narration_draws_for`](../../crates/engine-render/src/lib.rs)).

The narration **gates the Rim Elm hand-off**: [`World::take_prologue_handoff`](../../crates/engine-core/src/world.rs) returns nothing while the narration is on screen, so the opening order matches retail - narration plays, *then* a confirm press triggers the `town01` transition. The presenter's per-page dwell (`DEFAULT_PAGE_FRAMES = 120` ≈ 2.0 s) and the renderer's ¾-down placement are pinned to retail (below). The disc-gated test `crates/engine-core/tests/opdeene_narration_playback.rs` cold-boots `opdeene`, asserts the narration installs (22 pages) and gates the hand-off, ticks it to completion on the timer, and confirms the hand-off then releases to `town01`.

### Timeline execution model (Ghidra-traced)

The cutscene timeline runs on the **same field/event VM** (`FUN_801DE840`) as every other field script - there is no dedicated cutscene executor. The pieces:

- **Record header.** Partition-2 records are **named records**, *not* the partition-1 `[u8 N][N*2 locals][4-byte header]` shape. Layout: `[u8 name_len][name_len*2 SJIS name][u8 C0][C0 bytes][u8 C1][C1*u16][u8 C2][C2*u16]<script>`. The name length is in characters; the three condition-list gates are story-flag predicates the dispatcher tests before running the record (block 1 = OR gate, block 2 = AND gate; block 0 is skipped here). The script entry offset is `1 + name_len*2 + (1+C0) + (1+C1*2) + (1+C2*2)`. For `opdeene`'s record 18 (`name_len=6` "Opening", all blocks empty) that is `0x10` - the `0x34` EFFECT op (white fade-in) that opens the prologue, immediately followed by `GFLAG_SET 26` at `+0x17`. Decoder:
  [`man_field_scripts::partition_record_span`](../../crates/engine-core/src/man_field_scripts.rs) (`FUN_8003BDE0`).
- **Dispatch.** `FUN_8003BDE0` resolves a partition record by index, walks the header, and **spawns a VM context** (`ctx[+0x90]` = record base, `ctx[+0x9e]` = entry PC, `ctx[+0x10] |= 0x100` "run me"); the per-frame runner `FUN_80039B7C` then loops `FUN_801DE840` on it until a yield. The index is keyed by scene-entry / tile-trigger position (`FUN_801D27E0` / `FUN_801D1EC4`), not a sequential partition walk.
- **Cross-context target `0xF8`.** Nearly every op in the timeline carries the extended-target byte `0xF8` (`A3 F8 …` = MoveTo, `CC F8 …` = MenuCtrl). `FUN_8003C83C(0xF8)` resolves to `_DAT_8007C364` - the **player / camera-anchor actor** - so the timeline drives the camera/lead actor.
- **Narration op.** `CC F8 80 N` (op `0x4C`, outer-nibble 8, sub-0) **spawns a child text context** from the on-screen-text pool with the `N` inline pages as its bytecode (parent PC advances by 3; the pages stay embedded). Each page is drawn by `FUN_8003C764`: horizontally centered (`X = (320 − text_width)/2`), fixed `Y = 180` on the 240-px virtual screen, with the per-page display timer seeded to `0x78` = 120 frames.
- **Camera Configure op `0x45`.** The CONFIGURE sub-path (`op0 & 0xC0 == 0`) reads a big-endian 10-bit field mask `(op0<<8)|op1`; bit `(9−i)` selects param `i`, each a signed-16 LE word written into the camera staging struct at `0x801C6EA8 + 0x02 + i*4`, followed by the commit `FUN_801DE084(struct, apply_trigger)`. The commit (`overlay_cutscene_dialogue_801de084.txt`) maps every param to a camera global:

  | param | struct off | global | role |
  |---|---|---|---|
  | 0 | `+0x02` | `_DAT_8007b790` | **pitch** (GTE `RotMatrixX` angle) |
  | 1 | `+0x06` | `_DAT_8007b792` | **yaw** (GTE `RotMatrixY` angle / heading) |
  | 2 | `+0x0a` | `_DAT_8007b794` | **roll** (GTE `RotMatrixZ` angle; zeroed in the field-camera build path) |
  | 3 / 4 / 5 | `+0x0e/12/16` | `_DAT_800840b8/bc/c0` | shake / offset trio (battle screen-shake reuses these) |
  | 6 / 7 / 8 | `+0x1a/1e/22` | `_DAT_80089118/1c/20` | **camera focus** = the GTE translation `(-X, +Y, -Z)` |
  | 9 | `+0x26` | `_DAT_8007b6f4` | **GTE H** projection register (focal length / zoom) via `func_0x8003d254` = `setCopControlWord(2, …)` |

  The focus trio is the high-confidence pin: three independent consumers store the *negated* world focus there - the follow-cam `FUN_801DBE9C` sets `_DAT_80089118 = -(anchor+0x14)` (−X) / `_DAT_80089120 = -(anchor+0x18)` (−Z); the culling test `FUN_80021DF4` reads `-_DAT_80089118` as the world focus X; the smooth-scroll in `overlay_0896_801ca998` targets `tile*-0x80 - _DAT_80089118`. So the world focus is `(-param6, param7, -param8)` (Y is stored un-negated, per the camera-param builder `FUN_801DAB90`).

  **The view rotation is three Euler angles; "eye distance" is the only missing scalar (retail has none).** The GTE camera-rotation build is pinned: `FUN_8001CF50` composes the view rotation matrix by rotating about each axis with the three camera-angle globals - `RotMatrixX(pitch=_DAT_8007B790)` at `0x800461A4`, `RotMatrixY(yaw=_DAT_8007B792)` at `0x8004629C`, `RotMatrixZ(roll=_DAT_8007B794)` at `0x8004638C` (each masks the angle to 12 bits and indexes the shared sin/cos LUT at `0x80070A2C`, `4096 = 360°`, `+0x800` = the quarter-wave cosine offset; composed via GTE `mvmva`). Each rotation is gated per-object by a flag at `obj+0x52` so a draw can opt out and inherit the globals. **So param 0 is the camera PITCH, not a "rot/zoom" word** - the zoom is H (a separate projection register).
  The per-frame *interpolation* is also pinned: `FUN_801DB510` eases the focus globals, the shake/offset trio, and the typed `0x801F2798` param table toward their control-block targets every frame with an exponential right-shift lerp (`srav` by `_DAT_8007B60B>>4`), so Camera Configure beats blend rather than snap. The camera *position* is implicit: retail places the eye at the GTE translation (the focus) and rotates the world by `RotX·RotY·RotZ`, projecting through H - there is **no explicit eye-distance scalar**. Confirmed against the `new_game_cutscene_intro_a` save state: focus `(8640, 0, 10304)` (mode byte `0x10` = anchor-follow), pitch `180` (≈15.8°), yaw `-2967`, roll `0`, H `792`. Negative finding (don't re-walk):
  reading the GTE rotation matrix + translation straight from a save-state frame does **not** recover the camera - the matrix is the last-rendered object's composed transform (row norms ≈ 6.0, a per-object scale), not a unit camera-view rotation; recover the parametrization from the angle globals instead.

### Timeline execution (engine port)

The engine **executes** this timeline as a spawned field-VM context. On entering `opdeene` live, [`World::load_cutscene_timeline_from_man`](../../crates/engine-core/src/world.rs) locates the partition-2 record that issues `GFLAG_SET 26` (via [`man_field_scripts::walk_partition_gflag_sites`](../../crates/engine-core/src/man_field_scripts.rs)), resolves its named-record span, and installs a [`CutsceneTimeline`](../../crates/engine-core/src/cutscene_timeline.rs) - a second `FieldCtx` separate from the scene-entry system script on `World::field_ctx`, seeded on the system channel (`script_id = 0xFB`) so cross-context (`0x80`-bit) ops keep running after the record's first yield sets the context halt bit.

[`World::step_cutscene_timeline`](../../crates/engine-core/src/world.rs) runs that context through the same `legaia_engine_vm::field::step` each frame, run-until-yield (mirroring retail's per-frame dispatch), bounded by a per-frame step budget and a frame cap. The Camera Configure (`0x45`) and `MoveTo` (`0x23`) ops emit the same [`FieldEvent`](../../crates/engine-core/src/field_events.rs)s the runtime [`Camera`](../../crates/engine-core/src/camera.rs) folds in, and the closing `GFLAG_SET 26` writes the hand-off bit through the same host path the main field VM uses - so the `town01` hand-off **fires by execution**, not by a static MAN-walk derivation.
The static arm ([`World::arm_prologue_handoff_from_man`](../../crates/engine-core/src/world.rs)) remains as a fallback for a scene whose timeline record can't be resolved, and a safety net arms it if execution can't reach the closing op within the frame cap, so the prologue can never stall.

Two single-shared-VM accommodations, **approximate by design**:

- **Narration pages are neutralized.** In retail's cutscene context the `CC F8 80 N` op routes to `FUN_8003C764` (text draw) and consumes its `N` inline pages; the engine's single field VM decodes `0x4C` n8 sub-0 as the actor allocator, whose PC-advance would land on the page bytes. Because the engine presents the narration through the separate `CutsceneNarration` presenter, the loader overwrites each narration span (located by [`cutscene_text::NarrationBlock::byte_span`](../../crates/asset/src/cutscene_text.rs)) with field-VM NOPs (`0x21`) - an offset-preserving fill, so relative jumps still resolve and the camera/move/`GFLAG` ops at their original offsets still execute. The actor-allocator host hook is also suppressed while the timeline steps (`World::in_cutscene_timeline`).
- **Camera params.** The op-`0x45` events flow to the `Camera` controller and the host writes the param set to `World::camera_state`. The native `play-window` renders the cutscene with [`window::cutscene_camera_mvp`](../../crates/engine-render/src/window.rs) whenever a cutscene timeline is installed, decoding the pinned params (see the op-`0x45` table above) via `SceneHost`'s `cutscene_view`: it frames the **focus** `(-param6, param7, -param8)` - sign-corrected back to world space from the negated GTE-translation globals - tilted by the **pitch** (param 0) and rotated by the **yaw** (param 1, PSX `4096` = full turn), with the **FOV** derived from param 9 (the GTE H register). The eye **distance** is the one approximation left:
  retail has no explicit eye-distance param (the eye sits at the GTE translation and projects
  through H - see the op-`0x45` table above), so the engine frames a fixed world half-extent
  around the focus through the decoded FOV (a narrow retail H pulls the shot in close, a wide
  one backs off; the scene AABB only caps the distance - a scene-radius orbit mis-framed multi-
  area cutscene scenes like `opdeene`, whose vignette islands span the whole map), but the orbit
  *angles* (pitch + yaw) are now the decoded `RotMatrixX`/`RotMatrixY` angles rather than a
  fixed tilt. A beat that omits the pitch slot falls back to the prior fixed ~24° downward
  framing. The shot re-targets each time the timeline executes a new Camera Configure op; rather
  than cutting, `play-window` eases the rendered `(focus, pitch, yaw, FOV)` toward each new beat
  through [`window::CutsceneCameraInterp`](../../crates/engine-render/src/window.rs) (per-frame
  ease, angles along the shortest arc;
  reset to snap when the timeline first installs) - mirroring retail's own per-frame `FUN_801DB510` exponential ease - so beats blend the way retail's GTE camera does.

The same machinery drives the **`town01` opening** (a sibling partition-2 record, `P2[3]`). On the new-game prologue hand-off, [`World::take_prologue_handoff`](../../crates/engine-core/src/world.rs) sets `entering_town01_opening`, and the `town01` field entry installs that record via [`World::install_town01_opening_timeline`](../../crates/engine-core/src/world.rs). Two differences from the opdeene prologue:

- **It does not arm a scene hand-off.** opdeene's timeline carries `arms_prologue_handoff` (its terminal `GFLAG_SET 26` / the frame-cap safety net arms the `town01` change); `town01`'s opening sets it `false` - `town01` is the destination, so its completion just drops the timeline (reverting the cutscene camera to normal field gameplay).
- **It opens name entry at op `0x49`.** `step_cutscene_timeline` steps past the conditional-wait parks the engine doesn't model - `0x4C` nibble-C `script_alloc` / globals-gate and `0x2D` / `0x30` flag-tests, all handshakes a spawned sub-context would satisfy - by their encoded width, while keeping `0x4A` WAIT_FRAMES (a timed wait that plays out over frames) and `0x49` STATE_RESUME parking. So the establishing camera + Vahn's walk-out beats play over ~490 frames, then the pinned op `0x49` at body `0x02c6` opens the *"Select your name."* overlay through the op-49 host hooks (`op49_invoke_setup` → [`World::open_name_entry`](../../crates/engine-core/src/world.rs); `op49_state` reports Armed while the overlay is up, Done once a name commits).
  The timeline is frozen while name entry is open (the STATE_RESUME suspend) and resumes when the player names the lead. See [`boot.md`](boot.md#name-entry-overlay).

Disc-gated coverage: `crates/engine-core/tests/opdeene_timeline_execution.rs` cold-boots `opdeene`, asserts the timeline installs with the hand-off bit clear, ticks until it sets the bit by execution, and reports the frame it armed; `crates/engine-core/tests/town01_opening_name_entry_wiring.rs` drives the `town01` opening end to end (install → camera/wait beats → name entry opens at op `0x49` → freeze → commit → resume → drop); `crates/engine-core/tests/town01_opening_timeline_trace.rs` pins the op-`0x49` site. The CI synthetic `crates/engine-core/tests/cutscene_timeline_synthetic.rs` exercises both paths (GFLAG-by-execution + safety net + idempotent completion for the hand-off timeline; op-`0x49` name-entry open / freeze / resume for the opening timeline) without disc data.

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

**Approximate by design:** the channel-completion handshake (`CFLAG_TST` / the halt-acquire
state-resume protocol) is not fully modelled - the simplified channels don't always raise the
exact sync flag the timeline waits on, so `step_cutscene_timeline` steps past a cross-context
flag-test wait (`0x2D`/`0x30`/`0x33`, all 2-byte / 3-byte extended, correct-width for a fixed
step-past) rather than parking on it, keeping the timeline flowing through all its camera beats.
Real timed `0x4A` WAIT_FRAMES and the `0x49` STATE_RESUME name-entry suspend are still honoured.

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
