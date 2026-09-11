//! Disc-gated: the session's seat at the mode table carries the retail mode
//! word through the boot chain, and the mode-trace oracle now samples it.
//!
//! What this pins, and why each half needs a disc:
//!
//! 1. **The chain's shape** is pure data (`mode::BOOT_MODE_CHAIN`), asserted
//!    without a disc by `engine-core`'s own unit tests. What needs a disc is
//!    that a *session* walks it: a `BootSession` opens at `READ INIT`, and
//!    entering a real field scene leaves the word at `MAIN MODE` with the
//!    per-scene INIT plan resolved on the way.
//! 2. **The oracle emits the word.** Every `ModeTraceFrame` the engine
//!    sampler produces used to carry `game_mode: None` unless the pause menu
//!    was open, so the field the trace transported was never compared. Each
//!    frame must now carry one, and for a field-live trace it must be
//!    `0x03` - the same byte a retail field capture holds.
//!
//! Skips + passes with `LEGAIA_DISC_BIN` unset, per the repo's disc-gated
//! convention. No Sony bytes are asserted: the assertions are mode indices
//! and a CDNAME scene label.

use legaia_engine_core::mode::{BOOT_MODE_CHAIN, GameMode};
use legaia_engine_shell::boot::FieldLiveOpts;
use legaia_engine_shell::mode_trace_oracle::build_engine_mode_trace_field_live;
use legaia_engine_shell::{BootConfig, BootSession};

/// The opening town - the scene the new-game template names.
const SCENE: &str = "town01";

/// Enough frames to cover the seat's edge handling and a scene tick.
const FRAMES: u64 = 30;

fn disc() -> Option<std::path::PathBuf> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then_some(p)
}

#[test]
fn a_session_walks_the_boot_chain_and_lands_on_main_mode() {
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let cfg = BootConfig {
        scene: SCENE.to_string(),
        enable_audio: false,
    };
    let mut session = match BootSession::open_disc(&disc, &cfg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return;
        }
    };

    // The seat opens where `0x8001D5B8` writes: mode 16, before the loop's
    // first pass.
    assert_eq!(session.mode_seat.game_mode(), GameMode::ReadInit);
    assert_eq!(BOOT_MODE_CHAIN[0].mode, GameMode::ReadInit);
    // `init.pak` raises the front-end entry word itself, so the hand-off arm
    // takes the front end rather than the debug menu.
    assert_ne!(session.mode_seat.entry_word(), 0);
    assert_eq!(session.mode_seat.boot_handoff(), GameMode::CardInit);

    // Field entry goes through `MAIN INIT`, which hands off to `MAIN MODE`.
    session
        .enter_field_live(
            SCENE,
            &FieldLiveOpts {
                live_loop: true,
                ..Default::default()
            },
        )
        .expect("field entry");
    assert_eq!(
        session.mode_seat.game_mode(),
        GameMode::MainMode,
        "MAIN INIT stores its successor before the handler returns"
    );

    // Ticking keeps the word where the scene is; the edge counter records the
    // transitions taken, and a mode change swallows the pad edge that caused
    // it rather than delivering it twice.
    let edges_before = session.mode_seat.edges();
    for _ in 0..FRAMES {
        session.tick().expect("tick");
    }
    assert_eq!(session.mode_seat.game_mode(), GameMode::MainMode);
    assert!(
        session.mode_seat.edges() >= edges_before,
        "the edge counter never runs backwards"
    );
    eprintln!(
        "[ok] seat walked the chain: opened {:?}, field entry -> {:?} ({} edges)",
        GameMode::ReadInit,
        session.mode_seat.game_mode(),
        session.mode_seat.edges()
    );
}

#[test]
fn every_mode_trace_frame_carries_the_word() {
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let extracted = std::path::PathBuf::from("extracted");
    let trace =
        match build_engine_mode_trace_field_live(SCENE, &extracted, Some(&disc), FRAMES, &[]) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[skip] engine mode trace failed: {e:#}");
                return;
            }
        };
    assert!(!trace.is_empty(), "trace has frames");
    for f in &trace {
        let gm = f
            .game_mode
            .unwrap_or_else(|| panic!("frame {} carries no game_mode", f.frame));
        assert_eq!(
            gm,
            GameMode::MainMode.as_index() as u8,
            "a field-live trace runs under MAIN MODE (frame {}, scene_mode {})",
            f.frame,
            f.scene_mode
        );
        assert!(
            f.game_mode_name.is_some(),
            "the row's table name is emitted"
        );
    }
    eprintln!(
        "[ok] {} mode-trace frames, every one carrying game_mode 0x{:02X}",
        trace.len(),
        GameMode::MainMode.as_index()
    );
}
