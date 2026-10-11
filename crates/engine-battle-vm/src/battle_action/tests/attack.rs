//! The attack band: face, approach, chain, counter, x2 refill, recovery and return.

use super::*;

#[test]
fn attack_face_in_range_routes_to_chain() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackFace.as_byte();
    host.actors[1].active_target = 4;
    // No range entry → returns 0 (in range).
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackChain.as_byte()
    ));
}

#[test]
fn attack_face_out_of_range_party_routes_to_short_step() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackFace.as_byte();
    host.actors[1].active_target = 4;
    host.ranges.insert((1, 4), 100);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackShortStep.as_byte()
    ));
}

/// Monster 73's tag table off PROT 0867: the pre-approach (`0x20`) sits at
/// entry 10 and the close-in (`0x21`) at entry 11 - far from the literal
/// `1` a party arm stages.
const TAGS_WITH_PRE_APPROACH: [u8; 12] = [
    0x00, 0x01, 0x0B, 0x02, 0x04, 0x05, 0x0D, 0x0E, 0x11, 0x23, 0x20, 0x21,
];

#[test]
fn attack_face_out_of_range_monster_with_a_pre_approach_routes_to_windup() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 4);
    ctx.action_state = ActionState::AttackFace.as_byte();
    host.actors[4].active_target = 1;
    host.ranges.insert((4, 1), 100);
    host.monster_tags.insert(4, TAGS_WITH_PRE_APPROACH.to_vec());
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackWindup.as_byte()
    ));
    // The tag-0x20 entry's INDEX is what lands in `+0x1DA`.
    assert_eq!(host.actors[4].queued_anim, 10);
}

/// 180 of the 186 populated monster records carry no tag-`0x20` entry;
/// retail stages their tag-`1` walk and routes them to the party's `0x19`
/// short step (`0x801E3284..0x801E32D0`), not the windup chain.
#[test]
fn attack_face_out_of_range_monster_without_a_pre_approach_takes_the_short_step() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 4);
    ctx.action_state = ActionState::AttackFace.as_byte();
    host.actors[4].active_target = 1;
    host.ranges.insert((4, 1), 100);
    host.monster_tags.insert(4, vec![0x00, 0x02, 0x01, 0x04]);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackShortStep.as_byte()
    ));
    assert_eq!(host.actors[4].queued_anim, 2, "the tag-1 entry's index");
}

/// The windup stages the walk once the pre-approach has committed, and the
/// advance stages the close-in on arrival - each by tag.
#[test]
fn the_monster_approach_chain_stages_walk_then_close_in_by_tag() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 4);
    host.actors[4].active_target = 1;
    host.monster_tags.insert(4, TAGS_WITH_PRE_APPROACH.to_vec());
    ctx.action_state = ActionState::AttackWindup.as_byte();
    host.actors[4].queued_anim = 10;
    host.actors[4].current_anim = 10;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::AttackAdvance.as_byte());
    assert_eq!(host.actors[4].queued_anim, 1, "tag-1 walk is entry 1 here");
    // In range (the test host's default range is 0): arrival.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::AttackCloseRange.as_byte());
    assert_eq!(host.actors[4].queued_anim, 11, "tag-0x21 close-in");
}

