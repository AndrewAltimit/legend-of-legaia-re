//! The retail play loop over the sector ring ([`legaia_mdec::str_player`]).
//!
//! The point of the port is that a *segment* of a movie is playable: one
//! `MVn.STR` can carry several cutscenes as abutting frame ranges (`MV3.STR`
//! carries four), and the FMV dispatch slot's `start_frame` / `end_frame` are
//! what select one. The simple [`StrFrameAssembler`] path has no notion of a
//! window and hands back every frame in the file.
//!
//! The synthetic tests here build STR sector headers by hand - no Sony bytes.
//! The disc-gated tail unpacks the real STRv2 VLC table out of the user's own
//! STR/MDEC overlay and skips (passing) when `LEGAIA_DISC_BIN` is unset.

use legaia_mdec::str_player::{Bitstream, FmvSlot, PumpIdle, StrPlayer, seek_sector_offset};

/// Build one 2048-byte STR video sector.
fn video_sector(frame_number: u32, chunk: u16, chunks: u16, payload_byte: u8) -> Vec<u8> {
    let mut s = vec![0u8; 2048];
    s[0x00..0x02].copy_from_slice(&0x0160u16.to_le_bytes());
    s[0x02..0x04].copy_from_slice(&0x8001u16.to_le_bytes());
    s[0x04..0x06].copy_from_slice(&chunk.to_le_bytes());
    s[0x06..0x08].copy_from_slice(&chunks.to_le_bytes());
    s[0x08..0x0C].copy_from_slice(&frame_number.to_le_bytes());
    // One chunk of real payload per sector; the pump truncates to this.
    s[0x0C..0x10].copy_from_slice(&(chunks as u32 * 2016).to_le_bytes());
    s[0x10..0x12].copy_from_slice(&320u16.to_le_bytes());
    s[0x12..0x14].copy_from_slice(&240u16.to_le_bytes());
    s[32..].fill(payload_byte);
    s
}

/// A ten-frame movie, one sector per frame, frame numbers `1..=10`.
fn ten_frame_movie() -> Vec<Vec<u8>> {
    (1..=10u32)
        .map(|f| video_sector(f, 0, 1, f as u8))
        .collect()
}

/// Run a slot over a sector list and collect the frame numbers that came out.
fn play(slot: FmvSlot, sectors: &[Vec<u8>]) -> Vec<u32> {
    let mut player = StrPlayer::open(slot, Bitstream::Iki);
    let mut seen = Vec::new();
    'outer: for sector in sectors {
        player.deliver_sector(sector);
        loop {
            match player.next_frame() {
                Ok(frame) => {
                    // The payload really is the frame's own bytes, truncated to
                    // the declared bitstream length.
                    assert_eq!(frame.bitstream.len(), 2016);
                    assert!(
                        frame
                            .bitstream
                            .iter()
                            .all(|&b| b == frame.frame_number as u8),
                        "frame {} carries another frame's payload",
                        frame.frame_number
                    );
                    seen.push(frame.frame_number);
                }
                Err(PumpIdle::NeedSectors) => break,
                Err(PumpIdle::Finished) => break 'outer,
            }
        }
    }
    seen
}

#[test]
fn a_whole_file_slot_plays_every_frame() {
    // The non-vacuous baseline: without a window, nothing is dropped. If this
    // ever fails alongside the windowed test, the ring is broken, not the
    // window.
    let seen = play(FmvSlot::whole_file(320, 240), &ten_frame_movie());
    assert_eq!(seen, (1..=10).collect::<Vec<_>>());
}

#[test]
fn start_frame_seeks_past_the_earlier_segment() {
    // `StSetStream`'s armed seek drops whole sectors until the segment's first
    // frame - the behaviour `StrFrameAssembler` has no way to express.
    let slot = FmvSlot {
        start_frame: 4,
        ..FmvSlot::whole_file(320, 240)
    };
    let seen = play(slot, &ten_frame_movie());
    assert_eq!(seen, (4..=10).collect::<Vec<_>>());
    assert_eq!(
        seen.first(),
        Some(&4),
        "frames 1..3 belong to a prior segment"
    );
}

#[test]
fn end_frame_stops_after_decoding_its_own_frame() {
    // The end frame is *inclusive*: `FUN_801CF740` latches end-of-stream on the
    // frame whose number reaches the slot's `+0x0C`, and the play loop exits
    // only after that frame has been decoded and displayed.
    let slot = FmvSlot {
        start_frame: 4,
        end_frame: 7,
        ..FmvSlot::whole_file(320, 240)
    };
    let seen = play(slot, &ten_frame_movie());
    assert_eq!(seen, vec![4, 5, 6, 7]);
}

