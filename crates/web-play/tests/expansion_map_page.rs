//! The play page's reads on an expansion-pack map: the state JSON names the
//! map, and the "no random encounters here" hint answers for the map's own
//! encounter zones rather than the base scene's MAN.

use crate::runtime::LegaiaRuntime;

fn rebirth_bundle() -> Vec<u8> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
    legaia_expansion_format::bundle::write_bundle(&files).expect("bundle")
}

/// Disc-gated: Rebirth's Atrium rides a base scene that rolls nothing, yet
/// its encounter zones roll the pack's tables, so the page must not tell
/// the player the map has no fights; and the state JSON names the map.
#[test]
fn a_pack_map_names_itself_and_rolls_its_zones() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let bytes = std::fs::read(disc).expect("read the disc");
    let mut rt = LegaiaRuntime::new();
    assert!(rt.load_disc(bytes, String::new()).is_ok(), "load the disc");
    let v: serde_json::Value =
        serde_json::from_str(&rt.load_expansion_core(&rebirth_bundle())).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    let state: serde_json::Value = serde_json::from_str(
        &rt.enter_expansion_map_core("rebirth_tower_1f", "")
            .expect("enter the pack map"),
    )
    .unwrap();
    assert_eq!(state["expansion_map"]["id"], "rebirth_tower_1f");
    assert_eq!(
        state["expansion_map"]["name"],
        "Tower of Rebirth 1F - Atrium"
    );
    assert!(rt.scene_rolls_encounters(), "the Atrium's zones roll");

    // A retail town is unchanged: no map in the state, and Rim Elm rolls
    // nothing.
    let town: serde_json::Value =
        serde_json::from_str(&rt.enter_field("town01").expect("enter Rim Elm")).unwrap();
    assert!(town["expansion_map"].is_null());
    assert!(!rt.scene_rolls_encounters());
}
