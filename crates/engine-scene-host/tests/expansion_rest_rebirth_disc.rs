//! The resting check over the flagship pack, with the meshes measured off
//! the disc: every Rebirth map prepares, every `on` resolves, and the items
//! that rest on nothing (floating, off a table's top, overhanging it) are
//! listed (`legaia_expansion_format::map_rest`). Skips without
//! `LEGAIA_DISC_BIN`.

use legaia_engine_scene_host::expansion_preload::PreparedMap;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_expansion_format::ExpansionPack;

#[test]
fn rebirth_items_rest_on_something() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let host = SceneHost::open_disc(disc).expect("open the disc");
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let pack = ExpansionPack::from_dir(std::path::Path::new(dir)).expect("Rebirth loads");
    let mut unresolved = Vec::new();
    let mut resting = 0;
    for map_id in pack.maps.keys() {
        let prepared = PreparedMap::prepare(&host.index, &pack, map_id)
            .unwrap_or_else(|e| panic!("{map_id}: {e:#}"));
        for (_, note) in &prepared.notes {
            if note.contains(": rest: ") {
                resting += 1;
                eprintln!("{note}");
            } else if note.contains(".on: ") {
                unresolved.push(note.clone());
            }
        }
    }
    eprintln!("[rest] {resting} items rest on nothing");
    assert!(unresolved.is_empty(), "{unresolved:#?}");
}
