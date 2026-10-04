//! The commit log's **launch** - the log sliding off the left edge when the
//! member entering a command leaves the ring for a sub-screen, and back when
//! they return. The kernel is `legaia_engine_vm::battle_commit_log::LogLaunch`
//! (the step table, the mode per step and the glide it runs); this module is
//! where the engine's command flow raises it.
//!
//! The engine's command surfaces are not retail's flow steps one for one, so
//! the raise sites are the engine transitions that stand for them:
//!
//! * ring -> the `Auto` / `Command` prompt (retail step `0x2A`), ring ->
//!   target cursor under the `Automatic` option (`0x30`), ring -> arts entry
//!   under the `Command` option (`0x09`), ring -> item window (`0x05`), ring
//!   -> magic window (`0x07`): **outbound**;
//! * the prompt or the attack target cursor cancelled back to the ring
//!   (`0x2B`, `0x31`), and a sub-screen backed out of onto the ring (`0x08`
//!   for the magic window; the item window and arts entry take the same
//!   return here by inference - their own cancel steps are not in the
//!   command SM's call list): **inbound**.
//!
//! A commit clears the launch: the next member's ring opens with the log at
//! rest, re-landed by the commit arms. Retail's Begin confirm is not a launch
//! (its tail-table entry is the plain exit), and the log leaves with the
//! round as before.

use super::*;

impl World {
    /// Raise a launch in `inbound`'s direction.
    pub(in crate::world) fn launch_commit_log(&mut self, inbound: bool) {
        self.battle.commit_log_launch =
            Some(legaia_engine_vm::battle_commit_log::LogLaunch::new(inbound));
    }

    /// One frame of the launch glide, on the world's frame step
    /// (`0x1F800393`). A settled inbound launch retires - the log is back
    /// at rest; a settled outbound one stays, which keeps the log hidden
    /// until the member returns to the ring or commits.
    pub(in crate::world) fn step_commit_log_launch(&mut self) {
        let step = self.clock.frame_step.max(1);
        if let Some(l) = self.battle.commit_log_launch.as_mut() {
            l.step(step);
            if l.inbound && l.settled() {
                self.battle.commit_log_launch = None;
            }
        }
    }

    /// Note a HUD element raise for the two action plates that glide in:
    /// `FUN_801D8DE8(0x44 | 0x51, 0)` from the action seed's banner plan
    /// starts that plate's glide from seat A.
    pub(in crate::world) fn note_action_plate_raise(&mut self, element: u8, mode: u8) {
        use legaia_engine_vm::battle_commit_log::LogLaunch;
        use legaia_engine_vm::battle_cue_group::{HUD_CASTER_BANNER, HUD_TARGET_BANNER};
        if mode & 1 != 0 {
            return;
        }
        if element == HUD_CASTER_BANNER {
            // A new action's seed: whatever cleared the last target plate's
            // content is behind it.
            self.battle.action_plaque_glide = Some(LogLaunch::new(false));
            self.battle.target_plate_cleared = false;
        } else if element == HUD_TARGET_BANNER {
            self.battle.target_plaque_glide = Some(LogLaunch::new(false));
            self.battle.target_plate_cleared = false;
        }
    }

    /// One frame of the two action-plate glides, on the frame step the
    /// commit log's launch steps by (`FUN_801D9BBC` walks every tracked
    /// widget in one pass).
    pub(in crate::world) fn step_action_plate_glides(&mut self) {
        let step = self.clock.frame_step.max(1);
        for g in [
            self.battle.action_plaque_glide.as_mut(),
            self.battle.target_plaque_glide.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            g.step(step);
        }
    }

    /// Raise the launch a command-session frame implies, from the phase it
    /// was on before the frame and the one it is on after.
    pub(in crate::world) fn launch_commit_log_for(
        &mut self,
        before: &crate::battle_input::CommandPhase,
        after: &crate::battle_input::CommandPhase,
    ) {
        use crate::battle_input::{BattleCommand, CommandPhase as P};
        let leaves_ring = matches!(before, P::Menu { .. })
            && matches!(
                after,
                P::AttackMode { .. }
                    | P::Targeting {
                        command: BattleCommand::Attack,
                        ..
                    }
                    | P::OpenArtsMenu
                    | P::OpenItemMenu
                    | P::OpenSpellMenu
            );
        let returns_to_ring = matches!(
            before,
            P::AttackMode { .. }
                | P::Targeting {
                    command: BattleCommand::Attack,
                    ..
                }
        ) && matches!(after, P::Menu { .. });
        if leaves_ring {
            self.launch_commit_log(false);
        } else if returns_to_ring {
            self.launch_commit_log(true);
        }
    }
}
