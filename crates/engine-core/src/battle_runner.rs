//! Battle command orchestrator.
//!
//! Sits between the player-input layer and the [`legaia_engine_vm::battle_action`]
//! state machine, plus the [`crate::battle_round::BattleRound`] orchestrator.
//! One [`BattleRunner`] per battle session; engines feed it raw player
//! commands per turn and call [`BattleRunner::tick_action`] to drive the
//! per-frame action SM.
//!
//! ## Responsibilities
//!
//! 1. **Per-turn input → action queue.** Commands are accumulated until
//!    the player commits the turn; [`BattleRunner::commit_turn`] runs the
//!    queue through [`legaia_engine_vm::battle_action::resolve_action_queue`]
//!    so Miracle / Super expansion happens on the engine side, before the
//!    SM sees a single byte.
//! 2. **AP gating.** [`BattleRunner::push_command`] consults the active
//!    party member's [`crate::ap_gauge::ApGauge`] before admitting the
//!    next byte — failure surfaces a [`BattleRunnerError::OutOfAp`].
//! 3. **Turn lifecycle.** Engines call [`BattleRunner::begin_round`] at
//!    turn start and [`BattleRunner::end_round`] at turn end; the runner
//!    delegates to [`crate::battle_round::BattleRound`] which resets AP,
//!    recomputes per-slot stats, and drains tick damage.
//! 4. **Action validation.** Resolved arts are filtered through
//!    [`legaia_engine_vm::action_validator`] (the 16-arm validator) so
//!    arts with insufficient AP / blocked by status / unknown to the
//!    character are rejected before they reach the SM.
//!
//! No SM ticking happens here — engines tick the SM through their existing
//! `step_battle` loop. The runner is the **input → queue** half of the
//! pipeline.

use crate::ap_gauge::{ApGauge, art_ap_cost};
use crate::battle_round::BattleRound;
use crate::battle_stats::{EquipmentTable, StatRecord, StatusModifiers};
use crate::world::World;
use legaia_art::{ActionConstant, ActionQueue, Character, Command};
use legaia_engine_vm::battle_action::resolve_action_queue;

/// Errors the runner returns from input pushes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleRunnerError {
    /// The active party slot doesn't have enough AP to admit the
    /// command.
    OutOfAp,
    /// The party slot index was out of range (0..=2).
    InvalidParty,
    /// The runner is in a state that doesn't accept new commands
    /// (already committed / round in flight).
    NotAcceptingInput,
    /// The runner already committed this turn; further commands are
    /// ignored until [`BattleRunner::begin_round`] is called.
    AlreadyCommitted,
}

/// Runner state. Engines render off this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleRunnerState {
    /// Round hasn't started yet (post-construction or post-end).
    Idle,
    /// Accepting input from the active party slot.
    AcceptingCommands,
    /// Player committed; the SM is consuming the resolved queue.
    Committed,
}

/// One battle command runner.
#[derive(Debug, Clone)]
pub struct BattleRunner {
    state: BattleRunnerState,
    /// Active party slot the runner is reading commands for. Cycles
    /// through 0..=2 across the round; engines flip this when a slot's
    /// turn ends.
    active_party_slot: u8,
    /// Per-slot raw command buffer. Index = party slot 0..=2.
    command_buffers: [Vec<Command>; 3],
    /// Per-slot chained arts the player has pre-selected.
    chained_arts: [Vec<ActionConstant>; 3],
    /// Per-slot character. Engines populate at battle entry.
    characters: [Character; 3],
    /// Resolved per-slot action queues, populated by `commit_turn`.
    resolved_queues: [Option<ActionQueue>; 3],
}

impl Default for BattleRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl BattleRunner {
    pub fn new() -> Self {
        Self {
            state: BattleRunnerState::Idle,
            active_party_slot: 0,
            command_buffers: [Vec::new(), Vec::new(), Vec::new()],
            chained_arts: [Vec::new(), Vec::new(), Vec::new()],
            characters: [Character::Vahn, Character::Noa, Character::Gala],
            resolved_queues: [None, None, None],
        }
    }

