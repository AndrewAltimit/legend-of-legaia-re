use super::*;

/// Synthetic tables: band slot deltas at (512,256)/(640,256)/(768,256),
/// char 0 eye frames in a strip at (32,128), mouth at (32,196).
fn tables() -> FaceFrameTables {
    let mut t = FaceFrameTables {
        eye_src: [[(0, 0); EYE_FRAME_COUNT]; FACE_CHAR_COUNT],
        mouth_src: [[(0, 0); MOUTH_FRAME_COUNT]; FACE_CHAR_COUNT],
        eye_geo: [FaceGeo {
            dest_x: 0,
            dest_y: 16,
            w: 15,
            h: 17,
        }; FACE_CHAR_COUNT],
        mouth_geo: [FaceGeo {
            dest_x: 4,
            dest_y: 42,
            w: 7,
            h: 16,
        }; FACE_CHAR_COUNT],
        slot_delta: [(512, 256), (640, 256), (768, 256)],
    };
    for c in 0..FACE_CHAR_COUNT {
        for f in 0..EYE_FRAME_COUNT {
            t.eye_src[c][f] = (32, 128 + 17 * f as i16);
        }
        for f in 0..MOUTH_FRAME_COUNT {
            t.mouth_src[c][f] = (32, 196 + 16 * f as i16);
        }
    }
    t
}

fn blink_tracks() -> FaceTracks {
    let mut t = FaceTracks::default();
    // Eyes: frame 1 on clip frames 10..=12.
    t.eyes[0] = FaceTrackRecord {
        frame: 1,
        start: 10,
        end: 12,
    };
    // Mouth: frame 2 on clip frames 4..=6.
    t.mouth[0] = FaceTrackRecord {
        frame: 2,
        start: 4,
        end: 6,
    };
    t
}

#[test]
fn neutral_face_when_no_record_is_active() {
    let t = tables();
    let tr = blink_tracks();
    let stamps = t.stamps(0, 0, Some(&tr), 0, false);
    // Neutral mouth + neutral eyes, both frame 0 (mouth pass first).
    assert_eq!(stamps.len(), 2);
    assert_eq!(
        stamps[0],
        FaceStamp {
            src_x: 544,
            src_y: 452,
            w: 7,
            h: 16,
            dst_x: 516,
            dst_y: 298,
        }
    );
    assert_eq!(
        stamps[1],
        FaceStamp {
            src_x: 544,
            src_y: 384,
            w: 15,
            h: 17,
            dst_x: 512,
            dst_y: 272,
        }
    );
}

#[test]
fn active_records_pick_their_frames() {
    let t = tables();
    let tr = blink_tracks();
    // Frame 11: blink active (eye frame 1), mouth inactive -> neutral.
    let stamps = t.stamps(0, 0, Some(&tr), 11, false);
    assert_eq!(stamps.len(), 2);
    assert_eq!(stamps[0].src_y, 452, "mouth neutral");
    assert_eq!(stamps[1].src_y, 384 + 17, "eye frame 1 row");
    // Frame 5: mouth frame 2 active, eyes neutral.
    let stamps = t.stamps(0, 0, Some(&tr), 5, false);
    assert_eq!(stamps[0].src_y, 452 + 32, "mouth frame 2 row");
    assert_eq!(stamps[1].src_y, 384, "eye neutral");
}

#[test]
fn interval_edges_match_retail_inclusive_compare() {
    let t = tables();
    let tr = blink_tracks();
    assert_eq!(t.stamps(0, 0, Some(&tr), 10, false)[1].src_y, 384 + 17);
    assert_eq!(t.stamps(0, 0, Some(&tr), 12, false)[1].src_y, 384 + 17);
    assert_eq!(t.stamps(0, 0, Some(&tr), 13, false)[1].src_y, 384);
    // An end byte of 0 marks the record unused even at frame 0.
    let mut dead = FaceTracks::default();
    dead.eyes[0] = FaceTrackRecord {
        frame: 1,
        start: 0,
        end: 0,
    };
    assert_eq!(t.stamps(0, 0, Some(&dead), 0, false)[1].src_y, 384);
}

#[test]
fn frame_counter_clamps_at_0xfe() {
    let t = tables();
    let mut tr = FaceTracks::default();
    tr.eyes[0] = FaceTrackRecord {
        frame: 1,
        start: 0xF0,
        end: 0xFE,
    };
    // 0x1234 clamps to 0xFE -> the record is active.
    assert_eq!(t.stamps(0, 0, Some(&tr), 0x1234, false)[1].src_y, 384 + 17);
}

