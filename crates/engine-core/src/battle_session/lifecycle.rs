//! Session lifecycle + top-level phase SM: construction, configuration
//! setters, phase accessors, `begin_round`, the per-frame `tick`, the phase
//! transition primitives, and the escape / sub-phase helpers.

use super::*;

impl Default for BattleSession {
    fn default() -> Self {
        Self::new()
    }
}

impl BattleSession {
    pub fn new() -> Self {
        Self {
            phase: BattlePhase::Idle,
            phase_frames: 0,
            intro_frames: 60,
            outro_frames: 30,
            turn: 0,
            runner: BattleRunner::new(),
            hud: BattleHud::new(),
            round: None,
            slots: Default::default(),
            per_slot_records: Default::default(),
            equipment: EquipmentTable::new(),
            modifiers: StatusModifiers::default(),
            monster_count: 0,
            target_picker: None,
            pending_target_command: None,
            resolve_driver: None,
            rng_seed: 0xDEAD_C0DE,
        }
    }

    /// Pin the deterministic RNG seed used by the in-session accuracy +
    /// variance rolls. Engines that want a reproducible playthrough seed
    /// this once before [`Self::begin_round`].
    pub fn with_rng_seed(mut self, seed: u32) -> Self {
        self.rng_seed = seed;
        self
    }

    /// Configure intro / outro phase durations. Engines that want a faster
    /// or slower splash can set these before `begin_round`.
    pub fn with_phase_durations(mut self, intro_frames: u64, outro_frames: u64) -> Self {
        self.intro_frames = intro_frames;
        self.outro_frames = outro_frames;
        self
    }

    /// Set the three party-slot characters. Engines pull from
    /// `World::roster` or the live `Party` shape.
    pub fn set_party(&mut self, characters: [Character; 3]) {
        self.runner.set_characters(characters);
    }

    /// Provide the per-slot bundle (name + party flag + record + MP cap).
    /// Engines call this before `begin_round`.
    pub fn set_slot_info(&mut self, slot: u8, info: SessionSlotInfo) {
        if (slot as usize) >= self.slots.len() {
            return;
        }
        self.per_slot_records[slot as usize] = info.record;
        self.slots[slot as usize] = info;
    }

    /// How many monster slots are non-empty. Used by the wipe detector.
    pub fn set_monster_count(&mut self, count: u8) {
        self.monster_count = count.min(5);
    }

    /// Currently-active phase.
    pub fn phase(&self) -> BattlePhase {
        self.phase
    }

    /// Frames spent in the current phase so far.
    pub fn phase_frames(&self) -> u64 {
        self.phase_frames
    }

    /// `true` if the session is in a terminal state (Victory / Defeat /
    /// Escaped).
    pub fn is_done(&self) -> bool {
        matches!(
            self.phase,
            BattlePhase::Victory | BattlePhase::Defeat | BattlePhase::Escaped
        )
    }

    /// Begin a fresh round. Drives [`BattleRound::begin`] (AP refresh, stat
    /// recompute, blocked-arrays) and syncs the HUD.
    ///
    /// Transitions phase: `Idle` / `RoundOutro` → `RoundIntro`.
    pub fn begin_round(&mut self, world: &mut World) -> &BattleRound {
        let round = self.runner.begin_round(
            world,
            &self.per_slot_records,
            &self.equipment,
            &self.modifiers,
        );
        self.round = Some(round);
        // Sync HUD slots from the per-slot info + battle actor mirror.
        for i in 0..self.slots.len() {
            let info = &self.slots[i];
            if info.record.is_none() && info.name.is_empty() {
                self.hud.clear_slot(i as u8);
                continue;
            }
            // Pull live HP / MP from the BattleActor (slots 0..=7 must
            // exist in `world.actors` for `BattleRound::begin` to have
            // populated them).
            let (hp, hp_max, mp) = if let Some(actor) = world.actors.get(i) {
                (actor.battle.hp, actor.battle.max_hp, actor.battle.mp)
            } else {
                (0, 0, 0)
            };
            let ap = if info.is_party && i < 3 {
                Some(&world.ap_gauges[i])
            } else {
                None
            };
            self.hud.sync_slot(
                i as u8,
                SlotSyncInfo {
                    name: &info.name,
                    is_party: info.is_party,
                    alive: hp > 0,
                    hp,
                    hp_max,
                    mp,
                    mp_max: info.mp_max,
                    ap,
                },
            );
            self.hud.sync_status(i as u8, &world.status_effects);
        }
        self.transition(BattlePhase::RoundIntro);
        self.round.as_ref().unwrap()
    }

