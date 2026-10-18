//! Handler slot `0x23` of the field overlay's submode dispatcher, the
//! flag-window picker `FUN_801EF014`: the `World` and `SceneHost` seams.
//!
//! The slot's arm, its state and the window layout are
//! `legaia_engine_field::field_submode_flag_window`; this module applies the
//! picker's flag writes to the world and reads the legend strings off the
//! user's disc.

use legaia_engine_field::field_submode_flag_window::*;
use legaia_engine_field::world_map_panel_actors::FlagWindowDescriptor;

/// The dispatcher slot `49 04` opens (`OP49_SUBOP_SLOTS[4]`).
pub const FLAG_WINDOW_SLOT: u16 = 0x23;

/// The descriptor the picker reads through `_DAT_8007B450`, from the op-`0x49`
/// bytes after the opcode (`[sub_op, count, first_visible, rows, base_lo,
/// base_hi]`). The base is the unaligned `u16` at `+4` (`FUN_8003CE9C(desc +
/// 4)` at `0x801EF040`).
pub fn descriptor_from_operand(op: &[u8]) -> FlagWindowDescriptor {
    let b = |i: usize| op.get(i).copied().unwrap_or(0);
    FlagWindowDescriptor {
        count: b(1),
        first_visible: b(2),
        rows: b(3),
        base_flag: i32::from(u16::from_le_bytes([b(4), b(5)])),
    }
}

impl crate::world::World {
    /// Slot `0x23`'s descriptor, off the parked operand.
    pub(crate) fn flag_window_descriptor(&self) -> FlagWindowDescriptor {
        descriptor_from_operand(&self.field_vm.submode_screen.op49_operand)
    }

    /// Apply the picker's flag writes to the system flag bank.
    pub(crate) fn apply_flag_window_writes(&mut self, writes: &[FlagWrite]) {
        for w in writes {
            match *w {
                FlagWrite::Clear(id) => {
                    if let Ok(i) = u16::try_from(id) {
                        self.system_flag_clear(i);
                    }
                }
                FlagWrite::Set(id) => {
                    if let Ok(i) = u16::try_from(id) {
                        self.system_flag_set(i);
                    }
                }
            }
        }
    }

