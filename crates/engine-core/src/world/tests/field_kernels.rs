//! Tests of `legaia-engine-field` kernels that need `World` or the mode
//! table, so they stay on the engine-core side of the crate split.

use crate::field_anim::{FieldClipPlayer, FieldPlayerAnim, synth_anm_bundle};
use crate::field_submode::{CARD_REQUEST_MODE, request_card_mode};
use crate::scene_transition_actor::{HeldTransition, SceneTransitionHold};

#[test]
fn world_tick_drives_walk_idle_switch_into_pose_frame() {
    use crate::world::{SceneMode, World};
    let mut w = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    w.install_field_player(0);
    let bundle = synth_anm_bundle(&[(2, 3), (2, 2)]);
    let mut idle = FieldClipPlayer::from_record(&bundle, 1).unwrap();
    let mut walk = FieldClipPlayer::from_record(&bundle, 0).unwrap();
    idle.set_step(16);
    walk.set_step(16);
    w.set_field_player_anim(Some(FieldPlayerAnim::new(idle, walk)));
    // Standing frame: idle clip pose lands in the player's pose_frame.
    w.set_pad(0);
    let _ = w.tick();
    let pose = w.actors[0].pose_frame.clone().expect("idle pose set");
    assert_eq!(pose.bone_outputs[0].0[0], 100, "idle record tag");
    assert!(!w.locomotion.player_anim.as_ref().unwrap().walking);
    // Held direction: locomotion flags the move, the walk clip plays.
    w.set_pad(crate::input::PadButton::Up.mask());
    let _ = w.tick();
    let pose = w.actors[0].pose_frame.clone().expect("walk pose set");
    assert_eq!(pose.bone_outputs[0].0[0], 0, "walk record restarts");
    assert!(w.locomotion.player_anim.as_ref().unwrap().walking);
    // Release: back to idle, restarted at frame 0.
    w.set_pad(0);
    let _ = w.tick();
    let pose = w.actors[0].pose_frame.clone().expect("idle pose set");
    assert_eq!(pose.bone_outputs[0].0[0], 100);
    assert!(!w.locomotion.player_anim.as_ref().unwrap().walking);
}

/// A parked transition engages the player the way the parked door record
/// does, so the pad cannot walk the player out of a held door.
#[test]
fn a_parked_transition_engages_the_player() {
    let mut w = crate::world::World::new();
    assert!(!w.script_context_engages_player());
    w.scene_transition_hold = Some(SceneTransitionHold::new(HeldTransition::Portal(3, 1)));
    assert!(w.script_context_engages_player());
}

#[test]
fn the_card_request_matches_the_bgm_barrier_abort_mode() {
    let r = request_card_mode();
    assert_eq!(r.game_mode, CARD_REQUEST_MODE);
    assert_eq!(r.flag, 1);
    // The leaf writes exactly the mode the field initialiser's BGM wait
    // barrier bails out on - the two are the same gate seen from each end.
    assert_eq!(
        r.game_mode,
        crate::mode_entry_init::FIELD_BGM_WAIT_ABORT_MODE
    );
    // And it is mode 22, the CARD init half.
    assert_eq!(
        crate::mode::GameMode::from_index(r.game_mode as usize),
        Some(crate::mode::GameMode::CardInit)
    );
}
