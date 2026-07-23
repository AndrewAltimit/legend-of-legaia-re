//! Streaming-chunk installer - the `[type, size, data]` list walker that
//! routes scene-load side-band chunks into their hardware sinks.
//!
//! Retail `FUN_8001E54C` (`ghidra/scripts/funcs/8001e54c.txt`; the
//! scene-load chunk loader pinned by the working-buffer write capture in
//! [`docs/formats/world-map-overlay.md`]) walks a word-aligned chunk list:
//! each chunk opens with a u32 header whose low 24 bits are the payload
//! **byte** size and whose top byte is the chunk type, followed by the
//! payload; the walker advances `size >> 2` words past the payload and stops
//! at a zero-size header. Types `0..=0xC` dispatch through a jump table:
//!
//! | type | action |
//! |---|---|
//! | `0` | release the slot's open SEQ handle (`FUN_8001FF58`), then raw-copy the payload into the slot's destination buffer |
//! | `1` | VAB bank upload (`FUN_8002630C` = SsVabOpenHead/TransBody wrapper), set the slot's loaded flag |
//! | `2` | VRAM rect upload via the slot's staging buffer + finalize (`FUN_80026410`) |
//! | `3` | budget-bounded VAB upload (remaining transfer budget), set the loaded flag, stop the walk, return the leftover byte count |
//! | `4` | set the cross-call stream flag (`gp+0x700`), and copy into the staging buffer when the transfer-mode global (`_DAT_8007B8B8`) is 0 |
//! | `0xC` | VRAM rect upload into the fixed globals window (`_DAT_80091574..7C`), finalize |
//! | `5..=0xB` | reserved - advance only |
//!
//! Types `>= 0xD` skip the dispatch but still advance (the retail
//! `sltiu 0xd` guard). Slot bookkeeping targets the 12-byte-stride resource
//! table at `0x80091508` (`+0x8` = SEQ/bank id byte, `+0xB` = loaded flag);
//! this walker is the install counterpart of the `FUN_8001FF58` release.
//!
//! Scope: the port covers the streaming-list arm (`param_1 >> 16 == 0`).
//! The sibling direct arm (high half set: one whole-buffer VAB upload plus
//! the deferred stage/finalize keyed off the stream flag) and the
//! `_DAT_8007B868` busy gate are host-side re-entry plumbing, not list
//! decoding. // REF: FUN_8002630C // REF: FUN_8001FF58

/// Where a raw chunk copy lands (the two per-slot pointers of the
/// `0x80091508` record the retail dispatcher dereferences).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyDest {
    /// The slot's destination buffer (record `+0x0`; type-0 copy).
    SlotDest,
    /// The slot's staging buffer (record `+0x4`; type-4 copy).
    SlotStage,
}

/// Which VRAM rect window a type-2 / type-0xC chunk targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VramRectKind {
    /// Type 2: the slot's own staging window (`DAT_8007051C + slot*0x10`).
    Slot,
    /// Type 0xC: the fixed globals window (`_DAT_80091574..7C`,
    /// finalized through the `0x800705AC` record).
    Fixed,
}

/// Host-owned leaf actions of the chunk dispatch. Hardware sinks (SPU DMA,
/// VRAM upload, the destination heap) stay behind this trait; the walker
/// only decodes framing and routing.
pub trait InstallHost {
    /// Type-0 prelude: release the slot's open SEQ handle
    /// (retail `FUN_8001FF58` - clear the handle, drop the loaded flag).
    fn seq_release(&mut self, id: i8);
    /// Types 0 / 4: copy `data` into the host buffer selected by `dest`.
    fn raw_copy(&mut self, dest: CopyDest, data: &[u8]);
    /// Types 1 / 3: VAB bank upload. `budget` is `None` on the plain type-1
    /// path and `Some(remaining_bytes)` on the type-3 bounded path.
    fn vab_upload(&mut self, id: i8, data: &[u8], budget: Option<usize>);
    /// Types 2 / 0xC: VRAM rect upload + finalize into `kind`'s window.
    fn vram_rect(&mut self, kind: VramRectKind, id: i8, data: &[u8]);
    /// Type 4: set the cross-call stream flag (retail `gp+0x700`, consumed
    /// by the direct arm's deferred finalize).
    fn set_stream_flag(&mut self);
    /// The `_DAT_8007B8B8` transfer-mode global (0 = copy-through; the
    /// type-4 staging copy only runs when it reads 0).
    fn transfer_mode(&self) -> u32;
}

/// One entry of the 12-byte-stride resource table at `0x80091508`, reduced
/// to the two fields the walker itself touches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SeqSlot {
    /// SEQ / bank id byte (record `+0x8`, read sign-extended).
    pub id: i8,
    /// Loaded flag (record `+0xB`) - set by the VAB-upload types (1 / 3),
    /// cleared by the `FUN_8001FF58` release.
    pub loaded: bool,
}

