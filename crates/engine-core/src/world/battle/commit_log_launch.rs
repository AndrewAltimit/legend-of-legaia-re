//! The commit log's **launch** - the log sliding off the left edge when the
//! member entering a command leaves the ring for a sub-screen, and back when
//! they return. The kernel is `legaia_engine_battle_vm::battle_commit_log::LogLaunch`
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

/// Placement record 7 - the active-actor readout bar.
const READOUT_BAR_ELEMENT: u8 = 7;

impl World {
    /// Note a HUD element raise for the two action plates that glide in:
    /// `FUN_801D8DE8(0x44 | 0x51, 0)` from the action seed's banner plan
    /// starts that plate's glide from seat A.
    pub(crate) fn note_action_plate_raise(&mut self, element: u8, mode: u8) {
        use legaia_engine_battle_vm::battle_commit_log::LogLaunch;
        use legaia_engine_battle_vm::battle_cue_group::{HUD_CASTER_BANNER, HUD_TARGET_BANNER};
        if mode & 1 != 0 {
            // The Done band's teardown. The actor plaque's unload
            // (`li a0,0x44` at `0x801E61FC`) is issued for every category
            // but Run and stands for the list wipe ahead of it
            // (`FUN_801D99BC`, `0x801E6170`): the target plaque is gone
            // unless the sweep that follows re-spawns it on its closing
            // glide (`FUN_801D8DE8(0x51, 1)`, `0x801E6344`).
            if element == HUD_CASTER_BANNER {
                self.battle.action_hud_wiped = true;
                self.battle.target_plaque_glide = None;
            } else if element == HUD_TARGET_BANNER && self.battle.action_hud_wiped {
                self.battle.target_plaque_glide = Some(LogLaunch::new(true));
            }
            return;
        }
        if element == HUD_CASTER_BANNER {
            // A new action's seed: whatever cleared the last target plate's
            // content is behind it.
            self.battle.action_plaque_glide = Some(LogLaunch::new(false));
            self.battle.target_plate_cleared = false;
            self.battle.action_hud_wiped = false;
            self.battle.counter_hud = None;
            // A module's panel raise belongs to the action that made it.
            self.battle.module_panels_glide = None;
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
        } else if element == legaia_engine_battle_vm::battle_action::SPIRIT_AP_BAR_ELEMENT {
            // The Spirit arm's raise of the AP bar and, on the same pass,
            // the AP plate (`0x801E2FD4` / `0x801E2FE0`): one glide.
            self.battle.spirit_gauge_glide = Some(LogLaunch::new(false));
        } else if element == HUD_TARGET_BANNER {
            self.battle.target_plaque_glide = Some(LogLaunch::new(false));
            self.battle.target_plate_cleared = false;
            self.battle.action_hud_wiped = false;
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
        use legaia_engine_battle_vm::battle_action::ActionCategory;
        let a = self.battle_ctx.active_actor;
        let Some(actor) = self.actors.get(usize::from(a)) else {
            return false;
        };
        let cat = actor.battle.action_category;
        (cat == ActionCategory::Attack.as_byte() || cat == ActionCategory::Magic.as_byte())
            && actor.battle.active_target < self.party.party_count
    }

    /// Seat the HUD widget glide whose retail record lands on `target` at
    /// `elapsed` - the record's `+0x04` / `+0x06` target seat names the
    /// widget: `(16, 12)` the actor plaque (record `0x44`), `(16, 192)` the
    /// readout bar (record 7), any other seat on the bar's row the target
    /// plaque (record `0x51`, `x = 304 - w`); the round prompt's chips land
    /// on `(104, 86)` / `(180, 86)` (records 0 / 3) and its panels on row
    /// `168` at any `x` but the combo cluster's `168` (records 6 / `0x4E` /
    /// `0x4F`, re-seated per party size); the ring's arms land on
    /// `(160 | 248, 64)` / `(204, 32 | 96)` (records 9 / `0x0A` / 8 /
    /// `0x0B`), its plaque on `(68, 12)` (record `0x1A`) and its AP plate on
    /// `(208, 172)` (record `0x52`). Returns whether a glide the
    /// engine holds took the seat. The retail-compare capture seats this
    /// from a capture whose glides were in flight; no game path calls it.
    pub fn seat_battle_hud_glide(&mut self, target: [i16; 2], elapsed: u8) -> bool {
        // A Spirit turn raises no readout bar and no ring: its AP bar and AP
        // plate are the records landing on the bar's seat and the ring
        // plate's.
        let spirit = self.spirit_gauge_view().is_some();
        let glide = match target {
            [16, 12] => self.battle.action_plaque_glide.as_mut(),
            [16, 192] | [208, 172] if spirit => Some(self.battle.spirit_gauge_glide.get_or_insert(
                legaia_engine_battle_vm::battle_commit_log::LogLaunch::new(false),
            )),
            // The message bar takes the readout bar's seat (record 91 over
            // record 7) while a caption is up.
            [16, 192] if self.battle.steal_caption.is_some() => {
                self.battle.message_bar_glide.as_mut()
            }
            [16, 192] => self.battle.readout_bar_glide.as_mut(),
            [_, 192] => self.battle.target_plaque_glide.as_mut(),
            [104 | 180, 86] => self.battle.round_prompt_glide.as_mut(),
            [160 | 248, 64] | [204, 32 | 96] | [68, 12] | [208, 172] => {
                self.battle.ring_open_glide.as_mut()
            }
            [x, 168] if x != 168 => self.battle.round_prompt_glide.as_mut(),
            _ => None,
        };
        let Some(g) = glide else {
            return false;
        };
        g.elapsed = elapsed.min(g.total);
        // What the stepper returned on the pass the capture froze.
        let ret = if g.settled() {
            0
        } else {
            g.total - g.elapsed + 1
        };
        self.battle.widget_glide_return = ret;
        true
    }

    /// A press is parked only while the stepper's return is under this
    /// (`sltiu v0,s0,0x6` at `0x801D0B3C` / `0x801D0B58`): the last passes
    /// of a glide and the pass it lands on.
    pub(crate) const GLIDE_PAD_BUFFER_BELOW: u8 = 6;

    /// The menu SM's pre-dispatch gate: whether this tick's pass leaves
    /// without running a command state.
    ///
    /// `FUN_801D0748` calls the glide stepper ahead of its state switch
    /// (`jal 0x801D9BBC` at `0x801D0B2C`) and, on a non-zero return, stores
    /// it in `ctx[+0x6B2]` and exits (`0x801D0B9C..0x801D0BA4`) - no state
    /// arm runs, so nothing reads the pad and the state's own draws (the
    /// D-pad glyph) are not issued. On the way out it parks the pad word:
    ///
    /// ```text
    /// if ret < 6 && s2 != 0 && ctx[+0x884] == 0 && _DAT_800846C8 == 0 {
    ///     ctx[+0x884] = s2                    ; 0x801D0B5C..0x801D0B98
    /// }
    /// ```
    ///
    /// `_DAT_800846C8` is the Battle Command option, so only the
    /// directional-buttons entry parks a press. The first pass with a zero
    /// return then takes one more exit when a stage module is paged in
    /// (`ctx[+0x6B2] != 0` and stage id `_DAT_8007B64A != 0`,
    /// `0x801D0BA8..0x801D0BCC`), clearing the latch.
    ///
    /// The parked word is consumed by [`Self::take_command_pad_buffer`].
    /// Not modelled: the same exit under `ctx[+0x26A] != 0`
    /// (`0x801D0B4C..0x801D0B54`), and the held-for-32-vsyncs term of `s2`
    /// (`_DAT_8007B938`) - the port's word is the newly-pressed edge alone.
    ///
    /// PORT: FUN_801D0748 (`0x801D0B2C..0x801D0BCC`)
    pub(in crate::world) fn battle_command_glide_gate(&mut self) -> bool {
        use legaia_engine_system::option_values::BattleCommandOpt;
        let ret = self.battle.widget_glide_return;
        if ret != 0 {
            let pressed = self.input.pad() & !self.input.pad_prev() != 0;
            if ret < Self::GLIDE_PAD_BUFFER_BELOW
                && pressed
                && self.battle.command_pad_buffer.is_none()
                && self.toggles.battle_command == BattleCommandOpt::DirectionalButtons
            {
                self.battle.command_pad_buffer = Some(self.battle_command_pad_edges());
            }
            self.battle.widget_glide_latch = ret;
            return true;
        }
        if self.battle.widget_glide_latch != 0 {
            self.battle.widget_glide_latch = 0;
            if self.battle.stage_id != 0 {
                return true;
            }
        }
        false
    }

    /// This tick's command pad edges.
    pub(in crate::world) fn battle_command_pad_edges(
        &self,
    ) -> legaia_engine_battle_menus::battle_input::BattleCommandInput {
        use legaia_engine_system::input::PadButton;
        legaia_engine_battle_menus::battle_input::BattleCommandInput {
            up: self.input.just_pressed(PadButton::Up),
            down: self.input.just_pressed(PadButton::Down),
            left: self.input.just_pressed(PadButton::Left),
            right: self.input.just_pressed(PadButton::Right),
            cross: self.input.just_pressed(PadButton::Cross),
            circle: self.input.just_pressed(PadButton::Circle),
            // The ring's Attack arm reads the option word with the pad.
            select_attack: self.toggles.select_attack,
        }
    }

    /// The pad word a dispatched pass reads: this tick's edges, or the
    /// parked press when the tick has none. The park is emptied either way
    /// (`bne s2,zero` at `0x801D0C1C`, `move s2,v0` at `0x801D0C3C`,
    /// `sw zero,0x884(v1)` at `0x801D0C58`).
    ///
    /// PORT: FUN_801D0748 (`0x801D0C1C..0x801D0C58`)
    pub(in crate::world) fn take_command_pad_buffer(
        &mut self,
    ) -> legaia_engine_battle_menus::battle_input::BattleCommandInput {
        let live = self.battle_command_pad_edges();
        let parked = self.battle.command_pad_buffer.take();
        let pressed = self.input.pad() & !self.input.pad_prev() != 0;
        match parked {
            Some(p) if !pressed => legaia_engine_battle_menus::battle_input::BattleCommandInput {
                select_attack: live.select_attack,
                ..p
            },
            _ => live,
        }
    }

    /// Raise the launch a command-session frame implies, from the phase it
    /// was on before the frame and the one it is on after.
    pub(in crate::world) fn launch_commit_log_for(
        &mut self,
        before: &legaia_engine_battle_menus::battle_input::CommandPhase,
        after: &legaia_engine_battle_menus::battle_input::CommandPhase,
    ) {
        use legaia_engine_battle_menus::battle_input::{BattleCommand, CommandPhase as P};
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
            self.battle.launch_commit_log(false);
        } else if returns_to_ring {
            self.battle.launch_commit_log(true);
        }
    }
}
