//! Controller input for the play window.
//!
//! gilrs reports every connected gamepad; this module renames its buttons to
//! the W3C standard-mapping indices and folds them through the shared layout
//! ([`legaia_engine_system::gamepad`]), the one the browser pages fold the
//! Gamepad API through. The window keeps the folded word beside its keyboard
//! word and ORs the two into each tick's pad (`run_sim_ticks`), so a
//! controller reaches every arm a bound key reaches - the boot title, the
//! pause menu, a shop, the field, a battle, a minigame - with no arm of its
//! own.
//!
//! Polled once per display frame, ahead of the frame's ticks, as the pages
//! poll `navigator.getGamepads()` once per animation frame. A press that
//! starts and ends between two polls still reaches a tick through the
//! window's tap latch (`PadTapLatch`, the keyboard's sub-frame rule).
//!
//! Scripted and headless runs open no controller: the pad word there belongs
//! to the script.

use legaia_engine_system::gamepad::{GamepadEdges, StandardGamepad};

/// The W3C standard-mapping index of a gilrs button, or `None` for one the
/// standard layout has no slot for (`Mode`, `C`, `Z`, `Unknown`).
#[cfg(feature = "gamepad")]
pub(crate) fn standard_index(button: gilrs::Button) -> Option<usize> {
    use gilrs::Button::*;
    Some(match button {
        South => 0,
        East => 1,
        West => 2,
        North => 3,
        LeftTrigger => 4,
        RightTrigger => 5,
        LeftTrigger2 => 6,
        RightTrigger2 => 7,
        Select => 8,
        Start => 9,
        LeftThumb => 10,
        RightThumb => 11,
        DPadUp => 12,
        DPadDown => 13,
        DPadLeft => 14,
        DPadRight => 15,
        _ => return None,
    })
}

/// Every gilrs button with a standard-mapping slot, for the per-poll fold.
#[cfg(feature = "gamepad")]
const MAPPED_BUTTONS: [gilrs::Button; 16] = {
    use gilrs::Button::*;
    [
        South,
        East,
        West,
        North,
        LeftTrigger,
        RightTrigger,
        LeftTrigger2,
        RightTrigger2,
        Select,
        Start,
        LeftThumb,
        RightThumb,
        DPadUp,
        DPadDown,
        DPadLeft,
        DPadRight,
    ]
};

/// One poll's result: the edges against the last poll, plus every button
/// pressed since it (including one already released again).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GamepadPoll {
    pub edges: GamepadEdges,
    pub taps: u16,
}

/// Where a [`GamepadHost`] reads its controllers from.
enum Source {
    /// The platform backend.
    #[cfg(feature = "gamepad")]
    Gilrs(Box<gilrs::Gilrs>),
    /// A folded word the caller sets (the headless window's controller,
    /// `HeadlessPlayWindow::set_controller`): the poll, the edges and the
    /// tick composition all run as for a real controller.
    Scripted(u16),
}

/// The window's controller reader.
pub(crate) struct GamepadHost {
    source: Source,
    tracker: StandardGamepad,
}

impl GamepadHost {
    /// Open the platform's controller backend. `None` (logged) when this
    /// build has no gamepad support or the backend cannot start; the window
    /// then runs keyboard-only, as it always could.
    pub(crate) fn open() -> Option<Self> {
        #[cfg(feature = "gamepad")]
        {
            match gilrs::Gilrs::new() {
                Ok(gilrs) => {
                    for (_, gp) in gilrs.gamepads() {
                        log::info!("gamepad: {} connected", gp.name());
                    }
                    Some(Self {
                        source: Source::Gilrs(Box::new(gilrs)),
                        tracker: StandardGamepad::default(),
                    })
                }
                Err(e) => {
                    log::warn!("gamepad: controller backend unavailable ({e}); keyboard only");
                    None
                }
            }
        }
        #[cfg(not(feature = "gamepad"))]
        {
            log::debug!("gamepad: built without the `gamepad` feature; keyboard only");
            None
        }
    }

