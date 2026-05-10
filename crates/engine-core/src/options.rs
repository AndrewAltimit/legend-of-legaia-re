//! Options / config screen.
//!
//! Surfaces the retail config menu's settings as a typed [`OptionsState`]
//! that engines persist alongside [`crate::input::Mapping`]: BGM volume,
//! SFX volume, message speed, vibration on/off, audio stereo/mono.
//! The session never touches disk; engines wire `serde` round-trip
//! through their own settings file.
//!
//! ## States
//!
//! `Browsing { cursor } → (in-place edit on each row) → Done`

use crate::input::PadButton;
use serde::{Deserialize, Serialize};

/// Audio stereo / mono toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AudioMode {
    #[default]
    Stereo,
    Mono,
}

impl AudioMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stereo => "Stereo",
            Self::Mono => "Mono",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Stereo => Self::Mono,
            Self::Mono => Self::Stereo,
        }
    }
}

/// Full set of user-editable options. Engines round-trip via
/// `serde_json` / `toml` whichever they prefer; the in-engine API is
/// the same.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionsState {
    /// 0..=10. Engines convert to their per-channel scalar.
    pub bgm_volume: u8,
    /// 0..=10. Engines convert to their per-channel scalar.
    pub sfx_volume: u8,
    /// 1..=8 (1 = slowest). Wired to dialog auto-advance interval.
    pub message_speed: u8,
    pub vibration: bool,
    pub audio: AudioMode,
}

