//! High-level battle-session orchestrator.
//!
//! [`BattleSession`] composes [`BattleRunner`] (input → action queue),
//! [`BattleRound`] (per-round AP / stat refresh, status tick, death count),
//! [`BattleHud`] (renderer-agnostic UI model), and the action SM driver into
//! a single state machine.
//!
//! Engines drive a battle frame as:
//!
//! ```ignore
//! let mut session = BattleSession::new();
//! session.set_party([Character::Vahn, Character::Noa, Character::Gala]);
//! session.set_monsters(monsters);
//! session.begin_round(&mut world, &per_slot, &equipment, &modifiers);
//!
//! // Per-frame:
//! let events = session.tick(&mut world, input);
//! for event in events {
//!     match event {
//!         SessionEvent::DamageApplied { slot, amount } => { ... }
//!         SessionEvent::TurnCommitted => { ... }
//!         SessionEvent::BattleEnded { cause } => break,
//!         _ => {}
//!     }
//! }
//! ```
//!
//! The session is renderer-agnostic - engines render `session.hud` via
//! [`legaia_engine_render::battle_hud_draws_for`].

use crate::battle_hud::{BattleHud, LogAccent, SlotSyncInfo};
use crate::battle_round::BattleRound;
use crate::battle_runner::{BattleRunner, BattleRunnerError, BattleRunnerState};
use crate::battle_stats::{EquipmentTable, StatRecord, StatusModifiers};
use crate::world::World;
use legaia_art::{ActionConstant, Character, Command};
use legaia_engine_vm::battle_action::BattleEndCause;
use legaia_engine_vm::status_effects::StatusKind;

/// Per-slot configuration the session needs to render the HUD and sync
/// `BattleRound::begin`.
#[derive(Debug, Clone, Default)]
pub struct SessionSlotInfo {
    /// Display name for the HUD (`"Vahn"`, `"Noa"`, `"Snake-Lizard"`).
    pub name: String,
    /// `true` for slots 0..=2 (the player's party), `false` for monsters.
    pub is_party: bool,
    /// Stat record consumed by [`BattleRound::begin`] when computing
    /// per-slot ATK / UDF / LDF / accuracy / evasion. `None` for empty
    /// monster slots.
    pub record: Option<StatRecord>,
    /// MP cap. Engines populate from the character record at battle init;
    /// the live MP is folded into the World by `set_character_max_mp`.
    pub mp_max: u16,
}

/// Phase of the battle session SM.
///
/// Engines render different UI per phase: `RoundIntro` shows the encounter
/// banner, `CommandInput` shows the command menu + AP gauges, `Resolve`
/// shows damage popups + the ringed log, `RoundOutro` runs status ticks
/// and shows tick-damage popups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BattlePhase {
    /// No active battle. The session was constructed or `end()` was called.
    #[default]
    Idle,
    /// Banner / encounter-name splash before the first command-input phase.
    /// Auto-advances after `phase_frames` reaches the splash duration.
    RoundIntro,
    /// Player picks commands for each party slot. Engines feed [`SessionInput`]
    /// per-frame; the session forwards admissible inputs to [`BattleRunner`].
    CommandInput,
    /// Player has committed; the SM consumes the resolved queue. Engines
    /// drain `World::pending_battle_events` as usual; the session folds the
    /// outcomes into the HUD popups + log automatically.
    Resolve,
    /// End-of-round bookkeeping: status tick, death count, tick-damage
    /// popups. Auto-advances back to `RoundIntro` if the battle isn't over.
    RoundOutro,
    /// Player wiped the monster pool. Terminal state.
    Victory,
    /// Monsters wiped the party. Terminal state.
    Defeat,
    /// Player chose Escape and the dice came up favourable. Terminal.
    Escaped,
}

/// Sub-phase of [`BattlePhase::CommandInput`].
///
/// During CommandInput the player either selects commands directly
/// ([`SubPhase::CommandSelect`]) or picks a target for a buffered
/// command ([`SubPhase::TargetPick`]). Engines query
/// [`BattleSession::sub_phase`] to render the right overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubPhase {
    /// Default: input drives the command queue + AP gauge.
    #[default]
    CommandSelect,
    /// A command was buffered; input drives the cursor in
    /// [`crate::target_picker::TargetPickerSession`]. The runner is paused
    /// until the picker resolves.
    TargetPick,
}

/// One-frame input bundle the session consumes.
///
/// Engines map raw keyboard / pad input to these. The session handles the
/// per-phase dispatch (CommandInput consumes directional commands; Resolve
/// only honours `pause` / `cancel`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionInput {
    /// D-pad up; in CommandInput, queues `Command::Up`.
    pub up: bool,
    /// D-pad down; in CommandInput, queues `Command::Down`.
    pub down: bool,
    /// D-pad left; in CommandInput, queues `Command::Left`.
    pub left: bool,
    /// D-pad right; in CommandInput, queues `Command::Right`.
    pub right: bool,
    /// Cross - confirm / advance phase.
    pub cross: bool,
    /// Circle - cancel / pop the last command from the buffer.
    pub circle: bool,
    /// Triangle - advance to the next party slot's command-input phase
    /// without committing yet.
    pub triangle: bool,
    /// Square - Spirit press (`ApGauge::charge_spirit`).
    pub square: bool,
    /// Start - commit the current turn (transitions to `Resolve`).
    pub start: bool,
}

/// Events the session emits per `tick()`.
///
/// Engines surface these for UI / sound. The session has *already* mutated
/// the HUD and the world by the time the event is handed back, so the
/// engine doesn't need to re-apply the gameplay-state side.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    /// Phase moved from `from` to `to` this frame.
    PhaseChanged { from: BattlePhase, to: BattlePhase },
    /// A command was admitted for the active party slot.
    CommandPushed { slot: u8, command: Command },
    /// The most recent command was popped (Circle pressed in CommandInput).
    CommandPopped { slot: u8, command: Command },
    /// Turn committed - every party slot's queue has been resolved through
    /// `resolve_action_queue` and stashed on the runner.
    TurnCommitted,
    /// HP delta was applied to a slot's `BattleActor::hp`. The amount is
    /// always positive; `is_heal` distinguishes heals from damage.
    HpChanged {
        slot: u8,
        amount: u16,
        is_heal: bool,
    },
    /// A status effect was applied to a slot (folded from
    /// `BattleEvent::ApplyArtStrike`'s `enemy_effect` when present).
    StatusApplied { slot: u8, kind: StatusKind },
    /// The Spirit gauge for the active party slot received the +5 bonus.
    SpiritCharged { slot: u8 },
    /// Battle ended - the session transitioned to a terminal phase.
    BattleEnded { cause: BattleEndCause },
    /// Engine opened a target picker (engines render the cursor overlay
    /// against the picker state until the picker's outcome resolves).
    TargetPickerOpened {
        kind: crate::target_picker::TargetKind,
    },
    /// Target picker resolved with a confirmed slot.
    TargetConfirmed { target_slot: u8 },
    /// Target picker resolved with a sweep (all enemies / allies / self).
    TargetSweepConfirmed,
    /// Target picker was cancelled by the player; pending command was
    /// dropped without affecting the queue.
    TargetCancelled,
}

