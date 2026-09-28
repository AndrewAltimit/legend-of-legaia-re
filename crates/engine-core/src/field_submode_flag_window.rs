//! Handler slot `0x23` of the field overlay's submode dispatcher: the
//! **flag-window picker** `FUN_801EF014`, on the field path that reaches it.
//!
//! The field VM's `49 04` (`OP49_SUBOP_SLOTS[4] = 0x23`) opens it. Its one
//! disc use is the Uru Mais warp pads in `kor` / `kor3` / `kor4`: every
//! carrier issues `49 04 08 00 08 38 01` or `49 04 08 04 04 38 01` - eight
//! system flags from `0x138`, eight or four rows visible - and then dispatches
//! an eight-way scene change on the one flag left set (the kor-family flag
//! window in `docs/reference/re-settled-threads.md`). The row cells are floor
//! plates in the system-UI sheet (record `0x4F` the basement plate,
//! `0x50..=0x56` the digits `1..=7`, `0x57` the floor suffix; `0x58..=0x60`
//! the same set in the marked ink - read off the widget table `0x800732A4`
//! against a field state's VRAM), so each destination is a floor: the current
//! one remembered, the pick committed as the only set flag.
//!
//! The picker had a port (`legaia_engine_vm::world_map_panel_actors::flag_window_tick`)
//! hosted only by `crate::world_map_panel_host` behind a world-map pad chord,
//! while the field path fell into [`crate::field_submode_screen`]'s empty
//! default arm: the dispatcher retired the actor on its first frame, the script
//! unparked with no floor chosen (the pad's own row stays the only set
//! flag, so the ladder sends the party to the pad it stands on), and nothing
//! drew. This module is the slot's arm on the field path and the painter of
//! the window it installs.
//!
//! ## The actor contract
//!
//! `FUN_801EF014` runs on the submode driver actor like every other slot: its
//! phase is the actor's `+0x54` ([`HubActor::sub`]), and its exit arm (phase
//! `3`, `0x801EF260..0x801EF280`) writes the dispatcher context's `+0x2E = -1`
//! and `+0x40 = actor[+0x50]`, re-arms `+0x50 = 0x1A` (the draw tick, which
//! clears the completion gate) and zeroes the phase. So the hand-back is the
//! same one the ported start menu makes, and the dispatcher retires the actor
//! on the frame after.
//!
//! ## The window
//!
//! Phase 0 installs descriptor `0x801F3304` - one entry, kind `1`, window
//! `0x0E` - after sizing panel record 14 (`0x801F2B98 + 14 * 0x1C`: `+0xA` =
//! `(8 - rows) * 16 + 0x48`, `+0xE` = `rows * 16`). Record 14's other
//! geometry is `x = 0x18`, `w = 0x48`; its painter is `FUN_801E6984`, ported
//! as [`crate::field_submode::submode_panel_rows`] for the row loop. The rest
//! of the painter (`0x801E6A74..0x801E6B0C`) is the legend beside the panel:
//! the Cross and Circle icons (`0x37` / `0x38`) at `(x + w + 0x20, y + h -
//! 0x20)` and one row below, the two field-overlay strings `0x801CF0B8` /
//! `0x801CF0C4` at `(x + w + 0x34, y + h - 0x1E)` / `(.., y + h - 0x0E)`, and a
//! `0x9C x 0x20` widget frame at `(x + w + 0x10, y + h - 0x20)`.

use legaia_engine_vm::baka_hub_actors::{HubAction, HubActor, HubFrame, HubGrid, slot};
use legaia_engine_vm::world_map_panel::CursorPad;
use legaia_engine_vm::world_map_panel_actors::{
    FLAG_WINDOW_SCRIPT, FlagWindowDescriptor, FlagWindowEffect, FlagWindowInput, flag_window_tick,
};

/// The dispatcher slot `49 04` opens (`OP49_SUBOP_SLOTS[4]`).
pub const FLAG_WINDOW_SLOT: u16 = 0x23;

/// The panel-window record the picker's descriptor installs.
pub const FLAG_WINDOW_RECORD: usize = 14;

