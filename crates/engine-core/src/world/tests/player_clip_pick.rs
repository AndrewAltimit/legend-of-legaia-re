//! The player clip pick's retail inputs beyond the pad: the op-`4C CE`
//! override `_DAT_8007B6AC`, the script arms that aim a clip at the player
//! (op `0x22`, the player arm of `4C 51`), and the scene-bank binds they
//! produce; plus the kind-0 warp's text-balloon tear-down.

use super::*;
use crate::field_anim::{FieldClipPlayer, FieldPlayerAnim, synth_anm_bundle};

/// A field world with a player and a two-bone clip pair installed.
fn world_with_anim() -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.install_field_player(0);
    let party = synth_anm_bundle(&[(2, 3), (2, 2)]);
    let anim = FieldPlayerAnim::new(
        FieldClipPlayer::from_record(&party, 1).unwrap(),
        FieldClipPlayer::from_record(&party, 0).unwrap(),
    );
    w.set_field_player_anim(Some(anim));
    w
}

fn scene_record(w: &World) -> Option<u16> {
    w.locomotion.player_anim.as_ref().unwrap().scene_record()
}

#[test]
fn op_4c_ce_stores_the_clip_override() {
    let mut w = World::new();
    let mut ctx = FieldCtx::default();
    let mut host = FieldHostImpl { world: &mut w };
    match vm::field::step(&mut host, &mut ctx, &[0x4C, 0xCE, 0x24], 0) {
        FieldStepResult::Advance { next_pc } => assert_eq!(next_pc, 3),
        other => panic!("4C CE should advance 3 bytes, got {other:?}"),
    }
    assert_eq!(w.locomotion.clip_override, 0x24);
    // Scene entry drops it (SCUS 0x8003B6F0).
    w.reset_field_warp_and_clip();
    assert_eq!(w.locomotion.clip_override, 0);
}

#[test]
fn the_override_points_the_settle_pick_at_the_scene_bank() {
    let mut w = world_with_anim();
    w.locomotion.clip_base = vm::field_player_clip::BASE_WALK;
    w.field_settle_clip_tail();
    assert_eq!(scene_record(&w), None, "no override: the party bank");
    // `jagaroom`'s `4C CE 24`: walk base 1 binds scene clip 0x24 = record
    // 0x23, and the party-bank bit survives the bind.
    w.locomotion.clip_override = 0x24;
    w.field_settle_clip_tail();
    assert_eq!(w.locomotion.player_clip, 0x24);
    assert_eq!(scene_record(&w), Some(0x23));
    assert!(w.locomotion.player_party_bank);
    // Run (base 3) rides the same offset.
    w.locomotion.clip_base = vm::field_player_clip::BASE_RUN;
    w.field_settle_clip_tail();
    assert_eq!(scene_record(&w), Some(0x25));
}

#[test]
fn the_scene_sentinel_binds_scene_record_leader() {
    let mut w = world_with_anim();
    w.locomotion.clip_base = vm::field_player_clip::BASE_SCENE_SENTINEL;
    w.field_settle_clip_tail();
    assert_eq!(scene_record(&w), Some(0), "leader 0 -> clip 1 -> record 0");
    assert!(!w.locomotion.player_party_bank);
}

#[test]
fn a_script_clip_aimed_at_the_player_writes_the_base_and_binds() {
    let mut w = world_with_anim();
    w.locomotion.clip_override = 0x24;
    // `jagaroom` P1[8]: `4C CE 24` then `A2 F8 01`.
    w.field_player_script_clip(1);
    assert_eq!(w.locomotion.clip_base, 1);
    assert_eq!(scene_record(&w), Some(0x23));
    // Without the override a flagged player strides into the party bank.
    w.locomotion.clip_override = 0;
    w.field_player_script_clip(2);
    assert_eq!(scene_record(&w), None);
    assert_eq!(
        w.locomotion.player_anim.as_ref().unwrap().retail_slot(),
        Some(1)
    );
    // An unflagged player binds scene record `move_id - 1`.
    w.locomotion.player_party_bank = false;
    w.field_player_script_clip(0x30);
    assert_eq!(scene_record(&w), Some(0x2F));
}

#[test]
fn a_running_warp_tears_down_the_text_balloon() {
    let mut w = world_with_anim();
    w.cutscene.text_balloon = Some(crate::text_balloon::TextBalloon::spawn(b"x"));
    w.tick_field_warp();
    assert!(
        w.cutscene.text_balloon.is_some(),
        "no warp: the balloon stays"
    );
    w.arm_field_warp((10, 10));
    w.tick_field_warp();
    assert!(
        w.cutscene.text_balloon.is_none(),
        "the timer-running half tags 0x801DA7F0 for tear-down"
    );
}
