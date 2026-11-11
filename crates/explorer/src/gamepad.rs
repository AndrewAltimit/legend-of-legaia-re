//! A controller works Explorer the way the keyboard does: each button is
//! turned into the key a keyboard user would press, and handed to egui with
//! the frame's other input ([`crate::app::App`]'s `raw_input_hook`). So the
//! D-pad walks the lists and moves between controls exactly as the arrow
//! keys do, Cross presses the focused button as `Enter` does, and nothing
//! in the views needs a controller arm of its own.
//!
//! | Button | Key | What it does |
//! |---|---|---|
//! | D-pad, left stick | arrows | walk a list, move between controls, step a frame |
//! | Cross (bottom) | `Enter` | press the focused button |
//! | Circle (right) | `Esc` | close a box, leave a control |
//! | Triangle (top) | `Space` | play or pause |
//! | L1 / R1 | `Shift+Tab` / `Tab` | the previous / next control |
//! | L2 / R2 | `Ctrl+Shift+Tab` / `Ctrl+Tab` | the previous / next preview tab |
//! | Select | `Ctrl+1` .. `Ctrl+4` | the next view |
//! | Start | `F5` | play the open disc |
//! | right stick | mouse wheel | scroll the pane under the pointer |
//!
//! The controllers are read on a thread of their own, which wakes the
//! window only when a button moves (a held direction repeats, as a held
//! arrow key does), so an idle Explorer with a pad plugged in draws nothing.

use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui;

/// What a controller asks of Explorer, in keyboard terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadKey {
    Up,
    Down,
    Left,
    Right,
    /// Cross: `Enter`.
    Activate,
    /// Circle: `Esc`.
    Back,
    /// Triangle: `Space`.
    PlayPause,
    /// L1: `Shift+Tab`.
    PrevControl,
    /// R1: `Tab`.
    NextControl,
    /// L2: `Ctrl+Shift+Tab`.
    PrevTab,
    /// R2: `Ctrl+Tab`.
    NextTab,
    /// Select: the next of the four views.
    NextView,
    /// Start: `F5`.
    Play,
}

impl PadKey {
    /// Whether holding the button repeats it, as a held arrow key does.
    pub fn repeats(self) -> bool {
        matches!(self, Self::Up | Self::Down | Self::Left | Self::Right)
    }
}

/// What the controller thread sends the window.
#[derive(Debug, Clone, PartialEq)]
pub enum PadInput {
    /// A press (or a held direction's repeat).
    Key(PadKey),
    /// The right stick, in scroll points for this step (positive down).
    Scroll(f32),
    Connected(String),
    Disconnected(String),
}

/// The key a gilrs button stands for, by its position on the pad.
pub fn pad_key(button: gilrs::Button) -> Option<PadKey> {
    use gilrs::Button::*;
    Some(match button {
        DPadUp => PadKey::Up,
        DPadDown => PadKey::Down,
        DPadLeft => PadKey::Left,
        DPadRight => PadKey::Right,
        South => PadKey::Activate,
        East => PadKey::Back,
        North => PadKey::PlayPause,
        LeftTrigger => PadKey::PrevControl,
        RightTrigger => PadKey::NextControl,
        LeftTrigger2 => PadKey::PrevTab,
        RightTrigger2 => PadKey::NextTab,
        Select => PadKey::NextView,
        Start => PadKey::Play,
        _ => return None,
    })
}

/// How far the left stick goes before it counts as the D-pad.
pub const STICK_THRESHOLD: f32 = 0.5;

/// The direction the left stick at `(x, y)` (gilrs: up is positive y)
/// presses, the stronger axis winning; `None` inside [`STICK_THRESHOLD`].
pub fn stick_key(x: f32, y: f32) -> Option<PadKey> {
    if x.abs() < STICK_THRESHOLD && y.abs() < STICK_THRESHOLD {
        None
    } else if y.abs() >= x.abs() {
        Some(if y > 0.0 { PadKey::Up } else { PadKey::Down })
    } else {
        Some(if x > 0.0 { PadKey::Right } else { PadKey::Left })
    }
}

