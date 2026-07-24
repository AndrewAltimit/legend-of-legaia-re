# Cutscene

Pre-rendered cutscene playback combines PSX STR video (MDEC hardware decoder)
with the XA-ADPCM audio interleaved in the same CD-XA sectors. The engine drives
it through game modes 26 and 27 (`StrInit` / `StrMode`), which map to
`SceneMode::Cutscene` in the clean-room port.

**Where it lives.** The playback engine is split: an STR overlay (master dispatch
`FUN_801CEA3C`, play loop `FUN_801CF098`) over the SCUS-resident `St` streaming
library. The FMV dispatch table is at `0x801D0A6C`.

**Port counterpart.** `crates/mdec` (the clean-room decoder) plus the
`legaia-engine play-str` loop; `legaia_asset::fmv_dispatch` reads the table.

**The two things that catch people out:**

- **Legaia movies are not STRv2.** They are the **Iki** bitstream - an
  LZSS-compressed per-block qscale/DC table plus an AC-only entropy stream. A
  standard STRv2 decoder will not decode them. See
  [MDEC decoder (Iki bitstream)](#mdec-decoder-iki-bitstream).
- **"The opening cutscene" is mostly not a movie.** The five-scene New-Game
  opening is rendered **in-engine in 3D**, not played back as FMV; only some legs
  are STR. See
  [In-engine 3D opening](#in-engine-3d-opening-the-five-scene-new-game-chain).

## Contents

- [Game modes](#game-modes) · [STR sector format](#str-sector-format)
- [Retail playback engine](#retail-playback-engine-str-overlay--scus-st-streaming-library) - [master dispatch](#master-dispatch---fun_801cea3c-overlay) · [play loop](#play-loop---fun_801cf098-overlay) · [frame-demux SM](#frame-demux-state-machine-scus-st-library) · [ring layout](#ring-layout--slot-status) · [ring port](#engine-port---legaia_mdecst_ring) · [bitstream decode + MDEC feed](#bitstream-decode--mdec-feed-overlay) · [STRv2 VLC table](#strv2-vlc-lookup-table-fun_801f1a00) · [play-loop port](#engine-port---legaia_mdecstr_player)
- [XA channel selection](#xa-channel-selection) · [XA audio](#xa-audio) · [A/V sync](#interleaved-cutscene-audio-av-sync)
- [MDEC decoder (Iki bitstream)](#mdec-decoder-iki-bitstream) - [frame header](#1-frame-header-10-bytes) · [LZSS qscale/DC table](#2-lzss-qscaledc-table) · [AC bitstream](#3-ac-bitstream) · [dequantize + IDCT](#4-dequantize--idct) · [macroblock layout](#5-macroblock-layout) · [upsampling + colour](#6-420-upsampling--bt601-colour-conversion)
- [Playback loop (`play-str`)](#playback-loop-play-str) · [frame-rate detection](#frame-rate-detection) · [CLI reference](#cli-reference)
- [CDNAME → STR override map](#cdname--str-override-map) · [overlay residency](#strmdec-fmv-overlay-residency) · [directory-record cache](#directory-record-cache) · [post-FMV return scenes](#post-fmv-return-scenes)
- [Field-VM FMV-trigger op](#field-vm-fmv-trigger-op) - [static trigger sites](#static-fmv-trigger-sites---exhaustive) · [trigger assignment is disc-sourced](#the-per-scene-trigger-assignment-is-disc-sourced-the-runtime-reconstructed-reading-is-falsified) · [per-STR trigger corpus](#per-str-fmv-trigger-corpus)
- [In-engine 3D opening](#in-engine-3d-opening-the-five-scene-new-game-chain) - [the five-scene chain](#the-five-scene-chain) · [record spawn](#record-spawn-mechanisms-live-probe-pinned) · [`opdeene` timeline record](#the-opdeene-timeline-record) · [inline narration](#inline-narration-format) · [crawl roller](#narration-playback---the-crawl-roller-fun_80037174) · [timeline execution](#timeline-execution-model-ghidra-traced) · [engine port](#timeline-execution-engine-port) · [vignette actors](#per-actor-channels---the-vignette-actors) · [screen fade](#scripted-screen-fade-op-0x4c-0x12--the-effect-colour-op-0x34-sub-0) · [sepia grade](#full-scene-sepia-grade-the-gold-prologue-look)
- [Field-to-battle transition](#field-to-battle-transition-the-battle-intro-overlay) - [tick + battle handoff](#transition-tick--battle-handoff---fun_801cf5bc) · [per-style emitters](#per-style-emitters-render-track-gtegpu)
- [Script-cutscene helpers](#script-cutscene-helpers-overlay_cutscene_dialogue)
- [Open items](#open-items) · [Provenance](#provenance)

## Game modes

| Index | Name | param | next |
|---|---|---|---|
| 26 | `STR` (`StrInit`) | `0x80A` | - |
| 27 | `STR MODE` (`StrMode`) | `0x000` | `ConfigInit` |

`StrInit` (index 26) bootstraps the cutscene: opens the STR stream, initialises the MDEC hardware
(or the clean-room decoder), starts the XA audio. `StrMode` (index 27) runs the per-frame loop:
reads the next batch of sectors, decodes a frame, blits it full-screen, and advances the audio
position. When the stream ends, the mode chain transitions to `ConfigInit` (index 1).

Both modes share `SceneMode::Cutscene` and see the same world state. The retail STR/MDEC FMV
handler lives in a dedicated overlay (game modes 26/27). Its **source is pinned: PROT 0970**
(`cutscene_str`, slot-A base `0x801CE818`), identified statically from the disc by its leading
`MV*.STR` movie paths + the MDEC decoder strings (`MDEC_in_sync` / `MDEC_out_sync` /
`MDEC_rest:bad option`); see [`static-overlay-pipeline.md`](../tooling/static-overlay-pipeline.md).
The overlay is Ghidra-importable straight from the disc (`asset overlay ghidra`), and the
master dispatch + play loop + frame-demux state machine are statically decompiled - see
"[Retail playback engine](#retail-playback-engine-str-overlay--scus-st-streaming-library)"
below; the per-scene
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

## Retail playback engine (STR overlay + SCUS St streaming library)

Statically decompiled from PROT 0970 at base `0x801CE818` plus the SCUS-resident PsyQ-libpress-shape
"St" streaming library it calls. Byte-identical against the FMV-resident RAM capture
(`overlay_str_fmv.bin`), so the capture-era dumps and the disc-sourced program agree address-for-address.

### Master dispatch - `FUN_801CEA3C` (overlay)

The mode-26/27 entry. Pre-play, per `_DAT_8007BA78` (the `fmv_id`):

- selects the bitstream decoder: `DAT_801E09FC = 1` (the **Iki** decoder) by default; dev slots
  9/10 (`MV1A.STR` / `MOV15.STR`) clear it to select the STRv2/v3 VLC-table decoder;
- clears the letterbox bands via `ClearImage` rects (fmv 3 opens on a white flash instead);
- calls the play loop on the dispatch slot: `FUN_801CF098(wide_flag, 0x801D0A6C + fmv_id * 0x20)`
  (**32-byte stride** - `sll v0,v0,0x5` at `0x801CEC9C`; an earlier `* 0x40` reading paired wrong
  slot halves, see [`str-fmv-table.md`](../formats/str-fmv-table.md#fmv-dispatch-table-0x801d0a6c-23--32-b)).

Post-play it hands control back per `fmv_id`: mid-game slots 1..4 and 6..8 copy a **return-scene
label** from the table at `0x801CE8AC` (`town0b` / `map01` / `chitei2` / `map02` / `jou` / `uru2` /
`town0e`) into the next-scene name global `0x80084548`, write a spawn/door word to `0x80084540`,
and set game mode 2; slot 0 (the intro) and dev slot 9 hand off to mode `0x16` (22 = CARD init,
`_DAT_8007BB00` = 2 / 1); slot 5 sets mode 2 with `_DAT_8007B8B8 = 2` and no scene-name write.
Full table in [`str-fmv-table.md`](../formats/str-fmv-table.md#authoritative-runtime-mapping).
`see ghidra/scripts/funcs/overlay_cutscene_str_0970_801cea3c.txt` (`0x801CECA0` is a body address
inside this function, not a sibling entry point).

Both `switch`es are ported in `legaia_engine_core::cutscene`: `fmv_post_play_handoff` returns the
control transfer as an `FmvHandoff` (field scene + spawn word / resume-in-place / card init /
mode 0), and `fmv_bitstream`, `fmv_is_skippable` and `fmv_clear_rects` carry the three pre-play
decisions the dispatch makes from the same `fmv_id`. Note the four pre-play `ClearImage` rects
bracket the tops and bottoms of **both** decode buffers rather than forming a letterbox - the
middle pair straddles the seam between the two frame rects and overlaps by four scanlines.

### Play loop - `FUN_801CF098` (overlay)

`(wide_flag, &dispatch_slot)`:

1. `CdSearchFile` (`FUN_8005DBB4`) resolves the slot's path string; the result populates the
   libcd `CdlFILE` directory cache at `0x801CAE08` (what an earlier reading mislabelled the
   "compact MV table" - see [`str-fmv-table.md`](../formats/str-fmv-table.md#directory-record-cache-0x801cae08-24-b-cdlfile-records)).
2. Ring + stream setup (below), then seek `(start_frame - 1) * 10` sectors past the file start
   (`CdIntToPos`/`CdPosToInt` + `CdControl(CdlSetloc)`) - the 15 fps cadence.
3. Opens the SPU CD input for the interleaved XA audio: `FUN_800643C4(0, 0x7F, 0x7F)`
   (`SpuSetCommonAttr` CD-volume L/R) + `FUN_80062A0C(0, 0, 1)` (CD-mix enable).
4. Per frame: poll a complete demuxed frame (`FUN_801CFA14`, up to 2000 spins), VLC-decode it into
   the MDEC-code double buffer, kick MDEC (below), build the display rect (24-bit slots draw at
   `width * 3/2` 16-bit pixels), `LoadImage`/swap. A stall re-seeks via the timeout handler
   `FUN_801CFB94` (re-`Setloc` + re-read, `time out in strNext` debug print).
5. Loop exit: the demuxer's end-frame latch (`DAT_801E09F8`, set when the sector header's frame
   number reaches the slot's `end_frame`) - or a pad press (`_DAT_8007B850 & 0x1F0`) **only when
   `fmv_id == 0`**: the intro/attract movie is skippable, mid-game FMVs are not.
6. Teardown: CD-mix mute (`FUN_800643C4(0,0,0)`), MDEC reset, `CdControl(CdlPause)`.

`see ghidra/scripts/funcs/str0970_801cf098.txt`.

### Frame-demux state machine (SCUS St library)

The overlay primes a **sector ring** in the streaming asset buffer (`_DAT_8007B85C + 0x10000`,
end `+0x38000`) and registers the demuxer:

- `FUN_801CF8B0` - ring/VRAM-rect init: stores the per-slot frame rects (double-buffered at
  `(fb_x, fb_y)` and `(fb_x, fb_y + height)`).
- `FUN_801CF988` - `StSetRing` (`FUN_8005BBF8(ring, 0x20)` - 32 sectors) + `StSetStream`
  (`FUN_8005EDC4(color_flag, start_frame, -1, 0, 0)`) + MDEC reset + `DecDCToutCallback`
  (`FUN_801CFEBC` -> `FUN_801CF56C`) + first seek/read.
- `FUN_801CFB94` - seek + read start: `CdControlF(CdlSetloc)`, `CdControlF(CdlSetmode, 0x80)`
  (double speed for the seek), then `FUN_8005EB68(0x1E0)`: Setmode **`0xE0`**
  (`CdlModeSpeed | CdlModeRT | CdlModeSize1` - XA-ADPCM realtime play ON, **sector filter OFF**),
  install the CD data-ready callback (`CdReadyCallback(FUN_8005ECD4)`), issue **`CdlReadS`**
  (`0x1B`). See "[XA channel selection](#xa-channel-selection)" for why no filter is set.

The demuxer proper is the data-ready pair `FUN_8005ECD4` / `FUN_8005F024` (SCUS): per delivered
sector it DMAs the 32-byte STR sector header out of the CD FIFO and walks the assembler state:

- **video gate**: magic `0x160` at `+0x00` and the type field's stream number
  (`(type >> 10) & 0x1F`) matching `_DAT_801CADB0` (0 for every retail movie); non-matching
  sectors (any leftover data) are dropped - XA audio sectors never reach this path at all, the
  drive's RT mode consumes them in hardware;
- **seek-to-start state** (`_DAT_801CADD0 = 1`, armed by `StSetStream`): skip sectors until
  `frame_number == start_frame`, so a mid-file segment starts exactly on its first frame;
- **sequence check**: `chunk_number` must equal the running counter `_DAT_801CAD94` within the
  current `frame_number` (`_DAT_801CAD90`); chunk 0 latches a new frame;
- **end check**: on chunk 0, `end_frame` reached -> rewind the partial frame, re-arm the seek, fire
  the optional end callback; a frame that no longer fits before the ring end leaves a **wrap
  marker** (status 1) in the slot and restarts the write cursor at slot 0;
- **ring-full**: the target slot is still held by the decoder -> the sector is dropped rather than
  overrunning it, and the slot is left untouched;
- payloads DMA into per-frame ring slots (2016 bytes per sector after the 32-byte per-slot status
  headers), status 2 = frame complete.

The overlay consumes frames via `StGetNext` (`FUN_8005EF40`: status 2 -> 4 "in use") and returns
slots with `StFreeRing` (`FUN_8005EE4C`).

#### The frame window comes from `StSetStream`

`FUN_8005EDC4` installs the window exactly as its PsyQ prototype implies. Its first act is

```
8005ede4  jal 0x8005f004
8005ede8  _li a0,0x1        <- the delay slot writes a0 and nothing else
```

`a1` and `a2` are never written before that call (only `a3` is saved into `s1`), so `StSetStream`'s
own `start_frame` / `end_frame` arguments fall straight through into the callee. Retail is
`FUN_8005F004(1, start_frame, end_frame)`, which stores them to `_DAT_801CADD0` (seek arm, forced
to 1), `_DAT_801CADAC` (`start_frame`) and `_DAT_801CADCC` (`end_frame`). `FUN_8005F004` has no
other caller in any dump - it is `StSetStream`'s helper, not a separate entry point.

Ghidra's decompiler prints that call as `FUN_8005f004(1)` because it infers a one-argument
signature for the callee. **The dropped arguments are a decompiler artefact.** Reading the C
instead of the disassembly here yields the false conclusion that the window arrives from somewhere
else, and a port built on it never seeks - so every mid-file segment starts on the file's first
frame.

What *is* true is that the St library's end-frame stop is unused in retail. The one call site,
`FUN_801CF988`, is `StSetStream(slot[+0x04], slot[+0x08], -1, 0, 0)`: mode and `start_frame` come
from the FMV dispatch slot, but `end_frame` is a literal `-1`. The segment end is enforced one
level up, by the play loop `FUN_801CF098` comparing the demuxed frame number against the slot's
`+0x0C` (`801cf384 lw v0,0xc(s3)` / `801cf38c slt v0,v0,s0`).

### Ring layout + slot status

`StSetRing(base, slots)` hands the library one flat buffer holding two parallel arrays: `slots`
32-byte slot headers at `base`, then `slots` 2016-byte payload areas at `base + slots * 32`. One
slot holds one sector - its STR sector header (the `u16` at `+0x00` overwritten in place by the
slot status once inspected) and its payload. A frame occupies `chunks_per_frame` **consecutive**
slots, which is what makes an assembled frame a contiguous run the decoder reads without copying,
and what forces the wrap handling when a frame doesn't fit before the ring end.

| Status | Meaning |
|---:|---|
| 0 | free |
| 1 | wrap marker - the reader restarts at slot 0 on landing here |
| 2 | frame complete, ready for `StGetNext` |
| 3 | filling (sectors of this frame still arriving) |
| 4 | handed to the decoder; released by `StFreeRing` |

### Engine port - `legaia_mdec::st_ring`

[`StRing`](../../crates/mdec/src/st_ring.rs) is the clean-room port of the ring and its
per-sector state machine, minus the CD/DMA register pokes: `set_ring` / `set_stream` / `set_mask`
/ `deliver_sector` / `get_next` / `free_ring`, with the demuxer's own trace codes surfaced as
`StStatus` (ring full, sequence break, end frame, wrap-stop, wrap-blocked, accepted). It is the
back-pressure-aware sibling of [`StrFrameAssembler`](../../crates/mdec/src/str_sector.rs), which
stays the right tool for offline extraction where no ring exists to overrun. The disc-gated
`st_ring_real_str` test streams a real `MV1.STR` through both and asserts they agree
frame-for-frame and byte-for-byte, and that the armed seek lands exactly on `MV3.STR`'s
`0xE2` segment boundary.

`set_stream(mode, start_frame, end_frame)` mirrors retail's argument list and installs the window
itself; `set_mask` stays exposed because the demuxer re-arms the same three globals on its
end-frame path. Only bit 0 of `mode` is kept (`_DAT_801CAD98`), readable as `mode_flag()` - retail
uses it for the sector-lost check and the DMA attribute word, both hardware-side, so the port only
records it.

`StRing` is driven by [`legaia_mdec::str_player::StrPlayer`](#engine-port---legaia_mdecstr_player)
and through it by `mdec decode-str`, which is what makes a *segment* of a movie playable off the
CLI. The engine's own hosts (`legaia_engine_core::cutscene`, `legaia-engine play-str`) still demux
through `StrFrameAssembler`, which stays the right tool where no back-pressure exists.

### Engine port - `legaia_mdec::str_player`

[`str_player`](../../crates/mdec/src/str_player.rs) is the layer between the ring and the
bitstream decoder - the retail play loop minus its CD, DMA and GPU register pokes:

| Retail | Port |
|---|---|
| `FUN_801CF098` play loop | `StrPlayer` + `seek_sector_offset` + `vram_units` + `display_rect` |
| `FUN_801CF8B0` decode-env init | `DecodeEnv::init` |
| `FUN_801CF988` ring + stream setup | `StrPlayer::open` |
| `FUN_801CFA14` frame pump | `StrPlayer::next_frame` |
| `FUN_801CFD84` MDEC output control word | `mdec_output_control` |
| `FUN_801CFEBC` slice-callback (un)install | `DecodeEnv::set_slice_callback` |
| `FUN_801CF56C` MDEC-out slice callback | `DecodeEnv::advance_slice` |
| `FUN_801CF740` frame poll | `end_of_stream` + `DecodeEnv::apply_frame_dimensions` |

Four details the port pins that a reading of the loop's shape alone would miss:

- **The end frame is inclusive.** The latch is set inside the `StGetNext` wrapper `FUN_801CF740`
  (`801cf788`) on the frame whose number *reaches* the slot's `+0x0C`, so that frame is decoded
  and displayed before the loop exits.
- **The code-buffer toggle runs before use** (`FUN_801CFA14` computes `ctx[8] = (ctx[8] == 0)` and
  then indexes with the new value), so a movie's first frame decodes into buffer **1**, not 0.
- **Signed MDEC output is unconditional.** The one `FUN_801CFD84` call site passes flags `3` for a
  colour slot and `2` otherwise; bit 1 - the `0x02000000` signed-output bit - is set either way,
  and only the `0x08000000` depth bit tracks the slot. That is the register-level counterpart of
  the `+128` luma offset in [`MdecDecoder`](#6-420-upsampling--bt601-colour-conversion).
- **The decode geometry follows the bitstream, not the dispatch table.** `FUN_801CF740` reads the
  frame's width and height out of the **sector header** (`+0x10` / `+0x12`), caches them in
  `DAT_801D0D50` / `DAT_801D0D54`, and every frame writes them into five halfwords of the decode
  context: both frame rects' width (`+0x1C` / `+0x24`, put through the same `* 3 / 2` 24-bit scale
  as the rest of the loop) and height (`+0x1E` / `+0x26`), plus the slice rect's height (`+0x32`).
  The slice rect's *width* at `+0x30` is left alone - it is the fixed macroblock-column stride.
  So the slot's `+0x18` / `+0x1C` only seed `FUN_801CF8B0`, and a table that disagrees with the
  movie loses from the first frame onward.

Three ping-pongs run at different rates and are easy to conflate: the **MDEC code buffers**
(`ctx+0x00`/`+0x04`) flip once per frame, the **frame rects** (`ctx+0x18`/`+0x20`) once per frame
buffer, and the **slice staging buffers** (`ctx+0x0C`/`+0x10`) once per 16-pixel column.

The slice cursor itself is a small state machine: each MDEC-out completion advances `ctx+0x2C` by
one column (`0x18` VRAM cells at 24bpp, `0x10` at 16bpp), and when the cursor passes the active
rect's right edge the two frame rects flip and the cursor restarts on the new origin. A buffer
whose width is not a whole number of columns takes its remainder as the *leading* step, so the
last column of every row lands flush on the right edge.

### Bitstream decode + MDEC feed (overlay)

`FUN_801CFA14` VLC-decodes each demuxed frame into an MDEC-code list, double-buffered; the decoder
is selected by `DAT_801E09FC`:

- **Iki** (`FUN_801D0378`, retail movies): decompresses the per-block qscale/DC table with the
  LZSS decoder `FUN_801D0604` (the retail original of `legaia_mdec::iki_lzss_decompress` -
  control-byte LSB-first, length `+3`, 1/2-byte offsets) and converts the AC-only bitstream using
  the GTE leading-zero-count register as the VLC prefix scanner.
- **STRv2/v3** (`FUN_801D070C`, dev slots 9/10 only): standard VLC with per-block DC deltas,
  lookup table unpacked at runtime by `FUN_801F1A00` into `DAT_801E0A00`. The play loop calls the
  unpacker **unconditionally**, once per FMV (`801cf210`), even for Iki slots that never read the
  table. Ports: [`legaia_mdec::strv2_table`](../../crates/mdec/src/strv2_table.rs) (the unpacker,
  reachable as `mdec strv2-table <overlay>`) and its consumer
  [`legaia_mdec::strv2_decode::decode_frame`](../../crates/mdec/src/strv2_decode.rs). The table is
  not a run/level table - it stores the **pre-baked MDEC output codes** (one to three per hit, plus
  a per-entry bit length), carved into four regions: luma DC (`+0x0000`) and chroma DC (`+0x0400`)
  indexed by `acc >> 24`; AC primary (`+0x0800`, 8-byte, `acc >> 19`); AC secondary (`+0x10800`,
  `acc >> 23`). So `FUN_801D070C` is a bit-prefix lookup: only the DC coefficients (raw 10-bit in
  v2, size-prefixed predicted differences chained per channel in v3), the `0x7C1F`-escape raw codes
  and the 65-code `0xFE00` end padding are computed. Dead in retail (no released movie uses it), so
  the port has no golden decode to check against; the tests pin the distinct code paths against the
  disassembly. See [STRv2 VLC table](#strv2-vlc-lookup-table-fun_801f1a00).

#### STRv2 VLC lookup table (`FUN_801F1A00`)

The table is `0x8800` `u16` entries (`0x11000` bytes) at `0x801E0A00`, ending flush against
`FUN_801F1A00` itself - the abutment is what pins the size, and it matches the `0x87FF` loop bound
at `801f1ab8`. It is unpacked in two passes from a compressed blob at `0x801F1AE8`, the bytes
immediately after the unpacker:

1. **Mode-switched LZ77.** A control byte `< 0xF0` emits `n + 1` bytes; `0xF0` selects literal
   mode; `0xF1..=0xFF` reads one more byte and sets the match distance to
   `((b << 8) | next) - 0xF0FF`. The distance is *sticky* - it survives across control bytes until
   the next escape - and copies are byte-at-a-time, so they may overlap. `0xFF 0xFF` ends the
   stream (distance `0xF00`).
2. **XOR de-delta at a four-entry stride**: `out[i] ^= out[i - 4]` for every `u16` index
   `4..=0x87FF`. The eight-byte lag is the table's own record width, which is what makes the table
   compress at all.

The MDEC feed is register-level in the overlay: `FUN_801CFD84` sets the 24bpp/16bpp control bits
and starts the DMA-0 code upload (`FUN_801CFFDC`); the MDEC-out slice callback `FUN_801CF56C`
`LoadImage`s each decoded 32-pixel-wide strip into the slot's VRAM frame rect, alternating the
double-buffered rects. In/out sync waiters `FUN_801D0100` / `FUN_801D0198` spin on the MDEC status
register and dump the DMA/FIFO state on timeout (`FUN_801D0248` - the `MDEC_in_sync` /
`MDEC_out_sync` strings that identify the overlay); `FUN_801CFEE0` is the reset
(`MDEC_rest:bad option(%d)`).

Those five are the clean-room boundary of this subsystem, and they are the only part of the
STR overlay's decode path that is **not** ported. Everything above them - the play loop, the ring
and stream setup, the frame pump, the slice callback, the output control word - has a
[`crates/mdec`](../../crates/mdec/README.md) counterpart in the table above, because each is a
decision about the bitstream. `FUN_801CFFDC` / `FUN_801CFEE0` / `FUN_801D0100` / `FUN_801D0198` /
`FUN_801D0248` are instead MDEC command/status register writes, DMA-0/DMA-1 channel kicks, busy
spins on a `0x100000`-iteration budget and a printf of the FIFO bits: they describe a chip the
software decoder does not model, so they carry no port site and are listed in
`scripts/ci/port-catalog-ignore.toml`. The two spin waiters additionally share their VA with the
fishing overlay's own resident, so the bare address is not one function either.

#### Remaining MDEC / St helpers

Four more overlay helpers sit on that same clean-room boundary and carry no port site:
`FUN_801CFAD4` is the MDEC-decode watchdog - it spins up to `0x800000` iterations on the
decode-done flag `ctx+0x34` and, on timeout, prints `time out in decoding` and force-flips
the code buffer (`ctx+0x28`); `FUN_801CFE00` is an 8-instruction thunk to the DMA-0 code
upload `FUN_801D0070` (the `FUN_801CFFDC` family); `FUN_801CFC18` wraps the MDEC reset
`FUN_801CFEE0`, adding a DMA reset (`func_0x8005FD88`) when its argument is `0`; and
`FUN_801CFCDC` stages the two double-buffered output rects into `&DAT_801D0D5C` /
`&DAT_801D0D9C`. The frame-poll wrapper `FUN_801CF740` is the logic sibling that stays
*inside* the port: it loops `StGetNext` (`FUN_8005EF40`, up to 2000 spins), sets the
inclusive end-frame latch `DAT_801E09F8` when the demuxed frame number reaches the slot's
`+0x0C`, and re-programs the decode rects from the sector header's own dimensions - both
ported within [`str_player`](#engine-port---legaia_mdecstr_player). It also builds a stack
`RECT` of `(0, 0, slot_width * 3/2, slot_height * 2)` whenever the cached dimensions change;
nothing in the printed body consumes it, so the port does not reproduce it.
`see ghidra/scripts/funcs/overlay_str_fmv_0x801CFAD4.txt` /
`overlay_str_fmv_0x801CF740.txt`.

## XA channel selection

Two distinct retail paths, neither of which lives where the old hypothesis put it (the STR overlay
holds **no** channel selector):

- **STR FMVs: no channel selection at all.** The streaming read runs Setmode `0xE0` -
  `CdlModeRT` (play XA-ADPCM sectors in hardware) *without* `CdlModeSF` (sector filter). With the
  filter off the drive plays **every** ADPCM-flagged sector it passes, and each `MV*.STR`
  interleaves exactly one XA track - `(file 1, chan 0)`, stereo 37.8 kHz 4-bit, 1 audio sector per
  8 (verified across all six movies' raw subheaders). The audio is routed through the SPU CD input
  (opened/muted by the play loop), never through the data path. So the per-cutscene "(file_no,
  ch_no) -> name" question dissolves for FMVs: audio selection *is* file + frame-range selection
  via the dispatch table, and the multi-channel `\DATA\MOV.STR` container the old hypothesis
  invoked is a dev leftover that isn't on the retail disc.
- **XA clips (`XA1.XA..XA34.XA` - voice banks + streamed music): `CdlSetfilter`.** The
  SCUS-static clip starter `FUN_8003D53C(clip_id, chan, duration_sectors)` reads the 8-byte
  `[CdlLOC][u32 byte_len]` clip table at `0x801C6ED8` - **slot `i` = file `XA<i+1>`** (34 slots,
  runtime-built from the ISO file list; title-capture-pinned, lengths byte-exact vs the disc
  files) - then its CdSync-callback state machine `FUN_8003D764` sequences the drive:
  `CdlSeekL` -> Setmode **`0xC8`** (`Speed | RT | SF` - filter ON) -> `CdlSetloc` ->
  **`CdlSetfilter` with `{file = 1, chan = <the caller's chan argument>}`** (filter struct at
  `0x8007BBC0`/`0x8007BBC1`) -> `CdlReadS` -> `CdlNop`/`CdlGetlocP` polling until the end LBA
  (`gp+0x974`). Every XA sector on the disc carries `file_no = 1` (subheader-verified), matching
  the hard-coded file byte; the **channel is caller-supplied** - e.g. the menu voice dispatcher
  `FUN_8004FCC8` derives `clip slot = (id - 0x100) >> 3` (remapped `1/3/5 -> 0x1A/0x1B/0x1C`) and
  `chan = id & 7`. `FUN_8003EAE4` is the by-index sibling starter; `FUN_8003ED04` the stop.
  `see ghidra/scripts/funcs/8003d764.txt` / `8003d53c.txt`. The pure computations of that dispatch
  chain - the id -> `(clip_slot, channel)` mapping, the length-field -> `duration_sectors` scale
  `(len*60+99)/100`, and the starter's end-LBA offset `(duration*150+149)/60` clamped at `0x2A30` -
  are ported in [`legaia_engine_shell::xa_clip`](../../crates/engine-shell/src/xa_clip.rs); the CD
  control / `CdlSetfilter` state machines around them stay hardware-side and unported.

So the complete channel map is: **movies** = one track per file at `(1, 0)`, selected by
`fmv_id -> MVn.STR + frame range`; **XA files** = `(1, chan)` inside `XA<clip_id + 1>.XA`, with
`chan` picked per cue by the caller of the clip starter. The per-`(file, chan)` content itself is
extractable via `xa demux-disc-all` (316 channels across the 34 files - 16-channel mono voice
banks and 8-channel stereo music). Which game systems fire which `(clip_id, chan)` cues beyond
the menu voice path is per-caller data, not a single table - an open census, tracked in
[`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).

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

There is no per-cutscene channel map to recover: the retail STR player reads with the sector
filter **off** (`CdlModeRT` without `CdlModeSF`), so it plays whatever single XA track the movie
interleaves - see "[XA channel selection](#xa-channel-selection)" for the traced mechanism and for
the `CdlSetfilter` path the separate `XA*.XA` clips use. The 8-bit-ADPCM coding mode is decoded
too (the cutscene audio path maps each channel's `coding_info` width); no 8-bit audio appears in
the movie corpus, so it stays an untested fallback rather than a verified path.

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

For PROT-scene routing (`play --scene cutsceneN`), the retail mapping is fully disc-decoded:
`fmv_id -> movie + frame range` via `legaia_asset::fmv_dispatch`, per-scene trigger ids via
`man_field_scripts::scene_fmv_triggers`, and the post-play return scene via the master dispatch
(see [`str-fmv-table.md`](../formats/str-fmv-table.md#authoritative-runtime-mapping)).

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
| `0x801CAE08` | variable | 24 B | libcd `CdlFILE` directory cache (the `MOV` dir while an FMV plays: `.`, `..`, `MV1.STR;1`..`MV6.STR;1`) |
| `0x801CCA80` | 336 B | 56 B × 6 | ISO9660-shape directory record copies of the same six files |
| `0x801CE810` | ~150 B | variable | Path-string table (\\DATA\\MOV.STR;1, \\DATA\\MOV15.STR;1, \\MOV\\MV1A.STR;1, \\MOV\\MV6..MV1.STR;1) |
| `0x801CE8AC` | ~50 B | variable | Post-FMV return-scene labels (CDNAME shape) |

### MDECin DMA-callback hook (`FUN_801CFE98`)

`FUN_801CFE98` is a nine-instruction wrapper that forwards its single
argument to the PsyQ `DMACallback` entry `FUN_8005FDE8` with the channel
argument hard-coded to **0** - DMA channel 0 is MDECin, the CPU→MDEC
compressed-data feed. It is the FMV path's sibling of the SPU callback
registration `FUN_8006A0E0`, which calls the same PsyQ entry with channel
`4`.

The wrapper is byte-identical, and at the same VA, in PROT **0970**
(`cutscene_str`) and PROT **0971** (`debug_menu`) - both verified by
disassembling each extracted image at base `0x801CE818` (file offset
`0x1680`, inside 0971's own `0x1800` bytes, so this is genuine
co-residency and not the 0971 → 0972 over-read). No static caller appears
in either image; the callback is installed from a code path this corpus
does not cover, so **who** registers it is Unknown.

### Directory-record cache

The 24-byte records at `0x801CAE08` are PsyQ `CdlFILE` structs - `[u32 CdlLOC][u32 size][char name[16]]` - libcd's `CdSearchFile` cache for the last directory searched, not an FMV structure. An earlier name-first parse ("compact MV table at `0x801CAE40`") was phase-shifted 8 bytes and paired each name with the *next* record's location; the shift artefacts ("`MV1` points at disc `MV2`") dissolve at the `CdlFILE` phase. Details + the title-capture cross-check (the same cache holding `XA1.XA..XA34.XA`) in [`str-fmv-table.md`](../formats/str-fmv-table.md#directory-record-cache-0x801cae08-24-b-cdlfile-records).

The historical name-first window parser is `legaia_asset::str_fmv_table::parse_entries` (kept as a capture-forensics helper); the residency check + pinned addresses are in `legaia_engine_core::capture_observations::str_fmv_overlay`.

### Post-FMV return scenes

The FMV overlay's data section carries the CDNAME labels of seven field scenes - distinct from the `op*` / `ed*` engine cutscene scenes:

```text
town0b  map01  chitei2  map02  jou  uru2  town0e
```

These are the **destinations the master dispatch `FUN_801CEA3C` hands control to after playback**: one label per mid-game `fmv_id` (1..4, 6..8), written to the next-scene name global `0x80084548` with a spawn/door word at `0x80084540` (see [Retail playback engine](#retail-playback-engine-str-overlay--scus-st-streaming-library)). They are *not* the trigger-op scene list (the `0x4C 0xE2` ops live in a different scene set, only `chitei2` overlapping - a scene that returns to itself). The heuristic in [`cutscene_str_for`](../../crates/engine-core/src/scene.rs) covers the `op*` / `ed*` scenes in CDNAME order; `FMV_TRIGGER_FIELD_SCENES` (sibling constant) records this overlay label list.

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

- **`_DAT_8007BA78`** - FMV index. Read by the str_fmv overlay's master dispatch to select a 32-byte dispatch-table slot from `0x801D0A6C`. On retail USA the table has 23 slots; the nine retail movies occupy `fmv_id 0..=8` - exactly the range the per-STR FMV trigger corpus observes - and every `MVn.STR` on the disc is dispatched (`MV3.STR` carries four segments by frame range; slots 9+ are dev files absent from the disc). The table is static overlay data, decoded from the disc by `legaia_asset::fmv_dispatch`; see [`str-fmv-table.md`](../formats/str-fmv-table.md#fmv-dispatch-table-0x801d0a6c-23--32-b) for the mapping.
- **`_DAT_8007B83C`** - next-game-mode global. Setting it to `0x1A` (decimal 26) kicks the main mode dispatcher (`FUN_80017714`) into `StrInit` on the next frame, which loads the str_fmv overlay and reads `_DAT_8007BA78` to pick the file.

#### `_DAT_8007BA78` has exactly two writers

An instruction-level sweep - not a search over decompiled-C text - finds every
access to `0x8007BA78` across `SCUS_942.54` and all 1233 extracted `PROT` entries,
matching any `lb/lh/lw/lbu/lhu/sb/sh/sw` whose effective address resolves through a
`lui` / `lui`+`addiu` base. The result is six distinct sites:

| Site | Kind | Where |
|---|---|---|
| `0x801E30F4` | store | field overlay, the `4C E2` FMV-trigger op |
| `0x801DDCE8` | store | menu overlay, the title attract-countdown tick |
| `0x801CEA74`, `0x801CEC94`, `0x801CECA8`, `0x801CF4E0` | loads | STR overlay dispatch + play loop |

`SCUS_942.54` itself never touches it. Two apparent extra hits are duplicate
on-disc copies, not new sites: PROT 0896 carries the same field overlay as 0897
shifted by `0x9000` (a 0x46800-byte identical span straddles the store), and the
STR overlay is replicated across PROT 0967/0968/0969/0970. This is what rules out
a per-FMV event table: nothing but the trigger op and the attract tick can set the
id, so an FMV cannot carry teleport or story-flag side-effects of its own.

**Coverage limit.** The sweep reads raw bytes, so it cannot see code inside an
LZS-compressed section. Every code-bearing class in `PROT/categorize.json`
(`mips_overlay`, `overlay_data_blob`, `overlay_ptr_table`) is stored uncompressed,
so no overlay hides there - but that last step is an inference from the
classifier, not a decode. The dispatch record itself is no longer an open margin:
every one of the slot's eight words is read by the play loop and by nothing else -
see [the consumer sweep](../formats/str-fmv-table.md#every-word-of-the-record-is-a-play-loop-input).
`legaia_asset::fmv_dispatch` keeps six of the eight; the two it drops (`fb_x`,
`fb_y`) are the decode rect's VRAM origin, resolved in `legaia_mdec::str_player`.

The field-VM port handles this op as `op4c_n_e_sub2_fmv_trigger(fmv_id: i16)` in [`legaia_engine_vm::field`](../../crates/engine-vm/src/field.rs) and the world's [`FieldHostImpl`](../../crates/engine-core/src/world.rs) records the request as `World::pending_fmv_trigger` plus a `FieldEvent::FmvTrigger { fmv_id }`.

The world drives the Field → Cutscene → Field flow itself, mirroring the retail next-game-mode dispatch: the **next** `World::tick` consumes `pending_fmv_trigger` at the top of the frame (one frame after the op fires, exactly as `FUN_80017714` reads the next-game-mode global a frame late), and if the id resolves to a playable slot (`cutscene::fmv_index_to_str_filename` is `Some`) it flips `World::mode` into `SceneMode::Cutscene` and records the active FMV (`World::active_fmv()`). While the FMV plays the world **suspends the field VM** (the STR overlay owns the frame in retail); the host polls `World::active_fmv_str_filename()`, plays the resolved `MV*.STR`, and calls `World::finish_cutscene()` when playback ends,
which returns to the field with the field-VM program counter already past the op. A `fmv_id` whose runtime slot points at a dev/missing path is drained as a no-op (no mode flip).
The resolver (`fmv_index_to_str_filename`) mirrors the retail nine-slot map - `fmv_id 0..=8`, `MV3.STR` shared by slots `2..=5`, dev slots `9..=22` returning `None` - and its sibling `fmv_post_play_return_scene` carries the master dispatch's post-play return scenes (the `0x801CE8AC` list).
The disc-parsed `legaia_asset::fmv_dispatch::FmvTable` is the authoritative source (see [`str-fmv-table.md`](../formats/str-fmv-table.md#authoritative-runtime-mapping)). The `legaia-engine play` loop runs this flow headlessly, decoding the resolved STR via MDEC to report its frame count. The windowed `play-window` host plays it **in the engine window**: when a tick flips the world into `SceneMode::Cutscene`, it resolves the `MV*.STR` and decodes it (shared `cutscene_av` module with `play-str`), suspends world ticks, and shows the video one frame per redraw; once the frames drain it calls `finish_cutscene()` and resumes the field.
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

The overlay's seven-label list above is therefore *not* the trigger-scene set (only `chitei2` appears in both) - it is the post-play **return-scene** table. Outside the MAN-carried scripts, a raw sweep also finds in-range `4C E2` byte candidates in `taiku` (`fmv_id 5` - under the corrected dispatch stride that is the fourth `MV3.STR` segment, the one slot with a "stay in the current scene" hand-off, which fits a `taiku` trigger) and `opmap01` / `koin1b` (`fmv_id 7`) in uncompressed regions of non-MAN scene structures - uncontextualized byte matches (the same sweep "finds" triggers inside VAB sample data), kept as candidates rather than pins.

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

The engine's [`CutsceneNarration`](../../crates/engine-core/src/cutscene_narration.rs) is that roller as a state machine, with per-scene [`RollerParams::for_scene`](../../crates/engine-core/src/cutscene_narration.rs) carrying the capture-pinned values.
The timeline stepper installs each block's pages when its PC reaches the block's op ([`NarrationSite`](../../crates/engine-core/src/cutscene_timeline.rs)) and, mirroring the child-context spawn, lets the timeline **continue** (non-blocking) - for **every** block, the last included - so the record's own choreography plays under the crawl. Its two holds are exactly retail's: at a **new** block's op while a prior roller still scrolls, and at the record's **terminal SceneChange op** while any pages are still up. `World::tick` advances the roller independent of the timeline; the host renders `visible_lines()`.

The blocks-at-SceneChange placement (not at the last crawl's own op) is load-bearing for wall-time: `map01`'s fly-in record follows its final crawl with `WaitFrames` 600 + 330, and the retail capture's leg span only fits those waits running **concurrent** with the 3-page roller - a stepper that parks at the crawl op instead serializes the roller against the authored tail and overshoots the leg by the whole roller duration (~14 s). In the three `op*` legs the last crawl sits close to the SceneChange, so the two park placements are nearly indistinguishable there; `map01` is the discriminating case.

The `RollerParams` px/frame values are pinned against retail's **~60 Hz** field frames, but the engine sim ticks at **100 Hz** ([`redraw`](../../crates/engine-shell/src/bin/legaia-engine/window/event_handler/redraw.rs) `advance_tick(100)`). Advancing the roller once per sim tick would scroll it 1.67× too fast and drain the crawl ~6 s early, opening the inter-crawl gap. `World::tick` therefore drives the roller off a **60 fps sub-clock** (`field_frame_accum += 60; step = accum >= 100`, ~0.6 roller-frames per sim tick), so the crawl duration matches retail wall-time.

The old "field-VM step-parallelism" reading of the residual inter-crawl dead-air is **retired**. Retail has no hidden parallelism to catch up with: its actor lists are walked in full every frame (`FUN_8002519C`), so every script context - the timeline, the crawl roller, the camera mover - already gets one run-until-yield slice per frame, and the engine already runs the roller and the helper contexts that way. The measured gap was a **units** error instead: the engine's timeline was stepped once per 100 Hz sim tick while every duration a record can express is counted in retail's 60 Hz display frames, so `WaitFrames` drained 1.67x fast. See [Record pacing](#record-pacing---the-60-hz-sub-clock).

#### Roller op operands (Ghidra-traced)

The spawner and the crawl-geometry config are two distinct sub-ops of field-VM op `0x4C` (`MENU_CTRL`), dispatched by `switch(op0 >> 4)` then `switch(op0 & 0xF)` inside `FUN_801DE840` (`see ghidra/scripts/funcs/overlay_0897_801e0c3c.txt`, cases `0x4C` nibble-8 sub-0 and nibble-E sub-8). Both take the cross-context target `0xF8` (the player / camera-anchor actor).

- **Spawn - `CC F8 80 N`** (op `0x4C`, op0 `0x80`: outer nibble `8`, sub `0`). Operand `N` is the **page count**. The op allocates a child actor from the template `DAT_801F28A0` (via `FUN_80020DE0`, which copies template `+0x8 = FUN_80037174` into the actor's handler word `+0xC`), points the child's script pointer `+0x90` at the operand's `N` byte, and leaves the following bytes framed `[N][page0][page1]…[page(N-1)]` (each page `0x1F <ascii> 0x00`). The parent then measures each of the `N` pages (`FUN_8003CA38`) to advance its own PC past the whole block and **continues** (non-blocking). The roller reads `N` back as the first byte at `+0x90`.

- **Geometry config - `CC F8 E8 …`** (op `0x4C`, op0 `0xE8`: outer nibble `0xE`, sub `8`; 10-byte op). It fetches four signed-16 LE words at operand `+1/+3/+5/+7` (`word0..word3`); `word3` selects the sub-mode:
  - `word3 == 0` seeds the three crawl-geometry globals at `_DAT_801C6EA4` (each word defaults if written as `0`): `+0x4C = word0` (default `0x40`), `+0x4E = word1` (default `0x08`), `+0x50 = word2` (default `0x04`).
  - `word3 == 1` finds the live roller by handler `FUN_80037174` (`FUN_8003CF04`) and either **pauses** it (`word0 == 0` sets actor `+0x10 |= 0x80000`) or writes the stop trigger `_DAT_801C6EA4 +0x52 = word0`.
  - `word3 == 2` **resumes** the roller (clears `+0x10 & ~0x80000`); `word3 == 3` unlinks the child and raises the terminal-kill flag (`+0x10 |= 8`).

  Re-audited from the disassembly (not the decompiled C), the four-words / `word3`-selector shape holds and is **not** a dropped-slot artifact: the handler at `overlay_0897_801e0c3c.txt` `0x801E3378` advances the PC by `0xa`, then fetches `word0..word3` at operand `+1/+3/+5/+7` via `FUN_8003CE9C` (signed-16 LE). The `word3 == 0` seed writes exactly three stores - `sh` to `+0x4C/+0x4E/+0x50` of `_DAT_801C6EA4` at `0x801E34B0/B4/BC` (defaults `0x40/0x08/0x04` applied at `0x801E348C/98/A4` when the source word is `0`). The stop-trigger store `sh word0, +0x52` is at `0x801E3414`, the pause `+0x10 |= 0x80000` at `0x801E3408`, the resume clear at `0x801E343C`, the kill `+0x10 |= 8` at `0x801E347C`. `word3` is a selector only - it is never itself stored.

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

- **Record header.** Partition-2 records are **named records**, *not* the partition-1 `[u8 N][N*2 locals][4-byte header]` shape. Layout: `[u8 name_len][name_len*2 SJIS name][u8 C0][C0 bytes][u8 C1][C1*u16][u8 C2][C2*u16]<script>`. The name length is in characters; the three condition-list gates are story-flag predicates the dispatcher tests before running the record (block 1 = OR gate, block 2 = AND gate; block 0 is skipped here). The script entry offset is `1 + name_len*2 + (1+C0) + (1+C1*2) + (1+C2*2)`. For `opdeene`'s record 18 (`name_len=6` "Opening", all blocks empty) that is `0x10` - the `0x34` EFFECT op (an instant colour reset to neutral) that opens the prologue, immediately followed by `GFLAG_SET 26` at `+0x17`. Decoder:
  [`man_field_scripts::partition_record_span`](../../crates/engine-core/src/man_field_scripts.rs) (`FUN_8003BDE0`).
- **Dispatch.** `FUN_8003BDE0` resolves a partition record by index, walks the header, and **spawns a VM context** (`ctx[+0x90]` = record base, `ctx[+0x9e]` = entry PC, `ctx[+0x10] |= 0x100` "run me"); the per-frame runner `FUN_80039B7C` then loops `FUN_801DE840` on it until a yield. The index comes from the two caller families [above](#record-spawn-mechanisms-live-probe-pinned) - an entry-script op `0x44` or a walk-on tile trigger (`FUN_801D1EC4`) - not a sequential partition walk.
- **Cross-context target `0xF8`.** Nearly every op in the timeline carries the extended-target byte `0xF8` (`A3 F8 …` = MoveTo, `CC F8 …` = MenuCtrl). `FUN_8003C83C(0xF8)` resolves to `_DAT_8007C364` - the **player / camera-anchor actor** - so the timeline drives the camera/lead actor.
- **Narration op.** `CC F8 80 N` (op `0x4C`, outer-nibble 8, sub-0) **spawns the roller child** - the on-screen-text actor whose handler is `FUN_80037174` - over the `N` inline pages; the parent timeline **keeps running** so the between-block camera choreography plays under the scroll (see [Narration playback](#narration-playback---the-crawl-roller-fun_80037174)). The single-line balloon path (`4C E1`, spawner `FUN_8003C764` → handler `FUN_801DA7F0`: centered `X = (320 − width)/2`, `Y = 180`, 120-frame timer, kills its predecessor, ends early when the player-engaged flag `_DAT_8007C364 +0x10 & 0x80000` re-raises after the spawning engagement drops; port `engine-core::text_balloon`) is a **different op** - not the opening crawl.
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

  `engine-core`'s [`Camera`](../../crates/engine-core/src/camera.rs) holds the ten live globals as `RetailCamGlobals`, seeded on scene entry with the `FUN_80025C24` field defaults (angles `(0x1B8, 0x64, 0)`, `tr_eye = (0, -256, 16420)`). A Configure with `apply == 0` writes the masked slots through; `apply != 0` arms the shared `camera_mover` over the beat's duration in display frames.

  The **eye-space translation trio has no representation outside that struct and the shell's `cutscene_view`**, which is what made headless consumers (`sim-trace`, the state-trace oracle) frame every scripted shot from the follow orbit while the angles moved correctly around it. In free-roam the follow camera owns the focus globals (`FUN_801DBE9C`, negated anchor XZ); once a scene executes any Configure the script keeps them, matching retail's step-shaped focus across a shot.

  **The full transform is `screen = H · (R·(v − focus) + tr_eye) / Ze`; the eye-back depth is `tr_eye.z` (slot 5), not a missing scalar.** The once-per-frame view builder `FUN_800172c0` assembles it: build `R` from the angle globals (`FUN_80026988`), left-multiply the constant base matrix `DAT_8007BF10` (a uniform `24576·I` = **6× world scale**), copy the eye-space translation trio `_DAT_800840B8/BC/C0` into the view struct's `.t`, then MVMVA the negated focus `(_DAT_80089118/1C/20)` through `R` and add `.t` - giving the uploaded GTE translation `TR = R·(−focus) + tr_eye`, so every world vertex maps to `R·(v − focus) + tr_eye`.
  The camera-rotation build is pinned: `FUN_8001CF50` composes `R` by rotating about each axis with the angle globals - `RotMatrixX(pitch=_DAT_8007B790)` at `0x800461A4`, `RotMatrixY(yaw=_DAT_8007B792)` at `0x8004629C`, `RotMatrixZ(roll=_DAT_8007B794)` at `0x8004638C` (each masks the angle to 12 bits and indexes the shared sin/cos LUT at `0x80070A2C`, `4096 = 360°`, `+0x800` = the quarter-wave cosine offset; composed via GTE `mvmva`).
  **So param 0 is the camera PITCH, not a "rot/zoom" word** - the zoom is H (a separate projection register). The eye sits *behind* the focus by `tr_eye` (in the 6×-scaled space); it is NOT at the focus.
  The commit's second argument decides between two behaviours, and the third selects the ease curve: the field VM calls `FUN_801DE084(0x801C6EA8, apply, op0 >> 2 & 0xF)`, reading `apply` as the u16 at operand `+2` (`overlay_0897_801de840.txt`, case `0x45` sub-`0x00`).

  - **`apply == 0` - snap.** `FUN_801DE084` writes the ten params straight into the camera globals and marks every live mover actor dead, cancelling a glide in flight.
  - **`apply != 0` - glide.** It tail-calls `FUN_801DD310`, which finds (or allocates) the **one** camera-mover actor - the node in list `_DAT_8007C34C` whose tick fn is `FUN_801DC0BC` - and hands it a 40-byte block of ten `(start, end)` u16 pairs. `start` comes from the LIVE globals, `end` from the staging struct. It then sets `actor[+0x9C] = 0` (progress), `actor[+0x9E] = apply` (duration) and `actor[+0x50] = curve`.

  Two structural consequences:

  - The mover is a **separate actor**, dispatched by the per-frame actor-list walk `FUN_8002519C` like any other. The record that staged the beat does **not** block on it - choreography and glide run in parallel, and a record whose `WaitFrames` run out first simply moves on while the camera keeps travelling. A long `apply` is therefore a *dolly velocity*, not a promise of arrival: `opurud` stages an `apply 2300` eye glide whose next beat lands about a quarter of the way through, so retail never reaches that staged target.
  - A beat landing **mid-tween** re-seeds every axis' `start` from the current interpolated globals and resets the shared progress to `0`. There is one progress counter, one duration and one curve for all ten axes - no per-axis state, no carry-over, and no discontinuity.

  **Per-frame law** (`FUN_801DC0BC`, body `0x801DC104..0x801DD220`):

  ```text
  t = min(t + DAT_1F800393, d)
  per axis (start s, end e):  s if e == s;  e if t >= d;  else s + curve_offset(e - s, t, d, curve)
  ```

  `DAT_1F800393` is the adaptive frame-skip factor - the logic tick's `dt` in display frames - so `t` counts **display frames** and `apply` is a duration in display frames 1:1. Live-confirmed: `opurud`'s `apply 2300` beat advances its progress exactly 30 per 30 display frames while the factor reads `3`. Arrival is exact (no overshoot, no asymptote), and on `t >= d` the mover sets its own dead bit and frees the pair block, so a glide is one-shot.

  | `curve` | shape | `curve_offset(k, t, d, ·)` |
  |---|---|---|
  | `2` | quadratic ease-**out** | `n = k*t; (n + (n/d)*(d - t)) / d` |
  | `3` | quadratic ease-**in** | `((k*t)/d * t) / d` |
  | `4` | ease-in-out | quad-in over `h = d>>1` to the midpoint `k>>1`, then curve `2` from there over `h` |
  | `1`, and any other value | linear | `(k*t) / d` |

  The double truncating divisions are load-bearing - `(k*t/d)*t/d` is not `k*t*t/(d*d)` in integer arithmetic, and retail computes the former. Every axis uses the **same** curve, the three angles included; the angles are lerped as plain integers over their raw 12-bit values, with no shortest-arc handling.

  **Falsified**: the earlier reading that mode 1 runs the eye trio linear while pitch / yaw ease out. There is no per-axis curve split - the mover re-reads the same `actor[+0x50]` once per axis. Mode 4 is the two-half integer curve above, not smoothstep, and mode 3 (ease-in) was missing from the model entirely. The earlier attribution of the mover to `FUN_801DB510` was also wrong: that is the **follow / scroll** camera (its `srav` lerp toward `_DAT_801F2798`-table targets), a different mode of the same globals.

  The port is [`legaia_engine_vm::camera_mover`](../../crates/engine-vm/src/camera_mover.rs) (the integer law verbatim, plus `curve_unit` as the normalized `f32` shape the renderer's [`CutsceneCameraInterp`](../../crates/engine-render/src/window.rs) evaluates). It was validated against a live headless capture of the retail mover: 2471 of 2480 sampled `(axis, start, end, t, d, curve) -> global` tuples reproduce exactly, and every remaining sample resolves under a 1-6 display-frame read skew (the probe's own round-trip lag) except the two frames on which a new beat re-armed the block mid-read.

  A second, frame-exact validation replays whole staged beats against per-display-frame recomp captures of the opening chain (the [recomp differential harness](../tooling/recomp-differential.md)): the env-gated oracle [`camera_mover_recomp_oracle`](../../crates/engine-vm/tests/camera_mover_recomp_oracle.rs) (`LEGAIA_RECOMP_TRACE_DIR`) reproduces the snap, mode-1, mode-2 and mode-4 beats **bit-exact** per frame within the mover's own 2-3-frame tick quantisation.
  Mode 1 measures linear on pitch/yaw across three independent beats (the per-axis "eased angles" split fails those captures by an order of magnitude), and the `town01` arrival H glide (`P2[3] +0x00C4`, `op0 0x13`, `apply` 600, H 412 → 512) decodes and measures as **mode 4** ease-in-out - the H slot participates in the glide like every other slot. The disc-gated pin `town01_arrival_camera` holds the three arrival beats' decoded `(apply, mode)` staging.

  **Held divergence**: `CutsceneCameraInterp` still arms glides *per component* (only re-arming an axis whose target changed) where retail re-seeds all ten and restarts the shared progress on every apply beat. The per-component model was adopted to stop a single-slot follow-up poke cancelling an in-flight dolly; under the retail rule that poke instead re-times the whole glide over the new `apply`. Closing it needs a beat-sequence counter on `CameraState` so the interp can tell a real re-stage from an unchanged frame.

  Confirmed against the `new_game_cutscene_intro_a` save state: focus `(8640, 0, 10304)` (mode byte `0x10` = anchor-follow), pitch `180` (≈15.8°), yaw `-2967`, roll `0`, H `792`, `tr_eye = _DAT_800840B8 = (260, 1293, 17145)`; the focus projects to screen `(792·260/17145 + 160, 792·1293/17145 + 120) = (172, 180)`, matching the party position in that frame's framebuffer.
  The captured RAM is the interpolated tween between two op-`0x45` keyframes (`opdeene` beat 0 `tr_eye = (−740, 512, 16384)`, focus `(10816, ?, 12224)`; a later beat `tr_eye = (118, 2241, 20795)`, focus `(5824, ?, 1984)`) - every axis of the capture sits between them. Note (don't re-walk): the GTE rotation matrix read straight from a save state is the last-rendered object's composed transform (row norms ≈ 6.0 = the base-matrix world scale), so recover `R` from the angle globals - but that `6.0` **is** the camera world scale, folded into `R` via `DAT_8007BF10`.

### Record pacing - the 60 Hz sub-clock

Retail paces cutscene records in **display frames**, and the two clocks that matter both count them through the same factor:

- Op-`0x4A` `WAIT_FRAMES` accumulates `DAT_1F800393` into `ctx[+0x54]` per visit and returns to the caller while the sum is below the operand (`overlay_0897_801de840.txt`, case `0x4A`).
- The camera mover accumulates the same `DAT_1F800393` into its progress (`FUN_801DC0BC`).

`DAT_1F800393` is the adaptive frame-skip factor - the number of display frames one logic tick spans (it reads `2`-`3` through the opening chain). A logic tick that runs once per `dt` display frames and credits `dt` per visit therefore banks exactly one unit per display frame either way, so **every authored duration is a duration in 60 Hz frames**, independent of the skip factor.

The engine's sim clock runs at 100 Hz. The narration roller was already corrected onto a 60 Hz sub-clock (`field_frame_accum += 60; step = accum >= 100`); [`World::step_spawned_record_contexts`](../../crates/engine-core/src/world/narration.rs) paces the modal cutscene timeline and the concurrent helper contexts off that same sub-clock, so a wait-dominated leg keeps retail wall-time too. `World::field_frames` counts the elapsed display frames for consumers that have to advance something in retail-frame time across a variable number of sim ticks - the renderer's camera glide diffs it rather than counting sim ticks.

The disc-gated oracle [`opening_chain_wall_time`](../../crates/engine-core/tests/opening_chain_wall_time.rs) pins the result against a headless capture of retail playing the same zero-input chain, per leg and for the chain as a whole. Stepping a leg at the sim rate instead of the sub-clock puts it ~67 % fast, far outside the test's per-leg bound.

The residual errors are **one-sided**: a retail leg span (scene-label flip to scene-label flip) includes the scene's load + mode-transition window before its opening record's first tick, which the engine does not model - its scene loads are instant. The `map01` world-map leg carries the largest window (~355 display frames of kingdom-bundle load + mode-2 init before the walk-on trigger's record starts, measured by aligning the retail camera trace against the P2[38] disasm), so the engine plays the record-authored span 1:1 and lands short of the retail label-to-label span by roughly that window. The oracle's per-leg bands are asymmetric for this reason: running LONG is the regression signal.

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
  cutting, `play-window` moves the rendered `(focus, pitch, yaw, H, tr_eye)` toward each new beat
  through [`window::CutsceneCameraInterp`](../../crates/engine-render/src/window.rs), which
  implements the capture-pinned `FUN_801DC0BC` mover law above per component: a Configure with
  `apply == 0` snaps its staged components (a hard cut), `apply > 0` glides each re-staged
  component over exactly `apply` sim ticks with the beat's mode-selected curve applied to
  **every** slot, the angles included (mode 1 linear; mode 2 quadratic ease-out; mode 4 the
  two-half ease-in-out - the shapes of `camera_mover::curve_unit`),
  arriving exactly and holding. Components an in-flight glide owns are only re-armed when a new
  beat re-stages them, angles glide along the shortest arc, and the interp resets to snap when the
  timeline first installs. A long `apply` behaves as a dolly velocity: `opurud`'s `apply 2300`
  eye glide is still ~3/4 short of its staged target when the next snap beat lands, exactly like
  retail - an interp that compresses the glide to arrive early parks the camera at extreme staged
  eye targets retail only drifts toward (the "camera inside the scene geometry" failure). opdeene
  mixes snap and glide: the entry shot snaps (`apply 0`), but the mid-prologue forest dolly is
  `apply 840`, paired with a `760`-frame `WaitFrames`, so the camera glides continuously *while
  the narration crawl scrolls* rather than snapping to a still hold. The glide is stepped in
  **sim-tick time** (once per world tick that elapsed, not once per rendered frame), so an
  `apply`-paced glide spans its authored sim-frame count even across a long `WaitFrames` where few
  ticks advance but many redraws fire.
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

### Scripted screen fade (op `0x4C 0x12`) + the effect colour (op `0x34` sub-0)

**Op `0x4C 0x12`** (7 bytes `[4C, 12, r, g, b, ramp_lo, ramp_hi]`) is the retail **screen-fade
primitive**: the global multiply tint `DAT_8007BCB8/B9/BA` (neutral `0x80`), optionally ramped
over `LE_u16(ramp)` frames by the slot-job spawner `FUN_8003C5F0`. Every field scene's `P1[0]`
entry script carries the arrival arm of the `0x52F`/`0x530`/`0x531` fade handshake (see
[`script-vm.md`](script-vm.md#the-0x5270x531-scene-transition-scratch-band)) -
`4C 12 00 00 00 00 00` (instant black) then `4C 12 80 80 80 44 00` (ramp to neutral over 68
frames), the **fade-in from black** that opens the prologue. New Game arms the handshake
(`World::begin_new_game` sets sysflag `0x52F`, the boot-side stage retail performs before the
field launches); the destination entry script consumes it. The engine runs the entry script's
load-frame slice at prologue-scene entry (`World::pre_run_entry_script`), so the instant black is
on screen before the first rendered frame, matching retail's load-frame execution. The tint
darkens the drawn 3D scene only - the narration crawl is a separate draw path and keeps scrolling
bright, as the retail capture shows - and persists across scene changes (retail's cross-scene
fade continuity). Engine model: [`fade::SceneTintRamp`](../../crates/engine-core/src/fade.rs)
(normalized, `1.0` = neutral) in `World::screen_tint`, stepped per `World::tick`, surfaced by
`World::scene_screen_tint`; `play-window` folds it into the colour-grade + depth-cue staging
(both branches of the shaders' cue mix carry it, so the tint distributes to the final pixel). A
landed non-neutral tint holds; a landed neutral drops to the identity path.

**Op `0x34` sub-0** (7 bytes `[34, op0, r, g, b, ramp_lo, ramp_hi]`; the sub-0 arm at
`0x801E1FB0` inside the field-VM dispatcher `FUN_801DE840` - a Ghidra-promoted intra-function
label, not a separate function) ramps the **effect-layer global colour** (neutral `0xFF`) toward
the operand RGB over the trailing word's frame count. The opening timeline drives it in the crawl
gaps (`34 05 00 00 00 D2 00` = to black over 210 frames, `34 01 FF FF FF 00 00` = instant
neutral, `34 01 FF FF FF 78 00` = up over 120 frames); the timeline's first op
(`34 05 FF FF FF 00 00`, instant neutral) is a colour *reset*, not a white flash, and an all-zero
colour is a ramp target, not a clear. **It is not a screen fade**: the retail cold-boot capture
holds the lit villager tableau across the whole span where the timeline's `34 01 00 00 00 28 00`
→ `34 05 FF FF FF 5A 00` pair would black a full-screen fade, falsifying the earlier
"between-beat black fade" reading (and the older "white flash + 50% wash" model before it). The
value feeds the effect layer - the creation-glow planes are the likely consumer, still an open
thread. Engine model: the same ramp type in `World::effect_tint` (scene-local, kept out of
`scene_screen_tint`). Disc-gated `opening_fade_from_black` pins both value models against the
real `opdeene` bytecode.

### Full-scene sepia grade (the gold prologue look)

The whole prologue-cutscene leg of the opening renders in a **persistent warm gold/amber
monochrome** - every 3D surface (terrain, foliage, the vignette actors) is tinted gold while the
white narration text stays white. It is distinct from the transient colour fade above. The
cold-boot pixel capture pins its scope: the grade **persists across `opdeene` / `opstati` /
`opurud`** and drops for the full-colour `map01` fly-in and `town01`.

**Retail mechanism (capture-pinned).** The grade is applied to the **loaded scene assets**,
not per frame: a live capture of the retail opening (recomp cold boot; VRAM peeked against the
disc TIMs) shows every CLUT row the `opdeene` bundle uploads rewritten **entry-for-entry** from
the disc value `(r, g, b)` (5-bit BGR555) to

```
L = max(r, g, b)   →   (R, G, B) = (L, max(L − 1, 0), L >> 1)
```

with the STP bit preserved - zero mismatches across the graded terrain rows (the green
foliage/ground page's row 509, the amber-rock row 508, the grey-scree row 501; 768 entries).
The gold prologue is a **palette-space luminance collapse to a gold ray**, not a render-time
tint: the same texel indices draw through gold-monochrome palettes. Two companion facts from
the same capture close the older readings:

- **No depth cue runs.** Walking every render node (the seven list heads at `0x8007C34C..`)
  across the whole opening: node `+0x78` (`IR0`, the DPCS blend factor `FUN_8002735C` loads
  per node, far colour packed at `+0x74` → GTE cr21-23) holds **0** on every node at every
  beat - the only non-zero sightings are momentary `far = black, IR0 = 0x1000` fades on
  vignette/text actors. The earlier "gold far colour + per-node depth-graded IR0" model of
  the grade is **falsified**; the far-field crush is the palette law seen through dark
  authored gouraud words, not a DPCS pull.
- **Packet colours split by source.** The GP0 draw list's textured prims carry either the
  runtime-emitted neutral `0x80,0x80,0x80` (the ground tile kernel's quads - drawn gold
  purely by their law-collapsed CLUT) or a small **amber family** `≈ (M, 0.94·M, 0.43·M)` -
  the collapse of each loaded TMD's authored full-colour word (the `0749` pack authors these
  meshes in green/blue) to the same gold ray. Near-field graded surfaces land `B/R ≈ 0.44`
  (`(L >> 1) / L`), matching the law.

The `opdeene` MAN itself carries **no** colour op (no op `0x4C 0x8A` ambient, no op
`0x4C 0x81` far colour), and its motion-VM section carries no per-actor depth-cue op `0x0C`
either; the grade is applied by the cutscene host to the scene's decoded assets at load. The
GTE back/ambient colour `DAT_8007B788` is `0x00202020` in `opdeene` vs `0x00FFFFFF` in
`town01` (`FUN_80043390`), but the field path issues no light op, so it is not the grade
mechanism.

**Engine port.** The engine keeps the disc palettes in its software VRAM and applies the law
in the mesh shaders instead - exactly equivalent, because a 4/8bpp texel *is* a palette entry:
[`Renderer::set_palette_grade`](../../crates/engine-render/src/renderer/state.rs) arms the
**palette-collapse mode** (`palette_law_word` / `palette_collapse_prim` in
[`shaders.rs`](../../crates/engine-render/src/shaders.rs), CPU mirrors + lockstep tests in
[`psx_light.rs`](../../crates/engine-render/src/psx_light.rs)): each decoded texel word goes
through the exact 5-bit law, each non-neutral packet colour collapses to
`gold · max(r, g, b)` (gold = the staged
[`ColorGrade::PROLOGUE_SEPIA`](../../crates/engine-core/src/fade.rs) coefficients
`(1.0, 0.94, 0.43)`, the measured amber-family ratio), exact-neutral words stay neutral (the
ground tile kernel's runtime word, retail-verified), and the view-depth cue ramp is inert
(no node carries `IR0` in the capture). The op `0x4C 0x12` screen tint rides the palette
uniform's `rgb` so scene fades still multiply every graded pixel.
[`World::scene_color_grade`](../../crates/engine-core/src/world/narration.rs) still owns the
scene gate (the prologue legs `opdeene` / `opstati` / `opurud`, `None` elsewhere);
`play-window` stages the mode whenever the grade is active. With the mode off (every
interactive scene) all shader paths are bit-identical to the multiply-grade render.
Pixel-verified on the villager tableau against a matched-region retail capture of the same
beat: the ground lands **identically** at `G/R 0.890` / `B/R 0.46..0.48` on both sides
(retail `0.890` / `0.471`; the pre-law multiply grade left the engine ground green at
`G/R ≈ 1.07`), and the text/UI overlays keep their own shaders, so the narration stays white.
`scene_color_grade_only_on_the_prologue_cutscene` (engine-core) guards the gate.

The superseded engine approximations are retained as dormant plumbing: `apply_grade`'s pixel
multiply and the [`fade::DepthCueRamp`](../../crates/engine-core/src/fade.rs) view-depth ramp
(`Renderer::set_depth_cue_ramp`) still exist and are staged by the host, but the palette mode
bypasses both while active.

**Far-geometry brightness - not a separable law (resolved-negative).** The far geometry
(spires / wings) reads brighter and slightly blue-rich in the engine - the villager-tableau
matched-region capture measured retail at `B/R ≈ 0.15..0.16` / brightness `~51` vs the engine
at `B/R ≈ 0.27` / `~80`. This is **not** a missing far-field palette or depth law, and there
is no such law to port:

- **No load-time gold-law CPU pass is statically visible.** A signature scan (the law's
  `>>10/11` blue-field extract + `andi 0x1f` + `>>1` reconstruct + a `max`) across the STR
  host overlay 0970 (`overlay_cutscene_str_0970`, 28 functions), the field overlay 0897 (690)
  and `SCUS_942.54` (945) finds **no** CLUT-rewrite loop. Overlay 0970 is MDEC/STR play code
  only - its sole law-shaped shifts are frame-position sign-extract (`>>31`) and `/2` in the
  play loop `FUN_801CF098`; the strong `SCUS` hits are the SFX driver (`setbl`, `DAT_8006F198`)
  and the arts-gauge path (`DAT_801C9370`), neither a palette pass. The **"cutscene-host
  overlay 0970 load hooks are the candidates"** reading is therefore **falsified** - the CLUT
  rewrite the capture observed is a table / DMA upload, not a scannable arithmetic loop (the
  same shape as the XA-clip-table writer under "Open items").
- **The palette grade is faithful; the gap is source colour + region.** With `IR0 = 0` on
  every node (above), no DPCS pull acts on the far prims, and both halves of the grade are
  capture-pinned (CLUT law in VRAM, amber packet in the GP0 list) and reproduced by the
  engine. A far prim drawn with a baked amber packet lands `B/R ≈ 0.44 × 0.43 ≈ 0.19` on both
  sides. The engine's `0.27` excess is un-darkened **neutral** packets in the sampled region:
  lit-descriptor prims (rows 0/1 of `DAT_8007326C`, `byte1 = 0`, no baked colour block) are
  fed neutral `0x80` by the mesh builder (`prim.colors...unwrap_or([128,128,128])` in
  `crates/tmd/src/mesh/{color,vram}.rs`), so `palette_collapse_prim`'s neutral guard leaves
  them un-graded. Retail draws those same lit prims through the scene GTE back/far colour that
  its field renderer `FUN_80029888` loads (opdeene's ambient `DAT_8007B788 = 0x00202020`, dim,
  vs `town01`'s `0x00FFFFFF`; writer `FUN_80043390`) - the field-path GTE colour the engine
  deliberately omits (no field light source). That omission is a scene-wide boundary that only
  *shows* in the prologue because opdeene's ambient is unusually dim, and the port's absence of
  distance culling widens the sampled far region. Both are engine boundaries, not palette-law
  defects; there is no faithful separable palette / depth law to add.

## Field-to-battle transition (the battle-intro overlay)

Not an STR movie and not the 3D opening: the full-screen effect that plays between a field
encounter trigger and the battle scene - the screen shatters / swirls into battle - is its
own overlay. **Source: PROT 0979** (`field_battle_intro`, slot-A base `0x801CE818`),
identified statically by its head strings `efect init` / `battle bgm %d` (`0x801CE854`) /
`brule.xxx` (`0x801CE864`); see
[`static-overlay-pipeline.md`](../tooling/static-overlay-pipeline.md). It shares the
mode-26/27 overlay slot family with the STR player but is a distinct disc entry (own
content `0x4000`; the static footprint over-reads into the dance overlay 0980 past
`+0x4000`).

### Transition tick + battle handoff - `FUN_801CF5BC`

The per-frame driver. A **phase counter** at `actor+0x22` sequences the battle handoff:
phase 1 runs battle-mesh assembly (`FUN_80052770`), phase 2 loads the battle BGM
(`func_0x800567A8("battle bgm %d", id)`) and the battle-scene bundle
(`func_0x8001FC00(0x36F + id, ...)`). A parallel spin/camera timer `actor+0x1a` counts
display frames (`+= DAT_1F800393`) against the total intro duration `DAT_801D2458`: near
the end it raises the ready bits `actor+0x2a |= 1` / `2`, and at completion
(`actor+0x2a == 3`) it writes the game-mode handoff **`_DAT_8007B83C = 0x14`** (enter
battle). Full phase/BGM detail is in the `FUN_801CF5BC` row of
[`functions.md`](../reference/functions.md). `see
ghidra/scripts/funcs/overlay_field_battle_intro_801cf5bc.txt`.

Engine side, this state machine is the encounter session's `Transition` phase: the port
(`legaia_engine_vm::battle_intro_transition::tick_transition`) is driven once per frame by
`legaia_engine_core::World::tick_encounter` for as long as the session sits in that phase,
and its phase-2 `LoadBattleBgm` effect is what starts the battle track - during the spin,
which is where retail starts it, not at battle entry. The remaining effects (mesh
assembly, the bundle read, the load waits) are surfaced on `World::battle_intro_effects`
for a host that owns those reads.

Two switches drive the visuals. A **style selector `DAT_801D2460` (0..=4)** dispatches to
one of five per-frame transition emitters (below); a second switch then applies a per-style
screen fade `func_0x80024EE4(2, blend, level*0x10101)`, the fade `level` ramped from the
`actor+0x1a`-vs-`DAT_801D2458` remaining-time delta (a different slope + threshold per
style). **Dump caveat:** the classifier marks `801cf5bc` **UNCERTAIN** because its
disassembly section stops at `0x801CF8A8` without a `jr ra`. That is a truncated *dump
window*, not a short body - the decompiled C is complete (both switches, the fade and the
`return`), so the function is real and the truncation is the only anomaly.

### Per-style emitters (render-track GTE/GPU)

Each style is a direct GTE/GPU packet emitter: it builds primitives straight into the
ordering-table cursor `_DAT_1F8003A0`, transforms vertices through the GTE (`FUN_80026988`
RotMatrix, `FUN_8005BAC8` RotTransPers-class, `FUN_8003D2C4` / `FUN_8003D344` /
`FUN_8003D1A4` primitive helpers) and screen-clips before linking. The **packet assembly**
stays at the clean-room boundary; the per-record simulation each style runs around it -
seeding, gating and integration - is ported, inert, into `legaia-engine-vm`.

Every style is a **(init, tick)** pair, and the allocation sizes are what pair them:

| `DAT_801D2460` | Init | Tick | Sub-emitter | Working set |
|---:|---|---|---|---|
| 0 | `FUN_801CFBB4` | `FUN_801CFDA0` | - | one `0xDC00` block: 1280 records of `0x2C` |
| 1 | `FUN_801D0164` | `FUN_801D0370` | `FUN_801D1CFC` | the same shape as style 0 |
| 2 | `FUN_801D081C` | `FUN_801D0D24` | `FUN_801D0E54` | `0x908` corner grid + `0x5C00` tile records |
| 3 | - | `FUN_801D11D0` | `FUN_801CF1B0`, `FUN_801D1D9C` | the static descriptor table at `0x801D1EC4` |
| 4 | `FUN_801D1564` | `FUN_801D1888` | `FUN_801D1A20` | `0x100` + `0x6300` + `0x18C0` |

Ports: `battle_intro_particles` (the two seeders), `battle_intro_styles` (styles 0, 1, 3),
`battle_intro_tiles` (style 2), `battle_intro_swirl` (style 4). Any allocation failure bumps
the error counter `_DAT_8007B828` by ten. `FUN_801D1CD4` is an inert stub - it writes a
12-byte local (`0, 0, 0x7D0, 0, 0, 0`) that never escapes, then returns. `see
ghidra/scripts/funcs/overlay_field_battle_intro_<addr>.txt` for each.

#### What the styles actually draw

**Styles 0 and 1** are particle fields. Both walk `0x488` of the seeder's 1280 records - the
last 120 are seeded and never visited - and both read the record the same way: `+0x08..+0x0C`
is a translation integrated by `+0x20..+0x24`, `+0x10..+0x14` a rotation integrated by
`+0x18..+0x1C`, `+0x1E` a spawn delay measured against the entity clock, and `+0x04` a colour
whose **top byte non-zero skips the particle entirely**. That fixes `+0x20..+0x24` as a
velocity rather than as the sprite size and flag word an earlier reading of the seeders
assigned. Style 1 additionally ramps each particle's spin by `1.375x` per frame and decays
its colour by `-0x50505`.

**Style 2** shatters the screen into a `16 x 16` grid of tiles cut from a jittered `17 x 17`
corner lattice (only interior vertices are jittered, so the outline stays a clean rectangle).
A tile record carries eight `SVECTOR` corners - a front face at z `-0x80` and a back face at
z `+0x80` - and packs its angular and linear velocities into **five of those vectors' pad
halfwords** (`+0x1A`, `+0x22`, `+0x2A`, `+0x3A`, `+0x42`). `FUN_801D0E54` doubles the x and y
spin rates every frame, so a tumbling tile accelerates geometrically; since `FUN_801D081C`
writes `sin >> 5` / `cos >> 5` into `+0x1A` / `+0x22` and then immediately zeroes both
(`801d0bac` / `801d0bb0`), only the `DAT_801D2464 == 2` sub-style tumbles at all - the other
two spin about z only.

**Style 3** slices the screen twice. First `0xF0` horizontal strips, each drawn in two halves
(`0xC0 + 0x80 == 0x140`) at `y = (row - 120) * (clock + 28) / 28 + 120` - a vertical stretch
about the screen centre. Then `0x140` vertical strips, each warped the same way horizontally,
culled when the warp pushes them off-screen, and stretched vertically by
`(|col - 160| * clock) >> 5`. Both passes **patch the shared descriptor record in place**
before every `FUN_801CF1B0` call rather than carrying per-strip records.

**Style 4** is a radial fan. Each of 16 bands samples the trig tables at stride `0x80` - one
entry every 64 units of a 4096-entry table, so `0x21` columns span exactly half a turn - and
the other half is written as an x-negated mirror, which is why a band is `2 * 99` vertices
rather than `65 * 3`. A band carries an inner radius `4 + b * 0x10` and an outer
`0x14 + b * 0x10`; the products are clamped to `+-0xA00` (x) and `+-0x760` (y), so the outer
bands stop being circular and become the screen rectangle. Alternating bands get opposite
rotation rates, which is what makes the rings counter-rotate.

## Script-cutscene helpers (`overlay_cutscene_dialogue`)

The actor-scripted dialogue / scene sequences (the `op*` / `ed*` CDNAME scenes) run in a
separate overlay from the STR/MDEC player - the one that shares the town field-VM binary
(noted at the top of this page). These are per-frame effect steps in that overlay, driven
from the scene's actor records. The four below are byte-identical to the
`overlay_cutscene_mapview` capture (and `FUN_801D27E0` also to the world-map overlay), so
they are shared scripted-scene machinery rather than dialogue-only code.

| Address | Role |
|---|---|
| `FUN_801D27E0` | **party-leader swap** SM (6 states, actor `+0x54`). See below - it *changes* `DAT_80084597`, it does not merely focus on it. |
| `FUN_801D5C08` | per-frame position tween step: accumulates `+0x9c += (+0x9e) * DAT_1F800393`, lerps between the start (`+0x14`) and end (`+0x24`) vectors by `t = +0x9c / 0x1000` via `FUN_801E45BC`, writes the result onto the linked object (`+0x90`), and snaps to the end + sets done bit `8` at `t >= 0x1000` |
| `FUN_801D5D60` | scripted-element teardown: restores the camera (`FUN_801DB510` + `FUN_801DAA50`) when armed, and once the linked object's done bit `8` is set clears the enable flags (mask `~(+0x74)`) on the `+0x94` and camera objects |
| `FUN_801D6058` | ambient particle emitter (gated on `_DAT_8007B854`, optional `fog_set` trace): with actor `+0x1a == 0` occasionally spawns one particle at the actor position + random jitter via `FUN_801D629C`; otherwise loops 0x18 times spawning random bursts across the scene bounds (`DAT_1F8003E8..EB`) |

`see ghidra/scripts/funcs/overlay_cutscene_dialogue_<addr>.txt` for each.

### `FUN_801D27E0` swaps the party leader

The earlier reading of this state machine as a "scripted camera focus" is
**falsified** by its own state `2`. It does snap the camera onto the party
actor `DAT_80084597` - but only after writing a *new* index there.

Story flags `0x10`, `0x11` and `0x12` are the leader encoding: `801d2b04`..
`801d2b1c` clears all three (`func_0x8003CE34`) and sets `0x10 + new`
(`func_0x8003CE08`), while `801d2aec`..`801d2b08` writes the same index to
`_DAT_8007B8F8`, `DAT_80084597` and `DAT_80084598`. The new index is found by
stepping forward from the current one, wrapping at three, until a presence flag
`ctx[+0x50] + n` reads **clear**.

Around that, the six states are: cache the three party actors' `x/y/z/facing`
into the `0x800845E4` table and run the arm gate (`0`); hold `0x20` frames for
the fade-out (`1`); perform the swap, re-anchor the camera and field grid on
the incoming leader, and spawn the fade-in (`2`); release the fade object
(`3`); hold `0x20` frames and clear the camera's busy bit `0x80000` (`4`);
retire (`5`). The arm gate refuses when all three presence flags are set - a
full party has nothing to swap to - and when only two are set it additionally
requires the leader's own flag. Both fades are `crate::fade`-shaped templates:
kind `2`, `0x20` frames, black-to-white then white-to-black, so the swap happens
behind a white flash.

Port: `legaia_engine_core::cutscene_script_elements::LeaderSwap`.

### `FUN_801D5E20` rotates a mesh's own colour words

`FUN_801D8280` walks the resident-object table `DAT_8007C018` and calls
`FUN_801D5E20` on each object's primitive block. The routine is an HSV colour
grade applied **destructively to the TMD's packed colour words**: for every
primitive it converts each colour to HSV (`func_0x8001A78C`), adds the caller's
`(dh, ds, dv)`, and converts back (`func_0x8001A6C8`).

Two details matter to anyone reproducing it. The hue folds modulo **`0x167`**
(359), not 360, so a full turn of the shift walks the palette one step. And how
many colour words a primitive carries comes from a table at `0x801F26F0`
indexed by `group.flags >> 1` - a *different* selector from the TMD renderer's
own per-mode table at `DAT_8007326C`, which uses `((flags >> 1) - 8) >> 1`.

The cursor advance is also worth pinning: the `ilen * 4` stride add runs once
per primitive **and** once more after the group's loop (`801d5FF8` inside,
`801d6010` after), and the `count == 0` arm jumps straight to the trailing add.
Against the `count x ilen*4` body [`tmd.md`](../formats/tmd.md) documents, that
over-runs by one primitive per group.

Port: `legaia_engine_core::cutscene_script_elements::shift_primitive_colours`.

### What the tween and the emitter do beyond the one-line role

Three details of these bodies are only visible in the disassembly, and the
engine port (`legaia_engine_core::cutscene_script_elements`) carries all
three.

The tween's entry test is on the **linked** object, not on itself: `0x801D5C1C`
loads `linked[+0x10]` and branches on bit `8`, and the taken branch lands on
the store that sets the *element's own* bit `8`. So an element whose target has
already finished retires itself without writing a position. Every position
write is also accompanied by a facing/sort write - `linked[+0x8E] = -y` of the
value just applied - and the sole exception is the camera object, compared by
pointer identity against `_DAT_8007C364` (`0x801D5CA8` and `0x801D5D38`). Both
the blend arm and the snap arm do it.

The emitter picks between two parameter pairs on `_DAT_1F800394 & 1`
(`0x801D60A0`): bit clear leaves `(2, 1)`, bit set replaces them with
`(6, 0x0E)`. The first is added to the scene's Y span before it is halved, the
second is subtracted from each particle's Y offset - so the bit widens the
band and shifts it. Only the scene-wide burst arm reads them.

Finally, the decompiled C of the burst count reads `if ((uVar1 & 3) !=
0xffffffff)`, which is a decompiler artifact: the disassembly is `addiu
s2,v0,0x1` then `beq s2,zero`, a zero test on a value that is always `1..=4`.
Every burst that passes the one-in-sixteen gate spawns at least one particle.

## Open items

- **XA channel map - resolved.** There is no channel selector in the STR overlay: FMVs play with the sector filter off (each movie carries one `(1, 0)` track), and the `XA*.XA` clip path selects `CdlSetfilter {file 1, chan}` per cue in SCUS (`FUN_8003D764`), with `clip_id -> XA<n>.XA` via the table at `0x801C6ED8`. See [XA channel selection](#xa-channel-selection). The earlier "`\DATA\MOV.STR` multi-channel container drives it" hypothesis is falsified (dev leftover, not on the disc).
- **Frame-demux SM + master dispatch - statically decompiled.** `FUN_801CEA3C` (dispatch + return-scene hand-off), `FUN_801CF098` (play loop), the SCUS St-library demuxer `FUN_8005ECD4`/`FUN_8005F024`, and the two bitstream decoders are decoded; see [Retail playback engine](#retail-playback-engine-str-overlay--scus-st-streaming-library). The dispatch-table stride is 32 bytes (nine retail slots) - the superseded 64-byte reading is corrected in [`str-fmv-table.md`](../formats/str-fmv-table.md).
- **XA clip-table writer + cue census.** The `0x801C6ED8` clip-table *content* is pinned (34 slots = `XA1..XA34`, `[CdlLOC][byte len]`), but its filler is a DMA/computed-pointer write no static addressing-form scan sees (both `lui 0x801c`-materialised sites in SCUS are the readers `FUN_8003D53C`/`FUN_8003EAE4`). Which game systems fire which `(clip_id, chan)` cues beyond the menu voice dispatcher `FUN_8004FCC8` is a per-caller census, still open.
- **MOV15.STR + MV1A.STR.** Two extra path strings (`\DATA\MOV15.STR;1` and `\MOV\MV1A.STR;1`) appear alongside the six numbered MVs, dispatched by dev slots 9/10 (the only slots selecting the STRv2/v3 decoder and non-default VRAM rects): `MOV15` is the 15-FPS test file (referenced by the `psx.cdspeedup` / 15 fps debug paths), and `MV1A` is an alternate / cut version of MV1. Neither ships in the released disc layout.
- **8-bit ADPCM.** `coding_info` width detection drives a real 8-bit group decoder (`BitsPerSample::Eight`: 4 units/group, full-byte samples). No 8-bit audio has been observed in the corpus, so the path is covered by synthetic unit tests rather than a bit-exact reference.

## Provenance

| Subject | Source |
|---|---|
| Master dispatch + return-scene hand-off | `FUN_801CEA3C`; `see ghidra/scripts/funcs/overlay_cutscene_str_0970_801cea3c.txt` |
| Play loop | `FUN_801CF098`; `see ghidra/scripts/funcs/str0970_801cf098.txt` |
| Frame-demux SM (St library) | `FUN_8005ECD4` / `FUN_8005F024`; `see ghidra/scripts/funcs/8005f024.txt` |
| StGetNext frame poll + end latch + display width | `FUN_801CF740`; `see ghidra/scripts/funcs/overlay_str_fmv_0x801CF740.txt` |
| MDEC decode watchdog / reset / DMA-out thunk | `FUN_801CFAD4` / `FUN_801CFC18` / `FUN_801CFE00`; `see ghidra/scripts/funcs/overlay_str_fmv_0x801CFAD4.txt` |
| Field->battle transition SM + style dispatch | `FUN_801CF5BC` (PROT 0979 `field_battle_intro`); `see ghidra/scripts/funcs/overlay_field_battle_intro_801cf5bc.txt` |
| Field->battle per-style GTE/GPU emitters | `FUN_801CFDA0` / `FUN_801D0370` / `FUN_801D0D24` / `FUN_801D11D0` / `FUN_801D1888` + helpers `FUN_801CF1B0` / `FUN_801D0E54` / `FUN_801D0164` / `FUN_801D1564`; PROT 0979 `field_battle_intro` |
| Script-cutscene camera / tween / particle steps | `FUN_801D27E0` / `FUN_801D5C08` / `FUN_801D5D60` / `FUN_801D6058`; `see ghidra/scripts/funcs/overlay_cutscene_dialogue_<addr>.txt` |
| Iki / STRv2 bitstream decoders | `FUN_801D0378` / `FUN_801D070C` (+ LZSS `FUN_801D0604`); `see ghidra/scripts/funcs/overlay_str_fmv_0x801D0378.txt` |
| XA-clip channel selector (`CdlSetfilter`) | `FUN_8003D53C` / `FUN_8003D764`; `see ghidra/scripts/funcs/8003d764.txt` |
| Per-movie XA `(file 1, chan 0)` single track | raw-sector subheader scan of all six `MOV/MV*.STR` on the disc |
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
