//! The retail STR play loop, ported from the STR/MDEC overlay (PROT 0970,
//! base `0x801CE818`).
//!
//! This is the layer between [`crate::st_ring::StRing`] (the sector demuxer)
//! and [`crate::MdecDecoder`] (the bitstream decoder): the piece that decides
//! *which* sectors of a movie belong to an FMV, when a frame is ready, which of
//! the two decode buffers it lands in, and when playback is over.
//!
//! Ported functions:
//!
//! | Retail | Here |
//! |---|---|
//! | `FUN_801CF098` play loop | [`StrPlayer`] + [`seek_sector_offset`] + [`vram_units`] + [`StrPlayer::display_rect`] |
//! | `FUN_801CF8B0` decode-env init | [`DecodeEnv::init`] |
//! | `FUN_801CF988` ring + stream setup | [`StrPlayer::open`] |
//! | `FUN_801CFA14` frame pump | [`StrPlayer::next_frame`] |
//! | `FUN_801CF740` frame poll | [`end_of_stream`] + [`DecodeEnv::apply_frame_dimensions`] |
//! | `FUN_801CFD84` MDEC output control word | [`mdec_output_control`] |
//! | `FUN_801CFEBC` slice-callback (un)install | [`DecodeEnv::set_slice_callback`] |
//! | `FUN_801CF56C` MDEC-out slice callback | [`DecodeEnv::advance_slice`] |
//!
//! ## The dispatch slot is eight `u32`s and the play loop reads all of them
//!
//! `FUN_801CF098` takes `(wide_flag, &slot)` where `slot` is one 32-byte record
//! of the FMV dispatch table at `0x801D0A6C`. Every word is consumed, and each
//! one is consumed *by the play loop* - there is no field the table carries for
//! some other subsystem:
//!
//! | Offset | Field | Where the play loop reads it |
//! |---|---|---|
//! | `+0x00` | path pointer | `801cf0d0` `CdSearchFile` |
//! | `+0x04` | 24-bit colour flag | `801cf100`, `801cf27c`, `801cf2f4`, `801cf478` - the `* 3/2` VRAM scaling, `DISPENV.isrgb24`, and the MDEC depth bit |
//! | `+0x08` | start frame | `801cf1b8` seek `(start - 1) * 10` sectors; `801cf9e0` `StSetStream` |
//! | `+0x0C` | end frame | `801cf788` (in `FUN_801CF740`) latches end-of-stream when the demuxed frame number reaches it |
//! | `+0x10` | `fb_x` | `801cf110` / `801cf144` - the decode rect's VRAM x |
//! | `+0x14` | `fb_y` | `801cf168` / `801cf180` - the decode rect's VRAM y |
//! | `+0x18` | width | `801cf28c` - the `DISPENV` width |
//! | `+0x1C` | height | `801cf168` (second buffer at `fb_y + height`), `801cf2c8` (display rect `h = height * 2`) |
//!
//! `legaia_asset::fmv_dispatch::FmvEntry` keeps six of the eight (it drops
//! `fb_x` / `fb_y`); this module is where those two get their meaning. The
//! record is playback-only, and there are no undecoded bytes left in it.
//!
//! ## Double buffering runs at two independent levels
//!
//! Retail keeps *two* separate ping-pongs, and conflating them is the easy
//! mistake:
//!
//! - the **MDEC code buffers** (`ctx+0x00` / `ctx+0x04`), toggled once per
//!   *frame* by `FUN_801CFA14`, hold the VLC-decoded macroblock command list;
//! - the **frame rects** (`ctx+0x18` / `ctx+0x20`), toggled once per *frame
//!   buffer worth of slices* by `FUN_801CF56C`, are the two VRAM rects at
//!   `(fb_x, fb_y)` and `(fb_x, fb_y + height)` that the picture alternates
//!   between so the displayed frame is never the one being written.
//!
//! A third ping-pong (`ctx+0x0C` / `ctx+0x10`, toggled per *slice*) stages the
//! MDEC output of one 16-pixel-wide column while the previous column's
//! `LoadImage` is still in flight.
//!
//! See [`docs/subsystems/cutscene.md`](../../../../docs/subsystems/cutscene.md).

use crate::st_ring::{StFrame, StRing, StStep};

/// Sectors per STR video frame - the fixed 15 fps cadence. `FUN_801CF098`
/// computes the seek as `(start_frame - 1) * 10` at `801cf1b8..801cf1cc`
/// (`v0 * 4 + v0`, then `<< 1`).
pub const SECTORS_PER_FRAME: i32 = 10;

/// Ring slots `FUN_801CF988` hands `StSetRing` (`addiu a1,zero,0x20`).
pub const RING_SLOTS: usize = 0x20;

/// `end_frame` literal `FUN_801CF988` passes to `StSetStream`
/// (`addiu a2,zero,-1` at `801cf9d8`). The library-level end-frame stop is
/// therefore inert in retail; the segment end is enforced one level up, by
/// [`StrPlayer::next_frame`] comparing against the slot's `+0x0C`.
pub const ST_SET_STREAM_END_FRAME: u32 = u32::MAX;

