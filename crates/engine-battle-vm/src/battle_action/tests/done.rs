//! The done / fade-down / Seru-absorb band and its level-up banner.

use super::*;

#[test]
fn done_cleanup_sets_recoil_per_category() {
    let (mut ctx, mut host) = fresh(ActionCategory::Spirit, 1);
    ctx.action_state = ActionState::DoneCleanup.as_byte();
    step(&mut host, &mut ctx);
    // Spirit category → the accumulator is `0x20`, paid into the Spirit
    // gauge and then zeroed (`sb zero,0x224(s3)` at `0x801E5E70`).
    assert_eq!(host.actors[1].spirit_gauge, 0x20);
    assert_eq!(host.actors[1].action_recoil, 0);
    assert!(host.actors[1].flag_bits.has(ActorFlags::EXIT));
    assert_eq!(ctx.frame_timer, 0x3C);
}

/// A Spirit AP setting (`--spirit-ap`'s twin) replaces the `0x20`: a
/// positive one is paid like retail's, a negative one drains the gauge
/// (floored at zero) with the AP-Boost passives inert; retail's own value is
/// the unmodified band.
#[test]
fn done_cleanup_pays_the_spirit_ap_setting() {
    let gauge_after = |ap: Option<i16>, start: u16, bits: u32| {
        let (mut ctx, mut host) = fresh(ActionCategory::Spirit, 1);
        host.spirit_ap = ap;
        host.actors[1].spirit_gauge = start;
        host.ability_bits_high.insert(1, bits);
        ctx.action_state = ActionState::DoneCleanup.as_byte();
        step(&mut host, &mut ctx);
        assert_eq!(host.actors[1].action_recoil, 0);
        host.actors[1].spirit_gauge
    };
    assert_eq!(gauge_after(None, 10, 0), 10 + 0x20);
    assert_eq!(gauge_after(Some(SPIRIT_AP_RETAIL), 10, 0), 10 + 0x20);
    assert_eq!(gauge_after(Some(64), 10, 0), 74);
    assert_eq!(gauge_after(Some(100), 10, 0), 100, "capped at 100");
    assert_eq!(gauge_after(Some(0), 10, 0), 10);
    // AP Boost 2 (`+0xF8 & 0x200`) adds a quarter of the setting.
    assert_eq!(gauge_after(Some(64), 0, 0x200), 80);
    assert_eq!(gauge_after(Some(-16), 30, 0), 14);
    assert_eq!(gauge_after(Some(-16), 5, 0x200), 0, "floored, boosts inert");
    // The plate target state `0x46` stages reads the same setting.
    assert_eq!(spirit_turn_gains(32), (32, 40, 35));
    assert_eq!(spirit_turn_gains(-20), (-20, -20, -20));
}

#[test]
fn done_cleanup_attack_uses_recover_pose() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneCleanup.as_byte();
    step(&mut host, &mut ctx);
    assert!(host.take().contains(&Event::Pose(1, Pose::Recover)));
}

/// `DoneCleanup`'s tail `jal`s the gauge re-arm (`FUN_801E93C8` at
/// `0x801E5F64`): every one of the seven pool slots gets the neutral arm-width
/// seed and any `+0x21C` latch holding exactly `1` is cleared.
#[test]
fn done_cleanup_rearms_the_command_gauge_slots() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneCleanup.as_byte();
    ctx.gauge_rearm_latch = 7;
    // Party gate: `+0x1D9 < 0x10` (a plain direction command).
    host.actors[1].current_anim = 0x0C;
    for (i, a) in host.actors.iter_mut().enumerate() {
        a.render_flag = if i % 2 == 0 { 1 } else { 200 };
        a.anim_rate = crate::battle_anim_rate::AnimRate(0);
    }
    step(&mut host, &mut ctx);
    for i in 0..crate::battle_gauge_rearm::ANIM_RATE_SLOTS {
        assert_eq!(
            host.actors[i].anim_rate.get(),
            crate::battle_gauge_rearm::ANIM_RATE_SEED,
            "slot {i} arm width seeded"
        );
        let expect_latch = if i % 2 == 0 { 0 } else { 200 };
        assert_eq!(host.actors[i].render_flag, expect_latch, "slot {i} latch");
    }
    // Slot 7 is outside retail's `while (i < 7)` walk.
    assert_eq!(host.actors[7].anim_rate.get(), 0);
    assert_eq!(ctx.gauge_rearm_latch, 0);
}

