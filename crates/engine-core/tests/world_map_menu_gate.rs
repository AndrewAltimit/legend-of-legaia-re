//! The **menu-open gate** and the scene modes it admits.
//!
//! Retail has no global Start handler. The menu-open accept is a leg of the
//! pre-movement header inside the locomotion controller `FUN_801D01B0`
//! (`0x801D0250..0x801D032C`), so "can the pause menu open here" is exactly
//! "does this scene run that controller".
//!
//! The three kingdom overworlds **do**. They are ordinary `game_mode 0x03`
//! field-run scenes driven by the same `FUN_801D1344` -> `FUN_801D01B0` chain
//! as a town, and retail proves it inside the controller itself: the
//! base-step selector's `s4 = 5` arm at `0x801D0354` is taken exactly when
//! the world-map flag `_DAT_8007B6A8` is set, which would be unreachable if a
//! `_DAT_8007B6A8` scene never entered the function. `FUN_801E76D4` - the
//! function that reads like a second controller - is the top-view *debug*
//! renderer and branches straight to its epilogue (`0x801E9B14`) whenever
//! `DAT_801F2B94 == 0`.
//!
//! This matters because of where the Save row lives. `_DAT_8007B6A8` gates
//! that row and is set on exactly the three kingdom overworlds, so a port
//! that opens the menu only in `SceneMode::Field` has no pad route to Save
//! anywhere - the permitting scenes and the opening modes would not
//! intersect. The two halves are pinned together here so neither can drift
//! back on its own:
//!
//! * the **open** gate admits `SceneMode::WorldMap` (and refuses every
//!   suspended mode), and
//! * the scene that gate admits is the one whose MAN sets the Save bit.
//!
//! The mode-partition tests are disc-free. The scene-chain tests need
//! `extracted/` and skip-pass without it (CLAUDE.md disc-gated convention).

use std::path::PathBuf;

use legaia_engine_core::field_menu::{FieldMenuGate, FieldMenuRow, FieldMenuSession};
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::{SceneMode, World};

/// The three kingdom overworlds - the only scenes whose MAN permits saving.
const KINGDOM_MAPS: [&str; 3] = ["map01", "map02", "map03"];

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// The gate a host samples off the world at menu-open, exactly as
/// `BootSession::open_field_menu` does.
fn menu_for(world: &World) -> FieldMenuSession {
    let mut s = FieldMenuSession::new();
    s.set_gate(FieldMenuGate {
        entry_context_kind: world.menu_entry_context_kind(),
        save_allowed: world.scene_save_allowed,
    });
    s
}

// ---------------------------------------------------------------------------
// The mode partition (disc-free)
// ---------------------------------------------------------------------------

/// Both walking modes admit the menu; every suspended mode refuses it.
///
/// The port splits retail's single `game_mode 0x03` in two - `Field` for
/// towns and fields, `WorldMap` for the kingdom overworlds - so the predicate
/// has to name both. The refusing half is asserted explicitly because a
/// predicate that admitted everything would pass the admitting half alone.
#[test]
fn the_menu_open_gate_admits_exactly_the_two_walking_modes() {
    let mut world = World::new();

    for mode in [SceneMode::Field, SceneMode::WorldMap] {
        world.mode = mode;
        assert!(
            world.scene_mode_takes_menu_open(),
            "{mode:?} walks the player through the field locomotion \
             controller, so its pad reaches the menu-open accept"
        );
        assert!(
            world.field_menu_open_allowed(),
            "{mode:?} with no dialogue up must allow the menu to open"
        );
    }

    // Every mode that suspends field dispatch. Battle is the one that bit
    // first: Start mid-fight used to open the menu and freeze the fight.
    for mode in [
        SceneMode::Title,
        SceneMode::Battle,
        SceneMode::Cutscene,
        SceneMode::Menu,
        SceneMode::Dance,
        SceneMode::Fishing,
        SceneMode::SlotMachine,
        SceneMode::BakaFighter,
        SceneMode::MuscleDome,
    ] {
        world.mode = mode;
        assert!(
            !world.scene_mode_takes_menu_open(),
            "{mode:?} suspends field dispatch - its pad never reaches the accept"
        );
        assert!(!world.field_menu_open_allowed(), "{mode:?} must refuse");
    }
}