/// Spins `FUN_801CFA14` gives the frame poll before giving up and returning
/// `-1` (`addiu s0,zero,0x7d0` at `801cfa2c`). The caller then re-seeks through
/// the timeout handler `FUN_801CFB94`.
pub const FRAME_POLL_SPINS: u32 = 2000;

/// Pad mask the play loop tests to abort the intro
/// (`andi v0,v0,0x1f0` at `801cf4fc`, against `_DAT_8007B850`).
pub const SKIP_PAD_MASK: u32 = 0x1F0;

/// MDEC command-word bit `FUN_801CFD84` drives from flag bit 0: **clear** for
/// 24-bit output, **set** for 15-bit.
pub const MDEC_DEPTH_15BIT: u32 = 0x0800_0000;

/// MDEC command-word bit `FUN_801CFD84` drives from flag bit 1: signed
/// (zero-centred) output samples. The play loop always sets it, which is why
/// [`crate::MdecDecoder`] offsets luma by `+128` on the way to RGB.
pub const MDEC_SIGNED_OUTPUT: u32 = 0x0200_0000;

/// VRAM cells one 16-pixel-wide macroblock column occupies, 24-bit output
/// (`addiu v1,zero,0x18` at `801cf954`).
pub const SLICE_W_24BPP: i16 = 0x18;

/// VRAM cells one 16-pixel-wide macroblock column occupies, 15/16-bit output
/// (`addiu v1,zero,0x10` at `801cf950`).
pub const SLICE_W_16BPP: i16 = 0x10;

/// A PSX `RECT` - the shape `ctx+0x18`, `ctx+0x20` and `ctx+0x2C` all hold.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rect {
    /// VRAM x, in 16-bit cells.
    pub x: i16,
    /// VRAM y, in scanlines.
    pub y: i16,
    /// Width in 16-bit cells.
    pub w: i16,
    /// Height in scanlines.
    pub h: i16,
}

/// One 32-byte FMV dispatch slot, in the play loop's own terms.
///
/// Field-for-field the record documented in
/// `legaia_asset::fmv_dispatch`; kept here so `legaia-mdec` can drive a
/// segment without depending on the asset crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FmvSlot {
    /// `+0x04` - non-zero selects 24-bit colour output.
    pub colour: bool,
    /// `+0x08` - 1-based first frame of the segment.
    pub start_frame: u32,
    /// `+0x0C` - last frame of the segment (inclusive; it is decoded).
    pub end_frame: u32,
    /// `+0x10` - decode-rect x in *pixels* (scaled to VRAM cells for 24-bit).
    pub fb_x: u32,
    /// `+0x14` - decode-rect y in scanlines.
    pub fb_y: u32,
    /// `+0x18` - frame width in pixels.
    pub width: u32,
    /// `+0x1C` - frame height in scanlines.
    pub height: u32,
}

impl FmvSlot {
    /// Decode one 32-byte dispatch record. The `+0x00` path pointer is skipped -
    /// it is an overlay VA, not something this crate can resolve.
    pub fn from_record(rec: &[u8; 0x20]) -> Self {
        let w = |i: usize| u32::from_le_bytes(rec[i * 4..i * 4 + 4].try_into().unwrap());
        Self {
            colour: w(1) != 0,
            start_frame: w(2),
            end_frame: w(3),
            fb_x: w(4),
            fb_y: w(5),
            width: w(6),
            height: w(7),
        }
    }

    /// A whole-file window: play from the first frame that arrives to EOF, at
    /// `width`×`height`, 24-bit, decoding to the top-left of VRAM.
    pub fn whole_file(width: u32, height: u32) -> Self {
        Self {
            colour: true,
            start_frame: 0,
            end_frame: 0,
            fb_x: 0,
            fb_y: 0,
            width,
            height,
        }
    }
}

/// Convert a pixel count to 16-bit VRAM cells.
///
/// `FUN_801CF098` open-codes this four times (`801cf118`, `801cf14c`,
/// `801cf294`, `801cf3fc`): `(px * 3) / 2` truncating toward zero for 24-bit
/// output, identity otherwise. Three bytes per pixel over a two-byte cell.
// PORT: FUN_801cf098
pub fn vram_units(px: i32, colour: bool) -> i32 {
    if colour { (px * 3) / 2 } else { px }
}

/// Sector offset from the start of the movie file to `start_frame`.
///
/// `(start_frame - 1) * 10` - the seek `FUN_801CF098` adds to the file's start
/// LBA at `801cf1b8` before `CdControl(CdlSetloc)`. It is what lets `MV3.STR`
/// carry four cutscenes as abutting frame ranges.
// PORT: FUN_801cf098
pub fn seek_sector_offset(start_frame: u32) -> i32 {
    (start_frame as i32 - 1) * SECTORS_PER_FRAME
}

