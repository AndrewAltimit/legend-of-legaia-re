//! Gamepad -> PSX pad, the one layout every host reads.
//!
//! A gamepad is a *controller*, not a key layout: its table binds physical
//! buttons to PSX buttons, beside the keyboard's ([`crate::input::Mapping`]
//! carries both, so one file / one `localStorage` key stores them). Every
//! host folds a controller through [`GamepadMapping`]:
//!
//! - the native window hands it the buttons gilrs reports, renamed to the
//!   W3C standard-mapping index (`GamepadMapping::fold`);
//! - the browser pages read `navigator.getGamepads()` and hand each pad's
//!   button values and axes to the same fold (`gamepad_fold` in `web-play`),
//!   so no page writes the table down a second time.
//!
//! The default layout is the W3C "standard" gamepad mapping: the bottom /
//! right / left / top face buttons are Cross / Circle / Square / Triangle, the
//! bumpers L1 / R1, the triggers L2 / R2, back / start Select / Start, the
//! stick clicks L3 / R3, then the d-pad. The left stick drives the d-pad as
//! well, past the player's dead zone (`OptionsState::stick_dead_zone`,
//! [`STICK_DEAD_ZONE`] by default). The PSX pad has no analog fold-in on any
//! host (see `InputState::set_pad_reports`), so the stick is digital here.
//!
//! A browser pad that does not report the `standard` mapping (an unknown
//! controller in Firefox on Linux is the common case) arrives with its
//! buttons in the driver's own order. Those fold through a second table,
//! [`GamepadMapping::raw_buttons`], whose default guesses the standard order
//! for the first sixteen buttons, plus a d-pad read off a hat
//! ([`RawHat::detect`]). Key Config fixes whatever the guess gets wrong: a
//! rebind made with such a pad writes the raw table.
//!
//! Rebinding a controller button runs through the same screen as a key: the
//! session waiting for a key also takes a controller press, which the host
//! hands over as a capture name ([`capture_name`], `"Pad3"` / `"Raw7"`).

use crate::pad::PadButton;
use serde::{Deserialize, Serialize};

/// The PSX button each W3C standard-mapping button index drives, in index
/// order (`Gamepad.buttons[i]`). Index 16 (the "home" / guide button) has no
/// PSX twin and is absent.
pub const STANDARD_BUTTONS: [PadButton; 16] = [
    PadButton::Cross,
    PadButton::Circle,
    PadButton::Square,
    PadButton::Triangle,
    PadButton::L1,
    PadButton::R1,
    PadButton::L2,
    PadButton::R2,
    PadButton::Select,
    PadButton::Start,
    PadButton::L3,
    PadButton::R3,
    PadButton::Up,
    PadButton::Down,
    PadButton::Left,
    PadButton::Right,
];

/// Left-stick deflection, in `[-1, 1]`, past which the stick holds a d-pad
/// direction.
pub const STICK_DEAD_ZONE: f32 = 0.5;

/// Analog button value (the triggers) past which a button counts as held.
pub const BUTTON_PRESS_THRESHOLD: f32 = 0.5;

/// The pad button W3C standard index `index` drives, if any.
pub fn standard_button(index: usize) -> Option<PadButton> {
    STANDARD_BUTTONS.get(index).copied()
}

/// The d-pad bits a left-stick position holds. `y` is **down-positive**, the
/// W3C convention (`Gamepad.axes[1]`); a host whose stick reads up-positive
/// (gilrs) negates it first.
pub fn stick_dpad(x: f32, y: f32) -> u16 {
    stick_dpad_with(x, y, STICK_DEAD_ZONE)
}

/// The d-pad bits a left-stick position holds past `dead_zone` (a fraction
/// of full deflection). [`stick_dpad`] is this at [`STICK_DEAD_ZONE`].
pub fn stick_dpad_with(x: f32, y: f32, dead_zone: f32) -> u16 {
    let dz = dead_zone.clamp(0.05, 0.95);
    let mut mask = 0;
    if x <= -dz {
        mask |= PadButton::Left.mask();
    }
    if x >= dz {
        mask |= PadButton::Right.mask();
    }
    if y <= -dz {
        mask |= PadButton::Up.mask();
    }
    if y >= dz {
        mask |= PadButton::Down.mask();
    }
    mask
}