/// The engaged-bit refusal still applies on the overworld.
///
/// Retail's very first test in `FUN_801D01B0` (`0x801D01F0`) branches past
/// the whole pre-movement header when `player+0x10 & 0x80000` is set, and
/// that branch is upstream of the menu-open accept - so a talking player
/// opens nothing anywhere, overworld included. Widening the *mode* gate must
/// not have widened this one.
#[test]
fn a_dialogue_still_refuses_the_menu_on_the_overworld() {
    let mut world = World::new();
    world.mode = SceneMode::WorldMap;
    assert!(world.field_menu_open_allowed(), "control: idle overworld");

    world.start_inline_dialogue(vec![0x1F, b'h', b'i', 0x00, 0x21]);
    assert!(
        world.dialogue_owns_input(),
        "control: the inline runner owns the pad"
    );
    assert!(
        !world.field_menu_open_allowed(),
        "the engaged-bit refusal is upstream of the accept, so it holds on \
         the overworld too"
    );

    world.inline_dialogue = None;
    assert!(
        world.field_menu_open_allowed(),
        "the refusal is scoped to the conversation, not permanent"
    );
}

// ---------------------------------------------------------------------------
// The scene chain (disc-gated on `extracted/`)
// ---------------------------------------------------------------------------

/// On real disc data the two halves intersect: the mode that opens the menu
/// is the mode whose scene permits Save.
///
/// This is the assertion the port previously could not make. Entering a
/// kingdom overworld must leave the world in a mode the open gate admits
/// *and* with the MAN's save bit set, so the row the menu builds is both
/// reachable and enabled.
#[test]
fn a_kingdom_overworld_both_opens_the_menu_and_offers_save() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");

    for name in KINGDOM_MAPS {
        host.enter_world_map_scene(name)
            .unwrap_or_else(|e| panic!("enter {name} as overworld: {e:#}"));

        assert_eq!(
            host.world.mode,
            SceneMode::WorldMap,
            "{name} is an overworld scene"
        );
        assert!(
            host.world.scene_save_allowed,
            "{name}'s MAN sets the save-allow bit (_DAT_8007B6A8)"
        );
        assert!(
            host.world.field_menu_open_allowed(),
            "{name}: the pad must reach the menu-open accept here - this is \
             the only kind of scene where Save is legal, so a mode gate that \
             refuses it leaves Save unreachable everywhere"
        );

        let menu = menu_for(&host.world);
        assert!(
            menu.row_is_available(FieldMenuRow::Save),
            "{name}: the Save row must be offerable once the menu is open"
        );
        let view = menu.view();
        assert!(
            view.rows[FieldMenuRow::Save.index() as usize].enabled,
            "{name}: the renderer must draw the Save row live, not grey"
        );
    }
}

/// The contrast, on the same chain: a town opens the menu and greys Save.
///
/// Without this the test above would pass against a build that simply
/// enabled Save everywhere - which is the failure mode the whole gate exists
/// to prevent, and the one a widened mode gate could plausibly cause.
#[test]
fn a_town_opens_the_menu_but_greys_the_save_row() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("town01", 0).expect("enter town01");

    assert_eq!(host.world.mode, SceneMode::Field);
    assert!(
        host.world.field_menu_open_allowed(),
        "a town has always opened the menu"
    );
    assert!(
        !host.world.scene_save_allowed,
        "town01's MAN clears the save-allow bit"
    );

    let menu = menu_for(&host.world);
    assert!(
        !menu.row_is_available(FieldMenuRow::Save),
        "Save must stay refused in a town - the open gate widened, the row \
         gate did not"
    );
    // Every other row stays offerable: the widening is one gate wide.
    for r in FieldMenuRow::ALL {
        if r != FieldMenuRow::Save {
            assert!(
                menu.row_is_available(r),
                "{r:?} must stay available in a town"
            );
        }
    }
}
