//! Disc-gated: a re-aimed field camera still crops to the latched walk
//! region, so Uru Mais's ring-tunnel sub-areas never draw from the village.
//!
//! `uru`'s `.MAP` grid holds four type-0 walk regions side by side: the
//! village (region `9`, tiles `[0, 0, 67, 82)`) and three coloured
//! ring-tunnel sub-areas (regions `3`, `4` and `7`), whose decoration cells
//! stamp pack meshes `45..=50`, `59`, `63` and `64`. Retail's decoration pass
//! `FUN_801F7088` tests each cell against the latched box `0x1F800384..87`
//! before it asks the visible tile window (`0x801F74DC..0x801F7540`), and the
//! pass prologue clamps the ground emitter's window to the same box, so no
//! vantage of the village draws a tunnel cell. A free view (drag, tilt,
//! wheel zoom, `F3`) used to draw the map whole and showed the tunnels round
//! the village's edges.

use std::path::PathBuf;

use legaia_engine_camera::camera_distance::CameraDistance;
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_engine_scene_host::resume::ResumeLanding;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_world_views::field_view_window::{
    CropFraming, crop_framing, field_view_cells, placed_actor_visible,
};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// The village's walk region, `[x_lo, z_lo, x_hi, z_hi]` in tiles.
const VILLAGE: [u8; 4] = [0, 0, 67, 82];
/// The ring-tunnel sub-areas' decoration meshes (scene pack indices).
const TUNNEL_PACKS: [u16; 9] = [45, 46, 47, 48, 49, 50, 59, 63, 64];
/// Village tiles the player is seated on: the arrival tile and three near
/// the region's east, south and west edges.
const SEATS: [(i16, i16); 4] = [(38, 6), (62, 30), (40, 78), (6, 40)];

#[test]
fn a_free_view_of_the_village_draws_no_tunnel_cell() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open prot index");
    let scene = Scene::load(&index, "uru").expect("load uru");
    let tunnels: Vec<(i32, i32)> = scene
        .field_terrain_tiles(&index)
        .expect("uru terrain")
        .expect("uru has a .MAP")
        .into_iter()
        .filter(|p| p.pack_index.is_some_and(|k| TUNNEL_PACKS.contains(&k)))
        .map(|p| (i32::from(p.col), i32::from(p.row)))
        .collect();
    assert!(
        tunnels.len() > 200,
        "uru's tunnel cells: {} (expected the three sub-areas)",
        tunnels.len()
    );
    assert!(
        tunnels.iter().all(|&(c, r)| c >= 67 || r >= 82),
        "a tunnel cell inside the village's region"
    );

    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open boot session");
    let save = legaia_save::SaveFile {
        party: legaia_save::Party::zeroed(1),
        ..Default::default()
    };
    let landing = session.resume_save(save, "uru", &FieldLiveOpts::default());
    assert_eq!(landing, ResumeLanding::SavedScene("uru".into()));

    for (tx, tz) in SEATS {
        assert!(
            session
                .host
                .world
                .debug_seat_player(tx * 128 + 64, tz * 128 + 64)
        );
        session.camera.zone.arm_arrival();
        for _ in 0..40 {
            session.tick().expect("tick");
        }
        let world = &session.host.world;
        // The default `Far` preset, a swung follow camera and the debug
        // vantage: every one crops to the village.
        session.camera.distance = CameraDistance::Far;
        let mut framings = vec![(crop_framing(world, &session.camera, true), "F3")];
        session.camera.manual_orbit = 0.5;
        framings.push((crop_framing(world, &session.camera, false), "orbit"));
        session.camera.manual_orbit = 0.0;
        framings.push((crop_framing(world, &session.camera, false), "far"));
        assert_eq!(framings[0].0, CropFraming::Free);
        assert_eq!(framings[1].0, CropFraming::Free);
        for (framing, label) in framings {
            let cells = field_view_cells(world, framing)
                .unwrap_or_else(|| panic!("seat ({tx}, {tz}) {label}: no crop"));
            assert_eq!(cells.region, VILLAGE, "seat ({tx}, {tz}) {label}");
            for &(c, r) in &tunnels {
                assert!(
                    !cells.decoration_visible(c, r, 15) && !cells.ground_visible(c, r),
                    "seat ({tx}, {tz}) {label}: tunnel cell ({c}, {r}) drawn"
                );
                assert!(
                    !placed_actor_visible(world, Some(&cells), c * 128 + 64, r * 128 + 64, 15),
                    "seat ({tx}, {tz}) {label}: actor at tunnel cell ({c}, {r}) drawn"
                );
            }
            // The player's own tile always draws.
            let (pc, pr) = (i32::from(tx), i32::from(tz));
            assert!(
                cells.ground_visible(pc, pr),
                "seat ({tx}, {tz}) {label}: ground"
            );
        }
    }
    eprintln!(
        "[ran] uru: {} tunnel cells, none drawn from the village",
        tunnels.len()
    );
}
