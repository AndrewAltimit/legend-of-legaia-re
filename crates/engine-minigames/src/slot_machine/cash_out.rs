//! The cash-out submenu, rules pages, not-enough-coins prompt and leave fade.

use super::*;

// --- The cash-out submenu ----------------------------------------------------

/// Retail packed pad bits (`_DAT_8007B874`, `legaia_engine_system::retail_pad`) the
/// cash-out flow tests. The face/shoulder byte is the low byte, the
/// d-pad/system byte the high one.
pub mod menu_pad {
    /// State 1's submenu edge: Triangle (`0x10`) or Select (`0x100`)
    /// (`andi v0,v1,0x110` at `0x801CF40C`).
    pub const OPEN: u32 = 0x0110;
    /// Cursor up (`0x801CF950`).
    pub const UP: u32 = 0x1000;
    /// Cursor down (`0x801CF974`).
    pub const DOWN: u32 = 0x4000;
    /// Back out: Circle or L2 (`andi v0,a0,0x21` at `0x801CFA2C`).
    pub const CANCEL: u32 = 0x0021;
    /// Take the row / turn the page: Cross or L1 (`andi v0,a0,0x44`).
    pub const CONFIRM: u32 = 0x0044;
    /// Any face button - the not-enough-coins prompt's leave edge (`0xF0`).
    pub const FACE: u32 = 0x00F0;
}

/// Submenu confirm - and the not-enough-coins prompt's leave - into ring slot
/// 0 (`0x801CF418`, `0x801CFA58`, `0x801CFD8C`). A static-table id (`< 0x200`),
/// so it resolves through the class-0 bank, not the machine's own.
pub const CUE_MENU_CONFIRM: i16 = 0x20;
/// Submenu cursor move, and the first instruction page's turn (`0x801CF964`,
/// `0x801CF988`, `0x801CFB3C`).
pub const CUE_MENU_CURSOR: i16 = 0x21;
/// Submenu cancel, and the second instruction page's close (`0x801CFA38`,
/// `0x801CFBA8`).
pub const CUE_MENU_CANCEL: i16 = 0x37;

/// The submenu's rows, in cursor order. The rows' words are a 4bpp image on the
/// art pack (page `(832, 256)`, `uv (0, 160)`, 80x48 - `FUN_801D317C`), so the
/// order is read off the arms state `0x32` branches to, not off any string.
pub const MENU_ROW_PLAY: u32 = 0;
/// Row 1 commits the balance and leaves (state `100`).
pub const MENU_ROW_QUIT: u32 = 1;
/// Row 2 opens the two instruction pages (state `0x33`).
pub const MENU_ROW_RULES: u32 = 2;

/// The fade step the instruction pages and the leave fade ramp by, per frame.
pub const MENU_FADE_STEP: i32 = 0x10;

impl SlotMachine {
    /// The retail state word (`DAT_801d3c84`) this machine stands for.
    pub fn retail_state(&self) -> i32 {
        match self.phase {
            SlotPhase::Idle => 1,
            SlotPhase::Spinning => 2,
            SlotPhase::Stopping => 3,
            SlotPhase::Payout => 4,
            SlotPhase::Menu => i32::from(self.menu_state),
            SlotPhase::NoCoins => 0x5A,
            SlotPhase::Leaving | SlotPhase::CashedOut => 100,
        }
    }

    /// What to draw ([`SlotScreen`]).
    pub fn screen(&self) -> SlotScreen {
        match (self.phase, self.menu_state) {
            (SlotPhase::Menu, 0x32) => SlotScreen::Picker {
                row: self.menu_cursor,
            },
            (SlotPhase::Menu, 0x35) => SlotScreen::Instructions { page: 0 },
            (SlotPhase::Menu, 0x36 | 0x38) => SlotScreen::Instructions { page: 1 },
            (SlotPhase::NoCoins, _) => SlotScreen::NoCoins,
            _ => SlotScreen::Machine,
        }
    }

    /// The screen-fade level (`DAT_801d3c98`, `0..=0xFF`): how far the frame
    /// is faded to black (`FUN_80024EE4(0, 2, level * 0x10101)`). Non-zero only
    /// across the instruction pages' transitions and the leave fade.
    pub fn fade_level(&self) -> i32 {
        match self.phase {
            SlotPhase::Menu | SlotPhase::Leaving | SlotPhase::CashedOut => self.fade.clamp(0, 0xFF),
            _ => 0,
        }
    }

    /// `true` while the cash-out flow owns the pad: the submenu, its pages,
    /// the not-enough-coins prompt and the leave fade. A host must not feed
    /// spin / stop / collect input on such a frame.
    pub fn in_cash_out_flow(&self) -> bool {
        matches!(
            self.phase,
            SlotPhase::Menu | SlotPhase::NoCoins | SlotPhase::Leaving
        )
    }

