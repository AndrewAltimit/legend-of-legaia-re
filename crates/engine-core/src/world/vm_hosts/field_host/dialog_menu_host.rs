//! `FieldDialogMenuHost` for [`FieldHostImpl`].

use super::*;

impl<'a> FieldDialogMenuHost for FieldHostImpl<'a> {
    // Op `4C E0 <b1>` (arm `0x801E306C`): `0` raises the instant-page latch
    // `_DAT_801F2744`, `1..100` sets the reading box's row capacity
    // `_DAT_801F2740` - `izumi`'s help pages open twelve-row boxes that load
    // whole. `>= 100` writes the picker word `*(_DAT_801C6EA4) + 0xE`, which
    // nothing in the port reads.
    fn op4c_ne_sub0_state_write(&mut self, b1: u8) {
        if b1 == 0 {
            self.world.dialog.window_instant = true;
        } else if b1 < 100 {
            self.world.dialog.window_rows = b1;
        }
    }

    /// Op `4C D0`: a confirm or cancel **new press** - the pad edge
    /// `_DAT_8007B874` against `_DAT_800846D0 | _DAT_800846D4`, both in the
    /// packed pad layout - releases the prompt. The cursor `FUN_8002B994`
    /// draws while it holds has no engine sprite.
    fn press_prompt(&mut self, _x: u16, _y: u16) -> bool {
        use legaia_engine_field::world_map_panel_host::{
            PANEL_ACTION_A, PANEL_ACTION_B, packed_pad,
        };
        let pad = u32::from(packed_pad(self.world.input.pad()));
        let prev = u32::from(packed_pad(self.world.input.pad_prev()));
        (pad & !prev) & (PANEL_ACTION_A | PANEL_ACTION_B) != 0
    }

    // Op-0x49 STATE_RESUME, scoped to the `town01` opening cutscene timeline.
    // The pinned name-entry handoff (P2[3] body `0x02c6`, `49 03 00`) suspends
    // the script here; the engine opens the name-entry overlay on the Idle->arm
    // edge and keeps the op Armed (parked) until the player commits, then Done
    // (resume). Any other `49 03 <slot>` does the same in the context that
    // ran it (`CutsceneState::naming_owner`); a context with no naming
    // prompt pending falls back to the default Idle.
    // REF: FUN_801F03F0 (name-entry overlay) / op49_invoke_setup func_0x80020de0
    fn op49_state(&self) -> Op49State {
        Self::op49_state(self)
    }

    fn op49_arm(&mut self, sub_op: u8, _pc: usize, _field_90: u32) {
        // Retail's `sw s6,-0x4bb0(s0)`: the park holds the operand pointer,
        // whose first byte is this sub-op. Recording it is what lets
        // `World::menu_entry_context_kind` answer with anything other than
        // the two sub-ops the port could previously infer from its own
        // dedicated host paths.
        self.world.record_op49_park(sub_op);
    }

    fn op49_clear(&mut self) {
        Self::op49_clear(self)
    }

    fn op49_menu_request(&mut self, sub_op: u8, instr: &[u8]) {
        Self::op49_menu_request(self, sub_op, instr)
    }

    fn op49_invoke_setup(&mut self) {
        if self.world.cutscene.prologue_naming_pending
            && !self.world.cutscene.prologue_naming_armed
            && self.world.cutscene.naming_owner == Some(self.world.op49_park_owner())
            && !self.world.name_entry_active()
        {
            // The slot is the operand's byte after the sub-op (retail's
            // char-record pointer `_DAT_8007B450 + 1`): `00` = Vahn at the
            // opening, `01` = Noa in `cave01`.
            self.world.open_name_entry(self.world.cutscene.naming_slot);
            self.world.cutscene.prologue_naming_armed = true;
        }
    }

    fn stage_three_actor_talk(&mut self, actor_ids: [u8; 3], arg_word: u16, arg_byte: u8) {
        Self::stage_three_actor_talk(self, actor_ids, arg_word, arg_byte)
    }

    fn spawn_text_balloon(&mut self, text_buf: &[u8], script_id: u16) {
        // Field-VM op `4C E1` - spawn the single-line text balloon
        // (`FUN_8003C764`). Replacing the Option is the retail
        // predecessor-kill (the handler's first lines kill any live
        // sibling balloon). The X centering waits for a host-side font
        // measurement; the tick lives in `World::tick`
        // (`legaia_engine_dialog::text_balloon::TextBalloon::tick`).
        let _ = script_id;
        self.world.cutscene.text_balloon = Some(
            legaia_engine_dialog::text_balloon::TextBalloon::spawn(text_buf),
        );
    }

    fn open_dialog(
        &mut self,
        text_id: u16,
        inline: &[u8],
        world_x: u16,
        world_z: u16,
        depth_id: u8,
    ) {
        Self::open_dialog(self, text_id, inline, world_x, world_z, depth_id)
    }

    fn op4c_n5_sub4_dialog_advance(&mut self, _ctx: &mut FieldCtx) -> bool {
        Self::op4c_n5_sub4_dialog_advance(self, _ctx)
    }

    /// `[4C, 0x89, lo, hi]` - the pager's automatic-press countdown
    /// `_DAT_80073F00`, which a waiting box page counts down and then
    /// presses through ([`legaia_engine_dialog::dialog::OwnedDialogPanel::tick_at_auto`]).
    fn set_dialog_auto_press(&mut self, value: i16) {
        self.world.dialog.auto_press = value;
    }
}

#[cfg(test)]
mod window_format_tests {
    use crate::world::World;
    use crate::world::vm_hosts::{FieldHostImpl, field_step_routed};
    use legaia_engine_vm::field::FieldCtx;

    /// `izumi` P1[12]: `4C E0 00`, `4C E0 0C`, then the page. The box opens
    /// twelve rows tall with the page loaded whole, and the format is spent:
    /// the next box is the pager's three again.
    #[test]
    fn op_4c_e0_opens_one_twelve_row_instant_box() {
        let mut w = World::new();
        let mut ctx = FieldCtx::default();
        let mut host = FieldHostImpl { world: &mut w };
        field_step_routed(&mut host, &mut ctx, &[0x4C, 0xE0, 0x00], 0);
        field_step_routed(&mut host, &mut ctx, &[0x4C, 0xE0, 0x0C], 0);
        assert!(w.dialog.window_instant);
        assert_eq!(w.dialog.window_rows, 12);
        // Five one-glyph lines in one page.
        let mut bytes = Vec::new();
        for c in b"ABCDE" {
            bytes.extend_from_slice(&[0x1F, *c, 0x00]);
        }
        bytes.push(0x21);
        let bytes = std::sync::Arc::new(bytes);
        let mut panel =
            legaia_engine_dialog::dialog::OwnedDialogPanel::at_segment(bytes.clone(), 0);
        w.apply_dialog_window_format(&mut panel);
        assert_eq!(panel.box_rows(), Some(12));
        let mut auto = 0i16;
        panel.tick_at_auto(1, &mut auto);
        assert_eq!(panel.window().map(|win| win.rows.len()), Some(5));
        assert!(panel.window().is_some_and(|win| win.waiting()));
        let mut next = legaia_engine_dialog::dialog::OwnedDialogPanel::at_segment(bytes, 0);
        w.apply_dialog_window_format(&mut next);
        assert_eq!(next.box_rows(), Some(3), "a format lasts one box");
    }
}
