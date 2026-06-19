//! Disc-gated validation of the battle facial-animation data
//! (`legaia_asset::face_anim`): the static `SCUS_942.54` face-frame tables
//! and the player battle files' per-action eye/mouth tracks.
//!
//! Pins the live-traced anchors (the `autorun_battle_moveimage_trace.lua`
//! stamps documented in `docs/formats/battle-data-pack.md` § Facial
//! animation tracks): Vahn band-slot-0 eyes `(544,384) 15x17 -> (512,272)`
//! and mouth `(544,452) 7x16 -> (516,298)` - plus the track census the
//! stamp selection relies on:
//!
//! - every track record's `frame` id indexes inside its character's frame
//!   table (eyes < 8, mouth < 6);
//! - no record with a non-zero activity window selects frame 0 (the
//!   neutral face is fallback-only);
//! - the **idle** entry (slot 0) carries empty tracks in all four files -
//!   resting party faces are the re-stamped neutral frames;
//! - Vahn / Noa / Gala each have action entries with live eye AND mouth
//!   records (the blink / talk content); Terra's tracks are all empty
//!   (retail skips char 3 entirely);
//! - the art-bank records' embedded entries (record `+0xB0` / `+0xBC` -
//!   the tracks the animator reads while a materialized art clip plays)
//!   pass the same census, and nearly every Vahn / Noa / Gala art record
//!   carries live records;
//! - every stamp the selection can produce lands inside the member's
//!   128x256 texture band, for every character x band slot.
//!
//! Skips and passes when `LEGAIA_DISC_BIN` / `extracted/` are absent.

use std::path::PathBuf;

use legaia_asset::face_anim::{
    ART_BAND_COUNT, ART_BAND_FIRST, ART_BAND_LAST, ArtMouthOverride, ArtMouthTables,
    EYE_FRAME_COUNT, FACE_CHAR_COUNT, FACE_SLOT_COUNT, FaceFrameTables, FaceTracks,
    MOUTH_FRAME_COUNT, battle_face_tracks,
};

fn extracted_root() -> Option<PathBuf> {
    ["extracted", "../extracted", "../../extracted"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.join("SCUS_942.54").is_file() && p.join("PROT").is_dir())
}

const PLAYER_FILES: [(&str, &str); 4] = [
    ("Vahn", "0863_edstati3.BIN"),
    ("Noa", "0864_edstati3.BIN"),
    ("Gala", "0865_battle_data.BIN"),
    ("Terra", "0866_battle_data.BIN"),
];

fn gate() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/ missing");
        return None;
    };
    Some(root)
}