/// The attack band's own passes take the same tail (`0x801E5F64`): once the
/// acting member's art clip has ended (`+0x1D9 < 0x10`), a pass of `0x20`
/// returns every slot to normal speed - not only the Done band - while a
/// pass still on the art clip leaves the slow-motion standing.
#[test]
fn the_attack_band_restores_the_anim_rates_once_the_art_clip_ends() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackReturn.as_byte();
    for a in &mut host.actors {
        a.anim_rate = crate::battle_anim_rate::AnimRate(4);
    }
    host.actors[1].current_anim = 0x11;
    step(&mut host, &mut ctx);
    assert_eq!(host.actors[3].anim_rate.get(), 4, "art clip still playing");
    host.actors[1].current_anim = 0;
    step(&mut host, &mut ctx);
    assert_eq!(
        host.actors[3].anim_rate.get(),
        crate::battle_gauge_rearm::ANIM_RATE_SEED
    );
}

/// The gate is real: a materialised art (`+0x1D9 >= 0x10`) on a party slot
/// closes it and nothing is touched.
#[test]
fn done_cleanup_skips_the_rearm_for_a_materialised_art() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneCleanup.as_byte();
    ctx.gauge_rearm_latch = 7;
    host.actors[1].current_anim = 0x1B;
    for a in &mut host.actors {
        a.render_flag = 1;
        a.anim_rate = crate::battle_anim_rate::AnimRate(0);
    }
    step(&mut host, &mut ctx);
    assert!(host.actors.iter().all(|a| a.anim_rate.get() == 0));
    assert!(host.actors.iter().all(|a| a.render_flag == 1));
    assert_eq!(ctx.gauge_rearm_latch, 7);
}

/// A monster slot reads the art record's `+0x87` flag instead of the staged
/// id, through the host hook - the default (`0`) leaves the gate open.
#[test]
fn done_cleanup_rearm_gate_for_a_monster_slot_uses_the_record_flag() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 4);
    ctx.action_state = ActionState::DoneCleanup.as_byte();
    // A monster's staged id is irrelevant on this arm; the default hook
    // reports a clear record flag, so the re-arm runs.
    host.actors[4].current_anim = 0xFF;
    for a in &mut host.actors {
        a.anim_rate = crate::battle_anim_rate::AnimRate(0);
    }
    step(&mut host, &mut ctx);
    assert_eq!(
        host.actors[0].anim_rate.get(),
        crate::battle_gauge_rearm::ANIM_RATE_SEED
    );
}

/// The `0x50` seed's one override (`0x801E5F2C..0x801E5F3C`): a level-up
/// banner on the context re-seeds `0x96` in place of the `0x3C` just stored,
/// for every category arm.
#[test]
fn done_cleanup_extends_the_tail_for_a_level_up_banner() {
    for category in [
        ActionCategory::Attack,
        ActionCategory::Magic,
        ActionCategory::Run,
    ] {
        let (mut ctx, mut host) = fresh(category, 1);
        ctx.action_state = ActionState::DoneCleanup.as_byte();
        ctx.levelup_banner_element = 0x65;
        step(&mut host, &mut ctx);
        assert_eq!(
            ctx.frame_timer,
            crate::battle_action::done::DONE_LEVELUP_BANNER_FRAMES,
            "{category:?}"
        );
    }
}

/// The banner skip (`0x801E6078..0x801E60B4`) is two gates deep: a pad word
/// AND a countdown already below `0x5B`. Above the threshold the press does
/// nothing, so the banner is guaranteed its opening frames.
#[test]
fn done_fade_down_banner_skip_needs_both_the_pad_and_the_threshold() {
    use crate::battle_action::done::{DONE_BANNER_SKIP_BELOW, DONE_LEVELUP_BANNER_FRAMES};

    // Pad held, but the countdown is still above the threshold: no skip.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    host.pad_word = 0x40;
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.levelup_banner_element = 0x65;
    ctx.frame_timer = DONE_LEVELUP_BANNER_FRAMES;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.frame_timer, DONE_LEVELUP_BANNER_FRAMES - 1);

    // Below the threshold with the pad held: the tail is cut immediately.
    ctx.frame_timer = DONE_BANNER_SKIP_BELOW;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.frame_timer, -1);

    // Same frame, pad idle: the plain decrement.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.levelup_banner_element = 0x65;
    ctx.frame_timer = DONE_BANNER_SKIP_BELOW;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.frame_timer, DONE_BANNER_SKIP_BELOW - 1);

    // Same frame, pad held, but no banner: the skip is banner-gated.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    host.pad_word = 0x40;
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = DONE_BANNER_SKIP_BELOW;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.frame_timer, DONE_BANNER_SKIP_BELOW - 1);
}

