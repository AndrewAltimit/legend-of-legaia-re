//! The battle **side-band** host: the engine's seat of the SCUS pass
//! `FUN_80056208`, run once per live battle frame by both play hosts through
//! [`World::live_battle_tick`].
//!
//! The kernel is [`crate::battle_sideband`]; the two stage modules it drives
//! for the Cort fight are [`crate::battle_stage_module`]. This file builds
//! their inputs from live world state and applies what they return:
//!
//! * **stage 1** (the sparring tutorial): the round start runs the kernel with
//!   the flow byte at `0x14` ([`World::battle_sideband_holds_round`]), whose
//!   `IntroCaption` queues the opening caption on the tutorial box queue; each
//!   frame after that decays its hold timer, and the drain opens the round.
//! * **stage 2** (Cort's arrival, PROT 0968): the module owns the camera, the
//!   boss seat and the frame until its phase 6 hands the battle back and
//!   round one opens.
//! * **stage 3** (Cort's form transition, PROT 0969): raised by the Final
//!   Heal sweep's tail ([`World::run_boss_transition_arm`]), it owns the frame
//!   until it takes the battle back to the field.

use super::*;

use crate::battle_sideband::{self as sb, BattleSidebandEffect as Fx};
use crate::battle_stage_module::{self as stage, StageCamera, StageEffect};

impl World {
    /// The side-band's inputs this frame. `ctx_mode` overrides the command
    /// flow byte (the round start passes `0x14`).
    fn battle_sideband_inputs(&self, ctx_mode: Option<u8>) -> sb::BattleSidebandInputs {
        let stage_id = self.battle.stage_id;
        sb::BattleSidebandInputs {
            submode: stage_id,
            frame_step: self.clock.frame_step.max(1),
            // The `0xB5` formation's flow sits at `0x0C` until the arrival
            // hands it back (flow state `0x0A`'s `0xB5` arm stores it
            // directly, `0x801D0DEC..0x801D0E0C`).
            ctx_mode: ctx_mode.unwrap_or(if stage_id == sb::STAGE_ARRIVAL {
                sb::RAMP_MODE
            } else {
                self.battle.flow.raw()
            }),
            // The engine's scene loader is synchronous: a live battle frame
            // is always past the loader's last step, stage overlay included.
            loader_state: sb::BATTLE_RUNNING,
            pad_edge: self.input.pad() & !self.input.pad_prev() != 0,
            // The opening counter `gp[+0x330]` rests at `0xFF` on every frame
            // the engine drives a live battle.
            opening_negative: true,
            // `FUN_8003DE7C(1) == 0`: the modelled drive is idle once the
            // last battle clip's read span has elapsed.
            cd_idle: self.audio.battle_xa_busy_frames == 0,
        }
    }

    /// One side-band frame. Returns `true` when a stage module owns the
    /// battle frame - retail's flow is parked at `0x0C` under the arrival and
    /// the action SM at `0xFD` / `0xFC` under the form transition - so the
    /// caller runs nothing else this frame.
    ///
    /// REF: FUN_80056208 (called by the frame driver `FUN_80046A20` at
    /// `0x80046D60`, ahead of both battle state machines)
    pub(in crate::world) fn tick_battle_sideband(&mut self) -> bool {
        let inputs = self.battle_sideband_inputs(None);
        let fx = sb::battle_sideband_tick(&mut self.battle.sideband, &inputs);
        self.apply_battle_sideband(&fx.effects);
        // The form transition's exit leaves the battle on this frame.
        self.mode != SceneMode::Battle
            || matches!(
                self.battle.stage_id,
                sb::STAGE_ARRIVAL | sb::STAGE_FORM_TRANSITION
            )
    }

    /// The side-band's say over a round start (`FUN_801D0748` state `0x14`):
    /// `true` when the round has to wait. The sparring caption holds it
    /// (`ctx[+0x6B0]`, tested at `0x801D0BDC`), and under the arrival the
    /// flow never reaches `0x14` at all until the module's hand-back.
    pub(in crate::world) fn battle_sideband_holds_round(&mut self) -> bool {
        match self.battle.stage_id {
            sb::STAGE_SPARRING if self.battle.sideband.phase == 0 => {
                let inputs = self.battle_sideband_inputs(Some(sb::INTRO_ARM_MODE));
                let fx = sb::battle_sideband_tick(&mut self.battle.sideband, &inputs);
                self.apply_battle_sideband(&fx.effects);
                self.battle.sideband.hold != 0
            }
            sb::STAGE_ARRIVAL | sb::STAGE_FORM_TRANSITION => true,
            _ => false,
        }
    }

