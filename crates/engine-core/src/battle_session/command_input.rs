//! CommandInput-phase input handling: direction/confirm/cancel/spirit/commit
//! dispatch, the per-slot inputability gate, and the direct command-push
//! entry points engines call from their menu-cursor bindings.

use super::*;

impl BattleSession {
    /// Per-CommandInput tick. Direction presses queue commands, Cross
    /// confirms (currently a no-op stub for the menu cursor model - engines
    /// wire their own art/spell pickers and call [`Self::push_command`] /
    /// [`Self::push_chained_art`]), Circle pops, Square charges Spirit,
    /// Triangle advances slots, Start commits.
    pub(super) fn tick_command_input(
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
            // Commit. Resolved queues stay on the runner; build the
            // resolve driver queue from the live world + buffered
            // commands so the next phase can drive the action SM.
            if self.runner.commit_turn().is_ok() {
                out.push(SessionEvent::TurnCommitted);
                self.install_resolve_queue(world);
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
    pub(super) fn is_slot_inputable(&self, world: &World, party_slot: u8) -> bool {
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
        // Rot refuses the rotted limb's attack command (limb roll 0 = Left
        // arm, 1 = Right arm, 2 = Low attack) - the engine's reading of the
        // retail `+0x16E` limb-disable bits (the retail consumer lives in the
        // undumped command-menu controller; see engine-vm::status_effects).
        if let Some(limb) = world.status_effects.rot_limb(active) {
            let blocked = [1u8, 2, 3][limb.min(2) as usize];
            if cmd.as_byte() == blocked {
                return false;
            }
        }
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
}
