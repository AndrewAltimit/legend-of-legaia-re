//! The HP-bar drain gate and the cast census behind the state-0x51 exit.

use super::*;

#[test]
fn hp_bar_drain_freezes_done_fade_down() {
    // PORT: FUN_801E7250 - the state-0x51 arm only decrements the
    // +0x6D8 countdown when the settle check returns 0; a party target
    // (+0x1DD < 3) with live HP (+0x14C) != bar display (+0x172) holds
    // the timer.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 0;
    host.actors[0].hp = 50;
    host.actors[0].hp_display = Some(80); // drain still animating
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    assert_eq!(ctx.frame_timer, 0, "timer frozen while draining");
    // Drain settles → the timer counts down and the state advances.
    host.actors[0].hp_display = Some(50);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));
}

#[test]
fn state_51_waits_for_the_bar_ramp_and_exits_on_settle() {
    // End-to-end over the machinery the gate assumes: a hit lands on a party
    // slot, seeding the `+0x10` accumulator (`FUN_801EC3E4` convention), the
    // per-frame ramp (`FUN_80047430`) drains it a quarter at a time, and the
    // action SM sits in state `0x51` until the bar catches up.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 0;

    // The host draws this actor's bar, and a 40-point hit lands.
    host.actors[0].hp = 200;
    host.actors[0].arm_hp_bar();
    host.actors[0].hp -= 40;
    host.actors[0].accumulate_hp_bar(40);
    assert_eq!(host.actors[0].hp_display, Some(200));
    assert_eq!(host.actors[0].hp_bar_pending, 40);

    // Frames pass: the ramp runs, the SM holds.
    let mut frames = 0;
    let out = loop {
        crate::battle_action::tick_hp_bars(&mut host);
        let out = step(&mut host, &mut ctx);
        frames += 1;
        assert!(frames < 100, "state 0x51 never released");
        if out != StepOutcome::Stay {
            break out;
        }
        assert_eq!(ctx.frame_timer, 0, "timer frozen while the bar ramps");
    };
    assert!(frames > 1, "the gate has to hold for at least one frame");
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));
    // Settled exactly on live HP - the total bar travel equals the seed.
    assert_eq!(host.actors[0].hp_display, Some(160));
    assert_eq!(host.actors[0].hp_bar_pending, 0);
}

#[test]
fn state_51_parks_forever_on_a_desynced_bar_with_a_zero_accumulator() {
    // The softlock shape: `+0x14C != +0x172` with `+0x10 == 0` on a party
    // slot. `FUN_80047430`'s guard at `0x800474E8` leaves the bar alone, so
    // the mismatch is absorbing and the `0x51` gate never releases.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 0;
    host.actors[0].hp = 199;
    host.actors[0].hp_display = Some(200);
    host.actors[0].hp_bar_pending = 0;

    for _ in 0..64 {
        crate::battle_action::tick_hp_bars(&mut host);
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    }
    assert_eq!(host.actors[0].hp_display, Some(200), "bar never moved");

    // The one re-sync in the dumped corpus (`FUN_801E752C`, the per-round
    // status ticker) clears it, and the action completes.
    host.actors[0].resync_hp_bar();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));
}

/// The kernel-level guard for direct runtime HP writers:
/// [`BattleActor::set_hp_synced`] is the retail ticker's write-then-re-sync
/// shape, so a direct write through it can never produce the absorbing
/// `hp != hp_display` / zero-accumulator pair the test above parks on.
#[test]
fn set_hp_synced_direct_write_does_not_park_the_drain_gate() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 0;
    host.actors[0].hp = 120;
    host.actors[0].arm_hp_bar();

    // The trap, for contrast: a bare `hp` write desyncs the pair and the
    // gate holds (the coherence tripwire names exactly this state).
    host.actors[0].hp = 250;
    assert!(crate::battle_action::hp_bar_drain_pending(&host, &ctx));

    // The guarded write settles the pair in the same call - the gate is
    // open on the very next step, no ramp frames needed.
    host.actors[0].set_hp_synced(250);
    host.actors[0].debug_assert_hp_bar_coherent(0);
    assert!(!crate::battle_action::hp_bar_drain_pending(&host, &ctx));
    assert_eq!(host.actors[0].hp_display, Some(250));
    assert_eq!(host.actors[0].hp_bar_pending, 0);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));

    // A non-animating host (`hp_display == None`) stays non-animating.
    host.actors[2].hp_display = None;
    host.actors[2].set_hp_synced(77);
    assert_eq!(host.actors[2].hp, 77);
    assert_eq!(host.actors[2].hp_display, None);
}

