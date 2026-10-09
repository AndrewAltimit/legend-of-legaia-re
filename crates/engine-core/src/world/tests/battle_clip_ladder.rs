//! The commit's clip-tag ladder on the reaction channel
//! (`world::battle::clip_ladder`, `FUN_8004AD80` `0x8004BE30..0x8004BF4C`
//! and the monster-death arm `0x8004B094..0x8004B6A0`).

use super::*;
use vm::battle_action::ActorFlags;

fn clip(action_id: u8) -> MonsterAnimation {
    use legaia_asset::monster_archive::PartPose;
    MonsterAnimation {
        action_id,
        rate: 2,
        attach_key: 0,
        solo_flag: 0,
        impact_class: 0,
        effect_script: Vec::new(),
        part_count: 1,
        frame_count: 2,
        frames: vec![vec![PartPose::default()]; 2],
    }
}

/// Run the playing clip to its end and let the natural-end path act.
fn finish(world: &mut World, slot: usize) {
    world.actors[slot].battle_animation.as_mut().unwrap().step = 1024;
    world.tick_battle_animations();
    world.tick_battle_animations();
}

#[test]
fn a_downed_party_member_plays_knockdown_then_seven_then_latched_eight() {
    let mut world = World::new();
    world.actors[0].active = true;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 12];
    for t in [0u8, 2, 4, 5, 7, 8] {
        clips[usize::from(t)] = Some(clip(t));
    }
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.hp = 0;
    world.queue_battle_reaction(0, false);
    assert_eq!(world.actors[0].battle_reaction, Some(4));
    finish(&mut world, 0);
    assert_eq!(world.actors[0].battle_reaction_entry, Some(7));
    assert!(
        !world.actors[0]
            .battle
            .flag_bits
            .has(ActorFlags::FX_SUPPRESSED)
    );
    finish(&mut world, 0);
    assert_eq!(world.actors[0].battle_reaction_entry, Some(8));
    assert!(
        world.actors[0]
            .battle
            .flag_bits
            .has(ActorFlags::FX_SUPPRESSED),
        "the tag-8 commit raises the root-motion latch"
    );
    // Entry 8 re-commits itself at every natural end; the latch survives the
    // natural end's `& 0xF8`.
    finish(&mut world, 0);
    assert_eq!(world.actors[0].battle_reaction_entry, Some(8));
    assert!(
        world.actors[0]
            .battle
            .flag_bits
            .has(ActorFlags::FX_SUPPRESSED)
    );
}

#[test]
fn a_living_knockdown_is_not_latched_and_its_getup_returns_to_idle() {
    let mut world = World::new();
    world.actors[0].active = true;
    world.actors[0].battle.hp = 50;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 12];
    for t in [0u8, 2, 4, 5] {
        clips[usize::from(t)] = Some(clip(t));
    }
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.flag_bits = ActorFlags(ActorFlags::FX_SUPPRESSED);
    world.queue_battle_reaction(0, true);
    finish(&mut world, 0);
    // The knockdown's row cleared `+0x1DC`; the get-up's raised bit 2.
    assert_eq!(world.actors[0].battle_reaction, Some(5));
    assert_eq!(world.actors[0].battle.flag_bits.0, ActorFlags::EXIT);
    finish(&mut world, 0);
    assert_eq!(world.actors[0].battle_reaction, None);
    assert_eq!(world.actors[0].battle.flag_bits.0, 0);
}

fn dead_monster_world(seru: u8) -> World {
    let mut world = World::new();
    world.party.party_count = 1;
    world.actors[1].active = true;
    world.actors[1].battle_monster_id = Some(1);
    world.actors[1].battle.hp = 0;
    // Entry order is the archive's: knockdown at 2, get-up at 3.
    let clips = vec![Some(clip(0)), Some(clip(2)), Some(clip(4)), Some(clip(5))];
    world.set_actor_battle_action_clips(1, std::sync::Arc::new(clips));
    world.battle_ctx.absorbed_seru = seru;
    world
}

#[test]
fn a_dead_monster_holds_its_knockdown_with_the_latch() {
    let mut world = dead_monster_world(0);
    world.queue_battle_reaction(1, false);
    assert_eq!(world.actors[1].battle_reaction_entry, Some(2));
    finish(&mut world, 1);
    assert_eq!(world.actors[1].battle_reaction, Some(4));
    assert!(
        world.actors[1]
            .battle
            .flag_bits
            .has(ActorFlags::FX_SUPPRESSED)
    );
    assert!(
        world.actors[1]
            .battle_animation
            .as_ref()
            .unwrap()
            .finished()
    );
}

#[test]
fn a_dead_monster_with_a_staged_seru_rises_on_its_getup() {
    let mut world = dead_monster_world(1);
    world.queue_battle_reaction(1, false);
    finish(&mut world, 1);
    // `+0x1DC = 4` and the get-up entry `+0x1F2` (entry 3).
    assert_eq!(world.actors[1].battle_reaction_entry, Some(3));
    assert_eq!(world.actors[1].battle.flag_bits.0, ActorFlags::EXIT);
}
