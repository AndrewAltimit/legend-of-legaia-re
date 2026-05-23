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

- **`_DAT_8007BA78`** - FMV index. Read by the str_fmv overlay to select a 64-byte runtime FMV-state struct from the table at `0x801D0A6C`. On retail USA the table has 12 slots (`fmv_id ∈ 0..=11`); the field VM has been observed writing values up to `fmv_id = 8` via the per-STR FMV trigger corpus. The runtime mapping (`fmv_id → STR file`) is **not** a 1:1 walk over `MV1.STR..MV6.STR` - it skips `MV2.STR` and `MV5.STR` (disc-resident but unreferenced) and slots 5..=11 point at cut paths. See [`str-fmv-table.md`](../formats/str-fmv-table.md#runtime-fmv-state-table-0x801d0a6c-12--64-b) for the authoritative mapping.
- **`_DAT_8007B83C`** - next-game-mode global. Setting it to `0x1A` (decimal 26) kicks the main mode dispatcher (`FUN_80017714`) into `StrInit` on the next frame, which loads the str_fmv overlay and reads `_DAT_8007BA78` to pick the file.

The field-VM port handles this op as `op4c_n_e_sub2_fmv_trigger(fmv_id: i16)` in [`legaia_engine_vm::field`](../../crates/engine-vm/src/field.rs) and the world's [`FieldHostImpl`](../../crates/engine-core/src/world.rs) records the request as `World::pending_fmv_trigger` plus a `FieldEvent::FmvTrigger { fmv_id }`.

The world drives the Field → Cutscene → Field flow itself, mirroring the retail next-game-mode dispatch: the **next** `World::tick` consumes `pending_fmv_trigger` at the top of the frame (one frame after the op fires, exactly as `FUN_80017714` reads the next-game-mode global a frame late), and if the id resolves to a playable slot (`cutscene::fmv_index_to_str_filename` is `Some`) it flips `World::mode` into `SceneMode::Cutscene` and records the active FMV (`World::active_fmv()`). While the FMV plays the world **suspends the field VM** (the STR overlay owns the frame in retail); the host polls `World::active_fmv_str_filename()`, plays the resolved `MV*.STR`, and calls `World::finish_cutscene()` when playback ends, which returns to the field with the field-VM program counter already past the op. A `fmv_id` whose runtime slot points at a cut/missing path is drained as a no-op (no mode flip), matching the engine's "treat a cut slot as a no-op" rule. The `legaia-engine play` loop runs this flow headlessly, decoding the resolved STR via MDEC to report its frame count. The windowed `play-window` host plays it **in the engine window**: when a tick flips the world into `SceneMode::Cutscene`, it resolves the `MV*.STR` under the extracted root, decodes the frames (shared `decode_str_frames` with `play-str`), suspends world ticks, and shows the video one frame per redraw; once the frames drain it calls `finish_cutscene()` and resumes the field. (Disc-only `play-window` runs can't read the ISO STR at that point, so the cutscene drains as a no-op there.)

The trailing 3 bytes of the instruction are reserved by the dispatcher's PC math (the handler's `addiu s8, s8, 6` is fixed, but only bytes `+1..+3` are read). Disassemblers should leave them as opaque padding.

### Static FMV-trigger sites — exhaustive

A backward sweep of every Ghidra dump in the corpus surfaces **three** writers of `_DAT_8007B83C = 0x1A` in retail. The first two are codified in [`legaia_engine_vm::cutscene_trigger`](../../crates/engine-vm/src/cutscene_trigger.rs) as `FMV_TRIGGER_SITES`; the third was pinned later via a PCSX-Redux watchpoint on the title-attract countdown.

