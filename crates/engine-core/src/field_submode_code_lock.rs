//! Handler slot `0x21` of the field overlay's submode dispatcher: the
//! **code lock** `FUN_801EED58`, on the field path that reaches it.
//!
//! The field VM's `49 02` (`OP49_SUBOP_SLOTS[2] = 0x21`) opens it. Its one
//! disc use is `doman`'s streamed MAN (extraction `0401`, partition 1 record
//! 18): a line of dialogue, then `49 02` with the five-symbol code as the
//! operand bytes after the sub-op, then `70 09` - a system-flag test on flag
//! `9`, which is the flag the lock's verdict writes. So the script branches on
//! whether the player entered the code.
//!
//! Before this arm the slot fell into [`crate::field_submode_screen`]'s empty
//! default: nothing drew, no press was read, and flag `9` was never written,
//! so the script always took the branch its prior flag state picked.
//!
//! ## The actor contract
//!
//! The state machine is [`legaia_engine_vm::code_lock_actor::CodeLockActor`].
//! Retail keeps its phase in the driver actor's `+0x54` ([`HubActor::sub`]),
//! the entry cursor in `_DAT_8007BB88`, and the entered symbols in the
//! dispatcher context `DAT_801C6EA4 + 0x54..0x58` ([`HubGrid::columns`]). The
//! exit arm (phase 4, `0x801EEFD4..0x801EEFF8`) writes context `+0x2E = -1`,
//! `+0x40 = actor[+0x50]`, re-arms `+0x50 = 0x1A` (the draw tick) and zeroes the
//! phase - the same hand-back the flag window makes - so the dispatcher
//! retires the actor on a following frame and the op-`0x49` park reads Done.
//! Every frame ends in `FUN_80031D00`, the text-actor pump.
//!
//! ## The window
//!
//! Phase 0 installs descriptor `0x801F32F4` - one entry, kind `1`, window
//! `12` - through `FUN_801E9B3C`, which creates the window from record 12 and
//! moves it to the record's `+8` / `+0xA` (`x = 0x40`, `y = 0x90`; `w = 0xB8`,
//! `h = 0x20`). Record 12's painter is `FUN_801F17D8`: the header string
//! `0x801CF09C` at the window origin, then one system-UI cell per entered
//! symbol at `(x + 0x10 + 0x20 * i, y + 0x10)`, cell `0x37 + symbol`. Cells
//! `0x37` / `0x38` are the Cross / Circle icons the floor window's legend also
//! uses; `0x39` / `0x3A` follow the symbol order (Square / Triangle). Hosts
//! have no system-UI sprite pass for those records, so a cell is drawn as the
//! button's letter.

use legaia_engine_vm::baka_hub_actors::{HubAction, HubActor, HubFrame, HubGrid, slot, window};
use legaia_engine_vm::code_lock_actor::{CODE_LEN, CodeLockActor};

use crate::field_submode_flag_window::{FIELD_OVERLAY_BASE, FlagWindowLine, FlagWrite};

/// The dispatcher slot `49 02` opens (`OP49_SUBOP_SLOTS[2]`).
pub const CODE_LOCK_SLOT: u16 = 0x21;

/// The panel descriptor phase 0 installs (`lui a0,0x801f; addiu a0,a0,0x32f4`
/// at `0x801EED98`).
pub const CODE_LOCK_PANEL: u32 = 0x801F_32F4;

/// The panel-window record that descriptor names.
pub const CODE_LOCK_RECORD: usize = window::COLUMN_ROW;

/// Record 12's `+8` / `+0xA` - where `FUN_801E9B3C` places the window.
pub const CODE_LOCK_X: i16 = 0x40;
/// Record 12's y.
pub const CODE_LOCK_Y: i16 = 0x90;

/// The header string the painter prints (`0x801F17E4..0x801F17E8`).
pub const CODE_LOCK_HEADER_VA: u32 = 0x801C_F09C;

/// The system flag the verdict writes: set on a match (`FUN_8003CE08(9)` at
/// `0x801EEF74`), cleared otherwise (`FUN_8003CE34(9)` at `0x801EEF8C`).
pub const CODE_LOCK_RESULT_FLAG: i32 = 9;

/// The stand-in lettering for each stored symbol (`0` Cross, `1` Circle,
/// `2` Square, `3` Triangle).
pub const SYMBOL_GLYPHS: [&[u8]; 4] = [b"X", b"O", b"[]", b"^"];

