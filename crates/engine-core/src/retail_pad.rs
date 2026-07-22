//! Retail per-frame pad pump: raw libpad report -> packed Legaia pad words.
//!
//! // PORT: FUN_8001822c (SCUS_942.54 per-frame input handler)
//!
//! Every frame retail runs `FUN_8001822C`, which turns the raw libpad
//! report buffers (`0x800840F8` port 0, `0x8008411A` port 1) into the
//! **packed** pad words the whole game reads:
//!
//! | Global          | Meaning                              | Port here |
//! |-----------------|--------------------------------------|-----------|
//! | `_DAT_8007B850` | held mask (this frame)               | [`RetailPadState::held`] |
//! | `DAT_8007B7C4`  | changed mask (`held ^ prev`)         | [`RetailPadState::changed`] |
//! | `_DAT_8007B874` | newly-pressed mask (`held & !prev`)  | [`RetailPadState::pressed`] |
//! | `DAT_80089128`  | 32-entry per-vsync held history ring | [`RetailPadState`] internal |
//! | `gp+0x620`      | AND of the whole ring (held 32 vsyncs) | [`RetailPadState::held_32`] |
//! | `gp+0x624`      | auto-repeat pulse (every 8 vsyncs)   | [`RetailPadState::repeat_pulse`] |
//!
//! The packed layout is **not** the raw PSX pad word ([`crate::input::PadButton`]):
//! the second libpad button byte (face/shoulder cluster) lands in bits 0-7 and
//! the first byte (dpad/system cluster) in bits 8-15 - `mask = ~((b2 << 8) | b3)`.
//! See the [`dev_menu`](crate::dev_menu) `PACK_*` constants for the named bits.
//!
//! Retail behaviour pinned by the disassembly (`see ghidra/scripts/funcs/8001822c.txt`,
//! corroborated instruction-for-instruction by the static-recomp rendering of
//! `func_8001822C`):
//!
//! - **Digital decode** (report id high nibble `0x4`): `held = ~(b2<<8 | b3) & 0xFFFF`.
//! - **Port 1** contributes the *high* halfword the same way - but the whole
//!   high half is masked back off (`lhu`/`sw` truncation at `0x80018500`)
//!   unless the debug-mode word `_DAT_8007B98C` is non-zero. Retail leaves it
//!   zero, so retail input is port-0-only.
//! - **Analog fold-in** (report id high nibble `0x7`): the digital bits decode
//!   as above, then the **right stick** folds onto the face buttons (left =
//!   Square `0x80`, right = Circle `0x20`, up = Triangle `0x10`, down = Cross
//!   `0x40`) only while no face button is held (`held & 0xF0 == 0`), and the
//!   **left stick** folds onto the dpad (left `0x8000` / right `0x2000` /
//!   up `0x1000` / down `0x4000`) only while no dpad bit is held
//!   (`held & 0xF000 == 0`). Deadzone: `< 0x30` / `> 0xD0` on the raw axis byte.
//! - **SOCD cancel**: Left+Right together (`0xA000`) clear each other, then
//!   Up+Down together (`0x5000`) clear each other - two independent tests in
//!   that order, applied to the merged word.
//! - **Edges**: `changed = held ^ prev`, `pressed = held & !prev`, both from
//!   the post-truncation word.
//! - **Auto-repeat**: for each elapsed vsync the held word is written into a
//!   32-slot ring; `held_32` is the AND over all 32 slots (a button must have
//!   been held for the full window). A countdown decremented by the vsync
//!   delta pulses `repeat_pulse = held_32` and rearms at `+8` whenever it
//!   drops below zero - the retail menu auto-repeat cadence (fires every 8
//!   vsyncs once a button has been held 32).
//!
//! The tail of `FUN_8001822C` is the **dev-build hotkey block**, gated on
//! `_DAT_8007B98C != 0` (retail: zero, dead): pause-screen mode-0x14 entry on
//! Circle/Triangle while paused, an R2-edge frame-counter print, R1+Start
//! pause toggle in the live modes, and the Select+Start (`held == 0x900`)
//! in-field soft reset. Not ported - the engine's debug surface is the host's.
//! The vibration hand-off `FUN_80018F94(0, buf)` is libpad plumbing, also out
//! of scope.

/// One raw libpad report, as the BIOS/libpad DMA leaves it in RAM.
///
/// `status` is byte 0 (`0` = pad present), `id` is byte 1 (high nibble `0x4`
/// digital, `0x7` analog), `buttons` are the two active-low button bytes
/// (`[b2, b3]` = dpad/system cluster, face/shoulder cluster), `sticks` are
/// the four analog axis bytes `[rx, ry, lx, ly]` (`0x80` centred).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PadReport {
    /// Byte 0: `0` when a controller responded on this port.
    pub status: u8,
    /// Byte 1: report type; high nibble `0x4` = digital, `0x7` = analog.
    pub id: u8,
    /// Bytes 2..3: active-low button bytes `[dpad/system, face/shoulder]`.
    pub buttons: [u8; 2],
    /// Bytes 4..7: analog axes `[right_x, right_y, left_x, left_y]`.
    pub sticks: [u8; 4],
}