#[test]
fn flag_0x2000_restamps_neutral_mouth_over_active_records() {
    let t = tables();
    let tr = blink_tracks();
    let stamps = t.stamps(0, 0, Some(&tr), 5, true);
    // Active mouth stamp, the forced neutral overwrite, then eyes.
    assert_eq!(stamps.len(), 3);
    assert_eq!(stamps[0].src_y, 452 + 32);
    assert_eq!(stamps[1].src_y, 452, "neutral wins (stamped last)");
    assert_eq!(stamps[1].dst_x, stamps[0].dst_x);
    assert_eq!(stamps[2].src_y, 384, "eye neutral");
}

#[test]
fn terra_and_high_slots_get_no_stamps() {
    let t = tables();
    let tr = blink_tracks();
    assert!(t.stamps(3, 0, Some(&tr), 11, false).is_empty());
    assert!(t.stamps(0, 3, Some(&tr), 11, false).is_empty());
}

#[test]
fn missing_tracks_show_the_neutral_face() {
    let t = tables();
    let stamps = t.stamps(1, 2, None, 50, false);
    assert_eq!(stamps.len(), 2);
    // Slot 2 delta applies to both src and dst.
    assert_eq!(stamps[1].src_x, 768 + 32);
    assert_eq!(stamps[1].dst_x, 768);
}

/// A synthetic override track: mouth frame 2 on (post-shift) frames
/// 4..=6, frame 1 held from 20 to the 0xFF cap.
fn art_track() -> [FaceTrackRecord; ART_MOUTH_RECORD_COUNT] {
    let mut t = [FaceTrackRecord::default(); ART_MOUTH_RECORD_COUNT];
    t[0] = FaceTrackRecord {
        frame: 2,
        start: 4,
        end: 6,
    };
    t[1] = FaceTrackRecord {
        frame: 1,
        start: 20,
        end: 0xFF,
    };
    t
}

#[test]
fn art_window_mouth_uses_override_track_and_halved_counter() {
    let t = tables();
    let tr = blink_tracks();
    let track = art_track();
    // Raw counter 9 -> frame 4: override record 0 (mouth frame 2)
    // active; the entry's mouth record (4..=6) is ignored even though
    // frame 4 falls inside it - the source is swapped, not merged.
    let stamps = t.stamps_with_art_window(
        0,
        0,
        Some(&tr),
        0,
        Some(ArtMouthOverride {
            track: &track,
            counter: 9,
        }),
        false,
    );
    assert_eq!(stamps.len(), 2);
    assert_eq!(stamps[0].src_y, 452 + 32, "override mouth frame 2 row");
    assert_eq!(stamps[1].src_y, 384, "eyes neutral at frame 4");
    // Raw counter 14 -> frame 7: no override record active -> neutral
    // mouth re-stamp.
    let stamps = t.stamps_with_art_window(
        0,
        0,
        Some(&tr),
        0,
        Some(ArtMouthOverride {
            track: &track,
            counter: 14,
        }),
        false,
    );
    assert_eq!(stamps[0].src_y, 452, "neutral mouth between records");
}

#[test]
fn art_window_counter_clocks_the_eye_pass_too() {
    let t = tables();
    let tr = blink_tracks(); // eye record: frame 1 on 10..=12
    let track = art_track();
    // Clip frame would miss the blink (0), but the override counter
    // 22 >> 1 = 11 lands inside it: retail replaces the single frame
    // counter for both passes.
    let stamps = t.stamps_with_art_window(
        0,
        0,
        Some(&tr),
        0,
        Some(ArtMouthOverride {
            track: &track,
            counter: 22,
        }),
        false,
    );
    let eye = stamps.last().unwrap();
    assert_eq!(eye.src_y, 384 + 17, "eye frame 1 at override frame 11");
}

#[test]
fn art_window_counter_shifts_then_clamps() {
    let t = tables();
    let track = art_track();
    // Raw counter 0x7FFF -> >>1 = 0x3FFF -> clamps to 0xFE, which sits
    // inside record 1's 20..=0xFF window (0xFF > the 0xFE clamp, so the
    // record holds forever).
    let stamps = t.stamps_with_art_window(
        0,
        0,
        None,
        0,
        Some(ArtMouthOverride {
            track: &track,
            counter: 0x7FFF,
        }),
        false,
    );
    assert_eq!(stamps[0].src_y, 452 + 16, "held mouth frame 1 row");
}

