//! The whole dispatcher: one `step` per sub-mode over the tick's state.
//! Split out of `title_overlay.rs`.

use super::*;

impl TitleTickState {
    /// The state the title overlay's first tick sees on a **cold boot**.
    ///
    /// The entry word is raised, not the sub-mode: retail reaches sub-mode
    /// `0x10` by running `Init` with [`ENTRY_WORD_COLD_BOOT`] in
    /// [`ENTRY_WORD_ADDR`], which is what `init.pak` leaves there
    /// (`FUN_801CE9C0` at `0x801CEB84`). Hard-coding the mode instead
    /// skips the `0x11` `AttractDelay` leg the capture sees.
    pub fn cold_boot() -> Self {
        Self::with_entry_word(ENTRY_WORD_COLD_BOOT)
    }

    /// The same state with an explicit [`ENTRY_WORD_ADDR`] value - `0`
    /// reproduces the (retail-unreachable) `0x02` graph, `2` the return
    /// from the attract movie.
    pub fn with_entry_word(entry_word: u32) -> Self {
        Self {
            submode: TitleOverlaySubMode::Init as u8,
            prev_submode: TitleOverlaySubMode::Init as u8,
            arg1: 0,
            entry_word,
            menu_index: 0,
            linear_cursor: 0,
            cursor_x: 0,
            cursor_y: 0,
            row_counter: 0,
            countdown: COUNTDOWN_RESET_VALUE as i32,
            attract_delay: ATTRACT_DELAY_SEED,
            preroll: 0,
            fade: 0,
            panel_fade: 0,
            slider_x: 0x100,
            slider_dir: 0,
            op_kind: 0,
            save_timer: 0,
            check_timer: 0,
            launch_timer: 0,
            remap_gate: -1,
            cancel_target: None,
            fault_arm: false,
            cursor_kind: 0,
            rows: 0,
        }
    }

    /// The decoded sub-mode, or `None` when the selector is outside the
    /// dispatcher's `sltiu v0,s2,0x19` window (which routes to the tail).
    pub fn mode(&self) -> Option<TitleOverlaySubMode> {
        TitleOverlaySubMode::from_u8(self.submode)
    }

    /// One whole tick: the per-sub-mode handler followed by the shared
    /// epilogue, in retail's order.
    ///
    /// PORT: FUN_801DD35C
    pub fn step(&mut self, pad: TitleTickPad, card: TitleCardStatus) -> Vec<TitleTickEffect> {
        let mut fx = Vec::new();
        // Preamble: the mode-change edge the tick logs, then the per-frame
        // registers every handler re-seeds.
        self.prev_submode = self.submode;
        // The preamble's own register seeds, which a handler only changes
        // if it wants to: `li s3,0x14` at 0x801DD5BC (the default cancel
        // target is the main menu, not "no cancel"), `li s5,0x1` at
        // 0x801DD5E8 (the fault arm is ON unless a handler clears it),
        // `clear s6` at 0x801DD374 and `li s7,-0x1` at 0x801DD610.
        self.cancel_target = Some(0x14);
        self.fault_arm = true;
        self.cursor_kind = 0;
        self.rows = -1;
        let scalar = pad.frame_scalar.max(1) as i32;
        if self.handler(pad, card, scalar, &mut fx) {
            self.epilogue(pad, card, &mut fx);
        }
        fx
    }

