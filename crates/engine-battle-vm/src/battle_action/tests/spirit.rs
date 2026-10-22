//! The spirit band, its latch and the gauge-extend class.

use super::*;

/// The `0x3C` pre-arm raises the action-in-progress marker `ctx[+0x243]`
/// on every category (`sb v0,0x243(v1)` at `0x801E3B6C`), Item included.
#[test]
fn the_pre_arm_raises_the_action_marker() {
    for category in [ActionCategory::Spirit, ActionCategory::Item] {
        let (mut ctx, mut host) = fresh(category, 0);
        ctx.action_state = ActionState::SpiritPreArm.as_byte();
        step(&mut host, &mut ctx);
        assert_eq!(ctx.action_state, ActionState::SpiritWait.as_byte());
        assert_eq!(ctx.gauge_rearm_latch, 1, "{category:?}");
    }
}

// ---------------------------------------------------------------------------
// The Spirit seed arm - `0x801E2FF0..0x801E3024`
// ---------------------------------------------------------------------------

/// The Spirit arm bumps `ctx[+0x19]` (in the `jal`'s delay slot) and folds
/// its draw to `(rand % 2) * 2`, so a Spirit action is framed at variant `0`
/// or `2` and never from the mirrored side.
#[test]
fn spirit_seed_bumps_the_latch_and_folds_its_draw() {
    for (draw, expect) in [(0u32, 0u8), (1, 2), (4, 0), (7, 2)] {
        let (mut ctx, mut host) = fresh(ActionCategory::Spirit, 1);
        ctx.action_state = ActionState::ActionSeed.as_byte();
        host.rng_seq = vec![3, draw];
        step(&mut host, &mut ctx);
        assert_eq!(ctx.action_state, ActionState::SpiritArtsEntry.as_byte());
        assert_eq!(ctx.camera_variant, expect, "draw {draw}");
        assert_eq!(ctx.spirit_action_count, 1);
    }
}

/// The spirit band stages the committed clip, holds `0x20` steps, waits on
/// the clip, then spends the `0x300` flush hold on the flush's first pass
/// and leaves for the Done band - and a timer that went negative earlier
/// never strands it (the hold is level-triggered, not an edge).
#[test]
fn the_spirit_band_holds_its_clip_and_both_timers() {
    use crate::battle_action::spirit::SPIRIT_BAND_HOLD;
    let (mut ctx, mut host) = fresh(ActionCategory::Spirit, 0);
    host.actors[0].queued_anim_b = 0x10;
    ctx.frame_timer = -5;
    ctx.action_state = ActionState::SpiritArtsEntry.as_byte();
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::SpiritArtsSustain.as_byte());
    assert_eq!(
        host.actors[0].queued_anim, 0x10,
        "the committed clip is staged"
    );
    // The anim system commits the clip.
    host.actors[0].current_anim = 0x10;
    let mut steps = 0;
    while ctx.action_state == ActionState::SpiritArtsSustain.as_byte() && steps < 200 {
        step(&mut host, &mut ctx);
        steps += 1;
        if steps == 40 {
            // The clip ends: the flag clears and the pair settles on idle.
            host.actors[0].flag_bits = ActorFlags(0);
            host.actors[0].current_anim = 0;
        }
    }
    assert!(
        steps > SPIRIT_BAND_HOLD as usize,
        "held for the clip, {steps}"
    );
    assert_eq!(ctx.action_state, ActionState::SpiritArtsFlush.as_byte());
    // Only the plate's climb holds the flush now: the hold is spent at once.
    let climb = (ctx.hp_bar_target - ctx.spirit_plate_value).max(0) as usize;
    step(&mut host, &mut ctx);
    assert!(
        ctx.frame_timer <= 0,
        "the 0x300 hold is spent the first pass"
    );
    for _ in 0..climb {
        if ctx.action_state != ActionState::SpiritArtsFlush.as_byte() {
            break;
        }
        step(&mut host, &mut ctx);
    }
    assert_eq!(ctx.action_state, ActionState::DoneCleanup.as_byte());
}

/// `0x48`'s compare runs the other way from a drain: a timer at or above
/// `8 * step` is zeroed (`beq v0,zero,0x801E5760` -> `sh zero,0x2(s7)`), and
/// only one below it is stepped down, past zero.
#[test]
fn the_flush_spends_its_hold_on_the_first_pass() {
    use crate::battle_action::spirit::{SPIRIT_FLUSH_HOLD, spirit_flush_timer};
    assert_eq!(spirit_flush_timer(SPIRIT_FLUSH_HOLD, 1), 0);
    assert_eq!(spirit_flush_timer(SPIRIT_FLUSH_HOLD, 2), 0);
    assert_eq!(spirit_flush_timer(16, 2), 0);
    assert_eq!(spirit_flush_timer(15, 2), -1);
    assert_eq!(spirit_flush_timer(0, 2), 0);
    assert_eq!(spirit_flush_timer(-3, 2), -3);
}

