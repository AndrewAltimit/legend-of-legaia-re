//! The retail `St` streaming-library sector ring - the demux stage that sits
//! between the CD drive and the MDEC bitstream decoder.
//!
//! [`StrFrameAssembler`](crate::str_sector::StrFrameAssembler) is the simple
//! "concatenate the payloads of one frame" view of STR demuxing, and it is what
//! the offline tools use. Retail does the same job through a **fixed-size
//! sector ring** shared with the CD DMA engine, and the ring bookkeeping is
//! observable: it decides when a frame is dropped, when the stream is declared
//! finished, and which sectors are skipped while seeking to a mid-file segment.
//! This module is the clean-room port of that ring, minus the hardware pokes.
//!
//! ## Ring layout
//!
//! `StSetRing(base, slots)` hands the library one flat buffer holding two
//! parallel arrays:
//!
//! ```text
//! base + 0                        slots x 32-byte slot headers
//! base + slots * 32               slots x 2016-byte payload areas
//! ```
//!
//! One slot holds exactly one sector: its 32-byte STR sector header (with the
//! `u16` at `+0x00` repurposed as the slot **status** once the header has been
//! inspected) and its 2016-byte payload. A frame occupies `chunks_per_frame`
//! *consecutive* slots, which is what makes the assembled frame a contiguous
//! run in the payload area - the decoder reads it in place, with no copy.
//!
//! ## Slot status
//!
//! | Value | Meaning |
//! |------:|---------|
//! | 0 | free |
//! | 1 | wrap marker - the reader restarts at slot 0 when it lands here |
//! | 2 | frame complete, ready for [`StRing::get_next`] |
//! | 3 | filling (sectors of the current frame still arriving) |
//! | 4 | handed to the decoder; released by [`StRing::free_ring`] |
//!
//! ## Frame window
//!
//! [`StRing::set_stream`] **does** install the frame window, exactly as its PsyQ
//! prototype implies. Retail's `StSetStream` (`FUN_8005EDC4`) opens with
//!
//! ```text
//! 8005ede4  jal 0x8005f004
//! 8005ede8  _li a0,0x1        <- the delay slot writes a0 and nothing else
//! ```
//!
//! and never writes `a1` or `a2` before that call (only `a3` is saved into
//! `s1`), so its own `start_frame` / `end_frame` arguments fall straight through
//! into the callee. Retail is `FUN_8005f004(1, start_frame, end_frame)`: the
//! seek arm is hard-coded on, and the window comes from `StSetStream`.
//!
//! Ghidra prints that call as `FUN_8005f004(1)` because it infers a
//! one-argument signature for the callee from this one call site - the dropped
//! arguments are a decompiler artefact, not retail behaviour. `FUN_8005F004`
//! (ported as [`StRing::set_mask`]) has no other caller in any dump; it is
//! `StSetStream`'s window-installer helper, not a separate entry point the play
//! loop drives.
//!
//! The one retail call site, `FUN_801CF988`, is
//! `StSetStream(slot[+0x04], slot[+0x08], -1, 0, 0)`: mode and `start_frame`
//! come from the FMV dispatch slot, but `end_frame` is a literal `-1`. So the
//! St library's end-frame stop is effectively disabled in retail, and a
//! segment's end is enforced one level up by the play loop `FUN_801CF098`,
//! which compares the demuxed frame number against the slot's own `+0x0C`
//! (`801cf384 lw v0,0xc(s3)` / `801cf38c slt v0,v0,s0`).
//!
//! ## Provenance
//!
//! `see ghidra/scripts/funcs/8005bbf8.txt` (`StSetRing`), `8005edc4.txt`
//! (`StSetStream`), `8005ee4c.txt` (`StFreeRing`), `8005ef40.txt`
//! (`StGetNext`), `8005ecd4.txt` (the CD data-ready frame latch) and
//! `8005f004.txt` (the frame-window installer) and `8005f024.txt` (the
//! per-sector demux state machine). Subsystem write-up:
//! [`docs/subsystems/cutscene.md`](../../../docs/subsystems/cutscene.md).
//!
//! ## Wiring status
//!
//! NOT WIRED: this module is a standalone kernel. The engine's own cutscene
//! path (`legaia_engine_core::cutscene`, `legaia-engine play-str`) demuxes
//! through [`StrFrameAssembler`](crate::str_sector::StrFrameAssembler) instead,
//! and nothing outside this crate's tests constructs an [`StRing`]. It exists
//! as the faithful reading of retail's back-pressure and seek behaviour, and as
//! the oracle the assembler is cross-checked against.