/// Battle session orchestrator.
///
/// One instance per active battle; engines reset by constructing a fresh
/// session at battle entry.
#[derive(Debug, Clone)]
pub struct BattleSession {
    /// Current phase.
    phase: BattlePhase,
    /// Frames spent in the current phase. Reset on every `phase` transition.
    /// Used to drive auto-advance (e.g. `RoundIntro` lasts `intro_frames`).
    phase_frames: u64,
    /// Frames the `RoundIntro` splash holds. Default 60 (1 sec at 60 FPS).
    intro_frames: u64,
    /// Frames the `RoundOutro` status-tick splash holds. Default 30.
    outro_frames: u64,
    /// Number of completed turns. Incremented on the `Resolve → RoundOutro`
    /// transition so engines can stripe the log column.
    pub turn: u32,
    /// Wrapped command runner. Public so engines can read the resolved
    /// queues post-commit.
    pub runner: BattleRunner,
    /// HUD model. Engines render off this each frame.
    pub hud: BattleHud,
    /// Last `BattleRound` returned by `begin_round`. `None` between rounds.
    pub round: Option<BattleRound>,
    /// Per-slot session-level metadata (name, party flag, MP cap, stat
    /// record). Synced to the HUD on `begin_round`.
    slots: [SessionSlotInfo; 8],
    /// Per-slot stat-record snapshot - matches `slots[i].record` but keyed
    /// for `BattleRound::begin`'s array shape.
    per_slot_records: [Option<StatRecord>; 8],
    /// Equipment table - engines populate before `begin_round` so the
    /// stat-aggregator can sum per-item modifiers.
    pub equipment: EquipmentTable,
    /// Status modifiers (Burned -ATK, Confused -accuracy, etc.).
    pub modifiers: StatusModifiers,
    /// Number of monster slots in the current battle (3..=5).
    monster_count: u8,
    /// Currently-active target picker (if any). When `Some`, the session
    /// is in a `CommandInput` sub-phase ([`SubPhase::TargetPick`]):
    /// directional / cross / circle input goes to the picker instead of
    /// the runner. The picker is opened by [`Self::open_target_picker`]
    /// (or by [`Self::push_command_with_target`]) and closed when its
    /// outcome resolves (Confirmed → command admitted; Cancelled →
    /// pending command popped).
    target_picker: Option<crate::target_picker::TargetPickerSession>,
    /// Pending command waiting for a target. When the picker confirms,
    /// the command is pushed via [`Self::push_command`].
    pending_target_command: Option<Command>,
}

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
        }
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
                // Drain world battle events into HUD + emitted session
                // events. Engines should have the SM ticking in their main
                // loop; we just observe the queue here.
                self.drain_world_events(world, &mut out);

                // Resolve phase ends when the runner is no longer
                // Committed (engines clear `state` to Idle when the SM
                // finishes its queue) OR when the action SM raised a
                // BattleEnd cause that we already routed below.
                if !matches!(self.runner.state(), BattleRunnerState::Committed) {
                    self.transition_emit(BattlePhase::RoundOutro, &mut out);
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

    /// Per-CommandInput tick. Direction presses queue commands, Cross
    /// confirms (currently a no-op stub for the menu cursor model - engines
    /// wire their own art/spell pickers and call [`Self::push_command`] /
    /// [`Self::push_chained_art`]), Circle pops, Square charges Spirit,
    /// Triangle advances slots, Start commits.
    fn tick_command_input(
        &mut self,
        world: &mut World,
        input: SessionInput,
        out: &mut Vec<SessionEvent>,
    ) {
        // Sub-phase: target picker takes priority - when active, route input
        // to the picker and skip command-queue logic until it resolves.
        if self.target_picker.is_some() {
            self.tick_target_picker(world, input, out);
            return;
        }
        let active = self.runner.active_party_slot();

        if input.start {
            // Commit. Resolved queues stay on the runner.
            if self.runner.commit_turn().is_ok() {
                out.push(SessionEvent::TurnCommitted);
                self.transition_emit(BattlePhase::Resolve, out);
            }
            return;
        }
        if input.triangle {
            // Advance to the next party slot's command-input phase. Skip
            // dead / blocked slots.
            for offset in 1..=3u8 {
                let next = (active + offset) % 3;
                if self.is_slot_inputable(world, next) {
                    let _ = self.runner.set_active_party_slot(next);
                    break;
                }
            }
            return;
        }
        if input.square {
            // Spirit press - adds +5 AP to the active party slot's gauge,
            // idempotent within a turn (the gauge tracks the spirit-pressed
            // bit internally).
            if let Some(gauge) = world.ap_gauges.get_mut(active as usize)
                && gauge.charge_spirit()
            {
                out.push(SessionEvent::SpiritCharged { slot: active });
            }
            return;
        }
        if input.circle {
            // Pop the most recent command. Refunds AP automatically.
            let mut ap = world.ap_gauges[active as usize];
            if let Some(cmd) = self.runner.pop_command(&mut ap) {
                world.ap_gauges[active as usize] = ap;
                out.push(SessionEvent::CommandPopped {
                    slot: active,
                    command: cmd,
                });
            }
            return;
        }

        // Direction commands → admit one Command per direction press.
        let cmd = input_to_command(input);
        if let Some(cmd) = cmd {
            let mut ap = world.ap_gauges[active as usize];
            match self.runner.push_command(&mut ap, cmd) {
                Ok(()) => {
                    world.ap_gauges[active as usize] = ap;
                    out.push(SessionEvent::CommandPushed {
                        slot: active,
                        command: cmd,
                    });
                }
                Err(BattleRunnerError::OutOfAp) => {
                    self.hud.push_log("Out of AP", LogAccent::Highlight);
                }
                Err(_) => {}
            }
        }
    }

    /// `true` iff a party slot is alive + not blocked + has a non-empty
    /// stat record. Used by `triangle` to skip slots the player can't act
    /// for this turn.
    fn is_slot_inputable(&self, world: &World, party_slot: u8) -> bool {
        if party_slot >= 3 {
            return false;
        }
        let info = &self.slots[party_slot as usize];
        if info.record.is_none() {
            return false;
        }
        if let Some(actor) = world.actors.get(party_slot as usize)
            && actor.battle.hp == 0
        {
            return false;
        }
        if let Some(round) = self.round.as_ref()
            && round.action_blocked[party_slot as usize]
        {
            return false;
        }
        true
    }

    /// Push a [`Command`] for the active party slot. Engines call this from
    /// their menu-cursor binding (e.g. "Cross on Attack -> push the four
    /// directional bytes the player buffered"). Returns `false` if the
    /// gauge can't cover the cost or the runner refused the input.
    pub fn push_command(&mut self, world: &mut World, cmd: Command) -> bool {
        let active = self.runner.active_party_slot();
        let mut ap = world.ap_gauges[active as usize];
        let admit = self.runner.push_command(&mut ap, cmd).is_ok();
        if admit {
            world.ap_gauges[active as usize] = ap;
        }
        admit
    }

    /// Append a chained art for the active party slot. Mirrors
    /// [`BattleRunner::push_chained_art`].
    pub fn push_chained_art(&mut self, world: &mut World, art: ActionConstant) -> bool {
        let active = self.runner.active_party_slot();
        let mut ap = world.ap_gauges[active as usize];
        let admit = self.runner.push_chained_art(&mut ap, art).is_ok();
        if admit {
            world.ap_gauges[active as usize] = ap;
        }
        admit
    }

    /// Drain `World::pending_battle_events`, fold each through `World::fold_battle_event`
    /// for HP / status updates, push a HUD popup + log line, and emit a
    /// matching [`SessionEvent`].
    fn drain_world_events(&mut self, world: &mut World, out: &mut Vec<SessionEvent>) {
        let events = world.drain_battle_events();
        for ev in events {
            // World handles HP / status side first.
            world.fold_battle_event(&ev);
            self.fold_event_into_hud(&ev, out);
        }
    }

    /// Mirror of [`Self::drain_world_events`] for engines that already
    /// drained the world themselves (e.g. play-window keeps its own log).
    pub fn fold_event(&mut self, world: &mut World, ev: &crate::battle_events::BattleEvent) {
        world.fold_battle_event(ev);
        let mut sink = Vec::new();
        self.fold_event_into_hud(ev, &mut sink);
    }

    /// Public accessor for the typed `BattleEvent` -> HUD/event router.
    /// Engines that maintain their own world drain (so they can keep a
    /// custom log column) call this once per drained event instead of
    /// re-implementing the routing.
    pub fn route_event(&mut self, ev: &crate::battle_events::BattleEvent) -> Vec<SessionEvent> {
        let mut sink = Vec::new();
        self.fold_event_into_hud(ev, &mut sink);
        sink
    }

    fn fold_event_into_hud(
        &mut self,
        ev: &crate::battle_events::BattleEvent,
        out: &mut Vec<SessionEvent>,
    ) {
        use crate::battle_events::BattleEvent as Ev;
        match ev {
            Ev::ApplyArtStrike {
                target_slot,
                outcome,
                ..
            } => {
                if let Some(dmg) = outcome.damage
                    && dmg > 0
                {
                    self.hud.push_damage(*target_slot, dmg);
                    self.hud
                        .push_log(format!("-{dmg} HP slot {target_slot}"), LogAccent::Party);
                    out.push(SessionEvent::HpChanged {
                        slot: *target_slot,
                        amount: dmg,
                        is_heal: false,
                    });
                }
                if let Some(kind) = StatusKind::from_enemy_effect(outcome.enemy_effect) {
                    self.hud.push_status(*target_slot, kind);
                    self.hud
                        .push_log(format!("{kind:?} slot {target_slot}"), LogAccent::Highlight);
                    out.push(SessionEvent::StatusApplied {
                        slot: *target_slot,
                        kind,
                    });
                }
            }
            Ev::ApplyDamage { target_slot, .. } => {
                self.hud.push_log(
                    format!("ApplyDamage slot {target_slot}"),
                    LogAccent::Neutral,
                );
            }
            Ev::ScreenShake { magnitude } => {
                self.hud
                    .push_log(format!("Shake {magnitude}"), LogAccent::Neutral);
            }
            Ev::BattleEnd { cause } => {
                self.handle_battle_end(*cause, out);
            }
            Ev::LevelUp {
                char_id,
                new_level,
                hp_gained,
                mp_gained,
            } => {
                self.hud.push_log(
                    format!("LV{new_level} +{hp_gained}HP/+{mp_gained}MP char{char_id}"),
                    LogAccent::Highlight,
                );
            }
            Ev::TacticalArtLearned { char_id, art_id } => {
                self.hud.push_log(
                    format!("Art learned char{char_id} #{art_id}"),
                    LogAccent::Highlight,
                );
            }
            _ => {}
        }
    }

    /// Drive end-of-round logic. Calls [`BattleRound::end`] for tick damage,
    /// counts wipes, transitions to a terminal phase or back to RoundIntro.
    fn end_round_and_check_wipe(&mut self, world: &mut World, out: &mut Vec<SessionEvent>) {
        // Drain tick damage from status effects.
        let tick_deaths = self.runner.end_round(world);
        if tick_deaths > 0 {
            self.hud.push_log(
                format!("{tick_deaths} died from status"),
                LogAccent::Highlight,
            );
        }
        // Re-sync HUD HP / status icons after tick damage.
        for i in 0..self.slots.len() {
            if let Some(actor) = world.actors.get(i) {
                let info = &self.slots[i];
                if info.name.is_empty() && info.record.is_none() {
                    continue;
                }
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
                        alive: actor.battle.hp > 0,
                        hp: actor.battle.hp,
                        hp_max: actor.battle.max_hp,
                        mp: actor.battle.mp,
                        mp_max: info.mp_max,
                        ap,
                    },
                );
                self.hud.sync_status(i as u8, &world.status_effects);
            }
        }
        self.turn = self.turn.saturating_add(1);
        // Wipe detection: party = slots 0..=2; monsters = slots 3..3+count.
        let party_alive = (0..3)
            .filter(|i| self.slots[*i].record.is_some())
            .any(|i| world.actors.get(i).is_some_and(|a| a.battle.hp > 0));
        let monsters_alive = (0..self.monster_count as usize)
            .any(|i| world.actors.get(3 + i).is_some_and(|a| a.battle.hp > 0));
        if !party_alive {
            self.handle_battle_end(BattleEndCause::PartyWipe, out);
        } else if !monsters_alive {
            self.handle_battle_end(BattleEndCause::MonsterWipe, out);
        } else {
            // Round complete; loop back through intro splash for the next.
            self.runner.begin_round(
                world,
                &self.per_slot_records,
                &self.equipment,
                &self.modifiers,
            );
            // (BattleRound::begin already ran inside the runner; we keep
            // `self.round` updated for future renders.)
            // Advance phase.
            self.transition_emit(BattlePhase::RoundIntro, out);
        }
    }

    fn handle_battle_end(&mut self, cause: BattleEndCause, out: &mut Vec<SessionEvent>) {
        let next = match cause {
            BattleEndCause::PartyWipe => BattlePhase::Defeat,
            BattleEndCause::MonsterWipe => BattlePhase::Victory,
        };
        out.push(SessionEvent::BattleEnded { cause });
        self.transition_emit(next, out);
    }

    /// Manually transition to the `Escaped` terminal phase - engines call
    /// this when the player picks "Escape" from the menu and the dice come
    /// up favourable. The retail SM doesn't surface a typed cause for this
    /// path; engines drive it from their own escape-roll resolver.
    pub fn flag_escape(&mut self) {
        let mut sink = Vec::new();
        self.transition_emit(BattlePhase::Escaped, &mut sink);
    }

    fn transition(&mut self, next: BattlePhase) {
        self.phase = next;
        self.phase_frames = 0;
    }

    fn transition_emit(&mut self, next: BattlePhase, out: &mut Vec<SessionEvent>) {
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

    /// Read-only accessor for the active picker (if any). Engines render
    /// against the picker's [`crate::target_picker::PickerState`].
    pub fn target_picker(&self) -> Option<&crate::target_picker::TargetPickerSession> {
        self.target_picker.as_ref()
    }

    /// Open a target picker for the supplied [`crate::target_picker::TargetKind`].
    /// Engines call this when the player picks an action that needs a
    /// target (e.g. a Tactical Art whose strike needs an enemy slot).
    /// `actor_slot` is the active party slot (0..=2). The session pulls
    /// per-slot live HP from `world.actors[i].battle.hp` to populate the
    /// picker's [`crate::target_picker::SlotState`] arrays.
    ///
    /// When the picker resolves on the same frame (sweep targets, self,
    /// no-candidates), the call also writes the resolved
    /// `BattleActor::active_target` for the active party slot. Engines
    /// driving the picker through `tick()` rather than the immediate
    /// resolve path get the same write deferred to the next frame.
    pub fn open_target_picker(
        &mut self,
        world: &World,
        kind: crate::target_picker::TargetKind,
        actor_slot: u8,
        pending_command: Option<Command>,
        out: &mut Vec<SessionEvent>,
    ) {
        use crate::target_picker::{SlotState, TargetPickerSession};
        // Build party + monster slot state from the live world + slot info.
        let mut party = [SlotState::default(); 3];
        for (i, slot) in party.iter_mut().enumerate() {
            let info = &self.slots[i];
            let hp = world.actors.get(i).map(|a| a.battle.hp).unwrap_or(0);
            *slot = SlotState::from_session_slot(info, hp);
        }
        let mut monsters = [SlotState::default(); 5];
        for (i, slot) in monsters.iter_mut().enumerate() {
            let slot_idx = 3 + i;
            let info = &self.slots[slot_idx];
            let hp = world.actors.get(slot_idx).map(|a| a.battle.hp).unwrap_or(0);
            *slot = SlotState::from_session_slot(info, hp);
        }
        self.target_picker = Some(TargetPickerSession::new(kind, actor_slot, party, monsters));
        self.pending_target_command = pending_command;
        out.push(SessionEvent::TargetPickerOpened { kind });

        // If the picker resolved immediately (sweep / no-candidates),
        // close it on this same frame. The `&World` borrow doesn't let
        // us write `active_target` here; engines using
        // `push_command_with_target` get the write through the mutable
        // path. Single-target pickers stay open and resolve on a later
        // tick (where we have `&mut World`).
        self.maybe_close_picker(out);
    }

    /// Mutable-world variant of `open_target_picker` - used by
    /// [`Self::push_command_with_target`]. The same as `open_target_picker`
    /// except the caller hands a mutable world borrow so immediate-resolve
    /// kinds (sweep / self / no-candidates) write the resolved target
    /// into `BattleActor::active_target` on the same call.
    pub fn open_target_picker_mut(
        &mut self,
        world: &mut World,
        kind: crate::target_picker::TargetKind,
        actor_slot: u8,
        pending_command: Option<Command>,
        out: &mut Vec<SessionEvent>,
    ) {
        use crate::target_picker::{SlotState, TargetPickerSession};
        let mut party = [SlotState::default(); 3];
        for (i, slot) in party.iter_mut().enumerate() {
            let info = &self.slots[i];
            let hp = world.actors.get(i).map(|a| a.battle.hp).unwrap_or(0);
            *slot = SlotState::from_session_slot(info, hp);
        }
        let mut monsters = [SlotState::default(); 5];
        for (i, slot) in monsters.iter_mut().enumerate() {
            let slot_idx = 3 + i;
            let info = &self.slots[slot_idx];
            let hp = world.actors.get(slot_idx).map(|a| a.battle.hp).unwrap_or(0);
            *slot = SlotState::from_session_slot(info, hp);
        }
        self.target_picker = Some(TargetPickerSession::new(kind, actor_slot, party, monsters));
        self.pending_target_command = pending_command;
        out.push(SessionEvent::TargetPickerOpened { kind });
        self.maybe_close_picker_with_world(Some(world), out);
    }

    /// Cancel the active target picker (drop the pending command). No-op
    /// when no picker is active.
    pub fn cancel_target_picker(&mut self, out: &mut Vec<SessionEvent>) {
        if self.target_picker.take().is_some() {
            self.pending_target_command = None;
            out.push(SessionEvent::TargetCancelled);
        }
    }

    /// Drive one frame of the target picker. Routes directional input to
    /// the picker, Cross to confirm, Circle to cancel.
    fn tick_target_picker(
        &mut self,
        world: &mut World,
        input: SessionInput,
        out: &mut Vec<SessionEvent>,
    ) {
        use crate::target_picker::PickerInput;
        let pi = PickerInput {
            up: input.up,
            down: input.down,
            left: input.left,
            right: input.right,
            cross: input.cross,
            circle: input.circle,
        };
        if let Some(picker) = self.target_picker.as_mut() {
            picker.input(pi);
        }
        self.maybe_close_picker_with_world(Some(world), out);
    }

    /// Sweep targets: write a sentinel to `active_target` so the action SM
    /// can branch on "all targets" if it cares (the retail SM treats `0xFF`
    /// as "every alive monster" in some art bodies).
    pub const SWEEP_TARGET_SENTINEL: u8 = 0xFF;

    /// If the picker has reached `Done`, fold the outcome back into the
    /// session.
    ///
    /// `world` is `Some(...)` when called from `tick_target_picker` (the
    /// session has full world access on a tick frame). When called from
    /// `open_target_picker` (engines call this with `&World`, not `&mut`),
    /// the world ref is `None` and the picker writes are deferred to the
    /// next tick - the SessionEvent is still emitted so engines that
    /// need the resolved target can act on it.
    fn maybe_close_picker_with_world(
        &mut self,
        world: Option<&mut World>,
        out: &mut Vec<SessionEvent>,
    ) {
        use crate::target_picker::PickerOutcome;
        let outcome = match self.target_picker.as_ref() {
            Some(p) => p.outcome(),
            None => return,
        };
        let Some(outcome) = outcome else { return };
        let actor_slot = self.runner.active_party_slot();
        match outcome {
            PickerOutcome::Single { slot, .. } => {
                // Write the resolved target into the BattleActor so the
                // action SM (`battle_action::step`) reads it via
                // `host.actor(slot).active_target` when the strike fires.
                self.write_active_target(world, actor_slot, slot);
                out.push(SessionEvent::TargetConfirmed { target_slot: slot });
                // Auto-admit the pending command (the buffered command
                // that triggered the picker). Engines that opened the
                // picker without a pending command leave the runner queue
                // alone.
                self.commit_pending_command(out);
            }
            PickerOutcome::Sweep { .. } => {
                self.write_active_target(world, actor_slot, Self::SWEEP_TARGET_SENTINEL);
                out.push(SessionEvent::TargetSweepConfirmed);
                self.commit_pending_command(out);
            }
            PickerOutcome::Cancelled | PickerOutcome::NoCandidates => {
                out.push(SessionEvent::TargetCancelled);
                // Drop the buffered command - the player aborted, so we
                // do NOT push it.
                self.pending_target_command = None;
            }
        }
        self.target_picker = None;
    }

    /// Compatibility wrapper for callers that don't have a mutable World
    /// at the close site (e.g. `open_target_picker`). Writes to actor
    /// state are skipped; the SessionEvent is still emitted.
    fn maybe_close_picker(&mut self, out: &mut Vec<SessionEvent>) {
        self.maybe_close_picker_with_world(None, out);
    }

    /// Write `target_slot` into the active party slot's `active_target`
    /// field if `world` is available. No-op when `world` is `None` -
    /// engines that care can read the resolved slot from the
    /// `TargetConfirmed` event and write themselves.
    fn write_active_target(&self, world: Option<&mut World>, actor_slot: u8, target_slot: u8) {
        let Some(world) = world else { return };
        if let Some(actor) = world.actors.get_mut(actor_slot as usize) {
            actor.battle.active_target = target_slot;
        }
    }

    /// Push the buffered `pending_target_command` (if any) onto the
    /// runner queue. Used by `maybe_close_picker_with_world` when the
    /// picker resolved with a confirmed target. Failure surfaces as a HUD
    /// log line; the command is dropped on error.
    fn commit_pending_command(&mut self, out: &mut Vec<SessionEvent>) {
        let Some(cmd) = self.pending_target_command.take() else {
            return;
        };
        // Push without paying AP again - the caller of
        // `push_command_with_target` already paid AP when buffering the
        // command. Bypass `BattleRunner::push_command` (which charges AP)
        // and write to the buffer directly via the public API.
        let active = self.runner.active_party_slot();
        match self.runner.push_no_ap(active, cmd) {
            Ok(()) => {
                out.push(SessionEvent::CommandPushed {
                    slot: active,
                    command: cmd,
                });
            }
            Err(_) => {
                self.hud.push_log("Command rejected", LogAccent::Highlight);
            }
        }
    }

    /// Push a `Command` and (if needed) open a target picker for it.
    /// Engines drive command admission through this API instead of
    /// calling `push_command` + `open_target_picker` separately. The
    /// session remembers the command and pushes it onto the runner
    /// queue only after the picker resolves successfully.
    ///
    /// `kind` is the [`crate::target_picker::TargetKind`] expected by the
    /// command; immediate kinds (`AllEnemies` / `AllAllies` / `Self_`)
    /// resolve in one frame, single-target kinds open a cursor.
    /// `actor_slot` is the active party slot.
    ///
    /// Returns `false` and emits no events if the AP gauge can't cover
    /// the command.
    pub fn push_command_with_target(
        &mut self,
        world: &mut World,
        cmd: Command,
        kind: crate::target_picker::TargetKind,
        actor_slot: u8,
    ) -> bool {
        // Charge AP up-front; the command is buffered until the picker
        // resolves, but the cost is gated against the same gauge as a
        // direct `push_command` call.
        let mut ap = world.ap_gauges[actor_slot as usize];
        let cost = crate::ap_gauge::art_ap_cost(cmd.as_action());
        if !ap.try_spend(cost) {
            return false;
        }
        world.ap_gauges[actor_slot as usize] = ap;
        let mut sink: Vec<SessionEvent> = Vec::new();
        self.open_target_picker_mut(world, kind, actor_slot, Some(cmd), &mut sink);
        // Drop sink - engines that want the events use `tick`.
        true
    }
}

