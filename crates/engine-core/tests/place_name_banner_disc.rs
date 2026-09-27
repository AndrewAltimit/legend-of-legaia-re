//! Disc-gated: the place-name banner end to end.
//!
//! `FUN_8003AEB0`'s tail seats a `4C E1` text balloon carrying the scene MAN's
//! section-2 name when system flag 2 is armed, and clears the flag either way
//! (`legaia_engine_core::place_name_banner`). This drives the real loader path,
//! `SceneHost::load_scene`, over every CDNAME scene with the flag armed and
//! asserts the balloon carries exactly the section-2 bytes, then pins the
//! disarmed case and pins which scene scripts raise the flag: the three
//! kingdom (world-map) MANs, plus one more.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::{FlagBank, walk_partition_gflag_sites};
use legaia_engine_core::place_name_banner::{
    PLACE_NAME_BANNER_FLAG, man_scene_name_bytes, record_leading_flag_writes,
};
use legaia_engine_core::scene::{ProtIndex, Scene, SceneHost};
use legaia_engine_core::world::WorldMapEntityConfig;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn every_named_scene_announces_itself_when_the_flag_is_armed() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut scenes: Vec<String> = cdname.values().cloned().collect();
    scenes.sort();
    scenes.dedup();

    let (mut loaded, mut named, mut announced) = (0usize, 0usize, 0usize);
    let mut setters: Vec<String> = Vec::new();
    for scene in &scenes {
        host.world.system_flag_set(PLACE_NAME_BANNER_FLAG);
        host.world.cutscene.text_balloon = None;
        if host.load_scene(scene).is_err() {
            continue;
        }
        loaded += 1;
        assert!(
            !host.world.system_flag_test(PLACE_NAME_BANNER_FLAG),
            "{scene}: the loader clears flag 2 on every path"
        );
        let man = Scene::load(&index, scene)
            .ok()
            .and_then(|s| s.field_man_payload(&index).ok().flatten());
        let name = man.as_deref().map(man_scene_name_bytes).unwrap_or_default();
        match host.world.cutscene.text_balloon.as_ref() {
            Some(b) => {
                announced += 1;
                assert_eq!(b.text, name, "{scene}: the balloon is the section-2 name");
                assert!(!b.parent_link, "{scene}: the loader stores no parent link");
            }
            None => assert!(name.is_empty(), "{scene}: a named scene must announce"),
        }
        if !name.is_empty() {
            named += 1;
        }
        if let Some(man) = man.as_deref()
            && let Ok(mf) = legaia_asset::man_section::parse(man)
        {
            let sets = (0..3)
                .flat_map(|p| walk_partition_gflag_sites(&mf, man, p))
                .filter(|s| {
                    s.set
                        && s.clean
                        && s.bank == FlagBank::System
                        && s.flag == PLACE_NAME_BANNER_FLAG
                })
                .count();
            if sets > 0 {
                setters.push(format!("{scene}x{sets}"));
            }
        }
    }
    eprintln!(
        "[ok] {loaded} scenes loaded, {named} named, {announced} announced; \
         flag-2 SET sites in {} scene MANs: {setters:?}",
        setters.len()
    );
    assert!(loaded > 50, "the corpus loaded");
    assert_eq!(named, announced, "every named scene announces, no other");
    assert!(named > 0, "non-vacuous: some scene carries a name");
    // Who arms it: the three kingdom (world-map) MANs, i.e. leaving the
    // overworld into a place announces the place.
    for kingdom in ["map01", "map02", "map03"] {
        assert!(
            setters
                .iter()
                .any(|s| s.starts_with(&format!("{kingdom}x"))),
            "{kingdom}'s scripts raise flag 2"
        );
    }

    // Disarmed: no balloon, and a live one is left alone.
    host.world.system_flag_clear(PLACE_NAME_BANNER_FLAG);
    host.world.cutscene.text_balloon = None;
    host.load_scene("town01").expect("town01");
    assert!(host.world.cutscene.text_balloon.is_none());
}

/// Walking off the overworld into Rim Elm announces it: the entrance record's
/// leading `50 02` is replayed by the transition drain
/// (`place_name_banner::record_leading_flag_writes`), and the destination's
/// MAN loader turns the flag into the banner.
#[test]
fn walking_into_rim_elm_from_the_overworld_raises_the_banner() {
    use legaia_engine_core::input::PadButton;
    use legaia_engine_core::scene::SceneTickEvent;
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_world_map_scene("map01").expect("enter map01");
    for _ in 0..3 {
        host.tick().expect("tick");
    }
    assert!(!host.world.system_flag_test(PLACE_NAME_BANNER_FLAG));
    host.world.cutscene.text_balloon = None;
    // How many of map01's installed entrances announce their destination:
    // the ones whose opening flag operations raise flag 2.
    {
        let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
        let man = Scene::load(&index, "map01")
            .expect("map01")
            .field_man_payload(&index)
            .expect("man")
            .expect("map01 has a MAN");
        let mf = legaia_asset::man_section::parse(&man).expect("walk map01");
        let (mut portals, mut announcing) = (0usize, 0usize);
        for c in &host.world.world_map.entity_configs {
            if let WorldMapEntityConfig::OverworldPortal { record, .. } = c {
                portals += 1;
                let writes = record_leading_flag_writes(&mf, &man, usize::from(*record), |i| {
                    host.world.system_flag_test(i)
                });
                if writes.contains(&(true, PLACE_NAME_BANNER_FLAG)) {
                    announcing += 1;
                }
            }
        }
        eprintln!("[ok] map01: {announcing} of {portals} installed entrances raise flag 2");
        assert!(announcing > 0, "non-vacuous");
    }
    // The Rim Elm entrance, one tile Z- of the exit's arrival seat (the same
    // walk `scene_round_trip_disc` pins the arrival of).
    let slot = host.world.player_actor_slot.expect("player installed") as usize;
    host.world.actors[slot].move_state.world_x = 0x60 * 128 + 0x40;
    host.world.actors[slot].move_state.world_z = 0x19 * 128 + 0x40;
    for _ in 0..2 {
        host.tick().expect("tick");
    }
    let mut entered = None;
    for _ in 0..600 {
        host.world.set_pad(PadButton::Down.mask());
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            entered = Some(name);
            break;
        }
    }
    host.world.set_pad(0);
    assert_eq!(
        entered.as_deref(),
        Some("town0c"),
        "the walk re-enters Rim Elm"
    );
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let man = Scene::load(&index, "town0c")
        .expect("town0c")
        .field_man_payload(&index)
        .expect("man")
        .expect("town0c has a MAN");
    let name = man_scene_name_bytes(&man);
    let balloon = host
        .world
        .cutscene
        .text_balloon
        .as_ref()
        .expect("the entrance record raised flag 2 and the loader seated the banner");
    assert_eq!(balloon.text, name);
    assert!(
        !host.world.system_flag_test(PLACE_NAME_BANNER_FLAG),
        "consumed"
    );
    eprintln!("[ok] town0c banner: {:?}", String::from_utf8_lossy(&name));
}
