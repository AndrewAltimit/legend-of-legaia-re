//! Disc-gated: the two host screens this file pins read their text off the
//! user's disc and run on the world every host ticks.
//!
//! - The fishing venue hub (`engine-core::fishing_hub`): the entry every host
//!   shares (`SceneHost::enter_fishing_from_overlay`) decodes the menu rows and
//!   both help pages off PROT 0972, Triangle on the idle shore opens the menu,
//!   row 1 walks the two help pages (`FUN_801D72A0`), and row 4 leaves.
//! - The field floor window (`engine-core::field_submode_flag_window`, handler
//!   slot `0x23`): with the kor warp pad's operand parked, the legend strings
//!   come off the field overlay (PROT 0897) through `SceneHost`.
//!
//! Only structural facts are asserted - counts and non-emptiness, never text.
//! Skips and passes without `LEGAIA_DISC_BIN`.

use legaia_asset::static_overlay;
use legaia_engine_core::field_submode_flag_window::{FLAG_WINDOW_RECORD, FLAG_WINDOW_SLOT};
use legaia_engine_core::fishing_hub::HubScreen;
use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::{SceneMode, World};

fn host() -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return None;
    };
    match SceneHost::open_disc(&disc) {
        Ok(h) => Some(h),
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            None
        }
    }
}

fn press(world: &mut World, button: PadButton) {
    world.set_pad(button.mask());
    let _ = world.tick();
    world.set_pad(0);
    let _ = world.tick();
}

fn screen(world: &World) -> Option<HubScreen> {
    world
        .minigames
        .fishing
        .as_ref()
        .and_then(|s| s.hub())
        .map(|h| h.screen)
}

#[test]
fn the_venue_hub_reads_the_disc_and_walks_both_help_pages() {
    let Some(mut host) = host() else {
        return;
    };
    let rec = static_overlay::overlay_map()
        .by_prot_index(legaia_asset::fishing_species::FISHING_OVERLAY_PROT_INDEX as u32)
        .expect("fishing overlay in static map");
    let raw = host
        .index
        .entry_bytes_extended(rec.prot_index)
        .expect("read PROT 0972");
    let loaded = static_overlay::as_loaded(&raw, rec).expect("as-loaded form");
    host.world.mode = SceneMode::Field;
    assert!(host.enter_fishing_from_overlay(&loaded));

    let text = host
        .world
        .minigames
        .fishing_hub_text
        .clone()
        .expect("the hub text decodes off PROT 0972");
    assert_eq!(text.menu_rows.len(), 5);
    assert!(text.menu_rows.iter().all(|r| !r.is_empty()));
    assert_eq!((text.help[0].len(), text.help[1].len()), (14, 15));
    // Blank lines are real (the tables share one empty string), but most are text.
    assert!(text.help.iter().flatten().filter(|l| !l.is_empty()).count() >= 20);
    assert!(text.footers.iter().all(|f| !f.is_empty()));

    let world = &mut host.world;
    let _ = world.tick();
    // Triangle at the shore opens the menu (state 0x0C's `& 0x110`).
    press(world, PadButton::Triangle);
    assert_eq!(screen(world), Some(HubScreen::Menu));
    assert_eq!(world.fishing_hub_lines().len(), 5 + 1, "five rows and the cursor");
    // Row 1: help page 0, then page 1, then back to the menu.
    press(world, PadButton::Down);
    press(world, PadButton::Cross);
    assert_eq!(screen(world), Some(HubScreen::Help(0)));
    assert_eq!(world.fishing_hub_lines().len(), 14 + 1, "fourteen lines and the footer");
    press(world, PadButton::Cross);
    assert_eq!(screen(world), Some(HubScreen::Help(1)));
    assert_eq!(world.fishing_hub_lines().len(), 15 + 1);
    press(world, PadButton::Cross);
    assert_eq!(screen(world), Some(HubScreen::Menu));
    // Row 4 (up from row 1 wraps via 0 to 4): leave the venue.
    press(world, PadButton::Up);
    press(world, PadButton::Up);
    press(world, PadButton::Cross);
    assert_eq!(world.mode, SceneMode::Field, "row 4 leaves the venue");
    assert!(world.minigames.fishing.is_none());
}

#[test]
fn the_floor_window_legend_comes_off_the_field_overlay() {
    let Some(mut host) = host() else {
        return;
    };
    let world = &mut host.world;
    let _ = world.man_load_actor_reset();
    world.open_field_submode_screen(FLAG_WINDOW_SLOT, None);
    world.set_submode_board_entries(&[0x49, 0x04, 0x08, 0x00, 0x08, 0x38, 0x01]);
    world.tick_submode_screen(1);
    assert!(
        world
            .field_vm
            .submode_screen
            .installed_windows
            .contains(&FLAG_WINDOW_RECORD)
    );
    let lines = host.flag_window_lines();
    // Eight plates, seven suffixes, the cursor, two icons, two legend strings.
    assert_eq!(lines.len(), 8 + 7 + 1 + 2 + 2);
    let legend: Vec<_> = lines.iter().rev().take(3).step_by(2).collect();
    assert!(
        legend.iter().all(|l| !l.text.is_empty()),
        "both legend strings read off PROT 0897"
    );
}