// PORT: FUN_8001E54C - streaming-chunk list walker + 13-case type dispatch
// (raw copy / VAB upload / VRAM rect / stream flag / budget stop), with the
// hardware sinks behind [`InstallHost`] and the `0x80091508` slot
// bookkeeping on [`SeqSlot`].
// NOT WIRED: the engine resolves scene sub-assets through the typed
// `legaia_asset` dispatcher and uploads VRAM and VAB directly from those.
// Nothing produces retail's `[type, size, data]` side-band chunk list, so
// the walker has no stream to walk. Wiring it needs a producer that emits
// that side band - i.e. the retail streaming loader, not the typed one.
/// Walk `stream` as a `[header, payload]` chunk list and dispatch each chunk.
///
/// `budget` mirrors the retail third argument (the remaining transfer-byte
/// budget a type-3 chunk bounds itself by). Returns the type-3 leftover
/// byte count (`size - remaining_budget`), or 0 when the list ends normally.
///
/// Clean-room hardening over the retail walker: a header that overruns the
/// buffer, a payload size the buffer cannot hold, or a size too small to
/// advance the word cursor (`size >> 2 == 0`, which would spin retail
/// forever) all terminate the walk.
pub fn install_chunks<H: InstallHost>(
    host: &mut H,
    slot: &mut SeqSlot,
    stream: &[u8],
    budget: usize,
) -> usize {
    // Word cursor into the stream (retail iVar5, in u32 units).
    let mut word = 0usize;
    let header_at = |w: usize| -> Option<u32> {
        stream
            .get(w * 4..w * 4 + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    while let Some(header) = header_at(word) {
        let size = (header & 0x00FF_FFFF) as usize;
        if size == 0 {
            break; // list terminator
        }
        let ty = (header >> 24) as u8;
        word += 1; // step past the header word
        let payload_start = word * 4;
        let Some(payload) = stream.get(payload_start..payload_start + size) else {
            break; // malformed: declared size overruns the stream
        };
        match ty {
            0 => {
                host.seq_release(slot.id);
                host.raw_copy(CopyDest::SlotDest, payload);
            }
            1 => {
                host.vab_upload(slot.id, payload, None);
                slot.loaded = true;
            }
            2 => host.vram_rect(VramRectKind::Slot, slot.id, payload),
            0xC => host.vram_rect(VramRectKind::Fixed, slot.id, payload),
            3 => {
                // Budget stop: `remaining = budget - payload byte offset`;
                // upload what fits and report the leftover.
                let remaining = budget.saturating_sub(payload_start);
                host.vab_upload(slot.id, payload, Some(remaining));
                slot.loaded = true;
                return size.saturating_sub(remaining);
            }
            4 => {
                host.set_stream_flag();
                if host.transfer_mode() == 0 {
                    host.raw_copy(CopyDest::SlotStage, payload);
                }
            }
            // 5..=0xB reserved (jump-table no-ops); >= 0xD skipped by the
            // retail `sltiu 0xd` range guard. Both just advance.
            _ => {}
        }
        let advance = size >> 2;
        if advance == 0 {
            break; // sub-word size would never advance the cursor
        }
        word += advance;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records every leaf call so tests can assert routing + payloads.
    #[derive(Default)]
    struct SpyHost {
        calls: Vec<String>,
        mode: u32,
    }

    impl InstallHost for SpyHost {
        fn seq_release(&mut self, id: i8) {
            self.calls.push(format!("release id={id}"));
        }
        fn raw_copy(&mut self, dest: CopyDest, data: &[u8]) {
            self.calls.push(format!("copy {dest:?} {data:02X?}"));
        }
        fn vab_upload(&mut self, id: i8, data: &[u8], budget: Option<usize>) {
            self.calls
                .push(format!("vab id={id} len={} budget={budget:?}", data.len()));
        }
        fn vram_rect(&mut self, kind: VramRectKind, id: i8, data: &[u8]) {
            self.calls
                .push(format!("vram {kind:?} id={id} len={}", data.len()));
        }
        fn set_stream_flag(&mut self) {
            self.calls.push("flag".into());
        }
        fn transfer_mode(&self) -> u32 {
            self.mode
        }
    }

    /// `[type | size]` header word + payload bytes.
    fn chunk(ty: u8, payload: &[u8]) -> Vec<u8> {
        let header = ((ty as u32) << 24) | (payload.len() as u32 & 0x00FF_FFFF);
        let mut out = header.to_le_bytes().to_vec();
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn walks_multiple_chunks_and_routes_by_type() {
        let mut stream = Vec::new();
        stream.extend(chunk(0, &[1, 2, 3, 4])); // release + copy
        stream.extend(chunk(1, &[5, 6, 7, 8])); // VAB upload
        stream.extend(chunk(2, &[9, 10, 11, 12])); // VRAM rect (slot)
        stream.extend(chunk(0xC, &[13, 14, 15, 16])); // VRAM rect (fixed)
        stream.extend(&0u32.to_le_bytes()); // terminator
        let mut host = SpyHost::default();
        let mut slot = SeqSlot {
            id: 7,
            loaded: false,
        };
        let leftover = install_chunks(&mut host, &mut slot, &stream, 0x1000);
        assert_eq!(leftover, 0);
        assert_eq!(
            host.calls,
            vec![
                "release id=7".to_string(),
                "copy SlotDest [01, 02, 03, 04]".to_string(),
                "vab id=7 len=4 budget=None".to_string(),
                "vram Slot id=7 len=4".to_string(),
                "vram Fixed id=7 len=4".to_string(),
            ]
        );
        assert!(slot.loaded, "type-1 VAB upload sets the loaded flag");
    }

    #[test]
    fn advance_math_steps_size_over_4_words_past_each_payload() {
        // An 8-byte payload must put the cursor exactly on the next header.
        let mut stream = Vec::new();
        stream.extend(chunk(5, &[0xAA; 8])); // reserved type: advance only
        stream.extend(chunk(0, &[0xBB; 4]));
        stream.extend(&0u32.to_le_bytes());
        let mut host = SpyHost::default();
        let mut slot = SeqSlot::default();
        install_chunks(&mut host, &mut slot, &stream, 0);
        assert_eq!(
            host.calls,
            vec![
                "release id=0".to_string(),
                "copy SlotDest [BB, BB, BB, BB]".to_string(),
            ],
            "reserved type is silent; second chunk decoded at the right word"
        );
    }

    #[test]
    fn type_3_budget_stop_reports_the_leftover_and_sets_loaded() {
        // Header (4 bytes) + 12-byte payload; budget 8 leaves remaining =
        // 8 - 4 = 4 bytes -> leftover = 12 - 4 = 8.
        let mut stream = chunk(3, &[0xCC; 12]);
        stream.extend(chunk(0, &[0xDD; 4])); // never reached
        stream.extend(&0u32.to_le_bytes());
        let mut host = SpyHost::default();
        let mut slot = SeqSlot {
            id: 2,
            loaded: false,
        };
        let leftover = install_chunks(&mut host, &mut slot, &stream, 8);
        assert_eq!(leftover, 8);
        assert_eq!(
            host.calls,
            vec!["vab id=2 len=12 budget=Some(4)".to_string()]
        );
        assert!(slot.loaded);
    }

    #[test]
    fn type_4_sets_the_flag_and_copies_only_in_mode_0() {
        for (mode, expect_copy) in [(0u32, true), (2u32, false)] {
            let mut stream = chunk(4, &[0xEE; 4]);
            stream.extend(&0u32.to_le_bytes());
            let mut host = SpyHost {
                mode,
                ..Default::default()
            };
            let mut slot = SeqSlot::default();
            install_chunks(&mut host, &mut slot, &stream, 0);
            assert_eq!(host.calls[0], "flag");
            assert_eq!(
                host.calls.len(),
                if expect_copy { 2 } else { 1 },
                "mode {mode}: staging copy gated on transfer mode 0"
            );
            if expect_copy {
                assert_eq!(host.calls[1], "copy SlotStage [EE, EE, EE, EE]");
            }
        }
    }

    #[test]
    fn high_types_skip_dispatch_but_still_advance() {
        let mut stream = Vec::new();
        stream.extend(chunk(0xD, &[0x11; 4]));
        stream.extend(chunk(0xFF, &[0x22; 4]));
        stream.extend(chunk(1, &[0x33; 4]));
        stream.extend(&0u32.to_le_bytes());
        let mut host = SpyHost::default();
        let mut slot = SeqSlot::default();
        install_chunks(&mut host, &mut slot, &stream, 0);
        assert_eq!(host.calls, vec!["vab id=0 len=4 budget=None".to_string()]);
    }

    #[test]
    fn malformed_streams_terminate_cleanly() {
        let mut host = SpyHost::default();
        let mut slot = SeqSlot::default();
        // Empty stream.
        assert_eq!(install_chunks(&mut host, &mut slot, &[], 0), 0);
        // Truncated header.
        assert_eq!(install_chunks(&mut host, &mut slot, &[0x04, 0x00], 0), 0);
        // Declared size overruns the buffer.
        let over = chunk(1, &[0x55; 4]);
        assert_eq!(install_chunks(&mut host, &mut slot, &over[..6], 0), 0);
        // Sub-word size (advance 0) would spin retail; we stop after the
        // dispatch instead of looping forever.
        let mut tiny = ((1u32 << 24) | 2).to_le_bytes().to_vec();
        tiny.extend_from_slice(&[0x66; 8]);
        assert_eq!(install_chunks(&mut host, &mut slot, &tiny, 0), 0);
        assert!(host.calls.len() <= 2, "no runaway walk on sub-word sizes");
    }
}
