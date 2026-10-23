//! The run band and the capture band.

use super::*;

#[test]
fn run_begin_sets_timer_and_emits_run_ui() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::RunBegin.as_byte();
    step(&mut host, &mut ctx);
    assert_eq!(ctx.frame_timer, 0x3C);
    assert!(host.take().contains(&Event::Ui(0x43, 0)));
}

#[test]
fn run_begin_successful_escape_floors_downed_party_at_1() {
    // PORT: FUN_801E295C case 0x64 success branch - every party actor
    // with +0x14C == 0 is set to 1 (downed / petrified members leave the
    // battle alive). Monsters are untouched (the retail loop bound is
    // the party count ctx[+0]).
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::RunBegin.as_byte();
    ctx.absorbed_seru = 1; // run roll succeeded
    host.actors[0].liveness = 0;
    host.actors[0].hp = 0;
    host.actors[2].liveness = 0;
    host.actors[4].liveness = 0; // monster stays down
    step(&mut host, &mut ctx);
    assert_eq!(host.actors[0].liveness, 1);
    // Retail's store is ONE halfword (+0x14C = 1), which the port models as
    // both `liveness` and `hp` - a floor that writes only the flag is undone
    // by any driver that re-derives liveness from HP (the live loop's
    // dead-marking pass does, every tick).
    assert_eq!(host.actors[0].hp, 1, "the +0x14C floor covers live HP too");
    assert_eq!(host.actors[2].liveness, 1);
    assert_eq!(host.actors[4].liveness, 0, "monsters are not revived");
    assert_eq!(ctx.absorbed_seru, 1, "outcome gate left for RunWait");
}

#[test]
fn run_begin_failed_run_leaves_downed_party_down() {
    // The retail revive loop lives only in the success branch of case
    // 0x64; a failed run changes no HP.
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::RunBegin.as_byte();
    ctx.absorbed_seru = 0; // run roll failed
    host.actors[0].liveness = 0;
    step(&mut host, &mut ctx);
    assert_eq!(host.actors[0].liveness, 0);
}

#[test]
fn run_wait_failed_run_routes_to_done_cleanup_and_battle_continues() {
    // Retail 0x65 failure branch: the action is consumed (Done band),
    // the battle continues - no battle-end signal.
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::RunWait.as_byte();
    ctx.frame_timer = 0;
    ctx.absorbed_seru = 0; // run roll failed
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::DoneCleanup.as_byte()
    ));
}

#[test]
fn run_wait_escape_routes_to_run_escape_teardown() {
    // Retail 0x65 success branch -> 0x66: the escape teardown ends the
    // battle with the typed Escaped cause (DAT_8007BD71 = 0xFE, no wipe
    // cause byte).
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::RunWait.as_byte();
    ctx.frame_timer = 0;
    ctx.absorbed_seru = 1; // run roll succeeded
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::RunEscape.as_byte()
    ));
    let out = step(&mut host, &mut ctx);
    assert!(matches!(out, StepOutcome::BattleComplete));
    assert!(
        host.take()
            .contains(&Event::BattleEnd(BattleEndCause::Escaped)),
        "escape teardown signals the typed Escaped cause"
    );
}

#[test]
fn capture_start_uses_rng_for_combo_offset() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 5);
    ctx.action_state = ActionState::CaptureStart.as_byte();
    host.rng_seq = vec![1];
    step(&mut host, &mut ctx);
    // combo_timer += 0x780 + 0x80 (since rng%2 == 1) = 0x800 (2048).
    assert_eq!(ctx.combo_timer, 0x780 + 0x80);
    assert_eq!(ctx.frame_timer, 0x1E);
}

#[test]
fn capture_start_takedown_removes_the_monster() {
    // PORT: FUN_801E7824 - the state-0x68 arm zeroes the captured
    // monster's HP pair (+0x172 / +0x14C) and facing (+0x46), bumps the
    // +0x1DC flag byte by 1 (a raw increment, not a bit set), retargets
    // to 8 ("all"), and opens the run-UI banner (FUN_801D8DE8(0x43, 0)).
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 5);
    ctx.action_state = ActionState::CaptureStart.as_byte();
    host.actors[5].hp = 120;
    host.actors[5].hp_display = Some(120);
    host.actors[5].facing_angle = 0x800;
    host.actors[5].flag_bits = ActorFlags(0x02);
    host.actors[5].active_target = 0;
    step(&mut host, &mut ctx);
    let a = &host.actors[5];
    assert_eq!(a.hp, 0, "+0x172 zeroed");
    assert_eq!(a.hp_display, Some(0));
    assert_eq!(a.liveness, 0, "+0x14C zeroed");
    assert_eq!(a.facing_angle, 0, "+0x46 zeroed");
    assert_eq!(a.flag_bits.0, 0x03, "+0x1DC incremented by 1");
    assert_eq!(a.active_target, 8, "+0x1DD = 8");
    assert!(
        host.take().contains(&Event::Ui(0x43, 0)),
        "run banner opened"
    );
}