/// How many buttons the W3C standard mapping names (index 16 is home).
pub const STANDARD_BUTTON_COUNT: usize = 17;

/// How many buttons the table for a non-standard pad holds.
pub const RAW_BUTTON_COUNT: usize = 24;

/// Which index space a controller's buttons arrive in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ButtonSpace {
    #[default]
    /// The W3C standard mapping (every native controller, through gilrs'
    /// own mapping database; a browser pad reporting `mapping == "standard"`).
    Standard,
    /// A browser pad with no standard mapping: the driver's button order.
    Raw,
}

impl ButtonSpace {
    fn count(self) -> usize {
        match self {
            Self::Standard => STANDARD_BUTTON_COUNT,
            Self::Raw => RAW_BUTTON_COUNT,
        }
    }
}

/// Capture names for a standard-mapping press, by index: what a host hands
/// the Key Config screen in place of a key name (`"Pad0"` is the bottom face
/// button). Static so the native window's `&'static str` latch can hold one.
pub const STANDARD_CAPTURE_NAMES: [&str; STANDARD_BUTTON_COUNT] = [
    "Pad0", "Pad1", "Pad2", "Pad3", "Pad4", "Pad5", "Pad6", "Pad7", "Pad8", "Pad9", "Pad10",
    "Pad11", "Pad12", "Pad13", "Pad14", "Pad15", "Pad16",
];

/// Capture names for a non-standard pad's press, by raw index.
pub const RAW_CAPTURE_NAMES: [&str; RAW_BUTTON_COUNT] = [
    "Raw0", "Raw1", "Raw2", "Raw3", "Raw4", "Raw5", "Raw6", "Raw7", "Raw8", "Raw9", "Raw10",
    "Raw11", "Raw12", "Raw13", "Raw14", "Raw15", "Raw16", "Raw17", "Raw18", "Raw19", "Raw20",
    "Raw21", "Raw22", "Raw23",
];

/// The capture name for a press of `index` in `space`, if the table has a
/// slot for it.
pub fn capture_name(space: ButtonSpace, index: usize) -> Option<&'static str> {
    match space {
        ButtonSpace::Standard => STANDARD_CAPTURE_NAMES.get(index).copied(),
        ButtonSpace::Raw => RAW_CAPTURE_NAMES.get(index).copied(),
    }
}

/// The controller press a capture name stands for; `None` for a key name.
pub fn parse_capture_name(name: &str) -> Option<(ButtonSpace, usize)> {
    let (space, digits) = match name.strip_prefix("Pad") {
        Some(d) => (ButtonSpace::Standard, d),
        None => (ButtonSpace::Raw, name.strip_prefix("Raw")?),
    };
    let index: usize = digits.parse().ok()?;
    (index < space.count()).then_some((space, index))
}

/// The button names a controller's maker prints on it, by W3C standard
/// position. Prompts and the Key Config table use them, so a player reads
/// the label on their own pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GlyphSet {
    /// A, B, X, Y, LB, RB, ...: the W3C figure's own labels, and the
    /// default for a pad whose maker is not recognised.
    #[default]
    Xbox,
    /// Cross, Circle, Square, Triangle, L1, R1, ...
    PlayStation,
    /// B, A, Y, X (south, east, west, north), L, R, ZL, ZR, -, +.
    Nintendo,
}

impl GlyphSet {
    /// The set for a controller by the name its backend reports (gilrs'
    /// `Gamepad::name`, the Gamepad API's `Gamepad.id`, which carries the
    /// USB vendor id).
    pub fn from_name(name: &str) -> Self {
        let n = name.to_ascii_lowercase();
        let has = |w: &str| n.contains(w);
        if has("054c")
            || has("sony")
            || has("playstation")
            || has("dualshock")
            || has("dualsense")
            || has("ps3")
            || has("ps4")
            || has("ps5")
        {
            Self::PlayStation
        } else if has("057e")
            || has("nintendo")
            || has("pro controller")
            || has("joy-con")
            || has("switch")
        {
            Self::Nintendo
        } else {
            Self::Xbox
        }
    }