impl PadReport {
    /// A port with no controller (status non-zero) - decodes to no input.
    pub const DISCONNECTED: PadReport = PadReport {
        status: 0xFF,
        id: 0,
        buttons: [0xFF, 0xFF],
        sticks: [0x80, 0x80, 0x80, 0x80],
    };

    /// Convenience: a digital report holding exactly `packed` (packed-layout
    /// mask). Inverse of the packer's digital decode.
    pub fn digital(packed: u16) -> Self {
        PadReport {
            status: 0,
            id: 0x41,
            buttons: [!((packed >> 8) as u8), !(packed as u8)],
            sticks: [0x80, 0x80, 0x80, 0x80],
        }
    }

    /// Convenience: an analog (DualShock) report with `packed` digital bits
    /// and raw stick bytes `[rx, ry, lx, ly]`.
    pub fn analog(packed: u16, sticks: [u8; 4]) -> Self {
        PadReport {
            status: 0,
            id: 0x73,
            buttons: [!((packed >> 8) as u8), !(packed as u8)],
            sticks,
        }
    }
}

/// Analog deadzone low edge: an axis byte `< 0x30` reads as a press.
pub const STICK_LOW: u8 = 0x30;
/// Analog deadzone high edge: an axis byte `> 0xD0` reads as a press.
pub const STICK_HIGH: u8 = 0xD0;
/// Auto-repeat window length: the held-history ring the AND runs over.
pub const REPEAT_WINDOW: usize = 32;
/// Auto-repeat rearm period in vsyncs.
pub const REPEAT_PERIOD: i32 = 8;

/// Pack the two per-port reports into the 32-bit held word `_DAT_8007B850`,
/// before the debug-gate truncation (port 0 low half, port 1 high half).
///
/// Mirrors `0x80018298..0x800184DC`: digital port 0, digital port 1 into the
/// high half, analog port 0 with the two stick fold-ins, then the two SOCD
/// cancels.
pub fn pack_pad_masks(port0: &PadReport, port1: &PadReport) -> u32 {
    let mut held: u32 = 0;

    // Port 0 digital (report ok + id 0x4x).
    if port0.status == 0 && port0.id & 0xF0 == 0x40 {
        held = !(((port0.buttons[0] as u32) << 8) | port0.buttons[1] as u32) & 0xFFFF;
    }
    // Port 1 digital -> high halfword.
    if port1.status == 0 && port1.id & 0xF0 == 0x40 {
        held |= (!(((port1.buttons[0] as u32) << 8) | port1.buttons[1] as u32) & 0xFFFF) << 16;
    }
    // Port 0 analog: digital bits + stick fold-ins.
    if port0.status == 0 && port0.id & 0xF0 == 0x70 {
        held |= !(((port0.buttons[0] as u32) << 8) | port0.buttons[1] as u32) & 0xFFFF;
        // Right stick -> face buttons, only while no face button is held.
        if held & 0xF0 == 0 {
            let [rx, ry, _, _] = port0.sticks;
            if rx < STICK_LOW {
                held |= 0x80; // Square (stick left)
            }
            if rx > STICK_HIGH {
                held |= 0x20; // Circle (stick right)
            }
            if ry < STICK_LOW {
                held |= 0x10; // Triangle (stick up)
            }
            if ry > STICK_HIGH {
                held |= 0x40; // Cross (stick down)
            }
        }
        // Left stick -> dpad, only while no dpad bit is held (re-tested
        // after the right-stick fold, as retail reloads the word).
        if held & 0xF000 == 0 {
            let [_, _, lx, ly] = port0.sticks;
            if lx < STICK_LOW {
                held |= 0x8000; // Left
            }
            if lx > STICK_HIGH {
                held |= 0x2000; // Right
            }
            if ly < STICK_LOW {
                held |= 0x1000; // Up
            }
            if ly > STICK_HIGH {
                held |= 0x4000; // Down
            }
        }
    }
    // SOCD: Left+Right cancel, then Up+Down cancel (order per retail).
    if held & 0xA000 == 0xA000 {
        held &= 0xFFFF_5FFF;
    }
    if held & 0x5000 == 0x5000 {
        held &= 0xFFFF_AFFF;
    }
    held
}

