//! Post-FMV control transfer: the contract behind the `play` command's
//! hand-off call site.
//!
//! `legaia-engine play` runs `fmv_post_play_handoff` after cutscene playback
//! completes and applies the `Field` arm by entering the named scene. Two
//! things have to hold for that to be more than a log line:
//!
//! 1. every retail `fmv_id` resolves to an arm the host can act on, and the
//!    two the engine cannot act on are exactly the two mode arms; and
//! 2. every `Field` arm names a CDNAME scene the boot index can actually
//!    load - a typo or a renamed label would leave the player stranded in
//!    the trigger scene with only a printed error.
//!
//! Part 1 is disc-free. Part 2 is disc-gated and skip-passes without
//! `LEGAIA_DISC_BIN` + extracted assets, per the repo convention.

use std::path::PathBuf;

use legaia_engine_core::cutscene::{FmvHandoff, fmv_post_play_handoff};
use legaia_engine_shell::boot::{BootConfig, BootSession};

/// The nine `fmv_id`s that address a movie on the retail disc.
const RETAIL_FMV_IDS: std::ops::RangeInclusive<i16> = 0..=8;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Every retail movie hands control somewhere the `play` host recognises,
/// and only `fmv_id 0` needs a game mode the engine does not carry.
#[test]
fn every_retail_fmv_id_has_an_arm_the_host_handles() {
    let mut field_arms = 0;
    for id in RETAIL_FMV_IDS {
        match fmv_post_play_handoff(id) {
            // Applied by the host: it enters `scene`.
            FmvHandoff::Field { scene, .. } => {
                assert!(!scene.is_empty(), "fmv {id}: empty scene label");
                field_arms += 1;
            }
            // Applied by the host as a no-op: the trigger scene resumes.
            FmvHandoff::ResumeField => {}
            // Reported, not applied - game mode 22 is not in `SceneMode`.
            FmvHandoff::CardInit { .. } => assert_eq!(id, 0, "only the intro is CardInit"),
            other => panic!("fmv {id}: retail id resolved to {other:?}"),
        }
    }
    // 1..=4 and 6..=8 are the seven scene-returning movies; 5 resumes.
    assert_eq!(field_arms, 7);
    assert_eq!(fmv_post_play_handoff(5), FmvHandoff::ResumeField);
}

/// The two arms the host deliberately leaves unhandled are the two that
/// need a game mode the engine has no `SceneMode` for. Pinning them so a
/// future mode addition shows up here as a failing assertion rather than a
/// silently still-unhandled arm.
#[test]
fn only_the_two_mode_arms_are_unhandled() {
    assert!(matches!(
        fmv_post_play_handoff(0),
        FmvHandoff::CardInit { card_arg: 2 }
    ));
    assert!(matches!(
        fmv_post_play_handoff(9),
        FmvHandoff::CardInit { card_arg: 1 }
    ));
    assert_eq!(fmv_post_play_handoff(10), FmvHandoff::ModeZero);
    for id in 11..=22 {
        assert_eq!(fmv_post_play_handoff(id), FmvHandoff::None, "dev slot {id}");
    }
}

/// Disc-gated: each `Field` arm's CDNAME label loads and enters as a field
/// scene, which is exactly what the `play` hand-off does after playback.
#[test]
fn every_field_handoff_scene_enters() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let mut checked = 0;
    for id in RETAIL_FMV_IDS {
        let FmvHandoff::Field { scene, .. } = fmv_post_play_handoff(id) else {
            continue;
        };
        let cfg = BootConfig {
            scene: scene.to_string(),
            enable_audio: false,
        };
        let mut session = match BootSession::open(&extracted, &cfg) {
            Ok(s) => s,
            Err(e) => panic!("fmv {id}: boot on hand-off scene '{scene}' failed: {e:#}"),
        };
        session
            .host
            .enter_field_scene(scene, 0)
            .unwrap_or_else(|e| panic!("fmv {id}: hand-off scene '{scene}' did not enter: {e:#}"));
        assert_eq!(
            session.host.world.mode,
            legaia_engine_core::world::SceneMode::Field,
            "fmv {id}: hand-off left the world off the field"
        );
        checked += 1;
    }
    assert_eq!(checked, 7, "all seven scene-returning movies were checked");
}

/// The hand-off is a **one-shot edge**, not a per-frame condition.
///
/// All three hosts (`play`, `play-window`, the browser play page) now call
/// `SceneHost::apply_pending_fmv_handoff`, and the windowed host calls it at
/// two points in one frame. If the transfer were keyed on anything but a
/// drained edge, the second caller would re-enter the hand-off scene - which
/// on a scene whose entry script has side effects is a duplicated cutscene,
/// not a wasted load. So the assertion is the *contrast*: the first call
/// transfers, every later call reports nothing.
#[test]
fn the_handoff_transfers_once_however_many_hosts_poll() {
    use legaia_engine_core::scene::FmvHandoffOutcome;
    use legaia_engine_core::world::SceneMode;

    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    // `town01` is the retail trigger scene for `fmv_id 1`, whose hand-off
    // lands in `town0b` - the pairing the dispatch table encodes.
    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("boot town01");
    session.host.enter_field_scene("town01", 0).expect("enter");
    assert_eq!(
        session.host.scene.as_ref().map(|s| s.name.as_str()),
        Some("town01")
    );

    // Stand in for playback: the world is in the cutscene mode with fmv 1
    // live, exactly as the field VM's `0x4C 0xE2` op leaves it.
    session.host.world.mode = SceneMode::Cutscene;
    session.host.world.active_fmv = Some(1);
    session.host.world.cutscene_return_mode = Some(SceneMode::Field);
    session.host.world.finish_cutscene();

    match session.host.apply_pending_fmv_handoff() {
        Some(FmvHandoffOutcome::Entered { fmv_id, scene, .. }) => {
            assert_eq!(fmv_id, 1);
            assert_eq!(scene, "town0b");
        }
        other => panic!("first poll did not transfer: {other:?}"),
    }
    assert_eq!(
        session.host.scene.as_ref().map(|s| s.name.as_str()),
        Some("town0b"),
        "the hand-off scene is the one that is loaded"
    );
    assert_eq!(
        session.host.apply_pending_fmv_handoff(),
        None,
        "a second poll must not re-enter the hand-off scene"
    );
    assert_eq!(
        session.host.apply_pending_fmv_handoff(),
        None,
        "nor any later one"
    );
}
