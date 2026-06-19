//! Disc-gated enumeration test for scene-transition ("door / exit") sites: walk
//! the whole disc, locate every `0x3F` named-scene-change op via the clean
//! partition walk, and assert the census is sane - a healthy door count across
//! many scenes, the pinned town01 -> map01 exit present, every site carrying a
//! clean CDNAME-shaped destination name. Skips + passes without `LEGAIA_DISC_BIN`.

use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

#[test]
fn doors_enumerate_across_the_disc() {
    let Some(image) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(image).expect("open disc");
    let doors = apply::current_doors(&patcher).expect("enumerate doors");

    // A healthy corpus: many doors across many scenes (the retail disc has 160
    // across 48 scenes; assert comfortably under that so the test is stable).
    assert!(doors.len() >= 120, "found only {} doors", doors.len());
    let scenes: std::collections::BTreeSet<&str> =
        doors.iter().map(|d| d.home_scene.as_str()).collect();
    assert!(
        scenes.len() >= 30,
        "doors span only {} scenes",
        scenes.len()
    );

    // Every destination name is a clean CDNAME-shaped label (lowercase + digits,
    // 3..=12 chars) - the clean walk's gate; no text-desync phantoms.
    for d in &doors {
        assert!(
            (3..=12).contains(&d.dest_scene.len())
                && d.dest_scene
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()),
            "door in {} -> {:?} is not a clean scene label",
            d.home_scene,
            d.dest_scene
        );
    }

    // The pinned Rim Elm exit: town01 (PROT 4) -> map01 at op offset 0x6f95.
    let town01 = doors
        .iter()
        .find(|d| d.entry_idx == 4 && d.op_pc == 0x6f95)
        .expect("town01 exit present");
    assert_eq!(town01.home_scene, "town01");
    assert_eq!(town01.dest_scene, "map01");
    assert_eq!(town01.index, 85);

    // Overworld hubs fan out: map01 (PROT 86) carries many exits.
    let map01_doors = doors.iter().filter(|d| d.entry_idx == 86).count();
    assert!(map01_doors >= 20, "map01 has only {map01_doors} exits");
}