    /// A controller whose folded word the caller sets ([`Self::set_scripted`]).
    pub(crate) fn scripted() -> Self {
        Self {
            source: Source::Scripted(0),
            tracker: StandardGamepad::default(),
        }
    }

    /// Hold `word` on a [`Self::scripted`] controller from the next poll.
    // Without the `gamepad` feature `Scripted` is the only variant.
    #[allow(irrefutable_let_patterns)]
    pub(crate) fn set_scripted(&mut self, word: u16) {
        if let Source::Scripted(w) = &mut self.source {
            *w = word;
        }
    }

    /// Drain the backend's events and fold every connected controller's
    /// state through the shared layout.
    pub(crate) fn poll(&mut self) -> GamepadPoll {
        let (word, taps) = match &mut self.source {
            #[cfg(feature = "gamepad")]
            Source::Gilrs(gilrs) => poll_gilrs(gilrs),
            Source::Scripted(word) => (*word, 0),
        };
        GamepadPoll {
            edges: self.tracker.update(word),
            taps,
        }
    }

    /// Focus loss: forget the held word, so a button still held re-enters
    /// as a fresh press (the pages' `legaiaResetGamepad`).
    pub(crate) fn reset(&mut self) {
        self.tracker.reset();
    }
}

/// One gilrs poll: the folded word every connected controller holds, and
/// every button pressed since the last poll (a tap shorter than a frame).
#[cfg(feature = "gamepad")]
fn poll_gilrs(gilrs: &mut gilrs::Gilrs) -> (u16, u16) {
    use legaia_engine_system::gamepad::standard_button;
    let mut taps = 0u16;
    while let Some(ev) = gilrs.next_event() {
        match ev.event {
            gilrs::EventType::ButtonPressed(b, _) => {
                if let Some(btn) = standard_index(b).and_then(standard_button) {
                    taps |= btn.mask();
                }
            }
            gilrs::EventType::Connected => {
                let gp = gilrs.gamepad(ev.id);
                log::info!("gamepad: {} connected", gp.name());
            }
            gilrs::EventType::Disconnected => {
                log::info!("gamepad: controller {} disconnected", ev.id);
            }
            _ => {}
        }
    }
    let mut buttons = Vec::with_capacity(16);
    let mut sticks = Vec::new();
    for (_, gp) in gilrs.gamepads() {
        if !gp.is_connected() {
            continue;
        }
        for (i, b) in MAPPED_BUTTONS.iter().enumerate() {
            let value = if gp.is_pressed(*b) {
                1.0
            } else {
                gp.button_data(*b).map_or(0.0, |d| d.value())
            };
            buttons.push((i, value));
        }
        // gilrs' stick Y is up-positive; the layout reads the W3C
        // down-positive axis.
        sticks.push((
            gp.value(gilrs::Axis::LeftStickX),
            -gp.value(gilrs::Axis::LeftStickY),
        ));
    }
    (StandardGamepad::fold(buttons, sticks), taps)
}

#[cfg(all(test, feature = "gamepad"))]
mod tests {
    use super::*;
    use legaia_engine_system::gamepad::{STANDARD_BUTTONS, standard_button};

    #[test]
    fn gilrs_buttons_land_on_the_shared_layout_by_position() {
        for (i, b) in MAPPED_BUTTONS.iter().enumerate() {
            assert_eq!(standard_index(*b), Some(i), "{b:?}");
        }
        assert_eq!(MAPPED_BUTTONS.len(), STANDARD_BUTTONS.len());
        use legaia_engine_system::input::PadButton;
        // A PlayStation pad through gilrs: South is Cross, East Circle,
        // West Square, North Triangle.
        let psx = |b| standard_index(b).and_then(standard_button);
        assert_eq!(psx(gilrs::Button::South), Some(PadButton::Cross));
        assert_eq!(psx(gilrs::Button::East), Some(PadButton::Circle));
        assert_eq!(psx(gilrs::Button::West), Some(PadButton::Square));
        assert_eq!(psx(gilrs::Button::North), Some(PadButton::Triangle));
        assert_eq!(psx(gilrs::Button::Mode), None);
    }
}
