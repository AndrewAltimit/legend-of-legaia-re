//! The host hotkeys: the option toggles, cheats and camera knobs a play host
//! binds to keys that carry no pad button, written down once.
//!
//! Before this table the native window spelt twelve of them in its keyboard
//! arms and the browser play page spelt one (`F3`), so `F8`, `F9`, `F4` and
//! `I` - the keys the project's own docs name for the enhancement toggles -
//! did nothing on the page, and nothing compared the two sets. Both hosts now
//! resolve a physical key through [`HostHotkey::from_dom_code`] and act on
//! the variant:
//!
//! * the native window spells winit's `KeyCode` in W3C `KeyboardEvent.code`
//!   form (winit names its variants after that spec), so one table serves
//!   both;
//! * the page asks `LegaiaRuntime::play_host_hotkey(event.code)`, which runs
//!   the same [`apply_options_hotkey`] edit, and gets back the
//!   [`HostHotkey::id`] for the part only the page holds (a checkbox to
//!   mirror, its own HUD bar).
//!
//! Every key here is outside the bindable vocabulary
//! ([`legaia_engine_system::input::KEY_NAME_DOM_CODES`]): a hotkey arm runs
//! before the pad lookup on the native window, so a hotkey on a bindable key
//! would delete that button from the keyboard.

use legaia_engine_menus::options::OptionsState;
use legaia_render_kernels::scene_lighting::TimeOfDay;

/// One host hotkey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostHotkey {
    /// `F1`: the host's diagnostic readout (native text rows, the page's HUD bar).
    DiagRows,
    /// `F2`: master audio mute ([`OptionsState::muted`]).
    AudioMute,
    /// `F3`: the debug orbit vantage.
    DebugCamera,
    /// `F4`: the camera-occlusion fade enhancement ([`OptionsState::occlusion_fade`]).
    OcclusionFade,
    /// `F6`: cheat - fill the party's AP gauge.
    CheatMaxAp,
    /// `F7`: cheat - random encounters on / off.
    CheatEncounters,
    /// `F8`: cycle the enhanced-lighting time of day.
    TimeOfDay,
    /// `F9`: the volumetric ground-fog enhancement.
    VolumetricFog,
    /// `I`: the enhanced-lighting enhancement.
    EnhancedLighting,
    /// `Y`: the enhanced-lighting lamp shadows ([`OptionsState::light_shadows`]).
    LightShadows,
    /// `T`: cycle the follow camera's distance preset.
    CameraDistance,
    /// `R`: precise (free-angle) movement.
    PreciseMovement,
    // --- lane L ---
    /// `F10`: the bloom enhancement ([`OptionsState::bloom`]).
    Bloom,
}

/// Every hotkey with its `KeyboardEvent.code`.
pub const HOST_HOTKEYS: [(HostHotkey, &str); 13] = [
    (HostHotkey::DiagRows, "F1"),
    (HostHotkey::AudioMute, "F2"),
    (HostHotkey::DebugCamera, "F3"),
    (HostHotkey::OcclusionFade, "F4"),
    (HostHotkey::CheatMaxAp, "F6"),
    (HostHotkey::CheatEncounters, "F7"),
    (HostHotkey::TimeOfDay, "F8"),
    (HostHotkey::VolumetricFog, "F9"),
    (HostHotkey::EnhancedLighting, "KeyI"),
    (HostHotkey::LightShadows, "KeyY"),
    (HostHotkey::CameraDistance, "KeyT"),
    (HostHotkey::PreciseMovement, "KeyR"),
    // --- lane L ---
    (HostHotkey::Bloom, "F10"),
];

impl HostHotkey {
    /// The hotkey a `KeyboardEvent.code` (or a winit `KeyCode`'s `Debug`
    /// name, which is the same string) is bound to.
    pub fn from_dom_code(code: &str) -> Option<Self> {
        HOST_HOTKEYS
            .iter()
            .find(|(_, c)| *c == code)
            .map(|(hk, _)| *hk)
    }

