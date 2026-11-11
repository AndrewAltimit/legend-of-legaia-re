//! Controller input for the play window (and the launcher's screen, which
//! moves its focus with the same folded word).
//!
//! gilrs reports every connected gamepad; this module renames its buttons to
//! the W3C standard-mapping indices and folds them through the player's
//! controller table ([`legaia_engine_system::gamepad::GamepadMapping`], the
//! `[gamepad]` half of `legaia-input.toml`, [`GamepadHost::set_mapping`]) at
//! the player's stick dead zone ([`GamepadHost::set_dead_zone`]) - the fold
//! the browser pages run the Gamepad API through. Every connected controller
//! drives the one player: their words are ORed, so a second pad picked up
//! mid-game works at once, and a pad that leaves takes only its own buttons
//! with it. Each poll also reports which controllers came and went (the
//! window's notice line names them) and the first standard index pressed,
//! as a Key Config capture name. The window keeps the folded word beside its keyboard
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

use legaia_engine_system::gamepad::{
    ButtonSpace, GamepadEdges, GamepadMapping, GlyphSet, STICK_DEAD_ZONE, StandardGamepad,
    capture_name,
};

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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GamepadPoll {
    pub edges: GamepadEdges,
    pub taps: u16,
    /// The first physical button pressed since the last poll, as a Key
    /// Config capture name (`"Pad3"`), and the maker's labels of the pad it
    /// was on.
    pub capture: Option<(&'static str, GlyphSet)>,
    /// Controllers that appeared / left since the last poll, by name.
    pub connected: Vec<String>,
    pub disconnected: Vec<String>,
}

/// What one backend poll read, before the edge tracker.
#[derive(Default)]
struct RawPoll {
    word: u16,
    taps: u16,
    capture: Option<(&'static str, GlyphSet)>,
    connected: Vec<String>,
    disconnected: Vec<String>,
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
    /// Physical buttons the caller holds, as a bit per standard index, and a
    /// left stick: folded through the live table like a real controller's,
    /// so a rebind and the dead zone reach it.
    ScriptedButtons {
        held: u32,
        stick: (f32, f32),
        prev: u32,
    },
}

/// The window's controller reader.
pub struct GamepadHost {
    source: Source,
    tracker: StandardGamepad,
    /// The table the poll folds through, and the stick's dead zone.
    mapping: GamepadMapping,
    dead_zone: f32,
    /// The two force-feedback effects (`[strong, weak]`) playing at full
    /// magnitude on every rumble-capable controller, their gains the motors'
    /// levels (`Self::set_rumble`). Built on the first rumble.
    #[cfg(feature = "gamepad")]
    rumble: Option<[gilrs::ff::Effect; 2]>,
    /// The levels last handed to the effects.
    rumble_last: legaia_engine_system::rumble::RumbleFrame,
}

impl GamepadHost {
    /// Open the platform's controller backend. `None` (logged) when this
    /// build has no gamepad support or the backend cannot start; the window
    /// then runs keyboard-only, as it always could.
    pub fn open() -> Option<Self> {
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
                        mapping: GamepadMapping::default(),
                        dead_zone: STICK_DEAD_ZONE,
                        rumble: None,
                        rumble_last: Default::default(),
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
            mapping: GamepadMapping::default(),
            dead_zone: STICK_DEAD_ZONE,
            #[cfg(feature = "gamepad")]
            rumble: None,
            rumble_last: Default::default(),
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

    /// Hold `held` (a bit per W3C standard index) and `stick` on a
    /// [`Self::scripted`] controller from the next poll, as physical
    /// buttons: the poll folds them through the live table.
    pub(crate) fn set_scripted_buttons(&mut self, held: u32, stick: (f32, f32)) {
        match &mut self.source {
            Source::ScriptedButtons {
                held: h, stick: s, ..
            } => {
                *h = held;
                *s = stick;
            }
            other => {
                *other = Source::ScriptedButtons {
                    held,
                    stick,
                    prev: 0,
                }
            }
        }
    }

    /// Fold through `mapping` from the next poll (the window's live table,
    /// re-sent after a Key Config bind).
    pub fn set_mapping(&mut self, mapping: &GamepadMapping) {
        if self.mapping != *mapping {
            self.mapping = mapping.clone();
        }
    }

    /// The left stick holds a d-pad direction past `fraction` of full
    /// deflection (`OptionsState::stick_dead_zone_fraction`).
    pub fn set_dead_zone(&mut self, fraction: f32) {
        self.dead_zone = fraction;
    }

    /// Drain the backend's events and fold every connected controller's
    /// state through the live table.
    pub fn poll(&mut self) -> GamepadPoll {
        let raw = match &mut self.source {
            #[cfg(feature = "gamepad")]
            Source::Gilrs(gilrs) => poll_gilrs(gilrs, &self.mapping, self.dead_zone),
            Source::Scripted(word) => RawPoll {
                word: *word,
                ..Default::default()
            },
            Source::ScriptedButtons { held, stick, prev } => {
                let buttons = (0..32usize)
                    .filter(|i| *held & (1 << i) != 0)
                    .map(|i| (i, 1.0));
                let word = self.mapping.fold(buttons, [*stick], self.dead_zone);
                let new = *held & !*prev;
                *prev = *held;
                let capture = (new != 0)
                    .then(|| capture_name(ButtonSpace::Standard, new.trailing_zeros() as usize))
                    .flatten()
                    .map(|n| (n, GlyphSet::default()));
                RawPoll {
                    word,
                    capture,
                    ..Default::default()
                }
            }
        };
        GamepadPoll {
            edges: self.tracker.update(raw.word),
            taps: raw.taps,
            capture: raw.capture,
            connected: raw.connected,
            disconnected: raw.disconnected,
        }
    }

    /// Drive the controllers' motors at `frame`'s levels (the session's
    /// `BootSession::rumble_frame`, once per redraw after its ticks). A
    /// scripted controller has no motors; it only records the levels.
    pub(crate) fn set_rumble(&mut self, frame: legaia_engine_system::rumble::RumbleFrame) {
        let close = |a: f32, b: f32| (a - b).abs() < 1.0 / 64.0;
        if close(frame.strong, self.rumble_last.strong) && close(frame.weak, self.rumble_last.weak)
        {
            return;
        }
        self.rumble_last = frame;
        #[cfg(feature = "gamepad")]
        if let Source::Gilrs(gilrs) = &mut self.source {
            if self.rumble.is_none() && frame.is_active() {
                self.rumble = rumble_effects(gilrs);
            }
            if let Some([strong, weak]) = &self.rumble {
                let _ = strong.set_gain(frame.strong);
                let _ = weak.set_gain(frame.weak);
            }
        }
    }

    /// The levels [`Self::set_rumble`] last applied.
    #[cfg(test)]
    pub(crate) fn rumble_levels(&self) -> legaia_engine_system::rumble::RumbleFrame {
        self.rumble_last
    }

    /// Focus loss: forget the held word, so a button still held re-enters
    /// as a fresh press (the pages' `legaiaResetGamepad`).
    pub fn reset(&mut self) {
        self.tracker.reset();
    }
}

/// The `[strong, weak]` pair of endless full-magnitude effects on every
/// connected controller that supports force feedback, playing at gain `0`
/// until [`GamepadHost::set_rumble`] raises them. `None` when no controller
/// can rumble (logged once by gilrs' own refusal).
#[cfg(feature = "gamepad")]
fn rumble_effects(gilrs: &mut gilrs::Gilrs) -> Option<[gilrs::ff::Effect; 2]> {
    use gilrs::ff::{BaseEffect, BaseEffectType, EffectBuilder, Replay, Ticks};
    let ids: Vec<gilrs::GamepadId> = gilrs
        .gamepads()
        .filter(|(_, gp)| gp.is_connected() && gp.is_ff_supported())
        .map(|(id, _)| id)
        .collect();
    if ids.is_empty() {
        return None;
    }
    let mut build = |kind| {
        let effect = EffectBuilder::new()
            .add_effect(BaseEffect {
                kind,
                scheduling: Replay {
                    play_for: Ticks::from_ms(1000),
                    ..Default::default()
                },
                ..Default::default()
            })
            .gamepads(&ids)
            .finish(gilrs)
            .map_err(|e| log::warn!("gamepad: rumble unavailable ({e})"))
            .ok()?;
        let _ = effect.set_gain(0.0);
        effect.play().ok()?;
        Some(effect)
    };
    let strong = build(BaseEffectType::Strong {
        magnitude: u16::MAX,
    })?;
    let weak = build(BaseEffectType::Weak {
        magnitude: u16::MAX,
    })?;
    Some([strong, weak])
}

/// One gilrs poll: the word every connected controller holds through
/// `mapping`, every button pressed since the last poll (a tap shorter than a
/// frame), the first such press as a capture name, and the controllers that
/// came and went.
#[cfg(feature = "gamepad")]
fn poll_gilrs(gilrs: &mut gilrs::Gilrs, mapping: &GamepadMapping, dead_zone: f32) -> RawPoll {
    let mut out = RawPoll::default();
    while let Some(ev) = gilrs.next_event() {
        match ev.event {
            gilrs::EventType::ButtonPressed(b, _) => {
                if let Some(index) = standard_index(b) {
                    if let Some(btn) = mapping.button_for(ButtonSpace::Standard, index) {
                        out.taps |= btn.mask();
                    }
                    if out.capture.is_none() {
                        let glyphs = GlyphSet::from_name(gilrs.gamepad(ev.id).name());
                        out.capture =
                            capture_name(ButtonSpace::Standard, index).map(|n| (n, glyphs));
                    }
                }
            }
            gilrs::EventType::Connected => {
                let name = gilrs.gamepad(ev.id).name().to_string();
                log::info!("gamepad: {name} connected");
                out.connected.push(name);
            }
            gilrs::EventType::Disconnected => {
                let name = gilrs.gamepad(ev.id).name().to_string();
                log::info!("gamepad: {name} disconnected");
                out.disconnected.push(name);
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
    out.word = mapping.fold(buttons, sticks, dead_zone);
    out
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

#[cfg(test)]
mod rumble_tests {
    use super::*;
    use legaia_engine_system::rumble::RumbleFrame;

    /// The window hands every frame's levels on, and skips a frame whose
    /// levels have not moved.
    #[test]
    fn the_host_takes_the_session_rumble_levels() {
        let mut gp = GamepadHost::scripted();
        let on = RumbleFrame {
            strong: 0.75,
            weak: 0.5,
        };
        gp.set_rumble(on);
        assert_eq!(gp.rumble_levels(), on);
        gp.set_rumble(RumbleFrame {
            strong: 0.751,
            ..on
        });
        assert_eq!(gp.rumble_levels(), on, "a sub-step change is not re-sent");
        gp.set_rumble(RumbleFrame::default());
        assert!(!gp.rumble_levels().is_active());
    }
}