/// Whether a demuxed frame is the last of a segment - the end-of-stream latch
/// `FUN_801CF740` raises at `801cf794`..`801cf7a8`.
///
/// Retail's test is `slot.end_frame <= frame.frame_number`, **unsigned and
/// unguarded**: it compares the sector header's `+0x08` frame number against
/// the dispatch slot's `+0x0C` and stores `1` into `DAT_801E09F8`, the word the
/// play loop's exit test reads. There is no zero check, because every one of
/// the nine retail slots carries a real end frame.
///
/// The `end_frame != 0` guard here is the port's own, and it exists for
/// [`FmvSlot::whole_file`] - an engine-invented slot with no end frame, which
/// under retail's bare comparison would latch on its very first frame.
// PORT: FUN_801cf740
pub fn end_of_stream(frame_number: u32, end_frame: u32) -> bool {
    end_frame != 0 && frame_number >= end_frame
}

/// Apply `FUN_801CFD84` to an MDEC command word.
///
/// Bit 0 of `flags` selects the output depth and bit 1 the signedness:
///
/// - `flags & 1` **clears** [`MDEC_DEPTH_15BIT`] (24-bit output), else sets it;
/// - `flags & 2` **sets** [`MDEC_SIGNED_OUTPUT`], else clears it.
///
/// The one call site (`801cf2ec`..`801cf310`) passes `3` for a colour slot and
/// `2` otherwise - so signed output is unconditional and the depth bit is
/// exactly the slot's `+0x04` colour flag inverted. Retail then hands the
/// updated word's low half to the DMA-0 code upload `FUN_801CFFDC`.
// PORT: FUN_801cfd84
// NOT WIRED: the GPU-presentation half of the play loop has no consumer. The
// `mdec` CLI and the engine's `play-str` both drive the demux + frame-pump
// half of this module (`StrPlayer::open` / `deliver_sector` / `next_frame`)
// and then decode a whole frame to RGBA through `MdecDecoder::decode_frame`,
// handing the pixels to a texture upload. Neither ever programs the MDEC
// depth/sign bits, because neither writes MDEC hardware registers at all.
// A caller needs a VRAM-resident STR present path - one that decodes into the
// shared `PsxVram` rather than into an RGBA buffer - to exist first.
pub fn mdec_output_control(word: u32, flags: u32) -> u32 {
    let mut w = word;
    if flags & 1 != 0 {
        w &= !MDEC_DEPTH_15BIT;
    } else {
        w |= MDEC_DEPTH_15BIT;
    }
    if flags & 2 != 0 {
        w |= MDEC_SIGNED_OUTPUT;
    } else {
        w &= !MDEC_SIGNED_OUTPUT;
    }
    w
}

/// The `flags` argument the play loop passes [`mdec_output_control`] for a slot
/// (`addiu a1,zero,2` / `addiu a1,zero,3` at `801cf2ec`/`801cf304`).
// PORT: FUN_801cf098
// NOT WIRED: the argument-side companion to `mdec_output_control`, and inert
// for the same reason - no caller programs the MDEC output word, so nothing
// needs the flags to pass it.
pub fn mdec_control_flags(colour: bool) -> u32 {
    if colour { 3 } else { 2 }
}

/// 32-bit words in one macroblock-column slice: `(slice_w * 16) * ceil(rows/16)
/// / 2`.
///
/// `FUN_801CF56C` computes it at `801cf6a0`..`801cf6d8`: `((rows - 1) / 16 + 1)`
/// bands (the `+14`-before-`sra` is the truncating-division idiom for a
/// negative numerator), times `slice_w << 4` halfwords, then `>> 1` to words.
// PORT: FUN_801cf56c
// NOT WIRED: sizes a DMA-0 transfer of one macroblock column out of the MDEC
// into VRAM. Its only non-test caller is `DecodeEnv::advance_slice`, which is
// itself inert - see the tag there.
pub fn slice_word_count(slice_w: i16, rows: i16) -> i32 {
    let bands = ((rows as i32 - 1) / 16) + 1;
    (((slice_w as i32) << 4) * bands) >> 1
}

/// What one MDEC-out slice callback did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliceStep {
    /// The rect to `LoadImage` into - the slice rect **as it stood on entry**,
    /// before the cursor advanced (retail copies it at `801cf5b0` with
    /// `lwl`/`lwr` before touching anything).
    pub load_rect: Rect,
    /// Which of the two staging buffers (`ctx+0x0C` / `ctx+0x10`) holds the
    /// pixels for `load_rect`.
    pub load_buffer: usize,
    /// Words to kick into the next MDEC-out DMA, or `None` when this slice
    /// finished the frame buffer and the rects flipped instead.
    pub kick_words: Option<i32>,
    /// The frame buffer completed and [`DecodeEnv::active_buf`] flipped.
    pub flipped: bool,
}

/// The STR decode context - retail's struct at `0x801D19A0`.
///
/// Field names track the offsets: the two frame rects at `+0x18`/`+0x20`, the
/// live slice rect at `+0x2C`, the three toggles at `+0x08`/`+0x14`/`+0x28`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeEnv {
    /// `ctx+0x18` / `ctx+0x20` - the two VRAM frame rects.
    pub frame_buf: [Rect; 2],
    /// `ctx+0x2C` - the slice rect the callback walks across a frame buffer.
    pub slice: Rect,
    /// `ctx+0x28` - which frame rect is being written.
    pub active_buf: usize,
    /// `ctx+0x08` - which MDEC code buffer the next frame decodes into.
    pub code_buf: usize,
    /// `ctx+0x14` - which slice staging buffer the next callback stages into.
    pub slice_buf: usize,
    /// `DAT_801E09F0` - the next slice is the first of a frame buffer, so the
    /// cursor takes the leading partial-column step.
    pub first_slice: bool,
    /// `ctx+0x34` - a frame buffer completed since the flag was last cleared.
    pub buffer_complete: bool,
    /// `ctx+0x38` - the slot's 24-bit colour flag.
    pub colour: bool,
    /// Whether `DecDCToutCallback` currently points at the slice handler.
    slice_callback_armed: bool,
}

