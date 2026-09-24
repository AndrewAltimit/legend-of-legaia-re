//! The frame-step floor `DAT_8007B9D8` and the retail routines that write it.
//!
//! `FUN_80016B6C` raises the adaptive cadence `DAT_1F800393` to the floor's low
//! byte whenever it measures below it (`lw v1,-0x4628(v1)` / `slt` /
//! `lbu -0x4628` at `0x80017178..0x80017198`); nothing else reads the word
//! for timing. Every writer on the disc, read off the stores (`sw rt,-0x4628`
//! after `lui 0x8008`, and `sw v0,0x6c0(gp)`):
//!
//! | Writer | Site | Value | Engine |
//! |---|---|---|---|
//! | mode init `FUN_8001DCF8` | `0x8001DD2C` (`gp+0x6C0`) | `1` | superseded by each mode's own write below |
//! | field init `FUN_801D6704` | `0x801D6990` (PROT 0897) | `2` | scene entry installs it |
//! | move-VM ext `0x2F` | `0x801D45E4` (PROT 0897) | operand | `ext_set_8007b9d8` |
//! | name entry `FUN_801F03F0` | save `0x801F0468`, hold `0x801F0488`, restore `0x801F09A0` | `1` while open | [`World::open_name_entry`] / [`World::step_name_entry`] |
//! | save screen `FUN_801DC6B4` (PROT 0899) | `0x801DC7B4` / `0x801DC9BC`; save `0x801DD8AC`, restore `0x801DFC30` | `1`, then `2` | not modelled |
//! | fishing init (PROT 0972) | `0x801CF344` | `2` | not modelled |
//! | Baka Fighter (PROT 0976) | `0x801CFCC4` / `0x801D0FAC` | `1` / `4` | not modelled |
//! | arena init (PROT 0977) | `0x801CF8E0` | `2` | not modelled |
//! | battle intro (PROT 0979) | `0x801CFE00` | `3` | not modelled |
//! | dance (PROT 0980) | `0x801CF070` | `3` | not modelled |
//! | monster test (PROT 0981) | `0x801CE884` / `0x801CEC1C` | variable | dev mode, not ported |
//!
//! The unmodelled rows run in modes whose engine sessions keep their own
//! clocks; the audio ring ages by the display vsync, not by this floor, so
//! none of them moves a cue.
//!
//! REF: FUN_80016B6C, FUN_8001DCF8, FUN_801D6704, FUN_801F03F0

use super::*;

impl World {
    /// Install `floor` as the frame-step floor, and - because the engine runs
    /// the deterministic arm of `FUN_80016B6C`'s resolver, whose result is the
    /// floor itself - as the cadence too. Mirrors the move-VM ext `0x2F`
    /// write. A zero floor reads as `1` (the resolver never steps by zero).
    pub fn set_frame_step_floor(&mut self, floor: u8) {
        let floor = floor.max(1);
        self.clock.frame_step_floor = floor;
        self.clock.frame_step = floor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_entry_holds_the_floor_at_one_and_hands_it_back() {
        let mut w = World::new();
        w.set_frame_step_floor(2);
        w.open_name_entry(0);
        assert_eq!(w.clock.frame_step_floor, 1);
        assert_eq!(w.clock.frame_step, 1);
        // Force the commit state; the next step closes the screen.
        use crate::name_entry::NameEntryState;
        w.party.name_entry.as_mut().unwrap().state = NameEntryState::Done;
        w.step_name_entry(crate::name_entry::NameEntryInput::default());
        assert!(!w.name_entry_active());
        assert_eq!(w.clock.frame_step_floor, 2);
        assert_eq!(w.clock.frame_step, 2);
    }
}
