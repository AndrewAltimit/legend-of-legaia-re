//! Disc-gated: the window-script VM (`legaia_engine_vm::run`, retail
//! `FUN_801D6628`) executes **real disc programs** from its production
//! trigger.
//!
//! Chain under test: PROT 0899 menu-overlay bytes →
//! [`World::install_menu_overlay_tables`] resolves the widget programs
//! ([`legaia_engine_core::menu_widget`]) → `MenuRuntime::open_shop_menu` +
//! `tick` runs the shop open script on the picker entry edge and the
//! slide-away script on the Sell transition - the two edges retail's
//! `FUN_801DAFD4` drives (docs/subsystems/shop.md).
//!
//! Skips + passes when `extracted/PROT.DAT` or `LEGAIA_DISC_BIN` is
//! missing.
//!
//! [`World::install_menu_overlay_tables`]: legaia_engine_core::world::World::install_menu_overlay_tables

use legaia_engine_core::menu_runtime::MenuRuntime;
use legaia_engine_core::shop::{ShopInventory, ShopItem, ShopSession};
use legaia_engine_core::world::World;
use legaia_engine_vm::menu::{MenuInput, MenuState};
use legaia_prot::archive::Archive;
use std::path::PathBuf;

fn menu_overlay_bytes() -> Option<Vec<u8>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        return None;
    }
    let prot = [
        PathBuf::from("extracted/PROT.DAT"),
        PathBuf::from("../../extracted/PROT.DAT"),
    ]
    .into_iter()
    .find(|p| p.is_file())?;
    let mut archive = Archive::open(&prot).ok()?;
    let entry = archive
        .entries
        .get(legaia_asset::menu_windows::MENU_OVERLAY_PROT_INDEX)?
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
    assert!(world.menu_widget_scripts.is_some(), "scripts not resolved");
    assert!(!world.menu_widgets.any_open());

    let mut runtime = shop_runtime();
    runtime.tick(&mut world, IDLE);

    // The disc open script (`DAT_801E4E38`) opened the five shop windows.
    assert_eq!(
        world.menu_widgets.open_ids(),
        vec![0x20, 0x21, 0x22, 0x28, 0x2A]
    );
    // Home positions come from the window descriptor table: the gold box
    // (0x20) targets its descriptor rect, not the origin.
    let gold = world.menu_widgets.window(0x20).expect("gold window open");
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
    let mut runtime = shop_runtime();
    runtime.tick(&mut world, IDLE);
    assert_eq!(world.menu_widgets.open_ids().len(), 5);

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
    assert_eq!(world.menu_widgets.open_ids(), vec![0x20, 0x21]);
}

#[test]
fn every_scanned_program_executes_through_the_interpreter() {
    let Some(overlay) = menu_overlay_bytes() else {
        eprintln!("[skip] extracted/PROT.DAT or LEGAIA_DISC_BIN missing");
        return;
    };
    let world = world_with_overlay(&overlay);
    let scripts = world.menu_widget_scripts.as_ref().expect("resolved");
    // Non-vacuity: a real program table, executed end-to-end.
    assert!(scripts.programs.len() >= 30);
    let mut executed = 0usize;
    for (va, bytes) in &scripts.programs {
        let mut scratch = legaia_engine_core::menu_widget::MenuWidgetState::default();
        let end = legaia_engine_vm::run(&mut scratch, bytes)
            .unwrap_or_else(|e| panic!("program {va:#010x} failed: {e}"));
        assert_eq!(end + 4, bytes.len(), "program {va:#010x} length mismatch");
        executed += 1;
    }
    assert!(executed >= 1);
}
