//! Gamepad -> PSX pad, the one layout every host reads.
//!
//! A gamepad is a *controller*, not a key layout: it binds buttons to buttons
//! by physical position, so the Key Config table (which maps keyboard keys,
//! [`crate::input::Mapping`]) never reaches it. Every host folds a controller
//! through [`StandardGamepad`]:
//!
//! - the native window hands it the buttons gilrs reports, renamed to the
//!   W3C standard-mapping index ([`StandardGamepad::fold`]);
//! - the browser pages read `navigator.getGamepads()` and fold the same
//!   indices through the table this module serves as JSON
//!   (`gamepad_layout_json` in `web-viewer`), so no page writes the table
//!   down a second time.
//!
//! The layout is the W3C "standard" gamepad mapping: the bottom / right /
//! left / top face buttons are Cross / Circle / Square / Triangle, the bumpers
//! L1 / R1, the triggers L2 / R2, back / start Select / Start, the stick
//! clicks L3 / R3, then the d-pad. The left stick drives the d-pad as well,
//! past [`STICK_DEAD_ZONE`]. The PSX pad has no analog fold-in on any host
//! (see `InputState::set_pad_reports`), so the stick is digital here.

use crate::pad::PadButton;

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
    let mut mask = 0;
    if x <= -STICK_DEAD_ZONE {
        mask |= PadButton::Left.mask();
    }
    if x >= STICK_DEAD_ZONE {
        mask |= PadButton::Right.mask();
    }
    if y <= -STICK_DEAD_ZONE {
        mask |= PadButton::Up.mask();
    }
    if y >= STICK_DEAD_ZONE {
        mask |= PadButton::Down.mask();
    }
    mask
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
}