/// A monster that downs its target plays its tag-`0x22` entry - unless the
/// blow wiped the party.
#[test]
fn a_monster_that_downs_its_target_stages_its_ko_taunt_while_the_party_stands() {
    let tags = vec![0x00, 0x01, 0x02, 0x22];
    // Target down, another member standing: the taunt is staged.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 4);
    host.actors[4].active_target = 1;
    host.actors[1].liveness = 0;
    host.monster_tags.insert(4, tags.clone());
    ctx.action_state = ActionState::AttackReturn.as_byte();
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::DoneCleanup.as_byte());
    assert_eq!(host.actors[4].queued_anim, 3);
    assert!(host.actors[4].flag_bits.has(ActorFlags::ADVANCE_DONE));

    // The wiping blow: nobody left standing, nothing staged.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 4);
    host.actors[4].active_target = 1;
    for p in 0..3 {
        host.actors[p].liveness = 0;
    }
    host.monster_tags.insert(4, tags.clone());
    ctx.action_state = ActionState::AttackReturn.as_byte();
    step(&mut host, &mut ctx);
    assert_eq!(host.actors[4].queued_anim, 0);

    // Target still up: nothing staged.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 4);
    host.actors[4].active_target = 1;
    host.monster_tags.insert(4, tags);
    ctx.action_state = ActionState::AttackReturn.as_byte();
    step(&mut host, &mut ctx);
    assert_eq!(host.actors[4].queued_anim, 0);
}

#[test]
fn the_attack_x2_refill_replays_the_stream_once_and_demotes_the_starters() {
    // Retail's `0x801E39B4..0x801E3A64` arm: a party actor whose character
    // record carries the War God Icon bit replays the whole action stream
    // once, with every marked starter demoted from 0x1A to 0x19.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackChain.as_byte();
    host.ability_bits
        .insert(1, crate::battle_action::WAR_GOD_ATTACK_X2_BIT);
    host.actors[1].params[0] = 0x1A;
    host.actors[1].params[1] = 0x27;

    // Stage both bytes; the second step reads the terminator at the new
    // cursor and takes the refill instead of dropping to recovery.
    for _ in 0..2 {
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
        host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    }
    assert_eq!(
        ctx.action_state,
        ActionState::AttackChain.as_byte(),
        "the refill keeps the band in the strike loop"
    );
    assert_eq!(ctx.attack_x2_pass, 1);
    assert_eq!(host.actors[1].strike_index, 0, "the cursor is rewound");
    assert_eq!(
        host.actors[1].params[0], 0x19,
        "the marked starter is demoted for the second pass"
    );
    assert_eq!(host.actors[1].params[1], 0x27, "the art constant is kept");

    // The second pass runs to the terminator and then drops to recovery -
    // the counter is no longer zero, so the arm does not fire twice.
    for _ in 0..2 {
        step(&mut host, &mut ctx);
        host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    }
    assert_eq!(ctx.action_state, ActionState::AttackRecovery.as_byte());
    // Every second-pass stage bumps the counter (`0x801E37AC..0x801E37BC`):
    // 1 -> 2 -> 3. The refill tests `== 0`, so it still runs exactly twice,
    // and the carry arm (`< 2`) has released from the first second-pass
    // stage on.
    assert_eq!(ctx.attack_x2_pass, 3, "one bump per second-pass stage");
    assert!(
        ctx.attack_x2_pass >= crate::battle_action::ATTACK_X2_PASS_BOUND,
        "the damage kernel's carry arm is released for the second pass"
    );
}

#[test]
fn the_attack_x2_stage_bump_waits_for_the_refill() {
    // `0x801E37B4`: a zero counter is left alone, so the first pass stages
    // with the counter still `0`; the first second-pass stage lifts `1 -> 2`.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackChain.as_byte();
    host.ability_bits
        .insert(1, crate::battle_action::WAR_GOD_ATTACK_X2_BIT);
    host.actors[1].params[0] = 0x0C;
    host.actors[1].params[1] = 0x0D;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.attack_x2_pass, 0, "first-pass stage: no bump");
    host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    step(&mut host, &mut ctx);
    assert_eq!(ctx.attack_x2_pass, 1, "the refill, not the bump");
    host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    step(&mut host, &mut ctx);
    assert_eq!(ctx.attack_x2_pass, 2, "first second-pass stage");
}

#[test]
fn without_the_war_god_bit_the_stream_is_not_replayed() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackChain.as_byte();
    host.actors[1].params[0] = 0x1A;
    host.actors[1].params[1] = 0x27;
    for _ in 0..2 {
        step(&mut host, &mut ctx);
        host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    }
    assert_eq!(ctx.action_state, ActionState::AttackRecovery.as_byte());
    assert_eq!(ctx.attack_x2_pass, 0);
    assert_eq!(host.actors[1].params[0], 0x1A, "the queue is untouched");
}

