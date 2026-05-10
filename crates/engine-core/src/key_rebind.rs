//! Key rebind session.
//!
//! Drives the per-key rebind sub-screen reachable from
//! [`crate::options::OptionsSession`]: pick a [`PadButton`], then press the
//! desired keyboard key. The session swaps the new key into the
//! [`crate::input::Mapping`] table, evicting the previous binding (if any
//! key was already bound to the same button, it gets cleared so two keys
//! never report the same button).
//!
//! Renderer-agnostic. Engines drive [`KeyRebindSession::tick`] each frame
//! with a [`KeyRebindInput`] bundle (cursor moves + the most-recent key
//! press as a string) and consume the [`KeyRebindEvent`] stream.

use crate::input::{Mapping, PadButton};

/// One row in the rebind list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebindRow {
    pub button: PadButton,
    /// Currently-bound key name (e.g. `"Z"`). Empty if unbound.
    pub key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRebindPhase {
    Browsing {
        cursor: u8,
    },
    /// Waiting for the next keyboard event from the host.
    AwaitingKey {
        cursor: u8,
    },
    Done(KeyRebindOutcome),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRebindOutcome {
    Confirmed,
    Cancelled,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyRebindInput {
    pub up: bool,
    pub down: bool,
    pub cross: bool,
    pub circle: bool,
    pub start: bool,
    /// Most-recent key event from the host, encoded as the key name. The
    /// host shell translates winit `KeyCode` values to these strings.
    /// Only consumed in [`KeyRebindPhase::AwaitingKey`].
    pub key_pressed: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyRebindEvent {
    CursorMoved {
        row: u8,
    },
    EnteredAwaitingKey {
        button: PadButton,
    },
    Bound {
        button: PadButton,
        key: String,
        evicted: Option<String>,
    },
    Cancelled,
    Confirmed,
}

#[derive(Debug, Clone)]
pub struct KeyRebindSession {
    rows: Vec<RebindRow>,
    mapping: Mapping,
    phase: KeyRebindPhase,
}

impl KeyRebindSession {
    /// Build the row list from the current mapping. Rows are emitted in
    /// the canonical retail button order (Cross / Circle / Triangle /
    /// Square / D-pad / shoulders / Start / Select).
    pub fn new(mapping: Mapping) -> Self {
        const ORDER: [PadButton; 16] = [
            PadButton::Cross,
            PadButton::Circle,
            PadButton::Triangle,
            PadButton::Square,
            PadButton::Up,
            PadButton::Down,
            PadButton::Left,
            PadButton::Right,
            PadButton::L1,
            PadButton::R1,
            PadButton::L2,
            PadButton::R2,
            PadButton::Start,
            PadButton::Select,
            PadButton::L3,
            PadButton::R3,
        ];
        let rows = ORDER
            .iter()
            .map(|btn| {
                let key = mapping
                    .bindings
                    .iter()
                    .find(|(_, v)| PadButton::from_name(v) == Some(*btn))
                    .map(|(k, _)| k.clone())
                    .unwrap_or_default();
                RebindRow { button: *btn, key }
            })
            .collect();
        Self {
            rows,
            mapping,
            phase: KeyRebindPhase::Browsing { cursor: 0 },
        }
    }

    pub fn rows(&self) -> &[RebindRow] {
        &self.rows
    }

    pub fn mapping(&self) -> &Mapping {
        &self.mapping
    }

    pub fn phase(&self) -> KeyRebindPhase {
        self.phase
    }

    pub fn cursor(&self) -> u8 {
        match self.phase {
            KeyRebindPhase::Browsing { cursor } => cursor,
            KeyRebindPhase::AwaitingKey { cursor } => cursor,
            _ => 0,
        }
    }

    pub fn is_done(&self) -> bool {
        matches!(self.phase, KeyRebindPhase::Done(_))
    }

    pub fn outcome(&self) -> Option<KeyRebindOutcome> {
        match self.phase {
            KeyRebindPhase::Done(o) => Some(o),
            _ => None,
        }
    }

    fn step(cursor: u8, dir: i8, n: usize) -> u8 {
        if n == 0 {
            return 0;
        }
        ((cursor as i8 + dir).rem_euclid(n as i8)) as u8
    }

    fn rebuild_row(&mut self, button: PadButton) {
        let key = self
            .mapping
            .bindings
            .iter()
            .find(|(_, v)| PadButton::from_name(v) == Some(button))
            .map(|(k, _)| k.clone())
            .unwrap_or_default();
        if let Some(row) = self.rows.iter_mut().find(|r| r.button == button) {
            row.key = key;
        }
    }

    /// Apply a captured key press in [`KeyRebindPhase::AwaitingKey`]:
    /// remove any existing binding for the chosen button, install the
    /// new key→button binding, return `Some(evicted_key)` if a key got
    /// shadowed.
    fn apply_binding(&mut self, button: PadButton, key: &str) -> Option<String> {
        let evicted = {
            let to_remove: Vec<String> = self
                .mapping
                .bindings
                .iter()
                .filter(|(_, v)| PadButton::from_name(v) == Some(button))
                .map(|(k, _)| k.clone())
                .collect();
            let mut e = None;
            for k in &to_remove {
                self.mapping.bindings.remove(k);
                if Some(k.as_str()) != Some(key) {
                    e = Some(k.clone());
                }
            }
            e
        };
        // Remove any other binding currently using `key` (so we don't
        // double-bind a single key to two buttons).
        self.mapping.bindings.remove(key);
        self.mapping
            .bindings
            .insert(key.to_string(), button.name().to_string());
        // Refresh the row that just got rebound, plus any row whose key
        // matched and got shadowed.
        self.rebuild_row(button);
        if let Some(ref e) = evicted {
            // The evicted key may have been another button's binding —
            // refresh every row to be safe; the cost is trivial (16 rows).
            for btn in self.rows.iter().map(|r| r.button).collect::<Vec<_>>() {
                if btn != button {
                    self.rebuild_row(btn);
                }
            }
            let _ = e;
        }
        evicted
    }

    pub fn tick(&mut self, input: KeyRebindInput) -> Vec<KeyRebindEvent> {
        let mut events = Vec::new();
        match self.phase {
            KeyRebindPhase::Browsing { cursor } => {
                if input.start {
                    self.phase = KeyRebindPhase::Done(KeyRebindOutcome::Confirmed);
                    events.push(KeyRebindEvent::Confirmed);
                    return events;
                }
                if input.circle {
                    self.phase = KeyRebindPhase::Done(KeyRebindOutcome::Cancelled);
                    events.push(KeyRebindEvent::Cancelled);
                    return events;
                }
                let n = self.rows.len();
                let mut new_cursor = cursor;
                if input.up {
                    new_cursor = Self::step(cursor, -1, n);
                } else if input.down {
                    new_cursor = Self::step(cursor, 1, n);
                }
                if new_cursor != cursor {
                    self.phase = KeyRebindPhase::Browsing { cursor: new_cursor };
                    events.push(KeyRebindEvent::CursorMoved { row: new_cursor });
                }
                if input.cross
                    && let Some(row) = self.rows.get(new_cursor as usize)
                {
                    self.phase = KeyRebindPhase::AwaitingKey { cursor: new_cursor };
                    events.push(KeyRebindEvent::EnteredAwaitingKey { button: row.button });
                }
            }
            KeyRebindPhase::AwaitingKey { cursor } => {
                if input.circle {
                    // Cancel the rebind, drop back to Browsing.
                    self.phase = KeyRebindPhase::Browsing { cursor };
                    return events;
                }
                let Some(key) = input.key_pressed.clone() else {
                    return events;
                };
                let Some(row) = self.rows.get(cursor as usize).cloned() else {
                    self.phase = KeyRebindPhase::Browsing { cursor };
                    return events;
                };
                let evicted = self.apply_binding(row.button, &key);
                self.phase = KeyRebindPhase::Browsing { cursor };
                events.push(KeyRebindEvent::Bound {
                    button: row.button,
                    key,
                    evicted,
                });
            }
            KeyRebindPhase::Done(_) => {}
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_match_default_mapping() {
        let m = Mapping::default();
        let s = KeyRebindSession::new(m);
        let cross_row = s
            .rows()
            .iter()
            .find(|r| r.button == PadButton::Cross)
            .unwrap();
        assert_eq!(cross_row.key, "Z");
    }

    #[test]
    fn cursor_moves_and_enters_awaiting() {
        let m = Mapping::default();
        let mut s = KeyRebindSession::new(m);
        let _ = s.tick(KeyRebindInput {
            cross: true,
            ..Default::default()
        });
        assert!(matches!(s.phase, KeyRebindPhase::AwaitingKey { cursor: 0 }));
    }

    #[test]
    fn binding_replaces_evicts_old_key() {
        let m = Mapping::default();
        let mut s = KeyRebindSession::new(m);
        // Rebind Cross from "Z" to "K".
        let _ = s.tick(KeyRebindInput {
            cross: true,
            ..Default::default()
        });
        let evs = s.tick(KeyRebindInput {
            key_pressed: Some("K".into()),
            ..Default::default()
        });
        assert_eq!(s.mapping().pad_button_for_key("K"), Some(PadButton::Cross));
        // Old Z binding cleared.
        assert_eq!(s.mapping().pad_button_for_key("Z"), None);
        assert!(evs.iter().any(|e| matches!(
            e,
            KeyRebindEvent::Bound {
                button: PadButton::Cross,
                ..
            }
        )));
    }

    #[test]
    fn rebind_swaps_keys_when_target_was_in_use() {
        let m = Mapping::default();
        let mut s = KeyRebindSession::new(m);
        // Cursor at Cross (idx 0). Rebind Cross → "X" (which was Square).
        let _ = s.tick(KeyRebindInput {
            cross: true,
            ..Default::default()
        });
        let _ = s.tick(KeyRebindInput {
            key_pressed: Some("X".into()),
            ..Default::default()
        });
        assert_eq!(s.mapping().pad_button_for_key("X"), Some(PadButton::Cross));
        // Square row now has empty key.
        let square_row = s
            .rows()
            .iter()
            .find(|r| r.button == PadButton::Square)
            .unwrap();
        assert_eq!(square_row.key, "");
    }

    #[test]
    fn circle_in_awaiting_returns_to_browsing_no_change() {
        let m = Mapping::default();
        let mut s = KeyRebindSession::new(m);
        let _ = s.tick(KeyRebindInput {
            cross: true,
            ..Default::default()
        });
        let _ = s.tick(KeyRebindInput {
            circle: true,
            ..Default::default()
        });
        assert!(matches!(s.phase, KeyRebindPhase::Browsing { cursor: 0 }));
        assert_eq!(s.mapping().pad_button_for_key("Z"), Some(PadButton::Cross));
    }

    #[test]
    fn start_confirms_browsing() {
        let m = Mapping::default();
        let mut s = KeyRebindSession::new(m);
        let _ = s.tick(KeyRebindInput {
            start: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(KeyRebindOutcome::Confirmed));
    }

    #[test]
    fn circle_in_browsing_cancels() {
        let m = Mapping::default();
        let mut s = KeyRebindSession::new(m);
        let _ = s.tick(KeyRebindInput {
            circle: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(KeyRebindOutcome::Cancelled));
    }
}
