//! Battle **side-band tick** - the once-per-frame SCUS pass keyed on the
//! battle **stage id** `_DAT_8007B64A`: the sparring fight's opening caption
//! (stage `1`), and the host side of the two boss-stage modules (stage `2`,
//! Cort's arrival, PROT 0968; stage `3`, Cort's form transition, PROT 0969).
//!
//! PORT: FUN_80056208
//!
//! The battle frame driver `FUN_80046A20` calls it on every battle frame
//! (`jal 0x80056208` at `0x80046D60`), ahead of the opening-counter gate and of
//! both battle state machines. The engine's seat is
//! `World::tick_battle_sideband` (`world/battle/sideband.rs`), which runs this
//! kernel once per live battle frame on both hosts and applies the returned
//! [`BattleSidebandEffects`]:
//!
//! | stage | arm | engine |
//! |---|---|---|
//! | `1` | phases `0` / `1` - caption, hold timer, any-press skip, `ctx[+0x6B0]` | `World::tick_battle_sideband` + the tutorial box queue |
//! | `1` | phase `2` - the overlay-967 hook `0x801F6B70` | dispatched on flow edges by `World::set_battle_flow` (the hook's one-shot latch makes a per-frame call and an edge call the same) |
//! | `1` | phase `3` - the battle-teardown staging `FUN_80025358` (PROT 0978) | not reached: the 967 port has no completion countdown that advances `ctx[+0x289]` to `3` |
//! | `2` | the arrival module `FUN_801F69F4` | [`crate::battle_stage_module::arrival_tick`] |
//! | `2` | pad clear + camera pull-back while the module is still paging in | unreachable in the engine - the module is resident from the first frame |
//! | `3` | the form-transition module `FUN_801F69D8` | [`crate::battle_stage_module::form_transition_tick`] |
//!
//! It is **not** libgpu-band vendor infrastructure despite sitting between
//! the PsyQ veneers: it reads the game's own battle context, dispatches into
//! three battle overlay hooks, and points a caption pointer at a game string.
//!
//! REF: FUN_801d8de8 - battle UI-element dispatcher (the intro caption).
//! REF: FUN_801d829c - camera-state per-actor transform builder (the intro
//! camera aim).
//! REF: FUN_800355f0 - 2D floating-element list teardown, run as the intro's
//! phase-1 exit.
//! REF: FUN_80025358 - gated sub-overlay load sequencer, ticked by phase 3.
//! REF: FUN_8003de7c - CD read-idle poll, gating the outro hook.
//! REF: FUN_800520f0 - the battle scene loader whose step byte
//! `DAT_8007BD71` gates stage 2.
//!
//! # Three stage arms
//!
//! Only stages `1` and `2` publish a hold:
//!
//! | stage | role |
//! |---|---|
//! | `1` | the sparring intro, four phases on `ctx[+0x289]` |
//! | `2` | the arrival module's tick once it is resident; before that, pad clear + camera pull-back |
//! | `3` | wait out `ctx[+0x6D8]`, then the form-transition module once the CD is idle |
//!
//! Everything else falls straight through to the tail, which always writes
//! `ctx[+0x6B0]` - so the *hold* flag is published unconditionally and is `1`
//! only for intro phases `0` and `1`.
//!
//! # The two gate bytes are loader and opening state
//!
//! `DAT_8007BD71` is the battle scene loader's step byte (`FUN_800520F0`,
//! `gp[+0xA59]`): the loader parks it at `0x11` while it pages the stage
//! overlay in (`0x80052694..0x800526A0`, `FUN_8003EC70(stage + 0x47)`), steps it
//! past that once the read lands, and leaves `0xFF` for a running fight and
//! `0xFE` for an ending one. So stage 2's `>= 0x12` test is "the arrival module
//! is resident". The engine's loader is synchronous, so a live battle frame
//! always reads [`BATTLE_RUNNING`].
//!
//! `DAT_8007B648` is `gp[+0x330]`, the frame driver's opening counter
//! (`0x80046EEC..0x8004700C`): it counts up through the battle open and sits
//! at `0xFF` - negative as a signed byte - for the rest of the fight.
//!
//! # The intro's phase 1 wait is cancellable
//!
//! Phase 1 decays `ctx[+0x6AE]` by `8 * frame_step` per frame, and any pad edge
//! (`_DAT_8007B874 | _DAT_8007B938`, masked to 16 bits) zeroes it outright - so
//! a button press skips the caption. The advance to phase 2 is then further
//! gated on `DAT_8007BD71` reading `0xFF`: while it does not, the timer is
//! pinned at `1` and the phase holds.
//!
//! # The camera ramp is cadence-invariant
//!
//! Stage 2's pre-residency ramp adds `4 * frame_step` to `0x800840BC` and
//! `14 * frame_step` to `0x800840C0` per frame, capped by testing
//! `0x800840BC < 0xC00` *before* the add - so the register can overshoot the
//! cap by one step. Phase 3 of the intro pushes `0x800840C0` alone, by
//! `8 * frame_step`.
//!
//! Source: `ghidra/scripts/funcs/80056208.txt` (disassembly).