use crate::str_sector::{SECTOR_HEADER_BYTES, SECTOR_PAYLOAD_BYTES, VIDEO_SECTOR_MAGIC};

/// Ring slots the STR overlay allocates (`StSetRing(ring, 0x20)`).
pub const RETAIL_RING_SLOTS: usize = 0x20;

/// Slot status: free.
const SLOT_FREE: u16 = 0;
/// Slot status: wrap marker - the reader restarts at slot 0 here.
const SLOT_WRAP: u16 = 1;
/// Slot status: the frame starting at this slot is complete.
const SLOT_COMPLETE: u16 = 2;
/// Slot status: sectors of this frame are still arriving.
const SLOT_FILLING: u16 = 3;
/// Slot status: handed to the decoder, awaiting [`StRing::free_ring`].
const SLOT_IN_USE: u16 = 4;

/// Per-sector outcome of the demux state machine.
///
/// The discriminants are retail's own trace codes - the state machine writes
/// them to `DAT_800797B8` on every exit path, which makes them the natural
/// shape for the port's return value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StStatus {
    /// The ring slot the sector would land in is still occupied; the sector is
    /// dropped rather than overrunning a frame the decoder still holds.
    RingFull = 4,
    /// Not a video sector for this stream: either the `0x0160` magic is absent
    /// or the sector's stream number doesn't match the active filter.
    NotForStream = 5,
    /// `chunk_number` / `frame_number` broke sequence. The partial frame is
    /// discarded and the write cursor rewinds to its first slot.
    SequenceBreak = 6,
    /// The slot's `end_frame` was reached; the stream is finished and the seek
    /// state re-arms for the next segment.
    EndFrame = 7,
    /// The frame doesn't fit in the slots left before the end of the ring and
    /// no `end_frame` is set, so the stream ends here instead of wrapping.
    RingWrapEnd = 8,
    /// The frame doesn't fit before the end of the ring and slot 0 is still
    /// busy, so the wrap is refused and the sector dropped.
    RingWrapBlocked = 9,
    /// Sector accepted into the ring.
    Accepted = 10,
}

/// What [`StRing::deliver_sector`] observed while consuming one sector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StStep {
    /// Retail's per-sector trace code.
    pub status: StStatus,
    /// A frame became readable by [`StRing::get_next`] on this sector.
    pub frame_ready: bool,
    /// The stream ended on this sector (end frame reached, or the ring wrapped
    /// with no end frame set).
    pub end_of_stream: bool,
}

/// A complete frame the ring is holding for the decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StFrame {
    /// Ring slot the frame starts at. Pass to [`StRing::free_ring`] when done.
    pub slot: usize,
    /// Byte offset of the frame's payload within the ring's payload area.
    pub offset: usize,
    /// Payload length - `chunks_per_frame * 2016`, so the tail of the last
    /// sector is included and the caller truncates to `frame_size_bytes`.
    pub len: usize,
    /// Sequence number from the frame's first sector.
    pub frame_number: u32,
    /// Bitstream length the sector header declares for this frame.
    pub frame_size_bytes: u32,
}

/// One ring slot header: the fields the demuxer keeps from the STR sector
/// header, plus the slot status that overwrites the magic word in place.
#[derive(Debug, Clone, Copy, Default)]
struct Slot {
    status: u16,
    chunk_number: u16,
    chunks_per_frame: u16,
    frame_number: u32,
    frame_size_bytes: u32,
}