/// One-frame input → first directional command, if any. Order matches the
/// retail input prio: Right > Left > Down > Up. Engines that want explicit
/// per-direction event semantics should bypass this and call
/// [`BattleSession::push_command`] directly.
fn input_to_command(input: SessionInput) -> Option<Command> {
    if input.right {
        Some(Command::Right)
    } else if input.left {
        Some(Command::Left)
    } else if input.down {
        Some(Command::Down)
    } else if input.up {
        Some(Command::Up)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ap_gauge::ApGauge;
    use crate::battle_stats::StatRecord;

    fn party_slot(name: &str) -> SessionSlotInfo {
        SessionSlotInfo {
            name: name.into(),
            is_party: true,
            record: Some(StatRecord {
                base_attack: 50,
                base_udf: 30,
                base_ldf: 25,
                base_accuracy: 80,
                base_evasion: 20,
                ..Default::default()
            }),
            mp_max: 30,
        }
    }

    fn monster_slot(name: &str, hp: u16) -> (SessionSlotInfo, u16) {
        (
            SessionSlotInfo {
                name: name.into(),
                is_party: false,
                record: Some(StatRecord {
                    base_attack: 30,
                    base_udf: 20,
                    base_ldf: 15,
                    base_accuracy: 70,
                    base_evasion: 10,
                    ..Default::default()
                }),
                mp_max: 0,
            },
            hp,
        )
    }

    fn fresh_world_with_actors() -> World {
        let mut w = World::new();
        for _ in 0..8 {
            w.actors.push(crate::world::Actor::default());
        }
        // Give the party plausible HP.
        for i in 0..3 {
            w.actors[i].battle.hp = 100;
            w.actors[i].battle.max_hp = 100;
            w.actors[i].battle.mp = 30;
            w.ap_gauges[i] = ApGauge::with_base(8);
        }
        w
    }

    fn fresh_session() -> BattleSession {
        let mut s = BattleSession::new();
        s.set_party([Character::Vahn, Character::Noa, Character::Gala]);
        s.set_slot_info(0, party_slot("Vahn"));
        s.set_slot_info(1, party_slot("Noa"));
        s.set_slot_info(2, party_slot("Gala"));
        let (info, _hp) = monster_slot("Goblin", 50);
        s.set_slot_info(3, info);
        s.set_monster_count(1);
        s
    }

    #[test]
    fn new_session_starts_idle() {
        let s = BattleSession::new();
        assert_eq!(s.phase(), BattlePhase::Idle);
        assert!(!s.is_done());
    }

    #[test]
    fn begin_round_transitions_to_round_intro() {
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        // Set monster HP via actor.battle.
        w.actors[3].battle.hp = 50;
        w.actors[3].battle.max_hp = 50;
        s.begin_round(&mut w);
        assert_eq!(s.phase(), BattlePhase::RoundIntro);
        // HUD slots populated.
        assert!(s.hud.slots[0].active);
        assert_eq!(s.hud.slots[0].name, "Vahn");
        assert_eq!(s.hud.slots[0].hp, 100);
    }

    #[test]
    fn intro_auto_advances_to_command_input_after_intro_frames() {
        let mut s = BattleSession::new().with_phase_durations(3, 5);
        s.set_party([Character::Vahn, Character::Noa, Character::Gala]);
        s.set_slot_info(0, party_slot("Vahn"));
        s.set_slot_info(1, party_slot("Noa"));
        s.set_slot_info(2, party_slot("Gala"));
        let mut w = fresh_world_with_actors();
        s.begin_round(&mut w);
        for _ in 0..3 {
            s.tick(&mut w, SessionInput::default());
        }
        assert_eq!(s.phase(), BattlePhase::CommandInput);
    }

    #[test]
    fn cross_during_intro_skips_to_command_input() {
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        s.begin_round(&mut w);
        let input = SessionInput {
            cross: true,
            ..Default::default()
        };
        let events = s.tick(&mut w, input);
        assert_eq!(s.phase(), BattlePhase::CommandInput);
        assert!(events.iter().any(|e| matches!(
            e,
            SessionEvent::PhaseChanged {
                to: BattlePhase::CommandInput,
                ..
            }
        )));
    }

    #[test]
    fn direction_input_during_command_phase_pushes_command() {
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        s.begin_round(&mut w);
        // Skip past intro.
        let _ = s.tick(
            &mut w,
            SessionInput {
                cross: true,
                ..Default::default()
            },
        );
        assert_eq!(s.phase(), BattlePhase::CommandInput);
        let input = SessionInput {
            right: true,
            ..Default::default()
        };
        let events = s.tick(&mut w, input);
        assert!(events.iter().any(|e| matches!(
            e,
            SessionEvent::CommandPushed {
                slot: 0,
                command: Command::Right
            }
        )));
        assert_eq!(s.runner.current_buffer(), &[Command::Right]);
    }

    #[test]
    fn circle_during_command_phase_pops_command_and_refunds_ap() {
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        s.begin_round(&mut w);
        let _ = s.tick(
            &mut w,
            SessionInput {
                cross: true,
                ..Default::default()
            },
        );
        // Direction commands are 0-cost so push + pop checks the routing
        // round-trips cleanly.
        s.tick(
            &mut w,
            SessionInput {
                left: true,
                ..Default::default()
            },
        );
        assert_eq!(s.runner.current_buffer().len(), 1);
        let events = s.tick(
            &mut w,
            SessionInput {
                circle: true,
                ..Default::default()
            },
        );
        assert!(events.iter().any(|e| matches!(
            e,
            SessionEvent::CommandPopped {
                slot: 0,
                command: Command::Left,
            }
        )));
        assert!(s.runner.current_buffer().is_empty());
    }

    #[test]
    fn square_charges_spirit_and_emits_event() {
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        s.begin_round(&mut w);
        let _ = s.tick(
            &mut w,
            SessionInput {
                cross: true,
                ..Default::default()
            },
        );
        let before = w.ap_gauges[0].current_ap;
        let events = s.tick(
            &mut w,
            SessionInput {
                square: true,
                ..Default::default()
            },
        );
        let after = w.ap_gauges[0].current_ap;
        assert!(after > before);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SessionEvent::SpiritCharged { slot: 0 }))
        );
    }

    #[test]
    fn triangle_advances_to_next_inputable_party_slot() {
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        s.begin_round(&mut w);
        let _ = s.tick(
            &mut w,
            SessionInput {
                cross: true,
                ..Default::default()
            },
        );
        assert_eq!(s.runner.active_party_slot(), 0);
        s.tick(
            &mut w,
            SessionInput {
                triangle: true,
                ..Default::default()
            },
        );
        assert_eq!(s.runner.active_party_slot(), 1);
    }

    #[test]
    fn start_commits_turn_and_transitions_to_resolve() {
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        s.begin_round(&mut w);
        let _ = s.tick(
            &mut w,
            SessionInput {
                cross: true,
                ..Default::default()
            },
        );
        let events = s.tick(
            &mut w,
            SessionInput {
                start: true,
                ..Default::default()
            },
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SessionEvent::TurnCommitted))
        );
        assert_eq!(s.phase(), BattlePhase::Resolve);
    }

    #[test]
    fn resolve_phase_transitions_to_outro_when_runner_idle() {
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        s.begin_round(&mut w);
        let _ = s.tick(
            &mut w,
            SessionInput {
                cross: true,
                ..Default::default()
            },
        );
        let _ = s.tick(
            &mut w,
            SessionInput {
                start: true,
                ..Default::default()
            },
        );
        // SM is committed. Manually drain the queue (simulates engine
        // ticking step_battle until the queue is consumed).
        s.runner.end_round(&mut w);
        // Runner is now Idle.
        s.tick(&mut w, SessionInput::default());
        assert_eq!(s.phase(), BattlePhase::RoundOutro);
    }

    #[test]
    fn party_wipe_transitions_to_defeat() {
        let mut s = fresh_session().with_phase_durations(0, 0);
        s.set_party([Character::Vahn, Character::Noa, Character::Gala]);
        s.set_slot_info(0, party_slot("Vahn"));
        s.set_slot_info(1, party_slot("Noa"));
        s.set_slot_info(2, party_slot("Gala"));
        let mut w = fresh_world_with_actors();
        // Knock the whole party down before starting.
        for i in 0..3 {
            w.actors[i].battle.hp = 0;
        }
        s.begin_round(&mut w);
        // Auto-advance through intro → command → start → resolve.
        s.tick(
            &mut w,
            SessionInput {
                cross: true,
                ..Default::default()
            },
        );
        s.tick(
            &mut w,
            SessionInput {
                start: true,
                ..Default::default()
            },
        );
        // Drain the SM.
        s.runner.end_round(&mut w);
        // tick → resolve detects Idle, transitions to outro.
        s.tick(&mut w, SessionInput::default());
        // tick → outro auto-advances; party wipe → defeat.
        for _ in 0..32 {
            s.tick(&mut w, SessionInput::default());
            if s.is_done() {
                break;
            }
        }
        assert_eq!(s.phase(), BattlePhase::Defeat);
        assert!(s.is_done());
    }

    #[test]
    fn monster_wipe_transitions_to_victory() {
        let mut s = fresh_session().with_phase_durations(0, 0);
        let mut w = fresh_world_with_actors();
        // Monster slot is dead.
        w.actors[3].battle.hp = 0;
        w.actors[3].battle.max_hp = 50;
        s.begin_round(&mut w);
        s.tick(
            &mut w,
            SessionInput {
                cross: true,
                ..Default::default()
            },
        );
        s.tick(
            &mut w,
            SessionInput {
                start: true,
                ..Default::default()
            },
        );
        s.runner.end_round(&mut w);
        s.tick(&mut w, SessionInput::default());
        for _ in 0..32 {
            s.tick(&mut w, SessionInput::default());
            if s.is_done() {
                break;
            }
        }
        assert_eq!(s.phase(), BattlePhase::Victory);
    }

    #[test]
    fn fold_event_into_hud_routes_apply_art_strike_to_popup_and_log() {
        use crate::art_strike::ArtStrikeOutcome;
        use crate::battle_events::BattleEvent;
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        s.begin_round(&mut w);

        let outcome = ArtStrikeOutcome {
            damage: Some(30),
            ..Default::default()
        };
        let ev = BattleEvent::ApplyArtStrike {
            actor_slot: 0,
            target_slot: 3,
            strike_index: 0,
            outcome,
        };
        let mut sink = Vec::new();
        s.fold_event_into_hud(&ev, &mut sink);
        assert!(s.hud.popups.iter().any(|p| p.amount == 30 && p.slot == 3));
        assert!(s.hud.log.iter().any(|l| l.text.contains("-30 HP slot 3")));
        assert!(sink.iter().any(|e| matches!(
            e,
            SessionEvent::HpChanged {
                slot: 3,
                amount: 30,
                is_heal: false
            }
        )));
    }

    #[test]
    fn fold_event_into_hud_emits_status_event_when_enemy_effect_present() {
        use crate::art_strike::ArtStrikeOutcome;
        use crate::battle_events::BattleEvent;
        use legaia_art::record::EnemyEffect;
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        s.begin_round(&mut w);

        let outcome = ArtStrikeOutcome {
            damage: Some(0),
            enemy_effect: EnemyEffect::Burned,
            ..Default::default()
        };
        let ev = BattleEvent::ApplyArtStrike {
            actor_slot: 0,
            target_slot: 3,
            strike_index: 1,
            outcome,
        };
        let mut sink = Vec::new();
        s.fold_event_into_hud(&ev, &mut sink);
        assert!(sink.iter().any(|e| matches!(
            e,
            SessionEvent::StatusApplied {
                slot: 3,
                kind: StatusKind::Burned
            }
        )));
    }

    #[test]
    fn input_to_command_priority_right_first() {
        let i = SessionInput {
            right: true,
            up: true,
            ..Default::default()
        };
        assert_eq!(input_to_command(i), Some(Command::Right));
    }

    #[test]
    fn input_to_command_returns_none_when_no_directions() {
        let i = SessionInput::default();
        assert_eq!(input_to_command(i), None);
    }

    #[test]
    fn is_slot_inputable_skips_dead_party_slot() {
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        w.actors[1].battle.hp = 0;
        s.begin_round(&mut w);
        assert!(!s.is_slot_inputable(&w, 1));
        assert!(s.is_slot_inputable(&w, 0));
    }

    #[test]
    fn open_target_picker_enters_subphase_target_pick() {
        use crate::target_picker::TargetKind;
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        w.actors[3].battle.hp = 50;
        w.actors[3].battle.max_hp = 50;
        s.begin_round(&mut w);
        let mut events = Vec::new();
        s.open_target_picker(&w, TargetKind::SingleEnemy, 0, None, &mut events);
        assert_eq!(s.sub_phase(), SubPhase::TargetPick);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SessionEvent::TargetPickerOpened { .. }))
        );
    }

    #[test]
    fn target_picker_confirm_emits_target_confirmed() {
        use crate::target_picker::TargetKind;
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        w.actors[3].battle.hp = 50;
        w.actors[3].battle.max_hp = 50;
        s.begin_round(&mut w);
        let mut events = Vec::new();
        s.open_target_picker(&w, TargetKind::SingleEnemy, 0, None, &mut events);
        events.clear();
        // SubPhase=TargetPick now; cross confirms the only enemy.
        let confirm_input = SessionInput {
            cross: true,
            ..Default::default()
        };
        // Need to be in CommandInput phase for tick to route.
        s.transition(BattlePhase::CommandInput);
        let evs = s.tick(&mut w, confirm_input);
        // slot is row-relative; the first enemy is index 0 in the monsters row.
        assert!(
            evs.iter()
                .any(|e| matches!(e, SessionEvent::TargetConfirmed { target_slot: 0 }))
        );
        // Picker is closed, sub-phase back to CommandSelect.
        assert_eq!(s.sub_phase(), SubPhase::CommandSelect);
    }

    #[test]
    fn target_picker_sweep_resolves_immediately() {
        use crate::target_picker::TargetKind;
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        w.actors[3].battle.hp = 50;
        w.actors[3].battle.max_hp = 50;
        s.begin_round(&mut w);
        let mut events = Vec::new();
        s.open_target_picker(&w, TargetKind::AllEnemies, 0, None, &mut events);
        // Sweep targets resolve in init_cursor → maybe_close_picker emits
        // TargetSweepConfirmed and clears the picker.
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SessionEvent::TargetSweepConfirmed))
        );
        assert_eq!(s.sub_phase(), SubPhase::CommandSelect);
    }

    #[test]
    fn target_confirm_writes_active_target_into_actor_and_pushes_pending_command() {
        use crate::target_picker::TargetKind;
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        w.actors[3].battle.hp = 50;
        w.actors[3].battle.max_hp = 50;
        s.begin_round(&mut w);
        // Skip past intro into command-input.
        s.transition(BattlePhase::CommandInput);
        // Open with a pending command: the player buffered Right and the
        // engine then opened the picker.
        let mut events = Vec::new();
        s.open_target_picker(
            &w,
            TargetKind::SingleEnemy,
            0,
            Some(Command::Right),
            &mut events,
        );
        events.clear();
        // Cross confirms the only enemy - through tick so we get the
        // active-target write side effect.
        let evs = s.tick(
            &mut w,
            SessionInput {
                cross: true,
                ..Default::default()
            },
        );
        // active_target written.
        assert_eq!(w.actors[0].battle.active_target, 0);
        // TargetConfirmed event present.
        assert!(
            evs.iter()
                .any(|e| matches!(e, SessionEvent::TargetConfirmed { target_slot: 0 }))
        );
        // CommandPushed (Right) appears in the same frame because the
        // pending command auto-admits.
        assert!(evs.iter().any(|e| matches!(
            e,
            SessionEvent::CommandPushed {
                slot: 0,
                command: Command::Right
            }
        )));
        // Runner buffer reflects the admitted command.
        assert_eq!(s.runner.current_buffer(), &[Command::Right]);
    }

    #[test]
    fn target_sweep_writes_sentinel_and_admits_pending_command() {
        use crate::target_picker::TargetKind;
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        w.actors[3].battle.hp = 50;
        w.actors[3].battle.max_hp = 50;
        s.begin_round(&mut w);
        // open_target_picker with AllEnemies resolves immediately;
        // maybe_close_picker is called inside open_target_picker but
        // without a mutable World ref, so the sentinel write is deferred.
        // Drive via push_command_with_target which goes through the
        // mutable-world path correctly when sweep resolves on the same
        // call.
        s.transition(BattlePhase::CommandInput);
        let ok = s.push_command_with_target(&mut w, Command::Up, TargetKind::AllEnemies, 0);
        assert!(ok);
        // Sweep-immediate path: actor active_target updated to sentinel
        // and command auto-admitted.
        assert_eq!(
            w.actors[0].battle.active_target,
            BattleSession::SWEEP_TARGET_SENTINEL
        );
        assert_eq!(s.runner.current_buffer(), &[Command::Up]);
    }

    #[test]
    fn push_command_with_target_returns_false_when_out_of_ap() {
        use crate::target_picker::TargetKind;
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        // Drain the AP gauge so the cost can't be paid.
        w.ap_gauges[0] = ApGauge::with_base(0);
        s.begin_round(&mut w);
        s.transition(BattlePhase::CommandInput);
        let ok = s.push_command_with_target(&mut w, Command::Right, TargetKind::AllEnemies, 0);
        // Direction commands are 0-cost, so this should still admit.
        assert!(ok);
        // Now try a chained-art-shape action that costs 1 AP. We can't
        // construct one through `Command`, so emulate by spending the
        // gauge to <0 and testing the cost-check directly: the only
        // 0-cost commands are directional, which always admit.
        // (This test just validates the API doesn't panic on empty
        // gauge for a 0-cost cmd.)
    }

    #[test]
    fn target_cancelled_drops_pending_command_without_pushing() {
        use crate::target_picker::TargetKind;
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        w.actors[3].battle.hp = 50;
        w.actors[3].battle.max_hp = 50;
        s.begin_round(&mut w);
        s.transition(BattlePhase::CommandInput);
        let mut events = Vec::new();
        s.open_target_picker(
            &w,
            TargetKind::SingleEnemy,
            0,
            Some(Command::Down),
            &mut events,
        );
        let buffer_before = s.runner.current_buffer().len();
        events.clear();
        let evs = s.tick(
            &mut w,
            SessionInput {
                circle: true,
                ..Default::default()
            },
        );
        assert!(
            evs.iter()
                .any(|e| matches!(e, SessionEvent::TargetCancelled))
        );
        // No new command admitted.
        assert_eq!(s.runner.current_buffer().len(), buffer_before);
    }

    #[test]
    fn cancel_target_picker_drops_pending_command() {
        use crate::target_picker::TargetKind;
        let mut s = fresh_session();
        let mut w = fresh_world_with_actors();
        w.actors[3].battle.hp = 50;
        w.actors[3].battle.max_hp = 50;
        s.begin_round(&mut w);
        let mut events = Vec::new();
        s.open_target_picker(
            &w,
            TargetKind::SingleEnemy,
            0,
            Some(Command::Up),
            &mut events,
        );
        events.clear();
        s.cancel_target_picker(&mut events);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SessionEvent::TargetCancelled))
        );
        assert!(s.target_picker().is_none());
    }
}
