//! Disc-gated: an expansion pack active on the native session plays through
//! New Game and Continue. The committed `packs/rebirth` is activated as
//! `play-window --expansion` does; a New Game keeps it active with fresh
//! progress, a save made with it carries its progress back through a load,
//! and a session without the pack loads the same save, says so, and keeps
//! the block. Skips when `LEGAIA_DISC_BIN` is unset.

use std::sync::Arc;

use legaia_engine_expansion::quest::QuestEvent;
use legaia_engine_save_state::WorldSaveState;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_expansion_format::ExpansionPack;

fn open(with_pack: bool) -> Option<NativeSession> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = ["extracted", "../extracted", "../../extracted"]
        .into_iter()
        .map(std::path::PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists());
    let Some(extracted) = extracted else {
        eprintln!("[skip] extracted/ missing");
        return None;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open boot session");
    if with_pack {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
        let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
        let pack = ExpansionPack::from_files(&files).expect("the Rebirth pack parses");
        session
            .host
            .activate_expansion_pack(Arc::new(pack))
            .expect("the Rebirth pack activates");
    }
    Some(session)
}

#[test]
fn new_game_and_continue_play_with_the_pack() {
    let Some(mut session) = open(true) else {
        return;
    };
    let opts = FieldLiveOpts::default();
    // Progress from before the New Game does not survive it.
    session
        .host
        .world
        .expansion_notify(QuestEvent::FlagSet("stale".into()));
    let entered = session.start_new_game(&opts);
    assert!(entered.is_some(), "the New Game entered its opening");
    let world = &session.host.world;
    assert!(world.expansion.is_active(), "the pack plays the New Game");
    assert_eq!(world.expansion.quests.progress.pack_id, "rebirth");
    assert!(!world.expansion.quests.progress.flags.is_set("stale"));

    // A save made with the pack carries its progress back through a load.
    session
        .host
        .world
        .expansion_notify(QuestEvent::FlagSet("tower_heard".into()));
    let resume = session.current_resume();
    let save = session.host.world.save_full();
    let bytes = save.write();
    assert!(
        save.ext_v2.expansion.is_some(),
        "the save carries the block"
    );
    let reloaded = legaia_save::SaveFile::parse(&bytes).expect("the save parses");

    let Some(mut again) = open(true) else { return };
    let _ = again.resume_save(reloaded.clone(), &resume.scene, &opts);
    let world = &again.host.world;
    assert!(world.expansion.quests.progress.flags.is_set("tower_heard"));
    assert_eq!(world.expansion_dormant_pack_id(), None);

    // Without the pack the save still loads, names its pack, keeps the block.
    let Some(mut bare) = open(false) else { return };
    let _ = bare.resume_save(reloaded, &resume.scene, &opts);
    let world = &mut bare.host.world;
    assert!(!world.expansion.is_active());
    assert_eq!(
        world.expansion_dormant_pack_id().as_deref(),
        Some("rebirth")
    );
    assert_eq!(
        world.expansion_toast(),
        Some("Saved with expansion 'rebirth' - not loaded")
    );
    assert_eq!(world.expansion_save_block(), save.ext_v2.expansion);
}
