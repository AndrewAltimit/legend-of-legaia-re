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

/// Mode `1`, the header tab, as the dispatcher calls it (`0x801DFD40..`:
/// `t = 0` from `(0x30, 6)`) with the centring text drawer `FUN_801E3EE0`
/// and the width measure `FUN_80035F04` run live and the raw string emitter
/// `FUN_80036888` recorded: the tab's panel and its word (`Save` / `Load` off
/// the op flag `0x801F0200`), and, off the title screen (`_DAT_8007BB00`
/// clear), the location box and its place name. Against
/// `SAVE_SELECT_PANEL_POS` / `SIZE`, `save_select_title_pos` and the location
/// tab's rect and text origin.
#[test]
fn the_header_tab_sits_where_the_port_draws_it() {
    differential!(prot(899, SLIDE));
    use legaia_engine_ui::{
        SAVE_SELECT_LOCATION_TAB_RECT, SAVE_SELECT_LOCATION_TAB_TEXT, SAVE_SELECT_PANEL_POS,
        SAVE_SELECT_PANEL_SIZE, save_select_title_pos,
    };
    use psx_retail_exec::A3;
    const EMIT: u32 = 0x8003_6888;
    const OP_FLAG: u32 = 0x801F_0200;
    const TITLE_SCREEN: u32 = 0x8007_BB00;
    const PLACE_PTR: u32 = 0x8007_B44C;
    const PLACE: u32 = 0x8014_0000;
    let (Some(mut cpu), Some(d), Some(scus)) = (scus_cpu(), disc(), crate::common::scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(899).expect("PROT 0899"));
    let tim = d
        .read_prot_bytes(
            legaia_font::FONT_TIM_PROT_DAT_OFFSET,
            legaia_font::FONT_TIM_LEN,
        )
        .expect("font TIM");
    let font = legaia_font::Font::from_disc_tim_and_scus(&tim, &scus).expect("font");
    let log = Rc::new(RefCell::new(Vec::<(u32, i32, i32)>::new()));
    let l = log.clone();
    cpu.hook(PANEL, move |r, _| {
        let a = |k: usize| r.r[A0 + k] as i32;
        l.borrow_mut().push((0, a(0) - a(2) / 2 - 10, a(1) - 2));
        l.borrow_mut().push((1, a(2) + 16, a(3) + 16));
    });
    let l = log.clone();
    cpu.hook(EMIT, move |r, m| {
        let y = m.rd32(r.r[psx_retail_exec::SP] + 0x10) as i32;
        l.borrow_mut().push((2 + r.r[A0], r.r[A3] as i32, y));
    });
    cpu.hook(ICON, |_, _| {});
    // The debug print, which goes to the BIOS, and BIOS A(0x2B) `memset`,
    // the measure's scratch line clear.
    cpu.stub(0x8005_67A8, 0);
    cpu.hook(0x8005_6788, |r, m| {
        for i in 0..r.r[psx_retail_exec::A2] {
            m.wr8(r.r[A0] + i, r.r[psx_retail_exec::A1] as u8);
        }
    });
    cpu.mem.load(PLACE, b"Rim Elm\0");
    cpu.mem.wr32(PLACE_PTR, PLACE);
    for (op, title_screen) in [(0u32, 0u32), (1, 0), (0, 1), (1, 1)] {
        cpu.mem.wr32(OP_FLAG, op);
        cpu.mem.wr32(TITLE_SCREEN, title_screen);
        cpu.mem.wr32(STACK_TOP + 0x10, 0);
        cpu.mem.wr32(STACK_TOP + 0x14, 0);
        log.borrow_mut().clear();
        cpu.calls.clear();
        let stop = cpu.call(SLIDE, &[1, 0, 0x30, 6]);
        assert_eq!(stop, Stop::Returned, "{:x?}", cpu.calls);
        let calls = log.borrow().clone();
        let word = if op == 0 { "Save" } else { "Load" };
        let word_at = 0x801C_EF0C + 8 * op;
        let mut want = vec![
            (0, SAVE_SELECT_PANEL_POS.0, SAVE_SELECT_PANEL_POS.1),
            (1, SAVE_SELECT_PANEL_SIZE.0, SAVE_SELECT_PANEL_SIZE.1),
        ];
        let title = save_select_title_pos(&font, word);
        let mut got = calls.clone();
        // The word, drawn first, then the tab.
        let word_call = got.remove(0);
        assert_eq!(
            word_call,
            (2 + word_at, title.0, title.1),
            "{word}: the word"
        );
        if title_screen == 0 {
            let (x, y, w, h) = SAVE_SELECT_LOCATION_TAB_RECT;
            let (tx, ty) = SAVE_SELECT_LOCATION_TAB_TEXT;
            want.extend([(2 + PLACE, tx, ty), (0, x, y), (1, w, h)]);
        }
        assert_eq!(got, want, "op {op} title screen {title_screen}: {calls:?}");
    }
}
