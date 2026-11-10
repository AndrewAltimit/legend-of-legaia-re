//! The menu overlay's small sub-screens (PROT 0899) frame by frame against
//! `SaveScreenMachine`: the terminal screen `FUN_801DD12C` (`0x00`), the
//! Yes/No confirm `FUN_801D6D38` (`0x03`), the post-save notice
//! `FUN_801DD1B8` (`0x04`), the press-release wait `FUN_801DD26C` (`0x08`)
//! and the confirm-exit `FUN_801D8A58` (`0x0B`), with the list kernel
//! `FUN_801D688C` live beneath the two confirms and its port
//! `menu_cursor_nav` where the port's host calls it.
//!
//! The window VM and the window-list tick are logged and stubbed (the VM has
//! its own differential, `window_vm`); the busy word `0x8007BB80` they would
//! drive is scripted, as are the pad's edge and repeat words and the two
//! button masks. Each frame compares the screen id, the step, the outer
//! state a screen ends the flow with and the fade it flips, the cursor, the
//! scripts run, the cues of both enqueue paths and the list kernel's cues.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_menus::save_subscreen::{
    SaveEntryContext, SaveScreenMachine, SaveSubScreen, SubScreenEffect, SubScreenInput,
};
use legaia_engine_system::menu_input::{NavButtons, menu_cursor_nav};
use psx_retail_exec::{A0, Cpu, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const FINAL_EXIT: u32 = 0x801D_D12C;
const CONFIRM: u32 = 0x801D_6D38;
const NOTICE: u32 = 0x801D_D1B8;
const RELEASE_WAIT: u32 = 0x801D_D26C;
const CONFIRM_EXIT: u32 = 0x801D_8A58;
const LIST: u32 = 0x801D_688C;
const VM: u32 = 0x801D_6628;
const CUE: u32 = 0x8003_5BD0;
const CUE_88: u32 = 0x8004_2310;
const NAV_CUE: u32 = 0x8003_5B50;
const WINDOW_TICK: u32 = 0x8003_1D00;

const FADE: u32 = 0x801E_46A0;
const SCREEN: u32 = 0x801E_46A4;
const STEP: u32 = 0x801E_46AC;
const CURSOR: u32 = 0x801E_46D0;
const OUTER: u32 = 0x8007_B43C;
const BUSY: u32 = 0x8007_BB80;
const EDGE: u32 = 0x8007_B874;
const REPEAT: u32 = 0x8007_BB84;
const CONFIRM_MASK: u32 = 0x8008_46D0;
const CANCEL_MASK: u32 = 0x8008_46D4;
const LIST_CONFIRM_MASK: u32 = 0x801E_F0F0;
const LIST_CANCEL_MASK: u32 = 0x801E_F0F4;

#[derive(Default)]
struct Log {
    scripts: Vec<u32>,
    cues: Vec<u32>,
    nav_cues: Vec<u32>,
}

fn rig() -> Option<(Cpu, Rc<RefCell<Log>>)> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    let log = Rc::new(RefCell::new(Log::default()));
    let l = log.clone();
    cpu.hook(VM, move |r, _| l.borrow_mut().scripts.push(r.r[A0]));
    let l = log.clone();
    cpu.hook(CUE, move |r, _| l.borrow_mut().cues.push(r.r[A0]));
    let l = log.clone();
    cpu.hook(CUE_88, move |r, _| l.borrow_mut().cues.push(r.r[A0]));
    let l = log.clone();
    cpu.hook(NAV_CUE, move |r, _| l.borrow_mut().nav_cues.push(r.r[A0]));
    cpu.stub(WINDOW_TICK, 0);
    Some((cpu, log))
}

fn dispatching() -> SaveScreenMachine {
    let mut m = SaveScreenMachine::new(SaveEntryContext::DebugParamEditor);
    m.tick(SubScreenInput::default(), 0xFF);
    m.tick(SubScreenInput::default(), 0xFF);
    m
}

fn edge_word(rng: &mut Rng) -> u32 {
    match rng.below(8) {
        0 | 1 => 0,
        2 => 0x40,
        3 => 0x20,
        4 => 0x60,
        5 => [0x1000, 0x4000, 0x2000, 0x8000, 0x10, 0x80][rng.below(6) as usize],
        _ => rng.next() & 0xF0F0,
    }
}

