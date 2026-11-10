//! The per-frame pad pump `FUN_8001822C` against `RetailPadState::pump`, as
//! one stream of calls over generated libpad reports: digital and analog
//! pads on port 0, a second pad on port 1, missing pads, the stick fold-ins
//! on both sides of their dead zone, opposed directions, and every frame
//! step. Compared after each call: the held, changed and pressed words, the
//! 32-vsync held word and the repeat pulse. The debug word `_DAT_8007B98C`
//! stays zero, as it ships, so the dev hotkey tail is not entered.

use crate::common::{Rng, scus_cpu};
use legaia_engine_system::retail_pad::{PadReport, RetailPadState};
use psx_retail_exec::Stop;

const PUMP: u32 = 0x8001_822C;
const MEMSET: u32 = 0x8005_6788;
const VIBRATION: u32 = 0x8001_8F94;
const REPORTS: u32 = 0x8008_40F8;
const HELD: u32 = 0x8007_B850;
const CHANGED: u32 = 0x8007_B7C4;
const PRESSED: u32 = 0x8007_B874;
const HELD_32: u32 = 0x8007_B938;
const PULSE: u32 = 0x8007_B93C;
const FRAME_STEP: u32 = 0x1F80_0393;

fn report(rng: &mut Rng, buttons: u16) -> PadReport {
    let axis = |rng: &mut Rng| match rng.below(6) {
        0 => 0x2F,
        1 => 0x30,
        2 => 0xD0,
        3 => 0xD1,
        4 => rng.next() as u8,
        _ => 0x80,
    };
    PadReport {
        status: if rng.below(12) == 0 { 0xFF } else { 0 },
        id: match rng.below(8) {
            0 => 0x73,
            1 => 0x53,
            2 => 0x12,
            3 => 0x7F,
            _ => 0x41,
        },
        buttons: [!((buttons >> 8) as u8), !(buttons as u8)],
        sticks: std::array::from_fn(|_| axis(rng)),
    }
}

#[test]
fn the_pad_pump_is_retails() {
    differential!(scus(PUMP));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.hook(MEMSET, |_, _| {});
    cpu.hook(VIBRATION, |_, _| {});
    let mut rng = Rng(0x5041_4450);
    let mut pad = RetailPadState::default();
    let (mut pulses, mut analog, mut socd) = (0, 0, 0);
    let (mut held, mut steady) = (0u16, 0u32);
    for call in 0..60_000 {
        // Stretches of one clean report, so buttons stay down past the
        // 32-vsync window; between them the buttons and the pad both churn.
        if steady == 0 && rng.below(200) == 0 {
            steady = 40 + rng.below(80);
        }
        let p0 = if steady > 0 {
            steady -= 1;
            PadReport::digital(held)
        } else {
            if rng.below(24) == 0 {
                held = rng.next() as u16 & rng.next() as u16;
            } else if rng.below(6) == 0 {
                held ^= 1 << rng.below(16);
            }
            report(&mut rng, held)
        };
        let other = rng.next() as u16;
        let p1 = report(&mut rng, other);
        let step = match rng.below(10) {
            0 => 0,
            1 => 3,
            2 => 2,
            _ => 1,
        };
        for (base, p) in [(REPORTS, &p0), (REPORTS + 0x22, &p1)] {
            cpu.mem.wr8(base, p.status);
            cpu.mem.wr8(base + 1, p.id);
            cpu.mem.wr8(base + 2, p.buttons[0]);
            cpu.mem.wr8(base + 3, p.buttons[1]);
            cpu.mem.load(base + 4, &p.sticks);
        }
        cpu.mem.wr8(FRAME_STEP, step);
        assert_eq!(cpu.call(PUMP, &[]), Stop::Returned);
        pad.pump(&p0, &p1, false, u32::from(step));
        let retail = [HELD, CHANGED, PRESSED, HELD_32, PULSE].map(|a| cpu.mem.rd32(a));
        let port = [
            pad.held,
            pad.changed,
            pad.pressed,
            pad.held_32,
            pad.repeat_pulse,
        ];
        assert_eq!(
            retail, port,
            "call {call}: {p0:?} {p1:?} step {step}\n retail {retail:x?}\n port   {port:x?}"
        );
        pulses += u32::from(retail[4] != 0);
        analog += u32::from(p0.status == 0 && p0.id & 0xF0 == 0x70);
        socd += u32::from(held & 0xA000 == 0xA000 || held & 0x5000 == 0x5000);
    }
    assert!(
        pulses > 200 && analog > 5000 && socd > 5000,
        "{pulses} {analog} {socd}"
    );
}
