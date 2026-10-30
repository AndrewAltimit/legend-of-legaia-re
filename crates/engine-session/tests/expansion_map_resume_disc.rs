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
    let previous_scene = session.host.world.party.previous_scene.clone();

    // Walk away into a retail scene, then load the save.
    session
        .enter_field_live("town01", &opts)
        .expect("enter town01");
    assert!(session.host.active_expansion_map().is_none());
    let _ = session.resume_save(save.clone(), &resume.scene, &opts);
    assert_eq!(session.host.active_expansion_map(), Some(MAP_ID));
    let ms = &session.host.world.actors[0].move_state;
    assert_eq!((ms.world_x, ms.world_z), at);
    // The map re-entry is a scene change from its base scene; the save's own
    // previous scene is what stands after the load (a soak `+rt` finding).
    assert_eq!(session.host.world.party.previous_scene, previous_scene);
    assert_eq!(session.host.world.save_full().ext_v2, save.ext_v2);
}

/// Every monster the pack's fights seat has a catalog entry and every pack
/// formation is registered - the fight tables a pack battle resolves
/// against.
fn assert_pack_fight_tables(session: &NativeSession, at: &str) {
    let world = &session.host.world;
    let ids = world.expansion_base_monster_ids();
    assert!(!ids.is_empty(), "{at}: the pack seats retail monsters");
    for id in ids {
        assert!(
            world.tables.monster_catalog.get(id).is_some(),
            "{at}: monster {id} has no catalog entry"
        );
    }
    let compiled = world.expansion.battle.compiled.as_ref().expect("compiled");
    for f in &compiled.formations {
        assert!(
            world
                .tables
                .formation_table
                .by_id
                .contains_key(&f.def.formation_id),
            "{at}: pack formation {} is not registered",
            f.def.formation_id
        );
    }
    assert!(
        world.encounters.session.is_none(),
        "{at}: a pack map rolls no retail encounters"
    );
}

/// The live arm runs after the map entry's monster merge; with the live
/// loop on it must not swap the pack's monsters and formations for the
/// synthetic stand-ins (the fights then seat nameless retail-stat monsters).
/// Through the direct entry and the save resume, both of which arm.
#[test]
fn a_live_pack_map_keeps_the_packs_fight_tables() {
    let Some(mut session) = open() else { return };
    let opts = FieldLiveOpts {
        live_loop: true,
        player_battle: true,
        ..FieldLiveOpts::default()
    };
    session
        .enter_expansion_map_live(MAP_ID, "", &opts)
        .expect("enter the pack map");
    assert_pack_fight_tables(&session, "direct entry");

    let resume = session.current_resume();
    let save = session.host.world.save_full();
    session
        .enter_field_live("town01", &opts)
        .expect("enter town01");
    let _ = session.resume_save(save, &resume.scene, &opts);
    assert_eq!(session.host.active_expansion_map(), Some(MAP_ID));
    assert_pack_fight_tables(&session, "save resume");
}
