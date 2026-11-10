//! The naming screen (PROT 0897): its state machine `FUN_801F03F0` against
//! `engine-menus`' `NameEntry`, call by call over generated pad words, and
//! its renderer `FUN_801E6B34` with the machine's own prompt draws against
//! `engine-ui`'s `name_entry_draws_for` and the hand of
//! `name_entry_chrome_sprite_draws_for`.
//!
//! Retail edits the record's name in place, so the name is read back off the
//! record after every call. The pad is the menu's: the edge word, the
//! auto-repeat word, and the two configurable masks.

use crate::common::Rng;
use crate::menu_painters::{Ev, cstr, hands, rig_on};
use legaia_engine_menus::name_entry::{
    GRID, NameEntry, NameEntryCue, NameEntryInput, NameEntryState,
};
use legaia_engine_ui::menu::window_painters::PainterRect;
use legaia_engine_ui::{
    NAME_ENTRY_BASE, NameEntryView, SaveMenuAtlasRects, name_entry_chrome_sprite_draws_for,
    name_entry_draws_for, text_draws_for,
};
use legaia_ui_formats::name_entry_prompts::NameEntryPrompts;
use psx_retail_exec::{A0, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const MACHINE: u32 = 0x801F_03F0;
const RENDERER: u32 = 0x801E_6B34;
const OPEN_SCRIPT: u32 = 0x801E_9B3C;
const WINDOW_TICK: u32 = 0x8003_1D00;
const CUE_REPLACE: u32 = 0x8003_5BD0;
const CUE_PUSH: u32 = 0x8003_5B50;

const CTX: u32 = 0x8013_0000;
const HOST: u32 = 0x8013_1000;
const OP: u32 = 0x8013_2000;
const HOST_PTR: u32 = 0x801C_6EA4;
const OP_PTR: u32 = 0x8007_B450;
const CURSOR: u32 = 0x8007_BB88;
const SHAPE: u32 = 0x8007_BB94;
const BUSY: u32 = 0x8007_BB80;
const EDGE: u32 = 0x8007_B874;
const REPEAT: u32 = 0x8007_BB84;
const CONFIRM_MASK: u32 = 0x8008_46D0;
const CANCEL_MASK: u32 = 0x8008_46D4;
const CHOICE: u32 = 0x8007_B458;
const FLOOR: u32 = 0x8007_B9D8;
const FRAME: u32 = 0x8008_4570;
const NAMES: u32 = 0x8008_49AF;
const STRIDE: u32 = 0x414;
const DEFAULTS: u32 = 0x801F_2A6C;

const CONFIRM: u32 = 0x40;
const CANCEL: u32 = 0x10;
const START: u32 = 0x800;
const UP: u32 = 0x1000;
const RIGHT: u32 = 0x2000;
const DOWN: u32 = 0x4000;
const LEFT: u32 = 0x8000;

fn name_of(m: &mut psx_retail_exec::Memory, ch: u32) -> String {
    String::from_utf8(cstr(m, NAMES + ch * STRIDE)).expect("ASCII")
}

#[test]
fn the_naming_screen_steps_and_draws_as_retail_does() {
    differential!(prot(897, MACHINE), prot(897, RENDERER));
    let Some(mut rig) = rig_on(897) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = crate::common::disc()
        .unwrap()
        .read_entry_footprint(897)
        .expect("PROT 0897");
    let prompts = NameEntryPrompts::from_field_overlay(&overlay, 0x801C_E818);
    let scus = crate::common::scus().unwrap();
    // BIOS A(0x2B) `memset`, the measurer's scratch-line clear.
    rig.cpu.hook(0x8005_6788, |r, m| {
        for i in 0..r.r[psx_retail_exec::A2] {
            m.wr8(r.r[A0] + i, r.r[psx_retail_exec::A1] as u8);
        }
    });
    rig.cpu.hook(OPEN_SCRIPT, |_, _| {});
    rig.cpu.hook(WINDOW_TICK, |_, _| {});
    let cues: Rc<RefCell<Vec<(bool, u32)>>> = Rc::default();
    for (entry, push) in [(CUE_REPLACE, false), (CUE_PUSH, true)] {
        let c = cues.clone();
        rig.cpu
            .hook(entry, move |r, _| c.borrow_mut().push((push, r.r[A0])));
    }
    rig.cpu.mem.wr32(HOST_PTR, HOST);
    rig.cpu.mem.wr32(OP_PTR, OP);
    rig.cpu.mem.wr32(CONFIRM_MASK, CONFIRM);
    rig.cpu.mem.wr32(CANCEL_MASK, CANCEL);
    rig.cpu.mem.wr32(BUSY, 0);
    let piece = (100, 0, 8, 8);
    let atlas = SaveMenuAtlasRects {
        cursor: (1, 0, 1, 1),
        panel_tl: piece,
        panel_tr: piece,
        panel_bl: piece,
        panel_br: piece,
        panel_top: piece,
        panel_bot: piece,
        panel_left: piece,
        panel_right: piece,
        panel_interior: piece,
        panel_filigree: piece,
        ..Default::default()
    };
    let base = PainterRect::new(NAME_ENTRY_BASE.0, NAME_ENTRY_BASE.1, 0x100, 0x60);

    let mut rng = Rng(0x4E41_4D45);
    let (mut commits, mut typed, mut refused, mut starts, mut wraps, mut carets) =
        (0, 0, 0, 0, 0, 0);
    for session in 0..400 {
        let ch = rng.below(3);
        rig.cpu.mem.wr8(OP + 1, ch as u8);
        let default = rig.string(DEFAULTS + ch * 8);
        rig.poke_str(NAMES + ch * STRIDE, &default);
        rig.cpu.mem.wr16(CTX + 0x54, 0);
        rig.cpu.mem.wr16(CTX + 0x50, 0x49);
        rig.cpu.mem.wr32(FLOOR, 2 + rng.below(3));
        let floor = rig.cpu.mem.rd32(FLOOR);
        // State 0 seats the cursor and saves the floor.
        rig.cpu.mem.wr32(EDGE, 0);
        rig.cpu.mem.wr32(REPEAT, 0);
        assert_eq!(rig.cpu.call(MACHINE, &[CTX]), Stop::Returned);
        let mut port = NameEntry::new(ch as usize, &default).with_scus_widths(Some(&scus));
        assert_eq!(rig.cpu.mem.rd32(CURSOR) as usize, port.cursor);
        // Sessions start from other cells too, as a walk would reach them.
        if rng.below(3) == 0 {
            let cell = loop {
                let c = rng.below(0x77) as usize;
                // Columns 5 and 11 hold a separator in every glyph row, so
                // no walk reaches them on the control row either - and Up
                // from there never leaves retail's loop.
                if !matches!(c % 17, 5 | 11) {
                    break c;
                }
            };
            rig.cpu.mem.wr32(CURSOR, cell as u32);
            port.cursor = cell;
        }
        for step in 0..400 {
            let (edge, repeat) = match rng.below(24) {
                0..=3 => (CONFIRM, 0),
                4 => (CANCEL, 0),
                5 => (START, 0),
                6..=9 => (0, UP),
                10..=13 => (0, DOWN),
                14..=16 => (0, LEFT),
                17..=19 => (0, RIGHT),
                20 => {
                    // Several at once, the d-pad's bits in both words.
                    let d = [UP, DOWN, LEFT, RIGHT][rng.below(4) as usize]
                        | [0, UP, DOWN, LEFT, RIGHT][rng.below(5) as usize];
                    let b = [0, CONFIRM, CANCEL, START, CONFIRM | CANCEL][rng.below(5) as usize];
                    (b | (d * rng.below(2)), d)
                }
                21 => (START | CONFIRM, 0),
                _ => (0, 0),
            };
            starts += u32::from(edge == START);
            rig.cpu.mem.wr32(EDGE, edge);
            rig.cpu.mem.wr32(REPEAT, repeat);
            rig.cpu.mem.wr32(FRAME, rng.next());
            let before = (
                rig.cpu.mem.rd32(CURSOR),
                name_of(&mut rig.cpu.mem, ch),
                rig.cpu.mem.rd16(CTX + 0x54),
            );
            cues.borrow_mut().clear();
            rig.log.borrow_mut().clear();
            rig.cpu.mem.wr32(0x8007_B454, 0x33);
            assert_eq!(
                rig.cpu.call(MACHINE, &[CTX]),
                Stop::Returned,
                "session {session} step {step} edge {edge:#x} repeat {repeat:#x} from {before:?}"
            );
            let machine_log = rig.log.borrow().clone();
            let cue = port.step(NameEntryInput {
                up: repeat & UP != 0,
                down: repeat & DOWN != 0,
                left: repeat & LEFT != 0,
                right: repeat & RIGHT != 0,
                confirm: edge & CONFIRM != 0,
                cancel: edge & CANCEL != 0,
                start: edge == START,
            });
            let what = format!(
                "session {session} step {step} char {ch} edge {edge:#x} repeat {repeat:#x} from {before:?}"
            );
            let state = rig.cpu.mem.rd16(CTX + 0x54);
            let name = name_of(&mut rig.cpu.mem, ch);
            assert_eq!(port.name, name, "{what}: the name");
            let want_cue: Vec<(bool, u32)> = cue
                .map(|c: NameEntryCue| (c.pushes(), c.id() as u32))
                .into_iter()
                .collect();
            assert_eq!(*cues.borrow(), want_cue, "{what}: the cue");
            if state == 0 {
                // Committed: the script is handed its resume.
                assert_eq!(port.state, NameEntryState::Done, "{what}");
                assert_eq!(rig.cpu.mem.rd16(CTX + 0x50), 0x1A, "{what}");
                assert_eq!(rig.cpu.mem.rd16(HOST + 0x40), 0x49, "{what}");
                assert_eq!(
                    rig.cpu.mem.rd32(FLOOR),
                    floor,
                    "{what}: the floor handed back"
                );
                assert_eq!(port.committed_name(), name, "{what}");
                commits += 1;
                break;
            }
            assert_eq!(
                port.state,
                if state == 1 {
                    NameEntryState::Editing
                } else {
                    NameEntryState::Confirm
                },
                "{what}: state {state}"
            );
            assert_eq!(port.arming, state == 2, "{what}: the arming call");
            assert_eq!(
                rig.cpu.mem.rd32(CURSOR) as usize,
                port.cursor,
                "{what}: the cursor"
            );
            if state == 4 {
                assert_eq!(
                    rig.cpu.mem.rd32(CHOICE) == 0,
                    port.confirm_yes,
                    "{what}: the choice"
                );
            }
            typed += u32::from(name.len() > before.1.len() && before.2 == 1);
            refused +=
                u32::from(edge == CONFIRM && before.2 == 1 && before.0 < 0x66 && name == before.1);
            wraps += u32::from(
                before.2 == 1
                    && before.0 < 0x66
                    && repeat == RIGHT
                    && edge == 0
                    && before.0 % 17 == 16,
            );

            // The frame's draws: the machine's prompt, then the renderer.
            let frame = rig.cpu.mem.rd32(FRAME);
            let mut log = machine_log;
            log.extend(rig.run(RENDERER, base));
            let (grid_cursor, control_cursor) = port.cursor_cells();
            let caret_on = legaia_engine_menus::name_entry::caret_on(u64::from(frame));
            let view = NameEntryView {
                grid_rows: &GRID,
                name: &port.name,
                default_name: &default,
                grid_cursor,
                control_cursor,
                confirming: port.confirm_shape,
                confirm_yes: port.confirm_yes,
                caret_on,
                prompts: Some(&prompts),
                char_index: ch as usize,
                line: port.line as u8,
            };
            // Retail's draws, laid out with the port's primitives. The
            // working name is the name escape `C1 <char>`, which the string
            // writer expands off the record.
            let mut want = Vec::new();
            let mut caret_drawn = false;
            for e in &log {
                let (bytes, x, y, ink) = match e {
                    Ev::Text(s, x, y, ink) => (s.clone(), *x, *y, *ink),
                    Ev::Label(va, x, y, ink) => (cstr(&mut rig.cpu.mem, *va), *x, *y, *ink),
                    _ => continue,
                };
                let bytes = if bytes.first() == Some(&0xC1) {
                    // The token's operand is the character, and a `0` there
                    // reads as the string's end to this log.
                    assert_eq!(bytes.get(1).copied().unwrap_or(0), ch as u8, "{what}");
                    name.clone().into_bytes()
                } else {
                    bytes
                };
                caret_drawn |= bytes == b"_";
                want.extend(text_draws_for(
                    &rig.font.layout(&bytes),
                    (x, y),
                    legaia_engine_ui::menu_ink_color(ink),
                ));
            }
            carets += u32::from(caret_drawn);
            let got = name_entry_draws_for(&rig.font, &view);
            let key = |d: &[legaia_engine_ui::TextDraw]| {
                let mut k = crate::menu_painters::keys(d);
                k.sort();
                k
            };
            let (kw, kg) = (key(&want), key(&got));
            if kw != kg {
                let only = |a: &[crate::menu_painters::Key], b: &[crate::menu_painters::Key]| {
                    a.iter()
                        .filter(|k| !b.contains(k))
                        .map(|k| (k.0, k.1))
                        .collect::<Vec<_>>()
                };
                panic!(
                    "{what}: text, state {state}\nretail only {:?}\nport only {:?}",
                    only(&kw, &kg),
                    only(&kg, &kw)
                );
            }
            let sprites = name_entry_chrome_sprite_draws_for(&atlas, &view, (0, 0), 1);
            let port_hands: Vec<(i32, i32)> = sprites
                .iter()
                .filter(|s| s.src.0 == 1)
                .map(|s| (s.dst.0, s.dst.1))
                .collect();
            let retail_hands: Vec<(i32, i32)> = hands(&log).iter().map(|h| (h.x, h.y)).collect();
            assert_eq!(port_hands, retail_hands, "{what}: the hand");
            assert_eq!(
                log.iter()
                    .filter(|e| matches!(e, Ev::Frame(..)))
                    .collect::<Vec<_>>(),
                vec![&Ev::Frame(base.x + 0xAC, base.y - 0x14, 0x48, 0xC)],
                "{what}: the name field"
            );
        }
        let _ = rig.cpu.mem.rd32(SHAPE);
    }
    assert!(
        commits > 100
            && typed > 2000
            && refused > 200
            && starts > 1000
            && wraps > 20
            && carets > 5000,
        "{commits} {typed} {refused} {starts} {wraps} {carets}"
    );
}