#[test]
fn art_window_force_neutral_restamps_over_override() {
    let t = tables();
    let track = art_track();
    let stamps = t.stamps_with_art_window(
        0,
        0,
        None,
        0,
        Some(ArtMouthOverride {
            track: &track,
            counter: 9,
        }),
        true,
    );
    assert_eq!(stamps.len(), 3);
    assert_eq!(stamps[0].src_y, 452 + 32, "active override stamp");
    assert_eq!(stamps[1].src_y, 452, "forced neutral wins (stamped last)");
    assert_eq!(stamps[2].src_y, 384, "eye neutral");
}

#[test]
fn art_mouth_track_lookup_gates_on_the_band_window() {
    let mut t = ArtMouthTables {
        tracks: [[[FaceTrackRecord::default(); ART_MOUTH_RECORD_COUNT]; ART_BAND_COUNT];
            FACE_CHAR_COUNT],
    };
    t.tracks[1][0][0] = FaceTrackRecord {
        frame: 3,
        start: 1,
        end: 2,
    };
    assert_eq!(t.track(1, 0x11).unwrap()[0].frame, 3, "band 0x11 = row 0");
    assert!(t.track(1, 0x10).is_none(), "below the window");
    assert!(t.track(1, 0x19).is_none(), "above the window");
    assert!(t.track(3, 0x11).is_none(), "no fourth character row");
    assert!(t.track(2, 0x18).is_some(), "last band in range");
}

#[test]
fn art_mouth_tables_parse_from_a_synthetic_exe() {
    // Minimal PS-X EXE image covering the table block (the last
    // addressed row ends at +0x7B0: char 2 x 0x180 + band 0x18 x 0x30
    // + 16 records x 3).
    let t_addr = 0x8001_0000u32;
    let span = (ART_MOUTH_VA - t_addr) as usize + 0x7B0;
    let mut scus = vec![0u8; 0x800 + span];
    scus[0..8].copy_from_slice(b"PS-X EXE");
    scus[0x18..0x1C].copy_from_slice(&t_addr.to_le_bytes());
    scus[0x1C..0x20].copy_from_slice(&(span as u32).to_le_bytes());
    // One live record: char 1, band 0x13, record 2 = [frame 2, 5, 9].
    let off = (ART_MOUTH_VA - t_addr) as usize + 0x800 + 0x180 + 0x13 * 0x30 + 2 * 3;
    scus[off..off + 3].copy_from_slice(&[2, 5, 9]);
    let t = ArtMouthTables::from_scus(&scus).expect("plausible table");
    let track = t.track(1, 0x13).unwrap();
    assert_eq!(
        track[2],
        FaceTrackRecord {
            frame: 2,
            start: 5,
            end: 9,
        }
    );
    assert!(track[0].end == 0 && track[1].end == 0);
    // Plausibility gate: an out-of-range frame id rejects the table...
    let mut bad = scus.clone();
    bad[off] = MOUTH_FRAME_COUNT as u8;
    assert!(ArtMouthTables::from_scus(&bad).is_none());
    // ...as does a live record selecting the neutral frame, an
    // inverted window, junk in an unused record, or an all-dead table.
    bad[off] = 0;
    assert!(ArtMouthTables::from_scus(&bad).is_none());
    let mut inverted = scus.clone();
    inverted[off + 1] = 10;
    assert!(ArtMouthTables::from_scus(&inverted).is_none());
    let mut junk = scus.clone();
    junk[off + 4] = 7; // record 3: start byte without an end
    assert!(ArtMouthTables::from_scus(&junk).is_none());
    let mut dead = scus.clone();
    dead[off..off + 3].copy_from_slice(&[0, 0, 0]);
    assert!(ArtMouthTables::from_scus(&dead).is_none());
}

#[test]
fn tracks_parse_from_an_entry_header() {
    let mut block = vec![0u8; 0x100];
    // Entry at 0x10: eye record 0 = [2, 4, 6], mouth record 1 = [1, 10, 12].
    block[0x10 + EYE_TRACK_OFFSET..0x10 + EYE_TRACK_OFFSET + 3].copy_from_slice(&[2, 4, 6]);
    block[0x10 + MOUTH_TRACK_OFFSET + 3..0x10 + MOUTH_TRACK_OFFSET + 6]
        .copy_from_slice(&[1, 10, 12]);
    let t = FaceTracks::from_entry(&block, 0x10).expect("in range");
    assert_eq!(
        t.eyes[0],
        FaceTrackRecord {
            frame: 2,
            start: 4,
            end: 6,
        }
    );
    assert_eq!(
        t.mouth[1],
        FaceTrackRecord {
            frame: 1,
            start: 10,
            end: 12,
        }
    );
    assert!(!t.is_empty());
    assert!(FaceTracks::from_entry(&block, 0x60).is_none(), "truncated");
    assert!(FaceTracks::default().is_empty());
}