    /// The dispatched half - one arm per JT entry. Returns whether the
    /// shared epilogue runs afterwards: two arms jump straight to the
    /// function's exit at `0x801E0274` instead of falling into it - the
    /// `AttractIdle` row-0 confirm (`0x801DDC44`) and the launcher's tail
    /// (`0x801DFB54`).
    pub(super) fn handler(
        &mut self,
        pad: TitleTickPad,
        card: TitleCardStatus,
        scalar: i32,
        fx: &mut Vec<TitleTickEffect>,
    ) -> bool {
        let any = pad.edge & PADMASK_ANY_FACE_OR_L != 0;
        let confirm = pad.edge & PADMASK_CONFIRM_L1_CROSS != 0;
        let cancel = pad.edge & PADMASK_CANCEL_L2_CIRCLE != 0;
        match self.submode {
            // 0x00 Init - 0x801DD820.
            0x00 => {
                self.fault_arm = false;
                self.slider_x = 0x100;
                self.fade = 0x1000;
                self.slider_dir = 0;
                self.linear_cursor = 0;
                self.remap_gate = -1;
                self.submode = 0x02;
                self.countdown = COUNTDOWN_RESET_VALUE as i32;
                let mut arg1 = self.arg1;
                if self.entry_word != 0 {
                    self.submode = 0x11;
                    if self.entry_word == ENTRY_WORD_COLD_BOOT {
                        arg1 = 0;
                        fx.push(TitleTickEffect::LoadTitleAssets);
                    }
                }
                if arg1 == 1 {
                    self.menu_index = 0;
                    self.linear_cursor = 0;
                    self.submode = 0x14;
                } else if arg1 == 2 {
                    self.menu_index = 1;
                    self.linear_cursor = 0;
                    self.submode = 0x14;
                }
            }
            // 0x01 Idle - the JT entry is the epilogue itself.
            0x01 => {}
            // 0x02 - the (retail-unreachable) two-row text menu, 0x801DDDFC.
            0x02 => {
                self.fault_arm = false;
                self.remap_gate = -1;
                self.fade -= 0x100 * scalar;
                if self.fade > 0 {
                    self.cancel_target = None;
                    return true;
                }
                self.fade = 0;
                self.slider_x = 0x100;
                if self.arg1 != 0 {
                    self.submode = 0x16;
                    return true;
                }
                self.rows = 2;
                self.cursor_kind = 1;
                self.slider_dir = 0;
                self.cancel_target = Some(0x16);
                if confirm {
                    self.menu_index = self.linear_cursor as u32;
                    self.linear_cursor = 0;
                    self.submode = 0x14;
                }
            }
            // 0x03 - SAVE in progress, 0x801DF5BC.
            0x03 => {
                self.cancel_target = None;
                self.save_timer += scalar;
                if self.save_timer >= 0x4B1 || card.error != 0 || card.fault != 0 {
                    self.submode = 0x17;
                }
            }
            // 0x04 - block transfer, 0x801DF33C.
            0x04 => {
                self.cancel_target = None;
                if card.fault > 0 {
                    self.submode = 0x13;
                } else {
                    self.fault_arm = false;
                    if card.ready != 0 && card.draw_done == 0x1000 {
                        self.submode = 0x05;
                    }
                }
            }
            // 0x05 - post-load verify, 0x801DF82C.
            0x05 => {
                self.cancel_target = None;
                self.fault_arm = false;
                if card.verify_ok {
                    self.submode = 0x16;
                    self.op_kind = OP_KIND_LOAD;
                } else {
                    self.submode = 0x13;
                }
            }
            // 0x06 - the NEW GAME launcher, 0x801DFB5C.
            0x06 => {
                if self.entry_word != 0 {
                    fx.push(TitleTickEffect::LaunchGame { from_load: false });
                    self.entry_word = 0;
                }
                self.submode = 0x00;
                // `j 0x801E0274` at 0x801DFB54: the launcher returns 1 and
                // never reaches the epilogue.
                return false;
            }
            // 0x07 - card-op staging, 0x801DE134.
            0x07 => {
                self.cancel_target = None;
                if card.busy != 0 || card.error != 0 {
                    self.fault_arm = true;
                    return true;
                }
                self.fault_arm = false;
                self.slider_dir = 1;
                self.linear_cursor = 0;
                self.check_timer = 0;
                self.submode = 0x15;
            }
            // 0x08 - the card-fault message, 0x801DE4A4.
            0x08 => {
                self.cancel_target = None;
                if card.fault == 0 {
                    self.submode = 0x07;
                }
                // Sequential, not exclusive: the any-button arm at
                // 0x801DE624 runs after the fault-cleared arm above.
                if any {
                    self.fault_arm = false;
                    self.submode = 0x14;
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                }
            }
            // 0x09 - one-frame scan setup, 0x801DE638.
            0x09 => {
                self.cancel_target = None;
                self.fault_arm = false;
                self.submode = 0x0A;
            }
            // 0x0A - the block scan, 0x801DE798.
            0x0A => {
                self.cancel_target = None;
                if card.busy != 0
                    || card.fault != 0
                    || card.busy_gate != 0
                    || card.block_count != 0
                    || card.error != 0
                    || card.removed != 0
                {
                    self.submode = 0x07;
                    return true;
                }
                // (`state[-0xea4] = 1` here is the one-shot fade-direction
                // request the tick's preamble consumes; it moves no mode.)
                if self.menu_index != 0 && card.scan_total == 0 {
                    self.submode = 0x0C;
                }
                // Falls through: the scan-complete arm at 0x801DE934 runs
                // after the 0x0C arm, not instead of it.
                if card.scan_done == card.scan_total {
                    self.submode = 0x0B;
                }
            }
            // 0x0B - the 5x3 slot grid, 0x801DEA5C.
            0x0B => {
                self.panel_fade -= 0x100 * scalar;
                if self.panel_fade > 0 {
                    self.cancel_target = None;
                    return true;
                }
                self.panel_fade = 0;
                self.cursor_kind = 2;
                self.slider_dir = 2;
                if confirm && card.slot_ok {
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                    if self.menu_index == 0 {
                        self.linear_cursor = 0;
                        self.submode = 0x0E;
                    }
                }
                if card.removed != 0 {
                    self.submode = 0x07;
                }
            }
            // 0x0C / 0x0D - the two "no data" messages, 0x801DE680 / 0x801DE728.
            0x0C | 0x0D => {
                self.cancel_target = Some(0x14);
                if any {
                    self.submode = 0x14;
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                }
            }
            // 0x0E - the slot confirm prompt, 0x801DEC40.
            0x0E => {
                self.slider_dir = 0;
                self.panel_fade_up(scalar);
                if self.panel_fade < 0x1000 {
                    self.cancel_target = None;
                    return true;
                }
                self.panel_fade = 0x1000;
                self.cancel_target = None;
                if cancel {
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CANCEL));
                    self.submode = 0x0B;
                    self.slider_dir = 2;
                    return true;
                }
                self.slider_x = -0x16;
                self.rows = 2;
                self.cursor_kind = 1;
                if !confirm {
                    return true;
                }
                if self.linear_cursor != 1 {
                    self.submode = 0x0B;
                    self.slider_dir = 2;
                } else if self.menu_index != 0 {
                    // The LOAD route jumps into 0x13's body at 0x801DF47C.
                    self.submode = 0x04;
                } else {
                    self.submode = 0x03;
                }
            }
            // 0x0F - the format / overwrite prompt, 0x801DEE0C.
            0x0F => {
                self.rows = 2;
                self.cursor_kind = 1;
                if confirm {
                    self.submode = if self.linear_cursor == 1 { 0x12 } else { 0x14 };
                }
            }
            // 0x10 AttractIdle - 0x801DDB0C.
            0x10 => {
                self.cancel_target = None;
                self.fault_arm = false;
                self.remap_gate = -1;
                self.fade = 0;
                self.preroll -= 0x80 * scalar;
                if self.preroll >= 0 {
                    return true;
                }
                self.preroll = 0;
                self.cursor_kind = 1;
                if self.countdown >= ATTRACT_INPUT_FREEZE_BELOW {
                    if pad.nav & PADMASK_CURSOR_NEXT != 0 {
                        self.row_counter = self.row_counter.wrapping_add(1);
                        fx.push(TitleTickEffect::Sfx(TITLE_SFX_CURSOR_MOVE));
                    }
                    if pad.nav & PADMASK_CURSOR_PREV != 0 {
                        self.row_counter = self.row_counter.wrapping_sub(1);
                        fx.push(TitleTickEffect::Sfx(TITLE_SFX_CURSOR_MOVE));
                    }
                    self.row_counter &= 1;
                    if pad.edge & PADMASK_START_L1_CROSS != 0 {
                        fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                        if self.row_counter == TITLE_ROW_NEW_GAME as i32 {
                            // `j 0x801E0274` at 0x801DDC44 - straight to the
                            // function exit, past the shared epilogue.
                            self.submode = 0x16;
                            return false;
                        }
                        self.submode = 0x18;
                        self.menu_index = TITLE_ROW_CONTINUE as u32;
                        self.preroll = 0;
                        self.linear_cursor = 0;
                    }
                    if pad.held != 0 {
                        self.countdown = COUNTDOWN_RESET_VALUE as i32;
                    }
                }
                self.countdown -= scalar;
                if self.countdown < 0 {
                    fx.push(TitleTickEffect::FireAttract {
                        fmv_id: ATTRACT_FMV_ID,
                    });
                }
            }
            // 0x11 AttractDelay - 0x801DDA90.
            0x11 => {
                self.cancel_target = None;
                self.fault_arm = false;
                self.remap_gate = -1;
                self.fade = 0;
                if self.attract_delay > 0 {
                    self.attract_delay -= 8 * scalar;
                } else {
                    self.submode = 0x10;
                    self.countdown = COUNTDOWN_RESET_VALUE as i32;
                }
                if self.attract_delay < 0 {
                    self.attract_delay = 0;
                }
            }
            // 0x12 - the card-op sub-dispatcher, 0x801DEF38.
            0x12 => {
                self.cancel_target = None;
                self.fault_arm = false;
                if card.op_phase == 0 && any {
                    self.submode = 0x14;
                } else if card.op_phase == 0x0D && card.op_timer - scalar < 0 {
                    self.submode = 0x07;
                }
            }
            // 0x13 - the card-op result screen, 0x801DF404.
            0x13 => {
                self.cancel_target = None;
                self.fault_arm = false;
                if card.busy == 1 && card.retry_latch == 0 && card.retries + 1 < 5 {
                    self.submode = 0x04;
                } else if any {
                    self.submode = 0x14;
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                }
            }
            // 0x14 - the second two-row menu, 0x801DDF30.
            0x14 => {
                self.fault_arm = false;
                self.preroll = 0x1000;
                self.remap_gate = -1;
                self.fade -= 0x100 * scalar;
                if self.fade > 0 {
                    self.cancel_target = None;
                    return true;
                }
                if cancel {
                    self.remap_gate = 0;
                }
                self.fade = 0;
                self.cancel_target = Some(if self.arg1 != 0 {
                    0x16
                } else if self.entry_word != 0 {
                    0x10
                } else {
                    0x02
                });
                self.cursor_kind = 1;
                self.slider_x = 0x100;
                self.slider_dir = 0;
                self.rows = 2;
                if pad.nav & PADMASK_CURSOR_PREV != 0 {
                    self.row_counter -= 1;
                }
                if pad.nav & PADMASK_CURSOR_NEXT != 0 {
                    self.row_counter += 1;
                }
                self.row_counter &= 1;
                if confirm {
                    self.submode = 0x07;
                    self.menu_index = self.row_counter as u32;
                }
            }
            // 0x15 - the card check, 0x801DE260.
            0x15 => {
                self.cancel_target = None;
                self.fault_arm = true;
                self.check_timer += scalar;
                if self.check_timer < 0x259 {
                    if any {
                        // Falls into 0x0F's body at 0x801DEF2C.
                        self.submode = 0x14;
                    }
                    return true;
                }
                self.check_timer = 0x258;
                if card.error != 0 {
                    // Falls into 0x0A's body at 0x801DE838.
                    self.submode = 0x07;
                    return true;
                }
                if card.grace != 0 {
                    return true;
                }
                if card.slot_kind == 3 && card.scan_request == 0 {
                    self.submode = 0x09;
                    if card.scan_total == 0 {
                        if self.menu_index != 0 {
                            self.submode = 0x0C;
                        } else if card.has_blocks == 0 {
                            self.submode = 0x0D;
                        }
                    }
                }
                if card.fault > 0 {
                    self.submode = 0x08;
                }
                if card.block_count >= 2 {
                    self.submode = if self.menu_index == 0 { 0x0F } else { 0x0C };
                }
            }
            // 0x16 - the launch white-out, 0x801DF8D0.
            0x16 => {
                self.cancel_target = None;
                self.fault_arm = false;
                self.launch_timer += scalar;
                // The `+= 0x5A` skip is the delay slot of the `op_kind`
                // test at 0x801DF90C, so it lands whatever the banner is;
                // only the cue is gated.
                if any && self.launch_timer < 0x5A {
                    self.launch_timer += 0x5A;
                    if self.op_kind != 0 {
                        fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                    }
                }
                if self.launch_timer >= 0x5B {
                    self.fade += 0x100 * scalar;
                }
                if self.fade >= 0x1200 {
                    self.fade = 0x1200;
                    self.submode = 0x06;
                    if self.op_kind == OP_KIND_LOAD {
                        fx.push(TitleTickEffect::LaunchGame { from_load: true });
                        self.entry_word = 0;
                    }
                }
            }
            // 0x17 - the save result screen, 0x801DF6F4.
            0x17 => {
                self.fault_arm = false;
                if card.result == 1 && card.retry_latch == 0 && card.retries + 1 < 5 {
                    self.submode = 0x03;
                    self.cancel_target = None;
                    return true;
                }
                self.cancel_target = Some(0x14);
                self.op_kind = 0;
                if any {
                    self.submode = 0x14;
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                }
            }
            // 0x18 - the CONTINUE fade-in, 0x801DDD94.
            0x18 => {
                self.cancel_target = None;
                self.fault_arm = false;
                self.remap_gate = -1;
                self.preroll += 0x80 * scalar;
                if self.preroll >= 0x1001 {
                    self.preroll = 0x1000;
                    self.submode = 0x14;
                }
            }
            // Out of range: `sltiu v0,s2,0x19` sends it to the epilogue.
            _ => {}
        }
        true
    }

    /// The shared epilogue at [`SUBMODE_BODY_PC`], which runs after every
    /// handler (and *is* the handler for sub-mode `0x01` and for any
    /// out-of-range selector).
    pub(super) fn epilogue(
        &mut self,
        pad: TitleTickPad,
        card: TitleCardStatus,
        fx: &mut Vec<TitleTickEffect>,
    ) {
        let scalar = pad.frame_scalar.max(1) as i32;
        // The panel slider converges on 0x2C from whichever side it is on.
        match self.slider_dir {
            1 => {
                self.slider_x -= 8 * scalar;
                if self.slider_x < 0x2C {
                    self.slider_x = 0x2C;
                }
            }
            2 => {
                self.slider_x += 8 * scalar;
                if self.slider_x >= 0x2D {
                    self.slider_x = 0x2C;
                }
            }
            _ => {}
        }
        // `submode = FUN_801E1114(submode)` - the callee returns its
        // argument, so the store is an identity and only the model pass
        // it performs is a side effect.
        if self.remap_gate >= 0 {
            // identity
        }
        if self.fault_arm && card.fault > 0 && card.grace == 0 {
            self.submode = 0x08;
        }
        if self.entry_word != 0 {
            if self.submode == 0x02 {
                self.submode = 0x10;
            }
            if self.cancel_target == Some(0x02) {
                self.cancel_target = Some(0x10);
            }
        }
        if self.arg1 != 0 {
            if self.submode == 0x02 {
                self.submode = 0x16;
            }
            return;
        }
        if card.op_status == 0 {
            if self.submode == 0x03 {
                self.submode = if card.result != 0 { 0x17 } else { 0x16 };
            }
            if self.submode == 0x04 && card.busy != 0 {
                self.submode = 0x13;
            }
        }
        if self.submode != 0x03
            && self.submode != 0x04
            && pad.edge & PADMASK_CANCEL_L2_CIRCLE != 0
            && let Some(target) = self.cancel_target
        {
            fx.push(TitleTickEffect::Sfx(TITLE_SFX_CANCEL));
            self.submode = target;
        }
        if self.cursor_kind == 1 {
            if pad.edge & PADMASK_CONFIRM_L1_CROSS != 0 {
                fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
            }
            if pad.held & PADMASK_ANY_FACE_OR_L == 0 {
                if pad.nav & PADMASK_CURSOR_NEXT != 0 {
                    self.linear_cursor += 1;
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CURSOR_MOVE));
                }
                if pad.nav & PADMASK_CURSOR_PREV != 0 {
                    self.linear_cursor -= 1;
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CURSOR_MOVE));
                }
            }
            let last = self.rows - 1;
            if self.linear_cursor > last {
                self.linear_cursor = 0;
            }
            if self.linear_cursor < 0 {
                self.linear_cursor = last;
            }
        } else if self.cursor_kind == 2 && pad.held & PADMASK_ANY_FACE_OR_L == 0 {
            if pad.nav & PADMASK_CURSOR_NEXT != 0 {
                self.cursor_y += 1;
                fx.push(TitleTickEffect::Sfx(TITLE_SFX_CURSOR_MOVE));
            }
            if pad.nav & PADMASK_CURSOR_PREV != 0 {
                self.cursor_y -= 1;
                fx.push(TitleTickEffect::Sfx(TITLE_SFX_CURSOR_MOVE));
            }
            if pad.nav & PADMASK_GRID_LEFT != 0 {
                self.cursor_x -= 1;
                fx.push(TitleTickEffect::Sfx(TITLE_SFX_CURSOR_MOVE));
            }
            if pad.nav & PADMASK_GRID_RIGHT != 0 {
                self.cursor_x += 1;
                fx.push(TitleTickEffect::Sfx(TITLE_SFX_CURSOR_MOVE));
            }
            if self.cursor_x < 0 {
                self.cursor_x = GRID_COLUMNS - 1;
            }
            if self.cursor_x >= GRID_COLUMNS {
                self.cursor_x = 0;
            }
            if self.cursor_y < 0 {
                self.cursor_y = GRID_ROWS - 1;
            }
            if self.cursor_y >= GRID_ROWS {
                self.cursor_y = 0;
            }
        }
    }

    pub(super) fn panel_fade_up(&mut self, scalar: i32) {
        self.panel_fade += 0x100 * scalar;
    }
}

