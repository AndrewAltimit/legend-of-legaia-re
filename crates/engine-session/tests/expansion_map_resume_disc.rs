//! Disc-gated: a save made on an expansion-pack map resumes on that map.
//! The pack is the committed `packs/rebirth`; the session is the native one
//! `play-window` runs (`NativeSession::resume_save`). Skips when
//! `LEGAIA_DISC_BIN` is unset.

use std::sync::Arc;

use legaia_engine_save_state::WorldSaveState;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_expansion_format::ExpansionPack;

const MAP_ID: &str = "rebirth_tower_1f";

fn open() -> Option<NativeSession> {
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
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
    let pack = ExpansionPack::from_files(&files).expect("the Rebirth pack parses");
    session
        .host
        .activate_expansion_pack(Arc::new(pack))
        .expect("the Rebirth pack activates");
    Some(session)
}

#[test]
fn a_save_on_a_pack_map_resumes_on_the_map_where_it_was_made() {
    let Some(mut session) = open() else { return };
    let opts = FieldLiveOpts::default();
    session
        .enter_expansion_map_live(MAP_ID, "dais", &opts)
        .expect("enter the pack map");
    let at = {
        let ms = &session.host.world.actors[0].move_state;
        (ms.world_x, ms.world_z)
    };
    let resume = session.current_resume();
    let save = session.host.world.save_full();

    // Walk away into a retail scene, then load the save.
    session
        .enter_field_live("town01", &opts)
        .expect("enter town01");
    assert!(session.host.active_expansion_map().is_none());
    let _ = session.resume_save(save, &resume.scene, &opts);
    assert_eq!(session.host.active_expansion_map(), Some(MAP_ID));
    let ms = &session.host.world.actors[0].move_state;
    assert_eq!((ms.world_x, ms.world_z), at);
}
