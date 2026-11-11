//! The Muscle Dome contest hub `FUN_801CF870` in PROT 0977, whole, frame by
//! frame through a first visit against `FirstVisitHub::tick`: the arm, every
//! counter, the screens each arm draws and the level it draws them at, the
//! announcer lines it starts, and the packed cursor it rewrites at its tail
//! (`cursor_repack`). The drive's in-flight flag and the pad are scripted.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::muscle_dome::cursor_repack;
use legaia_engine_minigames::muscle_ringside::{FirstVisitArm, FirstVisitHub};
use psx_retail_exec::{A0, A1, A2, A3, SP, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const HUB: u32 = 0x801C_F870;
const DRAW: u32 = 0x801D_050C;
const COURSE_CARD: u32 = 0x801D_042C;
const ROUND_CARD: u32 = 0x801D_02F0;
const BACKDROP: u32 = 0x801D_00F8;
const FIGHT: u32 = 0x801D_1510;
const XA_START: u32 = 0x8003_D53C;
const XA_SEEK: u32 = 0x8003_EAE4;
const LOADER: u32 = 0x8005_2770;
const STATE: u32 = 0x801D_1A78;
/// `0x801D1A7C`, `0x801D1A80`, `0x801D1A84`, `0x801D1A88`, `0x801D1A70`,
/// `0x801D1A8C`, in `FirstVisitHub::words` order.
const WORDS: [u32; 6] = [
    0x801D_1A7C,
    0x801D_1A80,
    0x801D_1A84,
    0x801D_1A88,
    0x801D_1A70,
    0x801D_1A8C,
];
const COURSE: u32 = 0x801D_1A90;
const ROUND: u32 = 0x801D_1A94;
const LATCH: u32 = 0x801D_1AE0;
const WORD: u32 = 0x8007_BAC0;
const PAD_EDGE: u32 = 0x8007_B874;
const PAD_REPEAT: u32 = 0x8007_B938;
const XA_BUSY: u32 = 0x8007_BC20;
const LOAD_STATE: u32 = 0x8007_B648;
const FRAME_STEP: u32 = 0x1F80_0393;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Drawn {
    /// `FUN_801D050C(x, y, record, level, scale)` calls.
    sprites: Vec<(i32, i32, u32, i32, i32)>,
    course_card: Option<i32>,
    round_card: Option<i32>,
    backdrop: Option<i32>,
    xa: Option<(u32, u32, u32)>,
    seek: Option<(u32, u32)>,
    fight: bool,
}

#[test]
fn a_first_visit_runs_as_retail_runs_it() {
    differential!(prot(977, HUB));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(977).expect("PROT 0977"));
    let drawn = Rc::new(RefCell::new(Drawn::default()));
    let c = drawn.clone();
    cpu.hook(DRAW, move |r, m| {
        c.borrow_mut().sprites.push((
            r.r[A0] as i32,
            r.r[A1] as i32,
            r.r[A2],
            r.r[A3] as i32,
            m.rd32(r.r[SP] + 0x10) as i32,
        ));
    });
    let c = drawn.clone();
    cpu.hook(COURSE_CARD, move |_, m| {
        c.borrow_mut().course_card = Some(m.rd32(WORDS[2]) as i32);
    });
    let c = drawn.clone();
    cpu.hook(ROUND_CARD, move |r, _| {
        c.borrow_mut().round_card = Some(r.r[A0] as i32)
    });
    let c = drawn.clone();
    cpu.hook(BACKDROP, move |r, _| {
        c.borrow_mut().backdrop = Some(r.r[A0] as i32)
    });
    let c = drawn.clone();
    cpu.hook(FIGHT, move |_, _| c.borrow_mut().fight = true);
    let c = drawn.clone();
    cpu.hook(XA_START, move |r, m| {
        c.borrow_mut().xa = Some((r.r[A0], r.r[A1], r.r[A2]));
        // The clip starter raises the in-flight flag itself (`0x8003D658`).
        m.wr32(XA_BUSY, 2);
    });
    let c = drawn.clone();
    cpu.hook(XA_SEEK, move |r, _| {
        c.borrow_mut().seek = Some((r.r[A0], r.r[A1]))
    });
    cpu.stub(LOADER, 0);

    let mut rng = Rng(0x4855_4231);
    let (mut fights, mut skipped, mut waited, mut doubles) = (0, 0, 0, 0);
    for visit in 0..300 {
        let mut port = FirstVisitHub::new();
        let course = rng.below(4);
        let word = rng.next();
        cpu.mem.wr32(STATE, 0);
        for a in WORDS {
            cpu.mem.wr32(a, 0);
        }
        cpu.mem.wr32(COURSE, course);
        cpu.mem.wr32(ROUND, 0);
        cpu.mem.wr32(LATCH, 0);
        cpu.mem.wr32(WORD, word);
        cpu.mem.wr32(PAD_REPEAT, 0);
        let press_odds = [0, 6, 40][visit % 3];
        let mut busy = 0i32;
        for frame in 0..3000 {
            let dt = 1 + rng.below(3) as u8;
            let pad: u32 = if press_odds != 0 && rng.below(press_odds) == 0 {
                [0x10, 0x20, 0x40, 0x80, 0x04, 0x100, 0x08, 0x1000][rng.below(8) as usize]
            } else {
                0
            };
            // One visit in five presses on the very frame the ROUND card's
            // hold runs out.
            let pad = if visit % 5 == 4
                && port.arm() == FirstVisitArm::RoundIn
                && port.words()[5] + i32::from(dt) > 0x3C
            {
                0x20
            } else {
                pad
            };
            // The announcer line runs `busy` more ticks.
            busy = (busy - i32::from(dt)).max(0);
            port.seat_xa_busy(busy + i32::from(dt));
            cpu.mem.wr32(XA_BUSY, if busy > 0 { 2 } else { 0 });
            // The loader the hub kicks is done long before the hub asks.
            cpu.mem.wr8(LOAD_STATE, 0x80);
            cpu.mem.wr32(PAD_EDGE, pad);
            cpu.mem.wr8(FRAME_STEP, dt);
            *drawn.borrow_mut() = Drawn::default();
            let was = port.arm();
            let hold_before = port.words()[5];

            assert_eq!(cpu.call(HUB, &[]), Stop::Returned);
            port.tick(dt, pad as u16);

            let ctx = format!("visit {visit} frame {frame} arm {was:?} dt {dt} pad {pad:#x}");
            let got = drawn.borrow().clone();
            let mut state = cpu.mem.rd32(STATE);
            // Retail's arm `0x15` adds one for the hold running out and one
            // more for a press on that same frame (`0x801CFFB8..0x801D0000`),
            // which parks the hub on an arm it does not have. The port takes
            // the frame as one step.
            if state == 0x17 {
                assert!(
                    was == FirstVisitArm::RoundIn
                        && pad & 0xF4 != 0
                        && hold_before + i32::from(dt) > 0x3C,
                    "{ctx}: a double step"
                );
                doubles += 1;
                state = 0x16;
                cpu.mem.wr32(STATE, state);
            }
            let arm = match state {
                0 => FirstVisitArm::IntroIn,
                1 => FirstVisitArm::IntroHold,
                2 => FirstVisitArm::IntroOut,
                3 => FirstVisitArm::TitleZoom,
                4 => FirstVisitArm::CardIn,
                5 => FirstVisitArm::CardHold,
                6 => FirstVisitArm::Drain,
                0x14 => FirstVisitArm::Return,
                0x15 => FirstVisitArm::RoundIn,
                0x16 if got.fight => FirstVisitArm::Done,
                0x16 => FirstVisitArm::RoundOut,
                other => panic!("{ctx}: state {other:#x}"),
            };
            assert_eq!(arm, port.arm(), "{ctx}: the arm");
            assert_eq!(
                WORDS.map(|a| cpu.mem.rd32(a) as i32),
                port.words(),
                "{ctx}: the counters"
            );
            assert_eq!(
                cpu.mem.rd32(WORD),
                cursor_repack(word, course as usize, 0),
                "{ctx}: the cursor"
            );

            // What the arm drew.
            let f = port.frame();
            let mut sprites = Vec::new();
            if let Some(level) = f.intro {
                sprites.push((0xA0, 0x78, 3, level, 0x1000));
            }
            if let Some(scale) = f.title_scale {
                sprites.push((0xA0, 0x40, 4, 0x80, scale));
                sprites.push((0xA8, 0x48, 0x804, 0x80, 0x1000));
            }
            let xa = port.take_xa().map(|x| {
                (
                    u32::from(x.clip),
                    u32::from(x.channel),
                    u32::from(x.duration_sectors),
                )
            });
            let want = Drawn {
                sprites,
                course_card: f.course_card,
                round_card: f.round_card,
                backdrop: (f.backdrop != 0).then_some(f.backdrop),
                xa,
                // Arm `6` parks the drive on the ROUND line as it leaves.
                seek: (was == FirstVisitArm::Drain && port.arm() == FirstVisitArm::Return)
                    .then_some((0x1F, 0)),
                fight: port.done(),
            };
            assert_eq!(got, want, "{ctx}: the frame's calls");
            if xa.is_some() {
                // The line runs on, sometimes past the arms that wait on it.
                busy = rng.below(400) as i32;
            }
            waited += u32::from(
                busy > 0
                    && matches!(
                        port.arm(),
                        FirstVisitArm::TitleZoom | FirstVisitArm::CardIn | FirstVisitArm::RoundOut
                    ),
            );
            skipped += u32::from(was == FirstVisitArm::CardHold && pad & 0xF4 != 0);
            if port.done() {
                fights += 1;
                assert_eq!(cpu.mem.rd8(LOAD_STATE), 0x80);
                break;
            }
        }
        assert!(port.done(), "visit {visit}: the fight starts");
    }
    assert_eq!(fights, 300);
    assert!(
        skipped > 20 && waited > 500 && doubles > 0,
        "skipped {skipped} waited {waited} doubles {doubles}"
    );
}