/// The strike loop's counter head (`0x801E35F0..0x801E36E0`): a monster's
/// first strike frame with the latch armed on a party target that committed
/// an attack hands the loop to the counterer - it aims at the monster, its
/// initiative key is spent, the turn cursor steps, and its own first byte is
/// the one staged. The monster never swings.
#[test]
fn a_counter_latch_hands_the_strike_loop_to_the_counterer() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 3);
    ctx.action_state = ActionState::AttackChain.as_byte();
    host.actors[3].params[0] = 0x05;
    host.actors[3].active_target = 0;
    host.actors[0].action_category = ActionCategory::Attack.as_byte();
    host.actors[0].init_key = 40;
    host.counter_queue = Some(vec![0x0F, 0x0E, 0x00]);
    ctx.counter_pending = 1;
    let cursor = ctx.turn_cursor;

    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    assert_eq!(ctx.active_actor, 0, "the counterer acts");
    assert_eq!(ctx.counter_pending, 0, "the latch is consumed");
    assert_eq!(ctx.turn_cursor, cursor.wrapping_add(1));
    assert_eq!(host.actors[0].active_target, 3, "aimed back at the monster");
    assert_eq!(host.actors[0].init_key, 0, "its turn is spent");
    assert_eq!(host.actors[0].queued_anim, 0x0F, "its own first swing");
    assert_eq!(host.actors[3].queued_anim, 0, "the monster never swings");
    assert!(host.take().contains(&Event::Ui(0x66, 0)));
}

/// A target that did not commit an attack takes no counter: the latch drops
/// and the monster swings.
#[test]
fn a_counter_latch_without_a_committed_attack_drops() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 3);
    ctx.action_state = ActionState::AttackChain.as_byte();
    host.actors[3].params[0] = 0x05;
    host.actors[3].active_target = 0;
    host.actors[0].action_category = ActionCategory::Magic.as_byte();
    host.counter_queue = Some(vec![0x0F, 0x00]);
    ctx.counter_pending = 1;

    step(&mut host, &mut ctx);
    assert_eq!(ctx.active_actor, 3);
    assert_eq!(ctx.counter_pending, 0);
    assert_eq!(host.actors[3].queued_anim, 0x05, "the monster swings");
}

#[test]
fn attack_chain_walks_param_stream_until_terminator() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackChain.as_byte();
    // Strike sequence: 0x10, 0x12, 0xFF (terminator).
    host.actors[1].params[0] = 0x10;
    host.actors[1].params[1] = 0x12;
    host.actors[1].params[2] = 0xFF;

    // First step: queue 0x10 and fire damage.
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    assert_eq!(host.actors[1].queued_anim, 0x10);
    assert_eq!(host.actors[1].strike_index, 1);
    assert!(host.actors[1].flag_bits.has(ActorFlags::ADVANCE_DONE));
    // The strike loop fires no `apply_damage`: retail's one `jal 0x800402f4`
    // is in the spirit band, not here.
    assert!(
        !host
            .take()
            .iter()
            .any(|e| matches!(e, Event::ApplyDamage(..)))
    );

    // While ADVANCE_DONE is set the staged swing is in flight - the
    // chain holds without reading the next byte (the 0x801E370C gate).
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    assert_eq!(host.actors[1].strike_index, 1, "gated - no byte read");
    assert!(host.take().is_empty(), "gated - no damage fired");

    // Anim system signals clip end (clears ADVANCE_DONE).
    host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);

    // Second step: queue 0x12 and fire damage.
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    assert_eq!(host.actors[1].queued_anim, 0x12);
    assert_eq!(host.actors[1].strike_index, 2);
    assert!(
        !host
            .take()
            .iter()
            .any(|e| matches!(e, Event::ApplyDamage(..)))
    );
    host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);

    // Third step: terminator → recovery; SM clears ADVANCE_DONE. The cursor
    // is NOT rewound - retail never touches `ctx[+0x15]` in the `0x1E` loop
    // beyond its post-increment (`0x801E3734..0x801E3748`); it stays on the
    // terminator index until `0x1F -> 0x20` parks it at `0xFF`.
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackRecovery.as_byte()
    ));
    assert_eq!(
        host.actors[1].strike_index, 2,
        "cursor left on the terminator"
    );
    assert!(!host.actors[1].flag_bits.has(ActorFlags::ADVANCE_DONE));

    // `0x1F`: nothing in flight, so idle is staged over the last clip and the
    // cursor parks at `0xFF` on the way to `0x20`
    // (`0x801E3B04..0x801E3B1C`).
    ctx.action_state = ActionState::AttackRecovery.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackReturn.as_byte()
    ));
    assert_eq!(host.actors[1].strike_index, STRIKE_CURSOR_PARKED);
    assert_eq!(
        host.actors[1].queued_anim, 0,
        "idle staged over the last clip"
    );
}