#[test]
fn face_frame_tables_parse_with_the_live_traced_anchors() {
    let Some(root) = gate() else { return };
    let scus = std::fs::read(root.join("SCUS_942.54")).expect("read SCUS");
    let t = FaceFrameTables::from_scus(&scus).expect("parse face tables");

    // Band origins: the documented texture-band rule (x = 0x200 + p*0x80,
    // y = 0x100).
    assert_eq!(t.slot_delta, [(512, 256), (640, 256), (768, 256)]);

    // The live-traced Vahn band-slot-0 stamps: neutral (frame 0) eyes and
    // mouth.
    let stamps = t.stamps(0, 0, None, 0, false);
    assert_eq!(stamps.len(), 2, "neutral pass = one mouth + one eye stamp");
    let (mouth, eyes) = (stamps[0], stamps[1]);
    assert_eq!(
        (
            eyes.src_x, eyes.src_y, eyes.w, eyes.h, eyes.dst_x, eyes.dst_y
        ),
        (544, 384, 15, 17, 512, 272),
        "Vahn eyes (544,384) 15x17 -> (512,272)"
    );
    assert_eq!(
        (
            mouth.src_x,
            mouth.src_y,
            mouth.w,
            mouth.h,
            mouth.dst_x,
            mouth.dst_y
        ),
        (544, 452, 7, 16, 516, 298),
        "Vahn mouth (544,452) 7x16 -> (516,298)"
    );

    // Every reachable stamp stays inside its member band (the strip and
    // the live face rows are both band-resident).
    for c in 0..FACE_CHAR_COUNT {
        for p in 0..FACE_SLOT_COUNT {
            let (bx, by) = (512 + 128 * p as u32, 256u32);
            let mut tracks = FaceTracks::default();
            for f in 0..EYE_FRAME_COUNT.max(MOUTH_FRAME_COUNT) {
                tracks.eyes[0] = legaia_asset::face_anim::FaceTrackRecord {
                    frame: (f % EYE_FRAME_COUNT) as u8,
                    start: 0,
                    end: 1,
                };
                tracks.mouth[0] = legaia_asset::face_anim::FaceTrackRecord {
                    frame: (f % MOUTH_FRAME_COUNT) as u8,
                    start: 0,
                    end: 1,
                };
                for s in t.stamps(c, p, Some(&tracks), 0, false) {
                    for (x, y) in [(s.src_x, s.src_y), (s.dst_x, s.dst_y)] {
                        assert!(
                            x as u32 >= bx
                                && (x as u32 + s.w as u32) <= bx + 128
                                && y as u32 >= by
                                && (y as u32 + s.h as u32) <= by + 256,
                            "char {c} slot {p} frame {f}: stamp ({x},{y}) {}x{} leaves the band",
                            s.w,
                            s.h
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn art_mouth_override_table_anchors() {
    let Some(root) = gate() else { return };
    let scus = std::fs::read(root.join("SCUS_942.54")).expect("read SCUS");
    let t = ArtMouthTables::from_scus(&scus).expect("parse art-mouth override table");
    let frames = FaceFrameTables::from_scus(&scus).expect("parse face tables");

    let mut live_total = 0usize;
    for c in 0..FACE_CHAR_COUNT {
        let mut live_for_char = 0usize;
        for band in ART_BAND_FIRST..=ART_BAND_LAST {
            let track = t.track(c, band).expect("band in window");
            for r in track {
                if r.end == 0 {
                    assert_eq!(
                        (r.frame, r.start),
                        (0, 0),
                        "char {c} band {band:#04x}: unused record carries data"
                    );
                    continue;
                }
                // Every live record selects a non-neutral in-range mouth
                // frame over a well-formed window.
                assert!(
                    (1..MOUTH_FRAME_COUNT).contains(&(r.frame as usize)),
                    "char {c} band {band:#04x}: mouth frame id {} out of range",
                    r.frame
                );
                assert!(
                    r.start <= r.end,
                    "char {c} band {band:#04x}: inverted window {}..{}",
                    r.start,
                    r.end
                );
                live_total += 1;
                live_for_char += 1;
            }
        }
        assert!(
            live_for_char > 0,
            "char {c}: no live override records at all"
        );
    }
    // Stable disc invariant: the retail table carries exactly 40 live
    // records across the 3 x 8 (char x band) rows.
    assert_eq!(live_total, 40, "live override-record census");
    // Retail leaves specific bands without a win-quote mouth flap: Vahn's
    // 0x12 / 0x14 / 0x18 and Noa's 0x15 rows are all-zero.
    for (c, band) in [(0usize, 0x12u8), (0, 0x14), (0, 0x18), (1, 0x15)] {
        assert!(
            t.track(c, band).unwrap().iter().all(|r| r.end == 0),
            "char {c} band {band:#04x}: expected an empty override row"
        );
    }
    // Out-of-window ids never resolve a track (the retail +0x1DB gate).
    assert!(t.track(0, ART_BAND_FIRST - 1).is_none());
    assert!(t.track(0, ART_BAND_LAST + 1).is_none());
    assert_eq!(
        ART_BAND_COUNT,
        (ART_BAND_LAST - ART_BAND_FIRST + 1) as usize
    );

    // Every stamp the override window can produce lands inside the
    // member's 128x256 texture band, for every char x band slot x counter.
    for c in 0..FACE_CHAR_COUNT {
        for p in 0..FACE_SLOT_COUNT {
            let (bx, by) = (512 + 128 * p as u32, 256u32);
            for band in ART_BAND_FIRST..=ART_BAND_LAST {
                let track = t.track(c, band).unwrap();
                for counter in (0..=0x200u16).step_by(2) {
                    let stamps = frames.stamps_with_art_window(
                        c,
                        p,
                        None,
                        0,
                        Some(ArtMouthOverride { track, counter }),
                        false,
                    );
                    for s in &stamps {
                        for (x, y) in [(s.src_x, s.src_y), (s.dst_x, s.dst_y)] {
                            assert!(
                                x as u32 >= bx
                                    && (x as u32 + s.w as u32) <= bx + 128
                                    && y as u32 >= by
                                    && (y as u32 + s.h as u32) <= by + 256,
                                "char {c} slot {p} band {band:#04x} counter {counter}: \
                                 stamp ({x},{y}) {}x{} leaves the band",
                                s.w,
                                s.h
                            );
                        }
                    }
                }
            }
        }
    }
    eprintln!("[ok] art-mouth override table: {live_total} live records");
}

#[test]
fn player_face_tracks_census() {
    let Some(root) = gate() else { return };
    for (name, file) in PLAYER_FILES {
        let path = root.join("PROT").join(file);
        if !path.exists() {
            eprintln!("[skip] {} missing", path.display());
            continue;
        }
        let raw = std::fs::read(&path).expect("read player file");
        let tracks = battle_face_tracks(&raw).expect("decode face tracks");
        let mut live_eye = 0usize;
        let mut live_mouth = 0usize;
        for (slot, tr) in tracks.iter().enumerate() {
            let Some(tr) = tr else { continue };
            if slot == 0 {
                assert!(
                    tr.is_empty(),
                    "{name}: idle entry carries face records (expected the \
                     neutral-only resting face)"
                );
            }
            for r in &tr.eyes {
                if r.end != 0 {
                    assert!(
                        (r.frame as usize) < EYE_FRAME_COUNT,
                        "{name} slot {slot}: eye frame id {} out of range",
                        r.frame
                    );
                    assert_ne!(
                        r.frame, 0,
                        "{name} slot {slot}: active eye record selects the neutral frame"
                    );
                    live_eye += 1;
                }
            }
            for r in &tr.mouth {
                if r.end != 0 {
                    assert!(
                        (r.frame as usize) < MOUTH_FRAME_COUNT,
                        "{name} slot {slot}: mouth frame id {} out of range",
                        r.frame
                    );
                    assert_ne!(
                        r.frame, 0,
                        "{name} slot {slot}: active mouth record selects the neutral frame"
                    );
                    live_mouth += 1;
                }
            }
        }
        if name == "Terra" {
            assert_eq!(
                (live_eye, live_mouth),
                (0, 0),
                "Terra's tracks are empty (retail skips char 3)"
            );
        } else {
            assert!(
                live_eye > 0 && live_mouth > 0,
                "{name}: expected live eye + mouth records (got {live_eye}/{live_mouth})"
            );
        }
        eprintln!("[ok] {name}: {live_eye} eye + {live_mouth} mouth records");
    }
}

#[test]
fn art_bank_face_tracks_census() {
    let Some(root) = gate() else { return };
    for (ci, (name, file)) in PLAYER_FILES.iter().enumerate() {
        let path = root.join("PROT").join(file);
        if !path.exists() {
            eprintln!("[skip] {} missing", path.display());
            continue;
        }
        let raw = std::fs::read(&path).expect("read player file");
        let record0 =
            legaia_asset::battle_char_assembly::decode_record0(&raw).expect("decode record[0]");
        let bank =
            legaia_asset::battle_char_assembly::art_animation_bank(&record0).expect("parse bank");
        let mut live_eye = 0usize;
        let mut live_mouth = 0usize;
        let mut clips_with_tracks = 0usize;
        for rec in &bank {
            let face = rec.face.unwrap_or_else(|| {
                panic!("{name} art record {}: truncated face tracks", rec.index)
            });
            if !face.is_empty() {
                clips_with_tracks += 1;
            }
            for r in &face.eyes {
                if r.end == 0 {
                    continue;
                }
                assert!(
                    (1..EYE_FRAME_COUNT).contains(&(r.frame as usize)),
                    "{name} art record {}: eye frame id {} out of range / neutral",
                    rec.index,
                    r.frame
                );
                assert!(
                    r.start <= r.end,
                    "{name} art record {}: inverted eye window {}..{}",
                    rec.index,
                    r.start,
                    r.end
                );
                live_eye += 1;
            }
            for r in &face.mouth {
                if r.end == 0 {
                    continue;
                }
                assert!(
                    (1..MOUTH_FRAME_COUNT).contains(&(r.frame as usize)),
                    "{name} art record {}: mouth frame id {} out of range / neutral",
                    rec.index,
                    r.frame
                );
                assert!(
                    r.start <= r.end,
                    "{name} art record {}: inverted mouth window {}..{}",
                    rec.index,
                    r.start,
                    r.end
                );
                live_mouth += 1;
            }
        }
        // Stable disc invariants: the art clips are face-rich for the
        // animated trio (nearly every record carries live records); Terra's
        // bank is all-empty, matching her empty record[0] tracks (retail
        // skips char 3 in the animator anyway).
        let expect = [
            (33usize, 32usize, 71usize, 86usize), // Vahn
            (35, 33, 75, 53),                     // Noa
            (32, 30, 57, 56),                     // Gala
            (9, 0, 0, 0),                         // Terra
        ][ci];
        assert_eq!(
            (bank.len(), clips_with_tracks, live_eye, live_mouth),
            expect,
            "{name}: art-bank face-track census"
        );
        eprintln!(
            "[ok] {name} (char {ci}): {} art records, {clips_with_tracks} with non-empty \
             tracks, {live_eye} eye + {live_mouth} mouth records",
            bank.len()
        );
    }
}

#[test]
fn swing_face_tracks_index_in_range() {
    let Some(root) = gate() else { return };
    for (name, file) in PLAYER_FILES.iter().take(3) {
        let path = root.join("PROT").join(file);
        if !path.exists() {
            continue;
        }
        let raw = std::fs::read(&path).expect("read player file");
        let pack = legaia_asset::battle_data_pack::parse(&raw).expect("parse pack");
        // Default (unequipped) sections - the same fallback the engine uses
        // when the roster record is empty.
        let swings =
            legaia_asset::battle_char_assembly::swing_battle_animations(&raw, &pack, &[0; 5])
                .expect("decode swings");
        for s in &swings {
            let Some(face) = &s.face else {
                panic!("{name} swing slot {:#x}: truncated face tracks", s.slot)
            };
            for r in &face.eyes {
                assert!(
                    r.end == 0 || (r.frame as usize) < EYE_FRAME_COUNT,
                    "{name} swing slot {:#x}: eye frame id {} out of range",
                    s.slot,
                    r.frame
                );
            }
            for r in &face.mouth {
                assert!(
                    r.end == 0 || (r.frame as usize) < MOUTH_FRAME_COUNT,
                    "{name} swing slot {:#x}: mouth frame id {} out of range",
                    s.slot,
                    r.frame
                );
            }
        }
        eprintln!(
            "[ok] {name}: {} swing entries with face tracks",
            swings.len()
        );
    }
}
