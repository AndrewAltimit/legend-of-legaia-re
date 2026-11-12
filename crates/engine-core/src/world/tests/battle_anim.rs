use super::*;
use legaia_engine_mode::mode::SceneMode;

// --- battle pose -> action-clip switching -----------------------------------

/// Synthetic one-part action clip: `frames` keyframes translating from `tx`.
fn pose_test_clip(action_id: u8, frames: usize, tx: i16) -> MonsterAnimation {
    use legaia_actor_formats::monster_archive::PartPose;
    MonsterAnimation {
        variant_track: Default::default(),
        action_id,
        rate: 2,
        attach_key: 0,
        solo_flag: 0,
        impact_class: 0,
        effect_script: Vec::new(),
        part_count: 1,
        frame_count: frames,
        frames: (0..frames)
            .map(|f| {
                vec![PartPose {
                    tx: tx + f as i16,
                    ty: 0,
                    tz: 0,
                    rx: 0,
                    ry: 0,
                    rz: 0,
                }]
            })
            .collect(),
    }
}

fn pose_test_world() -> World {
    let mut world = World::new();
    world.actors[0].active = true;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 22];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[8] = Some(pose_test_clip(8, 2, 100));
    clips[9] = Some(pose_test_clip(9, 2, 200));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world
}

#[test]
fn ready_and_recover_poses_play_the_idle_loop_not_the_downed_chain() {
    // Entries 7 / 8 are the downed party member's chain (the clip-tag
    // ladder's knockdown -> 7 -> 8); the SM's pose 7 / 8 calls are camera
    // programs, so they leave the actor on its idle loop.
    for pose in [
        legaia_engine_battle_vm::battle_action::Pose::Ready,
        legaia_engine_battle_vm::battle_action::Pose::Recover,
    ] {
        let mut world = pose_test_world();
        world.apply_battle_pose(0, pose as u8);
        assert_eq!(
            world.actors[0].battle_pose,
            Some(legaia_engine_battle_vm::battle_action::Pose::Idle as u8)
        );
        world.actors[0].battle_animation.as_mut().unwrap().step = 1024;
        world.tick_battle_animations();
        assert!(
            !world.actors[0]
                .battle_animation
                .as_ref()
                .unwrap()
                .finished(),
            "idle loops"
        );
    }
}

#[test]
fn battle_pose_defeat_holds_final_frame() {
    let mut world = pose_test_world();
    world.apply_battle_pose(
        0,
        legaia_engine_battle_vm::battle_action::Pose::Defeat as u8,
    );
    world.actors[0].battle_animation.as_mut().unwrap().step = 1024;
    for _ in 0..5 {
        world.tick_battle_animations();
    }
    // Defeat never falls back to idle: the downed pose holds.
    assert_eq!(
        world.actors[0].battle_pose,
        Some(legaia_engine_battle_vm::battle_action::Pose::Defeat as u8)
    );
    assert!(
        world.actors[0]
            .battle_animation
            .as_ref()
            .unwrap()
            .finished()
    );
}

#[test]
fn battle_pose_missing_slot_falls_back_to_idle_loop() {
    let mut world = pose_test_world();
    // Slot 9 (defeat) is empty in this set: the request binds the idle loop
    // instead and records the pose so the SM isn't retried.
    let mut clips = (**world.actors[0].battle_action_clips.as_ref().unwrap()).clone();
    clips[9] = None;
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.apply_battle_pose(
        0,
        legaia_engine_battle_vm::battle_action::Pose::Defeat as u8,
    );
    assert_eq!(
        world.actors[0].battle_pose,
        Some(legaia_engine_battle_vm::battle_action::Pose::Defeat as u8)
    );
    world.tick_battle_animations();
    assert!(
        !world.actors[0]
            .battle_animation
            .as_ref()
            .unwrap()
            .finished()
    );
}

#[test]
fn battle_pose_repeat_request_keeps_playing_clip() {
    let mut world = pose_test_world();
    world.apply_battle_pose(
        0,
        legaia_engine_battle_vm::battle_action::Pose::Recover as u8,
    );
    world.actors[0].battle_animation.as_mut().unwrap().step = 7;
    world.tick_battle_animations();
    let phase_frame = world.actors[0].pose_frame.clone();
    // Re-requesting the same pose must not rewind the clip.
    world.apply_battle_pose(
        0,
        legaia_engine_battle_vm::battle_action::Pose::Recover as u8,
    );
    world.tick_battle_animations();
    assert_ne!(
        world.actors[0].pose_frame.as_ref().map(|f| f.factor),
        phase_frame.as_ref().map(|f| f.factor),
        "cursor advanced instead of restarting"
    );
}

#[test]
fn battle_pose_without_clips_is_inert() {
    let mut world = World::new();
    world.actors[0].active = true;
    world.apply_battle_pose(
        0,
        legaia_engine_battle_vm::battle_action::Pose::Recover as u8,
    );
    assert_eq!(world.actors[0].battle_pose, None);
    assert!(world.actors[0].battle_animation.is_none());
}

// --- battle hit reactions (retail +0x1EF tag map) ----------------------------

/// Clip set carrying the full party reaction family at identity indices
/// (action tags 2..5 + 0x0B), like a player battle file's record[0].
fn reaction_test_world(with_getup: bool) -> World {
    let mut world = World::new();
    world.actors[0].active = true;
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.max_hp = 100;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 12];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[2] = Some(pose_test_clip(2, 2, 20));
    clips[4] = Some(pose_test_clip(4, 2, 40));
    if with_getup {
        clips[5] = Some(pose_test_clip(5, 2, 50));
    }
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world
}

/// Run the active one-shot to completion and let the chain advance once.
fn finish_reaction_clip(world: &mut World) {
    world.actors[0].battle_animation.as_mut().unwrap().step = 1024;
    world.tick_battle_animations(); // finishes the clip
    world.tick_battle_animations(); // chain reacts to `finished`
}

#[test]
fn hit_reaction_knockdown_then_getup_then_idle() {
    // An actor WITH a get-up entry plays knockdown (tag 4) on any hit,
    // then get-up (tag 5), then resumes idle - the FUN_800402F4 staging +
    // FUN_8004AD80 record-type-4 chain.
    let mut world = reaction_test_world(true);
    world.queue_battle_reaction(0, true);
    assert_eq!(world.actors[0].battle_reaction, Some(4));
    finish_reaction_clip(&mut world);
    assert_eq!(
        world.actors[0].battle_reaction,
        Some(5),
        "living actor chains knockdown into get-up"
    );
    finish_reaction_clip(&mut world);
    assert_eq!(world.actors[0].battle_reaction, None);
    assert_eq!(
        world.actors[0].battle_pose,
        Some(legaia_engine_battle_vm::battle_action::Pose::Idle as u8),
        "reaction chain ends in the idle loop"
    );
}

/// On retail's cadence a reaction clip's natural end sits behind the advance
/// that crossed it (`0x80047A48..0x80047B58`): the pass that finishes the clip
/// lands on the idle at its first keyframe, not one pass later on the held
/// last frame, so state `0x20`'s hold lets go a pass sooner.
#[test]
fn a_reaction_ends_on_the_pass_its_cursor_crosses_the_end() {
    let mut world = reaction_test_world(false);
    world.mode = SceneMode::Battle;
    world.toggles.battle_sm_frame_cadence = true;
    world.queue_battle_reaction(0, true);
    // The primitive only stages; the seat's next node tick commits.
    assert_eq!(world.actors[0].battle_reaction_stage, Some((2, 5)));
    assert_eq!(world.actors[0].battle_reaction, None);
    world.battle.anim_pass_vsyncs = 1;
    world.tick_battle_anim_pass();
    assert_eq!(world.actors[0].battle_reaction, Some(2));
    assert_eq!(world.actors[0].battle_reaction_stage, None);
    world.actors[0].battle_animation.as_mut().unwrap().step = 1024;
    world.battle.anim_pass_vsyncs = 2;
    world.tick_battle_anim_pass();
    assert_eq!(world.actors[0].battle_reaction, None, "the same pass");
    assert_eq!(
        world.actors[0].battle_pose,
        Some(legaia_engine_battle_vm::battle_action::Pose::Idle as u8)
    );
}

/// A staged reaction over a clip whose entry carries the `+0x76` lock: the
/// bit-0 cut refuses it, the clip plays on, and its natural end commits the
/// staged byte - or idle, when the stage raised bit 2. The damage primitive
/// raises bit 2 on the flinch arm only (`0x80042124..0x80042144`), so a
/// flinch is dropped and a knockdown lands at the end.
#[test]
fn a_locked_clip_holds_a_staged_reaction_to_its_natural_end() {
    for (with_getup, want) in [(false, None), (true, Some(4))] {
        let mut world = reaction_test_world(with_getup);
        world.mode = SceneMode::Battle;
        world.toggles.battle_sm_frame_cadence = true;
        let mut swing = pose_test_clip(0x0C, 4, 70);
        swing.effect_script = vec![0; 0x80];
        swing.effect_script[0] = 0x0C;
        swing.effect_script[0x10] = 2;
        swing.effect_script[0x76] = 1;
        world.actors[0].battle_animation =
            legaia_engine_battle::battle_anim::MonsterAnimPlayer::new_one_shot(&swing);
        world.queue_battle_reaction(0, true);
        assert!(world.actors[0].battle_reaction_stage.is_some());
        world.battle.anim_pass_vsyncs = 1;
        world.tick_battle_anim_pass();
        assert_eq!(
            world.actors[0].battle_reaction, None,
            "the lock refuses the cut"
        );
        assert!(
            world.actors[0].battle_reaction_stage.is_some(),
            "still staged"
        );
        world.actors[0].battle_animation.as_mut().unwrap().step = 1024;
        world.battle.anim_pass_frame = None;
        world.tick_battle_anim_pass();
        assert_eq!(
            world.actors[0].battle_reaction_stage, None,
            "the end takes it"
        );
        assert_eq!(world.actors[0].battle_reaction, want, "getup {with_getup}");
    }
}

#[test]
fn hit_reaction_light_flinch_without_getup() {
    // A surviving target with no get-up entry plays the light flinch
    // (tag 2) and falls straight back to idle.
    let mut world = reaction_test_world(false);
    world.queue_battle_reaction(0, true);
    assert_eq!(world.actors[0].battle_reaction, Some(2));
    finish_reaction_clip(&mut world);
    assert_eq!(world.actors[0].battle_reaction, None);
    assert_eq!(
        world.actors[0].battle_pose,
        Some(legaia_engine_battle_vm::battle_action::Pose::Idle as u8)
    );
}

#[test]
fn hit_reaction_lethal_knockdown_holds_downed_frame() {
    let mut world = reaction_test_world(true);
    world.actors[0].battle.hp = 0;
    world.queue_battle_reaction(0, false);
    assert_eq!(world.actors[0].battle_reaction, Some(4));
    world.actors[0].battle_animation.as_mut().unwrap().step = 1024;
    for _ in 0..5 {
        world.tick_battle_animations();
    }
    // Dead: the knockdown holds its final keyframe; no get-up, no idle.
    assert_eq!(world.actors[0].battle_reaction, Some(4));
    assert!(
        world.actors[0]
            .battle_animation
            .as_ref()
            .unwrap()
            .finished()
    );
}

#[test]
fn reaction_outranks_pose_requests_until_done() {
    let mut world = reaction_test_world(true);
    world.queue_battle_reaction(0, true);
    // The SM keeps requesting poses every frame; an in-flight reaction wins.
    world.apply_battle_pose(0, legaia_engine_battle_vm::battle_action::Pose::Idle as u8);
    assert_eq!(world.actors[0].battle_reaction, Some(4));
    assert_eq!(world.actors[0].battle_pose, None);
}

