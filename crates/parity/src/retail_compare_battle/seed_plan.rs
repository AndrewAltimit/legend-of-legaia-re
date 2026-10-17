//! [`SeedPlan`]: which entry path puts the engine where a capture was
//! taken (the opening sweep, a pre-seed state, an in-flight action or the
//! results sequencer), and the span gate that decides it.

use super::*;

/// Where in the fight a capture sits, as far as the seed has to place the
/// engine to compare the same phase ([`RetailBattle::seed_plan`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedPlan {
    /// The fight is still opening: `ctx[+0x06]` holds one of the entry
    /// values below the round prompt. The engine is sampled once its
    /// entry sweep has run as far as retail's ([`entry_sweep_reached`]).
    Opening,
    /// A command-selection surface above the round prompt, on party seat
    /// `seat` (`ctx[+0x13]`, the member cursor): the engine's command flow
    /// is driven there through its pad path.
    Menu { flow: BattleFlowState, seat: u8 },
    /// A party cast in the summon band: replayed through
    /// [`legaia_engine_core::world::InflightCastSeed`] and gated on
    /// [`PhaseGate`].
    Cast,
    /// Any other action in flight (flow `0xFF`): the round is played out
    /// through the pad path until the engine's action SM holds the same
    /// state on the same seat.
    Action { seat: u8, state: u8 },
    /// The round prompt: park on it and settle.
    Prompt,
}

/// Ticks an opening seed may run waiting for the engine's entry sweep to
/// reach retail's counter ([`entry_sweep_reached`]); the sweep spans `0x41`
/// display frames.
pub(super) const ENTRY_SWEEP_TICKS: u32 = 120;

/// Ticks an opening seed of the sparring fight may run past its sweep for
/// the round start the intro names hold back
/// (`legaia_engine_core::world::intro_names::INTRO_HOLD_FRAMES` is `0x5A`).
pub(super) const SPARRING_OPEN_TICKS: u32 = 160;

/// `ctx[+0x06]` values below the round prompt: SCUS battle init's `0xFD`
/// (`FUN_80055B6C`, `sb v0,0x6(v1)` at `0x80055FA8`), the overlay's init
/// `0x00`, the intro timer `0x0A` / `0x0B`, the boss stage module's baton
/// `0x0C`, and the one-frame turn setup `0x14` (`FUN_801D0748`,
/// `docs/subsystems/battle.md`).
pub const OPENING_FLOWS: [u8; 6] = [0xFD, 0x00, 0x0A, 0x0B, 0x0C, 0x14];

impl RetailBattle {
    /// The battle frame step a replay of this capture runs its capture state
    /// on, and that state: the step the capture's own frame ran at, applied
    /// while the engine sits in the capture's action state
    /// (`World::seed_battle_frame_step`). `None` when that step is the
    /// engine's default, the capture is not a replayed cast, or the state is
    /// not the summon close-up.
    ///
    /// Retail's step is the maximum of its last sixteen frame times
    /// ([`frame_step`]), so the ring a capture holds says what its newest
    /// frames ran at - the capture state's - and nothing about the frames
    /// before them. Two limits keep the seed where it measures better:
    ///
    /// * a replayed cast ([`SeedPlan::Cast`]) runs the captured action alone,
    ///   where a pad drive plays every earlier round through the same states
    ///   and a seeded step rewrites the history it reaches the capture by
    ///   (`battle_vahn_tri_somersault_super` reached its round 380 ticks
    ///   later, camera `.864` to `.663`);
    /// * the summon close-up `0x33` / `0x34`
    ///   ([`legaia_engine_battle_vm::battle_cam_script::SUMMON_CAST_STATES`]) re-arms
    ///   `FUN_801DC0A0` case `0x12`'s three-frame tween every pass, which the
    ///   walker lands in one step at step `3` and trails at `2`. The module
    ///   states `0x35` / `0x36` measured mixed (`gimard_burning_attack`
    ///   `image` `+.022`, `camera` `-.026`).
    pub fn frame_step_seed(&self) -> Option<(u8, u8)> {
        (matches!(self.seed_plan(), SeedPlan::Cast)
            && legaia_engine_battle_vm::battle_cam_script::SUMMON_CAST_STATES
                .contains(&self.action_state)
            && self.frame_step != legaia_engine_core::world::DEFAULT_BATTLE_FRAME_STEP)
            .then_some((self.frame_step, self.action_state))
    }

