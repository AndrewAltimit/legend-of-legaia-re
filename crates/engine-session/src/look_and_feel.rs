//! The *Look and feel* preset both play hosts offer: one switch over every
//! setting that changes how the game looks or plays, read back as
//! **Enhanced** (the port's shipped choices), **Original** (as on a
//! PlayStation) or **Custom** (any other mix).
//!
//! The native launcher's and `play-window`'s `F12` panel row and the play
//! page's *Look and feel* selector both answer through this module, so the
//! two hosts cannot disagree on which rows the preset speaks for, or on what
//! Original turns on. The window, the sound levels and the menu colours are
//! the player's machine rather than a look, and stay out.
//!
//! Original is the session's retail-faithful switch plus the PlayStation's
//! own rasterisation; the rows under it keep their values, so stepping back
//! from Original finds a Custom mix where it was left. Enhanced is the reset:
//! every row the preset speaks for back on its first-start value.

use crate::session_switches::PersistedSwitches;
use legaia_engine_menus::options::OptionsState;

/// What the rows add up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookAndFeel {
    /// Every row the preset speaks for at its first-start value.
    Enhanced,
    /// The retail-faithful switch on (the rows under it keep their values).
    Original,
    /// Any other mix.
    Custom,
}

impl LookAndFeel {
    /// The row's value as both hosts show it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Enhanced => "Enhanced",
            Self::Original => "Original",
            Self::Custom => "Custom",
        }
    }

    /// [`Self::label`] read back; `None` for anything else.
    pub fn from_label(s: &str) -> Option<Self> {
        [Self::Enhanced, Self::Original, Self::Custom]
            .into_iter()
            .find(|p| p.label().eq_ignore_ascii_case(s.trim()))
    }
}

/// The options the preset speaks for, as a comparable tuple: everything
/// `OptionsState::pin_retail` pins, plus enhanced lighting's time of day.
#[allow(clippy::type_complexity)]
fn look_options(
    o: &OptionsState,
) -> (
    (bool, String, bool, bool, bool, bool, bool),
    (
        legaia_engine_camera::camera_distance::CameraDistance,
        bool,
        bool,
        bool,
        bool,
    ),
) {
    (
        (
            o.enhanced_lighting,
            o.lighting_time_of_day.clone(),
            o.light_shadows,
            o.occlusion_fade,
            o.volumetric_fog,
            o.bloom,
            o.ssao,
        ),
        (
            o.camera_distance,
            o.precise_movement,
            o.reduce_flashing,
            o.retail_view_window,
            o.retail_static_window,
        ),
    )
}

/// Which preset `opts` (the stored options, never a retail pin over them)
/// and `switches` (the remembered launch switches) add up to. Every
/// [`PersistedSwitches`] field but `retail` is a preset row.
pub fn look_and_feel(opts: &OptionsState, switches: &PersistedSwitches) -> LookAndFeel {
    if switches.retail {
        return LookAndFeel::Original;
    }
    let rows_default = look_options(opts) == look_options(&OptionsState::default())
        && *switches == PersistedSwitches::default();
    if rows_default {
        LookAndFeel::Enhanced
    } else {
        LookAndFeel::Custom
    }
}

/// Step the preset: Original on (with the PlayStation's rasterisation) or
/// off again (rasterisation back off), the rows under it untouched. Returns
/// whether Original now holds.
pub fn toggle_original(switches: &mut PersistedSwitches) -> bool {
    let on = !switches.retail;
    switches.retail = on;
    switches.psx_render = on;
    on
}

/// Put every row the preset speaks for back on its first-start value:
/// Enhanced. The window, the sound levels and the options-screen rows keep
/// the player's values.
pub fn reset_to_enhanced(opts: &mut OptionsState, switches: &mut PersistedSwitches) {
    let d = OptionsState::default();
    opts.reset_visual_enhancements();
    opts.precise_movement = d.precise_movement;
    opts.reduce_flashing = d.reduce_flashing;
    *switches = PersistedSwitches::default();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_enhanced_and_a_moved_row_is_custom() {
        let mut o = OptionsState::default();
        let mut s = PersistedSwitches::default();
        assert_eq!(look_and_feel(&o, &s), LookAndFeel::Enhanced);
        o.bgm_volume = 1;
        o.master_volume = 40;
        o.window_scale = 5;
        o.fullscreen ^= true;
        assert_eq!(
            look_and_feel(&o, &s),
            LookAndFeel::Enhanced,
            "machine, not look"
        );
        o.bloom ^= true;
        assert_eq!(look_and_feel(&o, &s), LookAndFeel::Custom);
        assert!(toggle_original(&mut s));
        assert!(s.psx_render);
        assert_eq!(look_and_feel(&o, &s), LookAndFeel::Original);
        assert!(!toggle_original(&mut s));
        assert_eq!(
            look_and_feel(&o, &s),
            LookAndFeel::Custom,
            "the mix is kept"
        );
        s.smart_monsters = true;
        reset_to_enhanced(&mut o, &mut s);
        assert_eq!(look_and_feel(&o, &s), LookAndFeel::Enhanced);
        assert_eq!((o.bgm_volume, o.window_scale), (1, 5));
    }

    #[test]
    fn every_switch_is_a_preset_row() {
        // Each stored switch, moved alone, reads Custom.
        let moved: Vec<PersistedSwitches> = vec![
            PersistedSwitches {
                psx_render: true,
                ..Default::default()
            },
            PersistedSwitches {
                no_entry_pulse: true,
                ..Default::default()
            },
            PersistedSwitches {
                smart_monsters: true,
                ..Default::default()
            },
            PersistedSwitches {
                arts_saved_list: true,
                ..Default::default()
            },
            PersistedSwitches {
                action_movement: true,
                ..Default::default()
            },
            PersistedSwitches {
                exp_rate_permille: Some(2000),
                ..Default::default()
            },
            PersistedSwitches {
                spirit_ap: Some(64),
                ..Default::default()
            },
        ];
        let o = OptionsState::default();
        for s in moved {
            assert_eq!(look_and_feel(&o, &s), LookAndFeel::Custom, "{s:?}");
        }
    }

    #[test]
    fn labels_round_trip() {
        for p in [
            LookAndFeel::Enhanced,
            LookAndFeel::Original,
            LookAndFeel::Custom,
        ] {
            assert_eq!(LookAndFeel::from_label(p.label()), Some(p));
        }
        assert_eq!(LookAndFeel::from_label("retail"), None);
    }
}
