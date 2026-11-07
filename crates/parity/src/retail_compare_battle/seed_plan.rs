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
    /// [`legaia_engine_battle_loop::battle_state::InflightCastSeed`] and gated on
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

/// A replay's battle frame step seed: the step, the action state it holds
/// in, the acting body's `(clip, cursor)` the step waits for
/// ([`RetailBattle::step_run_start`]), `None` to run it over the whole state,
/// and the close-up accumulator the step waits for
/// ([`RetailBattle::step_run_accum_hold`]).
pub type FrameStepSeed = (u8, u8, Option<(u8, i32)>, Option<u32>);

/// The first committed dynamic animation slot (`+0x1D9`), where a party art
/// materialises (`docs/subsystems/battle-actor-rendering.md`).
const DYNAMIC_SLOT_ART: u8 = 0x10;

impl RetailBattle {
    /// Whether the capture stands inside the ambush round the entry rolled: a
    /// monster on its action with the advantage latched and every party key
    /// still the zero the round's seeder gave the disadvantaged side. The
    /// action is then the fight's first, and no prompt comes ahead of it
    /// (`battle_deene_lapis_lapis_wave_mid_cast`: monster `0xA7` always ambushes,
    /// so the party's bars read before Lapis Wave has zeroed them).
    pub fn in_formation_round(&self) -> bool {
        self.formation_latch == 1
            && matches!(self.seed_plan(), SeedPlan::Action { seat, .. } if seat >= 3)
            && (0..usize::from(self.party_count.clamp(1, 3)))
                .all(|s| self.init_keys.get(s).copied().flatten() == Some(0))
    }