    /// The printed label of standard button `index`.
    pub fn standard_label(self, index: usize) -> &'static str {
        const XBOX: [&str; STANDARD_BUTTON_COUNT] = [
            "A", "B", "X", "Y", "LB", "RB", "LT", "RT", "Back", "Menu", "LS", "RS", "D-Up",
            "D-Down", "D-Left", "D-Right", "Home",
        ];
        const PS: [&str; STANDARD_BUTTON_COUNT] = [
            "Cross", "Circle", "Square", "Triangle", "L1", "R1", "L2", "R2", "Share", "Options",
            "L3", "R3", "D-Up", "D-Down", "D-Left", "D-Right", "PS",
        ];
        const NIN: [&str; STANDARD_BUTTON_COUNT] = [
            "B", "A", "Y", "X", "L", "R", "ZL", "ZR", "-", "+", "LS", "RS", "D-Up", "D-Down",
            "D-Left", "D-Right", "Home",
        ];
        let table = match self {
            Self::Xbox => &XBOX,
            Self::PlayStation => &PS,
            Self::Nintendo => &NIN,
        };
        table.get(index).copied().unwrap_or("?")
    }

    /// The label of a button in `space`: the printed name for a standard
    /// index, `"Btn N"` (1-based, as a driver's test page counts) for a raw
    /// one.
    pub fn label(self, space: ButtonSpace, index: usize) -> String {
        match space {
            ButtonSpace::Standard => self.standard_label(index).to_string(),
            ButtonSpace::Raw => format!("Btn {}", index + 1),
        }
    }
}

/// The controller a prompt is labelled for: its maker's labels and the
/// index space its buttons arrive in. A host keeps the last pressed pad's
/// as `Option<PadStyle>` - `None` while the keyboard was pressed last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PadStyle {
    pub glyphs: GlyphSet,
    pub space: ButtonSpace,
}

/// Where a non-standard pad's d-pad is, read off the axes' resting values
/// (the first poll after the pad appears, before anything is held).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RawHat {
    /// No hat found: the d-pad comes from buttons and the stick alone.
    #[default]
    None,
    /// A DirectInput point-of-view hat folded into one axis (Chromium on
    /// Windows): eight directions at `-1 + 2k/7`, neutral past `1`.
    Pov(usize),
    /// A hat reported as two axes of `-1` / `0` / `1` (the Linux joystick
    /// API's `ABS_HAT0X` / `ABS_HAT0Y`), after the sticks and triggers.
    Pair(usize, usize),
}

impl RawHat {
    /// Find the hat from `rest`, each axis' value with nothing held.
    ///
    /// A POV axis rests past `1` (its neutral code), which no stick or
    /// trigger does. Otherwise a pad with six or more axes is taken to end on
    /// a two-axis hat when its last two axes rest at `0` - a trigger rests at
    /// `-1`, which is what keeps a six-axis pad of two sticks and two
    /// triggers from reading its triggers as a held d-pad. A heuristic: a
    /// pad that breaks it is fixed by binding its d-pad buttons in Key Config.
    pub fn detect(rest: &[f32]) -> Self {
        if let Some(i) = rest.iter().position(|&v| v > 1.05 && v < 1.6) {
            return Self::Pov(i);
        }
        let n = rest.len();
        if n >= 6 && rest[n - 2].abs() < 0.1 && rest[n - 1].abs() < 0.1 {
            return Self::Pair(n - 2, n - 1);
        }
        Self::None
    }

