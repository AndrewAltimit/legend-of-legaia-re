//! Disc-gated: the window-script VM (`legaia_engine_vm::run`, retail
//! `FUN_801D6628`) executes **real disc programs** from its production
//! trigger.
//!
//! Chain under test: PROT 0899 menu-overlay bytes →
//! [`World::install_menu_overlay_tables`] resolves the widget programs
//! ([`legaia_engine_menus::menu_widget`]) → `MenuRuntime::open_shop_menu` +
//! `tick` runs the shop open script on the picker entry edge and the
//! slide-away script on the Sell transition - the two edges retail's
//! `FUN_801DAFD4` drives (docs/subsystems/shop.md).
//!
//! Skips + passes when `extracted/PROT.DAT` or `LEGAIA_DISC_BIN` is
//! missing.
//!
//! [`World::install_menu_overlay_tables`]: legaia_engine_core::world::World::install_menu_overlay_tables

use legaia_engine_core::world::World;
use legaia_engine_menu_runtime::menu_runtime::MenuRuntime;
use legaia_engine_menus::menu::{MenuInput, MenuState};
use legaia_engine_menus::shop::{ShopInventory, ShopItem, ShopSession};
use legaia_prot::archive::Archive;
use std::path::PathBuf;

fn menu_overlay_bytes() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let prot = [
        PathBuf::from("extracted/PROT.DAT"),
        PathBuf::from("../../extracted/PROT.DAT"),
    ]
    .into_iter()
    .find(|p| p.is_file())?;
    let mut archive = Archive::open(&prot).ok()?;
    let entry = archive
        .entries
        .get(legaia_game_tables::menu_windows::MENU_OVERLAY_PROT_INDEX)?
        .clone();
    let mut buf = Vec::new();
    archive.read_entry(&entry, &mut buf).ok()?;
    Some(buf)
}

fn world_with_overlay(overlay: &[u8]) -> World {
    let mut world = World::new();
    world.install_menu_overlay_tables(overlay);
    world
}

fn shop_runtime() -> MenuRuntime {
    let mut runtime = MenuRuntime::new(std::env::temp_dir());
    runtime.open_shop_menu(ShopSession::new(ShopInventory::new(
        0,
        vec![ShopItem {
            item_id: 0x77,
            price: 20,
        }],
    )));
    runtime
}

const IDLE: MenuInput = MenuInput {
    cross: false,
    circle: false,
    triangle: false,
    square: false,
    up: false,
    down: false,
    left: false,
    right: false,
};

#[test]
fn shop_open_edge_runs_disc_open_script() {
    let Some(overlay) = menu_overlay_bytes() else {
        eprintln!("[skip] extracted/PROT.DAT or LEGAIA_DISC_BIN missing");
        return;
    };
    let mut world = world_with_overlay(&overlay);
    // Non-vacuity: the overlay resolved into programs.
    assert!(world.menu.widget_scripts.is_some(), "scripts not resolved");
    assert!(!world.menu.widgets.any_open());

    let mut runtime = shop_runtime();
    runtime.tick(&mut world, IDLE);

    // The disc open script (`DAT_801E4E38`) opened the five shop windows.
    assert_eq!(
        world.menu.widgets.open_ids(),
        vec![0x20, 0x21, 0x22, 0x28, 0x2A]
    );
    // Home positions come from the window descriptor table: the gold box
    // (0x20) targets its descriptor rect, not the origin.
    let gold = world.menu.widgets.window(0x20).expect("gold window open");
    assert_ne!(
        (gold.target.x, gold.target.y),
        (0, 0),
        "gold window target not seeded from the descriptor table"
    );
}

#[test]
fn sell_transition_runs_disc_slide_away_script() {
    let Some(overlay) = menu_overlay_bytes() else {
        eprintln!("[skip] extracted/PROT.DAT or LEGAIA_DISC_BIN missing");
        return;
    };
    let mut world = world_with_overlay(&overlay);
    // Sell opens only over a bag holding something (`FUN_801DAFD4` row 1).
    world.party.inventory.insert(0x77, 1);
    let mut runtime = shop_runtime();
    runtime.tick(&mut world, IDLE);
    assert_eq!(world.menu.widgets.open_ids().len(), 5);

    // Drive the picker to the Sell row (Buy / Sell / Exit) and confirm.
    runtime.tick(&mut world, MenuInput { down: true, ..IDLE });
    runtime.tick(
        &mut world,
        MenuInput {
            cross: true,
            ..IDLE
        },
    );
    assert_eq!(
        MenuState::from_byte(runtime.ctx.state),
        Some(MenuState::ShopSell),
        "picker did not route to Sell"
    );

    // The slide-away script (`DAT_801E4E54`) closed the picker windows,
    // keeping the gold (0x20) + vendor plate (0x21).
    assert_eq!(world.menu.widgets.open_ids(), vec![0x20, 0x21]);
}