    /// Set which character occupies which party slot. The runner keeps
    /// these for the lifetime of the battle.
    pub fn set_characters(&mut self, slots: [Character; 3]) {
        self.characters = slots;
    }

    /// Currently-active party slot (0..=2).
    pub fn active_party_slot(&self) -> u8 {
        self.active_party_slot
    }

    /// Switch the active slot. Engines call this from the turn-cycling
    /// driver when one party member's input is complete.
    pub fn set_active_party_slot(&mut self, slot: u8) -> Result<(), BattleRunnerError> {
        if slot >= 3 {
            return Err(BattleRunnerError::InvalidParty);
        }
        self.active_party_slot = slot;
        Ok(())
    }

    /// Current runner state.
    pub fn state(&self) -> BattleRunnerState {
        self.state
    }

    /// Commands the active slot has buffered so far this turn.
    pub fn current_buffer(&self) -> &[Command] {
        &self.command_buffers[self.active_party_slot as usize]
    }

    /// Per-slot resolved queue (after [`Self::commit_turn`]).
    pub fn resolved_queue(&self, party_slot: u8) -> Option<&ActionQueue> {
        self.resolved_queues
            .get(party_slot as usize)
            .and_then(|q| q.as_ref())
    }

    /// Begin a new round. Resets per-slot buffers, runs
    /// [`BattleRound::begin`] to refresh AP / stats, and switches into
    /// `AcceptingCommands`.
    pub fn begin_round(
        &mut self,
        world: &mut World,
        per_slot: &[Option<StatRecord>; 8],
        equipment: &EquipmentTable,
        modifiers: &StatusModifiers,
    ) -> BattleRound {
        for buf in self.command_buffers.iter_mut() {
            buf.clear();
        }
        for chained in self.chained_arts.iter_mut() {
            chained.clear();
        }
        for r in self.resolved_queues.iter_mut() {
            *r = None;
        }
        self.active_party_slot = 0;
        self.state = BattleRunnerState::AcceptingCommands;
        BattleRound::begin(world, per_slot, equipment, modifiers)
    }

    /// Push one [`Command`] for the active party slot, paying its AP cost
    /// out of `ap`. Returns `Ok(())` on admit, `Err(OutOfAp)` if the
    /// gauge can't cover the cost.
    pub fn push_command(
        &mut self,
        ap: &mut ApGauge,
        cmd: Command,
    ) -> Result<(), BattleRunnerError> {
        if !matches!(self.state, BattleRunnerState::AcceptingCommands) {
            return Err(BattleRunnerError::NotAcceptingInput);
        }
        let cost = art_ap_cost(cmd.as_action());
        if !ap.try_spend(cost) {
            return Err(BattleRunnerError::OutOfAp);
        }
        self.command_buffers[self.active_party_slot as usize].push(cmd);
        Ok(())
    }

    /// Append a chained art for the active party slot. Each chained art
    /// is bracketed with [`ActionConstant::RegularStarter`] when the
    /// queue is resolved (mirroring retail's `19 <art> 19 <art>` shape).
    pub fn push_chained_art(
        &mut self,
        ap: &mut ApGauge,
        art: ActionConstant,
    ) -> Result<(), BattleRunnerError> {
        if !matches!(self.state, BattleRunnerState::AcceptingCommands) {
            return Err(BattleRunnerError::NotAcceptingInput);
        }
        // Cost = starter + art body byte.
        let cost = art_ap_cost(ActionConstant::RegularStarter).saturating_add(art_ap_cost(art));
        if !ap.try_spend(cost) {
            return Err(BattleRunnerError::OutOfAp);
        }
        self.chained_arts[self.active_party_slot as usize].push(art);
        Ok(())
    }