    /// The slot-1 backdrop angle the **displayed** frame was drawn at. The
    /// display scans out the frame built two game frames before the one the
    /// RAM holds, and each pass winds the angle by half the frame step while
    /// the keep byte is up (`FUN_80046A20`, `0x80046D34`), so the frame on
    /// screen is two such steps behind [`Self::backdrop_slot_1_yaw`]. A stage
    /// that drops object 1 does not wind, and shows the RAM's angle.
    pub fn displayed_backdrop_slot_1_yaw(&self) -> u16 {
        let wound = if self.keep_backdrop_object_1 {
            2 * u16::from(self.frame_step >> 1)
        } else {
            0
        };
        self.backdrop_slot_1_yaw.wrapping_sub(wound)
    }

    /// How the seed has to place the engine for this capture.
    pub fn seed_plan(&self) -> SeedPlan {
        if OPENING_FLOWS.contains(&self.flow) {
            return SeedPlan::Opening;
        }
        if self.inflight_cast().is_some() {
            return SeedPlan::Cast;
        }
        if self.flow == 0xFF {
            return SeedPlan::Action {
                seat: self.active_actor,
                state: self.action_state,
            };
        }
        match BattleFlowState::from_raw(self.flow) {
            BattleFlowState::Idle | BattleFlowState::TurnPrompt => SeedPlan::Prompt,
            flow => SeedPlan::Menu {
                flow,
                seat: self.active_actor,
            },
        }
    }
}

/// The battle-end signal byte the `0x5A` wipe gate raises.
pub(super) const END_SIGNAL: u32 = 0x8007_BD71;
/// The results sequencer's phase word (`_DAT_8007BD2C`; the wipe cause at
/// the signal).
pub(super) const END_PHASE_WORD: u32 = 0x8007_BD2C;
/// The results hold `gp+0xA54`.
pub(super) const END_RESULTS_HOLD: u32 = 0x8007_BD6C;

/// Read the [`SpanGate`] off a capture.
pub(super) fn span_gate(ram: &[u8], ctx: u32) -> SpanGate {
    if game_anchors::u8_at(ram, END_SIGNAL) != 0xFE {
        if matches!(game_anchors::u8_at(ram, ctx + 7), 0x51 | 0x52) {
            return SpanGate::DoneHold {
                timer: game_anchors::u16_at(ram, ctx + 0x6D8) as i16,
            };
        }
        if PRE_SEED_STATES.contains(&game_anchors::u8_at(ram, ctx + 7)) && camera_landed(ram, ctx) {
            return SpanGate::Landed;
        }
        if CAPTURE_FADE_STATES.contains(&game_anchors::u8_at(ram, ctx + 7)) {
            // In the module tick `0x70`, a directed module's arm and the
            // countdown word it gates on place the capture inside the run.
            let arm = (game_anchors::u8_at(ram, ctx + 7) == 0x70)
                .then(|| {
                    let actor = game_anchors::u32_at(
                        ram,
                        ACTOR_TABLE + u32::from(game_anchors::u8_at(ram, ctx + 0x13).min(7)) * 4,
                    );
                    let action = if in_ram(actor) {
                        game_anchors::u8_at(ram, actor + 0x1DF)
                    } else {
                        0
                    };
                    legaia_engine_cast::cast_module_camera::capture_countdown_va(action).map(|va| {
                        (
                            game_anchors::u8_at(ram, ctx + 0x279),
                            game_anchors::u32_at(ram, va) as i32,
                        )
                    })
                })
                .flatten();
            // The yaw counter is a clock only while the action SM drifts it
            // (any Battle Camera option but Far, `0x801E29D4`).
            let yaw = (game_anchors::u8_at(ram, BATTLE_CAMERA_OPTION) != 2)
                .then(|| game_anchors::u16_at(ram, ctx + 0x6DA));
            return SpanGate::CaptureFade {
                height: game_anchors::u16_at(ram, ctx + 0x6D0),
                accum: game_anchors::u32_at(ram, ctx + 0x87C).min(u32::from(u16::MAX)) as u16,
                arm,
                yaw,
            };
        }
        return SpanGate::Age {
            accum: game_anchors::u32_at(ram, ctx + 0x87C).min(u32::from(u16::MAX)) as u16,
        };
    }
    let half = game_anchors::u16_at(ram, ctx + 0x6CE);
    match (game_anchors::u32_at(ram, END_PHASE_WORD), half) {
        (_, h) if h >= 2 => SpanGate::Exit { phase: h },
        (5, 1) => SpanGate::Results {
            hold: game_anchors::u32_at(ram, END_RESULTS_HOLD) as u16,
        },
        _ => SpanGate::Loading,
    }
}
