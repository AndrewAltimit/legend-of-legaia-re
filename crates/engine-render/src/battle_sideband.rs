//! Battle **sideband tick** - the once-per-frame pass that runs the battle
//! intro sequence, holds the pad during a scene change, and ramps the two
//! camera-distance registers.
//!
//! PORT: FUN_80056208
//!
//! NOT WIRED: the pass is a state machine over the battle context
//! (`_DAT_8007BD24`) and four global registers, none of which this crate owns.
//! Its three inputs are the sideband submode byte `DAT_8007B64A`, the phase
//! counter `ctx[+0x289]` and the frame step `DAT_1F800393`; its outputs are the
//! camera registers `0x800840BC` / `0x800840C0`, the hold flag `ctx[+0x6B0]`
//! and three timers. The engine's battle camera is
//! `legaia_engine_core`'s orbit controller driven from the battle-action SM, and
//! its pad state is `engine-core`'s `retail_pad` - both outside this lane's file
//! scope. [`battle_sideband_tick`] is a pure transition function so wiring it is
//! a matter of the battle host owning a [`BattleSidebandState`] and applying the
//! returned [`BattleSidebandEffects`]; nothing does today.
//!
//! This also settles what the address is: it is **not** libgpu-band vendor
//! infrastructure despite sitting between the PsyQ veneers. It reads the game's
//! own battle context, dispatches into three battle overlay hooks, and points a
//! caption pointer at a game string.
//!
//! REF: FUN_801d8de8 - battle UI-element dispatcher (the intro caption).
//! REF: FUN_801d829c - camera-state per-actor transform builder (the intro
//! camera aim).
//! REF: FUN_800355f0 - 2D floating-element list teardown, run as the intro's
//! phase-1 exit.
//! REF: FUN_80025358 - gated sub-overlay load sequencer, ticked by phase 3.
//! REF: FUN_8003de7c - CD read-idle poll, gating the outro hook.
//!
//! # Three submodes
//!
//! `DAT_8007B64A` selects, and only `1` and `2` publish a hold:
//!
//! | submode | role |
//! |---|---|
//! | `1` | the intro sequence, four phases on `ctx[+0x289]` |
//! | `2` | in-battle: pad clear + camera pull-back ramp, or the overlay tick |
//! | `3` | outro: wait out `ctx[+0x6D8]`, then the overlay hook once the CD is idle |
//!
//! Everything else falls straight through to the tail, which always writes
//! `ctx[+0x6B0]` - so the *hold* flag is published unconditionally and is `1`
//! only for intro phases `0` and `1`.
//!
//! # The intro's phase 1 wait is cancellable
//!
//! Phase 1 decays `ctx[+0x6AE]` by `8 * frame_step` per frame, and any pad edge
//! (`_DAT_8007B874 | _DAT_8007B938`, masked to 16 bits) zeroes it outright - so
//! a button press skips the caption. The advance to phase 2 is then further
//! gated on the effect-VM ready flag reading `0xFF`: while it does not, the
//! timer is pinned at `1` and the phase holds.
//!
//! # The camera ramp is cadence-invariant
//!
//! Submode 2's ramp adds `4 * frame_step` to `0x800840BC` and `14 * frame_step`
//! to `0x800840C0` per frame, capped by testing `0x800840BC < 0xC00` *before*
//! the add - so the register can overshoot the cap by one step. Phase 3 of the
//! intro pushes `0x800840C0` alone, by `8 * frame_step`.
//!
//! Source: `ghidra/scripts/funcs/80056208.txt` (disassembly).

/// Sideband submode: the battle intro sequence.
pub const SUBMODE_INTRO: u8 = 1;
/// Sideband submode: in-battle.
pub const SUBMODE_IN_BATTLE: u8 = 2;
/// Sideband submode: the battle outro.
pub const SUBMODE_OUTRO: u8 = 3;

/// Effect-VM ready flag value that releases the intro's phase-1 gate.
pub const EFFECT_VM_READY: u8 = 0xFF;
/// Effect-VM ready threshold above which submode 2 delegates to the overlay
/// tick instead of running its own ramp.
pub const OVERLAY_TICK_FROM: u8 = 0x12;