    /// The `KeyboardEvent.code` this hotkey is bound to.
    pub fn dom_code(self) -> &'static str {
        HOST_HOTKEYS
            .iter()
            .find(|(hk, _)| *hk == self)
            .map(|(_, c)| *c)
            .unwrap_or("")
    }

    /// The stable name the page's action table is keyed by.
    pub fn id(self) -> &'static str {
        match self {
            HostHotkey::DiagRows => "diag_rows",
            HostHotkey::AudioMute => "audio_mute",
            HostHotkey::DebugCamera => "debug_camera",
            HostHotkey::OcclusionFade => "occlusion_fade",
            HostHotkey::CheatMaxAp => "cheat_max_ap",
            HostHotkey::CheatEncounters => "cheat_encounters",
            HostHotkey::TimeOfDay => "time_of_day",
            HostHotkey::VolumetricFog => "volumetric_fog",
            HostHotkey::EnhancedLighting => "enhanced_lighting",
            HostHotkey::LightShadows => "light_shadows",
            HostHotkey::CameraDistance => "camera_distance",
            HostHotkey::PreciseMovement => "precise_movement",
            HostHotkey::Bloom => "bloom",
        }
    }

    /// What the key changes, as a play host's on-screen notice and its
    /// hotkey help name it ("Ground fog").
    pub fn label(self) -> &'static str {
        match self {
            HostHotkey::DiagRows => "Diagnostics",
            HostHotkey::AudioMute => "Sound",
            HostHotkey::DebugCamera => "Debug camera",
            HostHotkey::OcclusionFade => "See-through walls",
            HostHotkey::CheatMaxAp => "Cheat: fill AP",
            HostHotkey::CheatEncounters => "Encounters",
            HostHotkey::TimeOfDay => "Time of day",
            HostHotkey::VolumetricFog => "Ground fog",
            HostHotkey::EnhancedLighting => "Enhanced lighting",
            HostHotkey::LightShadows => "Lamp shadows",
            HostHotkey::CameraDistance => "Camera distance",
            HostHotkey::PreciseMovement => "Free-angle movement",
            HostHotkey::Bloom => "Bloom",
        }
    }

    /// The key as a player reads it: `F9`, or `I` for `KeyI`.
    pub fn key_label(self) -> &'static str {
        let code = self.dom_code();
        code.strip_prefix("Key").unwrap_or(code)
    }

    /// Whether the key is refused while a front-end screen (boot logos,
    /// title, save select) owns the frame: the field camera's vantage and
    /// distance and the field locomotion knob have nothing to act on there.
    pub fn needs_field(self) -> bool {
        matches!(
            self,
            HostHotkey::DebugCamera | HostHotkey::CameraDistance | HostHotkey::PreciseMovement
        )
    }

    /// Whether a play host refuses this key with `screens` owning its frame:
    /// a [`Self::needs_field`] key under any front-end screen. The one rule
    /// both hosts ask. The native window refused under its pause menu and
    /// game-over panel while the page refused only under the logos and the
    /// title, so `T` / `R` edited and persisted options behind the page's
    /// pause menu; each host now names every screen.
    pub fn refused(self, screens: FrontEndScreens) -> bool {
        self.needs_field() && screens.any()
    }
}

/// The front-end screens that can own a play host's frame, as the hotkey
/// gate ([`HostHotkey::refused`]) reads them. Each host fills every field
/// from its own state; a gate pairs the two fills by name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrontEndScreens {
    /// The publisher logos.
    pub boot_logos: bool,
    /// The title screen.
    pub title: bool,
    /// A save-select screen outside the pause menu (the title's Continue).
    pub save_select: bool,
    /// The pause menu and the screens behind it.
    pub pause_menu: bool,
    /// The game-over panel.
    pub game_over: bool,
}

impl FrontEndScreens {
    /// Whether any front-end screen owns the frame.
    pub fn any(self) -> bool {
        let FrontEndScreens {
            boot_logos,
            title,
            save_select,
            pause_menu,
            game_over,
        } = self;
        boot_logos || title || save_select || pause_menu || game_over
    }
}

/// Apply `hotkey`'s edit to the persisted options, if it is an options
/// hotkey. `true` when `opts` changed; the caller persists and pushes the
/// options into their consumers (the world through
/// [`crate::BootSession::apply_option_knobs`], the audio output, the
/// renderer).
pub fn apply_options_hotkey(opts: &mut OptionsState, hotkey: HostHotkey) -> bool {
    match hotkey {
        HostHotkey::AudioMute => opts.muted = !opts.muted,
        HostHotkey::EnhancedLighting => opts.enhanced_lighting = !opts.enhanced_lighting,
        HostHotkey::TimeOfDay => {
            let next = TimeOfDay::from_name(&opts.lighting_time_of_day)
                .unwrap_or_default()
                .next();
            opts.lighting_time_of_day = next.name().to_string();
        }
        HostHotkey::VolumetricFog => opts.volumetric_fog = !opts.volumetric_fog,
        HostHotkey::CameraDistance => {
            opts.cycle_camera_distance();
        }
        HostHotkey::PreciseMovement => opts.precise_movement = !opts.precise_movement,
        HostHotkey::OcclusionFade => opts.occlusion_fade = !opts.occlusion_fade,
        HostHotkey::LightShadows => opts.light_shadows = !opts.light_shadows,
        HostHotkey::Bloom => opts.bloom = !opts.bloom,
        HostHotkey::DiagRows
        | HostHotkey::DebugCamera
        | HostHotkey::CheatMaxAp
        | HostHotkey::CheatEncounters => return false,
    }
    true
}

fn on_off(on: bool) -> String {
    if on { "on" } else { "off" }.to_string()
}