#[test]
fn attack_recovery_holds_until_advance_done_clears() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackRecovery.as_byte();
    host.actors[1].flag_bits.set(ActorFlags::ADVANCE_DONE);
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackReturn.as_byte()
    ));
}

/// The bypass bytes `+0x287` / `+0x288` let the band out with the target
/// still reacting, through the same `0x50` store as every other exit
/// (`0x801E5584..0x801E5590`) - no counter swap, no turn-cursor bump.
#[test]
fn attack_return_lone_defeat_bypass_routes_to_done_cleanup() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackReturn.as_byte();
    host.actors[1].active_target = 4;
    host.actors[4].current_anim = 4;
    ctx.scripted_fight = 4;
    ctx.lone_defeat_latch = 1;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::DoneCleanup.as_byte()
    ));
    assert_eq!(ctx.turn_cursor, 0);
}

/// Once the attacker's own clip is done, `0x20` waits out the target's
/// reaction (`0x801E54FC..0x801E5580`): a reacting monster holds the band
/// until it is back on idle.
#[test]
fn attack_return_holds_while_the_target_reacts() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackReturn.as_byte();
    host.actors[1].active_target = 4;
    host.actors[4].current_anim = 5;
    for _ in 0..4 {
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    }
    // The scripted-fight byte alone is not the bypass.
    ctx.scripted_fight = 4;
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    host.actors[4].current_anim = 0;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::DoneCleanup.as_byte());
}

/// A dead monster holds its knockdown frame while the defeat fade walks its
/// colour word to black; the band leaves when the node stops drawing.
#[test]
fn attack_return_holds_until_a_fading_target_is_gone() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackReturn.as_byte();
    host.actors[1].active_target = 4;
    host.actors[4].current_anim = 4;
    host.actors[4].render_flag = crate::battle_formulas::STATE_DEFEAT_FADE;
    host.actors[4].render_color = 0x0010_0401;
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    host.actors[4].render_color = 0;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::DoneCleanup.as_byte());
}

/// A party target parked on the downed loop (entry `8`) releases the band;
/// a monster on the same id does not (`0x801E5504..0x801E5518`).
#[test]
fn attack_return_releases_on_a_downed_party_target() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 4);
    host.party_count = 3;
    ctx.action_state = ActionState::AttackReturn.as_byte();
    host.actors[4].active_target = 1;
    host.actors[1].current_anim = 7;
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    host.actors[1].current_anim = 8;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::DoneCleanup.as_byte());

    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    host.party_count = 3;
    ctx.action_state = ActionState::AttackReturn.as_byte();
    host.actors[1].active_target = 5;
    host.actors[5].current_anim = 8;
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
}

#[test]
fn attack_return_without_counter_attack_routes_to_done_cleanup() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackReturn.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::DoneCleanup.as_byte()
    ));
}

