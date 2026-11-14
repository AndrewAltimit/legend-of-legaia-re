//! The Water Gate (`suimon`) through the native play window's own frame path
//! (`HeadlessPlayWindow`), one regression per defect a player met there.
//!
//! - Using the Water Gate key stages two shots from the controller's talk
//!   (`P1[4]`, `45 07 7B ..` at `+0x1E5` and `+0x214`): a three-quarter view
//!   of the sluice (`H` 744, eye depth 16903) and the gate head-on (`H`
//!   872, depth 15983). A retail capture of the beat
//!   (`captures/w42-I3/`, PCSX-Redux from `map01_field_door_hop`) holds
//!   those globals and frames the two towers edge to edge. The talk runs
//!   through the inline dialogue runner, not a timeline, so the port drew
//!   the shots through the follow arm with the `Far` distance preset's depth
//!   and pulled back past the edge of the scene, showing the void around the
//!   ditch.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use std::path::PathBuf;

use legaia_engine_shell::window::HeadlessPlayWindow;
use legaia_engine_system::input::PadButton;

use super::native_new_game_save_continue::{FACING_PROBES, mask};

fn disc() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)
}

fn seat_facing(w: &mut HeadlessPlayWindow, slot: u8) -> bool {
    let world = &mut w.session_mut().host.world;
    let Some(&(ax, az)) = world.npcs.positions.get(&slot) else {
        return false;
    };
    let Some(player) = world.player_actor_slot.map(usize::from) else {
        return false;
    };
    for q in [8i16, 7, 6, 5, 4] {
        for sector in [4usize, 0, 2, 6, 1, 3, 5, 7] {
            let (dx, dz) = FACING_PROBES[sector];
            let (sx, sz) = (ax - dx * q / 4, az + dz * q / 4);
            if world.field_walk_component_size(sx, sz) < 4 {
                continue;
            }
            let sy = world.sample_field_floor_height(sx as i32, sz as i32) as i16;
            let m = &mut world.actors[player].move_state;
            m.world_x = sx;
            m.world_y = sy;
            m.world_z = sz;
            m.render_26 = (((sector as i16) << 9) + 0x800) & 0x0FFF;
            if world.field_interact_probe_slot() == Some(slot) {
                return true;
            }
        }
    }
    false
}

/// The controller's two shots: `(H, eye depth)` as the record stages them.
const KEY_SHOTS: [(i32, i32); 2] = [(744, 16903), (872, 15983)];

#[test]
fn water_gate_key_shots_frame_at_retail_depth() {
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut w = HeadlessPlayWindow::open(&disc, "suimon").expect("open suimon");
    // The key's flag: the controller's talk asks to insert it.
    w.session_mut().host.world.system_flag_set(0x26F);
    for _ in 0..30 {
        w.display_frame(0, 1);
    }
    assert!(seat_facing(&mut w, 4), "no seat faces the controller");
    assert_eq!(
        w.session().camera.distance,
        legaia_engine_camera::camera_distance::CameraDistance::Far,
        "the play window's default distance preset is the one under test"
    );
    let mut seen = [false; 2];
    for f in 0..600u32 {
        let pad = if f % 16 < 2 {
            mask(PadButton::Cross)
        } else {
            0
        };
        w.display_frame(pad, 1);
        let s = w.session();
        let g = s.camera.globals.0;
        let Some(i) = KEY_SHOTS
            .iter()
            .position(|&(h, z)| i32::from(g[9] as i16) == h && i32::from(g[5] as i16) == z)
        else {
            continue;
        };
        let world = &s.host.world;
        assert!(
            world.scripted_shot_holds_camera(),
            "shot {i} staged under the talk, but the follow knobs still apply"
        );
        let view = legaia_engine_world_views::camera_view::field_follow_view(&s.camera, world)
            .expect("a follow view");
        let depth = view.tr_eye[2] * legaia_engine_field::camera::CUTSCENE_WORLD_SCALE;
        assert!(
            (depth - KEY_SHOTS[i].1 as f32).abs() < 1.0,
            "shot {i} frames at eye depth {depth}, not the staged {}",
            KEY_SHOTS[i].1
        );
        assert_eq!(view.h, KEY_SHOTS[i].0 as f32);
        seen[i] = true;
    }
    assert_eq!(seen, [true, true], "the talk never reached both shots");
}
