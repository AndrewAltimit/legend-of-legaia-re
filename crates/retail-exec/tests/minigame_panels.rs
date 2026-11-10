//! Four small minigame routines, each whole: the dance teardown `FUN_801D414C`
//! (PROT 0980) against `dance_scene_stage`, the Baka Fighter "How to Play"
//! panel `FUN_801D6CBC` (PROT 0976) against `pause_panel`, and the fishing
//! help panel `FUN_801D72A0` and centred caption `FUN_801D73B8` (PROT 0972)
//! against `help_panel_layout` and `centred_caption`.

use crate::common::{Rng, disc, scus_cpu};
use psx_retail_exec::{A0, A1, A2, A3, Cpu, SP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const PRINT: u32 = 0x8003_6888;
const FRAME: u32 = 0x8002_C69C;
/// The dialog font's mode byte both panels store on entry.
const FONT_MODE: u32 = 0x8007_3F20;

/// `(string, x, y)` per printed line, and the closing frame's rect.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Panel {
    lines: Vec<(u32, i32, i32)>,
    frame: Option<(i32, i32, i32, i32)>,
}

fn panel_cpu(entry: usize) -> Option<(Cpu, Vec<u8>, Rc<RefCell<Panel>>)> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    let overlay = d.read_entry(entry).expect("the overlay");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let panel = Rc::new(RefCell::new(Panel::default()));
    let p = panel.clone();
    cpu.hook(PRINT, move |r, m| {
        assert_eq!((r.r[A1], r.r[A2]), (0, 0));
        let y = m.rd32(r.r[SP] + 0x10) as i32;
        p.borrow_mut().lines.push((r.r[A0], r.r[A3] as i32, y));
    });
    let p = panel.clone();
    cpu.hook(FRAME, move |r, _| {
        p.borrow_mut().frame = Some((
            r.r[A0] as i32,
            r.r[A1] as i32,
            r.r[A2] as i32,
            r.r[A3] as i32,
        ));
    });
    Some((cpu, overlay, panel))
}

/// The NUL-terminated bytes at `va`.
fn c_string(cpu: &mut Cpu, va: u32) -> Vec<u8> {
    (0..)
        .map(|i| cpu.mem.rd8(va + i))
        .take_while(|&b| b != 0)
        .collect()
}

#[test]
fn the_dance_teardown_restores_what_retail_restores() {
    differential!(prot(980, TEARDOWN));
    const TEARDOWN: u32 = 0x801D_414C;
    const STRCPY: u32 = 0x8005_6758;
    const SCENE_SETUP: u32 = 0x8002_6018;
    const SCENE_NAME: u32 = 0x8008_4548;
    const SAVED_NAME: u32 = 0x801D_518C;
    const BLOCK_BASE: u32 = 0x8008_4540;
    const SAVED_BASE: u32 = 0x801D_5180;
    const PAD_LATCH: u32 = 0x8007_B880;
    const BGM_LATCH: u32 = 0x8007_BA9C;
    use legaia_engine_minigames::dance::dance_scene_stage;
    let Some((mut cpu, _, _)) = panel_cpu(980) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The copy, and what the globals read when the scene setup runs.
    let copies = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let c = copies.clone();
    cpu.hook(STRCPY, move |r, _| c.borrow_mut().push((r.r[A0], r.r[A1])));
    let at_setup = Rc::new(RefCell::new(Vec::<(u32, u32, u32)>::new()));
    let s = at_setup.clone();
    cpu.hook(SCENE_SETUP, move |_, m| {
        s.borrow_mut()
            .push((m.rd32(PAD_LATCH), m.rd32(BLOCK_BASE), m.rd32(BGM_LATCH)));
    });
    let stage = dance_scene_stage();
    let mut rng = Rng(0x5445_4152);
    for _ in 0..200 {
        let (base, saved, latch, bgm) = (rng.next(), rng.next(), rng.next(), rng.below(64));
        cpu.mem.wr32(BLOCK_BASE, base);
        cpu.mem.wr32(SAVED_BASE, saved);
        cpu.mem.wr32(PAD_LATCH, latch);
        cpu.mem.wr32(BGM_LATCH, bgm);
        copies.borrow_mut().clear();
        at_setup.borrow_mut().clear();
        assert_eq!(cpu.call(TEARDOWN, &[0]), Stop::Returned);
        assert_eq!(
            *copies.borrow() == [(SCENE_NAME, SAVED_NAME)],
            stage.restores_caller_scene
        );
        // The pad latch and the block base are back before the setup runs;
        // the BGM latch is forced after it.
        assert_eq!(
            *at_setup.borrow(),
            [(
                if stage.clear_pad_latch { 0 } else { latch },
                if stage.restores_scene_block_base {
                    saved
                } else {
                    base
                },
                bgm
            )]
        );
        assert_eq!(cpu.mem.rd32(BGM_LATCH) as i32, stage.bgm_force_reload);
    }
}

