//! Disc-gated: the dance entry `FUN_801CEF54`'s venue, end to end.
//!
//! Every field of `dance::dance_scene_entry()` has one consumer on the entry
//! path, and this test reaches each through the real disc:
//!
//! * `stream_ids.0` / `scene_block_base` name the venue block (`other7`), which
//!   `DanceVenue::build` loads and resolves a draw list for;
//! * `stream_ids.1` bounds the block for a host without `CDNAME.TXT`
//!   (`venue_cdname_stub`) and names the SFX VAB (`venue_sfx_vab_index`);
//! * `face_stamps` / `face_stamp_mode` are the five blits applied to the venue
//!   VRAM;
//! * `dancer_spawn` / `camera_pair` / `camera_angles` / `gte_h` are the camera
//!   the dance frame resolves through;
//! * `view_window` and `scene_block_base` are staged over the walked-in scene
//!   by `sync_dance_venue` and restored on the way out, while the walked-in
//!   scene itself stays loaded;
//! * `cleared_dancer_slots` is the qualifier floor size the run is parsed for.
//!
//! The remaining four (`screen_width`, `ot_depth`, `work_buffer_bytes`,
//! `prim_buffer_bytes`) size libgpu display and heap buffers the renderer
//! replaces; they are asserted against the values the record's own unit tests
//! pin, not consumed.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` are missing (`LEGAIA_EXTRACTED_DIR`
//! points at an extraction outside the checkout).

use std::path::PathBuf;

use legaia_engine_core::camera::Camera;
use legaia_engine_core::camera_view::{FieldCameraFrame, resolve_field_camera};
use legaia_engine_core::dance::{DanceGame, DanceMode, dance_scene_entry};
use legaia_engine_core::dance_venue::{
    DanceVenue, DanceVenueEdge, sync_dance_venue, venue_camera, venue_cdname_stub,
    venue_sfx_vab_index,
};
use legaia_engine_core::scene::{ProtIndex, SceneHost};
use legaia_engine_core::world::SceneMode;

fn extracted_dir() -> Option<PathBuf> {
    let over = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from);
    over.into_iter()
        .chain(
            ["extracted", "../extracted", "../../extracted"]
                .into_iter()
                .map(PathBuf::from),
        )
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

fn gate() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing");
        None
    })
}

fn dance_overlay(index: &ProtIndex) -> Vec<u8> {
    let rec = legaia_asset::static_overlay::overlay_map()
        .by_prot_index(legaia_asset::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32)
        .expect("0980 in the static-overlay map");
    let bytes = index.entry_bytes_extended(rec.prot_index).expect("0980");
    legaia_asset::static_overlay::as_loaded(&bytes, rec).expect("0980 loaded form")
}

#[test]
fn the_entry_record_loads_and_stamps_the_venue() {
    let Some(dir) = gate() else { return };
    let index = ProtIndex::open_extracted(&dir).expect("prot index");
    let overlay = dance_overlay(&index);
    let e = dance_scene_entry();

    let venue = DanceVenue::build(&index, Some(&overlay)).expect("venue builds");
    assert_eq!(venue.scene.name, legaia_asset::dance_cast::DANCE_SCENE_NAME);
    assert_eq!(
        index.block_range(&venue.scene.name).map(|r| r.0),
        Some(u32::from(e.scene_block_base)),
        "the block the record names is the block the scene loader frames"
    );
    assert_eq!(e.stream_ids.0, u32::from(e.scene_block_base));
    assert!(!venue.draws.is_empty(), "the hall resolves placed geometry");
    assert!(venue.hud_staged, "the HUD page reaches the venue VRAM");
    assert_eq!(venue.faces_stamped, 5, "all five entry blits land");
    assert_eq!(venue.camera, venue_camera(&e));

    // Each stamp's eye cell is a copy of its pose's source cell: re-read the
    // last stamp per rig (a later blit on the same rig wins).
    for s in venue.face_stamps.iter().rev() {
        let rig = &legaia_asset::dance_art::FACE_RIGS[s.rig];
        let frames = legaia_asset::dance_art::parse_face_frames(&overlay, rig).unwrap();
        let f = frames[usize::from(s.pose).min(frames.len() - 1)];
        let (sx, sy) = (
            rig.base.0 + u16::from(f[0] >> 2),
            rig.base.1 + u16::from(f[1]),
        );
        let src_nonzero = (0..rig.eyes.h).any(|r| {
            (0..rig.eyes.w_hw).any(|c| {
                venue
                    .resources
                    .vram
                    .pixel((sx + c) as usize, (sy + r) as usize)
                    != 0
            })
        });
        assert!(src_nonzero, "rig {} source cell is resident", s.rig);
    }

    // The browser pages' path: PROT bytes plus the record's two-define stub.
    let prot = std::fs::read(dir.join("PROT.DAT")).expect("PROT.DAT");
    let stub = ProtIndex::from_bytes(prot, Some(&venue_cdname_stub(&e))).expect("stub index");
    let stub_venue = DanceVenue::build(&stub, Some(&overlay)).expect("stub venue");
    assert_eq!(stub_venue.scene.name, venue.scene.name);
    assert_eq!(stub_venue.draws.len(), venue.draws.len());
    assert!(
        stub.entry_bytes(venue_sfx_vab_index(&e)).is_ok(),
        "the audio bank the record names is readable"
    );
}

#[test]
fn entering_the_dance_stages_the_venue_and_leaving_restores_the_scene() {
    let Some(dir) = gate() else { return };
    let mut host = SceneHost::open_extracted(&dir).expect("host");
    host.enter_field_scene("town01", 0).expect("town01");
    let mut camera = Camera::new();
    camera.reset_globals_for_scene_entry();
    let walked_in = host.scene.as_ref().map(|s| s.name.clone());
    let block_base = host.world.battle.map_id;
    let window = camera.zone.view_window;
    let e = dance_scene_entry();

    let overlay = dance_overlay(&host.index);
    let game = DanceGame::from_overlay_for_mode(&overlay, DanceMode::Qualifier, false)
        .expect("dance chart parses");
    assert_eq!(e.cleared_dancer_slots, DanceMode::Qualifier.cast_size());
    host.world.enter_dance(game);
    assert_eq!(
        sync_dance_venue(&mut host.world, &mut camera),
        Some(DanceVenueEdge::Staged)
    );
    assert_eq!(host.world.battle.map_id, u32::from(e.scene_block_base));
    let (x0, z0, x1, z1) = e.view_window;
    assert_eq!(camera.zone.view_window, [x0, z0, x1, z1]);
    match resolve_field_camera(&host.world, &camera, None, [0.0, 0.0]) {
        FieldCameraFrame::Venue(v) => assert_eq!(v, venue_camera(&e)),
        other => panic!("the dance frames through the venue camera, got {other:?}"),
    }
    // The walked-in scene is never unloaded under the dance.
    assert_eq!(host.scene.as_ref().map(|s| s.name.clone()), walked_in);

    host.world.exit_dance();
    assert_eq!(
        sync_dance_venue(&mut host.world, &mut camera),
        Some(DanceVenueEdge::Restored)
    );
    assert_eq!(host.world.battle.map_id, block_base);
    assert_eq!(camera.zone.view_window, window);
    assert_eq!(host.scene.as_ref().map(|s| s.name.clone()), walked_in);
    assert_ne!(host.world.mode, SceneMode::Dance);
    assert!(!matches!(
        resolve_field_camera(&host.world, &camera, None, [0.0, 0.0]),
        FieldCameraFrame::Venue(_)
    ));
}