impl DecodeEnv {
    /// `FUN_801CF8B0` - build the decode context for a dispatch slot.
    ///
    /// The two frame rects are `(fb_x, fb_y)` and `(fb_x, fb_y + height)`, with
    /// `fb_x` converted to VRAM cells; the slice rect starts on the active
    /// buffer's origin and is [`SLICE_W_24BPP`] or [`SLICE_W_16BPP`] wide by
    /// `height` tall.
    ///
    /// Retail keeps this behind a one-shot latch (`DAT_801D0D4C`, cleared at
    /// `801cf930`) so a second FMV in the same overlay residency reuses the
    /// buffer pointers; the port has no globals to preserve, so it always
    /// builds a complete context.
    // PORT: FUN_801cf8b0
    pub fn init(slot: &FmvSlot) -> Self {
        let x = vram_units(slot.fb_x as i32, slot.colour) as i16;
        let y0 = slot.fb_y as i16;
        let y1 = (slot.fb_y + slot.height) as i16;
        let w = vram_units(slot.width as i32, slot.colour) as i16;
        let h = slot.height as i16;
        let slice_w = if slot.colour {
            SLICE_W_24BPP
        } else {
            SLICE_W_16BPP
        };
        let frame_buf = [Rect { x, y: y0, w, h }, Rect { x, y: y1, w, h }];
        Self {
            frame_buf,
            slice: Rect {
                x,
                y: y0,
                w: slice_w,
                h,
            },
            active_buf: 0,
            code_buf: 0,
            slice_buf: 0,
            first_slice: true,
            buffer_complete: false,
            colour: slot.colour,
            slice_callback_armed: false,
        }
    }

    /// Re-size both frame rects from the dimensions the **sector header**
    /// declares - the second half of `FUN_801CF740` (`801cf7ac`..`801cf894`).
    ///
    /// The frame poll caches the header's `+0x10` width and `+0x12` height in
    /// `DAT_801D0D50` / `DAT_801D0D54` and, every frame, programs them into
    /// five halfwords of the decode context:
    ///
    /// | context word | written |
    /// |---|---|
    /// | `+0x1C` / `+0x24` | width, put through the same `* 3 / 2` 24-bit scale as [`vram_units`] |
    /// | `+0x1E` / `+0x26` | height, unscaled |
    /// | `+0x32` | height, unscaled - the slice rect's height |
    ///
    /// The slice rect's **width** at `+0x30` is deliberately not touched: it is
    /// the fixed macroblock-column stride [`SLICE_W_24BPP`] /
    /// [`SLICE_W_16BPP`] that `FUN_801CF8B0` set.
    ///
    /// So the decode geometry follows the bitstream, not the dispatch table:
    /// the slot's `+0x18` / `+0x1C` only seed [`DecodeEnv::init`], and any
    /// disagreement between the table and the movie is resolved in the movie's
    /// favour from the first frame onward.
    ///
    /// When the cached pair *changes*, retail additionally fills a stack `RECT`
    /// of `(0, 0, vram_units(slot.width), slot.height * 2)`. Nothing in the
    /// printed disassembly consumes it - there is no call between the stores
    /// and the return - so it is not reproduced here.
    // PORT: FUN_801cf740
    // NOT WIRED: needs the per-frame sector-header dimensions, and
    // `crate::st_ring::StFrame` does not carry them - `StRing` parses the
    // header's `+0x10` / `+0x12` and drops them, keeping only the frame number
    // and the bitstream length. `StrPlayer::next_frame` therefore has nothing
    // to pass. Wiring it is one field pair on `StFrame` plus the `deliver_sector`
    // capture; until then the rects keep `DecodeEnv::init`'s slot-derived size.
    pub fn apply_frame_dimensions(&mut self, width: u16, height: u16) {
        let w = vram_units(i32::from(width), self.colour) as i16;
        let h = height as i16;
        self.frame_buf[0].w = w;
        self.frame_buf[0].h = h;
        self.frame_buf[1].w = w;
        self.frame_buf[1].h = h;
        self.slice.h = h;
    }

    /// `FUN_801CFEBC` - `DecDCToutCallback(1, handler)`.
    ///
    /// The play loop arms it through `FUN_801CF988` before the first read
    /// (`801cf9c4`) and clears it with a null handler at teardown
    /// (`801cf524`, `move a0,zero`). While it is clear, MDEC-out completions do
    /// not advance the slice cursor - which is exactly what
    /// [`DecodeEnv::advance_slice`] reports by returning `None`.
    // PORT: FUN_801cfebc
    pub fn set_slice_callback(&mut self, armed: bool) {
        self.slice_callback_armed = armed;
    }