/// The five code bytes at `_DAT_8007B450 + 1..=5`, from the parked operand
/// (`[sub_op, c0, c1, c2, c3, c4]`, as the phase-3 compare reads them at
/// `0x801EEF0C..0x801EEF5C`).
pub fn code_from_operand(op: &[u8]) -> [u8; CODE_LEN] {
    let mut out = [0u8; CODE_LEN];
    for (i, b) in out.iter_mut().enumerate() {
        *b = op.get(1 + i).copied().unwrap_or(0);
    }
    out
}

/// Run one frame of slot `0x21` on the driver actor.
///
/// `pad_edge` is the packed just-pressed word (`_DAT_8007B874`), `locked` the
/// busy gate `_DAT_8007BB80`, `frame_delta` the cadence scalar `DAT_1F800393`.
/// The verdict's flag write comes back in `writes` for the caller to apply
/// after the dispatch, the way the floor window's do.
// PORT: FUN_801EED58 (the field-path arm: actor `+0x54` phase, the `0x801EEFD4` hand-back)
#[allow(clippy::too_many_arguments)]
pub fn code_lock_slot(
    actor: &mut HubActor,
    grid: &mut HubGrid,
    state: &mut CodeLockActor,
    code: &[u8; CODE_LEN],
    pad_edge: u32,
    locked: bool,
    frame_delta: i32,
    writes: &mut Vec<FlagWrite>,
) -> HubFrame {
    let mut frame = HubFrame::default();
    state.phase = actor.sub as u16;
    let out = state.tick(pad_edge as u16, locked, frame_delta as i16, code);
    actor.sub = state.phase as i16;
    grid.columns = state.entered.to_vec();
    if out.open_window {
        frame.actions.push(HubAction::InstallPanel(CODE_LOCK_PANEL));
    }
    match (out.cue, out.verdict) {
        // `FUN_80035BD0(0x23)` overwrites the last-written ring slot.
        (Some(cue), Some(false)) => frame.actions.push(HubAction::ConfirmCue(cue)),
        // The press and the success cue are `FUN_80035B50` pushes.
        (Some(cue), _) => frame.actions.push(HubAction::EntryCue(cue)),
        (None, _) => {}
    }
    match out.verdict {
        Some(true) => writes.push(FlagWrite::Set(CODE_LOCK_RESULT_FLAG)),
        Some(false) => writes.push(FlagWrite::Clear(CODE_LOCK_RESULT_FLAG)),
        None => {}
    }
    if out.released {
        grid.handback = -1;
        grid.stashed_state = actor.state;
        actor.state = slot::DRAW_TICK;
        actor.sub = 0;
    }
    frame.actions.push(HubAction::DrawPump);
    frame
}

/// Lay out record 12 the way `FUN_801F17D8` does: the header at the window
/// origin, then one cell per entered symbol (`index` of them).
pub fn code_lock_lines(header: &[u8], entered: &[u8], index: usize) -> Vec<FlagWindowLine> {
    let mut out = vec![FlagWindowLine {
        text: header.to_vec(),
        x: CODE_LOCK_X,
        y: CODE_LOCK_Y,
        marked: false,
    }];
    let mut x = CODE_LOCK_X + 0x10;
    for &sym in entered.iter().take(index) {
        out.push(FlagWindowLine {
            text: SYMBOL_GLYPHS
                .get(usize::from(sym))
                .copied()
                .unwrap_or(b"?")
                .to_vec(),
            x,
            y: CODE_LOCK_Y + 0x10,
            marked: false,
        });
        x += 0x20;
    }
    out
}

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

impl crate::scene::SceneHost {
    /// [`crate::world::World::code_lock_lines`] with the header read off the
    /// user's disc (the field overlay, extraction entry `0897`). The native
    /// window and the browser play page both draw this.
    pub fn code_lock_lines(&self) -> Vec<FlagWindowLine> {
        let s = &self.world.field_vm.submode_screen;
        if !s.is_open() || s.actor.state != CODE_LOCK_SLOT {
            return Vec::new();
        }
        let header = self
            .index
            .entry_bytes(crate::incense_notice::FIELD_OVERLAY_PROT_INDEX)
            .ok()
            .and_then(|b| {
                let off = CODE_LOCK_HEADER_VA.wrapping_sub(FIELD_OVERLAY_BASE) as usize;
                b.get(off..)
                    .and_then(|r| r.iter().position(|&c| c == 0).map(|e| r[..e].to_vec()))
            })
            .unwrap_or_default();
        self.world.code_lock_lines(&header)
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
        let pack = |m: u32| crate::dev_menu::retail_packed(m as u16);
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
