//! Disc-gated: the battle-camera per-character height table reaches the
//! `World` through the real `SceneHost` disc path.
//!
//! `legaia_asset::battle_camera_table` is pinned against raw PROT 0898 bytes
//! by the asset crate's own oracle; what this covers is the wiring - that
//! entering a scene installs the parsed table on
//! [`World::battle_camera_heights`], so the battle camera frames a non-Vahn
//! seat at that character's own height instead of falling back to Vahn's.
//! Skips and passes without `LEGAIA_DISC_BIN` (the workspace convention).

use std::path::PathBuf;

use legaia_engine_core::scene::SceneHost;

fn disc_path() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then_some(p)
}

#[test]
fn scene_entry_installs_the_battle_camera_height_table() {
    let Some(disc) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc host");
    assert!(
        host.world.battle_camera_heights.is_none(),
        "the table is installed lazily on scene entry, not at host open"
    );

    host.enter_field_scene("town01", 0).expect("enter town01");

    let table = host
        .world
        .battle_camera_heights
        .as_ref()
        .expect("scene entry installs the battle-camera height table");

    // Vahn's entry is the value the solo-Vahn camera trace pinned, so it
    // anchors the whole table to the measurement.
    assert_eq!(table.height_for_char_id(1), Some(0x480), "Vahn");
    // The other three are per-model heights a solo trace cannot observe -
    // the point of reading them off the disc rather than guessing.
    assert_eq!(table.height_for_char_id(2), Some(0x3C0), "Noa");
    assert_eq!(table.height_for_char_id(3), Some(0x580), "Gala");
    assert_eq!(table.height_for_char_id(4), Some(0x200), "fourth character");
    assert_eq!(table.height_for_char_id(0), None, "char ids are 1-based");

    // No two battle-party members share a height, which is exactly why the
    // camera cannot keep using one constant for every seat.
    let party: Vec<_> = (1..=3u8).map(|id| table.height_for_char_id(id)).collect();
    for (i, a) in party.iter().enumerate() {
        for b in &party[i + 1..] {
            assert_ne!(a, b, "party heights must be distinct");
        }
    }
}