impl TransitionTarget {
    /// The literal sub-modes this store can leave in the selector. Empty
    /// for the register-sourced and identity stores, whose target is a
    /// per-frame value rather than a constant.
    pub fn literals(self) -> Vec<u8> {
        match self {
            TransitionTarget::Mode(m) => vec![m],
            TransitionTarget::OneOf(v) => v.to_vec(),
            TransitionTarget::Register(_) | TransitionTarget::Identity => Vec::new(),
        }
    }
}

/// The two stores in [`STATE_204_WRITES`] that never survive the tick
/// that made them, and so carry no edge in the retail mode graph:
///
/// - `0x801DD920` writes `0x02` unconditionally in `Init`, and while the
///   entry word `_DAT_8007BB00` is up (which `init.pak` guarantees) the
///   sentinel arm at `0x801DD97C` replaces it in the same handler and the
///   shared epilogue would replace it again at `0x801DFEF8`;
/// - `0x801DDC38` writes `0` on the `AttractIdle` row-0 confirm and the
///   very next instruction (`0x801DDC3C`) writes `0x16` over it.
pub const OVERWRITTEN_STORES: &[u32] = &[0x801D_D920, 0x801D_DC38];

/// Four of the six shared-epilogue stores are guarded on the sub-mode the
/// handler left in the selector, so they are edges out of exactly that
/// mode rather than out of every mode. The remaining two - the identity
/// write-back at `0x801DFE88` and the fault arm at `0x801DFED0` - are not
/// (the fault arm only needs the per-frame `s5`), and carry no row here.
pub const EPILOGUE_GUARD_MODES: &[(u32, u8)] = &[
    (0x801D_FEF8, 0x02),
    (0x801D_FF28, 0x02),
    (0x801D_FF74, 0x03),
    (0x801D_FF9C, 0x04),
];