#[test]
fn monster_bar_settles_in_one_frame() {
    // `FUN_80047430`'s monster arm takes the whole delta at once, so a
    // monster target can never hold the gate even when the host animates it.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    host.actors[4].hp = 100;
    host.actors[4].arm_hp_bar();
    host.actors[4].hp -= 60;
    host.actors[4].accumulate_hp_bar(60);
    crate::battle_action::tick_hp_bars(&mut host);
    assert_eq!(host.actors[4].hp_display, Some(40));
    assert_eq!(host.actors[4].hp_bar_pending, 0);
    let _ = &mut ctx;
}

#[test]
fn a_small_partys_first_monster_never_holds_the_settle_gate() {
    // Retail's `FUN_801E7250` tests `+0x1DD < 3` on the **retail** pool slot,
    // where monster `k` always sits at `3 + k`. The engine seats it at
    // `party_count + k`, so in a two-member party the first monster is engine
    // slot 2. The shape that parked unseeded full-game runs in `vell`: a
    // monster hit earlier (bar armed) is killed by Nighto's instant-death arm,
    // which writes live HP with no readout seed - a monster-side desync retail
    // never consults.
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 0);
    host.party_count = 2;
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[0].active_target = 2;
    host.actors[2].hp = 175;
    host.actors[2].arm_hp_bar();
    host.actors[2].hp = 0;
    host.actors[2].hp_bar_pending = 0;

    assert!(!crate::battle_action::hp_bar_drain_pending(&host, &ctx));
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));

    // The same seat ramps on the monster arm: the whole delta in one frame.
    host.actors[2].hp = 100;
    host.actors[2].hp_display = Some(100);
    host.actors[2].hp -= 60;
    host.actors[2].accumulate_hp_bar(60);
    crate::battle_action::tick_hp_bars(&mut host);
    assert_eq!(host.actors[2].hp_display, Some(40));
    assert_eq!(host.actors[2].hp_bar_pending, 0);

    // Party seats still hold it, and the all-party code still scans only
    // the two seated members.
    host.actors[0].active_target = 1;
    host.actors[1].hp = 50;
    host.actors[1].hp_display = Some(80);
    assert!(crate::battle_action::hp_bar_drain_pending(&host, &ctx));
    host.actors[1].hp_display = Some(50);
    host.actors[0].active_target = 8;
    host.actors[2].hp_display = Some(99);
    assert!(!crate::battle_action::hp_bar_drain_pending(&host, &ctx));
}

#[test]
fn state_51_park_from_the_clamp_asymmetry() {
    // The softlock, reproduced end to end from the two appliers disagreeing
    // about what to clamp against (`FUN_801EC3E4`'s readout-side clamp at
    // `0x801EDB70` vs the live-HP-side commit at `0x801EEA10`). NB the
    // asymmetry only amplifies: the pre-lagged readout seeded below is the
    // precondition, and no retail-only capture has produced one that survives
    // an action's own commit + settle wait (battle-action.md, clamp section).
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 0;

    // A party slot whose readout is already behind live HP - the state a
    // previous action's assigning seed, or a truncated drain, leaves.
    host.actors[0].hp = 500;
    host.actors[0].arm_hp_bar();
    host.actors[0].hp_display = Some(40);
    host.actors[0].hp_bar_pending = 0;

    // A survivable hit, bigger than the readout but smaller than live HP.
    // The readout-side clamp caps the accumulator at the drawn 40; live HP
    // takes the whole 100.
    host.actors[0].hp -= 100;
    host.actors[0].accumulate_hp_bar(100);
    assert_eq!(host.actors[0].hp_bar_pending, 40, "clamped to the readout");

    // Drain it out. The readout lands on zero, live HP is still 400.
    for _ in 0..64 {
        crate::battle_action::tick_hp_bars(&mut host);
    }
    assert_eq!(
        host.actors[0].hp_display,
        Some(0),
        "readout drained to zero"
    );
    assert_eq!(host.actors[0].hp, 400, "live HP survived the hit");
    assert_eq!(host.actors[0].hp_bar_pending, 0, "accumulator spent");

    // Absorbing: `hp != hp_display` with a zero accumulator, so the ramp's
    // guard leaves the readout alone and the `0x51` gate never releases.
    for _ in 0..64 {
        crate::battle_action::tick_hp_bars(&mut host);
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    }
    assert_eq!(ctx.frame_timer, 0, "the countdown never ran");
    assert_eq!(host.actors[0].hp_display, Some(0));

    // A later ordinary hit does not clear it - the offset rides along.
    host.actors[0].hp -= 10;
    host.actors[0].accumulate_hp_bar(10);
    for _ in 0..64 {
        crate::battle_action::tick_hp_bars(&mut host);
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    }
    assert_ne!(
        host.actors[0].hp_display,
        Some(host.actors[0].hp),
        "still desynced"
    );
}