#[test]
fn a_finished_player_pumps_nothing_further() {
    let slot = FmvSlot {
        end_frame: 2,
        ..FmvSlot::whole_file(320, 240)
    };
    let mut player = StrPlayer::open(slot, Bitstream::Iki);
    for sector in ten_frame_movie() {
        player.deliver_sector(&sector);
        while player.next_frame().is_ok() {}
    }
    assert!(player.finished());
    assert_eq!(player.next_frame().unwrap_err(), PumpIdle::Finished);
}

#[test]
fn aborting_ends_playback_the_way_a_pad_skip_does() {
    let mut player = StrPlayer::open(FmvSlot::whole_file(320, 240), Bitstream::Iki);
    player.deliver_sector(&video_sector(1, 0, 1, 1));
    assert!(player.next_frame().is_ok());
    player.abort();
    player.deliver_sector(&video_sector(2, 0, 1, 2));
    assert_eq!(player.next_frame().unwrap_err(), PumpIdle::Finished);
}

#[test]
fn the_code_buffer_alternates_every_frame_starting_at_one() {
    // `FUN_801CFA14` toggles *before* use, so frame 1 lands in buffer 1.
    let mut player = StrPlayer::open(FmvSlot::whole_file(320, 240), Bitstream::Iki);
    let mut bufs = Vec::new();
    for sector in ten_frame_movie() {
        player.deliver_sector(&sector);
        while let Ok(frame) = player.next_frame() {
            bufs.push(frame.code_buf);
        }
    }
    assert_eq!(bufs, vec![1, 0, 1, 0, 1, 0, 1, 0, 1, 0]);
}

#[test]
fn multi_sector_frames_reassemble_in_the_ring() {
    // Three-sector frames exercise the ring's contiguous-run requirement.
    let mut sectors = Vec::new();
    for f in 1..=4u32 {
        for c in 0..3u16 {
            sectors.push(video_sector(f, c, 3, f as u8));
        }
    }
    let mut player = StrPlayer::open(FmvSlot::whole_file(320, 240), Bitstream::Iki);
    let mut seen = Vec::new();
    for sector in &sectors {
        player.deliver_sector(sector);
        while let Ok(frame) = player.next_frame() {
            assert_eq!(frame.bitstream.len(), 3 * 2016);
            seen.push(frame.frame_number);
        }
    }
    assert_eq!(seen, vec![1, 2, 3, 4]);
}

#[test]
fn the_mv3_segment_seeks_match_the_retail_frame_ranges() {
    // The four `MV3.STR` cutscenes, as the dispatch table encodes them.
    for (start, sectors) in [(1u32, 0i32), (0xE2, 2250), (0x1A5, 4200), (0x27C, 6350)] {
        assert_eq!(seek_sector_offset(start), sectors);
    }
}

/// Read the raw STR/MDEC overlay (PROT 0970) out of `extracted/PROT.DAT`.
fn str_overlay() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let prot = [
        std::path::PathBuf::from("extracted/PROT.DAT"),
        std::path::PathBuf::from("../../extracted/PROT.DAT"),
    ]
    .into_iter()
    .find(|p| p.is_file())?;
    let mut archive = legaia_prot::archive::Archive::open(&prot).ok()?;
    let entry = archive.entries.get(970)?.clone();
    let mut buf = Vec::new();
    archive.read_entry(&entry, &mut buf).ok()?;
    Some(buf)
}

#[test]
fn strv2_vlc_table_unpacks_to_its_exact_footprint() {
    use legaia_mdec::strv2_table;
    let Some(overlay) = str_overlay() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT.DAT missing");
        return;
    };
    let table = strv2_table::unpack_from_overlay(&overlay, 0x801C_E818)
        .expect("FUN_801F1A00 unpacks its own packed stream");
    // The table's byte footprint ends flush against FUN_801F1A00 - the geometry
    // that pins the 0x8800-entry size.
    assert_eq!(table.len(), strv2_table::STRV2_TABLE_U16S);
    assert_eq!(
        strv2_table::STRV2_TABLE_VA as usize + table.len() * 2,
        0x801F_1A00
    );
    // A VLC lookup table is not mostly zeros, and it is not constant.
    let nonzero = table.iter().filter(|&&v| v != 0).count();
    assert!(nonzero > table.len() / 8, "only {nonzero} non-zero entries");
    assert!(table.iter().any(|&v| v != table[0]));
}
