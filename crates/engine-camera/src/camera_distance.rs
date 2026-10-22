//! The field follow camera's eye-back distance preset, `CameraDistance`.
//!
//! An engine enhancement knob, not a retail global: the field camera
//! (`legaia_engine_field::camera::Camera::distance`) scales its eye-back
//! distance by it, and the options screen (`legaia_engine_menus::options`)
//! persists the player's choice. It sits here, below both, so the menu
//! front end can carry the setting without naming the field crate.

use serde::{Deserialize, Serialize};

/// Discrete camera-distance preset for the field follow camera. `Retail`
/// is the faithful framing; `Far` / `Farther` are engine enhancements that
/// pull the eye back so more of the scene is on screen. A pure framing
/// knob: it scales the eye-back distance only, so it never feeds the
/// world simulation (locomotion, encounters, replays are unaffected).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CameraDistance {
    /// The savestate-pinned retail framing (scale 1.0).
    #[default]
    Retail,
    /// A bit further out than retail - the interactive play-window default.
    Far,
    /// Wide vantage for eyeballing scene layout.
    Farther,
}

impl CameraDistance {
    /// Multiplier applied to the follow camera's eye-back distance.
    pub fn scale(self) -> f32 {
        match self {
            Self::Retail => 1.0,
            Self::Far => 1.35,
            Self::Farther => 1.8,
        }
    }

    /// Next preset in the cycle Retail -> Far -> Farther -> Retail.
    pub fn cycle(self) -> Self {
        match self {
            Self::Retail => Self::Far,
            Self::Far => Self::Farther,
            Self::Farther => Self::Retail,
        }
    }

    /// Human-readable label for HUD / logs.
    pub fn label(self) -> &'static str {
        match self {
            Self::Retail => "retail",
            Self::Far => "far",
            Self::Farther => "farther",
        }
    }

    /// Parse a CLI/HUD label (`retail` / `far` / `farther`),
    /// case-insensitive. `None` for unknown strings.
    pub fn from_label(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "retail" => Some(Self::Retail),
            "far" => Some(Self::Far),
            "farther" => Some(Self::Farther),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_cycle_and_labels_round_trip() {
        let mut d = CameraDistance::Retail;
        for _ in 0..3 {
            assert_eq!(CameraDistance::from_label(d.label()), Some(d));
            d = d.cycle();
        }
        assert_eq!(d, CameraDistance::Retail, "cycle is a 3-cycle");
        assert!(CameraDistance::Retail.scale() == 1.0);
        assert!(CameraDistance::Far.scale() > 1.0);
        assert!(CameraDistance::Farther.scale() > CameraDistance::Far.scale());
    }
}
