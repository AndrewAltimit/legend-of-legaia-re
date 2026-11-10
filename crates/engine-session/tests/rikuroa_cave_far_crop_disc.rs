//! Disc-gated: the default `Far` distance preset keeps retail's visible-tile
//! crop, widened, in Mt. Rikuroa's cave.
//!
//! The cave (`rikuroa`, tiles `56..70` by `100..125`) is the stretch Noa
//! walks alone. Lifting the crop whole under the pulled-back preset drew two
//! things retail's window leaves out at the first rockfall's tile:
//!
//! - the stalactite of terrain cell `(65, 104)`, fifteen rows nearer the
//!   lens than the player. Retail's window ends six rows out; the pulled-back
//!   eye sat a few hundred units behind the mesh, which then covered the
//!   player through the rockfall record `P2[44]`;
//! - the mountain's sky shell, the placed object anchored at cell
//!   `(68, 77)`, which showed as a grey sheet through the cave's open floor.
//!
//! A retail film of the same walk (PCSX-Redux, `autorun_seat_walk_film.lua`
//! from `rikuroa_field_card_walk`, seated on the cave mouth's teleport edge)
//! shows neither.

use std::path::PathBuf;

use legaia_engine_camera::camera_distance::CameraDistance;
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

/// The first rockfall's tile, world units: where `P2[44]` stops the player.
const SEAT: (i16, i16) = (8192, 15232);
/// The hanging stalactite's terrain cell.
const STALACTITE_CELL: (i32, i32) = (65, 104);
/// The sky shell's placed origin, world units (cell `(68, 77)`).
const SKY_SHELL: (i32, i32) = (8768, 9920);

#[test]
fn far_preset_crops_the_cave_by_the_widened_window() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open boot session");
    let opts = FieldLiveOpts::default();
    let save = legaia_save::SaveFile {
        party: legaia_save::Party::zeroed(1),
        ..Default::default()
    };
    let landing = session.resume_save(save, "rikuroa", &opts);
    // The mountain's first-arrival beat (`P2[43]`, `0x2FB`) and both
    // rockfalls (`P2[44]` / `P2[45]`, `0x281` / `0x282`) have played, so no
    // record takes the camera at the seat.
    for flag in [0x2FB, 0x281, 0x282] {
        session.host.world.system_flag_set(flag);
    }
    assert_eq!(landing, ResumeLanding::SavedScene("rikuroa".into()));
    assert!(session.host.world.debug_seat_player(SEAT.0, SEAT.1));
    session.camera.zone.arm_arrival();
    for _ in 0..30 {
        session.tick().expect("tick");
    }
    let world = &session.host.world;
    let view = world.npcs.cull_view.expect("a published cull view");
    eprintln!("cull view at the rockfall tile: {view:?}");
    let (tx, tz) = (i32::from(SEAT.0) >> 7, i32::from(SEAT.1) >> 7);

    session.camera.distance = CameraDistance::Retail;
    let retail = field_view_cells(world, crop_framing(world, &session.camera, false))
        .expect("the crop holds at retail framing");
    session.camera.distance = CameraDistance::Far;
    let framing = crop_framing(world, &session.camera, false);
    assert_eq!(framing, CropFraming::Widened(CameraDistance::Far.scale()));
    let far = field_view_cells(world, framing).expect("the Far preset keeps a crop");

    // The stalactite is past the near edge of both windows.
    let (sc, sr) = STALACTITE_CELL;
    assert!(!retail.decoration_visible(sc, sr, 0), "retail: {retail:?}");
    assert!(!far.decoration_visible(sc, sr, 0), "far: {far:?}");
    // The sky shell is not drawn from inside the cave.
    for cells in [&retail, &far] {
        assert!(
            !placed_actor_visible(world, Some(cells), SKY_SHELL.0, SKY_SHELL.1, 0),
            "sky shell drawn: {cells:?}"
        );
    }
    // The widened walk still covers everything retail's does, and the path
    // around the player in particular.
    for col in 0..128 {
        for row in 0..128 {
            if retail.decoration_visible(col, row, 0) {
                assert!(far.decoration_visible(col, row, 0), "cell ({col}, {row})");
            }
            if retail.ground_visible(col, row) {
                assert!(far.ground_visible(col, row), "ground ({col}, {row})");
            }
        }
    }
    assert!(far.ground_visible(tx, tz) && far.decoration_visible(tx, tz, 0));
    // A swung camera frees the view, as before.
    session.camera.manual_orbit = 0.5;
    assert_eq!(
        crop_framing(world, &session.camera, false),
        CropFraming::Free
    );
    eprintln!("[ran] rikuroa cave: retail {retail:?} far {far:?}");
}