    /// One-frame tick. Engines call this every render frame; the session
    /// drives the phase SM, forwards input to the runner during
    /// `CommandInput`, and folds drained world events into the HUD.
    ///
    /// Returns the events fired this frame (in order).
    pub fn tick(&mut self, world: &mut World, input: SessionInput) -> Vec<SessionEvent> {
        let mut out = Vec::new();
        self.phase_frames = self.phase_frames.saturating_add(1);

        // Drive the phase SM first - auto-advance might happen even when
        // the player provides no input.
        match self.phase {
            BattlePhase::Idle
            | BattlePhase::Victory
            | BattlePhase::Defeat
            | BattlePhase::Escaped => {
                // Terminal / pre-init: nothing to do.
                return out;
            }
            BattlePhase::RoundIntro => {
                // Auto-advance to CommandInput after `intro_frames` (or on
                // Cross press for "skip splash").
                if self.phase_frames >= self.intro_frames || input.cross {
                    self.transition_emit(BattlePhase::CommandInput, &mut out);
                }
            }
            BattlePhase::CommandInput => {
                self.tick_command_input(world, input, &mut out);
            }
            BattlePhase::Resolve => {
                // The session owns the action SM during Resolve: arm it
                // for the next attacker, run `world.tick()` one frame,
                // route StepOutcome transitions into HUD damage popups +
                // session events, then advance to the next attacker on
                // EndOfAction. Engines that prefer to drive `world.tick()`
                // themselves can skip [`Self::install_resolve_queue`] and
                // fall through the legacy "observe events only" path.
                if self.resolve_driver.is_some() {
                    self.step_resolve(world, &mut out);
                } else {
                    self.drain_world_events(world, &mut out);
                    if !matches!(self.runner.state(), BattleRunnerState::Committed) {
                        self.transition_emit(BattlePhase::RoundOutro, &mut out);
                    }
                }
            }
            BattlePhase::RoundOutro => {
                if self.phase_frames >= self.outro_frames {
                    self.end_round_and_check_wipe(world, &mut out);
                }
            }
        }
        // Tick HUD popups after the phase logic - popups queued this frame
        // get one full frame of visibility before fade.
        self.hud.tick();
        out
    }

    pub(super) fn transition(&mut self, next: BattlePhase) {
        self.phase = next;
        self.phase_frames = 0;
    }

    pub(super) fn transition_emit(&mut self, next: BattlePhase, out: &mut Vec<SessionEvent>) {
        let prev = self.phase;
        if prev == next {
            return;
        }
        self.transition(next);
        out.push(SessionEvent::PhaseChanged {
            from: prev,
            to: next,
        });
    }

    /// Manually transition to the `Escaped` terminal phase - engines call
    /// this when an escape resolves outside the SM run band (the Escape
    /// spell / smoke-item paths roll in `World` rather than through the
    /// `0x64..0x67` states). The SM-driven Run command surfaces the typed
    /// [`BattleEndCause::Escaped`] through the battle-end handler instead
    /// (the retail `0x66` teardown).
    pub fn flag_escape(&mut self) {
        let mut sink = Vec::new();
        self.transition_emit(BattlePhase::Escaped, &mut sink);
    }

    /// Current sub-phase of [`BattlePhase::CommandInput`]. Engines query
    /// this each frame to decide whether to render the command menu
    /// ([`SubPhase::CommandSelect`]) or the target cursor overlay
    /// ([`SubPhase::TargetPick`]).
    pub fn sub_phase(&self) -> SubPhase {
        if self.target_picker.is_some() {
            SubPhase::TargetPick
        } else {
            SubPhase::CommandSelect
        }
    }
}