    /// The d-pad bits the hat holds in `axes`.
    pub fn dpad(self, axes: &[f32]) -> u16 {
        let (up, right, down, left) = (
            PadButton::Up.mask(),
            PadButton::Right.mask(),
            PadButton::Down.mask(),
            PadButton::Left.mask(),
        );
        match self {
            Self::None => 0,
            Self::Pov(i) => {
                let Some(&v) = axes.get(i) else { return 0 };
                if !(-1.05..=1.05).contains(&v) {
                    return 0;
                }
                let k = ((v + 1.0) * 3.5).round() as i32;
                // The step must sit on one of the eight codes, or an analog
                // axis passing through would read as a direction.
                if (v - (-1.0 + 2.0 * k as f32 / 7.0)).abs() > 0.06 {
                    return 0;
                }
                match k {
                    0 => up,
                    1 => up | right,
                    2 => right,
                    3 => down | right,
                    4 => down,
                    5 => down | left,
                    6 => left,
                    7 => up | left,
                    _ => 0,
                }
            }
            Self::Pair(x, y) => {
                let ax = axes.get(x).copied().unwrap_or(0.0);
                let ay = axes.get(y).copied().unwrap_or(0.0);
                stick_dpad_with(ax, ay, 0.5)
            }
        }
    }
}

/// The controller's binding table, saved beside the keyboard's
/// ([`crate::input::Mapping::gamepad`]): which PSX button each physical
/// button drives, in both index spaces, and whether the left stick drives the
/// d-pad. Each slot names a [`PadButton`] (`"Cross"`) or is empty.
///
/// The shape is the keyboard table's turned around (a slot per physical
/// button rather than a key per entry), with the same rule on rebind
/// ([`Self::bind`]): a button moved onto a slot another button held hands
/// that button its old slot, so no PSX button is left unreachable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GamepadMapping {
    /// PSX button per W3C standard index.
    pub buttons: Vec<String>,
    /// PSX button per raw index, for a pad with no standard mapping.
    pub raw_buttons: Vec<String>,
    /// The left stick drives the d-pad.
    pub stick_dpad: bool,
}

impl Default for GamepadMapping {
    fn default() -> Self {
        let mut buttons: Vec<String> = STANDARD_BUTTONS
            .iter()
            .map(|b| b.name().to_string())
            .collect();
        buttons.resize(STANDARD_BUTTON_COUNT, String::new());
        let mut raw_buttons = buttons.clone();
        raw_buttons.resize(RAW_BUTTON_COUNT, String::new());
        Self {
            buttons,
            raw_buttons,
            stick_dpad: true,
        }
    }
}

impl GamepadMapping {
    fn table(&self, space: ButtonSpace) -> &[String] {
        match space {
            ButtonSpace::Standard => &self.buttons,
            ButtonSpace::Raw => &self.raw_buttons,
        }
    }

    fn table_mut(&mut self, space: ButtonSpace) -> &mut Vec<String> {
        let t = match space {
            ButtonSpace::Standard => &mut self.buttons,
            ButtonSpace::Raw => &mut self.raw_buttons,
        };
        t.resize(space.count(), String::new());
        t
    }

    /// The PSX button physical button `index` in `space` drives.
    pub fn button_for(&self, space: ButtonSpace, index: usize) -> Option<PadButton> {
        PadButton::from_name(self.table(space).get(index)?)
    }

    /// The physical buttons in `space` bound to `button`, lowest first.
    pub fn indices_for(&self, space: ButtonSpace, button: PadButton) -> Vec<usize> {
        self.table(space)
            .iter()
            .enumerate()
            .filter(|(_, b)| PadButton::from_name(b) == Some(button))
            .map(|(i, _)| i)
            .collect()
    }

    /// What a prompt names for `button` on a pad of `glyphs` in `space`: the
    /// label of the lowest physical button bound to it, or `None`.
    pub fn label_for(
        &self,
        space: ButtonSpace,
        button: PadButton,
        glyphs: GlyphSet,
    ) -> Option<String> {
        let i = *self.indices_for(space, button).first()?;
        Some(glyphs.label(space, i))
    }