/// The state an options hotkey leaves, worded for the toggle notice
/// ([`toggle_notice`]): `on` / `off`, the time-of-day name, the camera
/// distance preset. `None` for a hotkey whose state the host holds (the
/// diagnostic readout, the debug vantage, the cheats), which the host words
/// itself.
pub fn options_hotkey_state(opts: &OptionsState, hotkey: HostHotkey) -> Option<String> {
    Some(match hotkey {
        HostHotkey::AudioMute => if opts.muted { "muted" } else { "on" }.to_string(),
        HostHotkey::EnhancedLighting => on_off(opts.enhanced_lighting),
        HostHotkey::TimeOfDay => opts.lighting_time_of_day.clone(),
        HostHotkey::VolumetricFog => on_off(opts.volumetric_fog),
        HostHotkey::CameraDistance => opts.camera_distance.label().to_string(),
        HostHotkey::PreciseMovement => on_off(opts.precise_movement),
        HostHotkey::OcclusionFade => on_off(opts.occlusion_fade),
        HostHotkey::LightShadows => on_off(opts.light_shadows),
        HostHotkey::Bloom => on_off(opts.bloom),
        HostHotkey::DiagRows
        | HostHotkey::DebugCamera
        | HostHotkey::CheatMaxAp
        | HostHotkey::CheatEncounters => return None,
    })
}

/// The transient line a play host shows after a hotkey press:
/// `"Ground fog: off"`. The hotkeys change state nothing else on screen
/// names, so without it a press reads as nothing happening.
pub fn toggle_notice(hotkey: HostHotkey, state: &str) -> String {
    format!("{}: {state}", hotkey.label())
}

/// The notice for a hotkey a front-end screen refused
/// ([`HostHotkey::refused`]): the key did reach the host, it just has
/// nothing to act on yet.
pub fn refused_notice(hotkey: HostHotkey) -> String {
    format!("{}: only while walking around", hotkey.label())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No hotkey sits on a bindable key: the native window's hotkey arms run
    /// before its pad lookup, so a collision would take a button off the
    /// keyboard on that host and leave it on the other.
    #[test]
    fn no_hotkey_is_a_bindable_key() {
        for (hk, code) in HOST_HOTKEYS {
            assert!(
                legaia_engine_system::input::key_name_for_dom_code(code).is_none(),
                "{hk:?} on bindable key {code}"
            );
        }
    }

    #[test]
    fn codes_and_ids_are_unique_and_round_trip() {
        let mut codes = std::collections::HashSet::new();
        let mut ids = std::collections::HashSet::new();
        for (hk, code) in HOST_HOTKEYS {
            assert!(codes.insert(code));
            assert!(ids.insert(hk.id()));
            assert_eq!(HostHotkey::from_dom_code(code), Some(hk));
            assert_eq!(hk.dom_code(), code);
        }
        assert_eq!(HostHotkey::from_dom_code("KeyZ"), None);
    }

    #[test]
    fn every_hotkey_has_a_label_and_a_printable_key() {
        let mut labels = std::collections::HashSet::new();
        for (hk, _) in HOST_HOTKEYS {
            assert!(labels.insert(hk.label()), "{hk:?} label is shared");
            assert!(!hk.key_label().starts_with("Key"), "{hk:?}");
        }
        assert_eq!(HostHotkey::EnhancedLighting.key_label(), "I");
        assert_eq!(HostHotkey::VolumetricFog.key_label(), "F9");
    }

    #[test]
    fn notices_name_the_toggle_and_its_new_state() {
        let mut o = OptionsState {
            volumetric_fog: true,
            ..OptionsState::default()
        };
        apply_options_hotkey(&mut o, HostHotkey::VolumetricFog);
        let state = options_hotkey_state(&o, HostHotkey::VolumetricFog).unwrap();
        assert_eq!(
            toggle_notice(HostHotkey::VolumetricFog, &state),
            "Ground fog: off"
        );
        assert_eq!(
            options_hotkey_state(&o, HostHotkey::TimeOfDay).as_deref(),
            Some(o.lighting_time_of_day.as_str())
        );
        assert_eq!(options_hotkey_state(&o, HostHotkey::DiagRows), None);
    }

    #[test]
    fn options_hotkeys_edit_the_persisted_options() {
        let mut o = OptionsState::default();
        let fog = o.volumetric_fog;
        assert!(apply_options_hotkey(&mut o, HostHotkey::VolumetricFog));
        assert_eq!(o.volumetric_fog, !fog);
        let tod = o.lighting_time_of_day.clone();
        assert!(apply_options_hotkey(&mut o, HostHotkey::TimeOfDay));
        assert_ne!(o.lighting_time_of_day, tod);
        let dist = o.camera_distance;
        assert!(apply_options_hotkey(&mut o, HostHotkey::CameraDistance));
        assert_ne!(o.camera_distance, dist);
        // The render enhancements are options too, so a key and the page's
        // box edit one persisted value on both hosts.
        assert!(apply_options_hotkey(&mut o, HostHotkey::OcclusionFade));
        assert!(!o.occlusion_fade);
        assert!(apply_options_hotkey(&mut o, HostHotkey::LightShadows));
        assert!(!o.light_shadows);
        assert!(apply_options_hotkey(&mut o, HostHotkey::Bloom));
        assert!(o.bloom);
        let before = o.clone();
        assert!(!apply_options_hotkey(&mut o, HostHotkey::DebugCamera));
        assert_eq!(o, before);
    }
}