#[test]
fn every_scanned_program_executes_through_the_interpreter() {
    let Some(overlay) = menu_overlay_bytes() else {
        eprintln!("[skip] extracted/PROT.DAT or LEGAIA_DISC_BIN missing");
        return;
    };
    let world = world_with_overlay(&overlay);
    let scripts = world.menu.widget_scripts.as_ref().expect("resolved");
    // Non-vacuity: a real program table, executed end-to-end.
    assert!(scripts.programs.len() >= 30);
    let mut executed = 0usize;
    for (va, bytes) in &scripts.programs {
        let mut scratch = legaia_engine_menus::menu_widget::MenuWidgetState::default();
        let end = legaia_engine_vm::run(&mut scratch, bytes)
            .unwrap_or_else(|e| panic!("program {va:#010x} failed: {e}"));
        assert_eq!(end + 4, bytes.len(), "program {va:#010x} length mismatch");
        executed += 1;
    }
    assert!(executed >= 1);
}

/// Every program the slide lock runs resolves off the disc, and each entry
/// program closes the open set or opens a window - so every screen change
/// raises the latch (`legaia_engine_menus::menu_slide_lock`).
#[test]
fn the_slide_lock_programs_resolve_and_each_moves_a_window() {
    let Some(overlay) = menu_overlay_bytes() else {
        eprintln!("[skip] extracted/PROT.DAT or LEGAIA_DISC_BIN missing");
        return;
    };
    let scripts =
        legaia_engine_menus::menu_widget::MenuWidgetScripts::resolve_from_overlay(&overlay)
            .expect("scripts");
    use legaia_engine_menus::menu_slide_lock::{SUBSCREEN_ENTRY_PROGRAMS, SUBSCREEN_STEP_PROGRAMS};
    let moves = |va: u32| {
        let bytes = scripts.program(va).unwrap_or_else(|| panic!("{va:#x}"));
        bytes
            .chunks(4)
            .take_while(|i| i[0] != 0)
            .any(|i| matches!(i[0], 0x01 | 0x02 | 0x04 | 0x05))
    };
    for (id, vas) in SUBSCREEN_ENTRY_PROGRAMS {
        assert!(vas.iter().any(|&va| moves(va)), "sub-screen {id:#x}");
    }
    for &(id, step, enter, leave) in SUBSCREEN_STEP_PROGRAMS {
        assert!(
            moves(enter) && moves(leave),
            "sub-screen {id:#x} step {step}"
        );
    }
}

/// The casino prize counter on the disc's window programs: the opening
/// frame is the counter's phase 0, the next eleven read a raised latch, and
/// the twelfth after it takes the pad; the Yes / No window's program runs on
/// the press that opens it, so the pad is read again twelve frames after
/// that press. Left and Right never step it.
#[test]
fn the_prize_counter_waits_its_windows_out() {
    use legaia_engine_minigames::prize_exchange::PrizeRecord;
    use legaia_engine_system::input::PadButton as B;
    let Some(overlay) = menu_overlay_bytes() else {
        eprintln!("[skip] extracted/PROT.DAT or LEGAIA_DISC_BIN missing");
        return;
    };
    let mut w = world_with_overlay(&overlay);
    w.shops.prize_blocks = vec![vec![
        PrizeRecord {
            item_id: 0x10,
            gate: 0,
            price: 100,
        },
        PrizeRecord {
            item_id: 0x11,
            gate: 0,
            price: 100,
        },
    ]];
    w.minigames.casino_coins = 1_000;
    assert!(w.try_arm_prize_exchange(&[0x49, 0x07, 0x00]));
    let mut menu = MenuRuntime::new(std::env::temp_dir());
    assert!(
        menu.poll_field_overlay(&mut w).is_some(),
        "the counter opens"
    );
    let cursor = |m: &MenuRuntime| m.prize_session.as_ref().unwrap().cursor();
    // Down on every frame until it lands: frames 1..=11 after the open read
    // the latch the open's slide raised.
    let mut frame = 0;
    while cursor(&menu) == 0 {
        frame += 1;
        assert!(frame < 40);
        let _ = menu.step_field_session(&mut w, B::Down as u16);
    }
    assert_eq!(frame, 12, "the opening frame plus eleven");
    // Left / Right: the list kernel reads neither.
    for b in [B::Left, B::Right] {
        let _ = menu.step_field_session(&mut w, b as u16);
        assert_eq!(cursor(&menu), 1, "{b:?}");
    }
    // Cross opens the Yes / No window on the press; Up lands twelve later.
    let _ = menu.step_field_session(&mut w, B::Cross as u16);
    let confirm = |m: &MenuRuntime| m.prize_session.as_ref().unwrap().confirm_cursor();
    assert_eq!(confirm(&menu), 1, "No is the default");
    let mut frame = 0;
    while confirm(&menu) == 1 {
        frame += 1;
        assert!(frame < 40);
        let _ = menu.step_field_session(&mut w, B::Up as u16);
    }
    assert_eq!(frame, 12, "the window's slide-in");
}
