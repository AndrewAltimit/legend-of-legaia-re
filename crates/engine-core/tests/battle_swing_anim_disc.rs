//! Disc-gated: the battle party **plays its weapon-swing and art
//! animations** when the action SM walks the attack band.
//!
//! Builds Vahn's real battle clips the way the engine's battle entry does:
//! record[0] action slots, the equipment-spliced swings (runtime slots
//! `0xC..0xF`, `legaia_asset::battle_char_assembly::swing_battle_animations`)
//! and the art-animation bank resolved through the `readef.DAT` `"ME"`
//! archives (`art_animation_bank` / `art_me_archive` / `art_animation`).
//! Installs them on a party actor, then drives a scripted attack
//! (`params = [0x0C, 0x0D, 0x00]`) through `World::step_battle` +
//! `World::tick_battle_animations`, asserting:
//!
//! - the actor's pose during the swing window **differs from the idle
//!   pose** (the staged equipment swing actually deforms the skeleton);
//! - the chain holds in `AttackChain` while each swing plays (the
//!   `0x801E370C` `ADVANCE_DONE` read gate) and exits to recovery on the
//!   terminator;
//! - after the band the id pair converges back to idle `0` and the idle
//!   loop resumes;
//! - a staged art id (`0x1A`) commits through the bank with the retail
//!   rewrite to dynamic slot `0x11` and plays its ME-archive stream.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset or `extracted/PROT` is
//! missing (disc-gated convention).

use std::path::PathBuf;

use legaia_asset::battle_char_assembly;
use legaia_asset::monster_archive::MonsterAnimation;
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::battle_action::{ActionState, ActorFlags, StepOutcome};

fn extracted_prot_dir() -> Option<PathBuf> {
    [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ]
    .into_iter()
    .find(|p| p.is_dir())
}

fn gated_inputs() -> Option<(Vec<u8>, Vec<u8>)> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let Some(prot) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return None;
    };
    let player = std::fs::read(prot.join("0863_edstati3.BIN")).ok()?;
    let readef = std::fs::read(prot.join("0894_card_data.BIN")).ok()?;
    Some((player, readef))
}

/// Vahn's commit-ready battle clips + art bank, built the way the engine's
/// battle entry builds them (default / unequipped sections).
fn vahn_clips_and_bank(
    raw: &[u8],
    readef: &[u8],
) -> (
    Vec<Option<MonsterAnimation>>,
    Vec<Option<MonsterAnimation>>,
    MonsterAnimation,
) {
    let pack = legaia_asset::battle_data_pack::parse(raw).expect("player-file pack");
    let equipped = [0u8; 5];
    let asm = battle_char_assembly::assemble_character(raw, &pack, &equipped).expect("assembly");

    let mut clips: Vec<Option<MonsterAnimation>> =
        vec![None; battle_char_assembly::ACTION_SLOT_COUNT];
    for a in battle_char_assembly::battle_animations(raw).expect("record[0] action streams") {
        if let Some(slot) = clips.get_mut(a.action_id as usize) {
            *slot = Some(battle_char_assembly::expand_animation_for_objects(
                &a,
                &asm.anm_bones,
            ));
        }
    }
    for s in battle_char_assembly::swing_battle_animations(raw, &pack, &equipped).expect("swings") {
        clips[s.slot as usize] = Some(battle_char_assembly::expand_animation_for_objects(
            &s.anim,
            &asm.anm_bones,
        ));
    }

    let record0 = battle_char_assembly::decode_record0(raw).expect("record[0]");
    let records = battle_char_assembly::art_animation_bank(&record0).expect("art bank");
    let main = battle_char_assembly::art_me_archive(readef, 0, false).expect("main ME archive");
    let base = battle_char_assembly::art_me_archive(readef, 0, true).expect("base ME archive");
    let mut bank: Vec<Option<MonsterAnimation>> = vec![None; records.len()];
    for rec in &records {
        let archive = if rec.uses_base_archive() {
            &base
        } else {
            &main
        };
        let anim = battle_char_assembly::art_animation(rec, archive)
            .unwrap_or_else(|e| panic!("art record {} stream: {e:#}", rec.index));
        bank[rec.index] = Some(battle_char_assembly::expand_animation_for_objects(
            &anim,
            &asm.anm_bones,
        ));
    }

    let idle = clips[0].clone().expect("idle clip (slot 0)");
    (clips, bank, idle)
}

fn pose_outputs(world: &World, slot: usize) -> Option<Vec<([i16; 3], [i16; 3])>> {
    world.actors[slot]
        .pose_frame
        .as_ref()
        .map(|f| f.bone_outputs.clone())
}

/// `true` when any bone's translation/rotation differs from the idle rest
/// pose (idle clip frame 0).
fn differs_from(pose: &[([i16; 3], [i16; 3])], rest: &[([i16; 3], [i16; 3])]) -> bool {
    pose.iter().zip(rest.iter()).any(|(a, b)| a != b)
}

