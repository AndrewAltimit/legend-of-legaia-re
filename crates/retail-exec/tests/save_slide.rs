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

/// Mode `2`, the card pills, as the dispatcher calls it (`0x801DFD5C..`:
/// from `(0xA0, 0x60)` to `(0x30, 0x28)` on the pill timer `_DAT_801EF194`):
/// the raw `POLY_FT4` packets it writes into the primitive pool
/// `*0x1F8003A0` and links through `FUN_8003D2C4`, both pills from the
/// atlas records at `0x801E5048 + 0x48 / 0x54` at `t = 0`, the committed
/// card's pill alone (`_DAT_8007B834`) once the timer runs. Against the pill
/// sprites `save_select_overlay_draws` emits for the same timer: each sits
/// one texel inside its quad, the tight crop of the tile's content. At rest
/// retail draws each pill twice, additive then subtractive; the port once.
#[test]
fn the_card_pills_slide_where_the_port_draws_them() {
    differential!(prot(899, SLIDE));
    use legaia_engine_ui::{SaveSelectOverlayView, save_select_overlay_draws};
    use legaia_ui_formats::title_pak::{
        OVERLAY_SAVE_MENU_BAND_SLOT1 as SLOT1, OVERLAY_SAVE_MENU_BAND_SLOT2 as SLOT2,
    };
    const POOL_PTR: u32 = 0x1F80_03A0;
    const OT_PTR: u32 = 0x1F80_03F4;
    const POOL: u32 = 0x8015_0000;
    const OT: u32 = 0x8016_0000;
    const PILL_T: u32 = 0x801E_F194;
    const PORT: u32 = 0x8007_B834;
    const FADE: u32 = 0x801E_F120;
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &disc().expect("disc").read_entry(899).expect("PROT 0899"),
    );
    cpu.stub_outside = Some((SLIDE, SLIDE_END));
    let linked = Rc::new(RefCell::new(Vec::<u32>::new()));
    let l = linked.clone();
    cpu.hook(0x8003_D2C4, move |r, _| l.borrow_mut().push(r.r[A1]));
    let tile = (0, 0, 8, 8);
    let rects = SaveMenuAtlasRects {
        panel_tl: tile,
        panel_tr: tile,
        panel_bl: tile,
        panel_br: tile,
        panel_top: tile,
        panel_bot: tile,
        panel_left: tile,
        panel_right: tile,
        panel_interior: tile,
        slot1: SLOT1,
        slot2: SLOT2,
        cursor: (200, 200, 16, 16),
        ..SaveMenuAtlasRects::default()
    };
    let font = legaia_font::Font::placeholder();
    let mut rng = Rng(0x5049_4C4C);
    let mut slid = 0;
    for case in 0..2000 {
        let t = match rng.below(4) {
            0 => 0,
            1 => 0x1000,
            _ => rng.below(0x1001),
        };
        let card = rng.below(2);
        cpu.mem.wr32(PILL_T, t);
        cpu.mem.wr32(PORT, card);
        cpu.mem.wr32(FADE, rng.below(0x1001));
        cpu.mem.wr32(POOL_PTR, POOL);
        cpu.mem.wr32(OT_PTR, OT);
        cpu.mem.wr32(STACK_TOP + 0x10, 0x30);
        cpu.mem.wr32(STACK_TOP + 0x14, 0x28);
        linked.borrow_mut().clear();
        assert_eq!(
            cpu.call(SLIDE, &[2, t, 0xA0, 0x60]),
            Stop::Returned,
            "case {case}"
        );
        // Each linked packet: its first corner and its texel origin.
        let mut retail: Vec<(i32, i32, u32, u32, i32, i32)> = linked
            .borrow()
            .iter()
            .map(|&p| {
                let b = cpu.mem.read(p, 0x28);
                let h = |o: usize| i32::from(i16::from_le_bytes([b[o], b[o + 1]]));
                let (x0, y0) = (h(8), h(0xA));
                (
                    x0 + 1,
                    y0 + 1,
                    u32::from(b[0xC]) + 1,
                    u32::from(b[0xD]) + 1,
                    h(0x10) - x0,
                    h(0x1A) - y0,
                )
            })
            .collect();
        let pills: Vec<u8> = if t == 0 { vec![0, 1] } else { vec![card as u8] };
        let view = SaveSelectOverlayView {
            title: "Load",
            rows: &[],
            cursor: 0,
            single_pill: t != 0,
            pills: &pills,
            pill_cursor: None,
            slide_t: t as u16,
            info_t: 0,
            now_checking: false,
            now_checking_t: 0,
            banner: None,
            preview: None,
            confirm: None,
            location_tab: None,
        };
        let draws = save_select_overlay_draws(&font, Some(&rects), &view, (0, 0), 1);
        let mut port: Vec<(i32, i32, u32, u32, i32, i32)> = draws
            .sprites
            .iter()
            .filter(|s| s.src == SLOT1 || s.src == SLOT2)
            .map(|s| (s.dst.0, s.dst.1, s.src.0, s.src.1, 48, 16))
            .collect();
        // At rest retail draws each pill twice, an additive pass on the
        // record's CLUT (tpage `0x2F`) and a subtractive one on CLUT `+2`
        // (`0x4F`); the port draws the tile once.
        if t == 0 {
            port = port.iter().flat_map(|&p| [p, p]).collect();
        }
        retail.sort();
        port.sort();
        assert_eq!(retail, port, "case {case}: t {t:#x} card {card}");
        slid += usize::from(t != 0 && t != 0x1000);
    }
    assert!(slid > 500, "{slid}");
}