    /// The floor window's lines while record 14 is installed on an open
    /// submode screen, else empty. `legend` is the two field-overlay strings
    /// ([`FLAG_WINDOW_LEGEND_VAS`]); a host without them passes empty slices.
    pub fn flag_window_lines(&self, legend: [&[u8]; 2]) -> Vec<FlagWindowLine> {
        let s = &self.field_vm.submode_screen;
        if !s.is_open() || !s.installed_windows.contains(&FLAG_WINDOW_RECORD) {
            return Vec::new();
        }
        flag_window_lines(&s.flag_window, self.flag_window_descriptor(), legend)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_field::submode_hub_actors::{HubAction, HubActor, HubGrid};
    use legaia_engine_field::world_map_panel::CursorPad;
    use legaia_engine_field::world_map_panel_actors::{
        FLAG_WINDOW_SCRIPT, flag_window_panel_geometry,
    };

    fn pad(edge: u32) -> CursorPad {
        CursorPad {
            held: edge,
            pressed: edge,
            action_a_mask: 0x40,
            action_b_mask: 0x20,
        }
    }

    trait IntoI16Pair {
        fn into_i16(self) -> (i16, i16);
    }
    impl IntoI16Pair for (i32, i32) {
        fn into_i16(self) -> (i16, i16) {
            (self.0 as i16, self.1 as i16)
        }
    }

    const KOR: [u8; 6] = [0x04, 0x08, 0x00, 0x08, 0x38, 0x01];

    #[test]
    fn the_kor_operand_is_eight_floors_from_flag_0x138() {
        let d = descriptor_from_operand(&KOR);
        assert_eq!(
            (d.count, d.first_visible, d.rows, d.base_flag),
            (8, 0, 8, 0x138)
        );
    }

    #[test]
    fn a_pick_clears_the_range_sets_one_floor_and_hands_back_to_the_draw_tick() {
        let d = descriptor_from_operand(&KOR);
        let mut a = HubActor {
            state: FLAG_WINDOW_SLOT,
            ..Default::default()
        };
        let mut g = HubGrid::default();
        let mut s = FlagWindowState::default();
        let mut w = Vec::new();
        // Phase 0: floor 2 (flag 0x13A) is the current one.
        let f = flag_window_slot(&mut a, &mut g, &mut s, d, pad(0), |id| id == 0x13A, &mut w);
        assert_eq!((s.remembered, s.selection), (2, 2));
        assert_eq!(
            (s.panel_y, s.panel_h),
            flag_window_panel_geometry(8).into_i16()
        );
        assert!(
            f.actions
                .contains(&HubAction::InstallPanel(FLAG_WINDOW_SCRIPT))
        );
        assert_eq!(w.len(), 8);
        w.clear();
        // Phase 1: one step, then Cross.
        flag_window_slot(&mut a, &mut g, &mut s, d, pad(0x1000), |_| false, &mut w);
        flag_window_slot(&mut a, &mut g, &mut s, d, pad(0x40), |_| false, &mut w);
        assert_eq!(w.len(), 1);
        assert!(matches!(w[0], FlagWrite::Set(id) if (0x138..0x140).contains(&id) && id != 0x13A));
        // Phases 2 and 3: the hand-back.
        flag_window_slot(&mut a, &mut g, &mut s, d, pad(0), |_| false, &mut w);
        flag_window_slot(&mut a, &mut g, &mut s, d, pad(0), |_| false, &mut w);
        assert_eq!((a.state, a.sub), (FLAG_WINDOW_EXIT_SLOT, 0));
        assert_eq!((g.handback, g.stashed_state), (-1, FLAG_WINDOW_SLOT));
    }

    #[test]
    fn the_kor_warp_pad_picks_a_floor_on_the_world_and_unparks() {
        use crate::world::World;
        let mut w = World::new();
        w.man_load_actor_reset();
        w.system_flag_set(0x139);
        // The field VM's arm: open the slot, park the operand.
        w.open_field_submode_screen(FLAG_WINDOW_SLOT, None);
        w.set_submode_board_entries(&[0x49, 0x04, 0x08, 0x00, 0x08, 0x38, 0x01]);
        // Phase 0: the scan remembers floor 1, clears the range, installs
        // record 14.
        w.tick_submode_screen(1);
        let s = &w.field_vm.submode_screen;
        assert_eq!(s.flag_window.remembered, 1);
        assert!(s.installed_windows.contains(&FLAG_WINDOW_RECORD));
        assert!(!w.system_flag_test(0x139), "phase 0 clears the range");
        assert!(!w.flag_window_lines([b"", b""]).is_empty());
        // Phase 1: one step and Cross, each a fresh edge.
        let pack = |m: u32| legaia_engine_system::pad::retail_packed(m as u16);
        for m in [0x1000u32, 0, SUBMODE_ACCEPT, 0] {
            w.input.set_pad(pack(m));
            w.tick_submode_screen(1);
        }
        let set: Vec<u16> = (0x138..0x140).filter(|&f| w.system_flag_test(f)).collect();
        assert_eq!(set.len(), 1, "exactly one floor flag is set: {set:?}");
        assert_ne!(set[0], 0x139, "a different floor from the current one");
        // The hand-back retires the driver and unparks the script.
        let mut retired = false;
        for _ in 0..6 {
            if w.tick_submode_screen(1) {
                retired = true;
                break;
            }
        }
        assert!(retired);
        assert!(w.field_vm.submode_screen.is_done());
    }

    const SUBMODE_ACCEPT: u32 = crate::field_submode_screen::SUBMODE_ACCEPT_MASK;

    #[test]
    fn the_rows_draw_bottom_up_with_the_current_floor_marked() {
        let d = descriptor_from_operand(&KOR);
        let (y, h) = flag_window_panel_geometry(8);
        let s = FlagWindowState {
            selection: 3,
            remembered: 2,
            panel_y: y as i16,
            panel_h: h as i16,
        };
        let l = flag_window_lines(&s, d, [b"a", b"b"]);
        // Row 0 (the basement plate) is the lowest.
        let basement = l.iter().find(|l| l.text == b"B1F").unwrap();
        let top = l.iter().find(|l| l.text == b"7").unwrap();
        assert!(basement.y > top.y);
        assert!(l.iter().any(|l| l.text == b"2" && l.marked));
        assert!(l.iter().any(|l| l.text == b">"));
    }
}