#[test]
fn monster_slots_only_honor_idle_pose() {
    // Monster clip vectors are archive-order, not pose-indexed: a Defeat
    // pose request on a monster slot must not start clip index 9 (an
    // arbitrary spell action). Idle still maps to clip 0.
    let mut world = World::new();
    world.actors[3].active = true;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 12];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[9] = Some(pose_test_clip(0x0C, 2, 90));
    world.set_actor_battle_action_clips(3, std::sync::Arc::new(clips));
    world.apply_battle_pose(
        3,
        legaia_engine_battle_vm::battle_action::Pose::Defeat as u8,
    );
    assert_eq!(world.actors[3].battle_pose, None, "non-idle pose ignored");
    world.apply_battle_pose(3, legaia_engine_battle_vm::battle_action::Pose::Idle as u8);
    assert_eq!(world.actors[3].battle_pose, Some(6), "idle still maps");
}

// --- staged battle anim commit (weapon swings + art bank) -------------------

/// World with party actor 0 carrying swing clips (slots 0xC..0xF) and a
/// 12-record art bank, both synthetic.
fn staged_anim_test_world() -> World {
    let mut world = World::new();
    world.actors[0].active = true;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 22];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    for (slot, clip) in clips.iter_mut().enumerate().take(0x10).skip(0xC) {
        *clip = Some(pose_test_clip(slot as u8, 3, slot as i16 * 100));
    }
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    let bank: Vec<Option<MonsterAnimation>> = (0..12)
        .map(|r| Some(pose_test_clip(0x10 + r as u8, 3, 1000 + r as i16)))
        .collect();
    world.set_actor_battle_art_bank(0, std::sync::Arc::new(bank));
    world
}

#[test]
fn staged_swing_id_plays_equipment_clip_one_shot() {
    let mut world = staged_anim_test_world();
    world.actors[0].battle.queued_anim = 0x0C;
    world.commit_staged_battle_anim(0);
    let a = &world.actors[0];
    // Direct commit: no rewrite, ids converge on the swing slot.
    assert_eq!(a.battle.queued_anim, 0x0C);
    assert_eq!(a.battle.current_anim, 0x0C);
    assert_eq!(a.battle_staged_anim, Some(0x0C));
    // The swing clip (frame 0 tx = 0xC * 100) replaced the player.
    let mut p = a.battle_animation.clone().unwrap();
    p.step = 0; // sample frame 0
    assert_eq!(p.tick().bone_outputs[0].0[0], 0x0C * 100);
    assert!(!p.finished(), "one-shot, not yet finished");
}

#[test]
fn staged_art_ids_rewrite_to_dynamic_slots() {
    // 0x10 and 0x1A install at slot 0x11; other art ids at 0x10 - the
    // FUN_8004AD80 rewrite lands in BOTH id fields.
    for (staged, slot, record) in [(0x10u8, 0x11u8, 0usize), (0x1A, 0x11, 10), (0x12, 0x10, 2)] {
        let mut world = staged_anim_test_world();
        world.actors[0].battle.queued_anim = staged;
        world.commit_staged_battle_anim(0);
        let a = &world.actors[0];
        assert_eq!(a.battle.queued_anim, slot, "staged {staged:#x} rewritten");
        assert_eq!(a.battle.current_anim, slot);
        assert_eq!(a.battle_staged_anim, Some(slot));
        // The materialized clip is bank record `staged - 0x10`.
        let mut p = a.battle_animation.clone().unwrap();
        p.step = 0;
        assert_eq!(
            p.tick().bone_outputs[0].0[0],
            1000 + record as i16,
            "staged {staged:#x} materializes bank record {record}"
        );
    }
}

#[test]
fn staged_id_without_art_bank_is_a_plain_entry_index() {
    // Monsters carry no bank: ids >= 0x10 index the action-clip vector
    // directly (archive entry indices).
    let mut world = World::new();
    world.actors[3].active = true;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 24];
    clips[0x12] = Some(pose_test_clip(0x12, 3, 700));
    world.set_actor_battle_action_clips(3, std::sync::Arc::new(clips));
    world.actors[3].battle.queued_anim = 0x12;
    world.commit_staged_battle_anim(3);
    let a = &world.actors[3];
    assert_eq!(a.battle.queued_anim, 0x12, "no rewrite without a bank");
    assert_eq!(a.battle.current_anim, 0x12);
    let mut p = a.battle_animation.clone().unwrap();
    p.step = 0;
    assert_eq!(p.tick().bone_outputs[0].0[0], 700);
}

#[test]
fn staged_id_without_clip_converges_and_clears_advance_done() {
    use legaia_engine_battle_vm::battle_action::ActorFlags;
    let mut world = World::new();
    world.actors[0].active = true;
    world.actors[0].battle.queued_anim = 0x0C;
    world.actors[0]
        .battle
        .flag_bits
        .set(ActorFlags::ADVANCE_DONE);
    world.commit_staged_battle_anim(0);
    let a = &world.actors[0];
    // Clip-less host: a zero-length swing - ids converge, the attack
    // chain's read gate opens immediately.
    assert_eq!(a.battle.current_anim, 0x0C);
    assert!(!a.battle.flag_bits.has(ActorFlags::ADVANCE_DONE));
    assert!(a.battle_staged_anim.is_none());
}

#[test]
fn staged_swing_finish_clears_gate_and_resumes_idle() {
    use legaia_engine_battle_vm::battle_action::ActorFlags;
    let mut world = staged_anim_test_world();
    world.actors[0].battle.queued_anim = 0x0D;
    world.actors[0]
        .battle
        .flag_bits
        .set(ActorFlags::ADVANCE_DONE);
    world.commit_staged_battle_anim(0);
    assert_eq!(world.actors[0].battle_staged_anim, Some(0x0D));
    // While the swing plays, the SM's per-frame pose requests don't steal
    // the player.
    world.apply_battle_pose(0, legaia_engine_battle_vm::battle_action::Pose::Idle as u8);
    assert_eq!(world.actors[0].battle_staged_anim, Some(0x0D));
    // Run the 3-frame clip to its end, then let the tick observe the
    // finish: gate cleared, ids back to idle 0, idle loop restored.
    world.actors[0].battle_animation.as_mut().unwrap().step = 2048;
    world.tick_battle_animations(); // clip reaches its last keyframe
    world.tick_battle_animations(); // finish observed -> idle restore
    let a = &world.actors[0];
    assert!(a.battle_staged_anim.is_none());
    assert!(!a.battle.flag_bits.has(ActorFlags::ADVANCE_DONE));
    assert_eq!(a.battle.queued_anim, 0, "id pair converges to idle");
    assert_eq!(a.battle.current_anim, 0);
    assert_eq!(
        a.battle_pose,
        Some(legaia_engine_battle_vm::battle_action::Pose::Idle as u8),
        "idle loop resumes after the band"
    );
    assert!(!a.battle_animation.as_ref().unwrap().finished());
}

#[test]
fn a_byte_restaged_behind_its_own_clip_replays_it_at_the_natural_end() {
    // The taiku2 park: a monster stream `[5, 5, 0]` stages the same entry
    // twice. The second stage lands while the first swing plays, so the id
    // pair already agrees; only the stage latch says a byte is waiting.
    // Retail's natural-end path re-commits it (`FUN_80047430`
    // `0x80047B30..0x80047B58` -> `FUN_8004AD80`): the clip replays. The
    // engine used to converge the pair to idle there, dropping the second
    // swing and the hit that lands the combo total on live HP.
    use legaia_engine_battle_vm::battle_action::ActorFlags;
    let mut world = staged_anim_test_world();
    world.actors[0].battle.queued_anim = 0x0C;
    world.commit_staged_battle_anim(0);
    // The strike loop stages the same byte again behind the playing clip.
    world.actors[0].battle.queued_anim = 0x0C;
    world.actors[0]
        .battle
        .flag_bits
        .set(ActorFlags::ADVANCE_DONE);
    world.actors[0].battle_animation.as_mut().unwrap().step = 2048;
    world.tick_battle_animations(); // clip reaches its last keyframe
    // A slow step for the replay's first tick, so it is still in flight.
    world.actors[0].battle_animation.as_mut().unwrap().step = 1;
    world.tick_battle_animations(); // natural end observed -> re-commit
    let a = &world.actors[0];
    assert!(
        !a.battle.flag_bits.has(ActorFlags::ADVANCE_DONE),
        "the re-commit releases the stage latch"
    );
    assert_eq!(a.battle.current_anim, 0x0C, "the swing replays, not idle");
    assert_eq!(a.battle_staged_anim, Some(0x0C));
    assert_eq!(a.battle.input_cursor, 0, "hit index zeroed for the replay");
    assert!(
        !a.battle_animation.as_ref().unwrap().finished(),
        "the replay runs from its first keyframe"
    );
    assert_ne!(
        a.battle_pose,
        Some(legaia_engine_battle_vm::battle_action::Pose::Idle as u8)
    );
    // Nothing staged behind the replay (the SM left the loop with
    // `+0x1DA = 0`): its end converges to idle as before.
    world.actors[0].battle.queued_anim = 0;
    world.actors[0].battle_animation.as_mut().unwrap().step = 2048;
    world.tick_battle_animations();
    world.tick_battle_animations();
    let a = &world.actors[0];
    assert!(a.battle_staged_anim.is_none());
    assert_eq!(a.battle.current_anim, 0);
}

#[test]
fn attack_chain_paces_strikes_by_staged_clip_completion() {
    use legaia_engine_battle_vm::battle_action::{ActionState, ActorFlags, StepOutcome};
    // Full SM-driven check: a two-swing strike script holds in AttackChain
    // while each staged swing plays, reads the next byte only after the
    // clip-end signal, and exits to recovery on the terminator.
    let mut world = staged_anim_test_world();
    world.mode = SceneMode::Battle;
    world.actors[0].battle.liveness = 1;
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.params[0] = 0x0C;
    world.actors[0].battle.params[1] = 0x0D;
    world.actors[0].battle.params[2] = 0x00;
    world.battle_ctx.active_actor = 0;
    world.battle_ctx.action_state = ActionState::AttackChain.as_byte();

    // Step 1 stages swing 0xC; the tick commits + plays it.
    assert_eq!(world.step_battle(), StepOutcome::Stay);
    world.tick_battle_animations();
    assert_eq!(world.actors[0].battle_staged_anim, Some(0x0C));

    // The commit released the stage latch (`FUN_8004AD80`'s `andi 0xF8` /
    // `sb 0x1dc` at the tick's commit paths), so the `0x801E370C` read gate
    // is open WHILE 0x0C is still in flight. Retail stages one ahead: the
    // next step reads 0x0D at once (both captures: the next byte is staged
    // 2-4 vsyncs after the previous commit, `0E` behind a playing `0F`) and,
    // with the terminator at the new cursor, leaves the loop on the SAME
    // step (`0x801E3998..0x801E39AC` -> `0x1F`). The queued byte does not
    // commit yet - retail commits only from the tick's natural-end / event
    // paths, never because `+0x1DA != +0x1D9`.
    assert!(
        !world.actors[0]
            .battle
            .flag_bits
            .has(ActorFlags::ADVANCE_DONE),
        "the commit released the latch"
    );
    let out = world.step_battle();
    assert!(
        matches!(out, StepOutcome::Transition { to, .. }
            if to == ActionState::AttackRecovery.as_byte()),
        "next byte staged one-ahead + terminator -> recovery, got {out:?}"
    );
    assert_eq!(
        world.actors[0].battle.strike_index, 2,
        "cursor on the terminator"
    );
    assert!(
        world.actors[0]
            .battle
            .flag_bits
            .has(ActorFlags::ADVANCE_DONE)
    );
    assert_eq!(world.actors[0].battle.queued_anim, 0x0D);
    world.tick_battle_animations();
    assert_eq!(
        world.actors[0].battle_staged_anim,
        Some(0x0C),
        "0x0D waits for 0x0C's clip boundary"
    );

    // Recovery holds while the latch is set (`0x801E3AEC..0x801E3AF8`).
    assert_eq!(world.step_battle(), StepOutcome::Stay);

    // 0x0C's natural end commits 0x0D in the same tick and releases the
    // latch again.
    world.actors[0].battle_animation.as_mut().unwrap().step = 4096;
    world.tick_battle_animations();
    world.tick_battle_animations();
    assert_eq!(world.actors[0].battle_staged_anim, Some(0x0D));
    assert_eq!(world.actors[0].battle.current_anim, 0x0D);
    assert!(
        !world.actors[0]
            .battle
            .flag_bits
            .has(ActorFlags::ADVANCE_DONE)
    );

    // The next step stages idle over the playing 0x0D and parks the cursor
    // at 0xFF on the way to 0x20 (`0x801E3B04..0x801E3B1C`); the idle waits
    // for 0x0D's own boundary, which is why the last clip's hit lands with
    // the cursor already parked (the kernel's apply gate).
    let out = world.step_battle();
    assert!(
        matches!(out, StepOutcome::Transition { to, .. }
            if to == ActionState::AttackReturn.as_byte()),
        "recovery -> return once the last clip has committed, got {out:?}"
    );
    assert_eq!(
        world.actors[0].battle.strike_index,
        legaia_engine_battle_vm::battle_action::STRIKE_CURSOR_PARKED
    );
    assert_eq!(world.actors[0].battle.queued_anim, 0, "idle staged");
    assert_eq!(
        world.actors[0].battle_staged_anim,
        Some(0x0D),
        "the last clip is still playing under the staged idle"
    );
}

