//! The ring's **cancel**: stepping the command cursor back a member.
//!
//! Retail's ring state `0x28` tests the cancel mask `*(0x800846D4)` first
//! (`0x801D11B4` in `FUN_801D0748`) and forks on the step counter
//! `ctx[+0x1F]` - how many members the cursor has walked past this round:
//!
//! * `0` - the round's first member: the command window's case `2` and
//!   `ctx[+0x06] = 0x1E`, back to `Begin | Run` (`0x801D11D8..0x801D11E4`);
//! * otherwise - the window's case `0x10` (`0x801D1278`), whose tail is
//!   `FUN_801D32BC(1)` (`0x801D4010`): the cursor scans down to the previous
//!   selectable member and the flow stays on the ring, now that member's. If
//!   the member it lands on had committed an item (`+0x1DE == 1`), the copy
//!   the commit consumed goes back to the bag - `FUN_800421D4(+0x1DF, 1)` at
//!   `0x801D12AC..0x801D12B0`.
//!
//! A counter of zero and a backward scan that finds no selectable member
//! below the cursor are the same condition: the forward walk that raised the
//! counter stops on every selectable member, so it has walked past exactly
//! the ones the backward scan can land on. The port asks the scan.

use super::*;

impl World {
    /// Step the command cursor back from `actor`'s ring - retail's `0x28`
    /// cancel arm.
    ///
    /// REF: FUN_801D0748 (`0x801D11B4..0x801D12B8`, the ring's cancel arm)
    /// REF: FUN_801D388C (case `0x10`, `0x801D4010`)
    pub(in crate::world) fn step_back_battle_command(&mut self, actor: u8) {
        use crate::battle_flow::BattleFlowState as Flow;
        use crate::battle_round::PendingPartyAction;
        use vm::battle_cursor_pose::{ActorCursor, CursorActor, CursorStep, step_actor_cursor};
        let party_count = self.party.party_count.clamp(1, 3);
        let seats: Vec<CursorActor> = (0..party_count)
            .map(|s| CursorActor {
                liveness: self
                    .actors
                    .get(usize::from(s))
                    .filter(|a| a.battle.liveness != 0)
                    .map_or(0, |a| a.battle.hp),
                status: self.raw_status_word(s),
            })
            .collect();
        let mut cursor = ActorCursor {
            active: actor,
            ..ActorCursor::default()
        };
        step_actor_cursor(&mut cursor, CursorStep::Backward, party_count, &seats);
        let prev = cursor.active;
        if prev >= party_count {
            // Counter zero: the first member's cancel reopens the prompt.
            self.set_battle_flow(Flow::TurnPrompt);
            self.open_battle_command(actor);
            return;
        }
        let slot = usize::from(prev);
        if let Some(PendingPartyAction::Item { item_id, .. }) = self
            .battle
            .round_flow
            .pending
            .get_mut(slot)
            .and_then(Option::take)
        {
            let _ = self.party.inventory.add(item_id, 1);
        } else if let Some(p) = self.battle.round_flow.pending.get_mut(slot) {
            *p = None;
        }
        // The Spirit stance is the committed category; an un-committed member
        // holds none until it commits again.
        if let Some(g) = self.battle.guarding.get_mut(slot) {
            *g = false;
        }
        self.battle.round_flow.cursor = prev;
        self.set_battle_flow(Flow::CategoryMenu);
        self.open_battle_command(prev);
    }
}