    /// The image child's step seed past [`Self::frame_step_seed`]'s: the
    /// frame the child draws seats its parts on the passes it runs, so a
    /// capture whose drawn streams need retail's pass length runs on it while
    /// the headless seed, whose channels are the camera's and the logic
    /// words', keeps the default (seeded, `cort_evolved_ultra_charge_mid_cast`'s
    /// camera read `.998`). A harness seat.
    pub fn image_frame_step_seed(&self) -> Option<FrameStepSeed> {
        if let Some(seed) = self.frame_step_seed() {
            return Some(seed);
        }
        let default = self.child_ordinary_step();
        // A monster's capture on the capture band's module tick whose
        // history ran one other step all along stands on that step: a part a
        // module part seats mid-walk takes its first tick on the seat's own
        // pass (`SummonScene::seat_walk`), and on the child's default step
        // that pass is half of retail's, so every seated stream stood half
        // a pass young. `cort_evolved_ultra_charge_mid_cast` ran sixteen
        // step-`4` passes: its rings' waits sat `16` off the RAM's phase on
        // step `2` and match it on `4` (`image` `.988` to `.997`). The
        // history's oldest entry is a floor, so fifteen passes on the step
        // are the whole ring (`battle_nilboa_neo_grude_power_up_mid_cast`).
        // Only past arm 0 with the module's parts live: an arm-0 capture's
        // framing is the walk-in's, which `frame_step_seed` places, and
        // seeded there twenty of them parted on the camera.
        if matches!(self.span_gate, SpanGate::CaptureFade { .. })
            && matches!(self.seed_plan(), SeedPlan::Action { seat, .. } if seat >= 3)
            && self.action_state == 0x70
            && self.module_phase > 0
            && !self.part_ages.is_empty()
            && self.step_run.0 != default
            && self.step_run.1 >= 15
        {
            return Some((self.step_run.0, self.action_state, None, None));
        }
        None
    }

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
    pub fn frame_step_seed(&self) -> Option<FrameStepSeed> {
        let default = self.child_ordinary_step();
        // A capture on the capture band's module tick whose whole history ran the
        // default step stands on it: the arm step table
        // (`capture_arm_frame_step`) is one fight's reading, and
        // `zeto_big_wave_mid_cast` ran arm 5 at `2` where the table reads `4`.
        if self.action_state == 0x70
            && self.step_run == (default, self.step_run.1)
            && self.step_run.1 >= 16
            && !matches!(self.seed_plan(), SeedPlan::Cast)
        {
            return Some((default, self.action_state, None, None));
        }
        // A capture in the capture band's CD holds whose newest passes ran
        // another step: the branch `0x6E` re-arms case 6 every pass
        // (`FUN_801D5854(actor, 6)`) and the walker steps once a pass, so
        // how far the framing has closed counts passes, not vsyncs, and the
        // hold is placed on vsync clocks (the yaw counter, the depth ramp).
        // `battle_map01_gomboo_steal_mid_cast`'s newest seven passes ran at
        // step `3` (ring entries `575..585`), and at the default `2` the
        // replay's eye stood `89` closer than retail's (camera `.991` to
        // `.998`).
        // A capture on the module tick's arm 0 still frames on the walk-in's
        // case 6 (the approach leg's), which counts passes the same way, and
        // there a history that never left its step names the step of the
        // whole band: `battle_jou_freed_v3_water_crystals_mid_cast` ran sixteen
        // step-`3` passes, and on the replay's `2` its yaw stood `55` past
        // retail's (camera `.972` to `.999`).
        if matches!(self.span_gate, SpanGate::CaptureFade { .. })
            && (matches!(self.action_state, 0x6E | 0x6F) && self.step_run.1 < 16
                || self.action_state == 0x70 && self.module_phase == 0)
            && self.step_run.0 != default
        {
            return Some((self.step_run.0, 0x6E, None, None));
        }
        if matches!(self.seed_plan(), SeedPlan::Cast) {
            return (legaia_engine_battle_vm::battle_cam_script::SUMMON_CAST_STATES
                .contains(&self.action_state)
                && self.frame_step != default)
                .then_some((self.frame_step, self.action_state, None, None));
        }
        // A monster's magic-band capture on `0x2B`: the capture is a pass or
        // three past the clip commit, the commit's cursor and accumulator
        // pin those passes' step (Windhash cursor `36`, accumulator `48`:
        // three step-`3` passes, the first adding nothing to the
        // accumulator; Aluru Cannon `32` / `32`: two step-`4` passes), and
        // the displayed frame is two of them back.
        if self.monster_magic_band_age() && self.action_state == 0x2B {
            let (step, _) = self.step_run;
            return (step != default).then_some((
                step,
                self.action_state,
                None,
                self.step_run_accum_hold(),
            ));
        }
        // An action in flight: the newest passes' step, from the pass the
        // run began on (`step_run_start`) when the history holds where it
        // began, over the whole state when the history never left it.
        let (step, passes) = self.step_run;
        if !matches!(self.seed_plan(), SeedPlan::Action { .. }) || step == default {
            return None;
        }
        // A history that never left the step is not seeded: over the whole
        // state it measured `battle_vahn_tri_somersault_super` `.955` to
        // `.861` (step 4) and `battle_noa_miracle_art_combo` `.962` to
        // `.972` (step 3).
        if passes >= 16 {
            return None;
        }
        // Only a run inside a committed dynamic slot (an art, ring id
        // `0x10` and up): a run on the base clip `0` names no swing to
        // hold the step for. `cort_evil_seru_magic_mid_cast` (Cort's base
        // clip at cursor `56`, state `0x6F`) seeded that way read `image`
        // `1.000` to `.967` and `camera` `1.000` to `.987`.
        self.step_run_start
            .filter(|&(clip, _)| clip >= DYNAMIC_SLOT_ART)
            .map(|from| (step, self.action_state, Some(from), None))
    }

    /// Where a magic-band capture's step run began, as the close-up
    /// accumulator counts it: the value `ctx[+0x87C]` held after the `0x2B`
    /// passes that ran before the run. `None` when the run covers every
    /// `0x2B` pass, or the history does not add up to the capture's
    /// accumulator.
    ///
    /// Each `0x2B` pass's framing prologue adds `8 * step` of the step that
    /// pass ran at (`0x801D5900..0x801D5914`), and a pass runs at the step
    /// the frames before it left in `FUN_80016B6C`'s ring, so the pass that
    /// first draws an expensive effect runs at the cheaper step and only the
    /// passes after it at the dearer one. Caruban's Fire Breath on a full
    /// party (`battle_rugi_caruban_3_fire_breath_mid_cast`): accumulator `64`
    /// over steps `2`, `3`, `3`, the first `0x2B` frame costing `550`
    /// hblanks where the `0x29` frames cost `450`.
    pub fn step_run_accum_hold(&self) -> Option<u32> {
        let SpanGate::Age { accum } = self.span_gate else {
            return None;
        };
        let (_, run) = self.step_run;
        let mut sum = 0u32;
        let mut passes = 0usize;
        for &s in &self.recent_steps {
            if sum >= u32::from(accum) {
                break;
            }
            sum += 8 * u32::from(s);
            passes += 1;
        }
        if sum != u32::from(accum) || run >= passes {
            return None;
        }
        // The passes before the run stand on the replay's ordinary step,
        // so they must have run at it: `battle_deene_ironman_jagged_earth_mid_cast`
        // ran three step-`3` passes ahead of its step-`4` run, which the
        // replay's step `2` counts in five (`image` `.871` to `.846`).
        let ordinary = self.child_ordinary_step();
        if self.recent_steps[run..passes]
            .iter()
            .any(|&s| s != ordinary)
        {
            return None;
        }
        let in_run: u32 = self.recent_steps[..run]
            .iter()
            .map(|&s| 8 * u32::from(s))
            .sum();
        Some(u32::from(accum) - in_run)
    }