    /// Put `button` on physical button `index` in `space`. Every other slot
    /// `button` held is cleared; a button `index` held before gets
    /// `button`'s lowest old slot when that leaves it with none. Returns that
    /// handed-on slot. Out of range does nothing.
    pub fn bind(&mut self, space: ButtonSpace, index: usize, button: PadButton) -> Option<usize> {
        if index >= space.count() {
            return None;
        }
        let old = self.indices_for(space, button);
        let table = self.table_mut(space);
        let displaced = PadButton::from_name(&table[index]).filter(|b| *b != button);
        for &i in &old {
            table[i].clear();
        }
        table[index] = button.name().to_string();
        let handed = old.iter().copied().find(|&i| i != index);
        if let (Some(other), Some(slot)) = (displaced, handed)
            && !table.iter().any(|b| PadButton::from_name(b) == Some(other))
        {
            table[slot] = other.name().to_string();
            return Some(slot);
        }
        None
    }

    /// Fold one standard-mapping poll: `buttons` is `(standard index, value)`
    /// for every button of every connected controller, `sticks` each
    /// controller's left stick (`y` down-positive), `dead_zone` the stick's.
    pub fn fold(
        &self,
        buttons: impl IntoIterator<Item = (usize, f32)>,
        sticks: impl IntoIterator<Item = (f32, f32)>,
        dead_zone: f32,
    ) -> u16 {
        let mut mask = 0;
        for (index, value) in buttons {
            if value > BUTTON_PRESS_THRESHOLD
                && let Some(button) = self.button_for(ButtonSpace::Standard, index)
            {
                mask |= button.mask();
            }
        }
        if self.stick_dpad {
            for (x, y) in sticks {
                mask |= stick_dpad_with(x, y, dead_zone);
            }
        }
        mask
    }

    /// Fold one browser pad: its button values (`Gamepad.buttons[i].value`,
    /// `1` for a pressed digital button), its axes, and for a raw pad the
    /// axes' resting values ([`RawHat::detect`]).
    pub fn fold_pad(
        &self,
        space: ButtonSpace,
        buttons: &[f32],
        axes: &[f32],
        rest: &[f32],
        dead_zone: f32,
    ) -> u16 {
        let mut mask = 0;
        for (index, &value) in buttons.iter().enumerate() {
            if value > BUTTON_PRESS_THRESHOLD
                && let Some(button) = self.button_for(space, index)
            {
                mask |= button.mask();
            }
        }
        if self.stick_dpad && axes.len() >= 2 {
            mask |= stick_dpad_with(axes[0], axes[1], dead_zone);
        }
        if space == ButtonSpace::Raw {
            mask |= RawHat::detect(rest).dpad(axes);
        }
        mask
    }
}

/// What one poll of the controllers changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GamepadEdges {
    /// Every PSX bit some controller holds now.
    pub held: u16,
    /// Bits held now and not at the last poll (press edges).
    pub pressed: u16,
    /// Bits held at the last poll and not now.
    pub released: u16,
}

/// Edge tracker over the folded controller word, one per host.
///
/// Both hosts poll once per display frame. [`Self::reset`] is the focus-loss
/// rule both share: a host that drops its held keys on blur forgets the pad
/// too, so a button still held re-enters as a fresh press, not a stuck one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StandardGamepad {
    held: u16,
}

impl StandardGamepad {
    /// Fold one poll's raw state into a PSX word: `buttons` is
    /// `(standard index, value)` for every button of every connected
    /// controller, `sticks` each controller's left stick (`y` down-positive).
    pub fn fold(
        buttons: impl IntoIterator<Item = (usize, f32)>,
        sticks: impl IntoIterator<Item = (f32, f32)>,
    ) -> u16 {
        let mut mask = 0;
        for (index, value) in buttons {
            if value > BUTTON_PRESS_THRESHOLD
                && let Some(button) = standard_button(index)
            {
                mask |= button.mask();
            }
        }
        for (x, y) in sticks {
            mask |= stick_dpad(x, y);
        }
        mask
    }

    /// Adopt this poll's folded word and report what changed.
    pub fn update(&mut self, now: u16) -> GamepadEdges {
        let prev = std::mem::replace(&mut self.held, now);
        GamepadEdges {
            held: now,
            pressed: now & !prev,
            released: prev & !now,
        }
    }

    /// The word the last [`Self::update`] adopted.
    pub fn held(&self) -> u16 {
        self.held
    }