    fn apply_battle_sideband(&mut self, effects: &[Fx]) {
        for e in effects {
            match e {
                Fx::IntroCaption => self.raise_sparring_caption(),
                // The aim at the first monster seat (`FUN_801D829C` with
                // `TR (0, 0x500, 0x400)`) is the phase-scripted camera's
                // Dialogue close-up, which the caption's hold selects
                // (`battle_cam_inputs`).
                Fx::IntroCameraAim => {}
                Fx::DrainFloatingElements => self.drain_sparring_caption(),
                // The prompt machine is dispatched on every flow edge by
                // `World::set_battle_flow`; its one-shot latch makes that the
                // same as retail's per-frame call.
                Fx::SparringHook => {}
                // Only reached once the 967 machine advances `ctx[+0x289]` to
                // `3`, which its port does not; the staging itself is the
                // engine's synchronous battle teardown (`finish_battle`).
                Fx::TeardownStaging => {}
                Fx::ArrivalModule => self.run_arrival_module(),
                Fx::FormTransitionModule => self.run_form_transition_module(),
                Fx::ClearPadState => self.input.clear_edges(),
            }
        }
    }

    /// The camera a stage module takes over: the phase-scripted camera's
    /// current framing, on the first frame a module owns it.
    fn stage_camera_or_seed(&self) -> StageCamera {
        self.battle.stage_camera.unwrap_or_else(|| {
            let p = self
                .battle
                .camera
                .as_ref()
                .map(|c| c.framing_pose())
                .unwrap_or(legaia_engine_vm::battle_cam_script::BOOT_POSE);
            StageCamera {
                pitch: p.pitch as i32,
                yaw: p.yaw as i32,
                tr: p.tr.map(|v| v as i32),
                focus: p.focus.map(|v| v as i32),
            }
        })
    }

    /// Engine actor index of retail actor-table slot `slot` (`0..=2` party,
    /// `3..` monsters), when the engine seats one there.
    fn retail_table_actor(&self, slot: usize) -> Option<usize> {
        let pc = usize::from(self.party.party_count.clamp(1, 3));
        let idx = if slot < 3 {
            (slot < pc).then_some(slot)?
        } else {
            pc + slot - 3
        };
        (idx < self.actors.len()).then_some(idx)
    }

    /// PROT 0968's tick, over the live boss seat.
    fn run_arrival_module(&mut self) {
        let Some(seat_idx) = self.retail_table_actor(3) else {
            return;
        };
        let a = &self.actors[seat_idx];
        let mut v = stage::ArrivalView {
            phase: self.battle.sideband.phase,
            ramp_delay: self.battle.sideband.ramp_delay,
            ctx_243: self.battle_ctx.gauge_rearm_latch,
            ctx_278: self.casting.module_ctx_278,
            flow: sb::RAMP_MODE,
            stage_id: self.battle.stage_id,
            camera: self.stage_camera_or_seed(),
            seat: stage::ArrivalSeat {
                x: a.move_state.world_x,
                y: a.move_state.world_y,
                z: a.move_state.world_z,
                facing: a.battle.facing_angle,
                blend: a.battle.render_blend,
                render_flag: a.battle.render_flag,
                anim_rate: a.battle.anim_rate.get(),
            },
        };
        let step = self.clock.frame_step.max(1);
        let effects = stage::arrival_tick(&mut self.battle.arrival, &mut v, step);
        self.battle.sideband.phase = v.phase;
        self.battle.sideband.ramp_delay = v.ramp_delay;
        self.battle_ctx.gauge_rearm_latch = v.ctx_243;
        self.casting.module_ctx_278 = v.ctx_278;
        self.battle.stage_id = v.stage_id;
        self.battle.stage_camera = Some(v.camera);
        let a = &mut self.actors[seat_idx];
        a.move_state.world_y = v.seat.y;
        a.battle.render_blend = v.seat.blend;
        a.battle.render_flag = v.seat.render_flag;
        a.battle.anim_rate = legaia_engine_vm::battle_anim_rate::AnimRate(v.seat.anim_rate);
        for e in effects {
            match e {
                StageEffect::HandBack => {
                    // Flow `0x0B` with the intro timer the module cleared:
                    // the timer expires at once, `FUN_800355F0` sweeps the
                    // banner, and `0x14` opens round one.
                    self.battle.stage_banner = None;
                    if let (Some(cam), Some(c)) =
                        (self.battle.stage_camera.take(), self.battle.camera.as_mut())
                    {
                        c.hand_back_from(stage_camera_pose(&cam));
                    }
                    self.begin_battle_round();
                }
                other => self.apply_stage_effect(other, seat_idx),
            }
        }
    }