    /// The step a replay's ordinary frames run on. A replayed cast, a monster's
    /// capture-band walk-in and a Done-band hold gate on a word of the action, standing on a camera that
    /// starts from the replay's own prompt rather than the earlier actor's shot, so
    /// they read the framing by how long the walk has had, and the capture holds no
    /// word for the passes before its history. They keep the default step
    /// (`LEGAIA_BATTLE_PARTY_STEP=0`); a played round takes the party's.
    pub fn child_ordinary_step(&self) -> u8 {
        if self.replay_keeps_default_step() {
            legaia_engine_battle_loop::frame_clock::DEFAULT_BATTLE_FRAME_STEP
        } else {
            legaia_engine_battle_loop::frame_clock::steady_battle_frame_step(self.party_count)
        }
    }

    /// See [`Self::child_ordinary_step`].
    pub fn replay_keeps_default_step(&self) -> bool {
        // A history that left the step inside its sixteen passes ran the walk
        // up to the capture on a cheaper one (`FUN_80016B6C`'s ring holds the
        // frames): only a history that never left the party's step stands on it.
        let unsteady = self.step_run.1 < 16;
        unsteady
            || matches!(
                self.span_gate,
                SpanGate::CaptureFade { .. } | SpanGate::DoneHold { .. }
            )
            || matches!(self.seed_plan(), SeedPlan::Cast)
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

/// The acting seat's queued action id `actor[+0x1DF]`, `0` when the seat's
/// record is not in RAM.
fn capture_action(ram: &[u8], ctx: u32) -> u8 {
    let actor = game_anchors::u32_at(
        ram,
        ACTOR_TABLE + u32::from(game_anchors::u8_at(ram, ctx + 0x13).min(7)) * 4,
    );
    if in_ram(actor) {
        game_anchors::u8_at(ram, actor + 0x1DF)
    } else {
        0
    }
}

/// Whether the capture-class action `action` reaches a walk-in body whose
/// arm 0 stores its own yaw counter rung (`li v0,0x200; sh v0,0x6DA(v1)`,
/// PROT 0952 at `0x801F7204` for Bloody Horns): a capture past that arm holds
/// the walk's counter, not the one the action seed started.
pub(super) fn yaw_counter_reseeded_by(action: u8) -> bool {
    legaia_engine_cast::cast_module_ticks::capture_body_for_action(action).is_some_and(
        |(entry, body)| {
            legaia_engine_cast::cast_module_camera::capture_approach_yaw_base(entry, Some(body))
                .is_some()
        },
    )
}

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
                    let phase = game_anchors::u8_at(ram, ctx + 0x279);
                    // A body that gates on no module word (PROT 0962's
                    // charge bodies wait on the caster's cursor) still
                    // places the capture by its phase. Neo Star Slash and
                    // Doomsday gate on module words their directors carry,
                    // so a capture one arm in places by how far the word
                    // has run down: on the arm's first pass the engine's
                    // camera has not yet walked the shot retail's had.
                    use legaia_engine_cast::cast_module_camera as cmc;
                    Some(cmc::capture_countdown_va(action).map_or(
                        (phase, legaia_engine_capture::battle_drive::PHASE_ONLY),
                        |va| {
                            let word = game_anchors::u32_at(ram, va) as i32;
                            (
                                phase,
                                word.saturating_mul(cmc::capture_countdown_units(action)),
                            )
                        },
                    ))
                })
                .flatten();
            // The yaw counter is a clock only while the action SM drifts it
            // (any Battle Camera option but Far, `0x801E29D4`), and only
            // from the seed: a walk-in body's arm 0 stores its own rung over
            // it (`capture_approach_yaw_base`), so past that arm it counts the
            // walk, not the band.
            let reseeded = game_anchors::u8_at(ram, ctx + 7) == 0x70
                && game_anchors::u8_at(ram, ctx + 0x279) >= 1
                && yaw_counter_reseeded_by(capture_action(ram, ctx));
            let yaw = (game_anchors::u8_at(ram, BATTLE_CAMERA_OPTION) != 2 && !reseeded)
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