/// The key press and release a [`PadKey`] hands egui. `NextView` takes the
/// view after `view` (0 Browse .. 3 Extract) as `Ctrl+1` .. `Ctrl+4`.
pub fn key_events(key: PadKey, view: usize) -> Vec<egui::Event> {
    use egui::{Key, Modifiers};
    let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;
    let (k, m) = match key {
        PadKey::Up => (Key::ArrowUp, Modifiers::NONE),
        PadKey::Down => (Key::ArrowDown, Modifiers::NONE),
        PadKey::Left => (Key::ArrowLeft, Modifiers::NONE),
        PadKey::Right => (Key::ArrowRight, Modifiers::NONE),
        PadKey::Activate => (Key::Enter, Modifiers::NONE),
        PadKey::Back => (Key::Escape, Modifiers::NONE),
        PadKey::PlayPause => (Key::Space, Modifiers::NONE),
        PadKey::PrevControl => (Key::Tab, Modifiers::SHIFT),
        PadKey::NextControl => (Key::Tab, Modifiers::NONE),
        PadKey::PrevTab => (Key::Tab, ctrl_shift),
        PadKey::NextTab => (Key::Tab, Modifiers::COMMAND),
        PadKey::NextView => {
            let next = [Key::Num1, Key::Num2, Key::Num3, Key::Num4][(view + 1) % 4];
            (next, Modifiers::COMMAND)
        }
        PadKey::Play => (Key::F5, Modifiers::NONE),
    };
    [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key {
            key: k,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: m,
        })
        .collect()
}

/// The status line when a controller arrives: its name and the buttons
/// that matter first.
pub fn connected_line(name: &str) -> String {
    format!(
        "Controller connected: {name}. D-pad moves, Cross presses, Circle backs out, \
         L1 / R1 move between controls, Select changes view. The ? box lists every button."
    )
}

/// The rows the shortcuts box adds for a controller.
pub const PAD_ROWS: &[(&str, &str)] = &[
    ("D-pad, left stick", "walk a list, move between controls"),
    ("Cross", "press the focused button (Enter)"),
    ("Circle", "close a box, leave a control (Esc)"),
    ("Triangle", "play or pause (Space)"),
    ("L1 / R1", "previous / next control (Shift+Tab / Tab)"),
    ("L2 / R2", "previous / next preview tab"),
    ("Select", "the next view"),
    ("Start", "play the open disc (F5)"),
    ("Right stick", "scroll the pane under the pointer"),
];

/// When a held direction first repeats, then how often.
const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_EVERY: Duration = Duration::from_millis(110);
/// How often the right stick scrolls while it is held over.
const SCROLL_EVERY: Duration = Duration::from_millis(16);
/// Scroll points per [`SCROLL_EVERY`] step at full tilt.
const SCROLL_SPEED: f32 = 24.0;

/// Start reading the controllers on a thread of their own; `None` when the
/// platform's controller backend cannot start (Explorer then runs on the
/// keyboard and mouse, as it always could). Each input wakes `ctx`.
pub fn spawn(ctx: egui::Context) -> Option<Receiver<PadInput>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("explorer-gamepad".into())
        .spawn(move || match gilrs::Gilrs::new() {
            Ok(g) => {
                let _ = ready_tx.send(true);
                read_loop(g, &tx, &ctx);
            }
            Err(e) => {
                log::info!("gamepad: controller backend unavailable ({e}); keyboard and mouse");
                let _ = ready_tx.send(false);
            }
        })
        .ok()?;
    ready_rx.recv().ok()?.then_some(rx)
}