/// Battle-context phase byte value that arms the intro (`ctx[+6]`).
pub const INTRO_ARM_MODE: u8 = 0x14;
/// Battle-context phase byte value the camera ramp requires (`ctx[+6]`).
pub const RAMP_MODE: u8 = 0x0C;

/// Caption timer seeded when the intro arms (`ctx[+0x6AE]`).
pub const INTRO_CAPTION_FRAMES: i16 = 0x0B40;
/// Per-frame decay multiplier on that timer.
pub const INTRO_CAPTION_DECAY: i32 = 8;
/// UI element id the intro caption dispatches.
pub const INTRO_CAPTION_UI_ELEMENT: u16 = 0x5A;
/// `ctx[+0x1B]` / `ctx[+0x1C]` the intro stamps alongside the caption.
pub const INTRO_CTX_STAMP: (u8, u8) = (1, 0x10);
/// Camera-aim seat the intro's transform builder reads (`DAT_801C9370 + 0xC`,
/// i.e. actor-table slot 3 - the first monster).
pub const INTRO_CAMERA_SEAT: usize = 3;
/// The two constant terms of the intro camera vector.
pub const INTRO_CAMERA_CONSTS: (i16, i16) = (0x500, 0x400);
/// Mode argument the intro passes to the transform builder.
pub const INTRO_CAMERA_MODE: u16 = 0xC;

/// Cap tested on `0x800840BC` *before* the ramp step is added.
pub const CAMERA_RAMP_CAP: i32 = 0xC00;
/// Per-frame multiplier applied to `0x800840BC`.
pub const CAMERA_RAMP_A: i32 = 4;
/// Per-frame multiplier applied to `0x800840C0` alongside it.
pub const CAMERA_RAMP_B: i32 = 14;
/// Per-frame multiplier phase 3 applies to `0x800840C0` on its own.
pub const PHASE3_RAMP_B: i32 = 8;

/// The mutable state the tick owns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BattleSidebandState {
    /// `ctx + 0x289` - the intro phase counter.
    pub phase: u8,
    /// `ctx + 0x6AE` - the intro caption timer.
    pub caption_timer: i16,
    /// `ctx + 0x6CE` - a phase-3 accumulator.
    pub phase3_accum: u16,
    /// `ctx + 0x6D6` - the delay submode 2 burns before it starts ramping.
    pub ramp_delay: i16,
    /// `ctx + 0x6D8` - the outro wait timer.
    pub outro_timer: i16,
    /// `0x800840BC` - camera register A.
    pub camera_a: i32,
    /// `0x800840C0` - camera register B.
    pub camera_b: i32,
    /// `ctx + 0x6B0` - the hold flag the battle SM reads.
    pub hold: u16,
}

/// The read-only inputs.
#[derive(Debug, Clone, Copy, Default)]
pub struct BattleSidebandInputs {
    /// `DAT_8007B64A` - the submode selector.
    pub submode: u8,
    /// `DAT_1F800393` - the adaptive frame step, in vsyncs.
    pub frame_step: u8,
    /// `ctx + 6` - the battle context's own phase byte.
    pub ctx_mode: u8,
    /// `DAT_8007BD71` - the effect-VM ready flag.
    pub effect_vm_ready: u8,
    /// `_DAT_8007B874 | _DAT_8007B938` masked to 16 bits - any pad edge.
    pub pad_edge: bool,
    /// `DAT_8007B648 < 0` - a scene transition is in flight.
    pub scene_changing: bool,
    /// `FUN_8003DE7C(1) == 0` - the CD is idle.
    pub cd_idle: bool,
}

/// A call or store the tick asks the host to make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleSidebandEffect {
    /// Arm the intro: stamp `ctx[+0x1B]`/`ctx[+0x1C]`, point the caption
    /// pointer at the tutorial string and dispatch UI element `0x5A`.
    IntroCaption,
    /// Aim the intro camera at [`INTRO_CAMERA_SEAT`] through
    /// `FUN_801D829C`, negating that seat's world X / Z.
    IntroCameraAim,
    /// `FUN_800355F0` - drain the 2D floating-element list.
    DrainFloatingElements,
    /// `FUN_801F6B70` - intro phase-2 overlay hook.
    IntroOverlayHook,
    /// `FUN_80025358` - tick the gated sub-overlay loader; its return lands in
    /// `ctx[+0xB]`.
    SubOverlayTick,
    /// `FUN_801F69F4` - in-battle overlay tick (taken instead of the ramp once
    /// the effect VM is live).
    InBattleOverlayTick,
    /// `FUN_801F69D8` - outro overlay hook.
    OutroOverlayHook,
    /// Clear the pad masks and `ctx[+0x884]` - the in-battle input hold.
    ClearPadState,
}

