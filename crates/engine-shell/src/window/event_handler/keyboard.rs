//! `KeyboardInput` window-event handler: the `Escape` quit, then
//! [`PlayWindowApp::handle_key`]'s ordered key arms, ending in the pad
//! mapping.

use super::super::*;

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
        if matches!(code, KeyCode::Escape) && state == ElementState::Pressed {
            if let Some(log) = self.record_log.as_mut()
                && let Err(e) = log.flush()
            {
                log::error!("record: flush on Escape failed: {e:#}");
            }
            evl.exit();
            return;
        }
        self.handle_key(code, state);
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
        self.latch_key_name(code, state);
        if self.debug_spawn_key(code, state)
            || self.minigame_key(code, state)
            || self.option_key(code, state)
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
