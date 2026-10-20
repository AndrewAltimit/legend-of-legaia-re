//! Field-host bodies for the op-`0x49` submode gate: state query, clear and the
//! menu request.

use super::*;

impl FieldHostImpl<'_> {
    pub(super) fn op49_state(&self) -> Op49State {
        // A field-VM-opened gold shop (op 0x49 sub-0 inline shop record) gates
        // the resume the same way name-entry does: Armed while the shop UI is
        // up, Done once the host closes it (`finish_field_shop`), so the VM
        // suspends across the shop and then advances past the merchant op.
        if self.world.shops.shop_armed {
            return if self.world.shops.shop_open {
                Op49State::Armed
            } else {
                Op49State::Done
            };
        }
        // A sub-7 casino prize exchange (`World::try_arm_prize_exchange`):
        // same shape as the gold shop - Armed while the exchange UI is up,
        // Done once the host closes it (`finish_prize_exchange`).
        if self.world.shops.prize_exchange_armed {
            return if self.world.shops.prize_exchange_open {
                Op49State::Armed
            } else {
                Op49State::Done
            };
        }
        // A sub-5 tile-board install (`World::try_install_tile_board`): Armed
        // while the board mode runs, Done once an event cell exits it, so the
        // script suspends across the whole board segment.
        if self.world.board.armed {
            return if self.world.board.grid.is_some() {
                Op49State::Armed
            } else {
                Op49State::Done
            };
        }
        if self.world.cutscene.prologue_naming_armed
            && self.world.cutscene.naming_owner == Some(self.world.op49_park_owner())
        {
            if self.world.name_entry_active() {
                Op49State::Armed
            } else {
                Op49State::Done
            }
        } else {
            // Every other sub-op parks on `_DAT_8007B450` until the
            // `FUN_801F159C`-class actor writes `1` there. That writer is
            // `World::tick_submode_screen`; without it these sub-ops re-armed
            // and halted on the same PC every frame, forever.
            //
            // The park is read only by the context that armed it
            // (`Op49ParkOwner`): retail's `_DAT_8007B450` is one global, but
            // the port steps the field script, the channels and the spawned
            // record contexts inside one `World::tick`, so a screen armed by
            // the field script must not park the cutscene timeline's own
            // op-`0x49` - which is the town01 name-entry hand-off.
            let owner = self.world.op49_park_owner();
            if self.world.field_vm.submode_screen.is_open_for(owner) {
                Op49State::Armed
            } else if self.world.field_vm.submode_screen.is_done_for(owner) {
                Op49State::Done
            } else {
                Op49State::Idle
            }
        }
    }
    pub(super) fn op49_clear(&mut self) {
        // Retail's `sw zero,-0x4bb0(s0)` on the Done edge.
        self.world.clear_op49_park();
        // The shop op's resume ran: drop the arm so a later op-0x49 can open
        // the next merchant. (Name-entry clears via its own pending flags.)
        self.world.shops.shop_armed = false;
        // Same for a finished prize exchange (sub-7).
        self.world.shops.prize_exchange_armed = false;
        // A finished tile-board segment resumes the same way.
        self.world.board.armed = false;
        // The submode screen's Done is one-shot: consume it so the next
        // op-0x49 opens a fresh screen rather than resuming instantly. Only
        // the context that armed the park may consume it - the name-entry
        // hand-off's own resume runs through this same hook, and clearing
        // another context's pending Done would strand it back on Idle and
        // re-open the screen it had just finished.
        let owner = self.world.op49_park_owner();
        if self.world.field_vm.submode_screen.owner == owner {
            self.world.field_vm.submode_screen.done = false;
        }
        // A finished naming prompt outside the opening is spent with its
        // resume, so the context's later op-0x49s park on their own screens.
        // The opening timeline keeps its latch: its later STATE_RESUMEs have
        // always resolved through it.
        if self.world.cutscene.prologue_naming_armed
            && !self.world.name_entry_active()
            && self.world.cutscene.naming_owner == Some(owner)
            && owner != crate::field_submode_screen::Op49ParkOwner::CutsceneTimeline
        {
            self.world.cutscene.prologue_naming_pending = false;
            self.world.cutscene.prologue_naming_armed = false;
            self.world.cutscene.naming_owner = None;
        }
    }
    pub(super) fn op49_menu_request(&mut self, sub_op: u8, instr: &[u8]) {
        // Every Idle arm spawns the subsystem actor, whose installer stands
        // the player and takes down a held box before any screen runs.
        self.world.run_submode_installer();
        // Recognise + open an inline gold shop (sub-0); non-shop op-0x49 sub-0
        // payloads (inn / save prompts) fail the priced-record validation.
        if sub_op == 0 {
            self.world.try_arm_field_shop(instr);
        }
        // Sub-5 carries the inline 13-byte tile-board header: install the
        // board (retail arms `_DAT_8007b450` at this window and the board
        // consumer takes over the frame).
        if sub_op == 5 {
            self.world.try_install_tile_board(instr);
        }
        // Sub-3 is the name-entry hand-off wherever it executes: retail's
        // handler table maps it to `FUN_801F03F0` unconditionally, with no
        // test of how the record was reached. The opening install raises
        // the pending flag up front; a record reached any other way (a card
        // load replaying `town01` P2[3], the comparison corpus's resume)
        // raises it here, so `op49_invoke_setup` opens the screen on the
        // same Idle->arm edge.
        if sub_op == 3 {
            self.world.cutscene.prologue_naming_pending = true;
            self.world.cutscene.prologue_naming_armed = false;
            self.world.cutscene.naming_owner = Some(self.world.op49_park_owner());
            self.world.cutscene.naming_slot = instr.get(2).map_or(0, |&b| usize::from(b));
        }
        // Sub-7 is the casino prize-exchange counter (menu-overlay
        // sub-screen 0x20); the byte after the sub-op selects the prize
        // block (koin1 = 0, balden = 1).
        if sub_op == 7 {
            self.world.try_arm_prize_exchange(instr);
        }
        // Anything else is a submode sub-screen: open one on the driver actor
        // so the dispatcher runs it and, when it hands back, unparks this op.
        if let Some(slot) = crate::field_submode_screen::slot_for_op49_sub_op(sub_op) {
            self.world.open_field_submode_screen(slot, None);
            // Retail parks the operand *pointer*, and the screens read past
            // the sub-op byte through it (the start menu counts `+1..=3`), so
            // the payload has to travel with the arm.
            self.world.set_submode_board_entries(instr);
        }
    }
}
