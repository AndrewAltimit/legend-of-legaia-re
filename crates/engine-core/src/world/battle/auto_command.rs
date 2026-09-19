//! The battle **auto command string**: the per-character 16-byte action
//! queue retail parks in the save record between turns, and replays when the
//! player attacks without entering arrows.
//!
//! Two retail leaves own it and this module is their live call site:
//!
//! - `FUN_801DA34C` reads one of the record's two bands into battle-actor
//!   `+0x1DF..+0x1EF`. Its retail call sites are both in the command SM
//!   `FUN_801D0748` - the Attack confirm (`0x801D15C8`, in the delay slot of
//!   the `sb 3, +0x1DE` that stamps the action category) and the arts-input
//!   entry (`0x801D1734`, beside the `sb 0x50` phase store).
//! - `FUN_801DA59C` writes the actor's live `+0x1DF..+0x1EF` window back into
//!   the band the same predicate picks (`0x801D22BC`, on the input confirm).
//!
//! The band predicate is the actor's action gauge, not a slot index:
//! `sltu(+0x156, +0x154)` - base gauge strictly below live gauge takes
//! [`legaia_save::AutoCommandBand::Primary`], everything else the secondary
//! band. Reading it the other way round makes a replay pick the string the
//! current turn cannot pay for.
//!
//! ### Where the port puts each call
//!
//! The read is at **dispatch**, not at the confirm press, for the same reason
//! [`crate::world::World::seed_basic_attack_queue`] is: the port arms a party
//! member's action stream when the initiative pick lands on it, where retail
//! arms it at the confirm and lets the SM carry it. The string that lands in
//! `+0x1DF` is identical either way - nothing between the two moments writes
//! that window - and the engine's own no-input swing roll is the fallback for
//! a character that has never confirmed one, which is the zero-fill leg of
//! the retail reader.
//!
//! The write is on the **arts commit**, which is where retail's is: the queue
//! `FUN_801DA59C` copies out is the one the input gauge just built, and its
//! two guards (`+0x14C != 0`, `+0x1DE == 3`) are the liveness and category the
//! commit has by construction.
//!
//! Neither call reaches [`crate::world::PartyState::saved_chains`]. That list
//! is the engine's **named** chain library (LGSF v2, edited by
//! `tactical_arts_editor`); this is retail's unnamed last-confirmed string,
//! and the two are different data with different lifetimes - the library
//! survives a save file, the auto string is overwritten by every arts commit.

use super::*;
use legaia_save::{AUTO_COMMAND_STRING_LEN, AutoCommandBand};

impl World {
    /// The [`AutoCommandBand`] an actor's live action gauge selects -
    /// retail's `sltu v0, u16[+0x156], u16[+0x154]` at `0x801DA3A4`
    /// (read leg) and `0x801DA5E0` (write leg).
    ///
    /// An actor slot that does not exist reads as the secondary band, which
    /// is the answer a zeroed gauge pair gives.
    ///
    /// REF: FUN_801DA34C (`0x801DA398..0x801DA3A8`)
    pub(in crate::world) fn auto_command_band(&self, actor: u8) -> AutoCommandBand {
        match self.actors.get(usize::from(actor)) {
            Some(a) => AutoCommandBand::for_gauge(a.battle.agl, a.battle.agl_base),
            None => AutoCommandBand::Secondary,
        }
    }

    /// Load `actor`'s auto command string into its `+0x1DF` action-parameter
    /// window and return how many leading non-terminator bytes it carries.
    ///
    /// `0` means the reader zero-filled - either the character has no record,
    /// or both bands' head bytes are zero - and the caller falls back to the
    /// engine's own swing roll. The window is written in every case, which is
    /// what retail's zero-fill loop at `0x801DA558` does.
    ///
    /// PORT: FUN_801DA34C
    pub(in crate::world) fn preseed_auto_command_string(&mut self, actor: u8) -> usize {
        let band = self.auto_command_band(actor);
        let roster_slot = self.party_roster_slot(usize::from(actor));
        let (first, second) = match self.party.roster.members.get(roster_slot) {
            Some(rec) => (
                rec.auto_command_string(AutoCommandBand::Primary),
                rec.auto_command_string(AutoCommandBand::Secondary),
            ),
            None => (
                [0u8; AUTO_COMMAND_STRING_LEN],
                [0u8; AUTO_COMMAND_STRING_LEN],
            ),
        };
        let mut queue = [0u8; vm::battle_action::ACTION_QUEUE_CAP];
        vm::battle_action::preseed_action_queue(
            &mut queue,
            // Retail's staging gate `DAT_8007BD04`: the port has no separate
            // "a command string is staged" latch, and the condition it stands
            // for - a party member about to act under player control - is the
            // only situation this is called in.
            true,
            band == AutoCommandBand::Secondary,
            &first,
            &second,
        );
        let Some(a) = self.actors.get_mut(usize::from(actor)) else {
            return 0;
        };
        let n = queue.len().min(a.battle.params.len());
        a.battle.params[..n].copy_from_slice(&queue[..n]);
        a.battle.strike_index = 0;
        queue[..n].iter().take_while(|&&b| b != 0).count()
    }

    /// Write `actor`'s live `+0x1DF` window back into its character record's
    /// gauge-selected band, so the next Attack replays it.
    ///
    /// Returns `false` when retail's two guards refuse the write: a dead
    /// actor (`+0x14C == 0`) or one whose action category is not the
    /// attack/arts band (`+0x1DE != 3`).
    ///
    /// PORT: FUN_801DA59C
    pub(in crate::world) fn save_auto_command_string(&mut self, actor: u8) -> bool {
        let band = self.auto_command_band(actor);
        let roster_slot = self.party_roster_slot(usize::from(actor));
        let Some(a) = self.actors.get(usize::from(actor)) else {
            return false;
        };
        let alive = a.battle.liveness != 0;
        let category_is_arts = a.battle.action_category == 3;
        let mut queue = [0u8; AUTO_COMMAND_STRING_LEN];
        let n = queue.len().min(a.battle.params.len());
        queue[..n].copy_from_slice(&a.battle.params[..n]);
        let Some(rec) = self.party.roster.members.get_mut(roster_slot) else {
            return false;
        };
        let mut first = rec.auto_command_string(AutoCommandBand::Primary);
        let mut second = rec.auto_command_string(AutoCommandBand::Secondary);
        let wrote = vm::battle_action::save_action_queue(
            &queue,
            alive,
            category_is_arts,
            band == AutoCommandBand::Secondary,
            &mut first,
            &mut second,
        );
        if wrote {
            rec.set_auto_command_string(AutoCommandBand::Primary, first);
            rec.set_auto_command_string(AutoCommandBand::Secondary, second);
        }
        wrote
    }
}