#[test]
fn capture_wait_marks_capture_state_after_timer() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 5);
    ctx.action_state = ActionState::CaptureWait.as_byte();
    ctx.frame_timer = 0;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::CaptureSustain.as_byte()
    ));
    assert_eq!(host.actors[5].capture_state, 2);
    assert_eq!(host.actors[5].render_flag, 2);
}

// ---------------------------------------------------------------------------
// The capture band's per-frame hold - `0x70` (`0x801E504C..0x801E50E4`)
// ---------------------------------------------------------------------------

/// Retail re-enters `FUN_801F2160` every frame of phase `0x70` and holds on a
/// non-zero return (`jal` at `0x801E50C8`, `bne v0,zero` at `0x801E50D0`).
/// So a module that stages `n` frames keeps the phase for exactly `n` ticks
/// and leaves on the `n + 1`-th - and the tick is entered on every one of
/// them, including the frame it finally reports done.
#[test]
fn capture_phase2_holds_for_exactly_the_modules_staging_frames() {
    for n in [0u32, 1, 5, 40] {
        let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
        ctx.action_state = ActionState::MagicCapturePhase2.as_byte();
        *host.capture_busy_frames.borrow_mut() = n;

        for frame in 0..n {
            step(&mut host, &mut ctx);
            assert_eq!(
                ctx.action_state,
                ActionState::MagicCapturePhase2.as_byte(),
                "n={n} frame={frame}: still staging"
            );
        }
        step(&mut host, &mut ctx);
        assert_eq!(
            ctx.action_state,
            ActionState::MagicCaptureFinalize.as_byte(),
            "n={n}: the zero return advances to 0x71"
        );
        assert_eq!(
            *host.capture_ticks.borrow(),
            n + 1,
            "n={n}: one tick per frame the phase ran"
        );
    }
}

/// `ctx[+0xD] = 1` in the `jal`'s delay slot (`li v0,0x1` at `0x801E50C4`,
/// `sb v0,0xd(v1)` at `0x801E50CC`): the capture is framed from the mirrored
/// side whatever the action seed rolled, and the write happens on every pass,
/// including the ones that hold.
#[test]
fn capture_phase2_pins_the_camera_variant() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCapturePhase2.as_byte();
    ctx.camera_variant = 3;
    *host.capture_busy_frames.borrow_mut() = 2;
    for _ in 0..3 {
        step(&mut host, &mut ctx);
        assert_eq!(ctx.camera_variant, CAPTURE_CAMERA_VARIANT);
    }
}

/// The 75% duck is **gated** on `ctx[+0x287]` (`lbu v0,0x287(v0)` /
/// `beq v0,zero,0x801E50BC` at `0x801E5058`), the same gate state `0x6F`
/// runs. The port used to duck unconditionally.
#[test]
fn capture_phase2_ducks_only_behind_the_counter_flag() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCapturePhase2.as_byte();
    ctx.scripted_fight = 0;
    step(&mut host, &mut ctx);
    assert!(
        !host
            .take()
            .iter()
            .any(|e| matches!(e, Event::Brightness(_))),
        "flag clear: no ramp"
    );

    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCapturePhase2.as_byte();
    ctx.scripted_fight = 1;
    step(&mut host, &mut ctx);
    assert!(
        host.take().contains(&Event::Brightness(75)),
        "flag set: the 75% ramp"
    );
}

// ---------------------------------------------------------------------------
// The capture band's camera - `0x6F` ramp (`0x801E4FFC..0x801E5020`) and the
// `0x70` -> `0x71` re-seed (`0x801E50DC`)
// ---------------------------------------------------------------------------

