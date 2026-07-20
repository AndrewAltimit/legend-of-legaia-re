//! Disc-gated oracle for the retail `St` sector ring ([`legaia_mdec::st_ring`]).
//!
//! Streams real `MOV/MV*.STR` sectors off a user-supplied disc image through
//! the ring exactly as the STR overlay does - one 2048-byte sector at a time,
//! draining and freeing each frame as it completes - and cross-checks the
//! result against the simple [`StrFrameAssembler`] path. The two demuxers agree
//! frame-for-frame and byte-for-byte, which is what makes the ring port a
//! drop-in for the assembler rather than a second, divergent reading of the
//! format.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset - no Sony bytes here.

use legaia_mdec::st_ring::{RETAIL_RING_SLOTS, StRing, StStatus};
use legaia_mdec::str_sector::StrFrameAssembler;

/// Locate a movie's extent on the disc, if the env var points at one.
fn open_movie(name: &str) -> Option<(legaia_iso::raw::RawDisc, u32, u32)> {
    let path = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let path = legaia_iso::raw::resolve_disc_path(std::path::Path::new(&path)).ok()?;
    let mut disc = legaia_iso::raw::RawDisc::open(&path).ok()?;
    let volume = legaia_iso::iso9660::read_volume(&mut disc).ok()?;
    let files = legaia_iso::iso9660::walk_files(&mut disc, &volume.root).ok()?;
    let (_, rec) = files
        .into_iter()
        .find(|(p, _)| p.to_ascii_uppercase().contains(name))?;
    Some((disc, rec.lba, rec.size))
}

#[test]
fn ring_demux_matches_the_frame_assembler_on_a_real_movie() {
    // MV1.STR is the intro; short enough to walk in full, and its sectors are
    // XA-interleaved so the video gate gets exercised on real audio sectors.
    let Some((mut disc, lba, size)) = open_movie("MV1.STR") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or MV1.STR not found");
        return;
    };
    let sector_count = (size as usize).div_ceil(2048) as u32;

    let mut ring = StRing::retail();
    ring.set_stream(1);
    // No end frame: play to the ring's own wrap-stop, which is what the intro
    // slot does (its end frame is the movie's last frame).
    ring.set_mask(false, 0, 0xFFFF);

    let mut assembler = StrFrameAssembler::new();
    let mut ring_frames: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut asm_frames: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut accepted = 0usize;
    let mut skipped = 0usize;

    for i in 0..sector_count {
        let Ok(data) = disc.read_sector(lba + i) else {
            break;
        };

        // Reference path.
        if let Ok(Some((hdr, bytes))) = assembler.push_sector(&data) {
            asm_frames.push((hdr.frame_number, bytes));
        }

        // Ring path.
        let step = ring.deliver_sector(&data);
        match step.status {
            StStatus::Accepted => accepted += 1,
            StStatus::NotForStream => skipped += 1,
            StStatus::RingFull | StStatus::RingWrapBlocked => {
                panic!(
                    "ring stalled at sector {i}: {:?} - frames are not being freed",
                    step.status
                )
            }
            other => panic!("unexpected demux status at sector {i}: {other:?}"),
        }
        if step.frame_ready {
            let frame = ring
                .get_next()
                .expect("frame_ready implies a readable frame");
            ring_frames.push((frame.frame_number, ring.frame_bytes(&frame).to_vec()));
            assert!(
                ring.free_ring(frame.slot),
                "slot {} was handed out",
                frame.slot
            );
        }
    }

    assert!(accepted > 0, "no video sectors accepted");
    assert!(
        skipped > 0,
        "MV1.STR is XA-interleaved; audio sectors must be gated out"
    );
    assert_eq!(
        ring_frames.len(),
        asm_frames.len(),
        "ring and assembler disagree on the frame count"
    );
    assert!(ring_frames.len() > 100, "MV1.STR is a full-length movie");

    for (i, (ring_frame, asm_frame)) in ring_frames.iter().zip(&asm_frames).enumerate() {
        assert_eq!(ring_frame.0, asm_frame.0, "frame {i}: sequence number");
        assert_eq!(
            ring_frame.1, asm_frame.1,
            "frame {i} (#{}): demuxed bytes differ",
            ring_frame.0
        );
    }

    // A frame never spans a ring wrap: the payload run is contiguous, which is
    // what lets the decoder read it in place.
    assert_eq!(ring.slot_count(), RETAIL_RING_SLOTS);

    eprintln!(
        "[ok] MV1.STR: {} frames demuxed ({accepted} video / {skipped} non-video sectors)",
        ring_frames.len()
    );
}

#[test]
fn seek_to_start_frame_lands_on_a_mid_file_segment() {
    // MV3.STR carries four cutscenes as abutting frame ranges; the play loop
    // arms the seek so a segment starts exactly on its first frame.
    let Some((mut disc, lba, size)) = open_movie("MV3.STR") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or MV3.STR not found");
        return;
    };
    let sector_count = (size as usize).div_ceil(2048) as u32;

    // fmv_id 3's segment starts at frame 0xE2 per the dispatch table.
    const START: u32 = 0xE2;
    let mut ring = StRing::retail();
    ring.set_stream(1);
    ring.set_mask(true, START, 0xFFFF);

    let mut first_frame = None;
    for i in 0..sector_count {
        let Ok(data) = disc.read_sector(lba + i) else {
            break;
        };
        let step = ring.deliver_sector(&data);
        if step.frame_ready {
            let frame = ring.get_next().expect("readable frame");
            first_frame = Some(frame.frame_number);
            ring.free_ring(frame.slot);
            break;
        }
    }

    assert_eq!(
        first_frame,
        Some(START),
        "the armed seek must discard every frame before the segment start"
    );
    eprintln!("[ok] MV3.STR: seek landed on frame {START:#x}");
}