// --- effect-script walk (FUN_801DEA50 via the animation tick) ---------------

/// A clip whose entry head carries one effect-script record at cursor 0:
/// `[frame_gate, effect, x, y, z]` at `+0x14` (RECORD_BASE).
fn scripted_clip(action_id: u8, effect: (u8, u8, i16, i16, i16)) -> MonsterAnimation {
    use legaia_engine_effects::action_effect_script::{RECORD_BASE, RECORD_STRIDE};
    let mut clip = pose_test_clip(action_id, 4, 0);
    let mut head = vec![0u8; RECORD_BASE + 2 * RECORD_STRIDE];
    let (frame, id, x, y, z) = effect;
    head[RECORD_BASE] = frame;
    head[RECORD_BASE + 1] = id;
    head[RECORD_BASE + 2..RECORD_BASE + 4].copy_from_slice(&x.to_le_bytes());
    head[RECORD_BASE + 4..RECORD_BASE + 6].copy_from_slice(&y.to_le_bytes());
    head[RECORD_BASE + 6..RECORD_BASE + 8].copy_from_slice(&z.to_le_bytes());
    clip.effect_script = head;
    clip
}

#[test]
fn committed_clip_effect_script_queues_a_positioned_spawn() {
    let mut world = World::new();
    world.enter_battle(1, 1);
    // Slot 0 (party, seat (0, -800), facing 0 = +Z): a swing at slot 0xC
    // whose script fires effect 0x86 (direct form) at local (0, 300, 250)
    // once the clip reaches frame 1.
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 16];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[0x0C] = Some(scripted_clip(0x0C, (1, 0x86, 0, 300, 250)));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));

    // Stage + commit the swing.
    world.actors[0].battle.queued_anim = 0x0C;
    world.commit_staged_battle_anim(0);
    assert_eq!(
        world.actors[0].battle_effect_cursor, 0,
        "commit resets +0x1F5"
    );
    assert!(world.actors[0].battle_effect_script.is_some());

    world.tick_battle_animations();
    let spawns = world.drain_battle_effect_spawns();
    assert_eq!(spawns.len(), 1, "one record, one spawn");
    let s = spawns[0];
    assert_eq!(s.actor_slot, 0);
    assert_eq!(s.effect, 0x06, "direct bit stripped");
    assert!(s.direct);
    // Facing 0 through the retail LUT pair: the direct branch uses the
    // unbiased angle, so sin(0)=0 / cos(0)=4096 pass the local offset
    // through: x' = x, z' = z. Y is subtracted from the seat Y (0).
    assert_eq!(s.at, (0, -300, -800 + 250));
    assert_eq!(s.facing, 0);
    // The cursor advanced past the consumed record and holds - the next
    // frame does not re-fire it.
    assert_eq!(world.actors[0].battle_effect_cursor, 1);
    world.tick_battle_animations();
    assert!(world.drain_battle_effect_spawns().is_empty());

    // A new commit resets the walk (retail FUN_8004AD80 `sb zero,0x1f5`).
    // Idle staged over the still-playing swing waits for a clip boundary
    // (retail commits only from the tick's natural-end / event paths); the
    // boundary commit itself is what zeroes the cursor.
    world.actors[0].battle.queued_anim = 0;
    world.commit_staged_battle_anim(0);
    assert_eq!(
        world.actors[0].battle_effect_cursor, 1,
        "no boundary yet - the swing is still in flight"
    );
    world.commit_staged_battle_anim_at_boundary(0);
    assert_eq!(world.actors[0].battle_effect_cursor, 0);
}

/// The table arm's code substitution (`0x801DF054..0x801DF094`): a code-`0`
/// record reads as `9` while the active actor has the art slot `0x11`
/// committed, and stays `0` on any other clip.
#[test]
fn an_art_clip_turns_effect_code_zero_into_nine() {
    for (slot, want) in [(0x11u8, 9u8), (0x0C, 0)] {
        let mut world = World::new();
        world.enter_battle(1, 1);
        let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 0x20];
        clips[0] = Some(pose_test_clip(0, 2, 0));
        clips[usize::from(slot)] = Some(scripted_clip(slot, (1, 0x00, 0, 0, 0)));
        world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
        world.actors[0].battle.queued_anim = slot;
        world.commit_staged_battle_anim(0);
        assert_eq!(world.actors[0].battle.current_anim, slot);
        world.battle_ctx.active_actor = 0;
        world.tick_battle_animations();
        let spawns = world.drain_battle_effect_spawns();
        assert_eq!(spawns.len(), 1, "slot {slot:#x}");
        assert!(!spawns[0].direct);
        assert_eq!(spawns[0].effect, want, "slot {slot:#x}");
    }
}

/// On retail's cadence a clip the pass commits fires its frame-0 records on
/// that pass: `FUN_80047430` walks the new clip again after its commits
/// (`0x80047BC4..0x80047C38`), so the effect does not wait a battle frame.
#[test]
fn a_clip_committed_on_the_cadence_pass_walks_its_first_frame_at_once() {
    let mut world = World::new();
    world.enter_battle(1, 1);
    world.toggles.battle_sm_frame_cadence = true;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 16];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    // Gate `1`: the walker fires a record once `frame + 1` reaches it, so
    // this one is due at the clip's first frame.
    clips[0x0C] = Some(scripted_clip(0x0C, (1, 0x86, 0, 300, 250)));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.queued_anim = 0x0C;
    world.battle.anim_pass_vsyncs = 2;
    world.tick_battle_anim_pass();
    assert_eq!(world.actors[0].battle.current_anim, 0x0C, "committed");
    let spawns = world.drain_battle_effect_spawns();
    assert_eq!(
        spawns.len(),
        1,
        "the frame-0 record fires on the commit pass"
    );
    assert_eq!(world.actors[0].battle_effect_cursor, 1);
    // The next pass walks on from the cursor and fires nothing again.
    world.tick_battle_anim_pass();
    assert!(world.drain_battle_effect_spawns().is_empty());
}

#[test]
fn monster_facing_rotates_the_spawn_offset_half_a_turn() {
    let mut world = World::new();
    world.enter_battle(1, 1);
    // Slot 1 is the monster (seat (0, 800), seeded facing 0x800 = -Z). The
    // same local (0, 0, 250) record must land on the OTHER side of the
    // monster: the direct-form rotation by facing 0x800 negates the local Z.
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 8];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[6] = Some(scripted_clip(6, (1, 0x86, 0, 0, 250)));
    world.set_actor_battle_action_clips(1, std::sync::Arc::new(clips));
    world.actors[1].battle.queued_anim = 6;
    world.commit_staged_battle_anim(1);
    world.tick_battle_animations();
    let spawns = world.drain_battle_effect_spawns();
    assert_eq!(spawns.len(), 1);
    // sin(0x800) = 0, cos(0x800) = -4096: z' = -250, toward the party.
    assert_eq!(spawns[0].at, (0, 0, 800 - 250));
    assert_eq!(spawns[0].facing, 0x800);
}

#[test]
fn empty_effect_script_clips_step_nothing() {
    let mut world = World::new();
    world.enter_battle(1, 0);
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 16];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.apply_battle_pose(0, legaia_engine_battle_vm::battle_action::Pose::Idle as u8);
    assert!(world.actors[0].battle_effect_script.is_none());
    world.tick_battle_animations();
    assert!(world.drain_battle_effect_spawns().is_empty());
}

// --- arts slow-motion + after-image ghosts ----------------------------------

/// An art action constant (`>= 0x1B`) drops the whole battle to half speed
/// (`0x8004BB78..0x8004BBA8`, `ctx[+0x243]` clear), and the frozen actor's
/// pose stops advancing while a normal-rate actor's keeps moving.
#[test]
fn art_constant_commit_slows_everyone_and_rate_zero_freezes_the_pose() {
    use legaia_engine_battle_vm::battle_anim_rate::{AnimRate, RATE_HALF};
    let mut world = pose_test_world();
    world.actors[1].active = true;
    let mut bank: Vec<Option<MonsterAnimation>> = vec![None; 16];
    bank[0xB] = Some(pose_test_clip(0x1B, 60, 0));
    world.set_actor_battle_art_bank(0, std::sync::Arc::new(bank));
    world.actors[0].battle.queued_anim = 0x1B;
    world.commit_staged_battle_anim(0);
    assert_eq!(world.actors[0].battle.anim_rate.get(), RATE_HALF);
    assert_eq!(world.actors[1].battle.anim_rate.get(), RATE_HALF);
    // A frozen actor's clip cursor holds exactly.
    world.actors[0].battle.anim_rate = AnimRate(0);
    let before = world.actors[0]
        .battle_animation
        .as_ref()
        .unwrap()
        .current_frame();
    for _ in 0..5 {
        world.tick_battle_animations();
    }
    let after = world.actors[0]
        .battle_animation
        .as_ref()
        .unwrap()
        .current_frame();
    assert_eq!(before, after, "rate 0 froze the clip");
}

// --- trail-keyed test clips -------------------------------------------------

/// A clip with enough parts to carry Vahn's weapon-bone chain
/// (`base_part 0x0C..0x0F`) and the given `+0x77` identity byte. Part `p`'s
/// translation at frame `f` is `(p, f, 0)` so the sweep's ring samples are
/// distinguishable per point and per step.
fn trail_test_clip(attach_key: u8, frames: usize) -> MonsterAnimation {
    use legaia_actor_formats::monster_archive::PartPose;
    let parts = 16usize;
    MonsterAnimation {
        variant_track: Default::default(),
        action_id: 0xC,
        rate: 2,
        attach_key,
        solo_flag: 0,
        impact_class: 0,
        part_count: parts,
        frame_count: frames,
        frames: (0..frames)
            .map(|f| {
                (0..parts)
                    .map(|p| PartPose {
                        tx: p as i16,
                        ty: f as i16,
                        tz: 0,
                        ..Default::default()
                    })
                    .collect()
            })
            .collect(),
        effect_script: Vec::new(),
    }
}

