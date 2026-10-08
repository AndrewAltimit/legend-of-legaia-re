//! The volumetric ground fog stays out of the house rooms a town lays out
//! beside its streets.
//!
//! A Rim Elm house door is an intra-scene warp: the rooms are walk areas of
//! the same `town0b` map, at their own tiles. The bank follows the retail
//! fog pool's spawn gate - the MAN section-4 region table, first containing
//! box decides (`legaia_engine_core::fog_volume::region_weight`) - and
//! `town0b`'s one region covers the streets and none of the rooms. Seat the
//! player in a room and every sheet-mesh vertex around them carries no bank;
//! back on the street the bank is there.
//!
//! Where the table keeps one region over the whole map (`dolk`), the rooms
//! are told apart by the door warp that reaches them
//! (`legaia_engine_core::fog_volume::InteriorTracker`).
//!
//! Disc-gated: skips (and passes) without `LEGAIA_DISC_BIN`.

use legaia_engine_core::fog_volume::region_weight;
use legaia_engine_core::scene::SceneHost;

fn open_host() -> Option<SceneHost> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let path = std::path::PathBuf::from(disc);
    path.exists()
        .then(|| SceneHost::open_disc(&path).expect("open disc"))
}

/// Mean floor weight of the sheet-mesh vertices within `radius` world units
/// of the player, and how many there were.
fn bank_weight_around_player(host: &SceneHost, radius: f32) -> (f32, usize) {
    let w = &host.world;
    let frame = w.fog_volume_frame().expect("the bank is raised");
    let p = w.fog_player_world_pos();
    let (px, pz) = (p[0] as f32, p[2] as f32);
    let (mut sum, mut n) = (0.0, 0);
    for v in frame.mesh_positions().chunks(4) {
        let (dx, dz) = (v[0] - px, v[2] - pz);
        if dx * dx + dz * dz <= radius * radius {
            sum += v[3];
            n += 1;
        }
    }
    (sum / n.max(1) as f32, n)
}

#[test]
fn town0b_bank_stays_out_of_the_house_rooms() {
    let Some(mut host) = open_host() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    host.world.toggles.volumetric_fog = true;
    host.world.seed_free_roam_story_baseline("town0b");
    host.enter_field_scene("town0b", 0).expect("enter town0b");
    for _ in 0..200 {
        let _ = host.world.tick();
    }
    assert!(
        host.world.fog_volume_style().is_some(),
        "town0b raises a bank"
    );
    let regions = host.world.fog.regions.clone();
    assert!(!regions.is_empty(), "town0b carries a fog-region table");
    eprintln!("[ran] town0b regions {regions:?}");
    // The streets are fog; the house-room walk areas east of them are not.
    assert_eq!(region_weight(&regions, 20, 20), 1.0, "street tile");
    for (tx, tz) in [(97, 13), (117, 8), (114, 37), (97, 57), (117, 57), (97, 75)] {
        assert_eq!(
            region_weight(&regions, tx, tz),
            0.0,
            "room tile ({tx}, {tz})"
        );
    }

    // Seated in a room as if the scene had been entered there (a card
    // load), so the door rule has nothing to say and the table alone
    // decides: the bank around the player is gone.
    assert!(host.debug_seat_standing(97 * 128 + 64, 13 * 128 + 64));
    host.world.fog_volume.interiors.reset();
    for _ in 0..4 {
        let _ = host.world.tick();
    }
    assert!(!host.world.fog_volume.interiors.indoors);
    let (room, n) = bank_weight_around_player(&host, 3.0 * 128.0);
    eprintln!("[ran] room weight {room} over {n} vertices");
    assert!(n > 0);
    assert_eq!(room, 0.0, "no bank inside the house room");

    // Back on the street it is there again.
    assert!(host.debug_seat_standing(27 * 128 + 64, 30 * 128 + 64));
    host.world.fog_volume.interiors.reset();
    for _ in 0..4 {
        let _ = host.world.tick();
    }
    let (street, n) = bank_weight_around_player(&host, 3.0 * 128.0);
    eprintln!("[ran] street weight {street} over {n} vertices");
    assert!(street > 0.0, "the street keeps its bank");

    // Non-vacuous: without the table the same room floor would carry it.
    host.world.fog.regions.clear();
    assert!(host.debug_seat_standing(97 * 128 + 64, 13 * 128 + 64));
    host.world.fog_volume.interiors.reset();
    for _ in 0..4 {
        let _ = host.world.tick();
    }
    let (unmasked, _) = bank_weight_around_player(&host, 3.0 * 128.0);
    eprintln!("[ran] room weight without the table {unmasked}");
    assert!(unmasked > 0.0, "the room is floor the bank would lie on");
}

/// Drake Castle under the Mist keeps one fog region over its whole map, so
/// the table alone would leave the bank in every hall and room. The rooms
/// are door-reached walk areas: warped into one, the bank goes; back out on
/// the entrance plaza it returns.
#[test]
fn dolk_bank_leaves_with_the_door_into_a_room() {
    let Some(mut host) = open_host() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    host.world.toggles.volumetric_fog = true;
    host.world.seed_free_roam_story_baseline("dolk");
    host.enter_field_scene("dolk", 0).expect("enter dolk");
    // The entrance plaza, where the scene is entered from the overworld.
    assert!(host.debug_seat_standing(9056, 8992));
    for _ in 0..200 {
        let _ = host.world.tick();
    }
    assert!(
        host.world.fog_volume_style().is_some(),
        "dolk raises a bank"
    );
    assert!(!host.world.fog_volume.interiors.indoors);
    assert!(
        host.world.fog_volume_frame().is_some(),
        "the plaza carries the bank"
    );
    let regions = &host.world.fog.regions;
    assert!(
        region_weight(regions, 14, 7) > 0.0,
        "the room is inside an enabled region - the table alone keeps it"
    );

    // Door into the room west of the hall: a warp to a room-sized area.
    assert!(host.debug_seat_standing(14 * 128 + 64, 7 * 128 + 64));
    let _ = host.world.tick();
    assert!(
        host.world.fog_volume.interiors.indoors,
        "the room is indoors"
    );
    assert_eq!(host.world.fog_volume_style(), None);
    assert!(
        host.world.fog_volume_frame().is_none(),
        "the door cuts the bank off"
    );
    for _ in 0..60 {
        let _ = host.world.tick();
    }
    assert!(host.world.fog_volume_frame().is_none(), "and it stays off");

    // Back out onto the plaza: the bank eases back in.
    assert!(host.debug_seat_standing(9056, 8992));
    for _ in 0..30 {
        let _ = host.world.tick();
    }
    assert!(!host.world.fog_volume.interiors.indoors);
    assert!(host.world.fog_volume_frame().is_some(), "the plaza's bank");
    eprintln!("[ran] dolk plaza -> room -> plaza");
}