/// Result of one tick.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BattleSidebandEffects {
    /// Calls / stores in retail order.
    pub effects: Vec<BattleSidebandEffect>,
}

/// Run one sideband frame, mutating `state` and returning what the host must do.
pub fn battle_sideband_tick(
    state: &mut BattleSidebandState,
    inputs: &BattleSidebandInputs,
) -> BattleSidebandEffects {
    let mut out = BattleSidebandEffects::default();
    let step = i32::from(inputs.frame_step);
    // The tail publishes 1 only for intro phases 0 and 1.
    let mut hold: u16 = 0;

    match inputs.submode {
        SUBMODE_INTRO => match state.phase {
            0 => {
                if inputs.ctx_mode == INTRO_ARM_MODE {
                    hold = 1;
                    state.phase = state.phase.wrapping_add(1);
                    state.caption_timer = INTRO_CAPTION_FRAMES;
                    out.effects.push(BattleSidebandEffect::IntroCaption);
                    out.effects.push(BattleSidebandEffect::IntroCameraAim);
                }
            }
            1 => {
                hold = 1;
                state.caption_timer = state
                    .caption_timer
                    .wrapping_sub((INTRO_CAPTION_DECAY * step) as i16);
                if inputs.pad_edge {
                    state.caption_timer = 0;
                }
                if inputs.effect_vm_ready != EFFECT_VM_READY {
                    // Pin at 1 and hold this phase.
                    if state.caption_timer <= 0 {
                        state.caption_timer = 1;
                    }
                    state.hold = hold;
                    return out;
                }
                if state.caption_timer > 0 {
                    state.hold = hold;
                    return out;
                }
                state.caption_timer = 1;
                state.phase = state.phase.wrapping_add(1);
                out.effects
                    .push(BattleSidebandEffect::DrainFloatingElements);
            }
            2 => out.effects.push(BattleSidebandEffect::IntroOverlayHook),
            3 => {
                out.effects.push(BattleSidebandEffect::SubOverlayTick);
                state.phase3_accum = state
                    .phase3_accum
                    .wrapping_add(u16::from(inputs.frame_step));
                state.camera_b = state.camera_b.wrapping_add(PHASE3_RAMP_B * step);
            }
            _ => {}
        },
        SUBMODE_IN_BATTLE => {
            if inputs.effect_vm_ready >= OVERLAY_TICK_FROM {
                out.effects.push(BattleSidebandEffect::InBattleOverlayTick);
            } else {
                out.effects.push(BattleSidebandEffect::ClearPadState);
                if inputs.scene_changing && inputs.ctx_mode == RAMP_MODE {
                    if state.ramp_delay > 0 {
                        state.ramp_delay = state
                            .ramp_delay
                            .wrapping_sub((INTRO_CAPTION_DECAY * step) as i16);
                    } else {
                        state.ramp_delay = 0;
                        if state.camera_a < CAMERA_RAMP_CAP {
                            state.camera_a = state.camera_a.wrapping_add(CAMERA_RAMP_A * step);
                            state.camera_b = state.camera_b.wrapping_add(CAMERA_RAMP_B * step);
                        }
                    }
                }
            }
        }
        SUBMODE_OUTRO => {
            let mut expired = true;
            if state.outro_timer > 0 {
                state.outro_timer = state.outro_timer.wrapping_sub(i16::from(inputs.frame_step));
                expired = state.outro_timer <= 0;
            }
            if expired && inputs.cd_idle {
                out.effects.push(BattleSidebandEffect::OutroOverlayHook);
            }
        }
        _ => {}
    }

    state.hold = hold;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(submode: u8) -> BattleSidebandInputs {
        BattleSidebandInputs {
            submode,
            frame_step: 1,
            ctx_mode: 0,
            effect_vm_ready: EFFECT_VM_READY,
            pad_edge: false,
            scene_changing: false,
            cd_idle: true,
        }
    }

    #[test]
    fn phase_zero_needs_the_arm_mode_byte() {
        let mut s = BattleSidebandState::default();
        let mut i = inputs(SUBMODE_INTRO);
        assert!(battle_sideband_tick(&mut s, &i).effects.is_empty());
        assert_eq!(s.phase, 0);
        assert_eq!(s.hold, 0);

        i.ctx_mode = INTRO_ARM_MODE;
        let e = battle_sideband_tick(&mut s, &i);
        assert_eq!(
            e.effects,
            vec![
                BattleSidebandEffect::IntroCaption,
                BattleSidebandEffect::IntroCameraAim,
            ]
        );
        assert_eq!(s.phase, 1);
        assert_eq!(s.caption_timer, INTRO_CAPTION_FRAMES);
        assert_eq!(s.hold, 1);
    }

    #[test]
    fn phase_one_decays_eight_per_frame_step() {
        let mut s = BattleSidebandState {
            phase: 1,
            caption_timer: 100,
            ..Default::default()
        };
        let mut i = inputs(SUBMODE_INTRO);
        i.frame_step = 3;
        battle_sideband_tick(&mut s, &i);
        assert_eq!(s.caption_timer, 100 - 24);
        assert_eq!(s.phase, 1);
        assert_eq!(s.hold, 1);
    }

    #[test]
    fn any_pad_edge_cancels_the_caption_wait() {
        let mut s = BattleSidebandState {
            phase: 1,
            caption_timer: INTRO_CAPTION_FRAMES,
            ..Default::default()
        };
        let mut i = inputs(SUBMODE_INTRO);
        i.pad_edge = true;
        let e = battle_sideband_tick(&mut s, &i);
        assert_eq!(s.phase, 2);
        assert_eq!(e.effects, vec![BattleSidebandEffect::DrainFloatingElements]);
    }

    #[test]
    fn phase_one_holds_while_the_effect_vm_is_not_ready() {
        let mut s = BattleSidebandState {
            phase: 1,
            caption_timer: 4,
            ..Default::default()
        };
        let mut i = inputs(SUBMODE_INTRO);
        i.effect_vm_ready = 0x40;
        i.frame_step = 8; // 8*8 = 64 > 4, so the timer would go negative
        let e = battle_sideband_tick(&mut s, &i);
        assert!(e.effects.is_empty());
        assert_eq!(s.phase, 1);
        // Pinned at 1 rather than left negative.
        assert_eq!(s.caption_timer, 1);
    }

    #[test]
    fn phase_two_is_a_bare_overlay_hook_and_publishes_no_hold() {
        let mut s = BattleSidebandState {
            phase: 2,
            ..Default::default()
        };
        let e = battle_sideband_tick(&mut s, &inputs(SUBMODE_INTRO));
        assert_eq!(e.effects, vec![BattleSidebandEffect::IntroOverlayHook]);
        assert_eq!(s.hold, 0);
    }

    #[test]
    fn phase_three_pushes_camera_b_by_eight_per_step() {
        let mut s = BattleSidebandState {
            phase: 3,
            ..Default::default()
        };
        let mut i = inputs(SUBMODE_INTRO);
        i.frame_step = 2;
        let e = battle_sideband_tick(&mut s, &i);
        assert_eq!(e.effects, vec![BattleSidebandEffect::SubOverlayTick]);
        assert_eq!(s.camera_b, 16);
        assert_eq!(s.phase3_accum, 2);
        assert_eq!(s.camera_a, 0);
    }

    #[test]
    fn in_battle_delegates_to_the_overlay_once_the_effect_vm_is_live() {
        let mut s = BattleSidebandState::default();
        let e = battle_sideband_tick(&mut s, &inputs(SUBMODE_IN_BATTLE));
        assert_eq!(e.effects, vec![BattleSidebandEffect::InBattleOverlayTick]);
    }

    #[test]
    fn in_battle_clears_the_pad_below_the_overlay_threshold() {
        let mut s = BattleSidebandState::default();
        let mut i = inputs(SUBMODE_IN_BATTLE);
        i.effect_vm_ready = OVERLAY_TICK_FROM - 1;
        let e = battle_sideband_tick(&mut s, &i);
        assert_eq!(e.effects, vec![BattleSidebandEffect::ClearPadState]);
    }

    #[test]
    fn camera_ramp_needs_both_the_scene_change_and_the_ramp_mode() {
        let mut i = inputs(SUBMODE_IN_BATTLE);
        i.effect_vm_ready = 0;
        for (changing, mode, want) in [
            (false, RAMP_MODE, 0),
            (true, 0u8, 0),
            (true, RAMP_MODE, CAMERA_RAMP_A),
        ] {
            let mut s = BattleSidebandState::default();
            i.scene_changing = changing;
            i.ctx_mode = mode;
            battle_sideband_tick(&mut s, &i);
            assert_eq!(s.camera_a, want, "changing={changing} mode={mode}");
        }
    }

    #[test]
    fn camera_ramp_is_cadence_invariant_and_overshoots_its_cap_by_one_step() {
        let mut i = inputs(SUBMODE_IN_BATTLE);
        i.effect_vm_ready = 0;
        i.scene_changing = true;
        i.ctx_mode = RAMP_MODE;

        // Ten frames at step 1 lands where five frames at step 2 do.
        let mut a = BattleSidebandState::default();
        for _ in 0..10 {
            battle_sideband_tick(&mut a, &i);
        }
        let mut b = BattleSidebandState::default();
        i.frame_step = 2;
        for _ in 0..5 {
            battle_sideband_tick(&mut b, &i);
        }
        assert_eq!(a.camera_a, b.camera_a);
        assert_eq!(a.camera_b, b.camera_b);

        // The cap is tested before the add, so one step past it lands.
        i.frame_step = 1;
        let mut c = BattleSidebandState {
            camera_a: CAMERA_RAMP_CAP - 1,
            ..Default::default()
        };
        battle_sideband_tick(&mut c, &i);
        assert_eq!(c.camera_a, CAMERA_RAMP_CAP - 1 + CAMERA_RAMP_A);
        let before = c.camera_a;
        battle_sideband_tick(&mut c, &i);
        assert_eq!(c.camera_a, before);
    }

    #[test]
    fn in_battle_burns_its_own_delay_before_it_ramps() {
        // The delay is ctx+0x6D6, a different halfword from the outro's
        // ctx+0x6D8, and it decays 8 per step rather than 1.
        let mut i = inputs(SUBMODE_IN_BATTLE);
        i.effect_vm_ready = 0;
        i.scene_changing = true;
        i.ctx_mode = RAMP_MODE;
        let mut s = BattleSidebandState {
            ramp_delay: 16,
            outro_timer: 99,
            ..Default::default()
        };
        battle_sideband_tick(&mut s, &i);
        assert_eq!(s.ramp_delay, 8);
        assert_eq!(s.outro_timer, 99);
        assert_eq!(s.camera_a, 0);
    }

    #[test]
    fn outro_waits_out_its_timer_then_needs_the_cd_idle() {
        let mut i = inputs(SUBMODE_OUTRO);
        i.frame_step = 0x10;
        let mut s = BattleSidebandState {
            outro_timer: 0x20,
            ..Default::default()
        };
        assert!(battle_sideband_tick(&mut s, &i).effects.is_empty());
        assert_eq!(s.outro_timer, 0x10);

        i.cd_idle = false;
        assert!(battle_sideband_tick(&mut s, &i).effects.is_empty());
        assert_eq!(s.outro_timer, 0);

        i.cd_idle = true;
        let e = battle_sideband_tick(&mut s, &i);
        assert_eq!(e.effects, vec![BattleSidebandEffect::OutroOverlayHook]);
    }

    #[test]
    fn an_unknown_submode_still_publishes_the_hold() {
        let mut s = BattleSidebandState {
            hold: 1,
            ..Default::default()
        };
        assert!(battle_sideband_tick(&mut s, &inputs(9)).effects.is_empty());
        assert_eq!(s.hold, 0);
    }
}
