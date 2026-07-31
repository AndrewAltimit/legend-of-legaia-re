//! Disc-gated: `BootSession::open_field_menu` carries the scene's save
//! permission onto the pause menu it builds.
//!
//! This is the production path the windowed host and the headless tick share:
//! the Start edge calls `open_field_menu`, which samples the world's
//! `scene_save_allowed` (seeded at scene load from the MAN header bit retail
//! copies into `_DAT_8007B6A8`) into the session's `FieldMenuGate`. The
//! renderer then greys any row the session reports disabled.
//!
//! Rim Elm is a field scene, so its MAN clears the bit and the Save row must
//! come up grey; the Load row beside it must not.
//!
//! Skip-passes without disc data.

use std::path::PathBuf;

use legaia_engine_core::field_menu::FieldMenuRow;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn opening_the_pause_menu_in_a_no_save_scene_greys_the_save_row() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open extracted boot session");
    session
        .enter_field_live("town01", &FieldLiveOpts::default())
        .expect("enter town01 live");

    assert!(
        !session.host.world.scene_save_allowed,
        "town01's MAN clears the save-allow bit"
    );

    session.open_field_menu();
    let menu = session
        .field_menu
        .as_ref()
        .expect("open_field_menu builds a session");

    // The gate arrived from the world, not from a default.
    assert!(!menu.gate().save_allowed);

    let view = menu.view();
    let save = &view.rows[FieldMenuRow::Save.index() as usize];
    let load = &view.rows[FieldMenuRow::Load.index() as usize];
    assert_eq!(save.row, FieldMenuRow::Save, "row 6 is Save");
    assert_eq!(load.row, FieldMenuRow::Load, "row 5 is Load");
    assert!(
        !save.enabled,
        "the renderer greys a disabled row; Save must be disabled in a field scene"
    );
    assert!(
        load.enabled,
        "Load is gated on the entry context, not the MAN"
    );

    // Every other row stays offerable - the gate is one row wide.
    for r in FieldMenuRow::ALL {
        if r != FieldMenuRow::Save {
            assert!(
                view.rows[r.index() as usize].enabled,
                "{r:?} must stay available"
            );
        }
    }

    // Closing and re-opening re-samples rather than caching a stale gate.
    session.close_field_menu();
    session.open_field_menu();
    assert!(
        !session
            .field_menu
            .as_ref()
            .expect("re-opened")
            .gate()
            .save_allowed
    );
}

/// Start is inert while a dialogue engagement owns the player.
///
/// Retail's menu-open accept lives in the pre-movement header of
/// `FUN_801D01B0`, **after** the engaged-bit branch at `0x801D01F0`
/// (`lw v0,0x10(v0)` / `lui v1,0x8` / `and` / `bne -> 0x801D0334`). With
/// `player+0x10 & 0x80000` raised - which the touch post `FUN_801D5B5C` does
/// on every talk - the pad never reaches the accept, so no menu opens and not
/// even the refusal buzz plays. This asserts the engine refuses through the
/// same production entry point the windowed host's Start edge calls, for both
/// dialogue channels.
#[test]
fn start_is_inert_while_a_dialogue_owns_the_player() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open extracted boot session");
    session
        .enter_field_live("town01", &FieldLiveOpts::default())
        .expect("enter town01 live");

    // Control: with nothing up, Start opens the menu.
    session.open_field_menu();
    assert!(
        session.field_menu_is_open(),
        "control: Start opens the pause menu in an idle field"
    );
    session.close_field_menu();

    // The inline field-VM runner alone - the ordinary NPC talk, which for a
    // prologue-selected record sets no `current_dialog` at all.
    session
        .host
        .world
        .start_inline_dialogue(vec![0x1F, b'h', b'i', 0x00, 0x21]);
    session.open_field_menu();
    assert!(
        !session.field_menu_is_open(),
        "Start must be refused while the inline dialogue runner owns the player"
    );
    session.host.world.inline_dialogue = None;

    // And the simplified request channel.
    session.host.world.current_dialog = Some(legaia_engine_core::world::DialogRequest {
        text_id: 0,
        inline: Vec::new(),
        world_x: 0,
        world_z: 0,
        depth_id: 0,
    });
    session.open_field_menu();
    assert!(
        !session.field_menu_is_open(),
        "Start must be refused while a dialog request owns the player"
    );

    // Once the box is gone, Start works again.
    session.host.world.current_dialog = None;
    session.open_field_menu();
    assert!(
        session.field_menu_is_open(),
        "the refusal is scoped to the conversation, not permanent"
    );
}