// --- per-clip impact freeze + tint (FUN_8004CE2C pass 2) --------------------

/// Gala's clip-tag-0x18 window freezes the TARGET's pose and arms the
/// impact selector; the tint word itself needs the disc's impact table and
/// stays untouched here (disc-free run).
#[test]
fn gala_clip_key_0x18_freezes_the_target_in_window() {
    use legaia_engine_battle_vm::battle_anim_rate::{RATE_FROZEN, RATE_NORMAL};
    let mut world = World::new();
    world.enter_battle(1, 0);
    // Seat Gala (roster slot 2 -> retail char id 3) at battle ordinal 0.
    world.set_active_party(vec![2]);
    world.battle_ctx.active_actor = 0;
    world.actors[0].battle.active_target = 1;
    // Target: a monster with a ticking idle loop.
    world.actors[1].active = true;
    world.actors[1].battle_monster_id = Some(5);
    let mut mclips: Vec<Option<MonsterAnimation>> = vec![None; 4];
    mclips[0] = Some(pose_test_clip(0, 8, 0));
    world.set_actor_battle_action_clips(1, std::sync::Arc::new(mclips));
    world.apply_battle_pose(1, legaia_engine_battle_vm::battle_action::Pose::Idle as u8);
    // Acting clip: identity byte 0x18 (rate 2 -> the staged advance is 8
    // sixteenths per tick, so the 0x40..=0x80 window opens around tick 8).
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 16];
    clips[0] = Some(trail_test_clip(0, 8));
    clips[0xC] = Some(trail_test_clip(0x18, 30));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.queued_anim = 0xC;
    world.commit_staged_battle_anim(0);
    // Before the window: no freeze.
    for _ in 0..4 {
        world.tick_battle_animations();
    }
    assert_eq!(world.actors[1].battle.anim_rate.get(), RATE_NORMAL);
    // The acting selector is stamped on the tag match alone (0x8004D1B4).
    assert_eq!(world.actors[0].battle.impact_state, 2);
    // Into the window: the target's rate drops to the freeze.
    for _ in 0..6 {
        world.tick_battle_animations();
    }
    assert_eq!(
        world.actors[1].battle.anim_rate.get(),
        RATE_FROZEN,
        "target pose frozen inside the 0x40..=0x80 cursor window"
    );
    assert_eq!(world.actors[1].battle.impact_state, 2);
    // The world-side character gate: a Vahn seat playing the same clip
    // never freezes (Vahn's 0x18 arm is tint-only, and its window sits
    // elsewhere) - the target keeps its normal rate.
    let mut world2 = World::new();
    world2.enter_battle(1, 0);
    world2.set_active_party(vec![0]); // Vahn
    world2.battle_ctx.active_actor = 0;
    world2.actors[0].battle.active_target = 1;
    world2.actors[1].active = true;
    let mut clips2: Vec<Option<MonsterAnimation>> = vec![None; 16];
    clips2[0] = Some(trail_test_clip(0, 8));
    clips2[0xC] = Some(trail_test_clip(0x18, 30));
    world2.set_actor_battle_action_clips(0, std::sync::Arc::new(clips2));
    world2.actors[0].battle.queued_anim = 0xC;
    world2.commit_staged_battle_anim(0);
    for _ in 0..10 {
        world2.tick_battle_animations();
    }
    assert_eq!(world2.actors[1].battle.anim_rate.get(), RATE_NORMAL);
}

/// Gala's clip-tag-`0x67` arm raises the chained ribbon at the TARGET's seat
/// with trail id `0xC` on exactly the frames the cursor sits in
/// `0xB0..=0xF0` (`FUN_8004CE2C` `0x8004D1E8..0x8004D248`).
#[test]
fn gala_clip_key_0x67_raises_the_ribbon_at_the_target_seat_in_window() {
    use legaia_engine_battle_vm::battle_impact_fx as ifx;
    let mut world = World::new();
    world.enter_battle(1, 0);
    world.set_active_party(vec![2]);
    world.battle_ctx.active_actor = 0;
    world.actors[0].battle.active_target = 1;
    world.actors[1].active = true;
    world.actors[1].battle_monster_id = Some(5);
    world.actors[1].battle.seat = Some((120, 800));
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 16];
    clips[0] = Some(trail_test_clip(0, 8));
    clips[0xC] = Some(trail_test_clip(ifx::GALA_EFFECT_TAG, 40));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.queued_anim = 0xC;
    world.commit_staged_battle_anim(0);
    let mut raised = Vec::new();
    for _ in 0..40 {
        // The pass reads the cursor as it stands when the pass runs, which
        // is before this tick's advance.
        let cursor = world.actors[0]
            .battle_animation
            .as_ref()
            .map_or(0, |p| p.cursor_sixteenths());
        world.tick_battle_animations();
        let r = world.battle.clip_ribbon;
        assert_eq!(
            r.is_some(),
            ifx::GALA_EFFECT_WINDOW.contains(&cursor),
            "ribbon raised iff the cursor {cursor:#x} is in the window"
        );
        if let Some(r) = r {
            raised.push(r);
        }
    }
    assert!(!raised.is_empty(), "the window never opened");
    for r in raised {
        assert_eq!(r.seat, [120, 0, 800]);
        assert_eq!(r.trail_id, ifx::GALA_EFFECT_ARG);
    }
}

/// A solo commit's sweep (`FUN_8004E13C`): the bystanders go to the dim arm
/// from neutral, not from the engine's resting zero word, and ease toward
/// `0x20` a lane at `0x20` a tick under the full blend. The acting seat and
/// its target stay lit, as does a dead seat.
#[test]
fn a_solo_commit_dims_the_live_bystanders() {
    use legaia_engine_battle_vm::battle_formulas::STATE_DIM;
    use legaia_engine_battle_vm::battle_impact_fx as ifx;
    let mut world = World::new();
    world.enter_battle(3, 0);
    world.battle_ctx.active_actor = 0;
    world.actors[0].battle.active_target = 3;
    for i in 0..4 {
        world.actors[i].active = true;
        world.actors[i].battle.hp = 10;
        world.actors[i].battle.render_flag = 0;
        world.actors[i].battle.render_color = 0;
    }
    world.actors[2].battle.hp = 0;
    world.sweep_solo_bystanders();
    assert_eq!(world.actors[0].battle.render_flag, 0, "the acting seat");
    assert_eq!(world.actors[3].battle.render_flag, 0, "its target");
    assert_eq!(world.actors[2].battle.render_flag, 0, "a dead seat");
    assert_eq!(world.actors[1].battle.render_flag, STATE_DIM);
    assert_eq!(
        world.actors[1].battle.render_color,
        ifx::IMPACT_NEUTRAL_STATE
    );
    world.tick_battle_animations();
    let lanes = world.actors[1].battle.render_color & 0x3FF;
    assert_eq!(lanes, 0x200 - 0x20, "one ease step at the arm's rate `4`");
    assert_eq!(world.actors[1].battle.render_blend, 0x1000);
}

/// The ghost pass on a pose of the caller's: a bystander inside the near radius
/// of that pose's view axis takes the `0x83` mode bits, and off it drops them.
/// `FUN_8004DC68` reads the pose the walker left a pass earlier, so a capture
/// seat hands that pose in rather than the one it stands on.
#[test]
fn the_ghost_pass_reads_the_pose_it_is_handed() {
    use legaia_engine_battle_loop::round_flow::RoundPhase;
    use legaia_engine_camera::projection::BattleCamPose;
    let mut world = World::new();
    world.enter_battle(3, 0);
    world.battle.round_flow.phase = RoundPhase::Execute;
    world.battle_ctx.action_state = 0x1E;
    world.battle_ctx.active_actor = 0;
    world.actors[0].battle.active_target = 3;
    world.actors[1].battle.max_hp = 10;
    world.actors[1].battle_monster_id = None;
    world.actors[1].move_state.world_x = 0;
    world.actors[1].move_state.world_z = -960;
    let pose = |focus_z: f32| BattleCamPose {
        pitch: 0.0,
        yaw: 0.0,
        tr: [0.0, 1280.0, 4915.0],
        focus: [0.0, 0.0, focus_z],
    };
    world.refresh_battle_camera_ghost_on(pose(0.0));
    let near = world.actors[1].battle.flag_word & 0x8300_0000;
    world.refresh_battle_camera_ghost_on(pose(8000.0));
    let far = world.actors[1].battle.flag_word & 0x8300_0000;
    assert_ne!(near, far, "the handed pose decides the bits");
    assert_eq!(far, 0);
}

/// The armed tint eases back to the neutral word, the blend drains, and
/// the selector retires - `FUN_80050120` arm 0 over the `FUN_80050F30`
/// per-lane ease, in the disassembly's order (`0x800501A4..0x80050210`):
/// the eased word is compared against `0x20080200` on the SAME frame it
/// was eased, a non-zero `+0x0C` then drains by `0x20`, and only a zero
/// `+0x0C` clears `+0x21F`. So with no blend armed the selector clears on
/// the arrival frame itself, not a frame later.
#[test]
fn impact_tint_decays_to_neutral_and_clears() {
    use legaia_engine_battle_vm::battle_impact_fx as ifx;
    let mut world = World::new();
    world.enter_battle(1, 0);
    world.actors[0].battle.impact_state = 2;
    // Two ease steps above neutral on the R lane.
    world.actors[0].battle.render_color = ifx::IMPACT_NEUTRAL_STATE + 0x10;
    world.tick_battle_animations();
    assert_eq!(
        world.actors[0].battle.render_color,
        ifx::IMPACT_NEUTRAL_STATE + 0x8
    );
    assert_eq!(world.actors[0].battle.impact_state, 2);
    world.tick_battle_animations();
    assert_eq!(
        world.actors[0].battle.render_color,
        ifx::IMPACT_NEUTRAL_STATE
    );
    assert_eq!(
        world.actors[0].battle.impact_state, 0,
        "no blend armed: the selector retires on the arrival frame (`sb zero,0x21f` at 0x80050210)"
    );
    // With the retail hit triple armed the blend drains first: 0x1000 at
    // 0x20 per frame is 128 frames of blend after the colour arrives, and
    // the selector waits for all of them.
    world.actors[0].battle.impact_state = 1;
    world.actors[0].battle.render_color = ifx::IMPACT_NEUTRAL_STATE + 0x8;
    world.actors[0].battle.render_blend = 0x1000;
    world.tick_battle_animations();
    assert_eq!(
        world.actors[0].battle.render_color,
        ifx::IMPACT_NEUTRAL_STATE
    );
    assert_eq!(world.actors[0].battle.render_blend, 0x1000 - 0x20);
    assert_eq!(world.actors[0].battle.impact_state, 1);
    for _ in 0..(0x1000 / 0x20 - 1) {
        world.tick_battle_animations();
    }
    assert_eq!(world.actors[0].battle.render_blend, 0);
    assert_eq!(
        world.actors[0].battle.impact_state, 1,
        "the frame the blend hit zero"
    );
    world.tick_battle_animations();
    assert_eq!(world.actors[0].battle.impact_state, 0);
    // A render-flag override (the target cursor's `5`) is a hold arm -
    // retail's +0x21C dispatch never reaches the ease.
    world.actors[0].battle.impact_state = 1;
    world.actors[0].battle.render_color = ifx::IMPACT_NEUTRAL_STATE + 0x20;
    world.actors[0].battle.render_flag = 5;
    world.tick_battle_animations();
    assert_eq!(
        world.actors[0].battle.render_color,
        ifx::IMPACT_NEUTRAL_STATE + 0x20
    );
}