/// State `0x20` holds while the attacker's committed id is non-zero - the
/// last swing still playing, with its hit events (and the parked-cursor hit
/// that lands the combo total on live HP) still to fire. Leaving at once let
/// a long art's killing blow land during the next actor's action.
#[test]
fn attack_return_holds_until_the_last_swing_has_finished() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackReturn.as_byte();
    host.actors[1].current_anim = 0x11;
    for _ in 0..8 {
        let out = step(&mut host, &mut ctx);
        assert!(matches!(out, StepOutcome::Stay));
        assert_eq!(ctx.action_state, ActionState::AttackReturn.as_byte());
    }
    // The anim tick commits the idle `0x1F` staged: the band moves on.
    host.actors[1].current_anim = 0;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::DoneCleanup.as_byte());
}

// ---------------------------------------------------------------------------
// The Attack x2 refill's mark compare - `0x801E3A44..0x801E3A64`
// ---------------------------------------------------------------------------

/// `bne v0,a1,0x801E3A58` with `a1 = 1` is an **exact** compare against the
/// build loop's mark, so the Super tail-replace's `4` is skipped: a Super's
/// `0x1A` starter survives the War God Icon's second pass while an ordinary
/// newly-learned art's is demoted to `0x19`.
#[test]
fn attack_x2_refill_demotes_build_starters_and_spares_super_starters() {
    let mut host = RecHost::with_n_actors(ACTOR_SLOTS);
    for a in &mut host.actors {
        a.liveness = 1;
    }
    host.ability_bits.insert(0, WAR_GOD_ATTACK_X2_BIT);
    // `[0x0F, 0x1A(art), 0x27, 0x1A(super), 0x2C, 0x00]`: two `0x1A`
    // starters, one from the build loop and one the Super applier wrote.
    let a = &mut host.actors[0];
    a.params[0] = SWING_HIGH;
    a.params[1] = SPECIAL_STARTER;
    a.params[2] = 0x27;
    a.params[3] = SPECIAL_STARTER;
    a.params[4] = 0x2C;
    a.params[5] = 0;
    let mut marks = [0u32; ACTION_QUEUE_CAP];
    marks[1] = BUILD_STARTER_MARK;
    marks[3] = SUPER_STARTER_MARK;
    a.starter_marks = Some(marks);
    // The refill is reached by staging the stream's last byte and finding the
    // terminator at the bumped cursor, so park one byte short of it.
    a.strike_index = 4;

    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = 0;
    ctx.action_state = ActionState::AttackChain.as_byte();
    step(&mut host, &mut ctx);

    assert_eq!(
        host.actors[0].params[1], REGULAR_STARTER,
        "the build loop's starter is demoted"
    );
    assert_eq!(
        host.actors[0].params[3], SPECIAL_STARTER,
        "the Super applier's starter is not"
    );
    assert_eq!(ctx.attack_x2_pass, 1);
    assert_eq!(host.actors[0].strike_index, 0, "the stream replays from 0");
}

/// A queue no builder produced carries no marks; the refill then reconstructs
/// the build-loop marks from the bytes, which is the pre-carrier behaviour and
/// can only ever produce [`BUILD_STARTER_MARK`].
#[test]
fn attack_x2_refill_falls_back_to_reconstructed_marks() {
    let mut host = RecHost::with_n_actors(ACTOR_SLOTS);
    for a in &mut host.actors {
        a.liveness = 1;
    }
    host.ability_bits.insert(0, WAR_GOD_ATTACK_X2_BIT);
    let a = &mut host.actors[0];
    a.params[0] = SPECIAL_STARTER;
    a.params[1] = 0x27;
    a.params[2] = 0;
    a.starter_marks = None;
    a.strike_index = 1;

    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = 0;
    ctx.action_state = ActionState::AttackChain.as_byte();
    step(&mut host, &mut ctx);
    assert_eq!(host.actors[0].params[0], REGULAR_STARTER);
}