    /// PROT 0969's tick, over the four leading actor-table slots.
    fn run_form_transition_module(&mut self) {
        let slots: [Option<usize>; 4] = std::array::from_fn(|i| self.retail_table_actor(i));
        let seat_of = |w: &World, idx: Option<usize>| -> stage::TransitionSeat {
            let Some(a) = idx.and_then(|i| w.actors.get(i)) else {
                return stage::TransitionSeat::default();
            };
            stage::TransitionSeat {
                present: true,
                x: a.move_state.world_x,
                z: a.move_state.world_z,
                facing: a.battle.facing_angle,
                colour: a.battle.render_color,
                hp: a.battle.hp,
                queued_anim: a.battle.queued_anim,
                flags: a.battle.flag_bits.0,
                target: a.battle.active_target,
                render_flag: a.battle.render_flag,
                capture_state: a.battle.capture_state,
            }
        };
        let acting = self.retail_pool_slot(self.battle_ctx.active_actor);
        let mut v = stage::FormTransitionView {
            phase: self.battle.sideband.phase,
            action_state: self.battle_ctx.action_state,
            acting,
            ctx_243: self.battle_ctx.gauge_rearm_latch,
            ctx_278: self.casting.module_ctx_278,
            camera: self.stage_camera_or_seed(),
            seats: std::array::from_fn(|i| seat_of(self, slots[i])),
        };
        let step = self.clock.frame_step.max(1);
        let mut st = self.battle.form_transition;
        let effects = {
            let mut rand = || self.next_rand() as u16;
            stage::form_transition_tick(&mut st, &mut v, step, &mut rand)
        };
        self.battle.form_transition = st;
        self.battle.sideband.phase = v.phase;
        self.battle_ctx.action_state = v.action_state;
        self.battle_ctx.gauge_rearm_latch = v.ctx_243;
        self.casting.module_ctx_278 = v.ctx_278;
        self.battle.stage_camera = Some(v.camera);
        for (seat, idx) in v.seats.iter().zip(slots) {
            let Some(a) = idx.and_then(|i| self.actors.get_mut(i)) else {
                continue;
            };
            a.move_state.world_x = seat.x;
            a.move_state.world_z = seat.z;
            a.battle.facing_angle = seat.facing;
            a.battle.render_color = seat.colour;
            if a.battle.hp != seat.hp {
                // `+0x14C` is the live HP the engine splits into the HP
                // word and the liveness mirror the scans read.
                a.battle.hp = seat.hp;
                a.battle.liveness = u16::from(seat.hp != 0);
            }
            a.battle.queued_anim = seat.queued_anim;
            a.battle.flag_bits.0 = seat.flags;
            a.battle.active_target = seat.target;
            a.battle.render_flag = seat.render_flag;
            a.battle.capture_state = seat.capture_state;
            if let Some((sx, sz)) = a.battle.seat.as_mut() {
                // The seat anchor follows the live pair, as battle setup
                // keeps them (`+0x3C` / `+0x40`).
                *sx = seat.x;
                *sz = seat.z;
            }
        }
        let boss = slots[3].unwrap_or(0);
        for e in effects {
            self.apply_stage_effect(e, boss);
        }
    }

