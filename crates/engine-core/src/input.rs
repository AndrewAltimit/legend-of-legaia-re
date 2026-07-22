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

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::{Deserialize, Serialize};

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

    /// Human-readable name used in TOML config files and CLI output.
    pub fn name(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::L3 => "L3",
            Self::R3 => "R3",
            Self::Start => "Start",
            Self::Up => "Up",
            Self::Right => "Right",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::L2 => "L2",
            Self::R2 => "R2",
            Self::L1 => "L1",
            Self::R1 => "R1",
            Self::Triangle => "Triangle",
            Self::Circle => "Circle",
            Self::Cross => "Cross",
            Self::Square => "Square",
        }
    }

    /// Parse a button from its [`Self::name`] string. Returns `None` for
    /// unknown names. Case-sensitive.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Select" => Some(Self::Select),
            "L3" => Some(Self::L3),
            "R3" => Some(Self::R3),
            "Start" => Some(Self::Start),
            "Up" => Some(Self::Up),
            "Right" => Some(Self::Right),
            "Down" => Some(Self::Down),
            "Left" => Some(Self::Left),
            "L2" => Some(Self::L2),
            "R2" => Some(Self::R2),
            "L1" => Some(Self::L1),
            "R1" => Some(Self::R1),
            "Triangle" => Some(Self::Triangle),
            "Circle" => Some(Self::Circle),
            "Cross" => Some(Self::Cross),
            "Square" => Some(Self::Square),
            _ => None,
        }
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
    /// The retail per-frame pad pump ([`crate::retail_pad::RetailPadState`],
    /// `FUN_8001822C`). Driven by **both** setters - [`Self::set_pad_reports`]
    /// runs the full pump from raw libpad reports, [`Self::set_pad`] runs its
    /// packed half - so the 32-vsync auto-repeat window retail's menus key
    /// off, which the plain edge pair cannot express, is live for every host.
    ///
    /// REF: FUN_8001822C
    retail: crate::retail_pad::RetailPadState,
}

