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

/// `B1 F8 18` / `B2 F8 18`: op `0x31` / `0x32` with the extended target
/// `0xF8`, which retail resolves to the player object - so bit 24 is the
/// player's party-bank bit, not a bit of whichever context ran the op.
#[test]
fn player_targeted_cflag_bit_24_writes_the_party_bank_bit() {
    let mut w = World::new();
    assert!(w.locomotion.player_party_bank);
    let mut ctx = FieldCtx::default();
    {
        let mut host = FieldHostImpl { world: &mut w };
        match vm::field::step(&mut host, &mut ctx, &[0xB2, 0xF8, 0x18], 0) {
            FieldStepResult::Advance { next_pc } => assert_eq!(next_pc, 3),
            other => panic!("B2 F8 18 should advance 3 bytes, got {other:?}"),
        }
    }
    assert!(!w.locomotion.player_party_bank, "CFLAG_CLR dropped it");
    assert_eq!(ctx.flags, 0, "the caller's context is untouched");
    {
        let mut host = FieldHostImpl { world: &mut w };
        let _ = vm::field::step(&mut host, &mut ctx, &[0xB1, 0xF8, 0x18], 0);
    }
    assert!(w.locomotion.player_party_bank, "CFLAG_SET raised it");
    assert_eq!(ctx.flags, 0);
    // A bit the port has no player word for stays on the caller's context,
    // and a non-player target never reaches the hook.
    {
        let mut host = FieldHostImpl { world: &mut w };
        let _ = vm::field::step(&mut host, &mut ctx, &[0xB1, 0xF8, 0x15], 0);
        let _ = vm::field::step(&mut host, &mut ctx, &[0xB2, 0x05, 0x18], 0);
    }
    assert_eq!(ctx.flags, 1 << 0x15);
    assert!(w.locomotion.player_party_bank);
}

/// The dropped bit is what the next clip pick reads: with it down, a script
/// clip on the player binds a scene-bank record (`move_id - 1`).
#[test]
fn a_script_dropped_party_bank_bit_sends_the_next_pick_to_the_scene_bank() {
    let mut w = world_with_anim();
    let mut ctx = FieldCtx::default();
    {
        let mut host = FieldHostImpl { world: &mut w };
        let _ = vm::field::step(&mut host, &mut ctx, &[0xB2, 0xF8, 0x18], 0);
    }
    w.field_player_script_clip(0x30);
    assert_eq!(scene_record(&w), Some(0x2F));
}

/// The system channel's per-tick store of the idle base (`0x80039D94`) runs
/// after the settle and only while the player is not movement-locked. So the
/// tick a conversation opens on still re-idles the base, and every locked tick
/// after it binds idle - the player who opened the talk running does not run
/// in place through it (retail capture: clip id `3` on the talk's first tick,
/// `2` after, no further base store while the box is open).
#[test]
fn the_system_channel_reset_idles_a_player_who_opened_a_talk_running() {
    let mut w = world_with_anim();
    // The talk's first tick: the pad step stored run, the settle bound it.
    w.locomotion.clip_base = vm::field_player_clip::BASE_RUN;
    w.field_settle_clip_tail();
    assert_eq!(w.locomotion.player_clip, 3);
    w.tick_field_system_channel_clip_reset();
    assert_eq!(w.locomotion.clip_base, vm::field_player_clip::BASE_IDLE);
    // The box is up from here on: the pad step is skipped and the lock
    // closes the reset, so the settle binds the base the first tick left.
    w.dialog.current = Some(DialogRequest {
        text_id: 0,
        inline: Vec::new(),
        world_x: 0,
        world_z: 0,
        depth_id: 0,
    });
    assert!(w.field_player_movement_locked());
    w.field_settle_clip_tail();
    assert_eq!(w.locomotion.player_clip, 2, "idle through the talk");
    // A base something else writes while the box is up stays put: retail's
    // reset does not run under the lock (the poked-walk capture).
    w.locomotion.clip_base = vm::field_player_clip::BASE_WALK;
    w.tick_field_system_channel_clip_reset();
    assert_eq!(w.locomotion.clip_base, vm::field_player_clip::BASE_WALK);
}

/// The movement-lock bit, a ledge hop and the tile board each close the
/// reset; a kind-0 warp does not (it clears the lock when it arms).
#[test]
fn the_reset_runs_through_a_warp_but_not_under_a_lock() {
    let mut w = world_with_anim();
    let s = w.player_actor_slot.unwrap() as usize;
    w.actors[s].move_state.flags |= 0x0008_0000;
    assert!(w.field_player_movement_locked());
    w.actors[s].move_state.flags &= !0x0008_0000;
    assert!(!w.field_player_movement_locked());
    w.arm_field_warp((10, 10));
    assert!(!w.field_player_movement_locked(), "a warp is not a lock");
    w.locomotion.clip_base = vm::field_player_clip::BASE_WALK;
    w.tick_field_system_channel_clip_reset();
    assert_eq!(w.locomotion.clip_base, vm::field_player_clip::BASE_IDLE);
}

/// `CC F8 50 lo hi`: op `4C 50` aimed at the player writes the player's
/// party-bank bit (`>= 0xF0` raises it) and its live model, not the caller's
/// context - `jagaroom`'s `CC F8 50 26 00` and `urudre1`'s `CC F8 50 F0 00`.
#[test]
fn a_player_targeted_model_set_lands_on_the_player() {
    let mut w = World::new();
    let mut ctx = FieldCtx::default();
    {
        let mut host = FieldHostImpl { world: &mut w };
        match vm::field::step(&mut host, &mut ctx, &[0xCC, 0xF8, 0x50, 0x26, 0x00], 0) {
            FieldStepResult::Advance { next_pc } => assert_eq!(next_pc, 5),
            other => panic!("CC F8 50 should advance 5 bytes, got {other:?}"),
        }
    }
    assert_eq!(w.locomotion.player_live_model, Some(0x26));
    assert!(
        !w.locomotion.player_party_bank,
        "a scene-bank id drops the bit"
    );
    assert_eq!((ctx.model_id, ctx.flags), (0, 0), "the caller is untouched");
    {
        let mut host = FieldHostImpl { world: &mut w };
        let _ = vm::field::step(&mut host, &mut ctx, &[0xCC, 0xF8, 0x50, 0xF0, 0x00], 0);
    }
    assert_eq!(w.locomotion.player_live_model, Some(0xF0));
    assert!(w.locomotion.player_party_bank, "a player-bank id raises it");
    // Scene entry restores the lead's own mesh.
    w.reset_field_warp_and_clip();
    assert_eq!(w.locomotion.player_live_model, None);
}