    /// The calls a stage module makes into the rest of the game.
    fn apply_stage_effect(&mut self, e: StageEffect, seat_idx: usize) {
        match e {
            StageEffect::Cue(id) => {
                self.audio
                    .battle_sfx_cues
                    .push(crate::battle_events::BattleSfxCue {
                        kind: id,
                        timing_frames: 0,
                        actor_slot: seat_idx as u8,
                        target_slot: seat_idx as u8,
                    });
            }
            StageEffect::Fade(t) => {
                self.presentation.fade = Some(crate::fade::FadeState::load(&t));
            }
            StageEffect::Banner => {
                // `**0x801C9348`: the first enemy record's name.
                self.battle.stage_banner = self
                    .actors
                    .get(seat_idx)
                    .and_then(|a| a.battle_monster_id)
                    .and_then(|id| self.tables.monster_catalog.get(id))
                    .map(|d| d.name.clone());
            }
            StageEffect::ExitBattle => {
                // Mode word `2` with `DAT_8007BD60 = 0x80`: MAIN INIT's
                // back-from-battle arm reads the won bit and raises story
                // flag 1 (`0x8003B570..0x8003B590`). No results sequence ran,
                // so no spoils are credited.
                self.battle.stage_id = 0;
                self.battle.stage_camera = None;
                self.battle.stage_banner = None;
                self.battle.end = None;
                self.system_flag_set(1);
                self.finish_battle();
            }
            // Not staged (every record is a meshless `model_sel = -1` part)
            // and the HUD handle list is derived per frame, so its hard reset
            // has nothing to clear - see `crate::battle_stage_module`.
            StageEffect::Spawn { .. } | StageEffect::HudReset => {}
            // Consumed by the arrival's own loop above.
            StageEffect::HandBack => {}
        }
    }

    /// Queue the sparring caption - `IntroCaption`'s host half: the caption
    /// string `0x80078CB4` behind HUD element `0x5A`, carried on the tutorial
    /// box queue (both hosts draw it framed and text-measured) at the retail
    /// frame's placement, the emitter's style `9` corner. The side-band's own
    /// timer dismisses it, so the box never ages itself out.
    ///
    /// A world with no caption text (no disc) skips the hold outright rather
    /// than showing an empty window.
    fn raise_sparring_caption(&mut self) {
        use crate::battle_flow::ActiveTutorialBox;
        use crate::battle_tutorial::{SPARRING_CAPTION_FRAMES, SPARRING_CAPTION_STYLE};
        let Some(text) = self
            .battle
            .ui_strings
            .get(legaia_asset::battle_ui_strings::BattleUiLabel::SparringIntro)
            .map(str::to_string)
        else {
            self.battle.sideband.phase = 2;
            self.battle.sideband.hold = 0;
            return;
        };
        let group = self.next_battle_tutorial_group();
        self.battle.tutorial_boxes.push_back(ActiveTutorialBox {
            text,
            style: SPARRING_CAPTION_STYLE,
            waits_for_input: false,
            frames_remaining: SPARRING_CAPTION_FRAMES,
            group,
            any_press_dismisses: true,
        });
    }

    /// `DrainFloatingElements` (`FUN_800355F0`) at the caption's expiry:
    /// the caption goes, the hold drops, and the flow SM's `0x14` arm - held
    /// since the round start - runs.
    fn drain_sparring_caption(&mut self) {
        // The caption is the one box any press dismisses.
        self.battle
            .tutorial_boxes
            .retain(|b| !b.any_press_dismisses);
        self.begin_battle_round();
        // The press that ended the caption is spent on it. The side-band runs
        // ahead of the tutorial-box ticker in the same frame, so without this
        // the one edge would also confirm the lesson box the round start just
        // queued, and it would close before it was ever drawn.
        self.input.clear_edges();
    }
}

/// A stage camera as the phase-scripted camera's pose type.
fn stage_camera_pose(c: &StageCamera) -> legaia_engine_vm::battle_cam_script::BattleCamPose {
    legaia_engine_vm::battle_cam_script::BattleCamPose {
        pitch: c.pitch as f32,
        yaw: c.yaw as f32,
        tr: c.tr.map(|v| v as f32),
        focus: c.focus.map(|v| v as f32),
    }
}