#[test]
fn done_fade_down_holds_then_routes_to_end_of_action() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 2;
    // Two ticks bring timer below 0.
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::EndOfAction.as_byte()
    ));
}

#[test]
fn done_fade_down_with_absorbed_seru_routes_to_seru_absorb() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    ctx.absorbed_seru = 1;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::DoneSeruAbsorb.as_byte()
    ));
}

/// Retail's Done-band exit is a test on the timer's **value**, re-run every
/// pass (`bgez v0,0x801E6158` at `0x801E60F0`), not on the one frame the
/// countdown crosses zero.
///
/// The difference only shows when something else holds the band on that exact
/// frame: `ctx[+0x276]` up on the crossing pass used to consume the crossing
/// and leave the state with no way out at all.
#[test]
fn the_done_band_still_leaves_after_the_menu_flag_clears_on_the_crossing_frame() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    ctx.menu_open = 1;
    // The flag is up on the frame the countdown would have crossed, and for
    // a good while after.
    for _ in 0..40 {
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    }
    // Retail pins the countdown at the `0xC` floor rather than letting it
    // sink, so the tail still has twelve frames to run once the flag drops.
    assert_eq!(
        ctx.frame_timer,
        crate::battle_action::done::DONE_MENU_HOLD_FRAMES
    );
    ctx.menu_open = 0;
    let mut passes = 0;
    loop {
        match step(&mut host, &mut ctx) {
            StepOutcome::Stay => {
                passes += 1;
                assert!(passes < 64, "the Done band never left");
            }
            StepOutcome::Transition { to, .. } => {
                assert_eq!(to, ActionState::EndOfAction.as_byte());
                break;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(
        passes,
        crate::battle_action::done::DONE_MENU_HOLD_FRAMES as usize,
        "the floor's twelve frames still had to run"
    );
}

/// The Seru-absorb branch re-seeds the countdown (`li v0,0xb4` at
/// `0x801E6134`). Without it the state inherits an expired timer and the
/// action never ends.
#[test]
fn the_seru_absorb_branch_reseeds_its_own_timer_and_then_ends_the_action() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    ctx.absorbed_seru = 1;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::DoneSeruAbsorb.as_byte()
    ));
    assert_eq!(
        ctx.frame_timer,
        crate::battle_action::done::DONE_SERU_ABSORB_FRAMES
    );

    let mut passes = 0;
    loop {
        match step(&mut host, &mut ctx) {
            StepOutcome::Stay => {
                passes += 1;
                assert!(passes < 256, "DoneSeruAbsorb parked on an expired timer");
            }
            StepOutcome::Transition { to, .. } => {
                assert_eq!(to, ActionState::EndOfAction.as_byte());
                break;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(
        passes,
        crate::battle_action::done::DONE_SERU_ABSORB_FRAMES as usize
    );
    assert_eq!(ctx.absorbed_seru, 0, "the gate is consumed");
}

/// The whole Done band is bounded by `ctx[+0x6D8]`, and a settling HP bar
/// only *freezes* the countdown - it never removes the bound. Once the
/// readout has converged the remaining frames run out on schedule.
#[test]
fn a_settling_hp_bar_freezes_the_done_countdown_without_unbounding_it() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 4;
    // Target the acting party slot and leave its readout short of live HP.
    host.actors[0].active_target = 0;
    host.actors[0].hp = 40;
    host.actors[0].hp_display = Some(90);
    for _ in 0..30 {
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    }
    assert_eq!(ctx.frame_timer, 4, "the countdown is frozen, not spent");
    host.actors[0].hp_display = Some(40);
    for _ in 0..4 {
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    }
    assert!(matches!(
        step(&mut host, &mut ctx),
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));
}

// ---------------------------------------------------------------------------
// The Done band's UI teardown - `0x801E614C..0x801E6214`
// ---------------------------------------------------------------------------

/// The teardown is latched by `ctx[+0x17]` and gated on the countdown having
/// fallen below `0xC`, so every element - the level-up banner among them -
/// is unloaded exactly once per action, however many passes the band takes.
#[test]
fn done_band_unloads_the_level_up_banner_exactly_once() {
    let mut host = RecHost::with_n_actors(ACTOR_SLOTS);
    for a in &mut host.actors {
        a.liveness = 1;
    }
    host.actors[0].action_category = ActionCategory::Attack.as_byte();
    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = 0;
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.levelup_banner_element = 0x65;
    ctx.action_ui_element = 7;
    ctx.frame_timer = 4;

    // Four passes, all of them inside `0x51` (the countdown reaches `0` on
    // the fourth and only goes negative on the fifth) - so every unload
    // recorded here is this arm's.
    for _ in 0..4 {
        step(&mut host, &mut ctx);
    }
    assert_eq!(
        ctx.action_state,
        ActionState::DoneFadeDown.as_byte(),
        "the band is still in 0x51"
    );
    let events = host.take();
    let unloads: Vec<u8> = events
        .iter()
        .filter_map(|e| match e {
            Event::Ui(id, UI_UNLOAD) => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(
        unloads,
        vec![7, 0x65, DONE_ACTION_ELEMENT],
        "the action's element, the banner, then the non-Run element - once"
    );
    assert_eq!(ctx.done_ui_torn_down, 1);
}

/// The banner id round-trips: raised by whatever staged it, consumed by the
/// teardown at the id it was staged with, and cleared by the next action seed
/// (`sb zero,0x15(s5)` at `0x801E2CFC`).
#[test]
fn level_up_banner_element_round_trips_through_the_done_band() {
    for element in [0x65u8, 0x12, 0xFF] {
        let mut host = RecHost::with_n_actors(ACTOR_SLOTS);
        for a in &mut host.actors {
            a.liveness = 1;
        }
        host.actors[0].action_category = ActionCategory::Attack.as_byte();
        let mut ctx = BattleActionCtx::new();
        ctx.active_actor = 0;
        ctx.action_state = ActionState::DoneFadeDown.as_byte();
        ctx.levelup_banner_element = element;
        ctx.frame_timer = 2;
        step(&mut host, &mut ctx);
        assert!(
            host.take().contains(&Event::Ui(element, UI_UNLOAD)),
            "element {element:#04x} unloads at the id it was staged with"
        );
        // ...and the next seed clears the byte.
        ctx.action_state = ActionState::ActionSeed.as_byte();
        step(&mut host, &mut ctx);
        assert_eq!(ctx.levelup_banner_element, 0);
    }
}

/// A Run action skips element `0x44` (`lbu v1,0x1de(s3)` / `beq v1,0x5` at
/// `0x801E61F0..0x801E61F8`), and the Spirit latch adds the `0x0F` / `0x52`
/// pair (`0x801E61CC..0x801E61E8`).
#[test]
fn done_band_teardown_honours_the_category_and_spirit_gates() {
    let mut host = RecHost::with_n_actors(ACTOR_SLOTS);
    for a in &mut host.actors {
        a.liveness = 1;
    }
    host.actors[0].action_category = ActionCategory::Run.as_byte();
    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = 0;
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.spirit_action_count = 1;
    ctx.frame_timer = 2;
    step(&mut host, &mut ctx);
    let unloads: Vec<u8> = host
        .take()
        .iter()
        .filter_map(|e| match e {
            Event::Ui(id, UI_UNLOAD) => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(
        unloads,
        vec![DONE_SPIRIT_ELEMENT_A, DONE_SPIRIT_ELEMENT_B],
        "Run drops 0x44, the Spirit latch adds its pair"
    );
}

/// State `0x52`'s press skip (`0x801E639C..0x801E63B8`): at or above the
/// teardown threshold a newly-pressed button pins the countdown at `0x13`;
/// below it the press does nothing.
#[test]
fn seru_absorb_hold_press_clamps_the_wait_to_the_teardown() {
    use crate::battle_action::done::{DONE_SERU_ABSORB_FRAMES, DONE_SERU_ABSORB_TEARDOWN_BELOW};
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneSeruAbsorb.as_byte();
    ctx.frame_timer = DONE_SERU_ABSORB_FRAMES;
    step(&mut host, &mut ctx);
    assert_eq!(
        ctx.frame_timer,
        DONE_SERU_ABSORB_FRAMES - 1,
        "idle pad: plain decrement"
    );
    host.pad_word = 0x40;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.frame_timer, DONE_SERU_ABSORB_TEARDOWN_BELOW - 1);
    step(&mut host, &mut ctx);
    assert_eq!(
        ctx.frame_timer,
        DONE_SERU_ABSORB_TEARDOWN_BELOW - 2,
        "below the threshold a press is a plain decrement"
    );
}
