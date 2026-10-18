//! Handler slot `0x21` of the field overlay's submode dispatcher, the code
//! lock `FUN_801EED58`: the `World` and `SceneHost` seams.
//!
//! The slot's arm and the window layout are
//! `legaia_engine_field::field_submode_code_lock`; this module reads the code
//! off the parked operand and the header string off the user's disc.

use legaia_engine_field::code_lock_actor::CODE_LEN;
use legaia_engine_field::field_submode_code_lock::*;
use legaia_engine_field::field_submode_flag_window::FlagWindowLine;

impl crate::world::World {
    /// Slot `0x21`'s code, off the parked operand.
    pub(crate) fn code_lock_code(&self) -> [u8; CODE_LEN] {
        code_from_operand(&self.field_vm.submode_screen.op49_operand)
    }

    /// The code lock's lines while record 12 is installed on an open submode
    /// screen running slot `0x21`, else empty. `header` is the painter's
    /// field-overlay string ([`CODE_LOCK_HEADER_VA`]); a host without it
    /// passes an empty slice.
    pub fn code_lock_lines(&self, header: &[u8]) -> Vec<FlagWindowLine> {
        let s = &self.field_vm.submode_screen;
        if !s.is_open()
            || !s.installed_windows.contains(&CODE_LOCK_RECORD)
            || s.actor.state != CODE_LOCK_SLOT
        {
            return Vec::new();
        }
        code_lock_lines(header, &s.code_lock.entered, s.code_lock.index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field_submode_screen::SUBMODE_ACCEPT_MASK;
    use crate::world::World;

    /// A synthetic operand - not the disc's code.
    const OPERAND: [u8; 7] = [0x49, 0x02, 0x01, 0x00, 0x02, 0x03, 0x01];

    fn world_with_lock() -> World {
        let mut w = World::new();
        w.man_load_actor_reset();
        w.open_field_submode_screen(CODE_LOCK_SLOT, None);
        w.set_submode_board_entries(&OPERAND);
        w
    }

    fn bit_for(sym: u8) -> u32 {
        match sym {
            0 => 0x40,
            1 => 0x20,
            2 => 0x80,
            _ => 0x10,
        }
    }

    fn enter(w: &mut World, code: &[u8]) {
        let pack = |m: u32| legaia_engine_system::pad::retail_packed(m as u16);
        for &sym in code {
            w.input.set_pad(pack(bit_for(sym)));
            w.tick_submode_screen(1);
            w.input.set_pad(0);
            w.tick_submode_screen(1);
        }
    }

    fn run_out(w: &mut World) -> bool {
        for _ in 0..64 {
            if w.tick_submode_screen(1) {
                return true;
            }
        }
        false
    }

    #[test]
    fn the_operand_carries_the_code_after_the_sub_op() {
        assert_eq!(code_from_operand(&OPERAND[1..]), [1, 0, 2, 3, 1]);
    }

    #[test]
    fn phase_zero_installs_record_twelve() {
        let mut w = world_with_lock();
        w.tick_submode_screen(1);
        let s = &w.field_vm.submode_screen;
        assert!(s.installed_windows.contains(&CODE_LOCK_RECORD));
        assert_eq!(s.actor.sub, 1);
        assert_eq!(w.code_lock_lines(b"h").len(), 1, "the header alone");
    }

    #[test]
    fn the_right_code_sets_flag_nine_and_unparks() {
        let mut w = world_with_lock();
        w.system_flag_clear(9);
        w.tick_submode_screen(1);
        enter(&mut w, &[1, 0, 2]);
        let lines = w.code_lock_lines(b"h");
        assert_eq!(lines.len(), 1 + 3, "header and three cells");
        assert_eq!(lines[1].text, b"O");
        assert_eq!(
            (lines[2].x - lines[1].x, lines[1].y),
            (0x20, CODE_LOCK_Y + 0x10)
        );
        enter(&mut w, &[3, 1]);
        assert!(run_out(&mut w), "the dispatcher retires the driver");
        assert!(w.system_flag_test(9));
        assert!(w.field_vm.submode_screen.is_done());
    }

    #[test]
    fn a_wrong_code_clears_flag_nine() {
        let mut w = world_with_lock();
        w.system_flag_set(9);
        w.tick_submode_screen(1);
        enter(&mut w, &[0, 0, 0, 0, 0]);
        assert!(run_out(&mut w));
        assert!(!w.system_flag_test(9));
    }

    #[test]
    fn the_accept_button_is_a_symbol_not_a_confirm() {
        // Cross is both the submode accept mask and symbol 0: the lock reads
        // it as an entry, not as a confirm.
        assert_eq!(SUBMODE_ACCEPT_MASK, 0x40);
        let mut w = world_with_lock();
        w.tick_submode_screen(1);
        enter(&mut w, &[0]);
        assert_eq!(w.field_vm.submode_screen.code_lock.index, 1);
    }
}