#[test]
fn cast_census_drives_the_magic_band_exit_gate() {
    // `magic_exit_gate` (ctx `+0x249`) had no writer outside tests. The
    // census makes it a live measurement over the actor pool, so a visible
    // actor stuck mid-animation holds `MagicExit` open and clearing it
    // releases the state.
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicExit.as_byte();
    for a in &mut host.actors {
        a.render_color = 0;
        a.current_anim = 0;
    }
    host.actors[4].render_color = 0x0100_0000;
    host.actors[4].current_anim = 3;

    crate::battle_action::tick_cast_census(&host, &mut ctx);
    assert_eq!(ctx.magic_exit_gate, 1, "one visible actor still animating");
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);

    host.actors[4].current_anim = 0;
    crate::battle_action::tick_cast_census(&host, &mut ctx);
    assert_eq!(ctx.magic_exit_gate, 0);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::DoneCleanup.as_byte()
    ));
}

#[test]
fn cast_census_latches_the_sole_survivor_targets() {
    let (mut ctx, host) = fresh(ActionCategory::Magic, 1);
    // `fresh` marks every slot alive, so neither latch survives the
    // "exactly one" test.
    crate::battle_action::tick_cast_census(&host, &mut ctx);
    assert_eq!((ctx.item_target_a, ctx.item_target_b), (0, 0));

    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    for (i, a) in host.actors.iter_mut().enumerate() {
        a.liveness = u16::from(i == 2 || i == 5);
    }
    crate::battle_action::tick_cast_census(&host, &mut ctx);
    assert_eq!(ctx.item_target_a, 3, "party slot 2, stored 1-based");
    assert_eq!(ctx.item_target_b, 5, "monster slot 5, stored 0-based");
}

/// The census walks retail numbering (party `0..3`, monsters from `3`), so
/// a solo party's monsters - seated by the engine right behind it at slot
/// `1` - are not counted as party members. Read in engine order, Vahn alone
/// against a Gobu Gobu read two living "party" seats, the sole-party latch
/// never held, an all-allies cast kept its group code `8` and the cast-begin
/// arm turned him onto his own centroid (`evolved_0x91_midcast`).
#[test]
fn cast_census_maps_a_solo_party_into_retail_numbering() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    host.party_count = 1;
    for (i, a) in host.actors.iter_mut().enumerate() {
        a.liveness = u16::from(i <= 1);
    }
    crate::battle_action::tick_cast_census(&host, &mut ctx);
    assert_eq!(ctx.item_target_a, 1, "Vahn, the sole party seat, 1-based");
    assert_eq!(
        ctx.item_target_b, 1,
        "the sole monster, retail slot 3, is engine seat 1"
    );
}

#[test]
fn hp_bar_drain_monster_target_never_pends() {
    // FUN_801E7250's `2 < bVar1` early-out: monster targets (3..=7)
    // return 0 (settled) without inspecting the HP pair.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 4;
    host.actors[4].hp = 10;
    host.actors[4].hp_display = Some(99);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));
}

#[test]
fn hp_bar_drain_target_8_scans_the_party_side_only() {
    // FUN_801E7250's target-8 arm walks slots `0 .. ctx[+0x00] - 1`, and
    // `ctx[+0x00]` is the **party member count**, not the total actor count.
    // So an unsettled monster readout does not hold the gate even on the
    // all-target arm - the same answer the `3..=7` early-out gives.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 8;
    host.actors[6].hp = 10;
    host.actors[6].hp_display = Some(11);
    let out = step(&mut host, &mut ctx);
    assert!(
        matches!(out, StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()),
        "a monster slot is outside the scan window"
    );

    // A party slot inside the window does hold it.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 8;
    host.actors[2].hp = 10;
    host.actors[2].hp_display = Some(11);
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    host.actors[2].hp_display = None; // host stops animating → settled
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));
}