/// The `0x6F` hold does two things per frame the port used to drop: it ramps
/// `ctx[+0x6D0]` down by `frame_scalar * 16` (`lhu`/`subu`/`sh` at
/// `0x801E4FFC..0x801E5014`) and re-arms the framing program for the acting
/// slot (`FUN_801D5854(ctx[+0x13], 6)` at `0x801E5018`). Both are inside the
/// hold, so they repeat for as long as `FUN_8003F2B8(1)` reports busy.
#[test]
fn capture_fade_ramps_the_camera_and_rearms_the_framing_every_frame() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCaptureFade.as_byte();
    ctx.camera_frame_height = crate::battle_formulas::CAMERA_HEIGHT_MIN;
    host.prev_cleared = false;

    for frame in 1..=4i16 {
        step(&mut host, &mut ctx);
        assert_eq!(
            ctx.action_state,
            ActionState::MagicCaptureFade.as_byte(),
            "the hold holds while the previous action has not cleared"
        );
        assert_eq!(
            ctx.camera_frame_height,
            crate::battle_formulas::CAMERA_HEIGHT_MIN - frame * CAPTURE_FADE_CAMERA_STEP,
            "frame {frame}: 16 units per frame, no floor"
        );
        let events = host.take();
        assert!(
            events.contains(&Event::Pose(1, Pose::Idle)),
            "frame {frame}: the framing program is re-armed"
        );
    }
}

/// The `0x71` store's own `jal 0x801f0348` (`0x801E50DC`, whose delay slot is
/// the state write) puts the framing back where the size class says it
/// belongs. Without it the pull-in above would leak into the rest of the
/// action.
#[test]
fn capture_phase2_reseeds_the_camera_on_the_way_to_0x71() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCapturePhase2.as_byte();
    // Wherever the 0x6F ramp left it - deliberately not the seed value.
    ctx.camera_frame_height = 0x0100;
    *host.capture_busy_frames.borrow_mut() = 2;

    // The holding frames leave it alone: retail's re-seed is in the exit's
    // delay slot, not in the body.
    for _ in 0..2 {
        step(&mut host, &mut ctx);
        assert_eq!(
            ctx.camera_frame_height, 0x0100,
            "held frames do not re-seed"
        );
    }
    step(&mut host, &mut ctx);
    assert_eq!(
        ctx.action_state,
        ActionState::MagicCaptureFinalize.as_byte()
    );
    assert_eq!(
        ctx.camera_frame_height,
        crate::battle_formulas::CAMERA_HEIGHT_MIN,
        "the exit re-runs FUN_801F0348; a party caster frames at the floor"
    );
}

// ---------------------------------------------------------------------------
// The `0x51` teardown's tail sweep - `0x801E6218..0x801E6368`
// ---------------------------------------------------------------------------

/// `ctx[+0x269]` non-zero **raises** `0x59` (`a1 = 0` at `0x801E6244`), and
/// the target-banner close is gated on the acting actor's own two bytes:
/// `+0x1DD` in `3..=7` and `+0x1DE` in `1..=3`.
///
/// The window matters and it is narrow. `ctx[+0x269]` non-zero on the *exit*
/// pass re-seeds the countdown to `0xB4` before the teardown reloads it
/// (`sh v0,0x2(s7)` at `0x801E6138`, reload at `0x801E614C`), which fails the
/// `< 0xC` gate - so the only frames that see both a live capture byte and a
/// running teardown are the last twelve of the countdown, before it goes
/// negative. The tests below sit in that window deliberately.
#[test]
fn the_done_band_sweep_raises_the_capture_banner_and_closes_the_target_one() {
    // Category 2 (Magic), target slot 3: both gates pass, and a non-zero
    // capture byte raises 0x59.
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 5;
    ctx.absorbed_seru = 0x8B;
    host.actors[1].active_target = 3;
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    let events = host.take();
    assert!(
        events.contains(&Event::Ui(0x59, 0)),
        "the capture banner is RAISED, not unloaded: {events:?}"
    );
    assert!(
        events.contains(&Event::Ui(0x51, 1)),
        "the target banner is closed: {events:?}"
    );

    // A party-wide target (8) fails the slot gate, and a zero capture byte
    // raises nothing - while the rest of the teardown still runs, so this is
    // not vacuously asserting that nothing happened.
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 5;
    ctx.absorbed_seru = 0;
    host.actors[1].active_target = 8;
    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(
        events.contains(&Event::Ui(0x44, 1)),
        "the teardown block itself ran: {events:?}"
    );
    assert!(!events.contains(&Event::Ui(0x59, 0)), "{events:?}");
    assert!(!events.contains(&Event::Ui(0x51, 1)), "{events:?}");

    // The tail is behind the same latch as the block above it: the next pass
    // of the band re-runs neither.
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 5;
    ctx.absorbed_seru = 0x8B;
    host.actors[1].active_target = 3;
    step(&mut host, &mut ctx);
    let _ = host.take();
    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(
        !events.contains(&Event::Ui(0x59, 0)),
        "the latch makes the tail once-per-action: {events:?}"
    );
    assert!(!events.contains(&Event::Ui(0x51, 1)), "{events:?}");
}

