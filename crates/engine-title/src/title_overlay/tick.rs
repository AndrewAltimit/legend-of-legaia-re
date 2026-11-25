//! The whole dispatcher: one `step` per sub-mode over the tick's state.

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
            menu_row: 0,
            main_row: 0,
            probed_row: 0,
            slot_index: 0,
            alpha_a: 0,
            alpha_b: 0,
            alpha_c: 0,
            alpha_gate: 0,
            fault_flash: 0,
            sweep_dir: 0,
            sweep: 0,
            frame_counter: 0,
            cancel_arm: 0,
            probed: 0,
            fault_seen: 0,
            fault_timer: 0,
            music_level: 0,
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

    /// One whole tick: the preamble, the per-sub-mode handler and the
    /// shared epilogue, in retail's order. `card` is the card driver's
    /// words, which the tick reads and writes (see [`TitleCardStatus`]).
    ///
    /// The once-per-overlay setup pass behind `state[+0x230]` and every
    /// draw call are the host's, not the step's.
    ///
    /// PORT: FUN_801DD35C
    pub fn step(&mut self, pad: TitleTickPad, card: &mut TitleCardStatus) -> Vec<TitleTickEffect> {
        let mut fx = Vec::new();
        let scalar = i32::from(pad.frame_scalar);
        // Preamble, 0x801DD410..0x801DD7F8: the frame counter, the sweep
        // the previous frame's handler pointed (`state[-0xea4]`: 0 down,
        // 1 up, anything else holds), clamped to [0, 0x1000].
        self.frame_counter = self.frame_counter.wrapping_add(1);
        match self.sweep_dir {
            0 => self.sweep -= scalar << 8,
            1 => self.sweep += scalar << 8,
            _ => {}
        }
        self.sweep = self.sweep.clamp(0, 0x1000);
        self.prev_submode = self.submode;
        self.sweep_dir = 0;
        // The per-frame registers: `li s3,0x14` at 0x801DD5BC (the default
        // cancel target is the main menu), `li s5,0x1` at 0x801DD5E8 (the
        // fault arm is on unless a handler clears it), `clear s6` at
        // 0x801DD374 and `li s7,-0x1` at 0x801DD610. `fp` is the second
        // argument, which `Init` may clear for this frame.
        self.cancel_target = Some(0x14);
        self.fault_arm = true;
        self.cursor_kind = 0;
        self.rows = -1;
        let mut fp = self.arg1;
        if self.handler(pad, card, scalar, &mut fp, &mut fx) {
            self.epilogue(pad, card, fp, &mut fx);
        }
        fx
    }

    /// `x + 5y` back to the grid cursor, as `Init` and `0x14` derive it
    /// (`mult` by `0x66666667`, C division and remainder).
    fn seat_grid_cursor(&mut self) {
        self.cursor_x = self.slot_index % 5;
        self.cursor_y = self.slot_index / 5;
    }

    /// The dispatched half - one arm per JT entry. Returns whether the
    /// shared epilogue runs afterwards: three arms leave for the function's
    /// exit at `0x801E0274` instead - the `AttractIdle` row-0 confirm
    /// (`0x801DDC44`), the load route out of `0x16` (`0x801DFB54`) and the
    /// launcher `0x06` (`0x801DFC34`).
    pub(super) fn handler(
        &mut self,
        pad: TitleTickPad,
        card: &mut TitleCardStatus,
        scalar: i32,
        fp: &mut u32,
        fx: &mut Vec<TitleTickEffect>,
    ) -> bool {
        let any = pad.edge & PADMASK_ANY_FACE_OR_L != 0;
        let confirm = pad.edge & PADMASK_CONFIRM_L1_CROSS != 0;
        let cancel = pad.edge & PADMASK_CANCEL_L2_CIRCLE != 0;
        let reset =
            |fx: &mut Vec<TitleTickEffect>, arg: u32| fx.push(TitleTickEffect::CardReset { arg });
        match self.submode {
            // 0x00 Init - 0x801DD820.
            0x00 => {
                self.fault_arm = false;
                card.fault = 1;
                self.probed = 0;
                self.sweep_dir = 0;
                card.draw_done = 0;
                self.panel_fade = 0;
                self.alpha_b = 0;
                self.alpha_gate = 0;
                self.alpha_c = 0;
                self.cancel_arm = 0;
                self.slider_x = 0x100;
                self.slider_dir = 0;
                self.fade = 0x1000;
                self.main_row = self.menu_row;
                reset(fx, 0);
                self.remap_gate = -1;
                self.op_kind = 0;
                self.launch_timer = 0;
                self.linear_cursor = 0;
                self.submode = 0x02;
                self.countdown = COUNTDOWN_RESET_VALUE as i32;
                self.probed_row = self.menu_row;
                self.seat_grid_cursor();
                if self.entry_word != 0 {
                    self.submode = 0x11;
                    // `move fp,s5` sits in the delay slot of the cold-boot
                    // test at 0x801DD980, so any raised word clears it.
                    *fp = 0;
                    if self.entry_word == ENTRY_WORD_COLD_BOOT {
                        fx.push(TitleTickEffect::LoadTitleAssets);
                    }
                }
                if *fp == 1 {
                    self.menu_index = 0;
                    self.linear_cursor = 0;
                    self.submode = 0x14;
                }
                if *fp == 2 {
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
                self.fade -= scalar << 8;
                if self.fade > 0 {
                    self.cancel_target = None;
                    return true;
                }
                self.fade = 0;
                self.slider_x = 0x100;
                if *fp != 0 {
                    self.submode = 0x16;
                    return true;
                }
                self.rows = 2;
                self.cursor_kind = 1;
                self.slider_dir = 0;
                self.cancel_target = Some(0x16);
                if confirm {
                    card.scan_request = 1;
                    self.submode = 0x14;
                    self.menu_index = self.linear_cursor as u32;
                    self.linear_cursor = 0;
                }
            }
            // 0x03 - SAVE in progress, 0x801DF5BC. The fault arm stays on.
            0x03 => {
                self.cancel_target = None;
                self.save_timer += scalar;
                if self.save_timer >= 0x4B1 {
                    card.op_status = 0;
                    self.submode = 0x17;
                    reset(fx, 0);
                    card.result = 0;
                    card.retry_latch = 1;
                } else if card.error != 0 {
                    card.op_status = 0;
                    self.submode = 0x17;
                    if card.result == 0 {
                        reset(fx, 0);
                        card.retry_latch = 1;
                    }
                } else if card.fault != 0 {
                    card.op_status = 0;
                    self.submode = 0x17;
                    reset(fx, 0);
                    card.result = 0;
                    card.retry_latch = 1;
                } else {
                    self.op_kind = 2;
                }
            }
            // 0x04 - block transfer, 0x801DF33C.
            0x04 => {
                self.cancel_target = None;
                if card.fault > 0 {
                    self.submode = 0x13;
                    if card.busy == 0 {
                        reset(fx, 0);
                        card.retry_latch = 1;
                    }
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
                    card.busy = 2;
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
                // `j 0x801E0274` at 0x801DFC34: the launcher returns 1 and
                // never reaches the epilogue.
                return false;
            }
            // 0x07 - card-op staging, 0x801DE134.
            0x07 => {
                self.cancel_target = None;
                if card.busy != 0 || card.error != 0 {
                    reset(fx, 0);
                    return true;
                }
                self.fault_arm = false;
                card.draw_done = 0;
                card.grace = 0x78;
                self.remap_gate = self.menu_row;
                if self.menu_row != self.probed_row {
                    // `move a0,s5` with `s5` already cleared at 0x801DE184.
                    reset(fx, 0);
                    self.probed_row = self.menu_row;
                    card.grace = 0x78;
                }
                if self.probed == 0 {
                    card.grace += 0x96;
                }
                if self.fault_seen != 0 {
                    card.grace += 0xB4;
                }
                self.fault_seen = 0;
                self.slider_dir = 1;
                self.linear_cursor = 0;
                self.alpha_gate = 1;
                self.alpha_c = 1;
                self.probed = 1;
                self.check_timer = 0;
                self.submode = 0x15;
            }
            // 0x08 - the card-fault message, 0x801DE4A4. The fault arm stays
            // on until a button clears it.
            0x08 => {
                self.cancel_target = None;
                card.op_status = 0;
                self.fault_seen = 1;
                self.alpha_gate = 0;
                self.cancel_arm = 1;
                self.probed = 0;
                self.probed_row = -1;
                self.fault_timer += scalar;
                self.alpha_c -= scalar << 8;
                if self.alpha_c < 0 {
                    self.alpha_c = 0;
                }
                self.fault_flash -= scalar;
                if self.fault_flash < 0 {
                    self.fault_flash = -1;
                    self.panel_fade -= scalar << 8;
                }
                if self.panel_fade < 0 {
                    self.panel_fade = 0;
                }
                card.has_blocks = 0;
                if card.fault == 0 {
                    card.grace = 0x78;
                    self.submode = 0x07;
                    self.cancel_arm = 0;
                    self.fault_timer = 0;
                }
                if self.fault_timer >= 0xB5 {
                    reset(fx, 0);
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
                self.sweep_dir = 1;
                self.submode = 0x0A;
                card.scan_done = 0;
            }
            // 0x0A - the block scan, 0x801DE798. The fault arm stays on.
            0x0A => {
                self.cancel_target = None;
                if card.busy != 0
                    || card.fault != 0
                    || card.busy_gate != 0
                    || card.block_count != 0
                    || card.error != 0
                    || card.removed != 0
                {
                    card.busy_gate = 0;
                    reset(fx, 0);
                    self.submode = 0x07;
                    return true;
                }
                self.sweep_dir = 1;
                if self.menu_index != 0 && card.scan_total == 0 {
                    self.submode = 0x0C;
                }
                // Falls through: the scan-complete arm at 0x801DE934 runs
                // after the 0x0C arm, not instead of it. The block-by-block
                // walk behind it (`0x801DE958..0x801DEA58`) is the card
                // backend's; the port takes its progress as `scan_done`.
                if card.scan_done == card.scan_total {
                    self.submode = 0x0B;
                    self.sweep_dir = 0;
                }
            }
            // 0x0B - the 5x3 slot grid, 0x801DEA5C. The fault arm stays on.
            0x0B => {
                self.panel_fade -= scalar << 8;
                if self.panel_fade > 0 {
                    self.cancel_target = None;
                    return true;
                }
                self.cursor_kind = 2;
                self.panel_fade = 0;
                let cell = self.cursor_x + 5 * self.cursor_y;
                let mut picked = -1;
                if confirm {
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                    // A Save takes a readable save or a free block (`1` / `3`),
                    // a Load only a readable save (`0x801DEACC..0x801DEB00`).
                    let st = card.slot_status;
                    if st == 1 || (self.menu_index == 0 && st == 3) {
                        picked = cell;
                    }
                }
                self.slider_dir = 2;
                if picked == 0x80 {
                    self.submode = 0x14;
                    self.linear_cursor = 0;
                }
                if (picked as u32) < 0xF {
                    if self.menu_index != 0 {
                        card.ready = 0;
                    }
                    self.rows = 0;
                    self.linear_cursor = 0;
                    self.submode = 0x0E;
                }
                if card.removed != 0 {
                    card.removed = 0;
                    reset(fx, 0);
                    self.submode = 0x07;
                }
            }
            // 0x0C / 0x0D - the two "no data" messages, 0x801DE680 / 0x801DE728.
            // Both keep the fault arm on; `0x0D` also drops the removal flag.
            0x0C | 0x0D => {
                let mode = self.submode;
                self.cancel_target = Some(0x14);
                if any {
                    self.submode = 0x14;
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                }
                if mode == 0x0D {
                    card.removed = 0;
                }
            }
            // 0x0E - the slot confirm prompt, 0x801DEC40. The fault arm
            // stays on.
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
                    return true;
                }
                card.draw_done = 0;
                card.retries = 0;
                self.slot_index = self.cursor_x + 5 * self.cursor_y;
                if self.menu_index != 0 {
                    // The LOAD route jumps into 0x13's body at 0x801DF47C.
                    card.ready = 0;
                    card.op_status = 2;
                    self.submode = 0x04;
                } else {
                    card.op_status = 1;
                    self.submode = 0x03;
                }
            }
            // 0x0F - the format / overwrite prompt, 0x801DEE0C. The fault
            // arm stays on.
            0x0F => {
                self.rows = 2;
                self.alpha_c -= scalar << 8;
                if self.alpha_c < 0 {
                    self.alpha_c = 0;
                }
                self.cursor_kind = 1;
                card.op_result = 0;
                card.op_phase = 0;
                card.op_attempts = 0;
                if confirm {
                    self.submode = if self.linear_cursor == 1 { 0x12 } else { 0x14 };
                }
            }
            // 0x10 AttractIdle - 0x801DDB0C.
            0x10 => {
                self.cancel_target = None;
                self.fault_arm = false;
                self.rows = 2;
                self.panel_fade = 0;
                self.remap_gate = -1;
                self.fade = 0;
                self.preroll -= scalar << 7;
                if self.preroll >= 0 {
                    return true;
                }
                self.cursor_kind = 1;
                self.preroll = 0;
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
                } else if self.entry_word != 0 {
                    self.music_level = self.countdown << 4;
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
                self.alpha_gate = 0;
                self.fade = 0;
                if self.attract_delay > 0 {
                    self.attract_delay -= scalar << 3;
                } else {
                    self.submode = 0x10;
                    self.countdown = COUNTDOWN_RESET_VALUE as i32;
                }
                if self.attract_delay < 0 {
                    self.attract_delay = 0;
                }
            }
            // 0x12 - the format operation, 0x801DEF38.
            0x12 => {
                self.cancel_target = None;
                self.fault_arm = false;
                self.alpha_gate = 0;
                self.alpha_c -= scalar << 8;
                if self.alpha_c < 0 {
                    self.alpha_c = 0;
                }
                if card.op_result != 0 && card.op_request == 0 && any {
                    self.submode = 0x14;
                }
                self.format_phase(pad, card, scalar, fx);
            }
            // 0x13 - the card-op result screen, 0x801DF404.
            0x13 => {
                self.cancel_target = None;
                self.fault_arm = false;
                card.op_status = 0;
                if card.busy == 1 && card.retry_latch == 0 {
                    card.busy = 0;
                    card.retries += 1;
                    reset(fx, 1);
                    card.fault = 0;
                    if card.retries < 5 {
                        card.ready = 0;
                        card.op_status = 2;
                        self.submode = 0x04;
                        return true;
                    }
                    card.retry_latch = 2;
                }
                if any {
                    reset(fx, 0);
                    self.submode = 0x14;
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                    self.probed_row = -1;
                    self.alpha_gate = 0;
                    card.retry_latch = 0;
                    self.probed = 0;
                }
            }
            // 0x14 - the main menu, 0x801DDF30.
            0x14 => {
                self.fault_arm = false;
                self.op_kind = 0;
                self.preroll = 0x1000;
                self.panel_fade = 0;
                card.draw_done = 0;
                self.remap_gate = -1;
                self.fade -= scalar << 8;
                self.seat_grid_cursor();
                if self.fade > 0 {
                    self.cancel_target = None;
                    return true;
                }
                if cancel && self.cancel_arm != 0 {
                    self.remap_gate = 0;
                    reset(fx, 0);
                }
                self.fade = 0;
                self.alpha_gate = 0;
                self.alpha_c -= scalar << 8;
                if self.alpha_c > 0 {
                    self.cancel_target = None;
                    return true;
                }
                self.alpha_c = 0;
                self.alpha_b -= scalar << 8;
                if self.alpha_b > 0 {
                    self.cancel_target = None;
                    return true;
                }
                self.cancel_target = Some(if *fp != 0 {
                    0x16
                } else if self.entry_word != 0 {
                    0x10
                } else {
                    0x02
                });
                self.cursor_kind = 1;
                self.alpha_b = 0;
                self.panel_fade = 0;
                self.slider_x = 0x100;
                self.slider_dir = 0;
                self.rows = 2;
                if pad.nav & PADMASK_CURSOR_PREV != 0 {
                    self.main_row -= 1;
                }
                if pad.nav & PADMASK_CURSOR_NEXT != 0 {
                    self.main_row += 1;
                }
                self.main_row &= 1;
                if confirm {
                    self.submode = 0x07;
                    self.menu_row = self.main_row;
                }
            }
            // 0x15 - the card check, 0x801DE260. The fault arm is on.
            0x15 => {
                self.cancel_target = None;
                self.fault_arm = true;
                self.check_timer += scalar;
                if self.check_timer >= 0x259 {
                    // Timed out: the "no card" prompt, any button back to
                    // the menu through 0x0F's tail at 0x801DEF2C.
                    self.check_timer = 0x258;
                    if any {
                        self.probed_row = -1;
                        self.probed = 0;
                        reset(fx, 0);
                        self.submode = 0x14;
                    }
                    return true;
                }
                if card.error != 0 {
                    // Into 0x0A's body at 0x801DE838.
                    reset(fx, 0);
                    self.submode = 0x07;
                    return true;
                }
                if card.probe == 0 || card.grace != 0 {
                    card.grace -= scalar;
                    if card.grace < 0 {
                        card.grace = 0;
                    }
                    self.sweep_dir = 1;
                    return true;
                }
                if card.slot_kind == 3 && card.scan_request == 0 {
                    self.sweep_dir = 1;
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
                    self.fault_flash = 0x0A;
                }
                if card.block_count >= 2 {
                    card.scan_total = 0;
                    if self.menu_index == 0 {
                        card.op_request = 0;
                        self.submode = 0x0F;
                    } else {
                        self.submode = 0x0C;
                    }
                }
            }
            // 0x16 - the launch white-out, 0x801DF8D0.
            0x16 => {
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
                self.fault_arm = false;
                self.cancel_target = None;
                if self.launch_timer >= 0x5B {
                    self.fade += scalar << 8;
                }
                if self.entry_word != 0 {
                    let level = (0x1000 - self.fade).clamp(0, 0xFFF);
                    self.music_level = (level * 0xFF) >> 12;
                }
                if self.fade >= 0x1200 {
                    self.fade = 0x1200;
                    self.submode = 0x06;
                    if self.op_kind == OP_KIND_LOAD {
                        fx.push(TitleTickEffect::LaunchGame { from_load: true });
                        self.entry_word = 0;
                        // `j 0x801E0274` at 0x801DFB54.
                        return false;
                    }
                }
            }
            // 0x17 - the save result screen, 0x801DF6F4.
            0x17 => {
                self.cancel_target = None;
                self.fault_arm = false;
                card.op_status = 0;
                if card.result == 1 && card.retry_latch == 0 {
                    card.fault = 0;
                    card.result = 0;
                    reset(fx, 1);
                    card.retries += 1;
                    if card.retries < 5 {
                        card.ready = 0;
                        card.op_status = 1;
                        self.submode = 0x03;
                        return true;
                    }
                }
                self.cancel_target = Some(0x14);
                self.op_kind = 0;
                if any {
                    reset(fx, 0);
                    self.submode = 0x14;
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                    card.retry_latch = 0;
                    self.probed_row = -1;
                    self.alpha_gate = 0;
                    self.probed = 0;
                }
            }
            // 0x18 - the CONTINUE fade-in, 0x801DDD94.
            0x18 => {
                self.cancel_target = None;
                self.remap_gate = -1;
                self.preroll += scalar << 7;
                if self.preroll >= 0x1001 {
                    self.preroll = 0x1000;
                    self.submode = 0x14;
                }
                self.fault_arm = false;
            }
            // Out of range: `sltiu v0,s2,0x19` sends it to the epilogue.
            _ => {}
        }
        true
    }

    /// Mode `0x12`'s inner dispatch on `state[+0x1C0]` (21-entry JT at
    /// `0x801CF2AC`, `0x801DF048..0x801DF338`): the format operation's
    /// phases. Phases `3..=9` and `0x0E..=0x14` are the epilogue.
    fn format_phase(
        &mut self,
        pad: TitleTickPad,
        card: &mut TitleCardStatus,
        scalar: i32,
        fx: &mut Vec<TitleTickEffect>,
    ) {
        let any = pad.edge & PADMASK_ANY_FACE_OR_L != 0;
        match card.op_phase {
            0 => {
                card.op_attempts += 1;
                card.op_phase += 1;
            }
            1 => {
                if card.block_count != 0 && card.op_reply == -2 {
                    card.op_status = 7;
                    card.op_phase += 1;
                }
                if card.slot_kind == 3 || card.op_request == 3 {
                    card.op_request = 0;
                    card.op_phase += 1;
                    fx.push(TitleTickEffect::CardReset { arg: 0 });
                    card.op_timer = 0x78;
                }
                if card.fault > 0 {
                    card.op_phase = 0x0A;
                }
            }
            2 => {
                card.op_timer -= scalar;
                if card.op_timer < 0 {
                    card.op_phase = if card.slot_kind == 3 { 0x0C } else { 0x0D };
                }
            }
            0x0A => {
                if card.fault > 0 {
                    self.fault_arm = true;
                } else {
                    card.op_timer = 0x5A;
                    card.op_phase += 1;
                }
            }
            0x0B => {
                card.op_timer -= scalar;
                if card.op_timer < 0 {
                    card.op_timer = 0x64;
                    let from = card.op_phase;
                    card.op_phase = from + 1;
                    if card.block_count >= 2 {
                        card.op_phase = from + 2;
                    }
                }
            }
            0x0C => {
                if any && card.op_timer >= 0x5B {
                    card.op_timer -= 0x5A;
                    fx.push(TitleTickEffect::Sfx(TITLE_SFX_CONFIRM));
                }
                card.block_count = 0;
                self.probed_row = -1;
                self.probed = 0;
                card.op_timer -= scalar;
                if card.op_timer < 0 {
                    fx.push(TitleTickEffect::CardReset { arg: 0 });
                    self.submode = 0x07;
                }
            }
            0x0D => {
                if card.op_attempts >= 3 {
                    card.op_result = 2;
                } else {
                    card.op_phase = 0;
                    fx.push(TitleTickEffect::CardReset { arg: 0 });
                }
            }
            _ => {}
        }
    }

    /// The shared epilogue at [`SUBMODE_BODY_PC`], which runs after every
    /// handler (and *is* the handler for sub-mode `0x01` and for any
    /// out-of-range selector). `fp` is this frame's second argument.
    pub(super) fn epilogue(
        &mut self,
        pad: TitleTickPad,
        card: &mut TitleCardStatus,
        fp: u32,
        fx: &mut Vec<TitleTickEffect>,
    ) {
        let scalar = i32::from(pad.frame_scalar);
        // The panel slider converges on 0x2C from whichever side it is on.
        match self.slider_dir {
            1 => {
                self.slider_x -= scalar << 3;
                if self.slider_x < 0x2C {
                    self.slider_x = 0x2C;
                }
            }
            2 => {
                self.slider_x += scalar << 3;
                if self.slider_x >= 0x2D {
                    self.slider_x = 0x2C;
                }
            }
            _ => {}
        }
        // The fades, 0x801DFD9C..0x801DFE68.
        self.alpha_a += scalar << 8;
        if self.alpha_a > 0x1000 {
            self.alpha_a = 0x1000;
        }
        if self.alpha_gate != 0 {
            self.alpha_b += scalar << 8;
            if self.alpha_b > 0x1000 {
                self.alpha_b = 0x1000;
                if card.block_count == 0 && card.fault == 0 && card.scan_request == 0 {
                    self.alpha_c += scalar << 6;
                    if self.alpha_c > 0x1000 {
                        self.alpha_c = 0x1000;
                    }
                }
            }
        }
        // `submode = FUN_801E1114(submode)` - the callee returns its
        // argument, so the store is an identity and only the model pass it
        // performs is a side effect.
        if self.fault_arm && card.fault > 0 && card.grace == 0 {
            if self.submode != 0x08 {
                self.fault_flash = 0x10;
            }
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
        if fp != 0 && self.submode == 0x02 {
            // `j 0x801E0270` at 0x801DFF24: the rest of the epilogue is
            // skipped only on this rewrite.
            self.submode = 0x16;
            return;
        }
        if card.op_status == 0 {
            card.ready = 1;
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
        if let Some(shade) = self.backdrop_shade() {
            fx.push(TitleTickEffect::Backdrop { shade });
        }
    }

    /// The brightness the tick's tail draws the title backdrop at, or
    /// `None` when it draws none.
    ///
    /// The tail draws only while the entry word is raised and alpha
    /// channel B is short of `0x1000` (`0x801E01D0..0x801E01EC`), so the
    /// backdrop goes to black as B ramps up on a confirmed slot. The
    /// shade is the attract countdown clamped to `0..=16` times eight
    /// times `0x1000 - preroll / 2 - alpha_b`, over `0x1000` rounded
    /// toward zero, clamped to a byte (`0x801E01F0..0x801E025C`): `0x80`
    /// (neutral) at rest, half that while the pre-roll is full.
    ///
    /// PORT: FUN_801DD35C (`0x801E01D0..0x801E026C`, the backdrop shade)
    pub fn backdrop_shade(&self) -> Option<u8> {
        if self.entry_word == 0 || self.alpha_b == 0x1000 {
            return None;
        }
        // `slti 0x11` then `bgez`: the count is clamped high, then low.
        let count = self.countdown.clamp(0, 16);
        let span = 0x1000i32
            .wrapping_sub(self.preroll / 2)
            .wrapping_sub(self.alpha_b)
            .wrapping_mul(8);
        // `mult` / `mflo`, then the signed shift's round toward zero.
        let shade = count.wrapping_mul(span) / 0x1000;
        Some(shade.clamp(0, 0xFF) as u8)
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
