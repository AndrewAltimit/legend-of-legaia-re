//! The escape timer's HUD actor `FUN_801D2EBC` (PROT 0897), run frame by frame
//! over whole countdowns against `EscapeTimer::tick` and `EscapeTimerHud::step`:
//! the counter, the armed word, the flags it fires through `FUN_8003CE08`, the
//! actor's `+0x54` phase machine and kill bit, the digits it hands the draw
//! calls and the ink `_DAT_8007B454`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::escape_timer::{EscapeTimer, EscapeTimerHud};
use psx_retail_exec::{A0, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const HUD: u32 = 0x801D_2EBC;
const FLAG_SET: u32 = 0x8003_CE08;
const DRAW_MINUTES: u32 = 0x8003_4B78;
const DRAW_TWO_DIGITS: u32 = 0x8003_4E4C;
const PLAYER_PTR: u32 = 0x8007_C364;
const PAD_HOLD: u32 = 0x8007_B6B4;
const WARP_TIMER: u32 = 0x8007_B6B0;
const CLOCK: u32 = 0x8008_4570;
const LATCH: u32 = 0x8007_3ED4;
const REMAINING: u32 = 0x8008_45A0;
const ARMED: u32 = 0x8008_45B8;
const THRESHOLD: u32 = 0x8008_45BC;
const FLAG_WORD: u32 = 0x8008_45C0;
const INK: u32 = 0x8007_B454;
const FRAME_STEP: u32 = 0x1F80_0393;
const ACTOR: u32 = 0x8012_0000;
const PLAYER: u32 = 0x8013_0000;

#[test]
fn the_countdown_runs_as_retail_runs_it() {
    differential!(prot(897, HUD));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let flags: Rc<RefCell<Vec<u16>>> = Rc::new(RefCell::new(Vec::new()));
    let f = flags.clone();
    cpu.hook(FLAG_SET, move |r, _| f.borrow_mut().push(r.r[A0] as u16));
    let digits: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
    for at in [DRAW_MINUTES, DRAW_TWO_DIGITS] {
        let d = digits.clone();
        cpu.hook(at, move |r, _| d.borrow_mut().push(r.r[A0] as i32));
    }
    for draw in [0x8003_C1F8, 0x8003_4B6C, 0x8002_C69C] {
        cpu.stub(draw, 0);
    }
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    let mut rng = Rng(0x4553_4341);
    let (mut expired, mut killed, mut busy_frames, mut failures) = (0, 0, 0, Vec::new());
    for run in 0..300 {
        let duration = match rng.below(3) {
            0 => 1 + rng.below(60),
            1 => 1 + rng.below(0x900),
            _ => 1 + rng.below(0x8000),
        } as i32;
        let threshold = rng.below(duration as u32 + 200) as i32;
        let flag_word = rng.next();
        let mut clock = rng.next() >> 4;
        cpu.mem.wr32(CLOCK, clock);
        cpu.mem.wr32(LATCH, clock);
        cpu.mem.wr32(REMAINING, duration as u32);
        cpu.mem.wr32(ARMED, duration as u32);
        cpu.mem.wr32(THRESHOLD, threshold as u32);
        cpu.mem.wr32(FLAG_WORD, flag_word);
        for b in 0..0x80 {
            cpu.mem.wr8(ACTOR + b, 0);
        }
        cpu.mem.wr16(ACTOR + 0x14, 0x20);
        cpu.mem.wr16(ACTOR + 0x16, 0x10);
        let mut timer = EscapeTimer {
            remaining: duration,
            warn_threshold: threshold,
            armed: true,
        };
        let mut hud = EscapeTimerHud::default();
        // Fast-forward big counts with large clock deltas.
        let big = duration > 0x900;
        for frame in 0..4000 {
            let delta = if big { rng.below(400) } else { rng.below(4) };
            let step = 1 + rng.below(4) as u8;
            let busy = rng.below(10) == 0;
            let busy_kind = rng.below(3);
            cpu.mem.wr32(
                PLAYER + 0x10,
                if busy && busy_kind == 0 { 0x8_0000 } else { 0 },
            );
            cpu.mem.wr32(PAD_HOLD, u32::from(busy && busy_kind == 1));
            cpu.mem
                .wr32(WARP_TIMER, u32::from(busy && busy_kind == 2) * 5);
            cpu.mem.wr8(FRAME_STEP, step);
            clock = clock.wrapping_add(delta);
            cpu.mem.wr32(CLOCK, clock);
            flags.borrow_mut().clear();
            digits.borrow_mut().clear();
            assert_eq!(cpu.call(HUD, &[ACTOR]), Stop::Returned);
            let r_flags = flags.borrow().clone();
            let r_digits = digits.borrow().clone();
            let r_kill = cpu.mem.rd32(ACTOR + 0x10) & 8 != 0;
            let retail = (
                cpu.mem.rd32(REMAINING) as i32,
                cpu.mem.rd32(ARMED) != 0,
                r_flags,
                cpu.mem.rd16(ACTOR + 0x54),
                cpu.mem.rd16(ACTOR + 0x68) as i16,
                r_digits,
                if busy { 0 } else { cpu.mem.rd32(INK) },
                r_kill,
            );

            let ev = timer.tick(delta as i32, flag_word, busy);
            let mut p_flags = Vec::new();
            p_flags.extend(ev.expiry_flag);
            p_flags.extend(ev.warning_flag);
            let (p_digits, ink, kill) = if busy {
                (Vec::new(), 0, false)
            } else {
                let f = hud.step(&mut timer, step);
                (
                    vec![f.digits.0, f.digits.1, f.digits.2],
                    f.ink as u32,
                    f.teardown,
                )
            };
            let port = (
                timer.remaining,
                timer.armed,
                p_flags,
                u16::from(hud.phase),
                hud.hold,
                p_digits,
                ink,
                kill,
            );
            busy_frames += u32::from(busy);
            if retail != port {
                if failures.len() < 6 {
                    failures.push(format!(
                        "run {run} frame {frame}: dur {duration} thr {threshold} delta {delta} \
                         step {step} busy {busy}\n retail {retail:?}\n port   {port:?}"
                    ));
                }
                break;
            }
            if r_kill {
                killed += 1;
                break;
            }
        }
        expired += u32::from(!timer.armed);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        expired > 150 && killed > 150 && busy_frames > 1000,
        "{expired} {killed} {busy_frames}"
    );
}
