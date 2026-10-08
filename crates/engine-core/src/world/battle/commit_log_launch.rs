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

/// What one engine tick adds to a battle-pass counter that retail advances
/// by the frame step: a tracked widget glide's `elapsed` byte, the intro
/// names' hold, the timed message's hold.
///
/// `FUN_801D9BBC` adds the frame step `*(0x1F800393)` per battle pass
/// (`lbu v1,0x393(v1)` / `addu v0,a0,v1` at `0x801D9C18..0x801D9C28`), and a
/// pass spans that many vsyncs, so a glide's `total` counts vsyncs: the
/// sixteen-frame raise lasts sixteen vsyncs at any cadence. The holds drain
/// the same way (`ctx[+0x6D6] -= 0x1F800393`, `0x801D0E3C..0x801D0E58`;
/// the `0x801F6964` hold in `FUN_80046A20`). The world ticks once per vsync
/// (the action SM's own timers drain by `1` a tick), so the per-tick step is
/// `1`. Stepping by the frame step on every tick ran each of them at twice
/// retail's speed under the battle's step of `2`
/// (`nivora_duel_mid_blazing_slash`: retail's plaque and bar are ten of
/// sixteen into their raise, the engine's had landed).
pub(in crate::world) const BATTLE_PASS_STEP_PER_TICK: u8 = 1;

/// Placement record 7 - the active-actor readout bar.
const READOUT_BAR_ELEMENT: u8 = 7;

impl World {
    /// Raise a launch in `inbound`'s direction.
    pub(in crate::world) fn launch_commit_log(&mut self, inbound: bool) {
        self.battle.commit_log_launch =
            Some(legaia_engine_vm::battle_commit_log::LogLaunch::new(inbound));
    }

    /// One tick of the launch glide ([`BATTLE_PASS_STEP_PER_TICK`]). A
    /// settled inbound launch retires - the log is back at rest; a settled
    /// outbound one stays, which keeps the log hidden until the member
    /// returns to the ring or commits.
    pub(in crate::world) fn step_commit_log_launch(&mut self) {
        let step = BATTLE_PASS_STEP_PER_TICK;
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
            self.battle.counter_hud = None;
            // The seed's own record-7 open (`0x801E2F24..0x801E2F44`): the
            // Attack arm and the Magic arm past its item-class branch fall
            // into `sltiu v0,t2,3` on the target and raise the bar for a
            // party target with mode `0`. The engine's seed reports only the
            // banner plan, so the raise is taken here, on the same edge.
            self.battle.readout_bar_glide = self
                .seed_raises_readout_bar()
                .then(|| LogLaunch::new(false));
        } else if element == READOUT_BAR_ELEMENT {
            self.battle.readout_bar_glide = Some(LogLaunch::new(false));
        } else if element == HUD_TARGET_BANNER {
            self.battle.target_plaque_glide = Some(LogLaunch::new(false));
            self.battle.target_plate_cleared = false;
        }
    }

    /// Whether the action seed opens record 7 for its target: an Attack or a
    /// Magic action aimed at a party seat. The Magic arm's item-class branch
    /// (spell class `< 0x14` with id `< 0x65`, `0x801E2EEC..0x801E2EF8`)
    /// leaves for `0x3C` instead, whose own open (the acting member's,
    /// [`READOUT_BAR_ELEMENT`] through the host) restarts the glide; this
    /// test does not tell the branch apart, so such a cast starts the glide
    /// at the seed and again at `0x3C`.
    fn seed_raises_readout_bar(&self) -> bool {
        use legaia_engine_vm::battle_action::ActionCategory;
        let a = self.battle_ctx.active_actor;
        let Some(actor) = self.actors.get(usize::from(a)) else {
            return false;
        };
        let cat = actor.battle.action_category;
        (cat == ActionCategory::Attack.as_byte() || cat == ActionCategory::Magic.as_byte())
            && actor.battle.active_target < self.party.party_count
    }

    /// Land every HUD widget glide in flight on its rest seat, as
    /// `FUN_801D9BBC` does once a glide's `elapsed` reaches its `total`. The
    /// retail-compare capture seats this when the retail state's glides had
    /// all landed; no game path calls it.
    pub fn land_battle_hud_glides(&mut self) {
        for g in [
            self.battle.action_plaque_glide.as_mut(),
            self.battle.target_plaque_glide.as_mut(),
            self.battle.readout_bar_glide.as_mut(),
            self.battle.result_windows_glide.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            g.elapsed = g.total;
        }
    }

    /// Seat the HUD widget glide whose retail record lands on `target` at
    /// `elapsed` - the record's `+0x04` / `+0x06` target seat names the
    /// widget: `(16, 12)` the actor plaque (record `0x44`), `(16, 192)` the
    /// readout bar (record 7), any other seat on the bar's row the target
    /// plaque (record `0x51`, `x = 304 - w`). Returns whether a glide the
    /// engine holds took the seat. The retail-compare capture seats this
    /// from a capture whose glides were in flight; no game path calls it.
    pub fn seat_battle_hud_glide(&mut self, target: [i16; 2], elapsed: u8) -> bool {
        let glide = match target {
            [16, 12] => self.battle.action_plaque_glide.as_mut(),
            [16, 192] => self.battle.readout_bar_glide.as_mut(),
            [_, 192] => self.battle.target_plaque_glide.as_mut(),
            _ => None,
        };
        match glide {
            Some(g) => {
                g.elapsed = elapsed.min(g.total);
                true
            }
            None => false,
        }
    }

    /// One tick of the two action-plate glides, on the step the commit log's
    /// launch takes ([`BATTLE_PASS_STEP_PER_TICK`]; `FUN_801D9BBC` walks
    /// every tracked widget in one pass).
    pub(in crate::world) fn step_action_plate_glides(&mut self) {
        let step = BATTLE_PASS_STEP_PER_TICK;
        for g in [
            self.battle.action_plaque_glide.as_mut(),
            self.battle.target_plaque_glide.as_mut(),
            self.battle.readout_bar_glide.as_mut(),
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