    /// One frame of the cash-out flow on the retail packed pad-edge word
    /// `pressed` ([`menu_pad`]). Call it every frame, **before** the spin /
    /// stop input; it returns `true` when the flow owns this frame (the
    /// host then skips the rest of its slot input).
    ///
    /// In state 1 the submenu edge is tested first - so the submenu opens
    /// even on an empty balance - and only then the `< 3` coin gate, which
    /// raises the not-enough-coins prompt rather than refusing a spin.
    ///
    /// - `0x32`, the picker: Up / Down move the cursor (`0x21` each), kept as
    ///   an unsigned word `% 3`, so Up on row 0 stays put while Down on row 2
    ///   wraps; Circle / L2 cancel back to play (`0x37`); Cross / L1 take the
    ///   row (`0x20`) - play, quit (state `100`), or the rules pages
    ///   (state `0x33`).
    /// - `0x33..=0x39`, the rules: fade to black (`0x34`), page 0 fades in and
    ///   waits for Cross / L1 (`0x35`, `0x21`), page 1 waits for Cross / L1
    ///   (`0x36`, `0x37`), fade out (`0x38`), fade back in on the machine
    ///   (`0x39`), and back to state 1.
    /// - `0x5A`, the prompt: any face button leaves (`0x20`, state `100`).
    /// - `100`: fade out; at full black the balance is committed
    ///   ([`SlotPhase::CashedOut`]).
    ///
    /// Retail runs the confirm test after the cancel test in the same frame,
    /// relative to the state the cancel just wrote; a frame pressing both on
    /// the rules row therefore lands in state 2 with no bet charged. That
    /// collision is not reproduced: the rules row opens the rules.
    // PORT: FUN_801cf0d8 state 1 (submenu edge + coin gate), states 0x32..0x39, 0x5a, 100
    pub fn cash_out_input(&mut self, pressed: u32) -> bool {
        use menu_pad::*;
        match self.phase {
            SlotPhase::Idle => {
                if pressed & OPEN != 0 {
                    self.sounds.ring.push((0, CUE_MENU_CONFIRM));
                    self.menu_cursor = 0;
                    self.menu_state = 0x32;
                    self.phase = SlotPhase::Menu;
                    return true;
                }
                if self.balance < MIN_SPIN_BALANCE {
                    self.phase = SlotPhase::NoCoins;
                    return true;
                }
                false
            }
            SlotPhase::Menu => {
                self.menu_frame(pressed);
                true
            }
            SlotPhase::NoCoins => {
                if pressed & FACE != 0 {
                    self.sounds.ring.push((0, CUE_MENU_CONFIRM));
                    self.phase = SlotPhase::Leaving;
                }
                true
            }
            SlotPhase::Leaving => {
                self.fade += MENU_FADE_STEP;
                if self.fade > 0xFF {
                    self.fade = 0xFF;
                    self.phase = SlotPhase::CashedOut;
                }
                true
            }
            _ => false,
        }
    }

    fn menu_frame(&mut self, pressed: u32) {
        use menu_pad::*;
        let confirm = pressed & CONFIRM != 0;
        match self.menu_state {
            0x32 => {
                if pressed & UP != 0 {
                    self.sounds.ring.push((0, CUE_MENU_CURSOR));
                    self.menu_cursor = self.menu_cursor.wrapping_sub(1);
                }
                if pressed & DOWN != 0 {
                    self.sounds.ring.push((0, CUE_MENU_CURSOR));
                    self.menu_cursor = self.menu_cursor.wrapping_add(1);
                }
                // `multu` by `0xAAAAAAAB`: an unsigned remainder.
                self.menu_cursor %= 3;
                if pressed & CANCEL != 0 {
                    self.sounds.ring.push((0, CUE_MENU_CANCEL));
                    self.phase = SlotPhase::Idle;
                }
                if confirm {
                    self.sounds.ring.push((0, CUE_MENU_CONFIRM));
                    match self.menu_cursor {
                        MENU_ROW_PLAY => self.phase = SlotPhase::Idle,
                        MENU_ROW_QUIT => {
                            // The quit arm stores the confirm cue a second
                            // time (`0x801CFD8C`) - the same slot, one sound.
                            self.fade = 0;
                            self.phase = SlotPhase::Leaving;
                        }
                        _ => {
                            self.phase = SlotPhase::Menu;
                            self.menu_state = 0x33;
                        }
                    }
                }
            }
            0x33 => {
                self.fade = 0;
                self.menu_state = 0x34;
            }
            0x34 | 0x38 => {
                self.fade += MENU_FADE_STEP;
                if self.fade > 0xFF {
                    self.fade = 0xFF;
                    self.menu_state += 1;
                }
            }
            0x35 => {
                self.fade -= MENU_FADE_STEP;
                if self.fade < 0 {
                    self.fade = 0;
                    if confirm {
                        self.sounds.ring.push((0, CUE_MENU_CURSOR));
                        self.menu_state = 0x36;
                    }
                }
            }
            0x36 => {
                if confirm {
                    self.sounds.ring.push((0, CUE_MENU_CANCEL));
                    self.menu_state = 0x38;
                }
            }
            _ => {
                // 0x39: fade back in on the machine.
                self.fade -= MENU_FADE_STEP;
                if self.fade < 0 {
                    self.fade = 0;
                    self.phase = SlotPhase::Idle;
                }
            }
        }
    }
}