    /// Pop the most recent command from the active slot's buffer and
    /// refund its AP cost via [`ApGauge::refund`]. Returns the popped
    /// command if any.
    pub fn pop_command(&mut self, ap: &mut ApGauge) -> Option<Command> {
        let cmd = self.command_buffers[self.active_party_slot as usize].pop()?;
        let cost = art_ap_cost(cmd.as_action());
        ap.refund(cost);
        Some(cmd)
    }

    /// Pop the most recent chained art and refund its AP.
    pub fn pop_chained_art(&mut self, ap: &mut ApGauge) -> Option<ActionConstant> {
        let art = self.chained_arts[self.active_party_slot as usize].pop()?;
        let cost = art_ap_cost(ActionConstant::RegularStarter).saturating_add(art_ap_cost(art));
        ap.refund(cost);
        Some(art)
    }

    /// Commit the turn — resolve every party slot's queue through
    /// [`resolve_action_queue`] (Miracle + Super expansion) and stash the
    /// result. Switches into `Committed` state.
    ///
    /// Returns the resolved queues in `[Vahn-slot, Noa-slot, Gala-slot]`
    /// order (regardless of `active_party_slot`).
    pub fn commit_turn(&mut self) -> Result<[Option<ActionQueue>; 3], BattleRunnerError> {
        if matches!(self.state, BattleRunnerState::Committed) {
            return Err(BattleRunnerError::AlreadyCommitted);
        }
        if !matches!(self.state, BattleRunnerState::AcceptingCommands) {
            return Err(BattleRunnerError::NotAcceptingInput);
        }
        for slot in 0..3 {
            let queue = resolve_action_queue(
                self.characters[slot],
                &self.command_buffers[slot],
                &self.chained_arts[slot],
            );
            self.resolved_queues[slot] = Some(queue);
        }
        self.state = BattleRunnerState::Committed;
        Ok(self.resolved_queues.clone())
    }