/// Stage id `1`: the sparring fight (PROT 0967).
pub const STAGE_SPARRING: u8 = 1;
/// Stage id `2`: Cort's arrival (PROT 0968).
pub const STAGE_ARRIVAL: u8 = 2;
/// Stage id `3`: Cort's form transition (PROT 0969).
pub const STAGE_FORM_TRANSITION: u8 = 3;

/// `DAT_8007BD71` while a fight is running - the value that releases the
/// intro's phase-1 gate. The byte is the battle scene loader's step byte
/// (`FUN_800520F0`, `gp[+0xA59]`): `0xFF` once the load is done and the fight
/// runs, `0xFE` once a wipe or an escape ends it, lower values while the
/// battle is still loading.
pub const BATTLE_RUNNING: u8 = 0xFF;
/// `DAT_8007BD71` threshold at and above which stage 2 delegates to the
/// arrival module's tick instead of running its own ramp. The loader parks
/// the byte at `0x11` while the stage overlay pages in, so `>= 0x12` is "the
/// module is resident".
pub const STAGE_MODULE_RESIDENT: u8 = 0x12;

/// Battle-context phase byte value that arms the intro (`ctx[+6]`).
pub const INTRO_ARM_MODE: u8 = 0x14;
/// Command-flow byte value the camera ramp requires (`ctx[+6]`) - the value
/// flow state `0x0A` stores directly for the `0xB5` formation.
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
    /// `ctx + 0x289` - the stage phase cursor: the sparring intro's here, and
    /// the one both stage modules dispatch on.
    pub phase: u8,
    /// `ctx + 0x6AE` - the intro caption timer.
    pub caption_timer: i16,
    /// `ctx + 0x6CE` - a phase-3 accumulator.
    pub phase3_accum: u16,
    /// `ctx + 0x6D6` - the delay stage 2 burns before it starts ramping (the
    /// arrival module re-seeds it to `0x100` every tick).
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
    /// `DAT_8007B64A` - the battle stage id, which selects the arm.
    pub submode: u8,
    /// `DAT_1F800393` - the adaptive frame step, in vsyncs.
    pub frame_step: u8,
    /// `ctx + 6` - the battle context's own phase byte.
    pub ctx_mode: u8,
    /// `DAT_8007BD71` - the battle scene loader's step byte (see
    /// [`BATTLE_RUNNING`] and [`STAGE_MODULE_RESIDENT`]).
    pub loader_state: u8,
    /// `_DAT_8007B874 | _DAT_8007B938` masked to 16 bits - any pad edge.
    pub pad_edge: bool,
    /// `DAT_8007B648 < 0` - the frame driver's opening counter `gp[+0x330]`
    /// has passed its midpoint (it rests at `0xFF` for a running fight).
    pub opening_negative: bool,
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
    /// `FUN_801F6B70` - the sparring prompt machine's tick (PROT 0967).
    SparringHook,
    /// `FUN_80025358` - tick the battle-teardown staging (the PROT 0978 load
    /// and its streamer); its "still loading" return lands in `ctx[+0xB]`.
    TeardownStaging,
    /// `FUN_801F69F4` - the Cort arrival module's tick (PROT 0968), taken
    /// instead of the ramp once the module is resident.
    ArrivalModule,
    /// `FUN_801F69D8` - the Cort form-transition module (PROT 0969).
    FormTransitionModule,
    /// Clear the pad masks (`_DAT_8007B938` / `B874` / `B850`) and
    /// `ctx[+0x884]` - the input hold while the arrival module pages in.
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
        STAGE_SPARRING => match state.phase {
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
                if inputs.loader_state != BATTLE_RUNNING {
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
            2 => out.effects.push(BattleSidebandEffect::SparringHook),
            3 => {
                out.effects.push(BattleSidebandEffect::TeardownStaging);
                state.phase3_accum = state
                    .phase3_accum
                    .wrapping_add(u16::from(inputs.frame_step));
                state.camera_b = state.camera_b.wrapping_add(PHASE3_RAMP_B * step);
            }
            _ => {}
        },
        STAGE_ARRIVAL => {
            if inputs.loader_state >= STAGE_MODULE_RESIDENT {
                out.effects.push(BattleSidebandEffect::ArrivalModule);
            } else {
                out.effects.push(BattleSidebandEffect::ClearPadState);
                if inputs.opening_negative && inputs.ctx_mode == RAMP_MODE {
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
        STAGE_FORM_TRANSITION => {
            let mut expired = true;
            if state.outro_timer > 0 {
                state.outro_timer = state.outro_timer.wrapping_sub(i16::from(inputs.frame_step));
                expired = state.outro_timer <= 0;
            }
            if expired && inputs.cd_idle {
                out.effects.push(BattleSidebandEffect::FormTransitionModule);
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
            loader_state: BATTLE_RUNNING,
            pad_edge: false,
            opening_negative: false,
            cd_idle: true,
        }
    }

    #[test]
    fn phase_zero_needs_the_arm_mode_byte() {
        let mut s = BattleSidebandState::default();
        let mut i = inputs(STAGE_SPARRING);
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
        let mut i = inputs(STAGE_SPARRING);
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
        let mut i = inputs(STAGE_SPARRING);
        i.pad_edge = true;
        let e = battle_sideband_tick(&mut s, &i);
        assert_eq!(s.phase, 2);
        assert_eq!(e.effects, vec![BattleSidebandEffect::DrainFloatingElements]);
    }

    #[test]
    fn phase_one_holds_while_the_loader_is_not_done() {
        let mut s = BattleSidebandState {
            phase: 1,
            caption_timer: 4,
            ..Default::default()
        };
        let mut i = inputs(STAGE_SPARRING);
        i.loader_state = 0x40;
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
        let e = battle_sideband_tick(&mut s, &inputs(STAGE_SPARRING));
        assert_eq!(e.effects, vec![BattleSidebandEffect::SparringHook]);
        assert_eq!(s.hold, 0);
    }

    #[test]
    fn phase_three_pushes_camera_b_by_eight_per_step() {
        let mut s = BattleSidebandState {
            phase: 3,
            ..Default::default()
        };
        let mut i = inputs(STAGE_SPARRING);
        i.frame_step = 2;
        let e = battle_sideband_tick(&mut s, &i);
        assert_eq!(e.effects, vec![BattleSidebandEffect::TeardownStaging]);
        assert_eq!(s.camera_b, 16);
        assert_eq!(s.phase3_accum, 2);
        assert_eq!(s.camera_a, 0);
    }

    #[test]
    fn stage_two_delegates_to_the_arrival_module_once_it_is_resident() {
        let mut s = BattleSidebandState::default();
        let e = battle_sideband_tick(&mut s, &inputs(STAGE_ARRIVAL));
        assert_eq!(e.effects, vec![BattleSidebandEffect::ArrivalModule]);
    }

    #[test]
    fn stage_two_clears_the_pad_while_the_module_pages_in() {
        let mut s = BattleSidebandState::default();
        let mut i = inputs(STAGE_ARRIVAL);
        i.loader_state = STAGE_MODULE_RESIDENT - 1;
        let e = battle_sideband_tick(&mut s, &i);
        assert_eq!(e.effects, vec![BattleSidebandEffect::ClearPadState]);
    }

    #[test]
    fn camera_ramp_needs_both_the_scene_change_and_the_ramp_mode() {
        let mut i = inputs(STAGE_ARRIVAL);
        i.loader_state = 0;
        for (changing, mode, want) in [
            (false, RAMP_MODE, 0),
            (true, 0u8, 0),
            (true, RAMP_MODE, CAMERA_RAMP_A),
        ] {
            let mut s = BattleSidebandState::default();
            i.opening_negative = changing;
            i.ctx_mode = mode;
            battle_sideband_tick(&mut s, &i);
            assert_eq!(s.camera_a, want, "changing={changing} mode={mode}");
        }
    }

    #[test]
    fn camera_ramp_is_cadence_invariant_and_overshoots_its_cap_by_one_step() {
        let mut i = inputs(STAGE_ARRIVAL);
        i.loader_state = 0;
        i.opening_negative = true;
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
    fn stage_two_burns_its_own_delay_before_it_ramps() {
        // The delay is ctx+0x6D6, a different halfword from the outro's
        // ctx+0x6D8, and it decays 8 per step rather than 1.
        let mut i = inputs(STAGE_ARRIVAL);
        i.loader_state = 0;
        i.opening_negative = true;
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
    fn stage_three_waits_out_its_timer_then_needs_the_cd_idle() {
        let mut i = inputs(STAGE_FORM_TRANSITION);
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
        assert_eq!(e.effects, vec![BattleSidebandEffect::FormTransitionModule]);
    }

    #[test]
    fn an_unknown_stage_still_publishes_the_hold() {
        let mut s = BattleSidebandState {
            hold: 1,
            ..Default::default()
        };
        assert!(battle_sideband_tick(&mut s, &inputs(9)).effects.is_empty());
        assert_eq!(s.hold, 0);
    }
}