/// The per-frame pad state the packer maintains: held/changed/pressed words
/// plus the 32-vsync history ring feeding the menu auto-repeat.
#[derive(Debug, Clone)]
pub struct RetailPadState {
    prev: u32,
    /// Held mask this frame (`_DAT_8007B850` post-truncation).
    pub held: u32,
    /// Changed mask (`DAT_8007B7C4 = held ^ prev`).
    pub changed: u32,
    /// Newly-pressed mask (`_DAT_8007B874 = held & !prev`).
    pub pressed: u32,
    /// AND over the 32-slot history ring (`gp+0x620`): bits held for the
    /// whole window.
    pub held_32: u32,
    /// Auto-repeat pulse (`gp+0x624`): `held_32` on repeat frames, else 0.
    pub repeat_pulse: u32,
    ring: [u32; REPEAT_WINDOW],
    ring_idx: u32,
    countdown: i32,
}

impl Default for RetailPadState {
    fn default() -> Self {
        RetailPadState {
            prev: 0,
            held: 0,
            changed: 0,
            pressed: 0,
            held_32: 0,
            repeat_pulse: 0,
            ring: [0; REPEAT_WINDOW],
            ring_idx: 0,
            countdown: 0,
        }
    }
}

impl RetailPadState {
    /// Run one frame of the retail pad pump.
    ///
    /// `debug_mode` mirrors `_DAT_8007B98C`: when clear (retail) the held
    /// word truncates to port 0's 16 bits before the edge computation.
    /// `vsync_delta` is `DAT_1F800393` - vsyncs elapsed since the last pump
    /// (1 at 60 fps, 2 when a frame was dropped, 0 for a same-vsync re-pump).
    pub fn pump(
        &mut self,
        port0: &PadReport,
        port1: &PadReport,
        debug_mode: bool,
        vsync_delta: u32,
    ) {
        let mut held = pack_pad_masks(port0, port1);
        if !debug_mode {
            held &= 0xFFFF;
        }
        self.held = held;
        self.changed = held ^ self.prev;
        self.pressed = held & !self.prev;
        self.prev = held;

        // Per-vsync history ring (one write per elapsed vsync).
        for _ in 0..vsync_delta {
            self.ring[(self.ring_idx as usize) & (REPEAT_WINDOW - 1)] = held;
            self.ring_idx = self.ring_idx.wrapping_add(1);
        }
        // AND-window over the whole ring.
        self.held_32 = self.ring.iter().fold(u32::MAX, |acc, &m| acc & m);
        // Auto-repeat countdown: pulse + rearm at +8 when it underruns.
        self.repeat_pulse = 0;
        self.countdown -= vsync_delta as i32;
        if self.countdown < 0 {
            self.countdown += REPEAT_PERIOD;
            self.repeat_pulse = self.held_32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digital_decode_is_inverted_and_byte_swapped() {
        // Raw active-low bytes: b2 = dpad/system, b3 = face/shoulder.
        // Cross held = b3 bit 6 clear -> packed 0x40.
        let p0 = PadReport {
            status: 0,
            id: 0x41,
            buttons: [0xFF, !0x40],
            sticks: [0x80; 4],
        };
        assert_eq!(pack_pad_masks(&p0, &PadReport::DISCONNECTED), 0x40);
        // Start held = b2 bit 3 clear -> packed 0x800.
        let p0 = PadReport {
            status: 0,
            id: 0x41,
            buttons: [!0x08, 0xFF],
            sticks: [0x80; 4],
        };
        assert_eq!(pack_pad_masks(&p0, &PadReport::DISCONNECTED), 0x800);
    }

    #[test]
    fn disconnected_or_unknown_type_reads_zero() {
        assert_eq!(
            pack_pad_masks(&PadReport::DISCONNECTED, &PadReport::DISCONNECTED),
            0
        );
        // Mouse / unknown id: no decode.
        let mouse = PadReport {
            status: 0,
            id: 0x12,
            buttons: [0x00, 0x00],
            sticks: [0x80; 4],
        };
        assert_eq!(pack_pad_masks(&mouse, &PadReport::DISCONNECTED), 0);
    }

    #[test]
    fn port1_lands_in_high_halfword() {
        let p1 = PadReport::digital(0x0040);
        let held = pack_pad_masks(&PadReport::DISCONNECTED, &p1);
        assert_eq!(held, 0x0040_0000);
    }

    #[test]
    fn left_stick_folds_onto_dpad_only_when_dpad_clear() {
        // Stick hard left, no digital dpad -> packed Left.
        let p0 = PadReport::analog(0, [0x80, 0x80, 0x00, 0x80]);
        assert_eq!(pack_pad_masks(&p0, &PadReport::DISCONNECTED), 0x8000);
        // Digital Right held: stick fold suppressed entirely.
        let p0 = PadReport::analog(0x2000, [0x80, 0x80, 0x00, 0x80]);
        assert_eq!(pack_pad_masks(&p0, &PadReport::DISCONNECTED), 0x2000);
    }

    #[test]
    fn right_stick_folds_onto_face_buttons() {
        // Stick up -> Triangle (0x10); down -> Cross (0x40).
        let p0 = PadReport::analog(0, [0x80, 0x00, 0x80, 0x80]);
        assert_eq!(pack_pad_masks(&p0, &PadReport::DISCONNECTED), 0x10);
        let p0 = PadReport::analog(0, [0x80, 0xFF, 0x80, 0x80]);
        assert_eq!(pack_pad_masks(&p0, &PadReport::DISCONNECTED), 0x40);
        // Face button already held: no fold.
        let p0 = PadReport::analog(0x20, [0x00, 0x80, 0x80, 0x80]);
        assert_eq!(pack_pad_masks(&p0, &PadReport::DISCONNECTED), 0x20);
    }

    #[test]
    fn stick_deadzone_edges_are_exclusive() {
        // 0x30 and 0xD0 are inside the deadzone (strict < / >).
        let p0 = PadReport::analog(0, [0x80, 0x80, STICK_LOW, STICK_HIGH]);
        assert_eq!(pack_pad_masks(&p0, &PadReport::DISCONNECTED), 0);
    }

    #[test]
    fn socd_cancels_left_right_then_up_down() {
        // Digital Left+Right -> both clear.
        let p0 = PadReport::digital(0xA000);
        assert_eq!(pack_pad_masks(&p0, &PadReport::DISCONNECTED), 0);
        // Digital Up+Down -> both clear.
        let p0 = PadReport::digital(0x5000);
        assert_eq!(pack_pad_masks(&p0, &PadReport::DISCONNECTED), 0);
        // All four -> all clear; other bits survive.
        let p0 = PadReport::digital(0xF040);
        assert_eq!(pack_pad_masks(&p0, &PadReport::DISCONNECTED), 0x40);
    }

    #[test]
    fn retail_truncates_port1_and_computes_edges() {
        let mut st = RetailPadState::default();
        let p1 = PadReport::digital(0xFFFF);
        st.pump(&PadReport::digital(0x40), &p1, false, 1);
        assert_eq!(st.held, 0x40, "port 1 masked off in retail");
        assert_eq!(st.pressed, 0x40);
        assert_eq!(st.changed, 0x40);
        st.pump(&PadReport::digital(0x40), &p1, false, 1);
        assert_eq!(st.pressed, 0, "held, not newly pressed");
        assert_eq!(st.changed, 0);
        st.pump(&PadReport::digital(0), &p1, false, 1);
        assert_eq!(st.pressed, 0);
        assert_eq!(st.changed, 0x40, "release flips changed only");
        // Debug mode keeps the high half.
        let mut st = RetailPadState::default();
        st.pump(&PadReport::digital(0), &p1, true, 1);
        assert_eq!(st.held, 0xFFFF_0000);
    }

    #[test]
    fn auto_repeat_needs_full_window_then_pulses_every_8() {
        let mut st = RetailPadState::default();
        let held = PadReport::digital(0x1000);
        let mut pulses = Vec::new();
        for frame in 0..64 {
            st.pump(&held, &PadReport::DISCONNECTED, false, 1);
            if st.repeat_pulse != 0 {
                pulses.push(frame);
            }
        }
        // The ring only saturates after 32 writes; the countdown pulses
        // every 8 vsyncs from the start but reads 0 until then.
        assert!(st.held_32 == 0x1000);
        assert!(!pulses.is_empty());
        assert!(
            pulses[0] >= REPEAT_WINDOW - 1,
            "no pulse before the window fills: {pulses:?}"
        );
        for w in pulses.windows(2) {
            assert_eq!(w[1] - w[0], REPEAT_PERIOD as usize, "8-vsync cadence");
        }
    }

    #[test]
    fn zero_vsync_delta_pump_neither_writes_ring_nor_pulses() {
        let mut st = RetailPadState::default();
        let held = PadReport::digital(0x1000);
        // Fill the window.
        for _ in 0..40 {
            st.pump(&held, &PadReport::DISCONNECTED, false, 1);
        }
        let ring_before = st.ring;
        let countdown_before = st.countdown;
        st.pump(&PadReport::digital(0), &PadReport::DISCONNECTED, false, 0);
        assert_eq!(st.ring, ring_before, "no ring write with 0 vsyncs");
        assert_eq!(st.countdown, countdown_before);
        // But the AND-window still recomputes from the (unchanged) ring.
        assert_eq!(st.held_32, 0x1000);
    }
}