/// The retail STR sector ring and its demux state machine.
///
/// Feed it whole 2048-byte sector data areas with [`StRing::deliver_sector`],
/// drain complete frames with [`StRing::get_next`], and return each frame's
/// slots with [`StRing::free_ring`]. Failing to free is not a panic - it is
/// exactly the [`StStatus::RingFull`] back-pressure retail relies on.
pub struct StRing {
    slots: Vec<Slot>,
    payload: Vec<u8>,
    /// Next slot the demuxer writes (`_DAT_801CADBC`).
    write_slot: usize,
    /// First slot of the frame being written (`_DAT_801CADC0`).
    frame_start_slot: usize,
    /// Slot the reader is positioned on (`_DAT_801CADC4`).
    read_slot: usize,
    /// Frame number currently being accumulated, `0` = none (`_DAT_801CAD90`).
    cur_frame: u32,
    /// Chunk index the next sector of this frame must carry (`_DAT_801CAD94`).
    next_chunk: u32,
    /// Active stream-number filter (`_DAT_801CADB0`); `0` for every retail movie.
    stream_filter: u16,
    /// Filter that becomes active at the next frame boundary (`_DAT_801CADA8`).
    pending_filter: u16,
    /// Seek-to-`start_frame` armed (`_DAT_801CADD0`).
    seek_armed: bool,
    /// Frame number to seek to, `0` = no seek (`_DAT_801CADAC`).
    start_frame: u32,
    /// Frame number that ends the stream, `0` = play to EOF (`_DAT_801CADCC`).
    end_frame: u32,
    /// The last sector of a frame has landed; the completion latch is pending
    /// (`_DAT_801CADB4`).
    frame_pending: bool,
    /// Low bit of `StSetStream`'s mode word (`_DAT_801CAD98`).
    mode_flag: bool,
}

impl StRing {
    /// `StSetRing` - allocate the ring and reset every slot to free.
    ///
    /// `slots` is retail's second argument; the STR overlay passes
    /// [`RETAIL_RING_SLOTS`].
    // PORT: FUN_8005bbf8
    pub fn set_ring(slots: usize) -> Self {
        let slots = slots.max(1);
        Self {
            slots: vec![Slot::default(); slots],
            payload: vec![0u8; slots * SECTOR_PAYLOAD_BYTES],
            write_slot: 0,
            frame_start_slot: 0,
            read_slot: 0,
            cur_frame: 0,
            next_chunk: 0,
            stream_filter: 0,
            pending_filter: 0,
            seek_armed: false,
            start_frame: 0,
            end_frame: 0,
            frame_pending: false,
            mode_flag: false,
        }
    }

    /// The ring with retail's slot count.
    pub fn retail() -> Self {
        Self::set_ring(RETAIL_RING_SLOTS)
    }

    /// `StSetStream` - install the frame window and reset the demux cursors for
    /// a fresh stream.
    ///
    /// Retail's first act is `FUN_8005f004(1, start_frame, end_frame)`, so the
    /// seek is **always** armed here and both bounds are this function's own
    /// arguments falling through in `a1`/`a2` - see the module note on the frame
    /// window for the delay-slot evidence. Pass `start_frame = 0` for "start at
    /// the first frame that arrives" (retail's own `0` short-circuits the seek
    /// comparison) and `end_frame = 0` to play to EOF; the one retail call site
    /// passes `-1` for the latter, which behaves the same way for any real
    /// movie.
    ///
    /// `mode` is retail's mode word, of which only bit 0 is kept
    /// (`_DAT_801CAD98 = param_1 & 1`); see [`StRing::mode_flag`]. Retail's
    /// remaining two arguments are the frame-complete and end-of-stream
    /// callbacks, surfaced here as [`StStep::frame_ready`] /
    /// [`StStep::end_of_stream`] rather than as function pointers.
    // PORT: FUN_8005edc4
    pub fn set_stream(&mut self, mode: u32, start_frame: u32, end_frame: u32) {
        self.set_mask(true, start_frame, end_frame);
        self.stream_filter = 0;
        self.pending_filter = 0;
        self.next_chunk = 0;
        self.cur_frame = 0;
        // Retail leaves `_DAT_801CADB4` alone here; the port clears it because
        // its completion latch runs inline instead of from a DMA interrupt, so
        // a half-delivered frame can never be left pending across a restart.
        self.frame_pending = false;
        self.mode_flag = mode & 1 != 0;
    }

