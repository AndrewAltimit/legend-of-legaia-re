//! Disc-gated: the pause menu's **Save row gate** against real scene MANs.
//!
//! Retail's MAN loader copies the header bit `MAN[0x01] & 1` into the
//! per-scene save-allow byte `_DAT_8007B6A8` (`FUN_8003AEB0` at
//! `0x8003AF48..0x8003AF54`, a byte-wide `lbu`/`andi`/`sb`), and the pause
//! menu's row renderer greys the Save row - and its confirm arm buzzes it -
//! whenever that byte is zero (`FUN_801CFD68` at `0x801D008C`, `FUN_801D6B20`).
//!
//! This drives the engine's real chain on disc data:
//!
//! 1. `SceneHost::enter_field_scene` -> `load_scene` seeds
//!    `World::scene_save_allowed` from the scene's own MAN header.
//! 2. The menu-open sample (`FieldMenuGate`, what
//!    `BootSession::open_field_menu` builds) turns that into the row's ink.
//!
//! It also censuses the whole scene corpus, because a gate that never fires
//! either way is not a gate. On the retail disc the bit is set on exactly the
//! three kingdom world maps and clear on every field scene - which is the
//! game's own rule that you save on the overworld, not in a town.
//!
//! Skips (and passes) without `extracted/` or `LEGAIA_DISC_BIN`.

use std::path::PathBuf;

use legaia_engine_core::field_menu::{FieldMenuGate, FieldMenuRow, FieldMenuSession};
use legaia_engine_core::scene::SceneHost;

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
fn menu_for(world: &legaia_engine_core::world::World) -> FieldMenuSession {
    let mut s = FieldMenuSession::new();
    s.set_gate(FieldMenuGate {
        entry_context_kind: world.menu_entry_context_kind(),
        save_allowed: world.scene_save_allowed,
    });
    s
}

/// Census over every CDNAME scene: both branches of the gate must be
/// populated, or the gate is decoration.
#[test]
fn the_save_allow_bit_splits_the_scene_corpus() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();

    let mut allow: Vec<String> = Vec::new();
    let mut block: Vec<String> = Vec::new();
    let mut no_man = 0usize;

    let index = host.index.clone();
    for name in &names {
        if host.load_scene(name).is_err() {
            continue;
        }
        let has_man = host
            .scene
            .as_ref()
            .and_then(|s| s.field_man_payload(&index).ok().flatten())
            .is_some();
        if !has_man {
            no_man += 1;
            continue;
        }
        if host.world.scene_save_allowed {
            allow.push(name.clone());
        } else {
            block.push(name.clone());
        }
    }

    eprintln!(
        "save-allow census: {} permit, {} forbid, {} carry no MAN",
        allow.len(),
        block.len(),
        no_man,
    );
    eprintln!("permit: {allow:?}");

    assert!(
        !allow.is_empty(),
        "no scene permits saving - the gate would never open"
    );
    assert!(
        !block.is_empty(),
        "no scene forbids saving - the gate would never fire"
    );
    // The permitting set is the three kingdom world maps. Asserted as a set
    // membership rather than an exact list so a detector change that adds a
    // scene fails loudly on the *shape* (a field scene sneaking in), not on a
    // count.
    for s in &allow {
        assert!(
            s.starts_with("map"),
            "only the world maps permit saving; '{s}' does not look like one"
        );
    }
    assert!(
        block.iter().any(|s| s == "town01"),
        "Rim Elm is a field scene and must forbid saving"
    );
}

/// The live chain on one scene of each kind: scene entry seeds the world, the
/// menu-open sample inks the row.
#[test]
fn scene_entry_seeds_the_save_row_ink() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let save_idx = FieldMenuRow::Save.index() as usize;
    let load_idx = FieldMenuRow::Load.index() as usize;

    // A field scene: the MAN clears the bit, so Save greys and buzzes.
    host.enter_field_scene("town01", 0).expect("enter town01");
    assert!(
        !host.world.scene_save_allowed,
        "town01's MAN clears the save-allow bit"
    );
    let mut menu = menu_for(&host.world);
    let view = menu.view();
    assert_eq!(view.rows[load_idx].row, FieldMenuRow::Load, "row 5 is Load");
    assert_eq!(view.rows[save_idx].row, FieldMenuRow::Save, "row 6 is Save");
    assert!(
        !view.rows[save_idx].enabled,
        "Save must draw grey in town01"
    );
    assert!(view.rows[load_idx].enabled, "Load is not gated here");

    menu.tick(legaia_engine_core::field_menu::FieldMenuInput {
        up: true,
        ..Default::default()
    });
    assert_eq!(menu.cursor(), FieldMenuRow::Save.index(), "still navigable");
    let evs = menu.tick(legaia_engine_core::field_menu::FieldMenuInput {
        cross: true,
        ..Default::default()
    });
    assert!(
        evs.contains(
            &legaia_engine_core::field_menu::FieldMenuEvent::InvalidConfirm {
                row: FieldMenuRow::Save
            }
        ),
        "confirming a greyed Save buzzes"
    );
    assert!(!menu.is_suspended());

    // A world map: the same chain the other way.
    host.enter_field_scene("map01", 0).expect("enter map01");
    assert!(
        host.world.scene_save_allowed,
        "map01's MAN sets the save-allow bit"
    );
    let mut menu = menu_for(&host.world);
    assert!(
        menu.view().rows[save_idx].enabled,
        "Save draws white on map01"
    );
    menu.tick(legaia_engine_core::field_menu::FieldMenuInput {
        up: true,
        ..Default::default()
    });
    let evs = menu.tick(legaia_engine_core::field_menu::FieldMenuInput {
        cross: true,
        ..Default::default()
    });
    assert!(
        evs.contains(&legaia_engine_core::field_menu::FieldMenuEvent::Confirmed {
            row: FieldMenuRow::Save
        })
    );

    // Re-entering the field scene clears it again: the flag is per-scene, not
    // a latch.
    host.enter_field_scene("town01", 0)
        .expect("re-enter town01");
    assert!(!host.world.scene_save_allowed);
}