/// The same SpecialStarter commit raises the **Arts announcement banner**
/// (`FUN_8004AD80` `0x8004B754..0x8004BB44`), the per-frame ramp walks its
/// slide clock, and the shared render read hands both hosts the quad pair.
///
/// The three things a wire can each be missing on their own: the raise, the
/// step, and the emit. Asserted in that order.
#[test]
fn special_starter_commit_raises_the_arts_banner_and_the_ramp_emits_quads() {
    use legaia_engine_battle_vm::battle_action::{BANNER_DEFAULT, BANNER_SEAT_FLAG, LEVEL_MAX};
    let mut world = pose_test_world();
    world.mode = SceneMode::Battle;
    let mut bank: Vec<Option<MonsterAnimation>> = vec![None; 16];
    bank[0xA] = Some(pose_test_clip(0x1A, 60, 0));
    world.set_actor_battle_art_bank(0, std::sync::Arc::new(bank));
    assert_eq!(
        world.battle_ctx.arts_banner_stage, 0,
        "idle before the commit"
    );
    assert!(world.battle_arts_banner_quads().is_empty());

    world.actors[0].battle.queued_anim = 0x1A;
    world.commit_staged_battle_anim(0);
    assert_eq!(
        world.battle_ctx.arts_banner_stage, BANNER_DEFAULT,
        "with no seat flag and no table pick the commit falls back to banner 2"
    );
    assert_eq!(
        world.battle_ctx.arts_banner_level, 0,
        "a raise restarts the slide"
    );

    // Four layers at a zero clock: the three gated trail layers plus the
    // opaque one, two quads each.
    let quads = world.battle_arts_banner_quads();
    assert_eq!(quads.len(), 8, "four layers, a quad pair each");
    assert!(
        quads.iter().any(|q| q.code == 0x2C),
        "the ungated layer is the opaque one"
    );
    assert!(
        quads.iter().any(|q| q.code == 0x2E),
        "the trail layers are semi-transparent"
    );

    // The ramp walks the clock (`frame_delta << 3`) and saturates; by then the
    // trail has retracted to the one opaque pair.
    world.tick_arts_banner(1);
    assert_eq!(world.battle_ctx.arts_banner_level, 8);
    for _ in 0..64 {
        world.tick_arts_banner(1);
    }
    assert_eq!(world.battle_ctx.arts_banner_level, LEVEL_MAX);
    assert_eq!(
        world.battle_arts_banner_quads().len(),
        2,
        "a landed banner is a single opaque pair"
    );

    // The queue-builder's own side array is the middle pick, and its two
    // marks land on their own banner positions: the build loop's `1` is
    // `NEW ARTS!!`, the Super tail-replace's `4` is `SUPER ARTS!!`.
    for (mark, banner) in [
        (
            legaia_engine_battle_vm::battle_action::BUILD_STARTER_MARK,
            1u8,
        ),
        (
            legaia_engine_battle_vm::battle_action::SUPER_STARTER_MARK,
            4,
        ),
    ] {
        let mut marks = [0u32; legaia_engine_battle_vm::battle_action::ACTION_QUEUE_CAP];
        marks[0] = mark;
        world.actors[0].battle.starter_marks = Some(marks);
        world.actors[0].battle.strike_index = 1;
        world.actors[0].battle.current_anim = 0;
        world.actors[0].battle.queued_anim = 0x1A;
        world.commit_staged_battle_anim_at_boundary(0);
        assert_eq!(world.battle_ctx.arts_banner_stage, banner);
    }
    world.actors[0].battle.starter_marks = None;
    world.actors[0].battle.strike_index = 0;

    // A seat flag picks banner 3 instead, and the re-commit of the context's
    // own actor cancels whatever is in flight.
    world.battle_ctx.arts_banner_seat_flags[0] = 1;
    world.actors[0].battle.current_anim = 0;
    world.actors[0].battle.queued_anim = 0x1A;
    world.commit_staged_battle_anim_at_boundary(0);
    assert_eq!(world.battle_ctx.arts_banner_stage, BANNER_SEAT_FLAG);

    world.battle_ctx.active_actor = 0;
    world.actors[0].battle.current_anim = 0;
    world.actors[0].battle.queued_anim = 0x1A;
    world.commit_staged_battle_anim_at_boundary(0);
    // The prologue cancel ran first (3 -> 7), then the raise re-seated 3.
    assert_eq!(world.battle_ctx.arts_banner_stage, BANNER_SEAT_FLAG);
    // A cancel with no raise behind it retires on the next frame.
    world.battle_ctx.arts_banner_stage = BANNER_SEAT_FLAG + 4;
    assert!(!world.tick_arts_banner(1));
    assert_eq!(world.battle_ctx.arts_banner_stage, 0);
    assert!(world.battle_arts_banner_quads().is_empty());
}

/// The SpecialStarter commit's voice (`FUN_8004AD80` `0x8004B754..0x8004BB44`):
/// with no seat flag and no starter mark, the constant behind the starter
/// names a Hyper Art and the commit dispatches its fanfare - Vahn's Burning
/// Flare `0x1C` is id `0x104` or `0x107` (`XA1.XA` channel 4 or 7) on one
/// `rand()` coin. A second request inside the first clip's span plays over
/// it (the dispatcher's gate is a data stream read, not the clip). The
/// known-art starter (`0x19`) commit asks
/// the arts-voice selector instead, which a staged table resolves onto
/// `XA2.XA`.
#[test]
fn starter_commits_voice_the_hyper_fanfare_and_the_arts_shout() {
    let mut world = pose_test_world();
    world.mode = SceneMode::Battle;
    world.audio.xa_cue_durations = Some(vec![10; 0x200]);
    let stage = |w: &mut World, queue: &[u8], cursor: u8, staged: u8| {
        let b = &mut w.actors[0].battle;
        b.params[..queue.len()].copy_from_slice(queue);
        b.params[queue.len()] = 0;
        b.strike_index = cursor;
        b.starter_marks = None;
        b.current_anim = 0;
        b.queued_anim = staged;
    };
    stage(&mut world, &[0x1A, 0x1C], 1, 0x1A);
    world.commit_staged_battle_anim_at_boundary(0);
    let cues = world.drain_battle_xa_cues();
    assert_eq!(cues.len(), 1, "{cues:?}");
    assert_eq!(cues[0].clip, 0, "Vahn's fanfare bank, XA1");
    assert!(matches!(cues[0].channel, 4 | 7), "{cues:?}");
    assert_eq!(cues[0].duration_sectors, (10u32 * 60).div_ceil(100));

    // Inside that clip's read span the next one still starts.
    world.battle_ctx.arts_banner_stage = 0;
    stage(&mut world, &[0x1A, 0x1C], 1, 0x1A);
    world.commit_staged_battle_anim_at_boundary(0);
    assert_eq!(
        world.drain_battle_xa_cues().len(),
        1,
        "a sounding clip does not hold the dispatcher"
    );
    world.audio.battle_xa_busy_frames = 0;

    // A non-Hyper constant behind the starter plays nothing.
    world.battle_ctx.arts_banner_stage = 0;
    stage(&mut world, &[0x1A, 0x27], 1, 0x1A);
    world.commit_staged_battle_anim_at_boundary(0);
    assert!(world.drain_battle_xa_cues().is_empty());

    // The known-art starter: a selector call keyed on the constant behind it.
    use legaia_art::arts_voice::{
        ArtsVoiceSelectorTables, DUR_TABLE_VA, FIRST_HALF_VARIANT_BASES, RANGE_TABLE_VA,
        SELECTOR_SPAN_END, SELECTOR_SPAN_START,
    };
    let mut b = vec![0u8; (SELECTOR_SPAN_END - SELECTOR_SPAN_START) as usize];
    let mut put = |va: u32, bytes: &[u8]| {
        let o = (va - SELECTOR_SPAN_START) as usize;
        b[o..o + bytes.len()].copy_from_slice(bytes);
    };
    put(RANGE_TABLE_VA, &[0x1F, 0x29, 0x2B]);
    put(FIRST_HALF_VARIANT_BASES[0][0] + 2 * 0x0F, &[3, 5]);
    put(DUR_TABLE_VA, &[0, 0, 0, 50, 0, 70]);
    world.audio.arts_voice_tables = Some(std::sync::Arc::new(
        ArtsVoiceSelectorTables::from_span_bytes(b),
    ));
    world.battle.move_label_closed = false;
    stage(&mut world, &[0x19, 0x27], 1, 0x19);
    world.commit_staged_battle_anim_at_boundary(0);
    let shouts = world.drain_battle_shout_cues();
    assert_eq!(shouts.len(), 1);
    assert_eq!(
        (shouts[0].cslot, shouts[0].action, shouts[0].resolved),
        (0, 0x27, true)
    );
    let cues = world.drain_battle_xa_cues();
    assert_eq!(cues.len(), 1, "{cues:?}");
    assert_eq!(cues[0].clip, 1, "Vahn's shout bank, XA2");
    let want_dur = |raw: u32| (raw * 60).div_ceil(100);
    assert!(
        (cues[0].channel, cues[0].duration_sectors) == (3, want_dur(50))
            || (cues[0].channel, cues[0].duration_sectors) == (5, want_dur(70)),
        "{cues:?}"
    );
    // The art clip behind it (cursor now past the constant) asks for nothing.
    stage(&mut world, &[0x19, 0x27], 2, 0x27);
    world.commit_staged_battle_anim_at_boundary(0);
    assert!(world.drain_battle_shout_cues().is_empty());
}

// --- the decoder's last-frame tween + the end-of-clip displacement ---------

/// A two-frame one-shot whose entry head carries an end-of-clip step
/// (`+0x0E`), translating Z from `tz0` to `tz1`.
fn end_step_clip(action_id: u8, tz0: i16, tz1: i16, end_step: i16) -> MonsterAnimation {
    use legaia_actor_formats::monster_archive::PartPose;
    let mut head = vec![0u8; legaia_actor_formats::monster_archive::EFFECT_SCRIPT_HEAD_BYTES];
    head[0x0E..0x10].copy_from_slice(&end_step.to_le_bytes());
    let pose = |tz| {
        vec![PartPose {
            tx: 0,
            ty: 0,
            tz,
            rx: 0,
            ry: 0,
            rz: 0,
        }]
    };
    MonsterAnimation {
        variant_track: Default::default(),
        action_id,
        rate: 2,
        attach_key: 0,
        solo_flag: 0,
        impact_class: 0,
        effect_script: head,
        part_count: 1,
        frame_count: 2,
        frames: vec![pose(tz0), pose(tz1)],
    }
}

fn end_step_world(end_step: i16) -> World {
    let mut world = World::new();
    world.actors[0].active = true;
    world.actors[0].battle.hp = 100;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 12];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    // A lunge: the body ends 64 units forward in model Z, and the entry's
    // step carries the actor the same 64 at the natural end.
    clips[10] = Some(end_step_clip(10, 0, 64, end_step));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.apply_battle_pose(0, legaia_engine_battle_vm::battle_action::Pose::Idle as u8);
    world.actors[0].battle.queued_anim = 10;
    world.commit_staged_battle_anim(0);
    world
}

#[test]
fn a_swing_tweens_into_the_idle_it_falls_back_to_and_steps_the_actor() {
    let mut world = end_step_world(64);
    assert_eq!(world.actors[0].battle_staged_anim, Some(10));
    // Half a keyframe per tick (the action branch doubles the base step).
    world.actors[0].battle_animation.as_mut().unwrap().step = 64;
    let z0 = world.actors[0].move_state.world_z;
    let mut zs = Vec::new();
    for _ in 0..4 {
        world.tick_battle_animations();
        zs.push(world.actors[0].pose_frame.as_ref().unwrap().bone_outputs[0].0[2]);
    }
    // 0.5 / 1.0 / 1.5 / crossing: the last frame (64) blends toward the idle's
    // frame 0 (0) plus the Z term (64) - the body holds 64 through the
    // interval instead of snapping back - and the crossing tick draws the
    // idle's frame 0 with the actor moved 64 along its facing (facing 0: +Z).
    assert_eq!(zs, vec![32, 64, 64, 0]);
    assert_eq!(world.actors[0].move_state.world_z, z0.wrapping_add(64));
}

