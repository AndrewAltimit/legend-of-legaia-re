//! Disc-gated: an overworld entrance record's destination is where its
//! control flow ends under the live story flags, not its first `0x3F` in
//! byte order.
//!
//! `map03`'s Nivora entrance (partition-2 record 3) tests flag `0x378` and
//! jumps to NILBOA2 when it is set; the fall-through arm jumps over that
//! scene change to NILBOA. The taken arm comes first in byte order, so a
//! static "first `0x3F` = flag-clear destination" reading installed NILBOA2
//! on both arms and Nivora could never be entered. Record 7 nests three
//! tests: `0x3F2` set closes it, `0x4C8` clear leads to CONCNOW, `0x4C8` set
//! leads to CONCEND unless `0x6C2` is also set.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` or an extracted tree
//! (`$LEGAIA_EXTRACTED_DIR`, then `extracted/` up the tree).

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::{
    RecordPathEnd, overworld_portal_sites, partition2_record_path_scene_change,
};
use legaia_engine_core::scene::{ProtIndex, Scene, SceneHost};
use legaia_engine_core::world::WorldMapEntityConfig;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let env = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from);
    let found = env
        .into_iter()
        .chain(["extracted", "../extracted", "../../extracted"].map(PathBuf::from))
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists());
    if found.is_none() {
        eprintln!("[skip] extracted tree missing (set LEGAIA_EXTRACTED_DIR)");
    }
    found
}

fn dest(end: Option<RecordPathEnd>) -> String {
    match end {
        Some(RecordPathEnd::SceneChange((_, name, ..))) => name,
        Some(RecordPathEnd::Closed) => "<closed>".into(),
        Some(RecordPathEnd::Undecided) => "<undecided>".into(),
        None => "<no record>".into(),
    }
}

#[test]
fn map03_entrances_follow_their_flag_branches() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open PROT");
    let scene = Scene::load(&index, "map03").expect("load map03");
    let man = scene
        .field_man_payload(&index)
        .expect("MAN")
        .expect("map03 has a MAN");
    let mf = legaia_asset::man_section::parse(&man).expect("parse MAN");
    let under = |rec: usize, flags: &[u16]| {
        let set: BTreeSet<u16> = flags.iter().copied().collect();
        dest(partition2_record_path_scene_change(&mf, &man, rec, &|f| {
            set.contains(&f)
        }))
    };

    assert_eq!(under(3, &[]), "nilboa", "0x378 clear: the fall-through arm");
    assert_eq!(under(3, &[0x378]), "nilboa2", "0x378 set: the taken arm");
    assert_eq!(under(7, &[]), "concnow");
    assert_eq!(under(7, &[0x4C8]), "concend");
    assert_eq!(under(7, &[0x4C8, 0x6C2]), "<closed>");
    assert_eq!(under(7, &[0x3F2]), "<closed>");

    // The static portal table the route graph and the viewers read.
    let (p, f) = scene.field_tile_triggers(&index).expect("triggers");
    let triggers: Vec<_> = p.into_iter().chain(f).collect();
    let site = overworld_portal_sites(&mf, &man, &triggers)
        .into_iter()
        .find(|s| s.record == 3)
        .expect("a walk-on band reaches P2[3]");
    assert_eq!(site.scene_name, "nilboa");
    let alt = site.conditional.expect("P2[3] is conditional");
    assert_eq!((alt.flag, alt.scene_name.as_str()), (0x378, "nilboa2"));
    eprintln!("[ran] map03 P2[3] nilboa / nilboa2 on 0x378; P2[7] concnow / concend / closed");
}

#[test]
fn map03_installs_the_portal_its_flags_select() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let portals = |flags: &[u16]| -> BTreeSet<String> {
        let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
        for &f in flags {
            host.world.system_flag_set(f);
        }
        host.enter_world_map_scene("map03").expect("enter map03");
        host.world
            .world_map
            .entity_configs
            .iter()
            .filter_map(|c| match c {
                WorldMapEntityConfig::OverworldPortal { scene_name, .. } => {
                    Some(scene_name.clone())
                }
                _ => None,
            })
            .collect()
    };
    let fresh = portals(&[]);
    assert!(fresh.contains("nilboa"), "installed: {fresh:?}");
    assert!(!fresh.contains("nilboa2"), "installed: {fresh:?}");
    assert!(fresh.contains("concnow"), "installed: {fresh:?}");

    let later = portals(&[0x378, 0x4C8]);
    assert!(later.contains("nilboa2"), "installed: {later:?}");
    assert!(later.contains("concend"), "installed: {later:?}");
    assert!(!later.contains("concnow"), "installed: {later:?}");

    let shut = portals(&[0x3F2]);
    assert!(
        !shut.contains("concnow") && !shut.contains("concend"),
        "0x3F2 closes the Conkram entrance: {shut:?}"
    );
    eprintln!("[ran] map03 portals: fresh {fresh:?}; later {later:?}");
}