/// Every sub-mode a retail cold boot can put in the selector, walked from
/// [`TitleOverlaySubMode::Init`] over [`STATE_204_WRITES`]. Indexed by
/// mode byte.
///
/// [`OVERWRITTEN_STORES`] contribute no edge; the shared-epilogue rows
/// contribute one out of the mode [`EPILOGUE_GUARD_MODES`] names, or out
/// of every reached mode when they name none.
pub fn cold_boot_reachable_modes() -> [bool; SUBMODE_JT_ENTRY_COUNT] {
    let mut seen = [false; SUBMODE_JT_ENTRY_COUNT];
    seen[TitleOverlaySubMode::Init as usize] = true;
    let mut changed = true;
    while changed {
        changed = false;
        for w in STATE_204_WRITES {
            if OVERWRITTEN_STORES.contains(&w.pc) {
                continue;
            }
            let live = if w.from == SUBMODE_TAIL_SOURCE {
                match EPILOGUE_GUARD_MODES.iter().find(|(pc, _)| *pc == w.pc) {
                    Some((_, m)) => seen[*m as usize],
                    // Unguarded epilogue store: reachable once anything is.
                    None => true,
                }
            } else {
                seen[w.from as usize] || w.also_from.iter().any(|m| seen[*m as usize])
            };
            if !live {
                continue;
            }
            for t in w.target.literals() {
                if (t as usize) < SUBMODE_JT_ENTRY_COUNT && !seen[t as usize] {
                    seen[t as usize] = true;
                    changed = true;
                }
            }
        }
    }
    seen
}

/// The fmv id retail's attract arm hardcodes: `sh zero,-0x4588(v0)` at
/// `0x801DDCE8` zeroes `_DAT_8007BA78` before the mode write, so the
/// attract always plays `fmv_id 0` (`MV1.STR`, the intro).
pub const ATTRACT_FMV_ID: i16 = 0;

/// D-pad bit that steps the 5x3 grid cursor **left** (`andi v0,a2,0x8000`
/// at `0x801E0124`).
pub const PADMASK_GRID_LEFT: u16 = 0x8000;

/// D-pad bit that steps it **right** (`andi v0,a3,0x2000` at `0x801E0148`).
pub const PADMASK_GRID_RIGHT: u16 = 0x2000;

/// Columns in the slot grid the epilogue clamps `state[+0x1F4]` to
/// (`slti v0,v0,0x5` at `0x801E017C`).
pub const GRID_COLUMNS: i32 = 5;

/// Rows in that grid (`slti v0,v0,0x3` at `0x801E01B0`).
pub const GRID_ROWS: i32 = 3;
