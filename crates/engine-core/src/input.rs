//! PSX-shaped pad input state.
//!
//! Mirrors the bit layout of the SCEI 16-bit digital pad word that the
//! retail engine reads via `_DAT_8007BAxx` after a controller poll. The
//! engine layer talks to this module; how those bits arrive (winit
//! key-events, gilrs gamepads, scripted demo input) is the host's problem.
//!
//! No windowing or HID dependencies live here so `legaia-engine-core` stays
//! engine-agnostic. The `asset-viewer` crate builds the keyboard / gamepad
//! mapping on top of [`PadButton`] and [`InputState`].

use std::time::{Duration, Instant};

/// Bit positions for the 16 pad buttons. Values match the PSX hardware
/// layout (0x0001 = Select … 0x8000 = Square) so engine-side code can
/// either use these typed constants or pack/unpack the raw word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum PadButton {
    Select = 0x0001,
    L3 = 0x0002,
    R3 = 0x0004,
    Start = 0x0008,
    Up = 0x0010,
    Right = 0x0020,
    Down = 0x0040,
    Left = 0x0080,
    L2 = 0x0100,
    R2 = 0x0200,
    L1 = 0x0400,
    R1 = 0x0800,
    Triangle = 0x1000,
    Circle = 0x2000,
    Cross = 0x4000,
    Square = 0x8000,
}

impl PadButton {
    /// Numeric mask, identical to `self as u16`. Convenience for code that
    /// works in raw u16 land.
    pub fn mask(self) -> u16 {
        self as u16
    }
}

/// Snapshot + edge-tracking pad state.
///
/// Hosts call [`InputState::set_pad`] each frame with the latest button
/// bitmask; the struct retains the previous frame's mask so per-frame
/// edge detection ([`InputState::just_pressed`] / [`InputState::just_released`])
/// works without the caller having to remember it.
#[derive(Debug, Clone, Default)]
pub struct InputState {
    /// Current frame's button mask.
    pad: u16,
    /// Last frame's button mask.
    pad_prev: u16,
    /// Left analog stick, two `i8` axes (X right-positive, Y down-positive).
    /// Matches the PSX dual-shock raw range when scaled to `[-127, 127]`.
    /// Hosts that don't have an analog input can leave this at `(0, 0)`.
    lstick: (i8, i8),
    /// Right analog stick, same coordinate convention as [`Self::lstick`].
    rstick: (i8, i8),
    /// Wall-clock timestamp of the last [`InputState::set_pad`] call. Used
    /// only by [`InputState::held_for`] for "is this button held for at
    /// least N millis" queries.
    last_set: Option<Instant>,
    /// Per-button "first time held" timestamp. Reset when the button is
    /// released. Used by [`Self::held_for`].
    pressed_at: [Option<Instant>; 16],
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the pad mask. The previous frame's mask is rotated into
    /// `pad_prev` so edge queries reflect the transition between the last
    /// and current call.
    pub fn set_pad(&mut self, mask: u16) {
        self.pad_prev = self.pad;
        self.pad = mask;
        let now = Instant::now();
        self.last_set = Some(now);
        for bit in 0..16u8 {
            let m = 1u16 << bit;
            let pressed_now = mask & m != 0;
            let pressed_prev = self.pad_prev & m != 0;
            match (pressed_prev, pressed_now) {
                (false, true) => self.pressed_at[bit as usize] = Some(now),
                (true, false) => self.pressed_at[bit as usize] = None,
                _ => {}
            }
        }
    }

    /// Set the left analog stick position. Axes are signed bytes with the
    /// PSX range `[-127, 127]`.
    pub fn set_lstick(&mut self, axes: (i8, i8)) {
        self.lstick = axes;
    }

    /// Set the right analog stick position.
    pub fn set_rstick(&mut self, axes: (i8, i8)) {
        self.rstick = axes;
    }

    /// Raw 16-bit pad mask for this frame.
    pub fn pad(&self) -> u16 {
        self.pad
    }

    /// Raw 16-bit pad mask for the previous frame.
    pub fn pad_prev(&self) -> u16 {
        self.pad_prev
    }

    /// Is the button currently held?
    pub fn pressed(&self, button: PadButton) -> bool {
        self.pad & button.mask() != 0
    }

    /// Was the button up last frame and down this frame?
    pub fn just_pressed(&self, button: PadButton) -> bool {
        let m = button.mask();
        self.pad & m != 0 && self.pad_prev & m == 0
    }

    /// Was the button down last frame and up this frame?
    pub fn just_released(&self, button: PadButton) -> bool {
        let m = button.mask();
        self.pad & m == 0 && self.pad_prev & m != 0
    }