    /// Whether the slice callback is currently installed.
    pub fn slice_callback_armed(&self) -> bool {
        self.slice_callback_armed
    }

    /// `FUN_801CF56C` - one MDEC-out slice completion.
    ///
    /// Advances the slice cursor one macroblock column to the right; when the
    /// cursor passes the active frame rect's right edge the two frame rects
    /// flip and the cursor restarts on the new buffer's origin. Returns the
    /// `LoadImage` the callback issues on the way out, or `None` if the
    /// callback is not installed.
    // PORT: FUN_801cf56c
    // NOT WIRED: retail decodes a frame incrementally, one macroblock column
    // at a time, and this is the interrupt callback that walks the cursor and
    // issues each column's `LoadImage`. The port decodes a frame whole
    // (`MdecDecoder::decode_frame` takes the complete bitstream and returns
    // RGBA), so there are no per-slice completions to service. A slice-wise
    // decoder driving `PsxVram` would have to exist before this has a caller.
    pub fn advance_slice(&mut self) -> Option<SliceStep> {
        if !self.slice_callback_armed {
            return None;
        }
        // Retail copies the rect and reads the staging index *before* it
        // touches either, so the LoadImage describes the slice that just
        // finished, not the one about to start.
        let load_rect = self.slice;
        let load_buffer = self.slice_buf;
        self.slice_buf ^= 1;

        // Leading partial column: when the buffer width isn't a whole number of
        // slices, the first step is the remainder, so the *last* slice of the
        // row lands flush on the right edge.
        let remainder = if self.slice.w == 0 {
            0
        } else {
            self.frame_buf[self.active_buf].w % self.slice.w
        };
        if self.first_slice && remainder != 0 {
            self.first_slice = false;
            self.slice.x += remainder;
        } else {
            self.slice.x += self.slice.w;
        }

        let active = self.frame_buf[self.active_buf];
        let (kick_words, flipped) = if self.slice.x < active.x + active.w {
            (Some(slice_word_count(self.slice.w, self.slice.h)), false)
        } else {
            self.active_buf ^= 1;
            self.buffer_complete = true;
            self.slice.x = self.frame_buf[self.active_buf].x;
            self.slice.y = self.frame_buf[self.active_buf].y;
            self.first_slice = true;
            (None, true)
        };
        Some(SliceStep {
            load_rect,
            load_buffer,
            kick_words,
            flipped,
        })
    }
}

/// Which bitstream decoder the play loop dispatches a frame to
/// (`DAT_801E09FC`, tested at `801cfa70`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bitstream {
    /// `FUN_801D0378` - the Legaia "Iki" decoder. Every retail movie.
    Iki,
    /// `FUN_801D070C` - standard STRv2/v3 VLC against the table
    /// `FUN_801F1A00` unpacks to `0x801E0A00`. Dev slots 9/10 only, whose
    /// files are not on the released disc. Decoder:
    /// [`crate::strv2_decode::decode_frame`].
    Strv2,
}

/// A frame the pump handed over for decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PumpedFrame {
    /// Demuxed bitstream, truncated to the sector header's `frame_size_bytes`.
    pub bitstream: Vec<u8>,
    /// Sequence number from the frame's first sector.
    pub frame_number: u32,
    /// MDEC code buffer this frame decodes into (`ctx+0x08` after the toggle).
    pub code_buf: usize,
    /// Which decoder the frame goes to.
    pub bitstream_kind: Bitstream,
    /// This frame reached the slot's `end_frame`, so the play loop will exit
    /// after displaying it.
    pub is_last: bool,
}

/// Why [`StrPlayer::next_frame`] had nothing to hand over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PumpIdle {
    /// No complete frame in the ring yet - feed more sectors. Retail spins
    /// [`FRAME_POLL_SPINS`] times here before re-seeking.
    NeedSectors,
    /// Playback is over: the end frame was reached, or the caller aborted.
    Finished,
}

/// The retail STR play loop over a [`StRing`] and an [`crate::MdecDecoder`].
///
/// Feed whole 2048-byte sector data areas with [`StrPlayer::deliver_sector`]
/// and drain frames with [`StrPlayer::next_frame`].
pub struct StrPlayer {
    ring: StRing,
    slot: FmvSlot,
    env: DecodeEnv,
    bitstream: Bitstream,
    end_latch: bool,
    aborted: bool,
}

impl StrPlayer {
    /// `FUN_801CF988` - prime the ring and open the stream on a dispatch slot.
    ///
    /// `StSetRing(buffer, 0x20)` then
    /// `StSetStream(colour_flag, slot.start_frame, -1, 0, 0)`, then arm the
    /// MDEC-out slice callback (`FUN_801CFEBC`). The `-1` end frame is retail's
    /// own literal: the library-level stop is unused, and the segment end is
    /// enforced by [`StrPlayer::next_frame`] against `slot.end_frame`.
    // PORT: FUN_801cf988
    pub fn open(slot: FmvSlot, bitstream: Bitstream) -> Self {
        let mut ring = StRing::set_ring(RING_SLOTS);
        ring.set_stream(
            slot.colour as u32,
            slot.start_frame,
            ST_SET_STREAM_END_FRAME,
        );
        let mut env = DecodeEnv::init(&slot);
        env.set_slice_callback(true);
        Self {
            ring,
            slot,
            env,
            bitstream,
            end_latch: false,
            aborted: false,
        }
    }