#[test]
fn a_downed_actor_does_not_tween_or_step() {
    let mut world = end_step_world(64);
    world.actors[0].battle.hp = 0;
    world.actors[0].battle_animation.as_mut().unwrap().step = 64;
    let z0 = world.actors[0].move_state.world_z;
    let mut zs = Vec::new();
    for _ in 0..4 {
        world.tick_battle_animations();
        zs.push(world.actors[0].pose_frame.as_ref().unwrap().bone_outputs[0].0[2]);
    }
    // HP 0 fails the gate: the last frame blends toward itself, and the
    // natural end moves nothing (no Seru staged). The pass that crosses the
    // end commits the idle behind its advance and shows its first keyframe
    // (`0x80047A48..0x80047B58`).
    assert_eq!(zs, vec![32, 64, 64, 0]);
    assert_eq!(world.actors[0].move_state.world_z, z0);
}

/// A clip whose entry head carries an animation cue track at `+0x54`
/// (`(frame, cue)` pairs, `FUN_800508DC`).
fn cue_track_clip(action_id: u8, track: &[(u16, u16)]) -> MonsterAnimation {
    let mut clip = pose_test_clip(action_id, 8, 0);
    let mut head = vec![0u8; legaia_actor_formats::monster_archive::EFFECT_SCRIPT_HEAD_BYTES];
    for (k, &(frame, cue)) in track.iter().enumerate() {
        let o = legaia_engine_battle::anim_cue::ANIM_CUE_TRACK_OFFSET + k * 4;
        head[o..o + 2].copy_from_slice(&frame.to_le_bytes());
        head[o + 2..o + 4].copy_from_slice(&cue.to_le_bytes());
    }
    clip.effect_script = head;
    clip
}

/// A party swing's cue track reaches the SFX ring through the battle sound
/// funnel: `0x1A` takes the low leg (`id - 1`), `0xA7` the high leg - ring id
/// `0x243` after its `bse.dat` row's `+4` category is written with the
/// literal `2` a party cue `>= 0xA7` gets. Each cue fires once, on its frame.
#[test]
fn a_committed_swing_plays_its_animation_cue_track() {
    use legaia_engine_system::sound_state::SfxRingOp;
    let mut world = World::new();
    world.enter_battle(1, 1);
    // A `bse.dat`-shaped bank: `[tag 1][body_offset 4]` + rows; row `0x43`
    // (`0x243 - 0x200`) is the one the swing's `0xA7` lands on.
    let mut bse = vec![0u8; 4 + 0x50 * 8];
    bse[0] = 1;
    bse[2] = 4;
    world.install_battle_sfx_bank(&bse);
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 16];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[0x0C] = Some(cue_track_clip(0x0C, &[(0, 0xA7), (3, 0x1A)]));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.queued_anim = 0x0C;
    world.commit_staged_battle_anim(0);
    assert_eq!(world.actors[0].battle_anim_cue_cursor, 0);

    let mut ops = Vec::new();
    for _ in 0..12 {
        world.tick_battle_animations();
        // The clip commits along the way raise the sustained teardown
        // (`FUN_80017910`), which is not a cue of the track.
        ops.extend(
            world
                .take_sfx_ring_ops()
                .into_iter()
                .filter(|op| *op != SfxRingOp::ReleaseSustained),
        );
    }
    assert_eq!(
        ops,
        vec![
            SfxRingOp::PushUnlessSustained {
                id: 0x243,
                key: 0x243
            },
            SfxRingOp::PushUnlessSustained {
                id: 0x19,
                key: 0x19
            }
        ],
        "each cue once, in track order"
    );
    assert_eq!(world.actors[0].battle_anim_cue_cursor, 2);
    let row = world
        .runtime_sfx_descriptor(0x243)
        .expect("battle bank row");
    assert_eq!(row[4], 2, "the party literal category");
}

/// A character's cry on its cue track (`0xD7` / `0xE7` / `0xF7`) counts once on
/// the seat's record `+0x98`, the records page's Knockouts word, whether or not
/// the cry is muted: `FUN_800508DC` bumps it ahead of both gates.
#[test]
fn a_cry_on_the_cue_track_counts_a_knockout() {
    for muted in [false, true] {
        let mut world = World::new();
        world.load_party(legaia_save::Party::zeroed(4));
        world.enter_battle(1, 1);
        if muted {
            let rec = &mut world.party.roster.members[0];
            let mut bits = rec.ability_bits();
            bits[5] |= 0x20;
            rec.set_ability_bits(bits);
        }
        let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 16];
        clips[0] = Some(pose_test_clip(0, 2, 0));
        clips[0x0C] = Some(cue_track_clip(0x0C, &[(0, 0xE7), (2, 0x1A), (4, 0xD7)]));
        world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
        world.actors[0].battle.queued_anim = 0x0C;
        world.commit_staged_battle_anim(0);
        for _ in 0..12 {
            world.tick_battle_animations();
        }
        assert_eq!(
            world.party.roster.members[0].battle_records().knockouts,
            2,
            "muted {muted}"
        );
    }
}

#[test]
fn the_art_name_label_closes_when_the_art_clip_hands_off() {
    // FUN_8004AD80: an art constant's commit opens the label; the commit
    // that replaces that clip on slot 0x11 (attack command, latched id not
    // the SpecialStarter) destroys it, and the next art reopens it.
    let mut world = staged_anim_test_world();
    world.actors[0].battle.action_category =
        legaia_engine_battle_vm::battle_action::ActionCategory::Attack.as_byte();
    world.battle.move_label_closed = true;
    world.actors[0].battle.queued_anim = 0x1B;
    world.commit_staged_battle_anim_at_boundary(0);
    assert_eq!(world.actors[0].battle.current_anim, 0x11);
    assert!(!world.battle.move_label_closed, "the art opens its label");
    // The art clip ends on a plain swing: the label comes down.
    world.actors[0].battle.queued_anim = 0x0E;
    world.commit_staged_battle_anim_at_boundary(0);
    assert!(
        world.battle.move_label_closed,
        "the 0x11 hand-off closes it"
    );
    // A swing handing off to idle leaves it as it is.
    world.battle.move_label_closed = false;
    world.actors[0].battle.queued_anim = 0;
    world.commit_staged_battle_anim_at_boundary(0);
    assert!(!world.battle.move_label_closed, "only slot 0x11 closes it");
}

// --- the summon creature's clip loop counter `+0x21B` -----------------------

/// A one-part clip whose entry head parks on `park` with the count `0xFF`.
fn parked_clip(action_id: u8, frames: usize, park: u8) -> MonsterAnimation {
    let mut clip = pose_test_clip(action_id, frames, 0);
    let mut head = vec![0u8; legaia_actor_formats::monster_archive::EFFECT_SCRIPT_HEAD_BYTES];
    head[0x84] = 0xFF;
    head[0x85] = park;
    head[0x86] = park;
    clip.effect_script = head;
    clip
}

/// The commit seeds the creature's `+0x21B` with the entry's count and the
/// anim tick's park takes it off `0xFF` once the cursor reaches the park
/// frame - what PROT 0904's arm 7 and PROT 0907's arm 10 hold on. A seat
/// that is not the creature keeps its byte.
#[test]
fn the_creature_loop_counter_follows_its_clip_window() {
    let run = |creature: bool| -> Vec<u8> {
        let mut world = World::new();
        let slot = 1usize;
        world.mode = SceneMode::Battle;
        world.actors[slot].active = true;
        world.actors[slot].battle.hp = 100;
        if creature {
            world.casting.summon_actor_slot = Some(slot as u8);
        }
        let clips = vec![Some(pose_test_clip(0, 4, 0)), Some(parked_clip(1, 10, 3))];
        world.set_actor_battle_action_clips(slot, std::sync::Arc::new(clips));
        world.apply_battle_pose(
            slot,
            legaia_engine_battle_vm::battle_action::Pose::Idle as u8,
        );
        world.actors[slot].battle.queued_anim = 1;
        world.actors[slot]
            .battle
            .flag_bits
            .set(legaia_engine_battle_vm::battle_action::ActorFlags::WINDUP_DONE);
        world.commit_staged_battle_anim(slot);
        assert_eq!(world.actors[slot].battle.current_anim, 1);
        // A keyframe a tick (the normal rate doubles the base step).
        world.actors[slot].battle_animation.as_mut().unwrap().step = 128;
        let mut seen = vec![world.actors[slot].battle.hit_count_bound];
        for _ in 0..10 {
            world.tick_battle_animations();
            seen.push(world.actors[slot].battle.hit_count_bound);
        }
        seen
    };
    let creature = run(true);
    assert_eq!(creature[0], 0xFF, "the commit seeds the count");
    let left = creature
        .iter()
        .position(|&c| c != 0xFF)
        .expect("the park took the counter off 0xFF");
    // Frame 3 is reached on tick 3 with no overshoot; tick 4 spends a
    // keyframe of the budget, and every later tick another.
    assert_eq!(
        left, 4,
        "held until the cursor passed frame 3 ({creature:?})"
    );
    assert_eq!(
        &creature[left..],
        &[0xFE, 0xFD, 0xFC, 0xFB, 0xFA, 0xF9, 0xF8]
    );
    assert!(
        run(false).iter().all(|&c| c == 0),
        "a non-creature seat is untouched"
    );
}

/// A party member seated at zero HP kneels on entry `8` - the capture's
/// `+0x1D9 = +0x1DA = 8` - without the knockdown chain that leads there; a
/// living member and a monster are left alone.
#[test]
fn a_member_seated_down_kneels_on_entry_8() {
    let mut world = pose_test_world();
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 22];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[8] = Some(pose_test_clip(8, 12, 0));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.hp = 5;
    assert!(
        !world.seat_downed_kneel(0),
        "a living member keeps its stance"
    );
    world.actors[0].battle.hp = 0;
    assert!(world.seat_downed_kneel(0));
    assert_eq!(world.actors[0].battle_reaction, Some(8));
    assert!(world.seat_downed_kneel(0), "already kneeling");
}

/// `FUN_8004AD80` `0x8004AFC0..0x8004B05C`: a flinch re-committed onto
/// itself while its juggle byte `+0x1F7` is up seeds the cue cursor `+0x1F6`
/// at `1`, so the restarted clip does not fire its first cue again; a first
/// commit, or one after the window has dropped, starts at `0`.
#[test]
fn a_juggled_flinch_recommit_seeds_the_cue_cursor_past_its_first_cue() {
    let mut world = pose_test_world();
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 22];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[2] = Some(pose_test_clip(2, 12, 0));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.hp = 50;
    world.actors[0].battle_juggle_window = true;
    assert!(world.commit_battle_reaction_entry(0, 2));
    assert_eq!(world.actors[0].battle_anim_cue_cursor, 0, "first commit");
    assert!(world.commit_battle_reaction_entry(0, 2));
    assert_eq!(world.actors[0].battle_anim_cue_cursor, 1, "juggled re-hit");
    world.actors[0].battle_juggle_window = false;
    assert!(world.commit_battle_reaction_entry(0, 2));
    assert_eq!(world.actors[0].battle_anim_cue_cursor, 0, "window down");
}

