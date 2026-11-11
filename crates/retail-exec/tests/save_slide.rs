//! The save screen's slide-in primitive `FUN_801E1C1C` (PROT 0899) against
//! `legaia_engine_menus::save_select::interpolate_anim` and the panel
//! geometry `legaia_engine_ui` places its slid elements at: the "Now
//! checking" dialog (mode 0), the confirm dialog (mode 3, with its hand
//! cursor) and the card work panel (mode 4), each at its retail start and
//! target over random timers. Every call outside the routine is stubbed but
//! the panel drawer `FUN_801E36C4`, the centring text drawer `FUN_801E3EE0`
//! and the icon blit `FUN_8002C488`, which are recorded.

use crate::common::{Rng, STACK_TOP, disc, scus_cpu};
use legaia_engine_menus::save_select::interpolate_anim;
use legaia_engine_ui::{
    CARD_WORK_PANEL, CARD_WORK_SLIDE_START_X, CONFIRM_DIALOG_CENTER_X,
    CONFIRM_DIALOG_SLIDE_START_Y, CONFIRM_DIALOG_SLIDE_TARGET_Y, NOW_CHECKING_SLIDE_START_X,
    NOW_CHECKING_SLIDE_TARGET_X, SaveMenuAtlasRects, confirm_dialog_hand_draw_for,
};
use psx_retail_exec::{A0, A1, A2, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const SLIDE: u32 = 0x801E_1C1C;
const SLIDE_END: u32 = SLIDE + 4520;
const PANEL: u32 = 0x801E_36C4;
const TEXT: u32 = 0x801E_3EE0;
const ICON: u32 = 0x8002_C488;
/// The confirm dialog's cursor word (`_DAT_801F01FC`): row `(w + 1) & 1`.
const CONFIRM_CURSOR: u32 = 0x801F_01FC;
/// The "Now checking" dialog's parked centre rect height and its `y` above
/// the slide's (`FUN_801E36C4(x, y - 0xF, w, 26)`).
const NOW_CHECKING_H: i32 = 26;
const NOW_CHECKING_DY: i32 = -15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Call {
    Panel(i32, i32, i32, i32),
    Text(i32, i32),
    Icon(i32, i32, u32),
}

#[test]
fn the_slid_elements_land_where_the_port_puts_them() {
    differential!(prot(899, SLIDE));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &disc().expect("disc").read_entry(899).expect("PROT 0899"),
    );
    cpu.stub_outside = Some((SLIDE, SLIDE_END));
    let log = Rc::new(RefCell::new(Vec::<Call>::new()));
    let l = log.clone();
    cpu.hook(PANEL, move |r, _| {
        let a = |k: usize| r.r[A0 + k] as i32;
        l.borrow_mut().push(Call::Panel(a(0), a(1), a(2), a(3)));
    });
    let l = log.clone();
    cpu.hook(TEXT, move |r, _| {
        l.borrow_mut()
            .push(Call::Text(r.r[A1] as i32, r.r[A2] as i32));
    });
    let l = log.clone();
    cpu.hook(ICON, move |r, _| {
        l.borrow_mut()
            .push(Call::Icon(r.r[A0] as i32, r.r[A1] as i32, r.r[A2]));
    });
    let rects = SaveMenuAtlasRects::default();
    let mut rng = Rng(0x534C_4944);
    for case in 0..3000 {
        let t = if rng.below(4) == 0 {
            [0, 0x1000][rng.below(2) as usize]
        } else {
            rng.below(0x1001)
        };
        let mode = [0u32, 3, 4][rng.below(3) as usize];
        let (start, target) = match mode {
            0 => (
                (NOW_CHECKING_SLIDE_START_X, 112),
                (NOW_CHECKING_SLIDE_TARGET_X, 112),
            ),
            3 => (
                (CONFIRM_DIALOG_CENTER_X, CONFIRM_DIALOG_SLIDE_START_Y),
                (CONFIRM_DIALOG_CENTER_X, CONFIRM_DIALOG_SLIDE_TARGET_Y),
            ),
            _ => (
                (CARD_WORK_SLIDE_START_X, 112),
                (NOW_CHECKING_SLIDE_TARGET_X, 112),
            ),
        };
        let cursor = rng.below(4);
        cpu.mem.wr32(CONFIRM_CURSOR, cursor);
        cpu.mem.wr32(STACK_TOP + 0x10, target.0 as u32);
        cpu.mem.wr32(STACK_TOP + 0x14, target.1 as u32);
        log.borrow_mut().clear();
        let args = [mode, t, start.0 as u32, start.1 as u32];
        assert_eq!(cpu.call(SLIDE, &args), Stop::Returned, "case {case}");
        let (x, y) = interpolate_anim(start, target, t as u16);
        let calls = log.borrow().clone();
        let panels: Vec<Call> = calls
            .iter()
            .copied()
            .filter(|c| matches!(c, Call::Panel(..)))
            .collect();
        let ctx = format!("case {case}: mode {mode} t {t:#x} {calls:?}");
        match mode {
            0 => {
                let Call::Panel(px, py, _, ph) = panels[0] else {
                    unreachable!()
                };
                assert_eq!(
                    (px, py, ph),
                    (x, y + NOW_CHECKING_DY, NOW_CHECKING_H),
                    "{ctx}"
                );
            }
            3 => {
                assert_eq!(panels[0], Call::Panel(x, y, 284, 13), "{ctx}");
                assert_eq!(panels[1], Call::Panel(x, y + 0x20, 42, 26), "{ctx}");
                let row = ((cursor + 1) & 1) as u8;
                let hand = confirm_dialog_hand_draw_for(&rects, row, y, (0, 0), 1);
                assert!(
                    calls.contains(&Call::Icon(hand.dst.0, hand.dst.1, 0x4E)),
                    "{ctx}: the hand at {:?}",
                    hand.dst
                );
            }
            _ => {
                let (py, pw, ph, _) = CARD_WORK_PANEL;
                assert_eq!(panels[0], Call::Panel(x, py, pw, ph), "{ctx}");
            }
        }
    }
}
