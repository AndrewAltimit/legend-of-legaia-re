//! A summon creature seated on a seat the previous cast's creature left in
//! its defeat fade.
//!
//! The defect: a second Seru cast in one battle reuses the creature seat, and
//! the first creature can leave it with tint state `+0x21C = 2` and a
//! draining colour word. The engine's installer kept both, so the new
//! creature faded out, and the tint pass - which took any seat with no
//! monster record for a party seat - ran the party fade-done reset on it,
//! zeroing the clip PROT 0907's arm 7 had staged. Arm 8's bump then staged
//! clip `1` instead of `2`, and arm 10, which waits for clip `2`, held the
//! summon band's `0x36` for good (the full-game ladder's Rogue Tower boss,
//! Gala's Nighto after an earlier Puera).
//!
//! Retail's installer `FUN_801F19EC` resets the seat's presentation words,
//! and its tint pass `FUN_80050120` tests party by seat index (`< 3`), so
//! the creature's seat 7 never takes the party reset.

use super::*;
use legaia_engine_battle_vm::battle_action::ActorFlags;
use legaia_engine_battle_vm::battle_formulas::STATE_DEFEAT_FADE;

/// A creature seat left mid defeat fade by an earlier cast's creature.
fn dirty_seat(world: &mut World, slot: usize) {
    let b = &mut world.actors[slot].battle;
    b.render_flag = STATE_DEFEAT_FADE;
    b.render_color = 688;
    b.flag_word = 0x8100_0000;
    b.impact_state = 3;
    // A shattering creature's word, and an after-image history whose
    // frames clear the ring-id gate.
    b.shatter = 0x800;
    let frame = crate::world::BattleGhostFrame {
        pose: legaia_anm::player::PoseFrame {
            bone_outputs: Vec::new(),
            factor: 0,
            finished: false,
        },
        pos: [1, 2, 3],
        ghost_eligible: true,
        clip_key: 4,
        variant_track: Default::default(),
        frame: 5,
    };
    world.actors[slot].battle_pose_history = std::iter::repeat_n(frame, 32).collect();
}

#[test]
fn the_installer_resets_a_reused_seats_presentation_words() {
    let mut world = module_code_world();
    world.arm_summon_stager(0, 0x85);
    dirty_seat(&mut world, 9);
    world.seat_summon_actor(9);
    let b = &world.actors[9].battle;
    // `0x801F1D5C`, `0x801F1D68`, `0x801F1E7C`, `0x801F1E88`.
    assert_eq!(b.render_color, u32::MAX, "+0x04 = -1");
    assert_eq!(b.flag_word, 0, "+0x08 = 0");
    assert_eq!(
        b.render_flag, 0,
        "+0x21C = 0: the stale defeat fade is gone"
    );
    assert_eq!(b.impact_state, 0, "+0x21F = 0");
    assert_eq!(b.shatter, 0, "+0x1BA = 0 (`0x801F1E94`)");
    // `+0x1FB..+0x21A` zeroed (`0x801F1E98..0x801F1EB0`): no frame of the
    // ring is a ghost, and the rings it does not touch keep their bytes.
    let hist = &world.actors[9].battle_pose_history;
    assert_eq!(hist.len(), 32);
    assert!(hist.iter().all(|f| !f.ghost_eligible));
    assert!(hist.iter().all(|f| f.pos == [1, 2, 3] && f.clip_key == 4));
}

#[test]
fn the_creature_seats_fade_end_keeps_its_staged_clip() {
    let mut world = module_code_world();
    world.arm_summon_stager(0, 0x85);
    world.casting.summon_actor_slot = Some(9);
    world.actors[9].active = true;
    // A fade one step from its end, with a clip staged behind it.
    world.actors[9].battle.render_flag = STATE_DEFEAT_FADE;
    world.actors[9].battle.render_color = 8;
    world.actors[9].battle.queued_anim = 1;
    world.tick_battle_impact_fx();
    let b = &world.actors[9].battle;
    assert_eq!(b.render_color, 0, "the fade ran out");
    assert_eq!(
        b.queued_anim, 1,
        "seat 7 is not a party seat: no `+0x1DA = 0` reset"
    );
    assert_eq!(b.render_flag, STATE_DEFEAT_FADE, "no `+0x21C = 0` reset");
    assert!(!b.flag_bits.has(ActorFlags::WINDUP_DONE), "no `+0x1DC = 1`");

    // A party seat takes the reset (`0x80050344..0x80050354`).
    world.actors[0].battle.render_flag = STATE_DEFEAT_FADE;
    world.actors[0].battle.render_color = 8;
    world.actors[0].battle.queued_anim = 1;
    world.tick_battle_impact_fx();
    let b = &world.actors[0].battle;
    assert_eq!(b.queued_anim, 0);
    assert_eq!(b.render_flag, 0);
    assert!(b.flag_bits.has(ActorFlags::WINDUP_DONE));
}