    /// `FUN_8005F004` - install the frame window: seek arm, `start_frame`,
    /// `end_frame`.
    ///
    /// With `seek_armed` set and a non-zero `start_frame`, every sector before
    /// that frame is discarded, so a segment inside a multi-cutscene movie
    /// (`MV3.STR` carries four) starts exactly on its first frame. `end_frame`
    /// of `0` plays to EOF.
    ///
    /// In retail this is reached only through [`StRing::set_stream`], which is
    /// its sole caller; it stays public here because the demuxer re-arms the
    /// same three globals itself on the end-frame path.
    // PORT: FUN_8005f004
    pub fn set_mask(&mut self, seek_armed: bool, start_frame: u32, end_frame: u32) {
        self.seek_armed = seek_armed;
        self.start_frame = start_frame;
        self.end_frame = end_frame;
    }

    /// Bit 0 of the mode word [`StRing::set_stream`] was given
    /// (`_DAT_801CAD98`).
    ///
    /// Retail reads it in two places, both hardware-side: it gates the
    /// sector-lost check against the CD status word, and it selects between the
    /// two DMA attribute words used for the payload transfer. The port has no
    /// CD or DMA registers to poke, so it only records the flag.
    pub fn mode_flag(&self) -> bool {
        self.mode_flag
    }

    /// Number of ring slots.
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// The ring's payload area - frame bytes live at [`StFrame::offset`].
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Borrow the bytes of a frame [`StRing::get_next`] returned.
    pub fn frame_bytes(&self, frame: &StFrame) -> &[u8] {
        let end = (frame.offset + frame.len).min(self.payload.len());
        let bytes = &self.payload[frame.offset.min(end)..end];
        let want = frame.frame_size_bytes as usize;
        if want > 0 && want <= bytes.len() {
            &bytes[..want]
        } else {
            bytes
        }
    }

    /// The CD data-ready latch: publish the frame whose last sector just
    /// landed and advance the write cursor onto the next frame.
    ///
    /// Retail runs this from the DMA-completion interrupt (and inline on the
    /// memory-source path); the port always runs it inline from
    /// [`StRing::deliver_sector`].
    // PORT: FUN_8005ecd4
    fn latch_frame(&mut self) {
        self.slots[self.frame_start_slot].status = SLOT_COMPLETE;
        self.frame_start_slot = self.write_slot;
        self.frame_pending = false;
    }

    /// Free `count` slots starting at `start` (`FUN_8005EF04`).
    // REF: FUN_8005ef04
    fn free_slots(&mut self, start: usize, count: usize) {
        let n = self.slots.len();
        for i in 0..count {
            self.slots[(start + i) % n].status = SLOT_FREE;
        }
    }

    /// Discard the partially written frame and rewind the write cursor to its
    /// first slot - the shared tail of the sequence-break and end-frame paths.
    fn rewind_partial_frame(&mut self) {
        self.cur_frame = 0;
        self.next_chunk = 0;
        let count = self.write_slot.wrapping_sub(self.frame_start_slot);
        self.free_slots(self.frame_start_slot, count);
        self.write_slot = self.frame_start_slot;
    }

    /// `StGetNext` - hand the decoder the next complete frame, if one is ready.
    ///
    /// Landing on a wrap marker restarts the reader at slot 0 (clearing the
    /// marker only when an end frame is set, exactly as retail does). The
    /// returned frame's slots stay reserved until [`StRing::free_ring`].
    // PORT: FUN_8005ef40
    pub fn get_next(&mut self) -> Option<StFrame> {
        if self.slots[self.read_slot].status == SLOT_WRAP {
            let marker = self.read_slot;
            self.read_slot = 0;
            if self.end_frame != 0 {
                self.slots[marker].status = SLOT_FREE;
            }
        }
        let slot = self.read_slot;
        if self.slots[slot].status != SLOT_COMPLETE {
            return None;
        }
        self.slots[slot].status = SLOT_IN_USE;
        Some(StFrame {
            slot,
            offset: slot * SECTOR_PAYLOAD_BYTES,
            len: self.slots[slot].chunks_per_frame as usize * SECTOR_PAYLOAD_BYTES,
            frame_number: self.slots[slot].frame_number,
            frame_size_bytes: self.slots[slot].frame_size_bytes,
        })
    }

