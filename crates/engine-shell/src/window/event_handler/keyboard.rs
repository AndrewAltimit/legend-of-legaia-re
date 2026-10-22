//! `KeyboardInput` window-event handler: the `Escape` quit, then
//! [`PlayWindowApp::handle_key`]'s ordered key arms, ending in the pad
//! mapping.

use super::super::*;
use crate::window::app_state::escape_quits;

#[path = "keyboard/debug_spawn_keys.rs"]
mod debug_spawn_keys;
#[path = "keyboard/minigame_keys.rs"]
mod minigame_keys;
#[path = "keyboard/option_keys.rs"]
mod option_keys;

impl PlayWindowApp {
    pub(super) fn handle_keyboard(
        &mut self,
        evl: &ActiveEventLoop,
        code: KeyCode,
        state: ElementState,
    ) {
        if matches!(code, KeyCode::Escape) {
            // Held keys repeat as a stream of `Pressed` events; only the
            // leading edge counts, or holding Escape would confirm itself.
            match state {
                ElementState::Released => {
                    self.keys_down.remove(&code);
                }
                ElementState::Pressed if self.keys_down.insert(code) => {
                    let now = std::time::Instant::now();
                    if escape_quits(self.quit_prompt, now) {
                        if let Some(log) = self.record_log.as_mut()
                            && let Err(e) = log.flush()
                        {
                            log::error!("record: flush on Escape failed: {e:#}");
                        }
                        evl.exit();
                    } else {
                        log::info!("Escape: press it again to quit");
                        self.quit_prompt = Some(now);
                    }
                }
                ElementState::Pressed => {}
            }
            return;
        }
        if self.fullscreen_key(code, state) {
            return;
        }
        self.handle_key(code, state);
    }

    /// `F11`, or `Alt+Enter`: borderless fullscreen on or off, remembered
    /// in the options file. Window chrome on keys outside the pad bindings
    /// (Enter alone stays Start). Only the leading edge toggles.
    fn fullscreen_key(&mut self, code: KeyCode, state: ElementState) -> bool {
        let enter = matches!(code, KeyCode::Enter | KeyCode::NumpadEnter);
        let ours = code == KeyCode::F11 || (enter && (self.alt_held || self.fullscreen_key_held));
        if !ours {
            return false;
        }
        match state {
            ElementState::Pressed if !self.fullscreen_key_held => {
                self.fullscreen_key_held = true;
                let on = !self.win.is_fullscreen();
                self.win.set_fullscreen(on);
                self.options_state.fullscreen = on;
                self.persist_and_apply_options();
                self.show_notice(format!("Fullscreen: {}", if on { "on" } else { "off" }));
            }
            ElementState::Released => self.fullscreen_key_held = false,
            ElementState::Pressed => {}
        }
        true
    }

    /// The window to open: the launch flags over the persisted options. A
    /// `--screenshot` run ignores the persisted pair, so a capture's surface
    /// never depends on what a player last chose.
    pub(in crate::window) fn window_setup(&self) -> legaia_engine_render::window::WindowSetup {
        use legaia_engine_render::window::WindowSetup;
        let persisted = self.screenshot.is_none();
        let fullscreen = self
            .window_launch
            .fullscreen
            .unwrap_or(persisted && self.options_state.fullscreen);
        let scale = self.window_launch.scale.unwrap_or(if persisted {
            self.options_state.window_scale
        } else {
            WindowSetup::DEFAULT_SCALE
        });
        WindowSetup::scaled(scale, fullscreen)
    }

    /// Every key arm except the `Escape` quit, which is the only one that
    /// needs the event loop.
    ///
    /// Split out so the `--key-script` harness can deliver the same events a
    /// player's keyboard does without an `ActiveEventLoop` in hand: the
    /// scripted injection runs inside the per-tick loop, where no winit event
    /// is being dispatched. Everything reachable only from the keyboard - the
    /// minigame entries, the fishing prize exchange, the dialogue option
    /// picker - is therefore scriptable, which `--pad-script` alone cannot do
    /// (it writes the pad word and this function never runs).
    ///
    /// The arms run in this order and the first to consume the key ends the
    /// dispatch: an arm returns before the pad lookup, so a window arm
    /// sitting on a bound key deletes that button from the keyboard (see
    /// `keycode_to_name`).
    pub(in crate::window) fn handle_key(&mut self, code: KeyCode, state: ElementState) {
        // Read before the latch records this press: a press of a key that
        // is already down is the OS auto-repeat, not a new press.
        let repeat = state == ElementState::Pressed && self.keys_down.contains(&code);
        self.latch_key_name(code, state);
        if self.debug_spawn_key(code, state)
            || self.minigame_key(code, state)
            || self.option_key(code, state, repeat)
            || self.help_key(code, state)
            || self.key_name_entry(code, state)
        {
            return;
        }
        self.key_pad_button(code, state);
    }