/// The raise has a close. The `0x52` band's own tail
/// (`0x801E63F4..0x801E6424`) unloads `0x59` behind the same `ctx[+0x17]`
/// latch the `0x51` block set, and clears it - so a capture that raised the
/// banner in the fade-down cannot leave it standing.
#[test]
fn the_seru_absorb_band_closes_the_capture_banner_and_clears_the_latch() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::DoneSeruAbsorb.as_byte();
    ctx.frame_timer = 4;
    ctx.done_ui_torn_down = 1;
    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(
        events.contains(&Event::Ui(0x59, 1)),
        "the capture banner is closed: {events:?}"
    );
    assert_eq!(ctx.done_ui_torn_down, 0, "and the latch is cleared");

    // Once only: the next pass finds the latch clear.
    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(!events.contains(&Event::Ui(0x59, 1)), "{events:?}");

    // Above the window the tail does not run at all, even with the latch set.
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::DoneSeruAbsorb.as_byte();
    ctx.frame_timer = 0x40;
    ctx.done_ui_torn_down = 1;
    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(!events.contains(&Event::Ui(0x59, 1)), "{events:?}");
    assert_eq!(ctx.done_ui_torn_down, 1);
}

/// State `0x65`'s press skip (`0x801E5978..0x801E59A0`): a failed run's
/// wait ends on the press's own pass; a successful escape's ignores it.
#[test]
fn run_wait_press_skips_only_the_failed_run_message() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::RunWait.as_byte();
    ctx.frame_timer = 0x3C;
    ctx.absorbed_seru = 0; // run roll failed
    host.pad_word = 0x40;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::DoneCleanup.as_byte()
    ));

    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::RunWait.as_byte();
    ctx.frame_timer = 0x3C;
    ctx.absorbed_seru = 1; // escaped
    host.pad_word = 0x40;
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    assert_eq!(ctx.frame_timer, 0x3B);
}

// ---------------------------------------------------------------------------
// The capture band's settle test - `0x71` (`0x801E50FC..0x801E5200`)
// ---------------------------------------------------------------------------

/// A monster caster on slot 3 casting at `target`, in state `0x71`.
fn finalize_at(target: u8) -> (BattleActionCtx, RecHost) {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 3);
    ctx.action_state = ActionState::MagicCaptureFinalize.as_byte();
    host.actors[3].active_target = target;
    (ctx, host)
}

fn finalized(ctx: &BattleActionCtx) -> bool {
    ctx.action_state == ActionState::DoneCleanup.as_byte()
}

/// Neither arm reads the caster: a boss still on its cast clip (Cort's `8`
/// through Guilty Cross) does not hold the band once the seat it hit has
/// settled. The port used to wait for every live seat to leave clip `8`,
/// which held each such cast in `0x71` for the length of the caster's clip.
#[test]
fn capture_finalize_does_not_wait_out_the_casters_clip() {
    for target in [0u8, 8] {
        let (mut ctx, mut host) = finalize_at(target);
        host.actors[3].current_anim = 8;
        step(&mut host, &mut ctx);
        assert!(finalized(&ctx), "target {target}: the caster is not read");
    }
}

/// `target < 7`: that seat alone. A live seat holds on any clip but `0`.
#[test]
fn capture_finalize_single_target_waits_for_the_target_to_go_idle() {
    let (mut ctx, mut host) = finalize_at(1);
    host.actors[1].current_anim = 0x0C;
    host.actors[2].current_anim = 0x0C; // not the target: not read
    for _ in 0..3 {
        step(&mut host, &mut ctx);
        assert!(!finalized(&ctx), "the target is still on its reaction");
    }
    host.actors[1].current_anim = 0;
    step(&mut host, &mut ctx);
    assert!(finalized(&ctx));
}