#[test]
fn the_small_sub_screens_step_as_retail_steps() {
    differential!(
        prot(899, FINAL_EXIT),
        prot(899, CONFIRM),
        prot(899, NOTICE),
        prot(899, RELEASE_WAIT),
        prot(899, CONFIRM_EXIT),
        prot(899, LIST)
    );
    let Some((mut cpu, log)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The list kernel is entered from the screens; one direct call puts its
    // own bytes on the ledger.
    cpu.mem.wr32(CURSOR, 0);
    assert_eq!(cpu.call(LIST, &[CURSOR, 2, 1]), Stop::Returned);
    let screens = [
        (FINAL_EXIT, SaveSubScreen::FinalExit),
        (CONFIRM, SaveSubScreen::ConfirmYesNo),
        (NOTICE, SaveSubScreen::PostSaveReturn),
        (RELEASE_WAIT, SaveSubScreen::PadReleaseWait),
        (CONFIRM_EXIT, SaveSubScreen::ConfirmExit),
    ];
    let mut rng = Rng(0x5355_4253);
    let mut ends = [0u32; 5];
    for episode in 0..6000 {
        let k = rng.below(5) as usize;
        let (entry, screen) = screens[k];
        let (confirm, cancel) = if rng.below(4) == 0 {
            (0x20, 0x40)
        } else {
            (0x40, 0x20)
        };
        for (a, v) in [
            (CONFIRM_MASK, confirm),
            (CANCEL_MASK, cancel),
            (LIST_CONFIRM_MASK, confirm),
            (LIST_CANCEL_MASK, cancel),
        ] {
            cpu.mem.wr32(a, v);
        }
        let mut port = dispatching();
        port.goto(screen);
        let mut cursor: u32 = rng.below(4) | [0, 0x1000][rng.below(2) as usize];
        cpu.mem.wr32(CURSOR, cursor);
        cpu.mem.wr32(STEP, 0);
        cpu.mem.wr32(FADE, 0x11);
        cpu.mem.wr32(OUTER, 2);
        let mut port_log = Log::default();
        *log.borrow_mut() = Log::default();
        for frame in 0..40 {
            let what = format!("episode {episode} screen {:#x} frame {frame}", screen.id());
            let busy = u32::from(rng.below(3) == 0);
            let edge = edge_word(&mut rng);
            let repeat = [0, 0x1000, 0x4000, 0x2000, 0x8000][rng.below(5) as usize];
            cpu.mem.wr32(BUSY, busy);
            cpu.mem.wr32(EDGE, edge);
            cpu.mem.wr32(REPEAT, repeat);
            cpu.mem.wr32(SCREEN, u32::from(screen.id()));
            assert_eq!(cpu.call(entry, &[]), Stop::Returned, "{what}");

            // The port's host: the list kernel runs where the screen waits
            // on it, the rest of the input is the same words decoded.
            let step = port.step();
            let mut nav = 0;
            if matches!(
                screen,
                SaveSubScreen::ConfirmYesNo | SaveSubScreen::ConfirmExit
            ) && step == 1
                && busy == 0
            {
                let r = menu_cursor_nav(
                    &mut cursor,
                    2,
                    true,
                    NavButtons::new(
                        edge & confirm != 0,
                        edge & cancel != 0,
                        repeat & 0x1000 != 0,
                        repeat & 0x4000 != 0,
                    ),
                );
                nav = match r {
                    legaia_engine_system::menu_input::CursorNav::None => 0,
                    legaia_engine_system::menu_input::CursorNav::Confirm => 1,
                    legaia_engine_system::menu_input::CursorNav::Cancel => 2,
                    legaia_engine_system::menu_input::CursorNav::Moved => 3,
                };
                port_log.nav_cues.extend(r.sfx_cue().map(u32::from));
            }
            let fx = port.tick(
                SubScreenInput {
                    script_busy: busy != 0,
                    any_button_held: edge & (confirm | cancel) != 0,
                    nav,
                    cursor: (cursor & 0xFFF) as u16,
                    ..Default::default()
                },
                0,
            );
            for e in fx {
                match e {
                    SubScreenEffect::SeedCursor(v) => cursor = u32::from(v),
                    SubScreenEffect::RunScript => port_log.scripts.push(0),
                    SubScreenEffect::Sfx(c) => port_log.cues.push(u32::from(c)),
                    other => panic!("{what}: {other:?}"),
                }
            }

            let l = log.borrow();
            assert_eq!(l.scripts.len(), port_log.scripts.len(), "{what}: scripts");
            assert_eq!(l.cues, port_log.cues, "{what}: cues");
            assert_eq!(l.nav_cues, port_log.nav_cues, "{what}: list cues");
            assert_eq!(cpu.mem.rd32(CURSOR), cursor, "{what}: cursor");
            let left = cpu.mem.rd32(SCREEN);
            assert_eq!(left, u32::from(port.screen().id()), "{what}: screen");
            let outer = cpu.mem.rd32(OUTER);
            let exit = port.exit_code().map_or(2, u32::from);
            assert_eq!(outer, exit, "{what}: outer state");
            assert_eq!(cpu.mem.rd32(FADE) == 0xF2, exit != 2, "{what}: fade");
            if left != u32::from(screen.id()) || exit != 2 {
                ends[k] += 1;
                break;
            }
            assert_eq!(cpu.mem.rd32(STEP), u32::from(port.step()), "{what}: step");
        }
    }
    assert!(ends.iter().all(|&e| e > 500), "{ends:?}");
}

/// Retail pad bits and the port's for the same button.
const BUTTONS: [(u32, legaia_engine_system::input::PadButton); 7] = {
    use legaia_engine_system::input::PadButton as B;
    [
        (0x1000, B::Up),
        (0x2000, B::Right),
        (0x4000, B::Down),
        (0x8000, B::Left),
        (0x0040, B::Cross),
        (0x0020, B::Circle),
        (0x0800, B::Start),
    ]
};

/// The live pause menu's two screens over the same bytes:
/// `FieldMenuSession`'s ready check against `FUN_801D6D38` from its input
/// step on, and its notice against `FUN_801DD1B8`, on a fresh press each
/// frame (the repeat word carries the press's directions, as it does on the
/// frame a button goes down).
#[test]
fn the_pause_menus_ready_check_and_notice_step_as_retail_steps() {
    differential!(prot(899, CONFIRM), prot(899, NOTICE));
    use legaia_engine_menus::field_menu::{
        FieldMenuGate, FieldMenuInput, FieldMenuPhase, FieldMenuSession,
    };
    let Some((mut cpu, log)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    for (a, v) in [
        (CONFIRM_MASK, 0x40),
        (CANCEL_MASK, 0x20),
        (LIST_CONFIRM_MASK, 0x40),
        (LIST_CANCEL_MASK, 0x20),
    ] {
        cpu.mem.wr32(a, v);
    }
    cpu.mem.wr32(BUSY, 0);
    let mut rng = Rng(0x5245_4459);
    let (mut yes, mut no, mut back, mut moved) = (0, 0, 0, 0);
    for episode in 0..4000 {
        let mut s = FieldMenuSession::new();
        s.set_gate(FieldMenuGate {
            entry_context_kind: Some(legaia_engine_menus::pause_screens::ROOT_MENU_CONTEXT_LOCKED),
            save_allowed: true,
        });
        s.open_entry_screen();
        assert!(s.notice_is_up());
        let notice = episode % 2 == 0;
        if !notice {
            s.tick(FieldMenuInput {
                cross: true,
                ..Default::default()
            });
            s.tick(FieldMenuInput {
                circle: true,
                ..Default::default()
            });
            assert_eq!(s.ready_confirm_cursor(), Some(1));
            // Retail's opening step seeds the cursor and runs the script.
            cpu.mem.wr32(STEP, 0);
            cpu.mem.wr32(SCREEN, 3);
            assert_eq!(cpu.call(CONFIRM, &[]), Stop::Returned);
        } else {
            cpu.mem.wr32(STEP, 1);
        }
        *log.borrow_mut() = Log::default();
        for frame in 0..30 {
            let what = format!("episode {episode} frame {frame}");
            let edge = edge_word(&mut rng) | [0, 0x800][rng.below(8).min(1) as usize ^ 1];
            let pad = BUTTONS
                .iter()
                .filter(|(r, _)| edge & r != 0)
                .fold(0u16, |w, (_, b)| w | b.mask());
            cpu.mem.wr32(EDGE, edge);
            cpu.mem.wr32(REPEAT, edge & 0xF000);
            let id = if notice { 4 } else { 3 };
            cpu.mem.wr32(SCREEN, id);
            assert_eq!(
                cpu.call(if notice { NOTICE } else { CONFIRM }, &[]),
                Stop::Returned
            );
            s.tick(FieldMenuInput::from_pad_edge(pad));
            let next = cpu.mem.rd32(SCREEN);
            match (next, s.phase()) {
                (3, FieldMenuPhase::ReadyConfirm { cursor, .. }) => {
                    assert_eq!(cpu.mem.rd32(CURSOR) & 0xFFF, u32::from(cursor), "{what}");
                    moved += u32::from(!log.borrow().nav_cues.is_empty());
                }
                (4, FieldMenuPhase::Notice) => {}
                (0, FieldMenuPhase::Done(_)) => {
                    yes += 1;
                    break;
                }
                (1, FieldMenuPhase::Browsing { .. }) => {
                    if notice {
                        back += 1;
                    } else {
                        no += 1;
                    }
                    break;
                }
                (r, p) => panic!("{what}: retail screen {r}, port {p:?}"),
            }
            log.borrow_mut().nav_cues.clear();
        }
    }
    assert!(
        yes > 100 && no > 300 && back > 300 && moved > 300,
        "{yes} {no} {back} {moved}"
    );
}