/// The darkening every panel mode pushes under itself, `FUN_80024EE4(1, 2,
/// grey * 0x010101)` at `0x801E2D78..0x801E2D88`, against the port's
/// `SaveScreenDarken` level: mode `0` ("Now checking") off `_DAT_801EF160`
/// and mode `3` (the confirm) off `_DAT_801EF1A4`, each `min(t >> 5, 0x50)`
/// (`0x801E1D10..0x801E1D34`), and mode `4` (the write / read panel) off its
/// slide timer `_DAT_801F01CC` after the routine's own ramp, `min(t >> 4,
/// 0xFF)`. The timers run their whole `0..=0x1000` range, the caps included.
#[test]
fn the_panel_darkening_is_the_port_level() {
    differential!(prot(899, SLIDE));
    use legaia_engine_ui::{CardBannerView, SaveSelectOverlayView, save_select_overlay_draws};
    const DARKEN: u32 = 0x8002_4EE4;
    const SMALL_T: u32 = 0x801E_F160;
    const CONFIRM_T: u32 = 0x801E_F1A4;
    const WORK_T: u32 = 0x801F_01CC;
    const WORK_FILL: u32 = 0x801F_01D0;
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &disc().expect("disc").read_entry(899).expect("PROT 0899"),
    );
    cpu.stub_outside = Some((SLIDE, SLIDE_END));
    let grey = Rc::new(RefCell::new(None::<(u32, u32, u32)>));
    let g = grey.clone();
    cpu.hook(DARKEN, move |r, _| {
        *g.borrow_mut() = Some((r.r[A0], r.r[A1], r.r[A2]))
    });
    // Zero-sized tiles would leave the nine-slice frames nothing to step by.
    let tile = (0, 0, 8, 8);
    let rects = SaveMenuAtlasRects {
        panel_tl: tile,
        panel_tr: tile,
        panel_bl: tile,
        panel_br: tile,
        panel_top: tile,
        panel_bot: tile,
        panel_left: tile,
        panel_right: tile,
        panel_interior: tile,
        cursor: (200, 200, 16, 16),
        ..SaveMenuAtlasRects::default()
    };
    let font = legaia_font::Font::placeholder();
    let mut rng = Rng(0x4441_524B);
    let mut capped = [0u32; 3];
    for case in 0..3000 {
        let mode = [0u32, 3, 4][rng.below(3) as usize];
        let t = if rng.below(4) == 0 {
            [0u32, 0x1000, 0xA00, 0xA20, 0xFF0][rng.below(5) as usize]
        } else {
            rng.below(0x1001)
        };
        let step = 1 + rng.below(3);
        cpu.mem.wr8(0x1F80_0393, step as u8);
        cpu.mem.wr32(SMALL_T, t);
        cpu.mem.wr32(CONFIRM_T, t);
        cpu.mem.wr32(WORK_T, t);
        cpu.mem.wr32(WORK_FILL, rng.below(0x1001));
        cpu.mem.wr32(CONFIRM_CURSOR, rng.below(2));
        cpu.mem.wr32(0x1F80_03A0, 0x8015_0000);
        cpu.mem.wr32(0x1F80_03F4, 0x8016_0000);
        cpu.mem.wr32(STACK_TOP + 0x10, 0xA0);
        cpu.mem.wr32(STACK_TOP + 0x14, 0x70);
        *grey.borrow_mut() = None;
        assert_eq!(
            cpu.call(SLIDE, &[mode, 0x1000, 0xA0, 0x70]),
            Stop::Returned,
            "case {case}"
        );
        let (a0, a1, rgb) = grey.borrow().expect("the darkening is pushed");
        assert_eq!((a0, a1), (1, 2), "case {case}");
        let level = rgb & 0xFF;
        assert_eq!(rgb, level * 0x01_0101, "case {case}: a grey");
        // Mode 4 ramps its timer before it reads it.
        let seen = if mode == 4 { cpu.mem.rd32(WORK_T) } else { t } as u16;
        let banner = CardBannerView {
            lines: ("", ""),
            note: "",
            work: true,
            slide_t: seen,
            progress_t: 0,
        };
        let view = SaveSelectOverlayView {
            title: "Load",
            rows: &[],
            cursor: 0,
            single_pill: false,
            pills: &[],
            pill_cursor: None,
            slide_t: 0,
            info_t: if mode == 3 { seen } else { 0 },
            now_checking: mode == 0,
            now_checking_t: if mode == 0 { seen } else { 0 },
            banner: (mode == 4).then_some(banner),
            preview: None,
            confirm: (mode == 3).then_some(("", 0)),
            location_tab: None,
        };
        let draws = save_select_overlay_draws(&font, Some(&rects), &view, (0, 0), 1);
        let port = draws.darken.map_or(0, |d| u32::from(d.level));
        assert_eq!(
            level, port,
            "case {case}: mode {mode} t {t:#x} seen {seen:#x}"
        );
        let cap = if mode == 4 { 0xFF } else { 0x50 };
        capped[[0, 3, 4].iter().position(|&m| m == mode).unwrap()] += u32::from(level == cap);
    }
    assert!(capped.iter().all(|&n| n > 50), "{capped:?}");
}

