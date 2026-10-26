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