fn read_loop(mut g: gilrs::Gilrs, tx: &Sender<PadInput>, ctx: &egui::Context) {
    use gilrs::{Axis, EventType};
    // The held repeating key (a D-pad direction, or the stick's) and when
    // it next repeats.
    let mut held: Option<(PadKey, Instant)> = None;
    let mut stick = (0.0_f32, 0.0_f32);
    let mut stick_key_held: Option<PadKey> = None;
    let mut scroll = 0.0_f32;
    loop {
        let now = Instant::now();
        let mut wait = held.map(|(_, at)| at.saturating_duration_since(now));
        if scroll != 0.0 {
            wait = Some(wait.map_or(SCROLL_EVERY, |w| w.min(SCROLL_EVERY)));
        }
        let ev = g.next_event_blocking(wait);
        let mut out: Vec<PadInput> = Vec::new();
        if let Some(ev) = ev {
            match ev.event {
                EventType::Connected => {
                    out.push(PadInput::Connected(g.gamepad(ev.id).name().to_string()))
                }
                EventType::Disconnected => {
                    out.push(PadInput::Disconnected(g.gamepad(ev.id).name().to_string()));
                    held = None;
                }
                EventType::ButtonPressed(b, _) => {
                    if let Some(k) = pad_key(b) {
                        out.push(PadInput::Key(k));
                        if k.repeats() {
                            held = Some((k, Instant::now() + REPEAT_DELAY));
                        }
                    }
                }
                EventType::ButtonReleased(b, _) => {
                    if held.is_some_and(|(k, _)| Some(k) == pad_key(b)) {
                        held = None;
                    }
                }
                EventType::AxisChanged(Axis::LeftStickX, v, _) => stick.0 = v,
                EventType::AxisChanged(Axis::LeftStickY, v, _) => stick.1 = v,
                EventType::AxisChanged(Axis::RightStickY, v, _) => {
                    scroll = if v.abs() < 0.2 { 0.0 } else { -v };
                }
                _ => {}
            }
            // The left stick presses a direction when it crosses the
            // threshold, and lets go of it when it comes back.
            let k = stick_key(stick.0, stick.1);
            if k != stick_key_held {
                if let Some(k) = k {
                    out.push(PadInput::Key(k));
                    held = Some((k, Instant::now() + REPEAT_DELAY));
                } else if held.is_some_and(|(h, _)| Some(h) == stick_key_held) {
                    held = None;
                }
                stick_key_held = k;
            }
        }
        let now = Instant::now();
        if let Some((k, at)) = held
            && now >= at
        {
            out.push(PadInput::Key(k));
            held = Some((k, now + REPEAT_EVERY));
        }
        if scroll != 0.0 && ev.is_none() {
            out.push(PadInput::Scroll(scroll * SCROLL_SPEED));
        }
        if out.is_empty() {
            continue;
        }
        for i in out {
            if tx.send(i).is_err() {
                return;
            }
        }
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_stand_for_the_keys_a_keyboard_user_presses() {
        use gilrs::Button;
        assert_eq!(pad_key(Button::South), Some(PadKey::Activate));
        assert_eq!(pad_key(Button::East), Some(PadKey::Back));
        assert_eq!(pad_key(Button::DPadDown), Some(PadKey::Down));
        assert_eq!(pad_key(Button::Start), Some(PadKey::Play));
        assert_eq!(pad_key(Button::Mode), None);
        let press = |k| match key_events(k, 0).first() {
            Some(egui::Event::Key { key, modifiers, .. }) => (*key, *modifiers),
            other => panic!("{other:?}"),
        };
        assert_eq!(press(PadKey::Activate).0, egui::Key::Enter);
        assert_eq!(press(PadKey::Back).0, egui::Key::Escape);
        assert_eq!(
            press(PadKey::PrevControl),
            (egui::Key::Tab, egui::Modifiers::SHIFT)
        );
        assert_eq!(
            press(PadKey::NextTab),
            (egui::Key::Tab, egui::Modifiers::COMMAND)
        );
        // Each press comes with its release, so nothing reads as held.
        let ev = key_events(PadKey::Down, 0);
        assert!(matches!(
            ev[..],
            [
                egui::Event::Key { pressed: true, .. },
                egui::Event::Key { pressed: false, .. }
            ]
        ));
    }

    #[test]
    fn select_steps_to_the_next_view_and_wraps() {
        let view = |from| match key_events(PadKey::NextView, from).first() {
            Some(egui::Event::Key { key, .. }) => *key,
            other => panic!("{other:?}"),
        };
        assert_eq!(view(0), egui::Key::Num2);
        assert_eq!(view(3), egui::Key::Num1);
    }

    #[test]
    fn the_left_stick_is_the_d_pad_past_its_threshold() {
        assert_eq!(stick_key(0.1, -0.3), None);
        assert_eq!(stick_key(0.0, 0.9), Some(PadKey::Up));
        assert_eq!(stick_key(0.2, -0.7), Some(PadKey::Down));
        assert_eq!(stick_key(-0.8, 0.6), Some(PadKey::Left));
    }

    /// The events reach egui as a keyboard's would: R1 moves the focus to
    /// the next button and Cross presses it.
    #[test]
    fn r1_then_cross_presses_the_next_button() {
        let ctx = egui::Context::default();
        let mut pressed = Vec::new();
        let frame = |events: Vec<egui::Event>, first: bool, pressed: &mut Vec<&str>| {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                for name in ["Open", "Play"] {
                    let r = ui.button(name);
                    if first && name == "Open" {
                        r.request_focus();
                    }
                    if r.clicked() {
                        pressed.push(name);
                    }
                }
            });
        };
        frame(Vec::new(), true, &mut pressed);
        frame(key_events(PadKey::NextControl, 0), false, &mut pressed);
        frame(key_events(PadKey::Activate, 0), false, &mut pressed);
        assert_eq!(pressed, ["Play"]);
    }

    /// Every controller row fits the shortcuts box's key column as the
    /// keyboard rows do: short names, no sentence.
    #[test]
    fn controller_rows_are_short() {
        for (keys, what) in PAD_ROWS {
            assert!(keys.len() <= 18, "{keys}");
            assert!(what.len() <= 48, "{what}");
        }
    }
}
