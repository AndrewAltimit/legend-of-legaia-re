//! Disc-gated: noaru's ice-floor backdrop rides the camera focus.
//!
//! The ice cavern behind noaru's pillars is not ground and not a placed
//! object: it is the scene's ambient move-VM part on prescript stager record
//! 40 (model 6, the scene pack's slot 1 - a 96-prim bowl on page `(640, 0)`,
//! CLUT row 489), which loops `2F 24 1640 0000 10C0 1000 0000 1000`. Ext
//! sub-op `0x24` lerps the part's X / Z onto the camera focus, read out of
//! `_DAT_80089118 / 20` and un-negated (`0x801D4130..0x801D413C` in PROT
//! 0897), at `t = 0x1000` - so the bowl sits on the focus every frame. In
//! `noaru_field_card_walk` retail holds the stored focus at `(-5394, -12544)`
//! and the part at `(5394, 0, 12544)`.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing.

use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

#[test]
fn noaru_backdrop_part_sits_on_the_camera_focus() {
    let Some(ex) = extracted_dir() else { return };
    let mut host = SceneHost::open_extracted(&ex).expect("open");
    host.enter_field_scene("noaru", 0).expect("enter noaru");
    // The host publishes the stored focus before each world tick
    // (`frame_step::camera_before_world_tick`); stand in the capture's.
    for _ in 0..8 {
        host.world.set_camera_focus_stored(-5394, -12544);
        host.tick().expect("tick");
        let _ = host.step_headless_frame_tail();
    }
    let draws: Vec<_> = host
        .world
        .active_field_fx_part_draws()
        .into_iter()
        .filter(|d| d.model_index == 1)
        .collect();
    assert_eq!(draws.len(), 1, "one backdrop part draws the pack's slot 1");
    let p = draws[0].world_pos;
    assert_eq!(
        (p[0], p[2]),
        (5394.0, 12544.0),
        "the backdrop is seated on the camera focus, not on its operand"
    );

    // And it follows the focus as the camera moves (the loop's `09 0000`
    // yields between passes, so give it a few ticks).
    for _ in 0..4 {
        host.world.set_camera_focus_stored(-6000, -11000);
        host.tick().expect("tick");
        let _ = host.step_headless_frame_tail();
    }
    let p = host
        .world
        .active_field_fx_part_draws()
        .into_iter()
        .find(|d| d.model_index == 1)
        .expect("backdrop still drawn")
        .world_pos;
    assert_eq!((p[0], p[2]), (6000.0, 11000.0));
}

/// kor5's sky bowl (stager record 4, model 25) rides the same lerp -
/// `2F 24 1040 0000 1040 0C00 0000 0C00` - but its record opens with move-VM
/// op `0x43` (`+0x86 |= 0x2000`), so the part tick runs the field actor cull
/// `FUN_801D79E8` on it (`0x800228E4..0x80022914` in `FUN_80021DF4`). In
/// `kor5_post_43a_checkpoint` retail holds the part at `(4160, 0, 9920)`,
/// south of the region box `[20, 79, 43, 99]`, with flag bit `2` up: it
/// draws nothing.
#[test]
fn kor5_sky_part_is_culled_outside_the_region_box() {
    let Some(ex) = extracted_dir() else { return };
    let mut host = SceneHost::open_extracted(&ex).expect("open");
    host.enter_field_scene("kor5", 0).expect("enter kor5");
    let view = |attr_box| legaia_engine_field::camera::FieldCullView {
        focus_stored: [-4160, -11840],
        attr_box,
        window: [-15, -7, 13, 18],
    };
    let step = |host: &mut SceneHost, attr_box: [u8; 4]| {
        for _ in 0..8 {
            host.world.set_camera_focus_stored(-4160, -11840);
            host.world.npcs.cull_view = Some(view(attr_box));
            host.tick().expect("tick");
            let _ = host.step_headless_frame_tail();
        }
    };
    let sky = |host: &SceneHost| {
        host.world
            .ambient
            .fx
            .iter()
            .find(|p| p.model_sel == 25)
            .map(|p| (p.state.world_x, p.state.world_z, p.state.flags & 2))
    };
    step(&mut host, [20, 79, 43, 99]);
    assert_eq!(sky(&host), Some((4160, 9920, 2)), "seated, then culled");
    assert!(
        !host
            .world
            .active_field_fx_part_draws()
            .iter()
            .any(|d| d.model_index == 20),
        "a culled sky part draws nothing"
    );
    // Widen the box over the part and the cull lets it draw.
    step(&mut host, [20, 70, 43, 99]);
    assert_eq!(sky(&host), Some((4160, 9920, 0)));
    assert!(
        host.world
            .active_field_fx_part_draws()
            .iter()
            .any(|d| d.model_index == 20)
    );
}