/// The commit's rate decay (`0x8004B080..0x8004B090`) runs for a reaction
/// install too: a body slowed below normal when its reaction commits plays
/// it at half speed (PROT 0912's damage arm drops the victim to `2` beside
/// the knockdown it stages; retail's RAM holds `4` through the knockdown),
/// and a normal-speed body keeps its rate.
#[test]
fn a_reaction_commit_decays_a_slowed_rate_to_half() {
    use legaia_engine_battle_vm::battle_anim_rate::{AnimRate, RATE_HALF, RATE_NORMAL};
    let mut world = pose_test_world();
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 22];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[4] = Some(pose_test_clip(4, 12, 0));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.hp = 50;
    world.actors[0].battle.anim_rate = AnimRate(2);
    assert!(world.commit_battle_reaction_entry(0, 4));
    assert_eq!(world.actors[0].battle.anim_rate.get(), RATE_HALF);
    world.actors[0].battle.anim_rate = AnimRate(RATE_NORMAL);
    assert!(world.commit_battle_reaction_entry(0, 4));
    assert_eq!(world.actors[0].battle.anim_rate.get(), RATE_NORMAL);
}

/// A re-commit of the clip already playing passes the same decay
/// (`0x8004B080` sits ahead of the install arms): a seat the installer left at
/// `2` plays on at `4`.
#[test]
fn a_re_commit_decays_a_slowed_rate_to_half() {
    use legaia_engine_battle_vm::battle_anim_rate::{AnimRate, RATE_HALF};
    let mut world = pose_test_world();
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 22];
    clips[0] = Some(pose_test_clip(0, 12, 0));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.hp = 50;
    world.actors[0].battle.anim_rate = AnimRate(2);
    world.actors[0].battle.queued_anim = world.actors[0].battle.current_anim;
    world.commit_staged_battle_anim_at_boundary(0);
    assert_eq!(world.actors[0].battle.anim_rate.get(), RATE_HALF);
}

/// A struck body's reaction - a one-shot on the reaction channel - takes the
/// capture's cursor when the capture names that entry.
#[test]
fn a_reaction_entry_takes_the_capture_cursor() {
    let mut world = pose_test_world();
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 22];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[4] = Some(pose_test_clip(4, 12, 0));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.hp = 50;
    assert!(world.commit_battle_reaction_entry(0, 4));
    assert!(!world.seat_battle_clip_cursor(0, 2, 40), "another entry");
    assert!(world.seat_battle_clip_cursor(0, 4, 40));
    let cursor = world.actors[0]
        .battle_animation
        .as_ref()
        .map(|p| p.cursor_sixteenths());
    assert_eq!(cursor, Some(40));
}

/// A cast film's opening seat lands the acting body's one-shot clip, which a
/// still's clip seed leaves on the replay's clock, but only within a pass of
/// where the replay stands: a state saved mid draw tick can hold an
/// after-image's older cursor in the node word.
#[test]
fn a_film_seat_lands_a_one_shot_within_a_pass() {
    let mut world = pose_test_world();
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 22];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[9] = Some(pose_test_clip(9, 12, 0));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.hp = 50;
    world.actors[0].battle.queued_anim = 9;
    world.commit_staged_battle_anim_at_boundary(0);
    assert_eq!(world.actors[0].battle.current_anim, 9);
    let cursor = |w: &World| {
        w.actors[0]
            .battle_animation
            .as_ref()
            .map(|p| i32::from(p.cursor_sixteenths()))
    };
    let start = cursor(&world).expect("a playing clip");
    assert!(
        !world.seat_battle_clip_cursor(0, 9, start + 8),
        "a still's seed leaves a one-shot"
    );
    assert!(world.seat_acting_clip_cursor(0, 9, start + 8));
    assert_eq!(cursor(&world), Some(start + 8));
    assert!(
        !world.seat_acting_clip_cursor(0, 9, start + 8 + 32),
        "past a pass's reach"
    );
    assert_eq!(cursor(&world), Some(start + 8));
}

/// `FUN_80047430` skips the whole advance for a body on its idle whose
/// status word carries Stone (`0x4`) or Numb (`0x400`) (`andi v0,a0,0x404`
/// at `0x80047640`, the clip test at `0x8004764C`, `j 0x800480B0`): a
/// petrified member holds the keyframe it stood on, where a healthy one's
/// idle keeps swaying.
#[test]
fn a_stone_or_numb_body_on_its_idle_holds_its_keyframe() {
    use legaia_engine_battle_vm::status_effects::StatusKind;
    let run = |status: Option<StatusKind>| {
        let mut world = World::new();
        world.actors[0].active = true;
        let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 22];
        clips[0] = Some(pose_test_clip(0, 60, 0));
        world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
        world.apply_battle_pose(0, legaia_engine_battle_vm::battle_action::Pose::Idle as u8);
        if let Some(kind) = status {
            world.battle.status_effects.apply(0, kind);
        }
        let frame = |w: &World| {
            w.actors[0]
                .battle_animation
                .as_ref()
                .map(|p| p.current_frame())
        };
        let before = frame(&world);
        for _ in 0..8 {
            world.tick_battle_animations();
        }
        (before, frame(&world))
    };
    let (before, after) = run(None);
    assert!(before.is_some());
    assert_ne!(before, after, "a healthy idle advances");
    for kind in [StatusKind::Stone, StatusKind::Numb] {
        let (before, after) = run(Some(kind));
        assert_eq!(before, after, "{kind:?} freezes the idle");
    }
}

// --- the anim tick's Rage arm (`FUN_80047430` `0x80047F10..0x80047F98`) ----

fn rage_world(rage: bool) -> World {
    let mut w = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    w.mode = SceneMode::Battle;
    w.load_party(legaia_save::Party::zeroed(1));
    if let Some(m) = w.party.roster.members.get_mut(0) {
        let mut bits = m.ability_bits();
        let word1 = if rage { 0x2000u32 } else { 0 };
        bits[4..8].copy_from_slice(&word1.to_le_bytes());
        m.set_ability_bits(bits);
    }
    w.actors[0].battle.liveness = 1;
    w.actors[0].battle.hp = 10;
    w.actors[0].battle.impact_state = 2;
    w
}

/// A Rage seat is stamped with the delegation group and the tint words on
/// every advancing tick, and its impact selector cleared; a seat without
/// the passive is left alone.
#[test]
fn a_rage_seat_takes_the_delegation_group_every_tick() {
    let mut w = rage_world(true);
    w.tick_battle_animations();
    let b = &w.actors[0].battle;
    assert_eq!(b.field_flags & 0x380, 0x380);
    assert_eq!((b.render_color, b.render_blend), (0x3FF0_0000, 0x1000));
    assert_eq!(b.impact_state, 0);
    // A landed hit strips the group (`andi 0xf07f`); the next tick restores it.
    w.actors[0].battle.field_flags &= 0xF07F;
    w.tick_battle_animations();
    assert_eq!(w.actors[0].battle.field_flags & 0x380, 0x380);

    let mut plain = rage_world(false);
    plain.tick_battle_animations();
    assert_eq!(plain.actors[0].battle.field_flags & 0x380, 0);
    assert_eq!(plain.actors[0].battle.impact_state, 2);
}

/// The arm stands down under a presentation arm (`+0x21C != 0`).
#[test]
fn a_rage_seat_under_a_presentation_arm_is_not_stamped() {
    let mut w = rage_world(true);
    w.actors[0].battle.render_flag = 0xFF;
    w.tick_battle_animations();
    assert_eq!(w.actors[0].battle.field_flags & 0x380, 0);
}

/// Both flee sites zero the walk record's cue track for every party slot
/// (`0x801E7D68`, PROT 0925's `0x801F7600`), so the run off stage is silent while
/// the walk into a strike keeps its steps.
#[test]
fn a_granted_flee_silences_the_party_walk_clip() {
    let mut world = pose_test_world();
    world.party.party_count = 1;
    let mut head = vec![0u8; 0x88];
    head[0x54..0x58].copy_from_slice(&[5, 0, 0x12, 0]);
    let mut walk = pose_test_clip(1, 4, 0);
    walk.effect_script = head.clone();
    let a = &mut world.actors[0];
    a.battle_action_clips = Some(std::sync::Arc::new(vec![None, Some(walk)]));
    a.battle.current_anim = 1;
    a.battle_effect_script = Some(head);
    world.stage_party_flee();
    let a = &world.actors[0];
    let bank = a.battle_action_clips.as_ref().unwrap()[1].as_ref().unwrap();
    assert!(bank.effect_script[0x54..0x74].iter().all(|&b| b == 0));
    let live = a.battle_effect_script.as_ref().unwrap();
    assert!(live[0x54..0x74].iter().all(|&b| b == 0));
}

/// The rate restore's monster gate reads the `+0x87` of the clip its seat sits
/// on (`FUN_801E93C8`, `0x801E9458..0x801E94A4`), so a solo clip holds the
/// latch `ctx[+0x243]` through the strike loop, and the idle after it opens.
#[test]
fn a_monster_on_a_solo_clip_keeps_the_restore_gate_shut() {
    use legaia_engine_battle_vm::battle_action::BattleActionHost;
    let mut world = pose_test_world();
    let mut clip = pose_test_clip(1, 4, 0);
    let mut head = vec![0u8; 0x88];
    head[0x87] = 1;
    clip.effect_script = head;
    world.actors[0].battle_action_clips = Some(std::sync::Arc::new(vec![
        Some(pose_test_clip(0, 4, 0)),
        Some(clip),
    ]));
    let host = crate::world::vm_hosts::BattleHostImpl { world: &mut world };
    assert_eq!(host.staged_art_record_flag(0, 1), 1);
    assert_eq!(host.staged_art_record_flag(0, 0), 0);
    assert_eq!(host.staged_art_record_flag(0, 2), 0);
}

/// Case `0x13` of `FUN_801DC0A0` raises the caster's blue arm and impact
/// selector once the clip passes cursor `0xC0` (`0x801DCE54..0x801DCE60`), and
/// clears both when `ctx[+0x24D]` goes up (`0x801DCE80..0x801DCE88`): Astral
/// Wave's charge.
#[test]
fn spell_cam_case_0x13_raises_the_casters_render_arm() {
    use crate::world::vm_hosts::BattleHostImpl;
    use legaia_engine_battle_vm::battle_action::BattleActionHost;
    let mut world = World::new();
    world.actors[0].active = true;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 16];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[0x0C] = Some(pose_test_clip(0x0C, 60, 0));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.queued_anim = 0x0C;
    world.commit_staged_battle_anim(0);
    BattleHostImpl { world: &mut world }.spell_anim_sustain(0, 0x13);
    assert_eq!(world.actors[0].battle.render_flag, 0, "below cursor 0xC0");
    while world.actors[0]
        .battle_animation
        .as_ref()
        .is_some_and(|p| p.cursor_sixteenths() < 0xC0)
    {
        world.tick_battle_animations();
    }
    BattleHostImpl { world: &mut world }.spell_anim_sustain(0, 0x13);
    assert_eq!(world.actors[0].battle.render_flag, 4);
    assert_eq!(world.actors[0].battle.impact_state, 2);
    world.battle_ctx.magic_recovery_gate = 1;
    BattleHostImpl { world: &mut world }.spell_anim_sustain(0, 0x13);
    assert_eq!(world.actors[0].battle.render_flag, 0);
    assert_eq!(world.actors[0].battle.impact_state, 0);
}