/// Record 14's x (`+0x8`) and width (`+0xC`), as the table holds them.
pub const FLAG_WINDOW_X: i16 = 0x18;
/// Record 14's width.
pub const FLAG_WINDOW_W: i16 = 0x48;

/// Load base of the field overlay (PROT 0897), slot A.
pub const FIELD_OVERLAY_BASE: u32 = 0x801C_E818;

/// The legend's two field-overlay strings (`0x801E6ABC` / `0x801E6AE0`).
pub const FLAG_WINDOW_LEGEND_VAS: [u32; 2] = [0x801C_F0B8, 0x801C_F0C4];

/// The picker's state beside the actor: the two globals it keeps and the
/// geometry it wrote into record 14.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlagWindowState {
    /// `_DAT_8007BB88` - the selection (a flag offset from the base).
    pub selection: i32,
    /// `_DAT_8007BB9C` - the row whose flag was set on entry.
    pub remembered: i32,
    /// Record 14's `+0xA` / `+0xE` as the picker sized them.
    pub panel_y: i16,
    pub panel_h: i16,
}

/// One flag write the picker asks for (`FUN_8003CE34` / `FUN_8003CE08`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagWrite {
    Clear(i32),
    Set(i32),
}

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

/// Run one frame of slot `0x23` on the driver actor.
///
/// `flag_test` answers the phase-0 scan off the system flag bank; the writes
/// come back in `writes` for the caller to apply after the dispatch (the scan
/// borrows the bank the writes mutate).
// PORT: FUN_801EF014 (the field-path arm: actor `+0x54` phase, the `0x801EF260` hand-back)
pub fn flag_window_slot(
    actor: &mut HubActor,
    grid: &mut HubGrid,
    state: &mut FlagWindowState,
    desc: FlagWindowDescriptor,
    pad: CursorPad,
    flag_test: impl Fn(i32) -> bool,
    writes: &mut Vec<FlagWrite>,
) -> HubFrame {
    let mut frame = HubFrame::default();
    let input = FlagWindowInput {
        desc,
        input_locked: false,
        selection: state.selection,
        remembered: state.remembered,
        pad,
        handler_id: actor.state,
    };
    let (phase, selection, remembered, effects) = flag_window_tick(actor.sub, input, flag_test);
    actor.sub = phase;
    state.selection = selection;
    state.remembered = remembered;
    for e in effects {
        match e {
            FlagWindowEffect::ClearRange { base, count } => {
                writes.extend((0..i32::from(count)).map(|i| FlagWrite::Clear(base + i)));
            }
            FlagWindowEffect::SizePanel { y, height } => {
                state.panel_y = y as i16;
                state.panel_h = height as i16;
            }
            FlagWindowEffect::RunPanelScript(va) => frame.actions.push(HubAction::InstallPanel(va)),
            FlagWindowEffect::SetFlag(id) => writes.push(FlagWrite::Set(id)),
            FlagWindowEffect::Exit(x) => {
                grid.handback = -1;
                grid.stashed_state = x.saved_handler;
                actor.state = x.next_handler;
                actor.sub = 0;
            }
            FlagWindowEffect::TickTextActors => frame.actions.push(HubAction::DrawPump),
        }
    }
    frame
}

/// The windows descriptor `va` installs, with the picker's own descriptor
/// added to the ones [`legaia_engine_vm::baka_hub_actors::panel_windows`]
/// knows.
pub fn installed_windows(va: u32) -> Vec<usize> {
    if va == FLAG_WINDOW_SCRIPT {
        vec![FLAG_WINDOW_RECORD]
    } else {
        legaia_engine_vm::baka_hub_actors::panel_windows(va).to_vec()
    }
}

/// One laid-out line of the window: text (disc bytes, or the plate stand-in),
/// its pen, and whether it takes the marked ink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagWindowLine {
    pub text: Vec<u8>,
    pub x: i16,
    pub y: i16,
    pub marked: bool,
}

/// The text a floor plate cell reads as: record `0x4F` / `0x58` is the
/// basement plate, `0x50..=0x56` / `0x59..=0x5F` the digits `1..=7`, and the
/// second cell (`ink + 8`, `0x57` / `0x60`) the floor suffix. Hosts have no
/// system-UI sprite pass for these records, so the plate is drawn as its own
/// lettering.
fn plate_text(entry: u8) -> Vec<u8> {
    if entry == 0 {
        b"B1F".to_vec()
    } else {
        format!("{entry}").into_bytes()
    }
}