    /// The slot this player is running.
    pub fn slot(&self) -> &FmvSlot {
        &self.slot
    }

    /// The decode context.
    pub fn env(&self) -> &DecodeEnv {
        &self.env
    }

    /// Mutable decode context - the MDEC-out callback side.
    pub fn env_mut(&mut self) -> &mut DecodeEnv {
        &mut self.env
    }

    /// The underlying sector ring.
    pub fn ring(&self) -> &StRing {
        &self.ring
    }

    /// Sector offset the play loop seeks to before the first read.
    pub fn seek_sector_offset(&self) -> i32 {
        seek_sector_offset(self.slot.start_frame)
    }

    /// The MDEC command word for this slot's output depth, applied to `word`.
    pub fn mdec_control(&self, word: u32) -> u32 {
        mdec_output_control(word, mdec_control_flags(self.slot.colour))
    }

    /// `SetDefDispEnv` rect for the buffer *not* currently being decoded into -
    /// what `FUN_801CF098` puts on screen at `801cf3d4`..`801cf474`.
    ///
    /// x is always zero (the displayed buffer's VRAM x equals the decode x, and
    /// retail subtracts one from the other); y is `0` or `height` depending on
    /// which rect is live.
    // PORT: FUN_801cf098
    // NOT WIRED: describes a `SetDefDispEnv` over the *other* half of a
    // double-buffered PSX framebuffer. The port presents a decoded frame as a
    // texture and lets the swapchain handle buffering, so there is no second
    // VRAM buffer for this rect to name. It becomes callable once the STR path
    // decodes into `PsxVram` and presents by moving the display rect.
    pub fn display_rect(&self) -> Rect {
        let shown = self.env.active_buf ^ 1;
        Rect {
            x: self.env.frame_buf[shown].x
                - vram_units(self.slot.fb_x as i32, self.slot.colour) as i16,
            y: self.env.frame_buf[shown].y - self.slot.fb_y as i16,
            w: vram_units(self.slot.width as i32, self.slot.colour) as i16,
            h: self.slot.height as i16,
        }
    }

    /// Feed one 2048-byte sector data area into the ring.
    pub fn deliver_sector(&mut self, sector: &[u8]) -> StStep {
        self.ring.deliver_sector(sector)
    }

    /// Abort playback - the pad-skip path.
    ///
    /// `FUN_801CF098` only reaches it when the live `fmv_id` (`_DAT_8007BA78`)
    /// is zero, so the intro is skippable and every mid-game FMV plays out; see
    /// [`skip_requested`].
    pub fn abort(&mut self) {
        self.aborted = true;
    }

    /// Playback has ended - the end frame was decoded, or the caller aborted.
    ///
    /// This is the play loop's exit test at `801cf4c4` (`DAT_801E09F8 == 1`),
    /// or its pad branch.
    pub fn finished(&self) -> bool {
        self.end_latch || self.aborted
    }

    /// `FUN_801CFA14` - pump one frame out of the ring.
    ///
    /// Pulls the next complete frame, latches end-of-stream if it reached the
    /// slot's `end_frame` (retail does this inside the `StGetNext` wrapper
    /// `FUN_801CF740` at `801cf788`), toggles the MDEC code buffer, dispatches
    /// to the Iki or STRv2 decoder and releases the ring slots.
    ///
    /// The toggle is `code_buf = (code_buf == 0)` *before* use, so the very
    /// first frame of a movie decodes into buffer **1**, not 0.
    // PORT: FUN_801cfa14
    pub fn next_frame(&mut self) -> Result<PumpedFrame, PumpIdle> {
        if self.finished() {
            return Err(PumpIdle::Finished);
        }
        let Some(frame) = self.ring.get_next() else {
            return Err(PumpIdle::NeedSectors);
        };
        let is_last = end_of_stream(frame.frame_number, self.slot.end_frame);
        if is_last {
            self.end_latch = true;
        }
        self.env.code_buf = usize::from(self.env.code_buf == 0);
        let bitstream = self.frame_bytes(&frame);
        self.ring.free_ring(frame.slot);
        Ok(PumpedFrame {
            bitstream,
            frame_number: frame.frame_number,
            code_buf: self.env.code_buf,
            bitstream_kind: self.bitstream,
            is_last,
        })
    }

    /// Copy a held frame's payload out of the ring, truncated to the length the
    /// sector header declared.
    fn frame_bytes(&self, frame: &StFrame) -> Vec<u8> {
        let raw = self.ring.frame_bytes(frame);
        let n = (frame.frame_size_bytes as usize).min(raw.len());
        raw[..n].to_vec()
    }
}