/// Mode `4`'s two ramps, pass after pass at frame steps `1..=4`: the slide
/// timer `_DAT_801F01CC` (`+ step << 8`) and the progress timer
/// `_DAT_801F01D0` (`+ step << 5`), each clamped at `0x1000`
/// (`0x801E2D00..0x801E2D4C`), against the port's `commit_slide_t` /
/// `commit_progress_t` of the units elapsed (the steps summed).
#[test]
fn the_write_panel_ramps_by_the_frame_step() {
    differential!(prot(899, SLIDE));
    use legaia_engine_menus::save_select::{commit_progress_t, commit_slide_t};
    const WORK_T: u32 = 0x801F_01CC;
    const WORK_FILL: u32 = 0x801F_01D0;
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &disc().expect("disc").read_entry(899).expect("PROT 0899"),
    );
    cpu.stub_outside = Some((SLIDE, SLIDE_END));
    let mut rng = Rng(0x5241_4D50);
    let mut clamped = 0;
    for run in 0..300 {
        cpu.mem.wr32(WORK_T, 0);
        cpu.mem.wr32(WORK_FILL, 0);
        let fixed = rng.below(5);
        let mut elapsed = 0u16;
        for pass in 0..0x90 {
            let step = if fixed == 0 { 1 + rng.below(4) } else { fixed } as u16;
            cpu.mem.wr8(0x1F80_0393, step as u8);
            // The packet cursor and OT, reset so the quads never walk into
            // the timers.
            cpu.mem.wr32(0x1F80_03A0, 0x8015_0000);
            cpu.mem.wr32(0x1F80_03F4, 0x8016_0000);
            cpu.mem.wr32(STACK_TOP + 0x10, 0xA0);
            cpu.mem.wr32(STACK_TOP + 0x14, 0x70);
            assert_eq!(cpu.call(SLIDE, &[4, 0x1000, 0xA0, 0x70]), Stop::Returned);
            elapsed += step;
            let ctx = format!("run {run} pass {pass} step {step} elapsed {elapsed}");
            assert_eq!(
                cpu.mem.rd32(WORK_T),
                u32::from(commit_slide_t(elapsed)),
                "{ctx}"
            );
            assert_eq!(
                cpu.mem.rd32(WORK_FILL),
                u32::from(commit_progress_t(elapsed)),
                "{ctx}"
            );
            clamped += u32::from(cpu.mem.rd32(WORK_FILL) == 0x1000);
        }
    }
    assert!(clamped > 1000, "{clamped}");
}
