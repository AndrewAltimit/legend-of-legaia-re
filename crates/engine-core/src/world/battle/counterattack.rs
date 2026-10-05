//! The **Counterattack** passive (accessory passive index `0x0F`, the
//! Warrior Icon): a party member wearing it can answer a monster's strike
//! with its own committed attack before the monster swings.
//!
//! Two halves, both in the battle overlay (PROT 0898):
//!
//! * the turn picker `FUN_801DABA4` arms the latch `0x801F6970` straight after
//!   a monster's action pick and dead-target redirect
//!   (`0x801DAF74..0x801DB050`) - [`World::roll_counterattack`];
//! * the strike loop's head consumes it on the monster's first frame
//!   (`0x801E35F0..0x801E36E0`) - the action SM's half is
//!   `legaia_engine_vm::battle_action`'s counter swap, which calls back into
//!   [`World::begin_counterattack`] for the parts that are not actor or
//!   context bytes: the timed message, the counterer's strike queue and the
//!   cleared target plaque.
//!
//! The counter consumes the counterer's turn: the swap zeroes its initiative
//! key, and the port retires its committed command with the queue build.

use super::*;

/// The character-record ability bit the picker tests (`lw v0,0x6BC(v0)` /
/// `andi v0,v0,0x8000` at `0x801DB00C..0x801DB014` off `0x80084140 +
/// char * 0x414`, i.e. record `+0xF4`): accessory passive index `0x0F`,
/// Counterattack.
pub const COUNTERATTACK_BIT: u32 = 0x0000_8000;

/// The monster status bits that rule a counter out (`andi v0,v0,0x380` at
/// `0x801DAFBC` on the monster's `+0x16E`).
const COUNTER_BLOCKING_STATUS: u16 = 0x0380;

impl World {
    /// The turn picker's counter roll, run after a monster's physical pick
    /// and its dead-target redirect:
    ///
    /// ```text
    /// if monster[+0x1DE] == 3 && target < 3:            ; a strike on the party
    ///     if rand() & 1                                  ; BIOS A(2Fh)
    ///        && monster[+0x16E] & 0x380 == 0
    ///        && target[+0x16C] != 0                      ; target has not acted
    ///        && char[target][+0xF4] & 0x8000:            ; Counterattack
    ///         0x801F6970 = target + 1
    /// ```
    ///
    /// The draw is taken only when the first two tests pass, as in retail.
    ///
    /// PORT: FUN_801DABA4 (`0x801DAF74..0x801DB050`, the counter roll)
    pub(in crate::world) fn roll_counterattack(&mut self, monster: u8, target: u8) {
        let pc = self.party.party_count.clamp(1, 3);
        if target >= pc {
            return;
        }
        if self.next_rand() & 1 == 0 {
            return;
        }
        let blocked = self
            .actors
            .get(usize::from(monster))
            .is_none_or(|a| a.battle.field_flags & COUNTER_BLOCKING_STATUS != 0);
        if blocked {
            return;
        }
        if self
            .actors
            .get(usize::from(target))
            .is_none_or(|a| a.battle.init_key == 0)
        {
            return;
        }
        let bits = self
            .party
            .roster
            .members
            .get(self.party_roster_slot(usize::from(target)))
            .map(|r| r.ability_bits());
        let word0 = bits.map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        if word0.is_some_and(|w| w & COUNTERATTACK_BIT != 0) {
            self.battle_ctx.counter_pending = target + 1;
        }
    }

    /// Whether `slot` committed a physical attack this round: retail writes
    /// the category `+0x1DE = 3` at the command commit, the port keeps the
    /// command on [`crate::battle_round::RoundFlow::pending`] until its
    /// dispatch. A plain strike and an arts string are both category `3`.
    pub(in crate::world) fn committed_attack(&self, slot: u8) -> bool {
        use crate::battle_round::PendingPartyAction as P;
        matches!(
            self.battle.round_flow.pending.get(usize::from(slot)),
            Some(Some(P::Attack { .. } | P::Art { .. }))
        )
    }

    /// The engine half of the strike loop's counter swap: raise the
    /// counterattack line for its `0x78`-frame hold, build the counterer's
    /// queue from its committed command (`FUN_801EED1C`, through the same
    /// dispatch the member's own turn would have run - which retires the
    /// command), and clear the target plaque. `false` when the counterer has
    /// no committed attack left to build from.
    ///
    /// The dispatch also arms the context the way a turn's own dispatch
    /// does; the action SM's write-back overwrites that with its own swap.
    pub(in crate::world) fn begin_counterattack(&mut self, counterer: u8, _attacker: u8) -> bool {
        if !self.committed_attack(counterer) {
            return false;
        }
        let Some(action) = self
            .battle
            .round_flow
            .pending
            .get_mut(usize::from(counterer))
            .and_then(Option::take)
        else {
            return false;
        };
        self.raise_timed_message(COUNTER_MESSAGE_VA, COUNTER_MESSAGE_HOLD);
        self.pending_battle_events.push(BattleEvent::UiElement {
            effect_id: TIMED_MESSAGE_ELEMENT,
            mode: 0,
        });
        self.dispatch_pending_party_action(counterer, action);
        self.battle.target_plate_cleared = true;
        true
    }
}
