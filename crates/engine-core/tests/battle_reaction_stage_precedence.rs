//! Regression: a freshly staged battle anim REPLACES an in-flight hit
//! reaction instead of being swallowed by it.
//!
//! Retail has exactly one staged-anim channel. The damage arm writes the
//! reaction entry into `actor[+0x1DA]` (`FUN_800402F4` `0x80042118`
//! knockdown / `0x80042124` flinch) - the same byte the action state machine
//! stages the approach walk and each weapon swing into - and the anim commit
//! copies `+0x1DA` into `+0x1DB` with no reaction guard anywhere on that path
//! (`FUN_8004AD80` `0x8004AEB0..0x8004AEB8`). Even the knockdown -> get-up
//! chain runs by writing `+0x1DA = +0x1F2` (`0x8004B690`). So the last writer
//! wins.
//!
//! The port briefly gave its separate `battle_reaction` latch priority over
//! the staged channel, which meant a party member that had been hit spent its
//! own attack turn playing knockdown / get-up: it walked to the target and
//! back lying flat, with the approach clip and every weapon swing dropped.
//!
//! Disc-free: synthetic clips only.

use legaia_asset::monster_archive::{MonsterAnimation, PartPose};
use legaia_engine_core::world::World;

/// One-part clip whose frame `f` translates to `tx + f` on X, so a test can
/// identify which clip is playing from the posed output.
fn clip(action_id: u8, frames: usize, tx: i16) -> MonsterAnimation {
    MonsterAnimation {
        action_id,
        rate: 2,
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

/// Actor 0 with the player-file action-slot layout: idle 0, walk 1, flinch
/// 2, knockdown 4, get-up 5, and the four equipment swings at 0xC..0xF.
fn world_with_reaction_clips() -> World {
    let mut world = World::new();
    world.actors[0].active = true;
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.liveness = 1;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 22];
    for slot in [0usize, 1, 2, 4, 5, 0xC, 0xD, 0xE, 0xF] {
        clips[slot] = Some(clip(slot as u8, 4, slot as i16 * 100));
    }
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world
}

/// The clip currently bound to actor 0, by its `action_id`.
fn playing(world: &World) -> Option<u8> {
    world.actors[0]
        .battle_animation
        .as_ref()
        .map(|p| p.action_id())
}

#[test]
fn hit_reaction_starts_a_knockdown() {
    // Baseline, so the regression test below is not vacuous: a damaged actor
    // that owns a get-up entry takes the knockdown arm (retail
    // `FUN_800402F4`: `+0x1F2 != 0` -> `+0x1DA = +0x1F1`).
    let mut world = world_with_reaction_clips();
    world.queue_battle_reaction(0, true);
    assert_eq!(playing(&world), Some(4), "knockdown clip is in flight");
}

#[test]
fn staged_approach_replaces_an_in_flight_reaction() {
    let mut world = world_with_reaction_clips();
    world.queue_battle_reaction(0, true);
    assert_eq!(playing(&world), Some(4));

    // The action SM stages the party approach walk (literal anim id 1) while
    // the knockdown is still playing.
    world.actors[0].battle.queued_anim = 1;
    world.commit_staged_battle_anim(0);

    assert_eq!(
        playing(&world),
        Some(1),
        "the staged walk must take the player from the reaction"
    );
    assert_eq!(
        world.actors[0].battle_reaction, None,
        "committing a staged clip drops the reaction latch, so the \
         end-of-clip get-up chain cannot steal the clip back"
    );
    assert_eq!(world.actors[0].battle_staged_anim, Some(1));
}

#[test]
fn staged_swing_replaces_an_in_flight_reaction() {
    let mut world = world_with_reaction_clips();
    world.queue_battle_reaction(0, true);
    // A weapon swing staged out of the attack chain's strike loop.
    world.actors[0].battle.queued_anim = 0x0C;
    world.commit_staged_battle_anim(0);
    assert_eq!(playing(&world), Some(0x0C), "swing clip plays");
    assert_eq!(world.actors[0].battle_reaction, None);
}

#[test]
fn the_reaction_still_chains_to_get_up_when_nothing_is_staged() {
    // The knockdown -> get-up chain is unchanged for an actor whose turn is
    // not running: nothing overwrites `+0x1DA`, so the reaction owns the
    // clip until it completes.
    let mut world = world_with_reaction_clips();
    world.queue_battle_reaction(0, true);
    // Run the one-shot knockdown to its end.
    world.actors[0].battle_animation.as_mut().unwrap().step = 4096;
    world.tick_battle_animations();
    assert!(
        world.actors[0]
            .battle_animation
            .as_ref()
            .unwrap()
            .finished()
    );
    world.tick_battle_animations();
    assert_eq!(playing(&world), Some(5), "knockdown chains into get-up");
    assert_eq!(world.actors[0].battle_reaction, Some(5));
}
