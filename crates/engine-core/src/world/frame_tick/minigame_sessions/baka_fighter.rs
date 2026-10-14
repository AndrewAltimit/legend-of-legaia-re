//! The Baka Fighter session: enter, exit, tick and the XA prestage.

use super::*;
use legaia_engine_mode::mode::SceneMode;

impl World {
    /// Enter the Baka Fighter duel on `fight`, suspending the current scene
    /// mode (restored by [`World::exit_baka_fighter`]). Like the dance /
    /// fishing / slot / pause-menu suspend contract, the interrupted field
    /// state stays intact underneath.
    pub fn enter_baka_fighter(&mut self, fight: legaia_engine_minigames::baka_fighter::BakaFight) {
        if self.mode != SceneMode::BakaFighter {
            self.minigames.baka_return_mode = self.mode;
        }
        // Retail reaches the duel through the mode-24 door warp: the field-VM
        // `0x3E` arm zeroes the winnings accumulator `_DAT_80084440` and the
        // mode-24 OTHER-INIT `FUN_80025980` backs up the active scene name.
        // Only a field entry goes through that warp; an engine-only entry from
        // another mode keeps the plain suspend/restore contract.
        if self.minigames.baka_return_mode == SceneMode::Field {
            self.arm_minigame_warp();
        }
        self.minigames.baka_fighter = Some(fight);
        self.mode = SceneMode::BakaFighter;
        self.queue_baka_xa_prestage();
    }

    /// Leave the Baka Fighter duel through the mode-24 return warp
    /// ([`Self::minigame_return_warp`], retail `FUN_80026018`): the winnings
    /// accumulator is banked into [`legaia_engine_minigame_scenes::minigame_state::MinigameState::casino_coins`], the backed-up scene
    /// name is restored and the mode drops back to the field.
    ///
    /// On a decided match with a player win, whatever prize the end-of-match
    /// tally has not yet drained is added to the accumulator first - retail's
    /// tally (`FUN_801D239C` at `0x801D28A8..0x801D28BC`) drains
    /// `DAT_801DBEE8` into `_DAT_80084440` a step at a time while the result
    /// screen is up, so leaving early has to bank the remainder for the total
    /// paid to match the prize either way.
    ///
    /// Returns the fight so the host can read the final state. No-op when no
    /// duel is active.
    pub fn exit_baka_fighter(
        &mut self,
    ) -> Option<legaia_engine_minigames::baka_fighter::BakaFight> {
        let fight = self.minigames.baka_fighter.take();
        if let Some(f) = fight.as_ref()
            && f.winner() == Some(0)
        {
            let owed = f.tally_gold_remaining().max(0) as u32;
            self.minigames.winnings = self.minigames.winnings.saturating_add(owed);
        }
        let return_mode = self.minigames.baka_return_mode;
        if self.mode == SceneMode::BakaFighter {
            // The warp's own mode write is retail's mode-2 (field) latch. An
            // engine-only entry from another mode restores that mode instead,
            // keeping the suspend contract the other minigames use.
            self.minigame_return_warp();
            if return_mode != SceneMode::Field {
                self.mode = return_mode;
            }
        }
        fight
    }

    /// Advance the Baka Fighter duel one frame, reading this frame's pad:
    ///
    /// - [`Square`](input::PadButton::Square) / [`Circle`](input::PadButton::Circle)
    ///   / [`Cross`](input::PadButton::Cross) commit attack types 1 / 2 / 3
    ///   for the player slot - retail's slot-0 read of the edge word
    ///   `_DAT_8007B874` (`andi 0x80` -> type 1 at `0x801D43B4`, `0x20` ->
    ///   type 2 at `0x801D43CC`, `0x40` -> type 3 at `0x801D43E4`, in
    ///   Legaia's packed pad layout). The three tests run in that order and
    ///   each overwrites the last, so on a frame with several edges Cross
    ///   wins, then Circle.
    /// - [`Triangle`](input::PadButton::Triangle) commits the chargeable
    ///   special (type 4) - a port enhancement: retail has no button for
    ///   type 4, which is its auto-finisher (`0x801D4550`).
    /// - The CPU slot picks through the ported `FUN_801d487c` roll inside
    ///   [`legaia_engine_minigames::baka_fighter::BakaFight::tick`].
    /// - When the match is decided, the frame's packed pad edge goes to the
    ///   cabinet (`FUN_801CF388`, [`legaia_engine_minigames::baka_cabinet::BakaCabinet`]),
    ///   which runs the rest of the **ladder** inside this one mode-24 visit,
    ///   as retail does: a win reaches the tally and then the "NEXT GAME /
    ///   PAY OUT" choice (Left / Right, confirm Cross); NEXT GAME seats the
    ///   next rung's opponent through the cabinet's install state, PAY OUT
    ///   and the all-clear run the exit state. A loss runs "GAME OVER",
    ///   whose first frame zeroes the prize accumulator (`sw zero,0x300(s2)`
    ///   = `_DAT_80084440` at `0x801D1288`), then the exit. The exit's fade
    ///   ending is where the return warp ([`World::exit_baka_fighter`])
    ///   banks what is left into the coin bank.
    ///
    /// The ladder therefore never outlives the visit: every exit of the
    /// cabinet runs through state `0x1F4`, and there is no save point inside
    /// mode 24, so it has no save representation to carry - the only
    /// persistent result is the coin bank.
    ///
    /// PORT: the Baka Fighter per-frame drive (`FUN_801d3f44` player input →
    /// type commit; `FUN_801d3468` resolution SM via `BakaFight::tick`).
    pub(in crate::world::frame_tick) fn tick_baka_fighter(&mut self) {
        if self.minigames.baka_fighter.is_none() {
            // Mode is BakaFighter but no fight installed - drop back.
            self.mode = self.minigames.baka_return_mode;
            return;
        }
        // The cabinet's whole frame (`BakaFight::frame`), shared with the
        // standalone minigames page: the front end, the duel's throw, and
        // after a match the tally / NEXT GAME / PAY OUT sheet. Each drained
        // tally step banks into the mode-24 winnings accumulator exactly as
        // retail's `FUN_801D239C` adds it into `_DAT_80084440` - the coin
        // prize, not party gold (`0x8008459C`); the exit warp
        // ([`Self::minigame_return_warp`]) then pays the accumulator into the
        // casino coin bank.
        let edge =
            legaia_engine_system::pad::retail_packed(self.input.pad() & !self.input.pad_prev());
        let held = legaia_engine_system::pad::retail_packed(self.input.pad());
        let pot = self.minigames.winnings;
        let out = match self.minigames.baka_fighter.as_mut() {
            Some(f) => f.frame(edge, held, pot),
            None => return,
        };
        if out.paid > 0 {
            self.minigames.winnings = self.minigames.winnings.saturating_add(out.paid);
        }
        if out.forfeit {
            self.minigames.winnings = 0;
        }
        if out.exit {
            self.exit_baka_fighter();
            return;
        }
        self.queue_baka_xa_prestage();
    }

    /// The duel chrome's announcer lines not yet listed, onto the prestage
    /// list both hosts drain ([`Self::queue_xa_prestage`]).
    pub(in crate::world::frame_tick) fn queue_baka_xa_prestage(&mut self) {
        let lines = self
            .minigames
            .baka_fighter
            .as_mut()
            .map(|f| f.take_xa_prestage())
            .unwrap_or_default();
        self.queue_xa_prestage(lines);
    }
}