/// Lay the window out: the row loop (`FUN_801E6984`'s, via
/// [`crate::field_submode::submode_panel_rows`]) plus the legend. `legend` is
/// the two field-overlay strings, read off the disc by the host.
pub fn flag_window_lines(
    state: &FlagWindowState,
    desc: FlagWindowDescriptor,
    legend: [&[u8]; 2],
) -> Vec<FlagWindowLine> {
    use crate::field_submode::{PANEL_GLYPH_DX, PANEL_GLYPH2_DX, PANEL_HIGHLIGHT_DX};
    let (x, y) = (FLAG_WINDOW_X, state.panel_y);
    let (w, h) = (FLAG_WINDOW_W, state.panel_h);
    let mut out = Vec::new();
    let rows = crate::field_submode::submode_panel_rows(
        (x, y),
        desc.rows,
        desc.first_visible,
        state.selection.clamp(0, 0xFF) as u8,
        state.remembered.clamp(0, 0xFF) as u8,
    );
    for r in rows {
        if r.highlighted {
            // `FUN_8002B994(0, 1, x + 0xC, y)` - the row cursor.
            out.push(FlagWindowLine {
                text: b">".to_vec(),
                x: x + PANEL_HIGHLIGHT_DX,
                y: r.y,
                marked: false,
            });
        }
        let marked = r.ink == crate::field_submode::PANEL_INK_MARKED;
        out.push(FlagWindowLine {
            text: plate_text(r.entry),
            x: x + PANEL_GLYPH_DX,
            y: r.y,
            marked,
        });
        if r.second_run {
            out.push(FlagWindowLine {
                text: b"F".to_vec(),
                x: x + PANEL_GLYPH2_DX,
                y: r.y,
                marked,
            });
        }
    }
    // The legend: button icons (as their letters) and the two strings.
    let (lx, ly) = (x + w, y + h);
    for (i, (icon, label)) in [(&b"X"[..], legend[0]), (&b"O"[..], legend[1])]
        .into_iter()
        .enumerate()
    {
        let row = ly - 0x20 + 0x10 * i as i16;
        out.push(FlagWindowLine {
            text: icon.to_vec(),
            x: lx + 0x20,
            y: row,
            marked: false,
        });
        out.push(FlagWindowLine {
            text: label.to_vec(),
            x: lx + 0x34,
            y: row + 2,
            marked: false,
        });
    }
    out
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

impl crate::scene::SceneHost {
    /// [`crate::world::World::flag_window_lines`] with the legend read off the
    /// user's disc (the field overlay, extraction entry `0897`). The native
    /// window and the browser play page both draw this.
    pub fn flag_window_lines(&self) -> Vec<FlagWindowLine> {
        let s = &self.world.field_vm.submode_screen;
        if !s.is_open() || !s.installed_windows.contains(&FLAG_WINDOW_RECORD) {
            return Vec::new();
        }
        let bytes = self
            .index
            .entry_bytes(crate::incense_notice::FIELD_OVERLAY_PROT_INDEX)
            .ok();
        let read = |va: u32| -> Vec<u8> {
            let Some(b) = bytes.as_deref() else {
                return Vec::new();
            };
            let off = va.wrapping_sub(FIELD_OVERLAY_BASE) as usize;
            b.get(off..)
                .and_then(|r| r.iter().position(|&c| c == 0).map(|e| r[..e].to_vec()))
                .unwrap_or_default()
        };
        let (a, b) = (
            read(FLAG_WINDOW_LEGEND_VAS[0]),
            read(FLAG_WINDOW_LEGEND_VAS[1]),
        );
        self.world.flag_window_lines([&a, &b])
    }
}

/// The slot's hand-back target, re-exported for the host test.
pub const FLAG_WINDOW_EXIT_SLOT: u16 = slot::DRAW_TICK;

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_vm::world_map_panel_actors::flag_window_panel_geometry;

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
        let pack = |m: u32| crate::dev_menu::retail_packed(m as u16);
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