    /// `StFreeRing` - release every slot of a frame handed out by
    /// [`StRing::get_next`] and park the reader on the following frame.
    ///
    /// Returns `false` when the slot isn't one the decoder holds (retail's
    /// non-zero error return).
    // PORT: FUN_8005ee4c
    pub fn free_ring(&mut self, slot: usize) -> bool {
        let slot = slot % self.slots.len();
        if self.slots[slot].status != SLOT_IN_USE {
            return false;
        }
        let count = self.slots[slot].chunks_per_frame as usize;
        self.free_slots(slot, count);
        self.read_slot = (slot + count) % self.slots.len();
        true
    }

    /// The per-sector demux state machine.
    ///
    /// `sector` is one 2048-byte Mode 2 Form 1 data area (32-byte STR sector
    /// header + 2016 payload bytes). Short buffers are reported as
    /// [`StStatus::NotForStream`], the same class retail drops non-video
    /// sectors into.
    // PORT: FUN_8005f024
    pub fn deliver_sector(&mut self, sector: &[u8]) -> StStep {
        let no_frame = |status| StStep {
            status,
            frame_ready: false,
            end_of_stream: false,
        };
        if sector.len() < SECTOR_HEADER_BYTES {
            return no_frame(StStatus::NotForStream);
        }

        // The header DMA lands in the write slot before anything is inspected,
        // so a busy slot is back-pressure, not an error.
        if self.slots[self.write_slot].status != SLOT_FREE {
            return no_frame(StStatus::RingFull);
        }

        let rd16 = |o: usize| u16::from_le_bytes(sector[o..o + 2].try_into().unwrap());
        let rd32 = |o: usize| u32::from_le_bytes(sector[o..o + 4].try_into().unwrap());
        let magic = rd16(0x00);
        let sector_type = rd16(0x02);
        let chunk_number = rd16(0x04);
        let chunks_per_frame = rd16(0x06);
        let frame_number = rd32(0x08);
        let frame_size_bytes = rd32(0x0C);

        // Seek-to-start: drop whole sectors until the segment's first frame.
        if self.seek_armed && self.start_frame != 0 {
            if self.start_frame != (frame_number & 0xFFFF) {
                self.slots[self.write_slot].status = SLOT_FREE;
                return no_frame(StStatus::NotForStream);
            }
            self.seek_armed = false;
        }

        // Video gate. The stream number lives in the type word's bits 10..14;
        // it is 0 for every retail movie.
        if magic != VIDEO_SECTOR_MAGIC || ((sector_type >> 10) & 0x1F) != self.stream_filter {
            self.slots[self.write_slot].status = SLOT_FREE;
            return no_frame(StStatus::NotForStream);
        }

        // Sequence gate: chunks must arrive in order within one frame number.
        let in_sequence = self.next_chunk == chunk_number as u32
            && (self.cur_frame == 0 || self.cur_frame == (frame_number & 0xFFFF));
        if !in_sequence {
            self.slots[self.write_slot].status = SLOT_FREE;
            self.rewind_partial_frame();
            return no_frame(StStatus::SequenceBreak);
        }

        if chunk_number == 0 {
            self.cur_frame = frame_number & 0xFFFF;
            self.next_chunk = 0;

            if self.end_frame != 0 && self.end_frame <= self.cur_frame {
                self.slots[self.write_slot].status = SLOT_FREE;
                self.rewind_partial_frame();
                self.seek_armed = true;
                return StStep {
                    status: StStatus::EndFrame,
                    frame_ready: false,
                    end_of_stream: true,
                };
            }

            // Does the whole frame fit in the slots left before the ring end?
            let room = self.slots.len() - self.write_slot - 1;
            if room < chunks_per_frame as usize {
                if self.end_frame == 0 {
                    // No end frame to play towards: stop rather than wrap.
                    self.slots[self.write_slot].status = SLOT_WRAP;
                    self.seek_armed = true;
                    return StStep {
                        status: StStatus::RingWrapEnd,
                        frame_ready: false,
                        end_of_stream: true,
                    };
                }
                if self.slots[0].status != SLOT_FREE {
                    self.slots[self.write_slot].status = SLOT_FREE;
                    return no_frame(StStatus::RingWrapBlocked);
                }
                // Leave the wrap marker behind and restart at slot 0, carrying
                // the slot header down with the write cursor.
                self.slots[self.write_slot].status = SLOT_WRAP;
                let carried = self.slots[self.write_slot];
                self.write_slot = 0;
                self.slots[0] = carried;
            }
            self.frame_start_slot = self.write_slot;
        }

        let slot = self.write_slot;
        self.slots[slot].chunk_number = chunk_number;
        self.slots[slot].chunks_per_frame = chunks_per_frame;
        self.slots[slot].frame_number = frame_number;
        self.slots[slot].frame_size_bytes = frame_size_bytes;
        self.next_chunk += 1;

        let src = &sector[SECTOR_HEADER_BYTES..sector.len().min(2048)];
        let dst = slot * SECTOR_PAYLOAD_BYTES;
        let n = src.len().min(SECTOR_PAYLOAD_BYTES);
        self.payload[dst..dst + n].copy_from_slice(&src[..n]);
        self.payload[dst + n..dst + SECTOR_PAYLOAD_BYTES].fill(0);

        if chunks_per_frame.saturating_sub(1) == chunk_number {
            self.frame_pending = true;
            self.next_chunk = 0;
            self.cur_frame = 0;
            self.stream_filter = self.pending_filter;
        }
        self.slots[slot].status = SLOT_FILLING;
        self.write_slot += 1;

        let mut frame_ready = false;
        if self.frame_pending {
            self.latch_frame();
            frame_ready = true;
        }
        // The room check on chunk 0 guarantees the whole frame fits before the
        // ring end, so the cursor lands at `slots - 1` at worst.
        debug_assert!(self.write_slot < self.slots.len());
        StStep {
            status: StStatus::Accepted,
            frame_ready,
            end_of_stream: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build one 2048-byte STR video sector data area.
    fn sector(frame: u32, chunk: u16, chunks: u16, fill: u8) -> Vec<u8> {
        let mut s = vec![0u8; 2048];
        s[0..2].copy_from_slice(&VIDEO_SECTOR_MAGIC.to_le_bytes());
        s[2..4].copy_from_slice(&0x8001u16.to_le_bytes());
        s[4..6].copy_from_slice(&chunk.to_le_bytes());
        s[6..8].copy_from_slice(&chunks.to_le_bytes());
        s[8..12].copy_from_slice(&frame.to_le_bytes());
        let size = chunks as u32 * SECTOR_PAYLOAD_BYTES as u32;
        s[12..16].copy_from_slice(&size.to_le_bytes());
        s[16..18].copy_from_slice(&320u16.to_le_bytes());
        s[18..20].copy_from_slice(&240u16.to_le_bytes());
        s[32..].fill(fill);
        s
    }

    fn push_frame(ring: &mut StRing, frame: u32, chunks: u16, fill: u8) -> Vec<StStep> {
        (0..chunks)
            .map(|c| ring.deliver_sector(&sector(frame, c, chunks, fill)))
            .collect()
    }

    #[test]
    fn a_complete_frame_becomes_readable_and_frees() {
        let mut ring = StRing::retail();
        ring.set_stream(1, 0, 0);
        let steps = push_frame(&mut ring, 1, 3, 0xAB);
        assert!(steps.iter().all(|s| s.status == StStatus::Accepted));
        assert!(!steps[0].frame_ready && !steps[1].frame_ready);
        assert!(steps[2].frame_ready, "last chunk latches the frame");

        let frame = ring.get_next().expect("frame ready");
        assert_eq!(frame.slot, 0);
        assert_eq!(frame.frame_number, 1);
        assert_eq!(frame.len, 3 * SECTOR_PAYLOAD_BYTES);
        assert!(ring.frame_bytes(&frame).iter().all(|&b| b == 0xAB));

        assert!(ring.free_ring(frame.slot));
        // Freeing parks the reader on the slot after the frame.
        assert!(ring.get_next().is_none());
    }

    #[test]
    fn frames_occupy_consecutive_slots_so_payloads_are_contiguous() {
        let mut ring = StRing::retail();
        ring.set_stream(1, 0, 0);
        for (i, fill) in [(1u32, 0x11u8), (2, 0x22)] {
            push_frame(&mut ring, i, 2, fill);
            let f = ring.get_next().expect("frame ready");
            assert_eq!(f.offset, (i as usize - 1) * 2 * SECTOR_PAYLOAD_BYTES);
            assert!(ring.frame_bytes(&f).iter().all(|&b| b == fill));
            assert!(ring.free_ring(f.slot));
        }
    }

    #[test]
    fn non_video_and_filtered_sectors_are_dropped_silently() {
        let mut ring = StRing::retail();
        ring.set_stream(1, 0, 0);
        let mut audio = sector(1, 0, 1, 0);
        audio[0..2].copy_from_slice(&0x0100u16.to_le_bytes());
        assert_eq!(
            ring.deliver_sector(&audio).status,
            StStatus::NotForStream,
            "a non-0x0160 magic never reaches the ring"
        );
        let mut other_stream = sector(1, 0, 1, 0);
        other_stream[2..4].copy_from_slice(&(0x8001u16 | (3 << 10)).to_le_bytes());
        assert_eq!(
            ring.deliver_sector(&other_stream).status,
            StStatus::NotForStream
        );
        // Neither consumed a slot.
        assert!(push_frame(&mut ring, 1, 1, 0x5A)[0].frame_ready);
        assert_eq!(ring.get_next().expect("frame").slot, 0);
    }

    #[test]
    fn out_of_order_chunk_discards_the_partial_frame() {
        let mut ring = StRing::retail();
        ring.set_stream(1, 0, 0);
        ring.deliver_sector(&sector(1, 0, 3, 0x01));
        ring.deliver_sector(&sector(1, 1, 3, 0x01));
        // Chunk 2 is expected; chunk 0 of the next frame breaks sequence.
        let step = ring.deliver_sector(&sector(2, 0, 3, 0x02));
        assert_eq!(step.status, StStatus::SequenceBreak);
        assert!(ring.get_next().is_none(), "partial frame is not published");
        // The write cursor rewound, so the next good frame starts at slot 0.
        assert!(push_frame(&mut ring, 2, 2, 0x02)[1].frame_ready);
        assert_eq!(ring.get_next().expect("frame").slot, 0);
    }

    #[test]
    fn seek_to_start_frame_skips_earlier_sectors() {
        let mut ring = StRing::retail();
        ring.set_stream(1, 5, 0);
        for f in 1..5 {
            for c in 0..2 {
                assert_eq!(
                    ring.deliver_sector(&sector(f, c, 2, 0x00)).status,
                    StStatus::NotForStream,
                    "frames before start_frame are discarded whole"
                );
            }
        }
        assert!(push_frame(&mut ring, 5, 2, 0x77)[1].frame_ready);
        assert_eq!(ring.get_next().expect("frame").frame_number, 5);
    }

    /// `set_stream` alone must arm the seek and carry both bounds, because
    /// retail's `StSetStream` tail-passes its untouched `a1`/`a2` into
    /// `FUN_8005f004(1, start_frame, end_frame)`. A port that dropped them
    /// would still pass every other test here while never seeking, so this is
    /// the guard for exactly that regression.
    #[test]
    fn set_stream_alone_installs_the_window_no_set_mask_needed() {
        let mut ring = StRing::retail();
        ring.set_stream(1, 5, 9);
        assert_eq!(
            ring.deliver_sector(&sector(4, 0, 1, 0x00)).status,
            StStatus::NotForStream,
            "the seek is armed by set_stream, not by a separate set_mask call"
        );
        assert!(push_frame(&mut ring, 5, 1, 0x77)[0].frame_ready);
        assert_eq!(ring.get_next().expect("frame").frame_number, 5);
        // The end bound came through the same call.
        let step = ring.deliver_sector(&sector(9, 0, 1, 0x00));
        assert_eq!(step.status, StStatus::EndFrame);
        assert!(step.end_of_stream);
    }

    #[test]
    fn mode_word_keeps_only_its_low_bit() {
        let mut ring = StRing::retail();
        ring.set_stream(1, 0, 0);
        assert!(ring.mode_flag());
        ring.set_stream(2, 0, 0);
        assert!(!ring.mode_flag(), "_DAT_801CAD98 = param_1 & 1");
    }

    #[test]
    fn end_frame_terminates_the_stream_and_rearms_the_seek() {
        let mut ring = StRing::retail();
        ring.set_stream(1, 0, 3);
        push_frame(&mut ring, 1, 1, 0x01);
        let f = ring.get_next().expect("frame 1");
        ring.free_ring(f.slot);
        push_frame(&mut ring, 2, 1, 0x02);
        let f = ring.get_next().expect("frame 2");
        ring.free_ring(f.slot);
        // Frame 3 is the end frame: it is not published.
        let step = ring.deliver_sector(&sector(3, 0, 1, 0x03));
        assert_eq!(step.status, StStatus::EndFrame);
        assert!(step.end_of_stream);
        assert!(!step.frame_ready);
        assert!(ring.get_next().is_none());
    }

    #[test]
    fn ring_full_drops_the_sector_instead_of_overrunning() {
        // Four slots, and the decoder never frees: the fifth sector has
        // nowhere to go.
        let mut ring = StRing::set_ring(4);
        ring.set_stream(1, 0, 0);
        push_frame(&mut ring, 1, 1, 0x01);
        push_frame(&mut ring, 2, 1, 0x02);
        push_frame(&mut ring, 3, 1, 0x03);
        let step = ring.deliver_sector(&sector(4, 0, 1, 0x04));
        assert_eq!(step.status, StStatus::RingWrapEnd);
        assert!(
            step.end_of_stream,
            "no end frame set: the stream stops here"
        );
    }

    #[test]
    fn wrap_is_blocked_while_slot_zero_is_still_held() {
        let mut ring = StRing::set_ring(4);
        ring.set_stream(1, 0, 0xFFFF);
        push_frame(&mut ring, 1, 1, 0x01);
        // Hand frame 1 out but never free it, so slot 0 stays in use.
        let held = ring.get_next().expect("frame 1");
        assert_eq!(held.slot, 0);
        push_frame(&mut ring, 2, 1, 0x02);
        push_frame(&mut ring, 3, 1, 0x03);
        let step = ring.deliver_sector(&sector(4, 0, 1, 0x04));
        assert_eq!(step.status, StStatus::RingWrapBlocked);
        // Free it and the wrap goes through.
        assert!(ring.free_ring(held.slot));
        assert_eq!(
            ring.deliver_sector(&sector(4, 0, 1, 0x04)).status,
            StStatus::Accepted
        );
    }

    #[test]
    fn free_ring_rejects_a_slot_the_decoder_does_not_hold() {
        let mut ring = StRing::retail();
        ring.set_stream(1, 0, 0);
        assert!(
            !ring.free_ring(0),
            "a free slot is not the decoder's to give back"
        );
        push_frame(&mut ring, 1, 2, 0x01);
        assert!(!ring.free_ring(0), "complete but not yet handed out");
        let f = ring.get_next().expect("frame");
        assert!(ring.free_ring(f.slot));
        assert!(!ring.free_ring(f.slot), "double free is rejected");
    }
}