/// Whether a pad word aborts playback of `fmv_id`.
///
/// `FUN_801CF098` reads the live `fmv_id` from `_DAT_8007BA78` (`801cf4e0`)
/// and only consults the pad when it is zero; the mask is
/// [`SKIP_PAD_MASK`] over `_DAT_8007B850`, the per-frame button word
/// `FUN_8001822C` rebuilds.
// PORT: FUN_801cf098
pub fn skip_requested(fmv_id: i16, pad: u32) -> bool {
    fmv_id == 0 && (pad & SKIP_PAD_MASK) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_end_latch_is_inclusive_and_only_zero_is_exempt() {
        assert!(!end_of_stream(0xE0, 0xE1));
        assert!(end_of_stream(0xE1, 0xE1), "the end frame is decoded");
        assert!(
            end_of_stream(0xE2, 0xE1),
            "and anything past it latches too"
        );
        assert!(
            !end_of_stream(1, 0),
            "the port's own guard: a whole-file slot has no end frame"
        );
    }

    #[test]
    fn header_dimensions_resize_both_frame_rects_and_the_slice_height() {
        let slot = retail_slot();
        let mut env = DecodeEnv::init(&slot);
        let slice_w = env.slice.w;
        // A movie whose real frame is narrower than the dispatch record says.
        env.apply_frame_dimensions(256, 224);
        assert_eq!(env.frame_buf[0].w, vram_units(256, true) as i16);
        assert_eq!(env.frame_buf[1].w, env.frame_buf[0].w);
        assert_eq!((env.frame_buf[0].h, env.frame_buf[1].h), (224, 224));
        assert_eq!(env.slice.h, 224);
        assert_eq!(env.slice.w, slice_w, "+0x30 is never written");
        // The rect origins are the slot's and stay put.
        assert_eq!(env.frame_buf[0].y, 8);
        assert_eq!(env.frame_buf[1].y, 8 + 240);
    }

    #[test]
    fn header_dimensions_skip_the_scale_for_a_15_bit_slot() {
        let slot = FmvSlot {
            colour: false,
            ..retail_slot()
        };
        let mut env = DecodeEnv::init(&slot);
        env.apply_frame_dimensions(320, 240);
        assert_eq!(env.frame_buf[0].w, 320);
    }

    fn retail_slot() -> FmvSlot {
        // The shape every retail slot has: 320x240, 24-bit, decoding to
        // (0, 8) - the geometry `fmv_dispatch` reads off the disc.
        FmvSlot {
            colour: true,
            start_frame: 1,
            end_frame: 0xE1,
            fb_x: 0,
            fb_y: 8,
            width: 320,
            height: 240,
        }
    }

    #[test]
    fn slot_record_decodes_all_eight_words() {
        let mut rec = [0u8; 0x20];
        for (i, v) in [0x801D_0000u32, 1, 0xE2, 0x1A4, 4, 8, 320, 240]
            .into_iter()
            .enumerate()
        {
            rec[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        let slot = FmvSlot::from_record(&rec);
        assert!(slot.colour);
        assert_eq!(slot.start_frame, 0xE2);
        assert_eq!(slot.end_frame, 0x1A4);
        assert_eq!(slot.fb_x, 4);
        assert_eq!(slot.fb_y, 8);
        assert_eq!(slot.width, 320);
        assert_eq!(slot.height, 240);
    }

    #[test]
    fn vram_units_scales_only_for_24_bit() {
        assert_eq!(vram_units(320, true), 480);
        assert_eq!(vram_units(320, false), 320);
        // Truncation toward zero, matching the srl/sra sign-fix idiom.
        assert_eq!(vram_units(5, true), 7);
        assert_eq!(vram_units(-5, true), -7);
    }

    #[test]
    fn seek_offset_is_ten_sectors_per_frame_from_frame_one() {
        assert_eq!(seek_sector_offset(1), 0);
        // MV3.STR's four segments start at 1 / 0xE2 / 0x1A5 / 0x27C.
        assert_eq!(seek_sector_offset(0xE2), 0xE1 * 10);
        assert_eq!(seek_sector_offset(0x1A5), 0x1A4 * 10);
        assert_eq!(seek_sector_offset(0x27C), 0x27B * 10);
    }

    #[test]
    fn mdec_control_word_truth_table() {
        // flags bit 0 clears the depth bit; bit 1 sets the signed bit.
        assert_eq!(mdec_output_control(0, 0), MDEC_DEPTH_15BIT);
        assert_eq!(mdec_output_control(0, 1), 0);
        assert_eq!(
            mdec_output_control(0, 2),
            MDEC_DEPTH_15BIT | MDEC_SIGNED_OUTPUT
        );
        assert_eq!(mdec_output_control(0, 3), MDEC_SIGNED_OUTPUT);
        // Every other bit of the command word survives untouched.
        let word = 0x3800_ABCD;
        assert_eq!(
            mdec_output_control(word, 3) & !(MDEC_DEPTH_15BIT | MDEC_SIGNED_OUTPUT),
            word & !(MDEC_DEPTH_15BIT | MDEC_SIGNED_OUTPUT)
        );
    }

    #[test]
    fn retail_call_site_flags_pick_signed_output_always() {
        // The play loop passes 3 for colour slots, 2 otherwise - so signed
        // output is unconditional and the depth bit is the colour flag
        // inverted. That is why MdecDecoder offsets luma by +128.
        assert_eq!(mdec_control_flags(true), 3);
        assert_eq!(mdec_control_flags(false), 2);
        assert_eq!(
            mdec_output_control(0, mdec_control_flags(true)) & MDEC_SIGNED_OUTPUT,
            MDEC_SIGNED_OUTPUT
        );
        assert_eq!(
            mdec_output_control(0, mdec_control_flags(false)) & MDEC_SIGNED_OUTPUT,
            MDEC_SIGNED_OUTPUT
        );
        assert_eq!(
            mdec_output_control(0, mdec_control_flags(true)) & MDEC_DEPTH_15BIT,
            0
        );
        assert_eq!(
            mdec_output_control(0, mdec_control_flags(false)) & MDEC_DEPTH_15BIT,
            MDEC_DEPTH_15BIT
        );
    }

    #[test]
    fn env_init_stacks_the_two_frame_rects_by_height() {
        let env = DecodeEnv::init(&retail_slot());
        assert_eq!(
            env.frame_buf[0],
            Rect {
                x: 0,
                y: 8,
                w: 480,
                h: 240
            }
        );
        assert_eq!(
            env.frame_buf[1],
            Rect {
                x: 0,
                y: 248,
                w: 480,
                h: 240
            }
        );
        assert_eq!(env.slice.w, SLICE_W_24BPP);
        // A 16-bit slot uses the narrower slice and unscaled width.
        let mono = FmvSlot {
            colour: false,
            ..retail_slot()
        };
        let env = DecodeEnv::init(&mono);
        assert_eq!(env.frame_buf[0].w, 320);
        assert_eq!(env.slice.w, SLICE_W_16BPP);
    }

    #[test]
    fn slice_word_count_covers_one_full_column() {
        // 24 VRAM cells wide x 240 rows = 5760 halfwords = 2880 words.
        assert_eq!(slice_word_count(SLICE_W_24BPP, 240), 2880);
        assert_eq!(slice_word_count(SLICE_W_16BPP, 240), 1920);
        // The band count is ceil(rows / 16).
        assert_eq!(slice_word_count(SLICE_W_16BPP, 16), 16 * 16 / 2);
        assert_eq!(slice_word_count(SLICE_W_16BPP, 17), 2 * 16 * 16 / 2);
    }

    #[test]
    fn slice_cursor_walks_a_buffer_then_flips() {
        let mut env = DecodeEnv::init(&retail_slot());
        env.set_slice_callback(true);
        // 480 cells / 24 per slice = 20 slices per frame buffer.
        let mut flips = 0;
        let mut steps = 0;
        for _ in 0..40 {
            let step = env.advance_slice().expect("callback armed");
            steps += 1;
            if step.flipped {
                flips += 1;
                assert!(step.kick_words.is_none());
                if flips == 1 {
                    assert_eq!(steps, 20, "a 480-cell buffer holds 20 24-cell slices");
                    assert_eq!(env.active_buf, 1);
                    assert_eq!(env.slice.y, 248, "cursor moved to the second rect");
                }
            } else {
                assert_eq!(step.kick_words, Some(2880));
            }
        }
        assert_eq!(flips, 2, "40 slices = two full buffers");
        assert_eq!(env.active_buf, 0, "back on the first rect");
    }

    #[test]
    fn slice_callback_gates_the_cursor() {
        let mut env = DecodeEnv::init(&retail_slot());
        env.set_slice_callback(false);
        assert!(env.advance_slice().is_none());
        assert_eq!(env.slice.x, 0, "cursor must not move while uninstalled");
        env.set_slice_callback(true);
        assert!(env.advance_slice().is_some());
    }

    #[test]
    fn leading_partial_column_lands_the_last_slice_flush() {
        // A width that is not a whole number of slices: 24-cell slices over a
        // 500-cell buffer leaves a 20-cell remainder taken up front.
        let slot = FmvSlot {
            width: 500 * 2 / 3,
            ..retail_slot()
        };
        let mut env0 = DecodeEnv::init(&slot);
        env0.set_slice_callback(true);
        let remainder = env0.frame_buf[0].w % SLICE_W_24BPP;
        assert_ne!(remainder, 0);
        let mut env = env0;
        let first = env.advance_slice().unwrap();
        assert_eq!(first.load_rect.x, env0.slice.x);
        assert_eq!(env.slice.x, env0.slice.x + remainder);
        // From here every step is a full slice, and the walk ends exactly on
        // the right edge.
        while !env.advance_slice().unwrap().flipped {}
        assert_eq!(env.active_buf, 1);
    }

    #[test]
    fn skip_is_intro_only() {
        assert!(skip_requested(0, SKIP_PAD_MASK));
        assert!(skip_requested(0, 0x010));
        assert!(
            !skip_requested(0, 0x00F),
            "buttons outside the mask do not skip"
        );
        for id in 1..=8 {
            assert!(
                !skip_requested(id, 0xFFFF),
                "mid-game fmv {id} must play out"
            );
        }
    }
}
