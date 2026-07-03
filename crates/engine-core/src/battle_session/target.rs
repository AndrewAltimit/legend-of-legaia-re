//! Target-picker sub-phase: opening / cancelling / ticking the picker,
//! folding its outcome back into the runner queue + `active_target`, and the
//! combined `push_command_with_target` entry point.

use super::*;

impl BattleSession {
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
    pub(super) fn tick_target_picker(
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