#[test]
fn the_how_to_play_panel_is_laid_out_as_retails_is() {
    differential!(prot(976, BAKA_HELP));
    const BAKA_HELP: u32 = 0x801D_6CBC;
    use legaia_engine_minigames::baka_cabinet::{ST_HOWTO, pause_panel};
    let Some((mut cpu, overlay, panel)) = panel_cpu(976) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let text = legaia_minigame_formats::baka_opponents::parse_pause_text(&overlay).expect("text");
    // The cabinet's own call (`0x30`, `0x20`), which is the one seat the
    // port's layout holds.
    cpu.mem.wr8(FONT_MODE, 0);
    assert_eq!(cpu.call(BAKA_HELP, &[0x30, 0x20]), Stop::Returned);
    let (frame, lines) = pause_panel(ST_HOWTO, 0, &text).expect("the panel");
    let retail = panel.borrow().clone();
    assert_eq!(retail.lines.len(), lines.len());
    for ((va, x, y), port) in retail.lines.iter().zip(&lines) {
        // The line's bytes off the string retail prints: a `0xCE` escape's
        // operand may be zero, so the port's own length bounds the read.
        let bytes = cpu.mem.read(*va, port.text.len() + 1);
        assert_eq!(bytes[..port.text.len()], port.text[..]);
        assert_eq!(
            bytes[port.text.len()],
            0,
            "the line ends where the port's does"
        );
        assert_eq!((*x, *y), (port.x, port.y));
    }
    assert_eq!(retail.frame, Some((frame.x, frame.y, frame.w, frame.h)));
    assert_eq!(cpu.mem.rd8(FONT_MODE), 0xC, "the font's mode byte");
    assert!(lines.iter().filter(|l| !l.text.is_empty()).count() > 5);
}

#[test]
fn the_fishing_help_panel_is_laid_out_as_retails_is() {
    differential!(prot(972, FISHING_HELP));
    const FISHING_HELP: u32 = 0x801D_72A0;
    use legaia_engine_fishing::fishing::help_panel_layout;
    use legaia_engine_fishing::fishing_hub::{FishingHubText, HELP_FOOTER_VAS};
    let Some((mut cpu, overlay, panel)) = panel_cpu(972) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let text = FishingHubText::from_overlay(&overlay).expect("the hub's text");
    let mut rng = Rng(0x4845_4C50);
    for round in 0..200 {
        let page = round % 2;
        let (x, y) = (rng.below(64) as i16, rng.below(48) as i16);
        cpu.mem.wr8(FONT_MODE, 0);
        *panel.borrow_mut() = Panel::default();
        let arg = if page == 0 { 0 } else { 1 + rng.below(9) };
        assert_eq!(
            cpu.call(FISHING_HELP, &[x as u32, y as u32, arg]),
            Stop::Returned
        );
        let port = help_panel_layout(x, y, page != 0);
        let retail = panel.borrow().clone();
        let (footer, body) = retail.lines.split_last().expect("lines");
        assert_eq!(body.len(), port.lines.len(), "page {page}");
        for ((va, lx, ly), l) in body.iter().zip(&port.lines) {
            assert_eq!(
                (c_string(&mut cpu, *va), *lx, *ly),
                (
                    text.help[page][usize::from(l.string_index)].clone(),
                    i32::from(l.x),
                    i32::from(l.y)
                ),
                "page {page} line {}",
                l.string_index
            );
        }
        assert_eq!(
            *footer,
            (
                HELP_FOOTER_VAS[page],
                i32::from(port.footer.0),
                i32::from(port.footer.1)
            )
        );
        let f = port.frame;
        assert_eq!(
            retail.frame,
            Some((
                i32::from(f.0),
                i32::from(f.1),
                i32::from(f.2),
                i32::from(f.3)
            ))
        );
        assert_eq!(cpu.mem.rd8(FONT_MODE), 0x10);
    }
}

#[test]
fn a_centred_caption_sits_where_retail_puts_it() {
    differential!(prot(972, CAPTION));
    const CAPTION: u32 = 0x801D_73B8;
    const STRLEN: u32 = 0x8005_6768;
    const TEXT: u32 = 0x8014_0000;
    use legaia_engine_fishing::fishing_hub::centred_caption;
    let Some((mut cpu, _, panel)) = panel_cpu(972) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let len = Rc::new(std::cell::Cell::new(0u32));
    let l = len.clone();
    cpu.hook(STRLEN, move |r, _| {
        assert_eq!(r.r[A0], TEXT);
        r.r[V0] = l.get();
    });
    let mut rng = Rng(0x4341_5054);
    let mut drawn = [0u32; 2];
    for _ in 0..20_000 {
        let n = rng.below(40) as usize;
        let cx = rng.below(320) as i32;
        let y = match rng.below(4) {
            0 => 0xEE + rng.below(6) as i32,
            _ => rng.below(260) as i32 - 10,
        };
        len.set(n as u32);
        *panel.borrow_mut() = Panel::default();
        assert_eq!(
            cpu.call(CAPTION, &[TEXT, cx as u32, y as u32]),
            Stop::Returned
        );
        let port = centred_caption(n, cx, y);
        let retail = panel.borrow().lines.first().copied();
        assert_eq!(
            retail,
            port.map(|((x, y), _)| (TEXT, x, y)),
            "len {n} cx {cx} y {y}"
        );
        // Past the last row the routine returns its own gate's zero.
        assert_eq!(cpu.regs.r[V0] as i32, port.map_or(0, |(_, half)| half));
        drawn[usize::from(port.is_some())] += 1;
    }
    assert!(drawn[0] > 500 && drawn[1] > 5000, "{drawn:?}");
}
