//! The whole per-frame cabinet step every host runs, on the packed retail edge word.

use super::*;

/// What one [`SlotMachine::frame`] did that the host owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotFrameOutcome {
    /// An ordinary frame.
    Stepped,
    /// The cash-out flow committed this frame (state `100`'s tail): the host
    /// banks the balance and leaves the machine.
    CashedOut,
    /// The machine had already committed its cash-out before this frame:
    /// the host restores whatever the cabinet interrupted.
    Committed,
}

impl SlotMachine {
    /// One frame of the cabinet on this frame's **packed retail edge word**
    /// ([`packed_edges`]) - the whole per-frame step every host runs, so the
    /// play hosts' world tick and the standalone minigames page cannot answer
    /// the same press differently.
    ///
    /// Order is retail's: the reels tick, then the cash-out flow (submenu,
    /// rules pages, the not-enough-coins prompt, the leave fade) takes the
    /// edge when it owns the frame, otherwise any face-button edge latches the
    /// spin-up rarity and the phase reads its own button - Cross spins at
    /// idle, Square / Cross / Circle stop reels 0 / 1 / 2 while they are
    /// stoppable, and Cross on a resolved spin finishes its timed tally at
    /// once ([`Self::collect`]); left alone, state 4 counts the win in by
    /// itself and drops back to idle.
    // PORT: FUN_801cf0d8 (the per-frame state dispatch: state 1 spin, state 3
    //       stops, state 4 collect)
    pub fn frame(&mut self, packed: u32) -> SlotFrameOutcome {
        use legaia_engine_system::pad::{PACK_CIRCLE, PACK_CROSS, PACK_SQUARE, PACK_TRIANGLE};
        let on = |m: u16| packed & u32::from(m) != 0;
        let phase = self.phase();
        self.tick();
        if self.cash_out_input(packed) {
            return if self.phase() == SlotPhase::CashedOut {
                SlotFrameOutcome::CashedOut
            } else {
                SlotFrameOutcome::Stepped
            };
        }
        self.latch_spin_up(
            on(PACK_TRIANGLE) || on(PACK_CIRCLE) || on(PACK_CROSS) || on(PACK_SQUARE),
        );
        match phase {
            SlotPhase::Idle => {
                if on(PACK_CROSS) {
                    self.spin();
                }
            }
            SlotPhase::Stopping => {
                for (reel, m) in [PACK_SQUARE, PACK_CROSS, PACK_CIRCLE]
                    .into_iter()
                    .enumerate()
                {
                    if on(m) {
                        self.stop_reel(reel);
                    }
                }
            }
            SlotPhase::Payout => {
                if on(PACK_CROSS) {
                    self.collect();
                }
            }
            SlotPhase::CashedOut => return SlotFrameOutcome::Committed,
            SlotPhase::Spinning | SlotPhase::Menu | SlotPhase::NoCoins | SlotPhase::Leaving => {}
        }
        SlotFrameOutcome::Stepped
    }
}

/// The literal LCG seed the slot overlay's init writes to `DAT_801d3c80` -
/// the seed every host racks a cabinet with, so the same presses land the
/// same reels on the play hosts and the standalone minigames page.
pub const SLOT_RNG_SEED: u32 = 0x6C0A_2AF0;

/// The packed retail edge word for this frame off the engine's raw pad words
/// (`InputState::pad` / `pad_prev`, [`legaia_engine_system::pad::PadButton`] layout).
pub fn packed_edges(pad: u16, pad_prev: u16) -> u32 {
    u32::from(legaia_engine_system::pad::retail_packed(pad & !pad_prev))
}