    /// Forget the held word (focus loss).
    pub fn reset(&mut self) {
        self.held = 0;
    }
}

/// The layout as JSON for the browser pages:
/// `{"buttons":["Cross",...],"stick_dead_zone":0.5,"press_threshold":0.5}`,
/// `buttons[i]` naming the PSX button standard index `i` drives (the names
/// `pad_buttons_json` keys its bits by).
pub fn layout_json() -> String {
    let names = STANDARD_BUTTONS
        .iter()
        .map(|b| format!("\"{}\"", b.name()))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"buttons\":[{names}],\"stick_dead_zone\":{STICK_DEAD_ZONE},\"press_threshold\":{BUTTON_PRESS_THRESHOLD}}}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pad_button_has_exactly_one_standard_index() {
        let mut seen = 0u16;
        for b in STANDARD_BUTTONS {
            assert_eq!(seen & b.mask(), 0, "{} bound twice", b.name());
            seen |= b.mask();
        }
        assert_eq!(seen, 0xFFFF, "every PSX bit is reachable from a controller");
    }

    #[test]
    fn face_buttons_bind_by_position() {
        // W3C: 0 bottom, 1 right, 2 left, 3 top.
        assert_eq!(standard_button(0), Some(PadButton::Cross));
        assert_eq!(standard_button(1), Some(PadButton::Circle));
        assert_eq!(standard_button(2), Some(PadButton::Square));
        assert_eq!(standard_button(3), Some(PadButton::Triangle));
        assert_eq!(standard_button(9), Some(PadButton::Start));
        assert_eq!(standard_button(16), None, "home has no PSX twin");
    }

    #[test]
    fn stick_holds_dpad_past_dead_zone_only() {
        assert_eq!(stick_dpad(0.49, -0.49), 0);
        assert_eq!(stick_dpad(-0.5, 0.0), PadButton::Left.mask());
        assert_eq!(
            stick_dpad(0.9, 0.9),
            PadButton::Right.mask() | PadButton::Down.mask()
        );
        assert_eq!(stick_dpad(0.0, -1.0), PadButton::Up.mask());
    }

    #[test]
    fn fold_reads_values_against_the_threshold() {
        let w = StandardGamepad::fold([(0, 1.0), (6, 0.4), (7, 0.6), (16, 1.0)], [(0.0, 0.0)]);
        assert_eq!(w, PadButton::Cross.mask() | PadButton::R2.mask());
    }

    #[test]
    fn update_reports_edges_and_reset_re_arms_a_press() {
        let mut gp = StandardGamepad::default();
        let x = PadButton::Cross.mask();
        let e = gp.update(x);
        assert_eq!((e.held, e.pressed, e.released), (x, x, 0));
        let e = gp.update(x);
        assert_eq!(e.pressed, 0);
        gp.reset();
        assert_eq!(
            gp.update(x).pressed,
            x,
            "held through focus loss = fresh press"
        );
        assert_eq!(gp.update(0).released, x);
    }

    #[test]
    fn layout_json_lists_the_table_in_index_order() {
        let j = layout_json();
        assert!(j.starts_with("{\"buttons\":[\"Cross\",\"Circle\",\"Square\",\"Triangle\""));
        assert!(j.contains("\"stick_dead_zone\":0.5"));
        assert!(j.contains("\"press_threshold\":0.5"));
    }
    #[test]
    fn the_default_table_is_the_standard_layout_in_both_spaces() {
        let m = GamepadMapping::default();
        for (i, b) in STANDARD_BUTTONS.iter().enumerate() {
            assert_eq!(m.button_for(ButtonSpace::Standard, i), Some(*b));
            assert_eq!(m.button_for(ButtonSpace::Raw, i), Some(*b));
        }
        assert_eq!(m.button_for(ButtonSpace::Standard, 16), None);
        assert_eq!(m.button_for(ButtonSpace::Raw, 23), None);
        let fold = |b: &[(usize, f32)]| m.fold(b.iter().copied(), [(0.0, 0.0)], STICK_DEAD_ZONE);
        let legacy = |b: &[(usize, f32)]| StandardGamepad::fold(b.iter().copied(), [(0.0, 0.0)]);
        for i in 0..17 {
            assert_eq!(fold(&[(i, 1.0)]), legacy(&[(i, 1.0)]), "index {i}");
        }
    }

    /// A rebind moves the PSX button, and the button it displaced takes the
    /// old slot: Cross onto the right face button hands Circle the bottom.
    #[test]
    fn bind_swaps_like_the_key_table() {
        let mut m = GamepadMapping::default();
        let handed = m.bind(ButtonSpace::Standard, 1, PadButton::Cross);
        assert_eq!(handed, Some(0));
        assert_eq!(
            m.button_for(ButtonSpace::Standard, 1),
            Some(PadButton::Cross)
        );
        assert_eq!(
            m.button_for(ButtonSpace::Standard, 0),
            Some(PadButton::Circle)
        );
        // Binding the slot a button already has changes nothing.
        let before = m.clone();
        assert_eq!(m.bind(ButtonSpace::Standard, 1, PadButton::Cross), None);
        assert_eq!(m, before);
        // An empty slot takes the button and frees its old one.
        assert_eq!(m.bind(ButtonSpace::Standard, 16, PadButton::Select), None);
        assert_eq!(m.button_for(ButtonSpace::Standard, 8), None);
        assert_eq!(
            m.indices_for(ButtonSpace::Standard, PadButton::Select),
            vec![16]
        );
        // The raw table is its own.
        assert_eq!(m.button_for(ButtonSpace::Raw, 0), Some(PadButton::Cross));
        assert_eq!(m.bind(ButtonSpace::Standard, 99, PadButton::Cross), None);
    }

    #[test]
    fn capture_names_round_trip_and_reject_key_names() {
        for i in 0..STANDARD_BUTTON_COUNT {
            let n = capture_name(ButtonSpace::Standard, i).unwrap();
            assert_eq!(parse_capture_name(n), Some((ButtonSpace::Standard, i)));
        }
        for i in 0..RAW_BUTTON_COUNT {
            let n = capture_name(ButtonSpace::Raw, i).unwrap();
            assert_eq!(parse_capture_name(n), Some((ButtonSpace::Raw, i)));
        }
        assert_eq!(capture_name(ButtonSpace::Standard, 17), None);
        for key in ["Z", "PageUp", "Pad", "Pad17", "Raw-1", "RShift", "P"] {
            assert_eq!(parse_capture_name(key), None, "{key}");
        }
        // No capture name is a key name the keyboard table could hold.
        for (key, _) in crate::input::KEY_NAME_DOM_CODES {
            assert_eq!(parse_capture_name(key), None);
        }
    }

    #[test]
    fn glyphs_follow_the_maker() {
        let ds4 = "054c-09cc-Wireless Controller";
        assert_eq!(GlyphSet::from_name(ds4), GlyphSet::PlayStation);
        assert_eq!(
            GlyphSet::from_name("Sony Interactive Entertainment DualSense Wireless Controller"),
            GlyphSet::PlayStation
        );
        assert_eq!(
            GlyphSet::from_name("057e-2009-Pro Controller"),
            GlyphSet::Nintendo
        );
        assert_eq!(
            GlyphSet::from_name("Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e)"),
            GlyphSet::Xbox
        );
        assert_eq!(GlyphSet::from_name("Generic USB Joystick"), GlyphSet::Xbox);
        let m = GamepadMapping::default();
        let label = |g| m.label_for(ButtonSpace::Standard, PadButton::Cross, g);
        assert_eq!(label(GlyphSet::Xbox).as_deref(), Some("A"));
        assert_eq!(label(GlyphSet::PlayStation).as_deref(), Some("Cross"));
        // Positional: Nintendo's bottom button is B.
        assert_eq!(label(GlyphSet::Nintendo).as_deref(), Some("B"));
        assert_eq!(
            m.label_for(ButtonSpace::Raw, PadButton::Circle, GlyphSet::Xbox)
                .as_deref(),
            Some("Btn 2")
        );
    }

    #[test]
    fn the_dead_zone_moves_the_stick_threshold() {
        let m = GamepadMapping::default();
        let at = |x, dz| m.fold([], [(x, 0.0)], dz);
        assert_eq!(at(0.3, 0.5), 0);
        assert_eq!(at(0.3, 0.25), PadButton::Right.mask());
        let off = GamepadMapping {
            stick_dpad: false,
            ..m.clone()
        };
        assert_eq!(off.fold([], [(1.0, 1.0)], 0.5), 0, "stick off");
    }

    #[test]
    fn raw_hats_are_found_from_the_resting_axes() {
        // Chromium / Windows DirectInput: POV on axis 9, neutral 3.28/7.
        let mut rest = vec![0.0; 10];
        rest[9] = 1.2857;
        let hat = RawHat::detect(&rest);
        assert_eq!(hat, RawHat::Pov(9));
        let mut axes = rest.clone();
        axes[9] = -1.0;
        assert_eq!(hat.dpad(&axes), PadButton::Up.mask());
        axes[9] = -1.0 + 4.0 / 7.0;
        assert_eq!(hat.dpad(&axes), PadButton::Right.mask());
        axes[9] = 1.0;
        assert_eq!(
            hat.dpad(&axes),
            PadButton::Up.mask() | PadButton::Left.mask()
        );
        axes[9] = 0.0;
        assert_eq!(hat.dpad(&axes), 0, "between codes is no direction");
        axes[9] = 1.2857;
        assert_eq!(hat.dpad(&axes), 0, "neutral");
        // Linux joystick API: sticks, triggers, then ABS_HAT0X / Y.
        let rest = [0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0];
        assert_eq!(RawHat::detect(&rest), RawHat::Pair(6, 7));
        let mut axes = rest;
        axes[7] = 1.0;
        assert_eq!(RawHat::Pair(6, 7).dpad(&axes), PadButton::Down.mask());
        // Two sticks and two triggers: the triggers rest at -1, no hat.
        assert_eq!(
            RawHat::detect(&[0.0, 0.0, 0.0, 0.0, -1.0, -1.0]),
            RawHat::None
        );
        assert_eq!(RawHat::detect(&[0.0, 0.0, 0.0, 0.0]), RawHat::None);
    }

    #[test]
    fn a_raw_pad_folds_through_its_own_table_and_hat() {
        let mut m = GamepadMapping::default();
        let rest = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let mut axes = rest;
        axes[4] = -1.0;
        let w = m.fold_pad(ButtonSpace::Raw, &[0.0, 1.0], &axes, &rest, 0.5);
        assert_eq!(w, PadButton::Circle.mask() | PadButton::Left.mask());
        m.bind(ButtonSpace::Raw, 1, PadButton::Triangle);
        let w = m.fold_pad(ButtonSpace::Raw, &[0.0, 1.0], &rest, &rest, 0.5);
        assert_eq!(w, PadButton::Triangle.mask());
        // A standard pad reads no hat off its axes.
        let w = m.fold_pad(ButtonSpace::Standard, &[], &axes, &rest, 0.5);
        assert_eq!(w, 0);
    }

    #[test]
    fn the_table_round_trips_and_a_short_table_still_reads() {
        let mut m = GamepadMapping::default();
        m.bind(ButtonSpace::Standard, 3, PadButton::Cross);
        let toml = toml::to_string(&m).unwrap();
        let back: GamepadMapping = toml::from_str(&toml).unwrap();
        assert_eq!(back, m);
        let short: GamepadMapping = toml::from_str("buttons = [\"Circle\"]").unwrap();
        assert_eq!(
            short.button_for(ButtonSpace::Standard, 0),
            Some(PadButton::Circle)
        );
        assert_eq!(short.button_for(ButtonSpace::Standard, 5), None);
        assert!(short.stick_dpad, "absent keys take their default");
        let mut short = short;
        short.bind(ButtonSpace::Standard, 10, PadButton::L3);
        assert_eq!(short.buttons.len(), STANDARD_BUTTON_COUNT);
    }
}