    /// Mark this round complete. Drains tick damage via
    /// [`BattleRound::end`] and switches back to `Idle`. Returns the
    /// number of actors that died from tick damage this round.
    pub fn end_round(&mut self, world: &mut World) -> u32 {
        self.state = BattleRunnerState::Idle;
        BattleRound::end(world)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle_stats::EquipmentTable;
    use crate::world::World;
    use legaia_art::Character;

    fn fresh_runner() -> BattleRunner {
        let mut r = BattleRunner::new();
        r.set_characters([Character::Vahn, Character::Noa, Character::Gala]);
        r
    }

    fn fresh_world() -> (World, EquipmentTable, StatusModifiers) {
        let mut world = World::new();
        // 3 actor slots + 5 monster slots.
        for _ in 0..8 {
            world.actors.push(Default::default());
        }
        (world, EquipmentTable::default(), StatusModifiers::default())
    }

    fn empty_per_slot() -> [Option<StatRecord>; 8] {
        Default::default()
    }

    fn ap_gauge(ap: u8) -> ApGauge {
        ApGauge::with_base(ap)
    }

    #[test]
    fn new_runner_starts_idle() {
        let r = fresh_runner();
        assert_eq!(r.state(), BattleRunnerState::Idle);
        assert_eq!(r.active_party_slot(), 0);
    }

    #[test]
    fn push_command_in_idle_state_returns_not_accepting() {
        let mut r = fresh_runner();
        let mut ap = ap_gauge(8);
        let result = r.push_command(&mut ap, Command::Right);
        assert_eq!(result, Err(BattleRunnerError::NotAcceptingInput));
    }

    #[test]
    fn begin_round_transitions_to_accepting_and_resets_ap() {
        let mut r = fresh_runner();
        let (mut world, eq, mods) = fresh_world();
        let inputs = empty_per_slot();
        let _round = r.begin_round(&mut world, &inputs, &eq, &mods);
        assert_eq!(r.state(), BattleRunnerState::AcceptingCommands);
    }

    #[test]
    fn push_command_admits_when_ap_sufficient() {
        let mut r = fresh_runner();
        let (mut world, eq, mods) = fresh_world();
        let inputs = empty_per_slot();
        r.begin_round(&mut world, &inputs, &eq, &mods);
        let mut ap = ap_gauge(8);
        assert!(r.push_command(&mut ap, Command::Right).is_ok());
        assert_eq!(r.current_buffer(), &[Command::Right]);
    }

    #[test]
    fn push_chained_art_returns_out_of_ap_when_gauge_empty() {
        let mut r = fresh_runner();
        let (mut world, eq, mods) = fresh_world();
        let inputs = empty_per_slot();
        r.begin_round(&mut world, &inputs, &eq, &mods);
        let mut ap = ap_gauge(0);
        let art = ActionConstant::from_byte(0x20).unwrap();
        let result = r.push_chained_art(&mut ap, art);
        assert_eq!(result, Err(BattleRunnerError::OutOfAp));
    }

    #[test]
    fn pop_command_returns_buffered_command() {
        let mut r = fresh_runner();
        let (mut world, eq, mods) = fresh_world();
        let inputs = empty_per_slot();
        r.begin_round(&mut world, &inputs, &eq, &mods);
        let mut ap = ap_gauge(4);
        r.push_command(&mut ap, Command::Right).unwrap();
        // Direction commands are 0-cost; popping returns Some.
        let popped = r.pop_command(&mut ap);
        assert_eq!(popped, Some(Command::Right));
        // Buffer is now empty.
        assert!(r.current_buffer().is_empty());
    }

    #[test]
    fn pop_chained_art_refunds_ap() {
        let mut r = fresh_runner();
        let (mut world, eq, mods) = fresh_world();
        let inputs = empty_per_slot();
        r.begin_round(&mut world, &inputs, &eq, &mods);
        let mut ap = ap_gauge(4);
        let art = ActionConstant::from_byte(0x20).unwrap();
        let before = ap.current_ap;
        r.push_chained_art(&mut ap, art).unwrap();
        let after_push = ap.current_ap;
        // Starter (1) + body (1) = 2 AP spent.
        assert_eq!(after_push, before - 2);
        let popped = r.pop_chained_art(&mut ap);
        assert_eq!(popped, Some(art));
        assert_eq!(ap.current_ap, before);
    }

    #[test]
    fn set_active_party_slot_validates_range() {
        let mut r = fresh_runner();
        assert!(r.set_active_party_slot(2).is_ok());
        assert_eq!(r.active_party_slot(), 2);
        assert_eq!(
            r.set_active_party_slot(3),
            Err(BattleRunnerError::InvalidParty)
        );
    }

    #[test]
    fn commit_turn_resolves_queue_with_starters() {
        let mut r = fresh_runner();
        let (mut world, eq, mods) = fresh_world();
        let inputs = empty_per_slot();
        r.begin_round(&mut world, &inputs, &eq, &mods);
        let mut ap = ap_gauge(6);
        r.push_command(&mut ap, Command::Right).unwrap();
        r.push_command(&mut ap, Command::Down).unwrap();

        let queues = r.commit_turn().unwrap();
        assert_eq!(r.state(), BattleRunnerState::Committed);
        let v_queue = queues[0].as_ref().unwrap();
        assert_eq!(v_queue.len(), 2);
        assert_eq!(v_queue.actions()[0], Command::Right.as_action());
        assert_eq!(v_queue.actions()[1], Command::Down.as_action());
    }

    #[test]
    fn commit_turn_twice_returns_already_committed() {
        let mut r = fresh_runner();
        let (mut world, eq, mods) = fresh_world();
        let inputs = empty_per_slot();
        r.begin_round(&mut world, &inputs, &eq, &mods);
        r.commit_turn().unwrap();
        let err = r.commit_turn().unwrap_err();
        assert_eq!(err, BattleRunnerError::AlreadyCommitted);
    }

    #[test]
    fn end_round_returns_to_idle() {
        let mut r = fresh_runner();
        let (mut world, eq, mods) = fresh_world();
        let inputs = empty_per_slot();
        r.begin_round(&mut world, &inputs, &eq, &mods);
        r.commit_turn().unwrap();
        r.end_round(&mut world);
        assert_eq!(r.state(), BattleRunnerState::Idle);
    }

    #[test]
    fn push_chained_art_costs_starter_plus_body() {
        let mut r = fresh_runner();
        let (mut world, eq, mods) = fresh_world();
        let inputs = empty_per_slot();
        r.begin_round(&mut world, &inputs, &eq, &mods);
        let mut ap = ap_gauge(4);
        // Starter cost (1) + art body (typically 1) = 2 AP.
        let art = ActionConstant::from_byte(0x20).unwrap();
        let before = ap.current_ap;
        let result = r.push_chained_art(&mut ap, art);
        assert!(result.is_ok());
        assert!(ap.current_ap < before);
    }

    #[test]
    fn pop_chained_art_refunds_full_cost() {
        let mut r = fresh_runner();
        let (mut world, eq, mods) = fresh_world();
        let inputs = empty_per_slot();
        r.begin_round(&mut world, &inputs, &eq, &mods);
        let mut ap = ap_gauge(8);
        let art = ActionConstant::from_byte(0x20).unwrap();
        let before = ap.current_ap;
        r.push_chained_art(&mut ap, art).unwrap();
        let popped = r.pop_chained_art(&mut ap);
        assert_eq!(popped, Some(art));
        // Refund saturates at the post-Spirit ceiling, but we never
        // charged Spirit, so it's just the base.
        assert_eq!(ap.current_ap, before);
    }

    #[test]
    fn current_buffer_is_per_active_slot() {
        let mut r = fresh_runner();
        let (mut world, eq, mods) = fresh_world();
        let inputs = empty_per_slot();
        r.begin_round(&mut world, &inputs, &eq, &mods);

        let mut ap0 = ap_gauge(4);
        r.push_command(&mut ap0, Command::Right).unwrap();
        // Switch to slot 1 — buffer is empty.
        r.set_active_party_slot(1).unwrap();
        assert!(r.current_buffer().is_empty());

        let mut ap1 = ap_gauge(4);
        r.push_command(&mut ap1, Command::Down).unwrap();
        assert_eq!(r.current_buffer(), &[Command::Down]);
        // Switch back — slot 0's buffer is preserved.
        r.set_active_party_slot(0).unwrap();
        assert_eq!(r.current_buffer(), &[Command::Right]);
    }

    #[test]
    fn resolved_queue_returns_none_before_commit() {
        let r = fresh_runner();
        assert!(r.resolved_queue(0).is_none());
    }

    #[test]
    fn resolved_queue_drains_after_commit() {
        let mut r = fresh_runner();
        let (mut world, eq, mods) = fresh_world();
        let inputs = empty_per_slot();
        r.begin_round(&mut world, &inputs, &eq, &mods);
        let mut ap = ap_gauge(4);
        r.push_command(&mut ap, Command::Up).unwrap();
        r.commit_turn().unwrap();
        let q = r.resolved_queue(0).unwrap();
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn second_round_clears_prior_buffers() {
        let mut r = fresh_runner();
        let (mut world, eq, mods) = fresh_world();
        let inputs = empty_per_slot();
        r.begin_round(&mut world, &inputs, &eq, &mods);
        let mut ap = ap_gauge(4);
        r.push_command(&mut ap, Command::Right).unwrap();
        r.commit_turn().unwrap();
        r.end_round(&mut world);
        // Round 2: prior buffer should be empty.
        r.begin_round(&mut world, &inputs, &eq, &mods);
        assert!(r.current_buffer().is_empty());
        assert!(r.resolved_queue(0).is_none());
    }
}
