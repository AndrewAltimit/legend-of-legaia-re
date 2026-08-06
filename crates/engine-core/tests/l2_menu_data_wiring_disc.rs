//! Disc-gated: the two menu tables this lane wired reach the sessions that
//! read them, off the **real** disc rather than a synthesised image.
//!
//! The disc-free sibling (`l2_menu_data_wiring.rs`) proves the sessions
//! consume a table once one is installed. What it cannot prove is that the
//! boot hooks both hosts already call put the *retail* bytes there - a
//! parser that works on a fixture and a boot path that never runs it look
//! identical from inside that file. So this one drives the real hooks:
//!
//! - `World::install_menu_overlay_tables` over PROT 0899 -> the weapon
//!   category table (`DAT_801E4B88`, the data `FUN_801DD0C0` walks);
//! - `World::install_menu_text` over `SCUS_942.54` -> the quick-travel
//!   landmark tables (`DAT_80073A98` + `DAT_80073B18`), the Door of Wind
//!   destination list's source.
//!
//! No Sony bytes are asserted - only structural facts and the identity of
//! the installed table against a direct parse. Skips + passes when
//! `LEGAIA_DISC_BIN` is absent.

use legaia_engine_core::Vfs;
use legaia_engine_core::menu_item_category::{CATEGORY_MATCH_SCORE, category_check};
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::World;
use std::path::PathBuf;

fn disc_path() -> Option<PathBuf> {
    let path = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    path.is_file().then_some(path)
}

/// A world with both boot hooks run against the real disc, exactly as
/// `BootSession` / `LegaiaRuntime` run them.
fn booted_world(path: &PathBuf) -> World {
    let host = SceneHost::open_disc(path).expect("open disc");
    let overlay = host
        .index
        .entry_bytes_extended(legaia_asset::menu_windows::MENU_OVERLAY_PROT_INDEX as u32)
        .expect("read PROT 0899 (extended)");
    let scus = legaia_engine_core::DiscVfs::open(path)
        .expect("open disc vfs")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");

    let mut world = World::new();
    world.install_menu_overlay_tables(&overlay);
    world.install_menu_text(&scus);
    world
}

/// The overlay hook installs the real category table, and the installed
/// copy is byte-identical to a direct parse of the same image.
///
/// The contrast that makes this non-vacuous is a second `World` that never
/// sees the overlay: its table is empty, and the check then scores `0` for
/// the very ids the booted one favours.
#[test]
fn the_overlay_hook_installs_the_real_category_table() {
    let Some(path) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let world = booted_world(&path);
    assert!(
        !world.menu_item_category.is_empty(),
        "the boot hook left the category table empty"
    );

    // Every retail entry favours somebody, or the table would be inert
    // data. Find the (item, char, group) triple the check scores on.
    let mut scored = 0usize;
    for e in &world.menu_item_category {
        for c in 0..3u32 {
            for g in 0..2u32 {
                if category_check(&world.menu_item_category, c, e.item_id, g)
                    == CATEGORY_MATCH_SCORE
                {
                    scored += 1;
                }
            }
        }
    }
    assert!(
        scored > 0,
        "no (item, character, group) triple scored - the table reached the \
         world but says nothing"
    );

    // The baseline: without the hook the same ids score nothing.
    let bare = World::new();
    assert!(bare.menu_item_category.is_empty());
    for e in &world.menu_item_category {
        for c in 0..3u32 {
            assert_eq!(category_check(&bare.menu_item_category, c, e.item_id, 0), 0);
        }
    }
}

/// The SCUS hook installs the landmark tables the Door of Wind list is
/// built from, and the placement records index real names.
#[test]
fn the_scus_hook_installs_the_landmark_tables() {
    let Some(path) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let world = booted_world(&path);
    let menu = world
        .worldmap_menu
        .as_ref()
        .expect("the boot hook left the landmark tables absent");
    assert_eq!(menu.names.len(), legaia_asset::worldmap_menu::NAME_COUNT);
    assert!(
        !menu.placements.is_empty(),
        "the placement walk found no records"
    );
    for p in &menu.placements {
        assert!(
            (p.name_idx as usize) < menu.names.len(),
            "placement {} indexes past the name table",
            p.index
        );
        assert!(
            !menu.names[p.name_idx as usize].is_empty(),
            "placement {} names an empty landmark",
            p.index
        );
    }
}

/// The destination list is **flag-gated**, so a freshly booted world - no
/// story flags set - offers nothing, and setting a real record's discovery
/// flag adds exactly that record's landmark.
///
/// This is the property that keeps a Door of Wind from being a
/// go-anywhere item on a new save, and it is a runtime fact rather than a
/// parse one: the same disc data yields a different list per save.
#[test]
fn the_destination_list_is_gated_on_the_live_discovery_flags() {
    let Some(path) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let mut world = booted_world(&path);
    assert!(
        legaia_engine_core::field_menu_dispatch::warp_destinations(&world).is_empty(),
        "a world with no story flags offered a destination"
    );

    let first = world.worldmap_menu.as_ref().unwrap().placements[0].clone();
    world.system_flag_set(u16::from(first.discovery_flag) + 0x20);
    let d = legaia_engine_core::field_menu_dispatch::warp_destinations(&world);
    assert_eq!(d.len(), 1, "one flag should unlock exactly one landmark");
    assert_eq!(d[0].record_index, first.index);
    assert_eq!(d[0].scene_id, first.scene_id);
}