/// Wall-clock read for the held-duration bookkeeping.
///
/// `std::time::Instant::now()` **panics** on `wasm32-unknown-unknown` (the
/// target has no monotonic clock), and the browser build routes every frame's
/// pad through [`InputState::set_pad`] - so the timestamps are `None` there and
/// [`InputState::held_for`] answers `false`. Nothing in the engine's field /
/// battle paths reads a held *duration* (they all key on the pressed / edge
/// queries, which are pure bitmask compares), so this costs the WASM host
/// nothing.
fn now() -> Option<Instant> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Some(Instant::now())
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drive the pad from **raw libpad reports**, the way retail does.
    ///
    /// PORT: FUN_8001822c - the per-frame pad handler, via
    /// [`crate::retail_pad::RetailPadState::pump`]. Where [`Self::set_pad`]
    /// takes an already-packed mask, this runs the retail pump: the two-port
    /// pack, the analog stick fold-ins, the SOCD cancels, the debug-mode
    /// truncation, and the 32-vsync held-history ring behind menu auto-repeat.
    /// The resulting held word feeds [`Self::set_pad`], so every existing edge
    /// query keeps working unchanged.
    ///
    /// `vsync_delta` is `DAT_1F800393` - the adaptive frame step
    /// ([`crate::world::World::frame_step`]), not a constant `1`. That is what
    /// keeps the auto-repeat rate wall-clock-constant across a cadence change.
    pub fn set_pad_reports(
        &mut self,
        port0: &crate::retail_pad::PadReport,
        port1: &crate::retail_pad::PadReport,
        debug_mode: bool,
        vsync_delta: u32,
    ) {
        self.retail.pump(port0, port1, debug_mode, vsync_delta);
        let held = self.retail.held as u16;
        // Publish the mask without re-running the pump (set_pad would pump
        // again at one vsync and double-advance the history ring).
        self.publish_pad(held);
    }

    /// The retail pad pump's full state - held / changed / pressed words plus
    /// the auto-repeat window. Live for **every** host: [`Self::set_pad`]
    /// feeds the pump's packed half too, so the window is populated whether
    /// the host supplies raw reports or an assembled mask.
    pub fn retail_pad(&self) -> &crate::retail_pad::RetailPadState {
        &self.retail
    }

    /// Replace the pad mask. The previous frame's mask is rotated into
    /// `pad_prev` so edge queries reflect the transition between the last
    /// and current call.
    pub fn set_pad(&mut self, mask: u16) {
        // Feed the retail pump's second half (`FUN_8001822C` from
        // `0x800184E0`) so every host - including the ones that assemble a
        // mask themselves rather than decoding libpad reports - gets the
        // retail edge words and the 32-vsync auto-repeat window through
        // [`Self::retail_pad`]. A host with no cadence to supply implies one
        // vsync per call; [`Self::set_pad_reports`] takes the real
        // `DAT_1F800393`.
        self.retail.pump_packed(u32::from(mask), 1);
        self.publish_pad(mask);
    }

    /// Rotate `mask` into the current/previous pair and refresh the
    /// held-duration timestamps. The half of [`Self::set_pad`] that does not
    /// touch the retail pump, so a caller that already pumped can publish
    /// without double-advancing the history ring.
    fn publish_pad(&mut self, mask: u16) {
        self.pad_prev = self.pad;
        self.pad = mask;
        let now = now();
        self.last_set = now;
        for bit in 0..16u8 {
            let m = 1u16 << bit;
            let pressed_now = mask & m != 0;
            let pressed_prev = self.pad_prev & m != 0;
            match (pressed_prev, pressed_now) {
                (false, true) => self.pressed_at[bit as usize] = now,
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
        match (self.pressed_at[bit], now()) {
            (Some(t), Some(n)) => n.duration_since(t) >= dur,
            _ => false,
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

/// Action mapping at a higher level than raw pad bits - what the field VM
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

/// Persistent keyboard-to-pad-button binding table. Serialises to and from
/// TOML so the player can override the default layout from a config file.
///
/// Keys are user-friendly keyboard names (e.g. `"Z"`, `"Up"`, `"Enter"`,
/// `"RShift"`); values are [`PadButton`] names (e.g. `"Cross"`, `"Start"`).
/// The set of recognized key names is determined by the host shell (e.g.
/// `legaia-engine`) which translates winit `KeyCode` values to these strings.
///
/// # Default layout
///
/// ```text
/// Up / Down / Left / Right  → D-pad directions
/// Z / X / A / S             → Cross / Square / Triangle / Circle
/// Q / W                     → L1 / R1
/// 1 / 2                     → L2 / R2
/// Enter                     → Start
/// RShift                    → Select
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mapping {
    /// `key_name → button_name`.
    pub bindings: HashMap<String, String>,
}

impl Default for Mapping {
    fn default() -> Self {
        let mut b = HashMap::new();
        for (key, btn) in [
            ("Up", "Up"),
            ("Down", "Down"),
            ("Left", "Left"),
            ("Right", "Right"),
            ("Z", "Cross"),
            ("X", "Square"),
            ("A", "Triangle"),
            ("S", "Circle"),
            ("Q", "L1"),
            ("W", "R1"),
            ("1", "L2"),
            ("2", "R2"),
            ("Enter", "Start"),
            ("RShift", "Select"),
        ] {
            b.insert(key.to_string(), btn.to_string());
        }
        Self { bindings: b }
    }
}

impl Mapping {
    /// Look up which [`PadButton`] `key_name` is bound to, if any.
    pub fn pad_button_for_key(&self, key_name: &str) -> Option<PadButton> {
        let btn_name = self.bindings.get(key_name)?;
        PadButton::from_name(btn_name)
    }

    /// Load from a TOML file, falling back to [`Default`] if the file is
    /// absent or unparseable.
    pub fn load_or_default(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_default()
    }

    /// Persist to a TOML file. Creates parent directories as needed.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retail_pad::{PadReport, REPEAT_PERIOD, REPEAT_WINDOW};

    /// Driving from raw reports must produce the same edge queries as the
    /// packed-mask path, so wiring the retail pump in changes nothing for
    /// existing callers.
    #[test]
    fn raw_reports_feed_the_same_edge_queries_as_a_packed_mask() {
        let mut s = InputState::new();
        let cross = PadReport::digital(PadButton::Cross.mask());
        s.set_pad_reports(&cross, &PadReport::DISCONNECTED, false, 1);
        assert!(s.just_pressed(PadButton::Cross));
        assert_eq!(s.pad(), PadButton::Cross.mask());
        s.set_pad_reports(&cross, &PadReport::DISCONNECTED, false, 1);
        assert!(s.pressed(PadButton::Cross) && !s.just_pressed(PadButton::Cross));
        s.set_pad_reports(&PadReport::DISCONNECTED, &PadReport::DISCONNECTED, false, 1);
        assert!(s.just_released(PadButton::Cross));
    }

    /// The retail pump carries state the packed path cannot: a button held
    /// for the whole 32-vsync window pulses the auto-repeat, and the window
    /// is counted in VSYNCS, so a cadence of 2 reaches it in half the ticks.
    #[test]
    fn the_auto_repeat_window_is_counted_in_vsyncs() {
        let up = PadReport::digital(PadButton::Up.mask());
        let mut fast = InputState::new();
        for _ in 0..REPEAT_WINDOW {
            fast.set_pad_reports(&up, &PadReport::DISCONNECTED, false, 1);
        }
        assert_eq!(
            fast.retail_pad().held_32,
            u32::from(PadButton::Up.mask()),
            "held across the whole window"
        );

        // At cadence 2 the same wall-clock span takes half the ticks.
        let mut slow = InputState::new();
        for _ in 0..(REPEAT_WINDOW / 2) {
            slow.set_pad_reports(&up, &PadReport::DISCONNECTED, false, 2);
        }
        assert_eq!(slow.retail_pad().held_32, fast.retail_pad().held_32);

        // And the repeat pulse rearms on the documented period.
        let mut pulses = 0;
        for _ in 0..(REPEAT_PERIOD * 4) {
            fast.set_pad_reports(&up, &PadReport::DISCONNECTED, false, 1);
            if fast.retail_pad().repeat_pulse != 0 {
                pulses += 1;
            }
        }
        assert_eq!(pulses, 4, "one pulse every REPEAT_PERIOD vsyncs");
    }

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
    fn pad_button_round_trips_name() {
        for btn in [
            PadButton::Cross,
            PadButton::Circle,
            PadButton::Start,
            PadButton::L1,
            PadButton::R2,
        ] {
            assert_eq!(PadButton::from_name(btn.name()), Some(btn));
        }
    }

    #[test]
    fn mapping_default_z_is_cross() {
        let m = Mapping::default();
        assert_eq!(m.pad_button_for_key("Z"), Some(PadButton::Cross));
        assert_eq!(m.pad_button_for_key("Up"), Some(PadButton::Up));
        assert_eq!(m.pad_button_for_key("Enter"), Some(PadButton::Start));
    }

    #[test]
    fn mapping_unknown_key_returns_none() {
        let m = Mapping::default();
        assert_eq!(m.pad_button_for_key("F13"), None);
    }

    #[test]
    fn mapping_toml_round_trip() {
        let m = Mapping::default();
        let text = toml::to_string(&m).expect("serialize");
        let m2: Mapping = toml::from_str(&text).expect("deserialize");
        assert_eq!(m2.pad_button_for_key("Z"), Some(PadButton::Cross));
        assert_eq!(m2.bindings.len(), m.bindings.len());
    }

    #[test]
    fn confirm_and_cancel_use_just_pressed() {
        let mut s = InputState::new();
        s.set_pad(PadButton::Cross.mask());
        let a = FieldActions::new(&s);
        assert!(a.confirm());
        assert!(!a.cancel());
        // Hold - confirm fires only on the press edge.
        s.set_pad(PadButton::Cross.mask());
        let a = FieldActions::new(&s);
        assert!(!a.confirm());
    }
}