/// A Spirit turn's whole presentation, seed to Done: the seed raises the AP
/// bar (`0x0F`) and plate (`0x52`) sized off the live AGL, `0x46` pulls the
/// camera in to `0x800` and stages the extended gauge and the `+0x20` Spirit
/// target, the sustain grows the bar one step a frame to the extended gauge
/// less 6, the plate climbs onto its target, and the Done band pays the
/// `+0x20` into the gauge. The live values are a retail capture's: Noa at a
/// 194 AGL reads a 188-wide bar under a `ctx[+0x6DC]` of 279.
#[test]
fn a_spirit_turn_grows_the_ap_bar_and_climbs_the_plate() {
    let (mut ctx, mut host) = fresh(ActionCategory::Spirit, 0);
    host.actors[0].agl = 194;
    host.actors[0].agl_base = 194;
    host.actors[0].spirit_gauge = 40;
    host.actors[0].queued_anim_b = 0x10;
    ctx.action_state = ActionState::ActionSeed.as_byte();
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::SpiritArtsEntry.as_byte());
    let ui = host.take();
    assert!(ui.contains(&Event::Ui(0x0F, 0)), "the AP bar is raised");
    assert!(ui.contains(&Event::Ui(0x52, 0)), "the AP plate is raised");
    assert_eq!(ctx.spirit_bar_width, 188, "AGL less 6");
    assert_eq!(ctx.spirit_plate_value, 40, "the plate opens on the gauge");

    step(&mut host, &mut ctx);
    assert_eq!(ctx.camera_frame_height, 0x800, "the Spirit close-up");
    assert_eq!(ctx.damage_target, 279, "194 * 7 / 5 + 8");
    assert_eq!(ctx.hp_bar_target, 40 + 0x20);

    host.actors[0].current_anim = 0x10;
    let mut widths = vec![ctx.spirit_bar_width];
    let mut steps = 0;
    while ctx.action_state != ActionState::DoneCleanup.as_byte() && steps < 2000 {
        if steps == 150 {
            host.actors[0].flag_bits = ActorFlags(0);
            host.actors[0].current_anim = 0;
        }
        step(&mut host, &mut ctx);
        widths.push(ctx.spirit_bar_width);
        steps += 1;
    }
    assert_eq!(ctx.action_state, ActionState::DoneCleanup.as_byte());
    assert_eq!(ctx.spirit_bar_width, 279 - 6, "grown to the extended gauge");
    assert!(
        widths.windows(2).all(|w| w[1] >= w[0]),
        "the bar only grows"
    );
    assert_eq!(
        ctx.spirit_plate_value,
        40 + 0x20,
        "the plate sits on target"
    );

    step(&mut host, &mut ctx);
    assert_eq!(
        host.actors[0].spirit_gauge,
        40 + 0x20,
        "Done pays the +0x20"
    );
}

/// Every action ends `+8` Spirit through the Done accumulator, with the two
/// "spirit gain up" passives on top for a party seat, capped at 100.
#[test]
fn the_done_band_pays_the_accumulator_into_the_spirit_gauge() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    host.actors[0].spirit_gauge = 10;
    ctx.action_state = ActionState::DoneCleanup.as_byte();
    step(&mut host, &mut ctx);
    assert_eq!(host.actors[0].spirit_gauge, 18);

    let (mut ctx, mut host) = fresh(ActionCategory::Spirit, 0);
    host.actors[0].spirit_gauge = 10;
    host.ability_bits_high.insert(0, 0x300);
    ctx.action_state = ActionState::DoneCleanup.as_byte();
    step(&mut host, &mut ctx);
    assert_eq!(host.actors[0].spirit_gauge, 10 + 8 + 3 + 0x20);

    let (mut ctx, mut host) = fresh(ActionCategory::Spirit, 0);
    host.actors[0].spirit_gauge = 90;
    ctx.action_state = ActionState::DoneCleanup.as_byte();
    step(&mut host, &mut ctx);
    assert_eq!(host.actors[0].spirit_gauge, 100);
}

/// Nothing in the dispatcher clears `ctx[+0x19]`, so it is a per-battle latch
/// and a second Spirit action counts on top of the first.
#[test]
fn spirit_latch_accumulates_across_actions() {
    let (mut ctx, mut host) = fresh(ActionCategory::Spirit, 1);
    for n in 1..=3u8 {
        ctx.action_state = ActionState::ActionSeed.as_byte();
        step(&mut host, &mut ctx);
        assert_eq!(ctx.spirit_action_count, n);
    }
}

/// State `0x3E`'s class-5 arm (`0x801E3E80..0x801E4018`): the gauge-extension
/// item raises the two gauge elements, bumps their teardown latch, draws the
/// camera variant `(rand % 2) * 2` and stages the extended gauge and the
/// spirit target. Any other class draws nothing here.
#[test]
fn the_gauge_extend_class_draws_its_camera_variant_in_spirit_fire() {
    let (mut ctx, mut host) = fresh(ActionCategory::Item, 0);
    ctx.action_state = ActionState::SpiritFire.as_byte();
    host.actors[0].cast_class = 5;
    host.actors[0].active_target = 1;
    host.actors[0].spirit_gauge = 95;
    host.actors[1].agl_base = 150;
    host.rng_seq = vec![7];
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::SpiritFireDamage.as_byte());
    assert_eq!(*host.rng_pos.borrow(), 1, "one draw");
    assert_eq!(ctx.camera_variant, 2, "(7 % 2) * 2");
    assert_eq!(ctx.spirit_action_count, 1);
    assert_eq!(ctx.damage_target, 150 * 7 / 5 + 8);
    assert_eq!(ctx.hp_bar_target, 100, "95 + 8 capped at 100");
    let events = host.take();
    assert!(events.contains(&Event::Ui(0x0F, 0)));
    assert!(events.contains(&Event::Ui(0x52, 0)));

    // A big gauge caps at 0x120; another class draws nothing.
    let (mut ctx, mut host) = fresh(ActionCategory::Item, 0);
    ctx.action_state = ActionState::SpiritFire.as_byte();
    host.actors[0].cast_class = 5;
    host.actors[0].active_target = 1;
    host.actors[1].agl_base = 400;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.damage_target, 0x120);
    let (mut ctx, mut host) = fresh(ActionCategory::Item, 0);
    ctx.action_state = ActionState::SpiritFire.as_byte();
    host.actors[0].cast_class = 0;
    host.rng_seq = vec![7];
    step(&mut host, &mut ctx);
    assert_eq!(*host.rng_pos.borrow(), 0, "no draw off class 5");
}