| Site | Function | Mode-write addr | FMV-id source | Trigger condition |
|---|---|---|---|---|
| `field_vm_op_4c_e2` | `FUN_801DE840` | `0x801E3104` | `decode_u16_be(pc+1)` from field-VM bytecode | Field-VM bytecode hits `0x4C 0xE2 lo hi`; reached via JT chain `0x801CEE60` (high nibble 0xE) → `0x801CF008` (low nibble 0x2). |
| `title_attract_loop` (`FUN_801DE234` label) | `FUN_801DD35C` (label `FUN_801DE234`) | `0x801E0F50` | Hardcoded `0` (= `MV1.STR`, intro) | Title-screen idle countdown `DAT_801ef16c` underflows. |
| `title_tick_inline` | `FUN_801DD35C` | `0x801DDCF0` | Inline: `sh zero, -0x4588(v0)` zeroes `_DAT_8007BA78` at `0x801DDCE8` immediately before (= `MV1.STR`). | Inline fall-through past the decrement instruction at `0x801DDCCC` (`bgez v0, 0x801DFC3C` not taken). PC-verified via the live capture in [`subsystems/boot.md` § Tick function](boot.md#tick-function). |

Both title-side sites live in the same outer function `FUN_801DD35C` (the per-frame title-overlay tick); `FUN_801DE234` is a Ghidra-promoted label inside its body. The `0x801DDCF0` site is the one the watchpoint pins in practice — every per-frame decrement passes through `0x801DDCCC` and the underflow path immediately writes the mode-byte before any sub-call.

**`FUN_801E30E4` has zero static callers.** It is a label inside `FUN_801DE840`, not a callable subroutine — Ghidra promotes it to a `FUN_` symbol because the JT at `0x801CF008[2]` resolves to that address. The actual control flow is `outer 0x4C dispatcher → 0x801E0C3C → 0x801E3040 → jump-table indirect to 0x801E30E4`.

### Why the seven mid-game scenes don't surface in a bytewise PROT scan

`town0b`, `map01`, `chitei2`, `map02`, `jou`, `uru2`, `town0e` all kick off mid-game STR FMVs. Since `field_vm_op_4c_e2` is the only field-VM-driven trigger and the title-screen attract path is hardcoded to `fmv_id = 0`, those seven scenes **must** trigger via the same `0x4C 0xE2` op. The byte sequence is not statically present in their on-disc PROT entries, however — the disassembler's `--bytewise` mode (which scans every byte of every PROT entry) surfaces only one in-range trigger across the corpus (`PROT[371] taiku`, `fmv_id=5`).

The conclusion is that the seven scenes' field-VM bytecode is **reconstructed at scene-load time** from the field-pack preamble's runtime-projected slot. The lift is therefore blocked on the same intra-transition byte-level capture that gates the [field-pack runtime projection](../formats/field-pack.md) — once the loader's preamble → runtime-RAM-cell projection is mapped, the `0x4C 0xE2` op-byte sequence becomes scannable in RAM and the per-scene MV index falls out of a single-frame disassembly.

### Per-STR FMV trigger corpus

The current corpus carries nine save states captured RIGHT before each FMV begins playing, one per `_DAT_8007BA78` value (`fmv_id ∈ 0..=8`). They pin the trigger-side state across the full retail range:

- `_DAT_8007BA78 = expected_fmv_id` (s16 LE) for each of nine saves
- `_DAT_8007B83C = 0x1A` (StrInit) for every save
- `_DAT_8007BAC8 = 2000` (BGM ID) for every save
- Active scene = `map01` for every save (one of the seven mid-game FMV-trigger field scenes)
- `recover_base()` = `0x80139530` (`map01`'s field-pack base) for every save

The `0x4C 0xE2 lo hi` byte sequence does NOT appear in the field-pack RAM region for any save — the corpus was generated by **debug-menu-driven** trigger paths, NOT by stepping the field VM through a per-scene FMV trigger op. So the corpus pins the `(fmv_id, game_mode)` tuple across the full `0..=8` range but does not disambiguate which fmv_id each of the seven mid-game scenes' field-VM bytecode writes at runtime — that gap is still gated on intra-transition field-pack projection capture.

The corpus is codified at `legaia_engine_core::capture_observations::cutscene_trigger_corpus` and exercised by the disc-gated test `crates/mednafen/tests/real_saves.rs::cutscene_trigger_corpus_pins_fmv_id_across_nine_saves`.

## In-engine 3D cutscene (`opdeene` opening prologue)

Not every cutscene is an STR FMV. The New Game opening — the "Genesis tree" prologue with the *"…the Seru."* narration — is an **in-engine 3D cutscene scene** (`opdeene`, CDNAME/PROT #748), a field scene running in master game-mode `0x03` (field RUN), not a `MOV/MVn.STR` video. (`MV1.STR` is the title-attract movie; the opening 3D sequence is engine-rendered — see [`boot.md`](boot.md#opdeene--town01-handoff-scene-change-packet).)

The cutscene plays out of the scene MAN's **cutscene-timeline partition** (partition 2). Its closing record (record 18; record start at MAN offset `0xA47`) is a field-VM script that interleaves:

- camera staging — op `0x45` `Camera Configure` (a 23-byte payload block) and op `0x46` `RenderCfg`;
- actors — op `0x23` `MoveTo` and op `0x34` `Effect` spawns;
- the `town01` hand-off arm — op `0x2E` `GFLAG_SET 26` (`2E 1A` at `0xA5E`);
- **inline narration text** (below).

### Inline narration format

The on-screen narration is carried as **inline ASCII text pages embedded in the timeline script**, not as a `MES` text id. A narration **block** is introduced by a field-VM op `0x4C` in its outer-nibble-8 form with the cross-context extended target `0xF8`:

```text
0xCC 0xF8 0x80 N        ; op (0xCC = 0x80|0x4C extended), N = page count
1F <ascii…> 00          ; page 1
1F <ascii…> 00          ; page 2
…                       ; N pages total
```

Each page is framed `0x1F <printable ASCII> 0x00` — `0x1F` (ASCII Unit Separator) starts a page, `0x00` terminates it, the body is plain 7-bit ASCII. The page count `N` in the introducing op equals the number of `0x1F`-framed pages that follow, which both validates the parse and gives a consumer the cadence for revealing subtitles.

`opdeene`'s timeline carries two blocks: a 14-page creation prologue and an 8-page Seru-history block (22 pages total). The clean-room parser is [`legaia_asset::cutscene_text`](../../crates/asset/src/cutscene_text.rs) (`parse_narration` / `narration_pages`); it locates the introducing op and the page framing structurally and decodes the runtime disc bytes (no narration text is baked into the repo). Inspect it with:

```bash
legaia-engine man-scripts --scene opdeene --disc "<disc>.bin" \
  --narration --disasm-partition 2
```

The disc-gated test `crates/engine-core/tests/opdeene_narration.rs` ground-truths the structure (two blocks, 14 + 8 pages, every page non-empty ASCII, declared count matches decoded) without committing the text.

### Narration playback

Entering `opdeene` live installs the decoded pages on the world ([`World::open_cutscene_narration`](../../crates/engine-core/src/world.rs); the host gathers them via [`man_field_scripts::collect_partition_narration`](../../crates/engine-core/src/man_field_scripts.rs) over partition 2). The presenter [`CutsceneNarration`](../../crates/engine-core/src/cutscene_narration.rs) walks them one page at a time: `World::tick` advances a per-page timer (auto-advancing the subtitle, default `DEFAULT_PAGE_FRAMES` ≈ 2.5 s/page), and a confirm press skips to the next page. The host renders the active page centered near the bottom of the screen ([`cutscene_narration_draws_for`](../../crates/engine-render/src/lib.rs)).

The narration **gates the Rim Elm hand-off**: [`World::take_prologue_handoff`](../../crates/engine-core/src/world.rs) returns nothing while the narration is on screen, so the opening order matches retail — narration plays, *then* a confirm press triggers the `town01` transition. The presenter's per-page dwell (`DEFAULT_PAGE_FRAMES = 120` ≈ 2.0 s) and the renderer's ¾-down placement are pinned to retail (below). The disc-gated test `crates/engine-core/tests/opdeene_narration_playback.rs` cold-boots `opdeene`, asserts the narration installs (22 pages) and gates the hand-off, ticks it to completion on the timer, and confirms the hand-off then releases to `town01`.

### Timeline execution model (Ghidra-traced)

The cutscene timeline runs on the **same field/event VM** (`FUN_801DE840`) as every other field script — there is no dedicated cutscene executor. The pieces:

- **Record header.** Partition-2 records are **named records**, *not* the partition-1 `[u8 N][N*2 locals][4-byte header]` shape. Layout: `[u8 name_len][name_len*2 SJIS name][u8 C0][C0 bytes][u8 C1][C1*u16][u8 C2][C2*u16]<script>`. The name length is in characters; the three condition-list gates are story-flag predicates the dispatcher tests before running the record (block 1 = OR gate, block 2 = AND gate; block 0 is skipped here). The script entry offset is `1 + name_len*2 + (1+C0) + (1+C1*2) + (1+C2*2)`. For `opdeene`'s record 18 (`name_len=6` "Opening", all blocks empty) that is `0x10` — the `0x34` EFFECT op (white fade-in) that opens the prologue, immediately followed by `GFLAG_SET 26` at `+0x17`. Decoder: [`man_field_scripts::partition_record_span`](../../crates/engine-core/src/man_field_scripts.rs) (`FUN_8003BDE0`).
- **Dispatch.** `FUN_8003BDE0` resolves a partition record by index, walks the header, and **spawns a VM context** (`ctx[+0x90]` = record base, `ctx[+0x9e]` = entry PC, `ctx[+0x10] |= 0x100` "run me"); the per-frame runner `FUN_80039B7C` then loops `FUN_801DE840` on it until a yield. The index is keyed by scene-entry / tile-trigger position (`FUN_801D27E0` / `FUN_801D1EC4`), not a sequential partition walk.
- **Cross-context target `0xF8`.** Nearly every op in the timeline carries the extended-target byte `0xF8` (`A3 F8 …` = MoveTo, `CC F8 …` = MenuCtrl). `FUN_8003C83C(0xF8)` resolves to `_DAT_8007C364` — the **player / camera-anchor actor** — so the timeline drives the camera/lead actor.
- **Narration op.** `CC F8 80 N` (op `0x4C`, outer-nibble 8, sub-0) **spawns a child text context** from the on-screen-text pool with the `N` inline pages as its bytecode (parent PC advances by 3; the pages stay embedded). Each page is drawn by `FUN_8003C764`: horizontally centered (`X = (320 − text_width)/2`), fixed `Y = 180` on the 240-px virtual screen, with the per-page display timer seeded to `0x78` = 120 frames.
- **Camera Configure op `0x45`.** The CONFIGURE sub-path (`op0 & 0xC0 == 0`) reads a big-endian 10-bit field mask `(op0<<8)|op1`; bit `(9−i)` selects param `i`, each a signed-16 LE word written into the camera staging struct at `0x801C6EA8 + 0x02 + i*4`, followed by a commit (`FUN_801DE084(struct, apply_trigger, mode=(op0>>2)&0xF)`). Param 6 (`+0x1A`) is the camera-target X and param 8 (`+0x22`) the camera-target Z (pinned via the APPLY path, which swaps them through the documented `_DAT_80089118/80089120` scroll globals). The remaining params (eye / distance / pitch / roll / a `0x4000`-magnitude angle word) are not individually disambiguated from static data, and the snap-vs-interpolate behaviour of the commit (`FUN_801DE084`) needs a clean field-overlay re-dump.

### Timeline execution (engine port)

The engine **executes** this timeline as a spawned field-VM context. On entering `opdeene` live, [`World::load_cutscene_timeline_from_man`](../../crates/engine-core/src/world.rs) locates the partition-2 record that issues `GFLAG_SET 26` (via [`man_field_scripts::walk_partition_gflag_sites`](../../crates/engine-core/src/man_field_scripts.rs)), resolves its named-record span, and installs a [`CutsceneTimeline`](../../crates/engine-core/src/cutscene_timeline.rs) — a second `FieldCtx` separate from the scene-entry system script on `World::field_ctx`, seeded on the system channel (`script_id = 0xFB`) so cross-context (`0x80`-bit) ops keep running after the record's first yield sets the context halt bit.

[`World::step_cutscene_timeline`](../../crates/engine-core/src/world.rs) runs that context through the same `legaia_engine_vm::field::step` each frame, run-until-yield (mirroring retail's per-frame dispatch), bounded by a per-frame step budget and a frame cap. The Camera Configure (`0x45`) and `MoveTo` (`0x23`) ops emit the same [`FieldEvent`](../../crates/engine-core/src/field_events.rs)s the runtime [`Camera`](../../crates/engine-core/src/camera.rs) folds in, and the closing `GFLAG_SET 26` writes the hand-off bit through the same host path the main field VM uses — so the `town01` hand-off **fires by execution**, not by a static MAN-walk derivation. The static arm ([`World::arm_prologue_handoff_from_man`](../../crates/engine-core/src/world.rs)) remains as a fallback for a scene whose timeline record can't be resolved, and a safety net arms it if execution can't reach the closing op within the frame cap, so the prologue can never stall.

Two single-shared-VM accommodations, **approximate by design**:

- **Narration pages are neutralized.** In retail's cutscene context the `CC F8 80 N` op routes to `FUN_8003C764` (text draw) and consumes its `N` inline pages; the engine's single field VM decodes `0x4C` n8 sub-0 as the actor allocator, whose PC-advance would land on the page bytes. Because the engine presents the narration through the separate `CutsceneNarration` presenter, the loader overwrites each narration span (located by [`cutscene_text::NarrationBlock::byte_span`](../../crates/asset/src/cutscene_text.rs)) with field-VM NOPs (`0x21`) — an offset-preserving fill, so relative jumps still resolve and the camera/move/`GFLAG` ops at their original offsets still execute. The actor-allocator host hook is also suppressed while the timeline steps (`World::in_cutscene_timeline`).
- **Camera params.** The op-`0x45` events flow to the `Camera` controller and the host writes the param set to `World::camera_state`. The native `play-window` renders the cutscene with [`window::cutscene_camera_mvp`](../../crates/engine-render/src/window.rs) whenever a cutscene timeline is installed: a static cinematic shot framing the camera target the timeline staged (params 6 / 8 = target X / Z, read from `World::camera_state`), with the Y taken from the lead actor. Because only the X/Z target params are pinned — eye / distance / pitch and the snap-vs-interpolate commit (`FUN_801DE084`) are not yet disambiguated — the eye **vantage** is a fixed approximation (a three-quarter framing sized to the scene AABB) until a clean field-overlay re-dump (and, ideally, a mid-cutscene save capture) lands. The shot re-targets each time the timeline executes a new Camera Configure op, so it tracks the cutscene's beats even though the path between them is not interpolated.

Disc-gated coverage: `crates/engine-core/tests/opdeene_timeline_execution.rs` cold-boots `opdeene`, asserts the timeline installs with the hand-off bit clear, ticks until it sets the bit by execution, and reports the frame it armed. The CI synthetic `crates/engine-core/tests/cutscene_timeline_synthetic.rs` exercises the executor (GFLAG-by-execution, safety net, idempotent completion) without disc data.

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

## See also

**Reference** —
[STR FMV table](../formats/str-fmv-table.md) ·
[XA audio](../formats/xa.md) ·
[Audio stack](audio.md) ·
[Field/event VM](script-vm.md)
