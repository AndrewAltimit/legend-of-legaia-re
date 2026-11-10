//! Disc-gated: the pause menu waits out its windows' slides before it reads
//! the pad, as retail's sub-screens do (`_DAT_8007BB80`,
//! `legaia_engine_menus::menu_slide_lock`).
//!
//! The session installs the menu overlay's window programs at boot, so a
//! headless session runs the lock the play hosts run. Down is pressed on
//! every other frame from the frame the wipe spawns the menu: the root
//! picker's phase-0 frame and the eleven frames its slide-in holds the latch
//! read nothing, and the press after that moves the hand. Confirming a row
//! hands off to its sub-screen, whose own slide holds the pad again.
//!
//! Skip-passes without disc data.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Tick with `pad` held this frame; the pad is released on the next call
/// with `0`, so presses on alternate frames are each an edge.
fn tick(session: &mut NativeSession, pad: u16) {
    session.host.world.input.set_pad(pad);
    session.tick().expect("tick");
}

#[test]
fn the_pause_menu_reads_no_pad_while_its_windows_slide() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open extracted boot session");
    session
        .enter_field_live("town01", &FieldLiveOpts::default())
        .expect("enter town01 live");
    assert!(
        session.host.world.menu.widget_scripts.is_some(),
        "the session installs the menu overlay's programs"
    );

    // Settle the scene until Start is accepted (the entry script holds the
    // menu lock for its first beats).
    let mut settle = 0;
    while !session.host.world.field_menu_open_allowed() {
        tick(&mut session, 0);
        settle += 1;
        assert!(settle < 600, "Start never allowed");
    }
    assert!(
        session.press_field_menu(true) != legaia_engine_session::session::PauseMenuPress::None,
        "Start opens the menu"
    );
    let mut guard = 0;
    while !session.pause_wipe().menu_spawned() {
        tick(&mut session, 0);
        guard += 1;
        assert!(guard < 64, "the wipe spawns the menu");
    }
    let cursor = |s: &NativeSession| s.field_menu.as_ref().expect("menu").cursor();
    let mut frame = 0;
    while cursor(&session) == 0 {
        let pad = if frame % 2 == 0 {
            PadButton::Down.mask()
        } else {
            0
        };
        tick(&mut session, pad);
        frame += 1;
        assert!(frame < 40, "Down never landed");
    }
    // Frames 0..=11 are the root's phase 0 and its eleven held frames; the
    // first Down on frame 12 moves the hand (`frame` counts it).
    assert_eq!(frame, 13, "the first press after the slide-in");
}
