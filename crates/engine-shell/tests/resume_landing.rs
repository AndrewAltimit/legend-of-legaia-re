//! Pins the native half of the shared resume / New Game entry
//! (`BootSession::resume_save` / `BootSession::start_new_game`, both over
//! `legaia_engine_core::resume`). The browser page's `play_resume_save` /
//! `play_new_game` pin the same landings in `crates/web-viewer/src/resume.rs`.
//!
//! Skip-passes without disc data / extracted assets (the `LEGAIA_DISC_BIN`
//! convention).

use std::path::PathBuf;

use legaia_engine_core::resume::ResumeLanding;
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

fn open() -> Option<BootSession> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return None;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    Some(BootSession::open(&extracted, &cfg).expect("open boot session"))
}

#[test]
fn resume_lands_in_the_saved_scene_else_over_the_running_one_never_a_new_game() {
    let Some(mut session) = open() else { return };
    let opts = FieldLiveOpts::default();
    session
        .enter_field_live("town01", &opts)
        .expect("enter town01");

    session.host.world.party.money = 4321;
    let save = session.host.world.save_full();

    // A save naming a scene this host cannot enter loads over the running
    // scene - it neither re-enters anything nor becomes a New Game.
    session.host.world.party.money = 7;
    let landing = session.resume_save(save.clone(), "no_such_scene", &opts);
    assert_eq!(landing, ResumeLanding::CurrentScene("town01".into()));
    assert_eq!(session.host.world.party.money, 4321, "the save applied");
    assert_eq!(
        session.host.scene.as_ref().map(|s| s.name.as_str()),
        Some("town01")
    );

    // The saved scene is entered, even when it is the one already running.
    session.host.world.party.money = 7;
    let landing = session.resume_save(save.clone(), "town01", &opts);
    assert_eq!(landing, ResumeLanding::SavedScene("town01".into()));
    assert_eq!(session.host.world.party.money, 4321);
    session.host.world.party.money = 7;
    let landing = session.resume_save(save.clone(), "town0b", &opts);
    assert_eq!(landing, ResumeLanding::SavedScene("town0b".into()));
    assert!(landing.entered_scene());
    assert_eq!(
        session.host.scene.as_ref().map(|s| s.name.as_str()),
        Some("town0b")
    );
    assert_eq!(
        session.host.world.party.money, 4321,
        "the save lands after the entry"
    );
    eprintln!("[ok] resume_save: unenterable -> current, named -> saved scene");
}

#[test]
fn start_new_game_seeds_and_enters_the_prologue() {
    let Some(mut session) = open() else { return };
    let opts = FieldLiveOpts::default();
    session
        .enter_field_live("town01", &opts)
        .expect("enter town01");
    session.host.world.party.money = 4321;
    let entered = session.start_new_game(&opts);
    assert_eq!(
        entered,
        Some(legaia_asset::new_game::OPENING_CUTSCENE_SCENE),
        "a New Game enters the prologue cutscene scene"
    );
    assert_eq!(
        session.host.world.party.money,
        legaia_engine_core::world::NEW_GAME_STARTING_GOLD
    );
    if session.starting_party.is_some() {
        assert_eq!(session.host.world.party.party_count, 1, "Vahn alone");
    }
    eprintln!("[ok] start_new_game entered {entered:?}");
}
