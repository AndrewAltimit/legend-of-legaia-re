//! The Disco King's how-to script `FUN_801D0750` in PROT 0980, whole, frame by
//! frame against `DanceTutorial::step`: the actor's state, the prompt cursor
//! and the countdown, every hall word it stores (the dance overlay's whole
//! data window is compared, so a store the port does not make fails), the
//! lines it prints, the cursor it draws, the cue, the chart probes, the music
//! calls and the clip driver.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::dance_tutorial::{
    CaptionSource, DanceTutorial, MUSIC_FADE_ARG, TutorialHall, TutorialMusic,
};
use psx_retail_exec::{A0, A1, A2, A3, SP, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const SCRIPT: u32 = 0x801D_0750;
const ACTOR: u32 = 0x8014_0000;
const PRINT: u32 = 0x8003_6888;
const CURSOR_DRAW: u32 = 0x8002_C488;
const JUDGE: u32 = 0x801D_1960;
const SEQ_VOL: u32 = 0x8006_2004;
const RELEASE: u32 = 0x8002_66E0;
const CLIP_DRIVER: u32 = 0x8002_04F8;
const PAD_EDGE: u32 = 0x8007_B874;
const CUE: u32 = 0x8007_B6D8;
const VOLUME: u32 = 0x8007_B910;
const SEQ_ID: u32 = 0x8007_0576;
const BGM_SOURCE: u32 = 0x8007_056C;
const FRAME_STEP: u32 = 0x1F80_0393;
/// The overlay's data and bss, past its last routine.
const WINDOW: (u32, usize) = (0x801D_4800, 0x1900);
const CURSOR: u32 = 0x801D_607C;
const COUNTDOWN: u32 = 0x801D_6080;

/// Each hall word's address.
fn hall_words(h: &TutorialHall) -> [(u32, u32); 17] {
    [
        (0x801D_5138, h.song_hold),
        (0x801D_5830, h.load_gate),
        (0x801D_5334, h.hall_state),
        (0x801D_53CC, h.score),
        (0x801D_544C, h.gauge as u32),
        (0x801D_5150, h.progress as u32),
        (0x801D_5158, h.count_in),
        (0x801D_5140, h.miss_window as u32),
        (0x801D_5144, h.feedback as u32),
        (0x801D_512C, h.track_flash as u32),
        (0x801D_5170, h.bars),
        (0x801D_5828, h.bar_tail),
        (0x801D_581C, h.phase),
        (0x801D_5820, h.song),
        (0x801D_5824, h.bar_phase),
        (0x801D_534C, h.stock),
        (0x801D_570C, h.landed),
    ]
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Calls {
    /// `(string, x, y)` per printed line.
    lines: Vec<(u32, i32, i32)>,
    cursor: Option<(i32, i32, u32)>,
    probes: Vec<(u32, i32, u32)>,
    volume: Option<(i32, i32, u32)>,
    release: Option<u32>,
    clips: Vec<u32>,
}

#[test]
fn the_how_to_script_runs_as_retail_runs_it() {
    differential!(prot(980, SCRIPT));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(980).expect("PROT 0980"));
    let calls = Rc::new(RefCell::new(Calls::default()));
    let answers = Rc::new(Cell::new(0u32));
    let c = calls.clone();
    cpu.hook(PRINT, move |r, m| {
        let y = m.rd32(r.r[SP] + 0x10) as i32;
        assert_eq!((r.r[A1], r.r[A2]), (0, 0), "the print's colour words");
        c.borrow_mut().lines.push((r.r[A0], r.r[A3] as i32, y));
    });
    let c = calls.clone();
    cpu.hook(CURSOR_DRAW, move |r, _| {
        c.borrow_mut().cursor = Some((r.r[A0] as i32, r.r[A1] as i32, r.r[A2]));
    });
    let (c, a) = (calls.clone(), answers.clone());
    cpu.hook(JUDGE, move |r, _| {
        c.borrow_mut()
            .probes
            .push((r.r[A0], r.r[A1] as i32, r.r[A2]));
        r.r[V0] = a.get() >> (r.r[A2] & 1) & 1;
    });
    let c = calls.clone();
    cpu.hook(SEQ_VOL, move |r, _| {
        c.borrow_mut().volume = Some((r.r[A0] as i32, r.r[A1] as i32, r.r[A2]));
    });
    let c = calls.clone();
    cpu.hook(RELEASE, move |r, _| c.borrow_mut().release = Some(r.r[A0]));
    let c = calls.clone();
    cpu.hook(CLIP_DRIVER, move |r, _| c.borrow_mut().clips.push(r.r[A0]));

    let mut rng = Rng(0x5345_5455);
    let mut seen = [0u32; 0x13];
    let (mut fades, mut releases, mut restarts, mut probed, mut praised, mut scolded) =
        (0, 0, 0, 0, 0, 0);
    for scenario in 0..400 {
        let mut hall = TutorialHall {
            song_hold: rng.below(2),
            load_gate: rng.below(2),
            hall_state: rng.below(0x15),
            score: rng.below(5000),
            gauge: rng.below(6000) as i32 - 1000,
            progress: rng.below(1000) as i32,
            count_in: rng.below(200),
            miss_window: 0,
            feedback: 0,
            track_flash: 0,
            bars: rng.below(50),
            bar_tail: rng.below(4),
            phase: rng.below(9000),
            song: rng.below(0x6500),
            bar_phase: rng.below(0x464),
            stock: rng.below(4),
            landed: rng.below(3),
        };
        let mut t = DanceTutorial::new();
        // A scenario in three starts from the prompt and is answered through;
        // the rest are dropped on a state, the table's edges included.
        t.state = if scenario % 3 == 0 {
            0
        } else {
            rng.below(0x17) as i16 - 2
        };
        t.cursor = rng.next();
        t.countdown = rng.below(0x40) as i32;
        let volume = rng.next() & 0xFFFF;
        let seq = rng.next() as u16;
        cpu.mem.wr32(VOLUME, volume);
        cpu.mem.wr16(SEQ_ID, seq);
        cpu.mem.wr16(ACTOR + 0x9C, t.state as u16);
        cpu.mem.wr32(CURSOR, t.cursor);
        cpu.mem.wr32(COUNTDOWN, t.countdown as u32);
        for (a, v) in hall_words(&hall) {
            cpu.mem.wr32(a, v);
        }
        for frame in 0..160 {
            // What the hall's own routines do between the script's frames.
            match rng.below(12) {
                0 => hall.progress = 890 + rng.below(20) as i32,
                1 => hall.miss_window = [1, 0x1E, 0x20, 0x21][rng.below(4) as usize],
                2 => {
                    hall.feedback = [0x3C, 1, 2, -5][rng.below(4) as usize];
                    hall.landed = rng.below(3);
                }
                3 => hall.track_flash = [0x10, 1, 3][rng.below(3) as usize],
                4 => hall.gauge = rng.below(5000) as i32 - 500,
                5 => hall.score = rng.below(5000),
                _ => hall.progress = hall.progress.wrapping_add(3),
            }
            // A script that has run off its table is dropped back on it.
            if !(0..0x12).contains(&t.state) && rng.below(4) != 0 {
                t.state = rng.below(0x12) as i16;
                cpu.mem.wr16(ACTOR + 0x9C, t.state as u16);
            }
            for (a, v) in hall_words(&hall) {
                cpu.mem.wr32(a, v);
            }
            let pad = match rng.below(8) {
                0 => [0x10u32, 0x20, 0x40, 0x80][rng.below(4) as usize],
                1 => 0x1000,
                2 => 0x4000,
                3 => 0x5000 | (rng.next() & 0xF0),
                4 => rng.next() & 0xFFFF,
                _ => 0,
            };
            let dt = [1u8, 2, 3, 3, 3, 0, 0x40][rng.below(7) as usize];
            let answer = rng.below(4);
            answers.set(answer);
            cpu.mem.wr32(PAD_EDGE, pad);
            cpu.mem.wr8(FRAME_STEP, dt);
            cpu.mem.wr16(CUE, 0);
            *calls.borrow_mut() = Calls::default();
            let before = cpu.mem.read(WINDOW.0, WINDOW.1);
            let was = t.state;

            assert_eq!(cpu.call(SCRIPT, &[ACTOR]), Stop::Returned);

            let mut port_probes = Vec::new();
            let f = t.step(&mut hall, pad as u16, dt, |lane, symbol| {
                port_probes.push((0, lane, 0x100 | (u32::from(symbol) - 1)));
                answer >> (symbol - 1) & 1 != 0
            });
            let ctx = format!("scenario {scenario} frame {frame} state {was:#x} pad {pad:#x}");

            // The whole data window: what was there, with the port's words.
            let mut want = before;
            let mut put = |a: u32, v: u32| {
                let o = (a - WINDOW.0) as usize;
                want[o..o + 4].copy_from_slice(&v.to_le_bytes());
            };
            for (a, v) in hall_words(&hall) {
                put(a, v);
            }
            put(CURSOR, t.cursor);
            put(COUNTDOWN, t.countdown as u32);
            let got = cpu.mem.read(WINDOW.0, WINDOW.1);
            assert!(
                got == want,
                "{ctx}: {:?}",
                crate::common::diff_offsets(&got, &want)
            );
            assert_eq!(cpu.mem.rd16(ACTOR + 0x9C) as i16, t.state, "{ctx}");
            assert_eq!(cpu.mem.rd16(CUE), f.cue.unwrap_or(0), "{ctx}: the cue");

            let port = Calls {
                lines: f
                    .lines
                    .iter()
                    .map(|l| {
                        let text = match l.source {
                            CaptionSource::Table(va) => cpu.mem.rd32(va),
                            CaptionSource::Inline(va) => va,
                        };
                        (text, i32::from(l.x), i32::from(l.y))
                    })
                    .collect(),
                cursor: f
                    .cursor_pos
                    .map(|(x, y)| (i32::from(x), i32::from(y), 0x4E)),
                probes: port_probes,
                volume: match f.music {
                    Some(TutorialMusic::Fade { shift }) => Some((
                        i32::from(seq as i16),
                        ((volume << shift) as i32) >> 16,
                        MUSIC_FADE_ARG,
                    )),
                    _ => None,
                },
                release: (f.music == Some(TutorialMusic::Release)).then_some(BGM_SOURCE),
                clips: vec![ACTOR],
            };
            assert_eq!(*calls.borrow(), port, "{ctx}");
            assert_eq!(
                f.restart,
                matches!(was, 8 | 0xE) || (matches!(was, 0xC | 0x11) && t.state != was),
                "{ctx}: the restart"
            );
            assert!(f.lines.iter().all(|l| match l.source {
                CaptionSource::Table(va) => cpu.mem.rd32(va) != 0,
                CaptionSource::Inline(_) => true,
            }));

            if (0..0x13).contains(&was) {
                seen[was as usize] += 1;
            }
            fades += u32::from(matches!(f.music, Some(TutorialMusic::Fade { .. })));
            releases += u32::from(f.music == Some(TutorialMusic::Release));
            restarts += u32::from(f.restart);
            probed += u32::from(port.probes.len() == 2 && !f.lines.is_empty());
            praised += u32::from(f.feedback == Some(true));
            scolded += u32::from(f.feedback == Some(false));
        }
    }
    assert!(seen.iter().all(|&n| n > 50), "{seen:?}");
    assert!(
        fades > 20 && releases > 5 && restarts > 50 && probed > 50 && praised > 20 && scolded > 50,
        "fades {fades} releases {releases} restarts {restarts} probed {probed} praised {praised} scolded {scolded}"
    );
}