    /// One latched key name per physical press, for the pause menu's Key
    /// Config screen. Held keys repeat as a stream of `Pressed` events, so
    /// the set below collapses each stream to its leading edge - without
    /// it, holding the confirm key would rebind the highlighted row to
    /// that key the moment the screen opened.
    fn latch_key_name(&mut self, code: KeyCode, state: ElementState) {
        match state {
            ElementState::Pressed => {
                if self.keys_down.insert(code) {
                    let name = keycode_to_name(code);
                    if !name.is_empty() {
                        self.pending_key_name = Some(name);
                    }
                }
            }
            ElementState::Released => {
                self.keys_down.remove(&code);
            }
        }
    }

    /// `N`: open the name-entry overlay for the lead character. The
    /// NEW GAME flow opens it automatically when the `town01` opening
    /// timeline reaches its op-`0x49` STATE_RESUME (P2[3] body
    /// `0x02c6`); this key is a dev hand-trigger to exercise the
    /// ported overlay outside that flow.
    fn key_name_entry(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::KeyN)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
            && !self.session.host.world.name_entry_active()
        {
            self.session.host.world.open_name_entry(0);
            return true;
        }
        false
    }

    /// The pad mapping: a key bound to a pad button holds or releases its
    /// bit, and a transition is recorded for a replay.
    fn key_pad_button(&mut self, code: KeyCode, state: ElementState) {
        let key_name = keycode_to_name(code);
        if let Some(button) = self.mapping.pad_button_for_key(key_name) {
            let prev = self.pad;
            if state == ElementState::Pressed {
                self.pad |= button.mask();
                self.pad_taps.press(button.mask());
            } else {
                self.pad &= !button.mask();
            }
            // Record the transition iff the pad actually changed
            // (auto-repeat sends a stream of Pressed events with
            // identical mask; dedup in RecordLog::record_transition).
            if self.pad != prev
                && let Some(log) = self.record_log.as_mut()
            {
                log.record_transition(self.session.frames, self.pad | self.gamepad_held);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::app_state::QUIT_CONFIRM_WINDOW;
    use std::time::{Duration, Instant};

    /// The help keys take no pad button off the keyboard: neither is a
    /// bindable key name, on this host or the page.
    #[test]
    fn help_keys_are_not_bindable() {
        for code in [KeyCode::F12, KeyCode::Slash] {
            assert_eq!(keycode_to_name(code), "");
            assert!(
                legaia_engine_system::input::key_name_for_dom_code(&format!("{code:?}")).is_none()
            );
            assert!(
                legaia_engine_session::hotkeys::HostHotkey::from_dom_code(&format!("{code:?}"))
                    .is_none()
            );
        }
    }

    #[test]
    fn escape_quits_only_on_a_second_press_inside_the_window() {
        let t0 = Instant::now();
        assert!(!escape_quits(None, t0));
        assert!(escape_quits(Some(t0), t0 + Duration::from_secs(1)));
        assert!(escape_quits(Some(t0), t0 + QUIT_CONFIRM_WINDOW));
        assert!(!escape_quits(
            Some(t0),
            t0 + QUIT_CONFIRM_WINDOW + Duration::from_millis(1)
        ));
    }

    #[test]
    fn startup_hint_shows_only_when_armed_and_times_from_first_draw() {
        use crate::window::app_state::{STARTUP_HINT_DURATION, StartupHint};
        let t0 = Instant::now();
        // Unarmed (a capture, the headless window): never drawn.
        let quiet = StartupHint::default();
        assert!(!quiet.visible(t0));
        assert!(!quiet.visible(t0 + Duration::from_secs(1)));
        // Armed: the clock starts at the first frame that asks.
        let mut hint = StartupHint::default();
        hint.arm();
        let first = t0 + Duration::from_secs(30);
        assert!(hint.visible(first));
        assert!(hint.visible(first + STARTUP_HINT_DURATION));
        assert!(!hint.visible(first + STARTUP_HINT_DURATION + Duration::from_millis(1)));
        // Opening the help panel ends it early.
        let mut hint = StartupHint::default();
        hint.arm();
        assert!(hint.visible(t0));
        hint.dismiss();
        assert!(!hint.visible(t0 + Duration::from_secs(1)));
    }
}