#[test]
fn scripted_party_attack_plays_swings_then_returns_to_idle() {
    let Some((raw, readef)) = gated_inputs() else {
        return;
    };
    let (clips, bank, idle) = vahn_clips_and_bank(&raw, &readef);
    assert!(
        (0xC..=0xF).all(|s| clips[s].is_some()),
        "all four equipment swings decoded"
    );
    let rest: Vec<([i16; 3], [i16; 3])> = idle.frames[0]
        .iter()
        .map(|p| ([p.tx, p.ty, p.tz], [p.rx as i16, p.ry as i16, p.rz as i16]))
        .collect();

    let mut world = World::new();
    world.mode = SceneMode::Battle;
    world.actors[0].active = true;
    world.actors[0].battle.liveness = 1;
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.max_hp = 100;
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.set_actor_battle_art_bank(0, std::sync::Arc::new(bank));
    // Idle loop running, exactly like battle entry.
    world.apply_battle_pose(0, legaia_engine_vm::battle_action::Pose::Idle as u8);

    // Scripted two-swing attack: L (0x0C), R (0x0D), terminator.
    world.actors[0].battle.params[0] = 0x0C;
    world.actors[0].battle.params[1] = 0x0D;
    world.actors[0].battle.params[2] = 0x00;
    world.battle_ctx.active_actor = 0;
    world.battle_ctx.action_state = ActionState::AttackChain.as_byte();

    let mut swing_frames = 0usize;
    let mut swing_pose_moved = false;
    let mut reached_recovery = false;
    for _ in 0..5000 {
        let out = world.step_battle();
        world.tick_battle_animations();
        if world.actors[0].battle_staged_anim.is_some() {
            swing_frames += 1;
            if let Some(pose) = pose_outputs(&world, 0)
                && differs_from(&pose, &rest)
            {
                swing_pose_moved = true;
            }
        }
        if matches!(out, StepOutcome::Transition { to, .. }
            if to == ActionState::AttackRecovery.as_byte())
        {
            reached_recovery = true;
            break;
        }
    }
    assert!(reached_recovery, "attack band must reach recovery");
    assert!(
        swing_frames > 2,
        "the chain holds while each staged swing plays (saw {swing_frames} in-flight frames)"
    );
    assert!(
        swing_pose_moved,
        "the pose during the swing window differs from the idle rest pose"
    );

    // After the band: staged marker gone, gate open, ids converged to idle.
    let a = &world.actors[0];
    assert!(a.battle_staged_anim.is_none());
    assert!(!a.battle.flag_bits.has(ActorFlags::ADVANCE_DONE));
    assert_eq!(a.battle.queued_anim, 0, "id pair back at idle");
    assert_eq!(a.battle.current_anim, 0);
    // Recovery poses slot 8 (a one-shot); once it finishes the idle loop
    // resumes. Tick it through.
    for _ in 0..5000 {
        world.tick_battle_animations();
        if world.actors[0].battle_pose == Some(legaia_engine_vm::battle_action::Pose::Idle as u8) {
            break;
        }
    }
    assert_eq!(
        world.actors[0].battle_pose,
        Some(legaia_engine_vm::battle_action::Pose::Idle as u8),
        "idle resumes after the band"
    );
    assert!(
        !world.actors[0]
            .battle_animation
            .as_ref()
            .unwrap()
            .finished(),
        "idle player loops"
    );
}

#[test]
fn staged_art_id_commits_through_the_me_archive_bank() {
    let Some((raw, readef)) = gated_inputs() else {
        return;
    };
    let (clips, bank, idle) = vahn_clips_and_bank(&raw, &readef);
    assert!(bank.len() >= 0xB, "Vahn's bank covers id 0x1A (record 10)");
    let rest: Vec<([i16; 3], [i16; 3])> = idle.frames[0]
        .iter()
        .map(|p| ([p.tx, p.ty, p.tz], [p.rx as i16, p.ry as i16, p.rz as i16]))
        .collect();

    let mut world = World::new();
    world.mode = SceneMode::Battle;
    world.actors[0].active = true;
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.set_actor_battle_art_bank(0, std::sync::Arc::new(bank));

    // Stage art id 0x1A: bank record 10, dynamic slot 0x11 (the retail
    // FUN_8004AD80 rewrite).
    world.actors[0].battle.queued_anim = 0x1A;
    world.commit_staged_battle_anim(0);
    let a = &world.actors[0];
    assert_eq!(a.battle.queued_anim, 0x11, "staged 0x1A rewritten to 0x11");
    assert_eq!(a.battle.current_anim, 0x11);
    assert_eq!(a.battle_staged_anim, Some(0x11));
    assert!(a.battle_animation.is_some(), "art clip playing");

    // The materialized ME-archive stream moves the skeleton off the rest
    // pose at some point in the clip.
    let mut moved = false;
    for _ in 0..2000 {
        world.tick_battle_animations();
        if let Some(pose) = pose_outputs(&world, 0)
            && differs_from(&pose, &rest)
        {
            moved = true;
        }
        if world.actors[0].battle_staged_anim.is_none() {
            break;
        }
    }
    assert!(moved, "art stream deforms the skeleton");
    assert!(
        world.actors[0].battle_staged_anim.is_none(),
        "art clip completes and converges back"
    );
    assert_eq!(world.actors[0].battle.current_anim, 0);
}
