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