impl Default for OptionsState {
    fn default() -> Self {
        Self {
            bgm_volume: 8,
            sfx_volume: 8,
            message_speed: 5,
            vibration: true,
            audio: AudioMode::Stereo,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsRow {
    BgmVolume,
    SfxVolume,
    MessageSpeed,
    Vibration,
    Audio,
}

impl OptionsRow {
    pub const ALL: [Self; 5] = [
        Self::BgmVolume,
        Self::SfxVolume,
        Self::MessageSpeed,
        Self::Vibration,
        Self::Audio,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::BgmVolume => "BGM Volume",
            Self::SfxVolume => "SFX Volume",
            Self::MessageSpeed => "Message Speed",
            Self::Vibration => "Vibration",
            Self::Audio => "Audio",
        }
    }

    pub fn from_index(idx: u8) -> Option<Self> {
        Self::ALL.get(idx as usize).copied()
    }

    pub fn index(self) -> u8 {
        Self::ALL.iter().position(|r| *r == self).unwrap() as u8
    }
}

/// Phase of the SM. Edits are in-place — there is no separate "edit"
/// state; left/right adjusts the current row's value, up/down moves the
/// cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsPhase {
    Browsing { cursor: u8 },
    Done(OptionsOutcome),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsOutcome {
    /// Player accepted the changes (Cross or Start).
    Confirmed,
    /// Player cancelled out without saving (Circle without prior change is
    /// treated as Confirmed-without-changes; engines compare against the
    /// initial snapshot if they want a true "discard" path).
    Cancelled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OptionsInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub cross: bool,
    pub circle: bool,
    pub start: bool,
}

impl OptionsInput {
    pub fn from_pad_edge(pressed: u16) -> Self {
        Self {
            up: pressed & PadButton::Up.mask() != 0,
            down: pressed & PadButton::Down.mask() != 0,
            left: pressed & PadButton::Left.mask() != 0,
            right: pressed & PadButton::Right.mask() != 0,
            cross: pressed & PadButton::Cross.mask() != 0,
            circle: pressed & PadButton::Circle.mask() != 0,
            start: pressed & PadButton::Start.mask() != 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsEvent {
    CursorMoved { row: u8 },
    ValueChanged { row: OptionsRow },
    Confirmed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct OptionsSession {
    state: OptionsState,
    initial: OptionsState,
    phase: OptionsPhase,
}

impl OptionsSession {
    pub fn new(initial: OptionsState) -> Self {
        Self {
            initial: initial.clone(),
            state: initial,
            phase: OptionsPhase::Browsing { cursor: 0 },
        }
    }

    pub fn state(&self) -> &OptionsState {
        &self.state
    }

    pub fn cursor(&self) -> u8 {
        match self.phase {
            OptionsPhase::Browsing { cursor } => cursor,
            _ => 0,
        }
    }

    pub fn phase(&self) -> OptionsPhase {
        self.phase
    }

    pub fn is_done(&self) -> bool {
        matches!(self.phase, OptionsPhase::Done(_))
    }

    pub fn outcome(&self) -> Option<OptionsOutcome> {
        match self.phase {
            OptionsPhase::Done(o) => Some(o),
            _ => None,
        }
    }

    pub fn revert_if_cancelled(&mut self) {
        if let Some(OptionsOutcome::Cancelled) = self.outcome() {
            self.state = self.initial.clone();
        }
    }

    fn step(cursor: u8, dir: i8) -> u8 {
        let n = OptionsRow::ALL.len() as i8;
        ((cursor as i8 + dir).rem_euclid(n)) as u8
    }

    fn adjust_row(&mut self, row: OptionsRow, dir: i8) -> bool {
        match row {
            OptionsRow::BgmVolume => {
                let new = (self.state.bgm_volume as i8 + dir).clamp(0, 10) as u8;
                if new != self.state.bgm_volume {
                    self.state.bgm_volume = new;
                    return true;
                }
            }
            OptionsRow::SfxVolume => {
                let new = (self.state.sfx_volume as i8 + dir).clamp(0, 10) as u8;
                if new != self.state.sfx_volume {
                    self.state.sfx_volume = new;
                    return true;
                }
            }
            OptionsRow::MessageSpeed => {
                let new = (self.state.message_speed as i8 + dir).clamp(1, 8) as u8;
                if new != self.state.message_speed {
                    self.state.message_speed = new;
                    return true;
                }
            }
            OptionsRow::Vibration => {
                self.state.vibration = !self.state.vibration;
                return true;
            }
            OptionsRow::Audio => {
                self.state.audio = self.state.audio.toggle();
                return true;
            }
        }
        false
    }

    pub fn tick(&mut self, input: OptionsInput) -> Vec<OptionsEvent> {
        let mut events = Vec::new();
        if let OptionsPhase::Browsing { cursor } = self.phase {
            if input.start || input.cross {
                self.phase = OptionsPhase::Done(OptionsOutcome::Confirmed);
                events.push(OptionsEvent::Confirmed);
                return events;
            }
            if input.circle {
                self.phase = OptionsPhase::Done(OptionsOutcome::Cancelled);
                events.push(OptionsEvent::Cancelled);
                return events;
            }
            let mut new_cursor = cursor;
            if input.up {
                new_cursor = Self::step(cursor, -1);
            } else if input.down {
                new_cursor = Self::step(cursor, 1);
            }
            if new_cursor != cursor {
                self.phase = OptionsPhase::Browsing { cursor: new_cursor };
                events.push(OptionsEvent::CursorMoved { row: new_cursor });
            }
            let row = OptionsRow::from_index(new_cursor).unwrap_or(OptionsRow::BgmVolume);
            let dir = if input.left {
                -1
            } else if input.right {
                1
            } else {
                0
            };
            if (dir != 0
                || (matches!(row, OptionsRow::Vibration | OptionsRow::Audio) && input.left))
                && self.adjust_row(row, dir)
            {
                events.push(OptionsEvent::ValueChanged { row });
            }
        }
        events
    }
}

/// Plain-data view for the renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionsRowView {
    pub row: OptionsRow,
    pub label: &'static str,
    pub value: String,
}

impl OptionsState {
    pub fn rows(&self) -> [OptionsRowView; 5] {
        [
            OptionsRowView {
                row: OptionsRow::BgmVolume,
                label: OptionsRow::BgmVolume.label(),
                value: format!("{}/10", self.bgm_volume),
            },
            OptionsRowView {
                row: OptionsRow::SfxVolume,
                label: OptionsRow::SfxVolume.label(),
                value: format!("{}/10", self.sfx_volume),
            },
            OptionsRowView {
                row: OptionsRow::MessageSpeed,
                label: OptionsRow::MessageSpeed.label(),
                value: format!("{}/8", self.message_speed),
            },
            OptionsRowView {
                row: OptionsRow::Vibration,
                label: OptionsRow::Vibration.label(),
                value: if self.vibration {
                    "On".into()
                } else {
                    "Off".into()
                },
            },
            OptionsRowView {
                row: OptionsRow::Audio,
                label: OptionsRow::Audio.label(),
                value: self.audio.label().into(),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_moves_with_down() {
        let mut s = OptionsSession::new(OptionsState::default());
        let evs = s.tick(OptionsInput {
            down: true,
            ..Default::default()
        });
        assert_eq!(s.cursor(), 1);
        assert_eq!(evs, vec![OptionsEvent::CursorMoved { row: 1 }]);
    }

    #[test]
    fn left_decrements_clamped_at_zero() {
        let mut s = OptionsSession::new(OptionsState::default());
        for _ in 0..15 {
            s.tick(OptionsInput {
                left: true,
                ..Default::default()
            });
        }
        assert_eq!(s.state().bgm_volume, 0);
    }

    #[test]
    fn right_increments_clamped_at_max() {
        let mut s = OptionsSession::new(OptionsState::default());
        for _ in 0..15 {
            s.tick(OptionsInput {
                right: true,
                ..Default::default()
            });
        }
        assert_eq!(s.state().bgm_volume, 10);
    }

    #[test]
    fn vibration_toggle() {
        let mut s = OptionsSession::new(OptionsState::default());
        s.phase = OptionsPhase::Browsing {
            cursor: OptionsRow::Vibration.index(),
        };
        let _ = s.tick(OptionsInput {
            right: true,
            ..Default::default()
        });
        assert!(!s.state().vibration);
        let _ = s.tick(OptionsInput {
            right: true,
            ..Default::default()
        });
        assert!(s.state().vibration);
    }

    #[test]
    fn audio_mode_toggle() {
        let mut s = OptionsSession::new(OptionsState::default());
        s.phase = OptionsPhase::Browsing {
            cursor: OptionsRow::Audio.index(),
        };
        let _ = s.tick(OptionsInput {
            right: true,
            ..Default::default()
        });
        assert_eq!(s.state().audio, AudioMode::Mono);
    }

    #[test]
    fn cross_confirms() {
        let mut s = OptionsSession::new(OptionsState::default());
        let evs = s.tick(OptionsInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(OptionsOutcome::Confirmed));
        assert!(evs.contains(&OptionsEvent::Confirmed));
    }

    #[test]
    fn circle_cancels_and_revert_works() {
        let mut s = OptionsSession::new(OptionsState::default());
        // Change BGM volume.
        let _ = s.tick(OptionsInput {
            right: true,
            ..Default::default()
        });
        assert_eq!(s.state().bgm_volume, 9);
        let _ = s.tick(OptionsInput {
            circle: true,
            ..Default::default()
        });
        s.revert_if_cancelled();
        assert_eq!(s.state().bgm_volume, 8);
    }

    #[test]
    fn rows_view_renders_value_string() {
        let st = OptionsState::default();
        let rows = st.rows();
        assert_eq!(rows[0].value, "8/10");
        assert_eq!(rows[3].value, "On");
    }

    #[test]
    fn message_speed_clamped_at_one() {
        let mut s = OptionsSession::new(OptionsState::default());
        s.phase = OptionsPhase::Browsing {
            cursor: OptionsRow::MessageSpeed.index(),
        };
        for _ in 0..15 {
            let _ = s.tick(OptionsInput {
                left: true,
                ..Default::default()
            });
        }
        assert_eq!(s.state().message_speed, 1);
    }
}
