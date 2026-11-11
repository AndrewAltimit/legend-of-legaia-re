//! The per-frame facial animator `FUN_8004C7B4` (`SCUS_942.54`) against
//! `legaia_actor_formats::face_anim::FaceFrameTables::stamps_with_art_window`.
//!
//! Each stamp retail makes is a `MoveImage(rect, x, y)` (`FUN_80058490`),
//! hooked and logged in call order; the port's stamps come out in the same
//! order (the mouth pass, its neutral fallback, the eye pass, its neutral
//! fallback). Both tables are the executable's own; the action entry's two
//! tracks, the clip frame, the character flag and the victory-window gates
//! are generated.

use crate::common::{Rng, scus, scus_cpu};
use legaia_actor_formats::face_anim::{
    ART_BAND_COUNT, ART_BAND_FIRST, ArtMouthOverride, ArtMouthTables, EYE_FRAME_COUNT,
    FACE_CHAR_COUNT, FACE_SLOT_COUNT, FaceFrameTables, FaceStamp, FaceTrackRecord, FaceTracks,
    MOUTH_FRAME_COUNT,
};
use psx_retail_exec::{A0, A1, A2, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const ANIMATOR: u32 = 0x8004_C7B4;
const MOVE_IMAGE: u32 = 0x8005_8490;
const CURSOR: u32 = 0x1F80_03A0;
const SEQ_BYTE: u32 = 0x8007_BD71;
const VICTORY_CTX_PTR: u32 = 0x8007_BD24;
const BATTLE_FLAGS: u32 = 0x8007_BD60;
const VICTORY_COUNTER: u32 = 0x8007_BD02;
const ACTOR_TABLE: u32 = 0x801C_9370;
const CHAR_RECORDS: u32 = 0x8008_4140;

const ENTRY: u32 = 0x8012_0000;
const VICTORY_CTX: u32 = 0x8012_1000;
const ACTORS: u32 = 0x8012_2000;
const PACKETS: u32 = 0x8013_0000;

#[test]
fn the_face_animator_matches_retail() {
    differential!(scus(ANIMATOR));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let exe = scus().expect("disc present");
    let tables = FaceFrameTables::from_scus(&exe).expect("face tables");
    let art = ArtMouthTables::from_scus(&exe).expect("override tables");
    let log = Rc::new(RefCell::new(Vec::<FaceStamp>::new()));
    let l = log.clone();
    cpu.hook(MOVE_IMAGE, move |r, m| {
        let rect = r.r[A0];
        l.borrow_mut().push(FaceStamp {
            src_x: m.rd16(rect),
            src_y: m.rd16(rect + 2),
            w: m.rd16(rect + 4),
            h: m.rd16(rect + 6),
            dst_x: r.r[A1] as u16,
            dst_y: r.r[A2] as u16,
        });
    });
    cpu.mem.wr32(VICTORY_CTX_PTR, VICTORY_CTX);
    for slot in 0..4u32 {
        cpu.mem.wr32(ACTOR_TABLE + 4 * slot, ACTORS + 0x200 * slot);
    }

    let mut rng = Rng(0x4641_4345);
    let (mut overridden, mut neutral_forced, mut stamped) = (0, 0, 0usize);
    for round in 0..40000 {
        let c = rng.below(FACE_CHAR_COUNT as u32) as usize;
        let p = rng.below(FACE_SLOT_COUNT as u32) as usize;
        let record = |rng: &mut Rng, frames: usize| FaceTrackRecord {
            frame: rng.below(frames as u32) as u8,
            start: rng.below(0x40) as u8,
            end: if rng.below(5) == 0 {
                0
            } else {
                rng.below(0x100) as u8
            },
        };
        let tracks = FaceTracks {
            eyes: std::array::from_fn(|_| record(&mut rng, EYE_FRAME_COUNT)),
            mouth: std::array::from_fn(|_| record(&mut rng, MOUTH_FRAME_COUNT)),
        };
        let clip_frame = match rng.below(4) {
            0 => rng.next() as i16,
            1 => 0xF0 + rng.below(0x20) as i16,
            _ => rng.below(0x60) as i16,
        };
        let flag = rng.below(4) == 0;
        // The four victory-window gates, each held or not.
        let all = rng.below(3) == 0;
        let seq = if all || rng.below(2) == 0 {
            0xFE
        } else {
            rng.below(0xFE) as u8
        };
        let victory = if all || rng.below(2) == 0 {
            1 + rng.below(4) as u16
        } else {
            0
        };
        let bflags = if all || rng.below(2) == 0 {
            0x80 | rng.next() as u8
        } else {
            rng.next() as u8 & 0x7F
        };
        let band = if all || rng.below(2) == 0 {
            ART_BAND_FIRST + rng.below(ART_BAND_COUNT as u32) as u8
        } else {
            [0x10u8, 0x19, 0, 0xFF][rng.below(4) as usize]
        };
        let counter = rng.next() as u16;
        let window = seq == 0xFE
            && victory != 0
            && bflags & 0x80 != 0
            && (ART_BAND_FIRST..ART_BAND_FIRST + ART_BAND_COUNT as u8).contains(&band);

        for (k, r) in tracks.eyes.iter().chain(tracks.mouth.iter()).enumerate() {
            let at = ENTRY + 0x8C + 3 * k as u32;
            cpu.mem.wr8(at, r.frame);
            cpu.mem.wr8(at + 1, r.start);
            cpu.mem.wr8(at + 2, r.end);
        }
        let rec = CHAR_RECORDS + 0x6C0 + 0x414 * c as u32;
        let word = rng.next() & !0x2000 | if flag { 0x2000 } else { 0 };
        cpu.mem.wr32(rec, word);
        cpu.mem.wr8(SEQ_BYTE, seq);
        cpu.mem.wr16(VICTORY_CTX + 0x6CE, victory);
        cpu.mem.wr8(BATTLE_FLAGS, bflags);
        cpu.mem.wr8(ACTORS + 0x200 * p as u32 + 0x1DB, band);
        cpu.mem.wr16(VICTORY_COUNTER, counter);
        cpu.mem.wr32(CURSOR, PACKETS);
        log.borrow_mut().clear();
        assert_eq!(
            cpu.call(
                ANIMATOR,
                &[p as u32, c as u32, clip_frame as u16 as u32, ENTRY]
            ),
            Stop::Returned,
            "round {round}"
        );
        let over = window.then(|| ArtMouthOverride {
            track: art.track(c, band).expect("an override track"),
            counter,
        });
        let want = tables.stamps_with_art_window(c, p, Some(&tracks), clip_frame, over, flag);
        let ctx = format!(
            "round {round} char {c} slot {p} frame {clip_frame} flag {flag} window {window} \
             band {band:#x} counter {counter:#x} {tracks:?}"
        );
        assert_eq!(*log.borrow(), want, "stamps: {ctx}");
        assert_eq!(
            cpu.mem.rd32(CURSOR),
            PACKETS + 0x10,
            "the scratch rects: {ctx}"
        );
        overridden += u32::from(window);
        neutral_forced += u32::from(flag);
        stamped += want.len();
    }
    assert!(
        overridden > 4000 && neutral_forced > 5000 && stamped > 100_000,
        "{overridden} {neutral_forced} {stamped}"
    );
}