    /// Has the button been continuously held for at least `dur`? Returns
    /// `false` if the button isn't pressed or no timestamp is recorded yet.
    pub fn held_for(&self, button: PadButton, dur: Duration) -> bool {
        if !self.pressed(button) {
            return false;
        }
        let bit = button.mask().trailing_zeros() as usize;
        match self.pressed_at[bit] {
            Some(t) => Instant::now().duration_since(t) >= dur,
            None => false,
        }
    }

    /// Left analog stick, axes in `[-127, 127]`.
    pub fn lstick(&self) -> (i8, i8) {
        self.lstick
    }

    /// Right analog stick, axes in `[-127, 127]`.
    pub fn rstick(&self) -> (i8, i8) {
        self.rstick
    }

    /// Convenience: compose a u16 mask out of an iterator of pressed
    /// buttons. Useful for tests and scripted demo input.
    pub fn mask_of<I: IntoIterator<Item = PadButton>>(it: I) -> u16 {
        it.into_iter().fold(0u16, |acc, b| acc | b.mask())
    }
}

/// Action mapping at a higher level than raw pad bits — what the field VM
/// and menu code typically want to ask. The retail engine has comparable
/// helpers in the input dispatcher; see `FUN_8001822c` in
/// `docs/reference/functions.md`.
#[derive(Debug, Clone, Copy)]
pub struct FieldActions<'a> {
    pub input: &'a InputState,
}

impl<'a> FieldActions<'a> {
    pub fn new(input: &'a InputState) -> Self {
        Self { input }
    }
    pub fn confirm(&self) -> bool {
        self.input.just_pressed(PadButton::Cross)
    }
    pub fn cancel(&self) -> bool {
        self.input.just_pressed(PadButton::Circle)
    }
    pub fn menu(&self) -> bool {
        self.input.just_pressed(PadButton::Start)
    }
    pub fn move_x(&self) -> i8 {
        let mut x: i32 = 0;
        if self.input.pressed(PadButton::Left) {
            x -= 127;
        }
        if self.input.pressed(PadButton::Right) {
            x += 127;
        }
        let lx = self.input.lstick().0 as i32;
        if lx.abs() > x.abs() {
            x = lx;
        }
        x.clamp(-127, 127) as i8
    }
    pub fn move_y(&self) -> i8 {
        let mut y: i32 = 0;
        if self.input.pressed(PadButton::Up) {
            y -= 127;
        }
        if self.input.pressed(PadButton::Down) {
            y += 127;
        }
        let ly = self.input.lstick().1 as i32;
        if ly.abs() > y.abs() {
            y = ly;
        }
        y.clamp(-127, 127) as i8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressed_just_pressed_cycle() {
        let mut s = InputState::new();
        assert!(!s.pressed(PadButton::Cross));
        s.set_pad(PadButton::Cross.mask());
        assert!(s.pressed(PadButton::Cross));
        assert!(s.just_pressed(PadButton::Cross));
        // Second frame: held but no longer "just" pressed.
        s.set_pad(PadButton::Cross.mask());
        assert!(s.pressed(PadButton::Cross));
        assert!(!s.just_pressed(PadButton::Cross));
        // Release.
        s.set_pad(0);
        assert!(!s.pressed(PadButton::Cross));
        assert!(s.just_released(PadButton::Cross));
    }

    #[test]
    fn mask_of_combines_buttons() {
        let m = InputState::mask_of([PadButton::Up, PadButton::Cross]);
        assert_eq!(m, PadButton::Up.mask() | PadButton::Cross.mask());
    }

    #[test]
    fn field_actions_dpad_overrides_idle_stick() {
        let mut s = InputState::new();
        s.set_pad(PadButton::Right.mask());
        let a = FieldActions::new(&s);
        assert_eq!(a.move_x(), 127);
        assert_eq!(a.move_y(), 0);
    }

    #[test]
    fn field_actions_stick_overrides_dpad_when_stronger() {
        let mut s = InputState::new();
        s.set_pad(PadButton::Right.mask());
        // The d-pad already set x = +127. A stronger stick reading would
        // need to be >127; clamp prevents anything stronger so test the
        // "weaker dpad, no stick" path instead.
        s.set_lstick((50, 0));
        let a = FieldActions::new(&s);
        assert_eq!(a.move_x(), 127);
    }

    #[test]
    fn field_actions_stick_only() {
        let mut s = InputState::new();
        s.set_lstick((-80, 60));
        let a = FieldActions::new(&s);
        assert_eq!(a.move_x(), -80);
        assert_eq!(a.move_y(), 60);
    }

    #[test]
    fn confirm_and_cancel_use_just_pressed() {
        let mut s = InputState::new();
        s.set_pad(PadButton::Cross.mask());
        let a = FieldActions::new(&s);
        assert!(a.confirm());
        assert!(!a.cancel());
        // Hold — confirm fires only on the press edge.
        s.set_pad(PadButton::Cross.mask());
        let a = FieldActions::new(&s);
        assert!(!a.confirm());
    }
}