/// A dead seat: clip `0` or the down clip `8` has settled; otherwise a party
/// seat holds and a monster seat holds until its prim word reads `0`.
#[test]
fn capture_finalize_single_target_dead_seat_rules() {
    // Dead party seat mid-knockdown, then on the down clip.
    let (mut ctx, mut host) = finalize_at(0);
    host.actors[0].liveness = 0;
    host.actors[0].current_anim = 7;
    host.actors[0].render_color = 0;
    step(&mut host, &mut ctx);
    assert!(!finalized(&ctx), "a dead party seat waits for clip 8");
    host.actors[0].current_anim = CAPTURE_SETTLE_DOWN_CLIP;
    step(&mut host, &mut ctx);
    assert!(finalized(&ctx));

    // Dead monster seat on its knockdown: settled once the fade ran out.
    let (mut ctx, mut host) = finalize_at(4);
    host.actors[4].liveness = 0;
    host.actors[4].current_anim = 5;
    host.actors[4].render_color = 0x0040_1004;
    step(&mut host, &mut ctx);
    assert!(!finalized(&ctx), "the defeat fade is still drawing");
    host.actors[4].render_color = 0;
    step(&mut host, &mut ctx);
    assert!(finalized(&ctx));
}

/// `target >= 7`: party seats `0 .. ctx[+0x00]`, live ones on clip `0`, dead
/// ones on clip `8` - a dead seat back on clip `0` holds here, unlike the
/// single-target arm. Monster seats are not read.
#[test]
fn capture_finalize_party_wide_walks_the_party_row_only() {
    let (mut ctx, mut host) = finalize_at(8);
    host.actors[5].current_anim = 0x0C; // a monster mid-clip: not read
    host.actors[2].liveness = 0;
    host.actors[2].current_anim = 0;
    step(&mut host, &mut ctx);
    assert!(!finalized(&ctx), "a dead party seat on clip 0 holds");
    host.actors[2].current_anim = CAPTURE_SETTLE_DOWN_CLIP;
    step(&mut host, &mut ctx);
    assert!(finalized(&ctx));

    // Slot 7 is not a single-target code (`sltiu v0,s0,0x7`).
    let (mut ctx, mut host) = finalize_at(7);
    host.actors[7].current_anim = 0x0C;
    host.actors[0].current_anim = 0x0C;
    step(&mut host, &mut ctx);
    assert!(!finalized(&ctx), "target 7 walks the party row");
    host.actors[0].current_anim = 0;
    step(&mut host, &mut ctx);
    assert!(finalized(&ctx), "slot 7's own clip is not read");
}

/// The port bounds the unbounded retail wait.
#[test]
fn capture_finalize_lets_go_after_the_pass_limit() {
    let (mut ctx, mut host) = finalize_at(0);
    host.actors[0].current_anim = 0x0C;
    for _ in 0..CAPTURE_SETTLE_PASS_LIMIT {
        step(&mut host, &mut ctx);
        assert!(!finalized(&ctx));
    }
    step(&mut host, &mut ctx);
    assert!(finalized(&ctx));
    assert_eq!(ctx.capture_settle_passes, 0, "the count resets on the exit");
}

/// The exit walk (`0x801E524C..0x801E5294`) clears `+0x21C` on seats `0..7`
/// that are alive or party seats: a dead monster keeps its defeat-fade state.
#[test]
fn capture_finalize_exit_keeps_a_dead_monsters_fade_state() {
    let (mut ctx, mut host) = finalize_at(0);
    for a in &mut host.actors {
        a.render_flag = 2;
    }
    host.actors[4].liveness = 0;
    host.actors[1].liveness = 0;
    host.actors[1].current_anim = CAPTURE_SETTLE_DOWN_CLIP;
    step(&mut host, &mut ctx);
    assert!(finalized(&ctx));
    assert_eq!(host.actors[4].render_flag, 2, "dead monster: untouched");
    assert_eq!(host.actors[1].render_flag, 0, "dead party seat: cleared");
    assert_eq!(host.actors[5].render_flag, 0, "live monster: cleared");
    assert_eq!(host.actors[7].render_flag, 2, "slot 7 is outside the walk");
}
