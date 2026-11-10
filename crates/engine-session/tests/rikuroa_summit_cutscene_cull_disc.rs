//! Disc-gated: the visible-tile crop and the field actor cull hold through
//! Mt. Rikuroa's summit cutscene (`rikuroa` `P2[48]`, the Genesis Tree,
//! Zeto's portal and Caruban), as retail's do.
//!
//! The cull `FUN_801D79E8` reads the focus pair, the walk-region box and the
//! visible tile window every frame whoever owns the camera, so a scripted
//! shot culls like the follow camera. The summit's region box is
//! `[31, 49, 87, 88]`; Zeto's close-up frames the focus at `(10864, 8832)`,
//! and the only placed object the cull keeps there is the mountain's sky
//! shell (env pack `37`). A frame drawn with the cull off shows the moss
//! spires on the slopes below the summit as clumps of foliage floating in
//! the sky behind Zeto, which a retail film of the same shot does not.
//!
//! The record starts the way play starts it: the player stands on its
//! walk-on tile `(57, 76)` once the entry script has settled.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;
use legaia_engine_world_views::camera_view::cutscene_owns_camera;
use legaia_engine_world_views::field_view_window::{
    crop_framing, field_view_cells, placed_actor_visible,
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

#[test]
fn the_summit_cutscene_keeps_the_crop_and_the_actor_cull() {
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
    let mut s = NativeSession::open(&extracted, &cfg).expect("open boot session");
    let save = legaia_save::SaveFile {
        party: legaia_save::Party::zeroed(1),
        ..Default::default()
    };
    s.resume_save(save, "rikuroa", &FieldLiveOpts::default());
    // The arrival beat, both rockfalls and Terra's rest have played.
    for f in [0x2FB, 0x281, 0x282, 0x283, 0x284, 0x285] {
        s.host.world.system_flag_set(f);
    }
    for _ in 0..120 {
        s.tick().expect("tick");
    }
    assert!(
        s.host.world.debug_seat_player(7360, 9792),
        "the walk-on tile"
    );
    s.host.world.refresh_region_attributes_at_player();
    s.camera.zone.arm_arrival();
    for _ in 0..4 {
        s.tick().expect("tick");
    }
    assert!(
        cutscene_owns_camera(&s.host.world),
        "P2[48] took the camera"
    );

    let placements = {
        let scene = s.host.scene.as_ref().expect("rikuroa loaded");
        scene
            .field_object_placements(&s.host.index)
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    let mut zeto_shot = false;
    for t in 0..2400 {
        s.host.world.set_pad(if t % 8 < 2 {
            PadButton::Cross.mask()
        } else {
            0
        });
        s.tick().expect("tick");
        if t % 30 != 0 || !cutscene_owns_camera(&s.host.world) {
            continue;
        }
        let w = &s.host.world;
        let view = w.npcs.cull_view.expect("the cull view stays published");
        assert_eq!(view.attr_box, [31, 49, 87, 88], "the summit's box at t{t}");
        let cells = field_view_cells(w, crop_framing(w, &s.camera, false))
            .unwrap_or_else(|| panic!("the crop holds through the shot at t{t}"));
        if view.focus_stored == [-10864, -8832] {
            zeto_shot = true;
            let kept: Vec<_> = placements
                .iter()
                .filter(|p| {
                    placed_actor_visible(w, Some(&cells), p.world_x, p.world_z, p.cull_radius)
                })
                .map(|p| p.pack_index)
                .collect();
            assert_eq!(kept, vec![Some(37)], "only the sky shell in Zeto's shot");
        }
    }
    assert!(zeto_shot, "the run reached Zeto's close-up");
}
