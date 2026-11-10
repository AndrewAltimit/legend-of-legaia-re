//! The party HUD's status element `FUN_8002C2E4` against
//! `status_effects::status_icon`: every status word over a standing and a
//! fallen slot, with the icon drawer `FUN_8002C488` and the number drawer
//! `FUN_80034B78` hooked. Compared: which of the two is called, the sprite id,
//! and the pen offsets the port's doc states.

use crate::common::scus_cpu;
use legaia_engine_battle_vm::status_effects::{StatusIcon, status_icon};
use psx_retail_exec::{A0, A1, A2, A3, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const ELEMENT: u32 = 0x8002_C2E4;
const ICON: u32 = 0x8002_C488;
const NUMBER: u32 = 0x8003_4B78;
const DISPLAY: u32 = 0x8008_4140;
const STRIDE: u32 = 0x414;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Draw {
    Icon(i32, i32, u32),
    Number(u32, u32, i32, i32),
}

#[test]
fn the_status_element_is_retails() {
    differential!(scus(ELEMENT));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let draws = Rc::new(RefCell::new(Vec::new()));
    let d = draws.clone();
    cpu.hook(ICON, move |r, _| {
        d.borrow_mut()
            .push(Draw::Icon(r.r[A0] as i32, r.r[A1] as i32, r.r[A2]));
    });
    let d = draws.clone();
    cpu.hook(NUMBER, move |r, _| {
        d.borrow_mut().push(Draw::Number(
            r.r[A0],
            r.r[A1],
            r.r[A2] as i32,
            r.r[A3] as i32,
        ));
    });
    let (x, y) = (40i32, 100i32);
    let (mut sprites, mut none) = (0, 0);
    for slot in 0..3u32 {
        let rec = DISPLAY + slot * STRIDE;
        for word in 0..=0xFFFFu32 {
            for hp in [0u16, 1, 0x1234] {
                let level = (word as u8).wrapping_mul(7);
                cpu.mem.wr16(rec + 0x6F6, word as u16);
                cpu.mem.wr16(rec + 0x6CE, hp);
                cpu.mem.wr8(rec + 0x6F8, level);
                draws.borrow_mut().clear();
                assert_eq!(
                    cpu.call(ELEMENT, &[slot, x as u32, y as u32]),
                    Stop::Returned
                );
                let want = match status_icon(word as u16, hp != 0) {
                    StatusIcon::BaseWithCount => vec![
                        Draw::Icon(x + 0x3B, y + 2, 0x0A),
                        Draw::Number(u32::from(level), 2, x + 0x4B, y),
                    ],
                    StatusIcon::Sprite(id) => {
                        sprites += 1;
                        vec![Draw::Icon(x + 0x33, y - 4, u32::from(id))]
                    }
                    StatusIcon::None => {
                        none += 1;
                        vec![]
                    }
                };
                assert_eq!(*draws.borrow(), want, "slot {slot} word {word:#x} hp {hp}");
            }
        }
    }
    assert!(sprites > 100_000 && none > 0, "{sprites} {none}");
}