/// The capture band's two CD polls hold their states: `0x6E` walks the framing
/// and `0x6F` pulls `ctx[+0x6D0]` in on every pass of the wait.
#[test]
fn the_capture_band_polls_hold_for_the_read() {
    use crate::world::vm_hosts::BattleHostImpl;
    use legaia_engine_battle_vm::battle_action::{ActionState as S, BattleActionHost};
    let mut world = World::new();
    let start = world.clock.display_frames;
    world.battle_ctx.action_state = S::MagicCaptureBranch.as_byte();
    world.arm_capture_read_wait(S::MagicCaptureBranch.as_byte());
    assert!(world.capture_read_pending());
    world.clock.display_frames = start + 14;
    assert!(!world.capture_read_pending());
    world.battle_ctx.action_state = S::MagicCaptureFade.as_byte();
    world.arm_capture_read_wait(S::MagicCaptureFade.as_byte());
    assert!(!BattleHostImpl { world: &mut world }.previous_action_cleared(1));
    world.clock.display_frames = start + 18;
    assert!(BattleHostImpl { world: &mut world }.previous_action_cleared(1));
    // Other states ignore a stale word.
    world.battle.capture_read_until = u64::MAX;
    world.battle_ctx.action_state = S::MagicCapturePhase2.as_byte();
    assert!(!world.capture_read_pending());
}

/// `FUN_800520F0` stores no category on the summon seat's node, so its runtime
/// cues key record `0`; the party's keep the `7` of `0x80051548`.
#[test]
fn a_summon_creatures_runtime_cue_keys_record_zero() {
    let mut world = World::new();
    world.enter_battle(2, 1);
    assert_eq!(world.battle_sound_category(7), 0);
    assert_eq!(world.battle_sound_category(0), 7);
}

/// `FUN_8004AD80`'s monster arm (`0x8004AEBC..0x8004AF88`): a monster seat's
/// incoming clip whose entry `+0x77` is non-zero opens the move-name label on
/// that spell id; the commit that replaces it (the hand-off to idle included)
/// takes it down when the outgoing clip's byte is non-zero.
#[test]
fn a_monster_clip_names_its_move_until_it_hands_off() {
    let mut world = pose_test_world();
    world.actors[0].battle_monster_id = Some(49);
    let mut swing = pose_test_clip(5, 3, 0);
    swing.attach_key = 0x2C;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 8];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[4] = Some(pose_test_clip(4, 2, 0));
    clips[5] = Some(swing);
    world.actors[0].battle_action_clips = Some(std::sync::Arc::new(clips));
    world.actors[0].battle.queued_anim = 4;
    world.commit_staged_battle_anim_at_boundary(0);
    assert_eq!(
        world.battle.monster_move_label, None,
        "a clip with no +0x77"
    );
    world.actors[0].battle.queued_anim = 5;
    world.commit_staged_battle_anim_at_boundary(0);
    assert_eq!(world.battle.monster_move_label, Some(0x2C));
    world.actors[0].battle.queued_anim = 0;
    world.commit_staged_battle_anim_at_boundary(0);
    assert_eq!(world.battle.monster_move_label, None, "idle takes it down");
    // A party seat's clip never touches it (`sltiu v0,s3,0x3`).
    world.actors[0].battle_monster_id = None;
    world.actors[0].battle.queued_anim = 5;
    world.commit_staged_battle_anim_at_boundary(0);
    assert_eq!(world.battle.monster_move_label, None);
}

/// The burning-body emitter sits in the pose decode, which only the draw
/// callback reaches, and the draw tick `FUN_800480D8` skips the callback for
/// a body whose colour word is zero (`0x800481CC`). A drawn body with a
/// selector draws its emit points off `rand()`; the same body under a
/// module's hide (`+0x04 = 0`, `+0x21C = 0xFF`) sheds nothing and draws
/// nothing, however long it keeps the selector.
#[test]
fn a_hidden_body_sheds_no_impact_sprites() {
    let mut w = pose_test_world();
    w.mode = SceneMode::Battle;
    w.party.party_count = 1;
    w.apply_battle_pose(0, legaia_engine_battle_vm::battle_action::Pose::Idle as u8);
    w.tick_battle_animations();
    assert!(
        crate::world::battle::stream_pose(&w.actors[0]).is_some_and(|p| !p.is_empty()),
        "the body has a pose to shed from"
    );
    w.actors[0].battle.impact_state = 2;
    let before = w.rng_state;
    w.emit_battle_burn_sprites(0x10);
    assert_ne!(w.rng_state, before, "a drawn body draws its emit point");

    w.actors[0].battle.render_flag = 0xFF;
    w.actors[0].battle.render_color = 0;
    w.battle.burn_emit_accum = 0;
    let before = w.rng_state;
    w.emit_battle_burn_sprites(0x10);
    assert_eq!(w.rng_state, before, "a hidden body is not decoded");
    assert_eq!(w.actors[0].battle.impact_state, 2);
}

/// A monster's finished knockdown on zero HP credits the member holding the
/// action on its record's Monsters Defeated word (`+0x9C`), once: the death
/// arm's own `+0x1DC` bit 3 gates the credit on the passes after the first,
/// and a monster holding the action credits nobody.
#[test]
fn a_monster_death_credits_the_acting_member() {
    let mut world = World::new();
    world.load_party(legaia_save::Party::zeroed(4));
    world.enter_battle(2, 1);
    let monster = usize::from(world.party.party_count);
    let clips: Vec<Option<MonsterAnimation>> = vec![None; 16];
    world.set_actor_battle_action_clips(monster, std::sync::Arc::new(clips));
    world.actors[monster].battle_monster_id = Some(0x40);
    world.actors[monster].battle.hp = 0;
    world.battle_ctx.active_actor = 1;
    let rslot = world.party_roster_slot(1);
    for _ in 0..3 {
        world.finish_battle_reaction(monster, 4);
    }
    let defeated =
        |w: &World, r: usize| w.party.roster.members[r].battle_records().monsters_defeated;
    assert_eq!(defeated(&world, rslot), 1, "one credit per death");
    assert_eq!(defeated(&world, world.party_roster_slot(0)), 0);

    // A monster holding the action credits nobody.
    world.actors[monster].battle.flag_bits = legaia_engine_battle_vm::battle_action::ActorFlags(0);
    world.battle_ctx.active_actor = monster as u8;
    world.finish_battle_reaction(monster, 4);
    assert_eq!(defeated(&world, rslot), 1);
}

/// The combo readout folds a party Attack's landed string into the acting
/// member's Max Damage and Max Hits words, keeping the larger, and a monster's
/// string or a cast folds nothing (`FUN_801E805C`, `0x801E88D4..0x801E89D8`).
#[test]
fn a_landed_string_folds_into_the_records() {
    let mut world = World::new();
    world.load_party(legaia_save::Party::zeroed(4));
    world.enter_battle(2, 1);
    let monster = world.party.party_count;
    world.actors[usize::from(monster)].battle.hp = 1000;
    world.actors[usize::from(monster)].battle.max_hp = 1000;
    world.battle_ctx.active_actor = 1;
    world.actors[1].battle.action_category = 3;
    world.battle_ctx.readout_pushes = 4;
    world.actors[usize::from(monster)].battle.damage_accum = 321;
    world.apply_combo_total(monster);
    world.fold_combo_records();
    let rslot = world.party_roster_slot(1);
    let r = world.party.roster.members[rslot].battle_records();
    assert_eq!((r.max_damage, r.max_hits), (321, 4));

    // A smaller string keeps the record.
    world.battle_ctx.readout_pushes = 2;
    world.actors[usize::from(monster)].battle.damage_accum = 50;
    world.apply_combo_total(monster);
    world.fold_combo_records();
    let r = world.party.roster.members[rslot].battle_records();
    assert_eq!((r.max_damage, r.max_hits), (321, 4));

    // A cast's total folds nothing.
    world.actors[1].battle.action_category = 2;
    world.battle_ctx.readout_pushes = 9;
    world.actors[usize::from(monster)].battle.damage_accum = 900;
    world.apply_combo_total(monster);
    world.fold_combo_records();
    let r = world.party.roster.members[rslot].battle_records();
    assert_eq!((r.max_damage, r.max_hits), (321, 4));
}

// --- the root-motion term reads the entry the seat's own tick held ---------

/// A four-frame one-shot whose entry head carries a root speed (`+0x0C`).
fn root_speed_clip(action_id: u8, speed: i16) -> MonsterAnimation {
    let mut clip = pose_test_clip(action_id, 4, 0);
    let mut head = vec![0u8; legaia_actor_formats::monster_archive::EFFECT_SCRIPT_HEAD_BYTES];
    head[0x0C..0x0E].copy_from_slice(&speed.to_le_bytes());
    clip.effect_script = head;
    clip
}

/// A reaction a later seat's hit walk commits on a seat that has already
/// ticked this pass is only staged there in retail (`+0x1DA` with
/// `+0x1DC |= 5`, `0x801EEC6C..0x801EEC84`): the seat's tick stepped the root
/// motion of the entry it held (`0x80047D34`), and the reaction's backstep
/// starts on its next tick.
#[test]
fn a_reaction_staged_after_the_seats_slice_slides_from_its_next_tick() {
    let mut world = World::new();
    world.enter_battle(1, 1);
    assert!(world.toggles.battle_sm_frame_cadence);
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 12];
    clips[0] = Some(pose_test_clip(0, 4, 0));
    clips[11] = Some(root_speed_clip(11, -4));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.apply_battle_pose(0, legaia_engine_battle_vm::battle_action::Pose::Idle as u8);
    world.battle.anim_pass_vsyncs = 3;
    let pair = |w: &World| {
        (
            w.actors[0].move_state.world_x,
            w.actors[0].move_state.world_z,
        )
    };
    // The seat's own slice: on idle, which carries no speed.
    world.tick_battle_animations_walk(true);
    let stood = pair(&world);
    // The attacker's slice lands its hit and commits the block.
    assert!(world.commit_battle_reaction_entry(0, 11));
    world.tick_battle_locomotion_dt(3);
    assert_eq!(pair(&world), stood, "the hit's own pass steps the idle");
    // The next pass's slice holds the block.
    world.tick_battle_animations_walk(true);
    world.tick_battle_locomotion_dt(3);
    assert_ne!(pair(&world), stood, "the block slides from the next tick");
}

/// A clip a seat's own slice commits behind its advance - here the natural
/// end of a one-shot swing installing the swing staged behind it - is drawn
/// on its first keyframe on that pass: `FUN_8004AD80` zeroes the cursor
/// (`0x8004B068`) and the draw decodes the new entry there, so the body pair
/// the next pass's range law and separation read comes off the new swing's
/// keyframe 0, not the outgoing swing's last pose
/// (`battle_ropeway_party_gala_flying_knee_mid`: Gala's pair on clip `0x0F`
/// at cursor `0`).
#[test]
fn an_attackers_in_slice_commit_shows_the_new_clips_first_keyframe() {
    let mut world = pose_test_world();
    world.mode = SceneMode::Battle;
    world.toggles.battle_sm_frame_cadence = true;
    let swing = pose_test_clip(8, 2, 100);
    world.actors[0].battle_animation =
        legaia_engine_battle::battle_anim::MonsterAnimPlayer::new_one_shot(&swing);
    world.actors[0].battle.current_anim = 8;
    world.actors[0].battle.queued_anim = 9;
    world.actors[0].battle_animation.as_mut().unwrap().step = 1024;
    let commits = world.actors[0].battle_clip_commits;
    world.battle.anim_pass_vsyncs = 1;
    world.tick_battle_anim_pass();
    assert_ne!(
        world.actors[0].battle_clip_commits, commits,
        "the slice committed the staged swing"
    );
    assert_eq!(world.actors[0].battle.current_anim, 9);
    let pose = world.battle_stream_pose_translations(0).unwrap();
    assert_eq!(pose[0][0], 200, "the new swing's keyframe 0, not 101");
    let front = world.actors[0].battle_pose_history.front().unwrap();
    assert_eq!(front.frame, 0);
}
