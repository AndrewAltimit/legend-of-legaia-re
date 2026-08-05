use super::*;

// --- battle pose -> action-clip switching -----------------------------------

/// Synthetic one-part action clip: `frames` keyframes translating from `tx`.
fn pose_test_clip(action_id: u8, frames: usize, tx: i16) -> MonsterAnimation {
    use legaia_asset::monster_archive::PartPose;
    MonsterAnimation {
        action_id,
        rate: 2,
        attach_key: 0,
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
fn battle_pose_plays_action_clip_then_restores_idle() {
    let mut world = pose_test_world();
    world.apply_battle_pose(0, vm::battle_action::Pose::Recover as u8);
    assert_eq!(world.actors[0].battle_pose, Some(8));
    // One-shot: run the 2-frame clip to its end in one tick.
    world.actors[0].battle_animation.as_mut().unwrap().step = 1024;
    world.tick_battle_animations();
    assert!(
        world.actors[0]
            .battle_animation
            .as_ref()
            .unwrap()
            .finished(),
        "recover clip is a one-shot"
    );
    // The next tick falls back to the idle loop (slot 0).
    world.tick_battle_animations();
    assert_eq!(
        world.actors[0].battle_pose,
        Some(vm::battle_action::Pose::Idle as u8)
    );
    assert!(
        !world.actors[0]
            .battle_animation
            .as_ref()
            .unwrap()
            .finished(),
        "idle loops"
    );
}

#[test]
fn battle_pose_defeat_holds_final_frame() {
    let mut world = pose_test_world();
    world.apply_battle_pose(0, vm::battle_action::Pose::Defeat as u8);
    world.actors[0].battle_animation.as_mut().unwrap().step = 1024;
    for _ in 0..5 {
        world.tick_battle_animations();
    }
    // Defeat never falls back to idle: the downed pose holds.
    assert_eq!(
        world.actors[0].battle_pose,
        Some(vm::battle_action::Pose::Defeat as u8)
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
    // Slot 7 (ready) is empty in the installed set: the request binds the
    // idle loop instead and records the pose so the SM isn't retried.
    world.apply_battle_pose(0, vm::battle_action::Pose::Ready as u8);
    assert_eq!(
        world.actors[0].battle_pose,
        Some(vm::battle_action::Pose::Ready as u8)
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
    world.apply_battle_pose(0, vm::battle_action::Pose::Recover as u8);
    world.actors[0].battle_animation.as_mut().unwrap().step = 7;
    world.tick_battle_animations();
    let phase_frame = world.actors[0].pose_frame.clone();
    // Re-requesting the same pose must not rewind the clip.
    world.apply_battle_pose(0, vm::battle_action::Pose::Recover as u8);
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
    world.apply_battle_pose(0, vm::battle_action::Pose::Recover as u8);
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
        Some(vm::battle_action::Pose::Idle as u8),
        "reaction chain ends in the idle loop"
    );
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
        Some(vm::battle_action::Pose::Idle as u8)
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
    world.apply_battle_pose(0, vm::battle_action::Pose::Idle as u8);
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
    world.apply_battle_pose(3, vm::battle_action::Pose::Defeat as u8);
    assert_eq!(world.actors[3].battle_pose, None, "non-idle pose ignored");
    world.apply_battle_pose(3, vm::battle_action::Pose::Idle as u8);
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
    use vm::battle_action::ActorFlags;
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
    use vm::battle_action::ActorFlags;
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
    world.apply_battle_pose(0, vm::battle_action::Pose::Idle as u8);
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
        Some(vm::battle_action::Pose::Idle as u8),
        "idle loop resumes after the band"
    );
    assert!(!a.battle_animation.as_ref().unwrap().finished());
}

#[test]
fn attack_chain_paces_strikes_by_staged_clip_completion() {
    use vm::battle_action::{ActionState, StepOutcome};
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

    // While the swing is in flight the chain holds (the 0x801E370C gate).
    assert_eq!(world.step_battle(), StepOutcome::Stay);
    assert_eq!(world.actors[0].battle.strike_index, 1, "no byte read");

    // Finish the swing: the gate opens, the next step reads 0x0D.
    world.actors[0].battle_animation.as_mut().unwrap().step = 4096;
    world.tick_battle_animations();
    world.tick_battle_animations();
    assert!(world.actors[0].battle_staged_anim.is_none());
    assert_eq!(world.step_battle(), StepOutcome::Stay);
    world.tick_battle_animations();
    assert_eq!(world.actors[0].battle_staged_anim, Some(0x0D));
    assert_eq!(world.actors[0].battle.strike_index, 2);

    // Finish the second swing; the terminator exits the band.
    world.actors[0].battle_animation.as_mut().unwrap().step = 4096;
    world.tick_battle_animations();
    world.tick_battle_animations();
    let out = world.step_battle();
    assert!(
        matches!(out, StepOutcome::Transition { to, .. }
            if to == ActionState::AttackRecovery.as_byte()),
        "terminator -> recovery, got {out:?}"
    );
}

// --- effect-script walk (FUN_801DEA50 via the animation tick) ---------------

/// A clip whose entry head carries one effect-script record at cursor 0:
/// `[frame_gate, effect, x, y, z]` at `+0x14` (RECORD_BASE).
fn scripted_clip(action_id: u8, effect: (u8, u8, i16, i16, i16)) -> MonsterAnimation {
    use crate::action_effect_script::{RECORD_BASE, RECORD_STRIDE};
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
    world.actors[0].battle.queued_anim = 0;
    world.commit_staged_battle_anim(0);
    assert_eq!(world.actors[0].battle_effect_cursor, 0);
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
    world.apply_battle_pose(0, vm::battle_action::Pose::Idle as u8);
    assert!(world.actors[0].battle_effect_script.is_none());
    world.tick_battle_animations();
    assert!(world.drain_battle_effect_spawns().is_empty());
}

// --- arts slow-motion + after-image ghosts ----------------------------------

/// The SpecialStarter (`0x1A`) commit fires the retail freeze arm
/// (`FUN_8004AD80` `0x8004B728..0x8004B750`): every slot frozen, the acting
/// actor at quarter speed - and its dash frames are ghost-eligible (the
/// dynamic art slot `0x11` gate of `FUN_80049348`).
#[test]
fn special_starter_commit_freezes_the_battle_and_ghosts_the_dash() {
    use vm::battle_anim_rate::{RATE_FROZEN, RATE_QUARTER};
    let mut world = pose_test_world();
    world.actors[1].active = true;
    let mut bank: Vec<Option<MonsterAnimation>> = vec![None; 16];
    bank[0xA] = Some(pose_test_clip(0x1A, 60, 0));
    world.set_actor_battle_art_bank(0, std::sync::Arc::new(bank));
    world.actors[0].battle.queued_anim = 0x1A;
    world.commit_staged_battle_anim(0);
    assert_eq!(world.actors[0].battle.anim_rate.get(), RATE_QUARTER);
    assert_eq!(world.actors[1].battle.anim_rate.get(), RATE_FROZEN);
    // The commit materialised the starter at dynamic slot B (0x11).
    assert_eq!(
        world.actors[0].battle.current_anim,
        vm::anim_vm::DYNAMIC_ART_SLOT_B
    );
    for _ in 0..10 {
        world.tick_battle_animations();
    }
    let ghosts = world.battle_ghost_draws();
    assert_eq!(ghosts.len(), 2, "two ghosts per retail walk");
    // Quarter speed: step = 8/2 = 4, ghosts trail 4 and 8 frames.
    // Colour: Vahn's red base, the second ghost one decay dimmer.
    assert_eq!(ghosts[0].color, [0x60, 0x30, 0x30]);
    assert_eq!(ghosts[1].color, [0x50, 0x20, 0x20]);
}

/// An art action constant (`>= 0x1B`) drops the whole battle to half speed
/// (`0x8004BB78..0x8004BBA8`, `ctx[+0x243]` clear), and the frozen actor's
/// pose stops advancing while a normal-rate actor's keeps moving.
#[test]
fn art_constant_commit_slows_everyone_and_rate_zero_freezes_the_pose() {
    use vm::battle_anim_rate::{AnimRate, RATE_HALF};
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

/// Basic-attack swings (`0x0C..=0x0F`) fire no rate arm: the battle stays
/// at normal speed and no frame is ghost-eligible.
#[test]
fn direction_swing_commit_keeps_normal_speed_and_no_ghosts() {
    use vm::battle_anim_rate::RATE_NORMAL;
    let mut world = pose_test_world();
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 22];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[0x0C] = Some(pose_test_clip(0x0C, 60, 0));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.queued_anim = 0x0C;
    world.commit_staged_battle_anim(0);
    assert_eq!(world.actors[0].battle.anim_rate.get(), RATE_NORMAL);
    for _ in 0..10 {
        world.tick_battle_animations();
    }
    assert!(world.battle_ghost_draws().is_empty());
}
