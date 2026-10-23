//! The option keys: audio mute, the render enhancements, the two cheats,
//! the diagnostic rows and the camera / movement knobs.
//!
//! Which key is which is not written here: it is the shared table
//! [`legaia_engine_session::hotkeys::HOST_HOTKEYS`], which the browser play
//! page resolves through `play_host_hotkey`. This file holds only what each
//! key does to state the window alone holds (its renderer, its diagnostic
//! rows, its debug vantage); an options edit is the shared
//! [`legaia_engine_session::hotkeys::apply_options_hotkey`].

use super::super::super::*;
use legaia_engine_cheats::cheats::WorldCheats;
use legaia_engine_session::hotkeys::{HostHotkey, apply_options_hotkey};

impl PlayWindowApp {
    /// The window's hotkeys. `true` when one took the key.
    ///
    /// winit names `KeyCode` variants after the W3C `KeyboardEvent.code`
    /// values, so the variant's `Debug` name is the code the shared table is
    /// keyed by.
    ///
    /// A held key's auto-repeat (`repeat`) is taken and ignored: only the
    /// leading edge toggles, or holding `F9` would flicker the fog on and
    /// off.
    pub(super) fn option_key(&mut self, code: KeyCode, state: ElementState, repeat: bool) -> bool {
        if state != ElementState::Pressed {
            return false;
        }
        let Some(hotkey) = HostHotkey::from_dom_code(&format!("{code:?}")) else {
            return false;
        };
        let screens = legaia_engine_session::hotkeys::FrontEndScreens {
            boot_logos: self.boot_ui.logos_session().is_some(),
            title: self.boot_ui.title_session().is_some(),
            save_select: matches!(self.boot_ui, BootUiState::Continue(_)),
            pause_menu: matches!(self.boot_ui, BootUiState::FieldMenu { .. }),
            game_over: matches!(self.boot_ui, BootUiState::GameOver(_)),
        };
        if hotkey.refused(screens) {
            // The key reached the window; say why nothing changed rather
            // than leave the press looking dead.
            self.show_notice(legaia_engine_session::hotkeys::refused_notice(hotkey));
            return false;
        }
        if repeat {
            return true;
        }
        if hotkey == HostHotkey::EnhancedLighting {
            // A launch switch (`--dynamic-lighting` / `--no-dynamic-lighting`)
            // may hold the renderer off the stored option: toggle what is on
            // screen.
            self.options_state.enhanced_lighting = self.dynamic_lighting;
        }
        if apply_options_hotkey(&mut self.options_state, hotkey) {
            self.session.apply_option_knobs(&self.options_state, None);
            self.persist_and_apply_options();
        }
        match hotkey {
            HostHotkey::AudioMute => self.key_audio_mute(),
            HostHotkey::EnhancedLighting => self.key_enhanced_lighting(),
            HostHotkey::TimeOfDay => self.key_time_of_day(),
            HostHotkey::LightShadows => self.key_light_shadows(),
            HostHotkey::OcclusionFade => self.key_occlusion_fade(),
            HostHotkey::VolumetricFog => self.key_volumetric_fog(),
            HostHotkey::CheatMaxAp => self.key_cheat_max_ap(),
            HostHotkey::CheatEncounters => self.key_cheat_encounters(),
            HostHotkey::DiagRows => self.key_diag_rows(),
            HostHotkey::DebugCamera => self.key_debug_camera(),
            HostHotkey::CameraDistance => self.key_camera_distance(),
            HostHotkey::PreciseMovement => self.key_precise_movement(),
            HostHotkey::Bloom => self.key_bloom(),
        }
        let notice = self.hotkey_notice(hotkey);
        self.show_notice(notice);
        true
    }

    /// The state `hotkey` is in now, worded for the notice line and the
    /// hotkey help: the shared wording for an options key, this window's
    /// own for the keys whose state it holds.
    pub(in crate::window) fn hotkey_state(&self, hotkey: HostHotkey) -> String {
        use legaia_engine_session::hotkeys::options_hotkey_state;
        if hotkey == HostHotkey::EnhancedLighting {
            // A launch switch may hold the renderer off the stored option.
            return if self.dynamic_lighting { "on" } else { "off" }.to_string();
        }
        if let Some(s) = options_hotkey_state(&self.options_state, hotkey) {
            return s;
        }
        match hotkey {
            HostHotkey::DiagRows => if self.diag_rows { "on" } else { "off" }.to_string(),
            HostHotkey::DebugCamera => {
                if self.field_debug_camera { "on" } else { "off" }.to_string()
            }
            HostHotkey::CheatEncounters => if self.session.host.world.toggles.live_gameplay_loop {
                "on"
            } else {
                "off"
            }
            .to_string(),
            _ => String::new(),
        }
    }

    /// The notice a press of `hotkey` shows: `"Ground fog: off"`, with a
    /// hint when the key changed something enhanced lighting gates.
    fn hotkey_notice(&self, hotkey: HostHotkey) -> String {
        if hotkey == HostHotkey::CheatMaxAp {
            return format!("{}: done", hotkey.label());
        }
        let mut s =
            legaia_engine_session::hotkeys::toggle_notice(hotkey, &self.hotkey_state(hotkey));
        if matches!(hotkey, HostHotkey::TimeOfDay | HostHotkey::LightShadows)
            && !self.dynamic_lighting
        {
            s.push_str(" (needs enhanced lighting: I)");
        }
        s
    }

    /// Show `text` on the window's notice line for
    /// [`crate::window::app_state::NOTICE_DURATION`].
    pub(in crate::window) fn show_notice(&mut self, text: String) {
        self.notice = Some((text, std::time::Instant::now()));
    }

    /// `F12` or `/` (the `?` key): the hotkey help panel, on or off. Window
    /// chrome on keys no pad binding can hold (`keycode_to_name` has neither),
    /// so it never takes a button off the keyboard.
    pub(super) fn help_key(&mut self, code: KeyCode, state: ElementState) -> bool {
        if !matches!(code, KeyCode::F12 | KeyCode::Slash) {
            return false;
        }
        // A held key repeats as a stream of `Pressed` events; only the
        // leading edge flips the panel.
        match state {
            ElementState::Pressed if self.help_key_held != Some(code) => {
                self.help_key_held = Some(code);
                self.hotkey_help = !self.hotkey_help;
                self.startup_hint.dismiss();
            }
            ElementState::Released if self.help_key_held == Some(code) => {
                self.help_key_held = None;
            }
            _ => {}
        }
        true
    }

    /// `F2`: master audio mute. The shared edit flipped the engine-only
    /// `muted` option and the persist pushed it into the mixer's master gate
    /// (output silenced; sequencer + SPU keep ticking so unmute stays in
    /// sync). The HUD status line reflects the state ("audio muted").
    ///
    /// It used to be `V`, which is **Square** in the engine's default pad
    /// table - so the native window had no Square at all, and the dance
    /// minigame's third judged button, the fishing reel B and the
    /// three-actor talk's leader switch were all unpressable.
    fn key_audio_mute(&mut self) {
        log::info!(
            "audio: {}",
            if self.options_state.muted {
                "muted (F2 to unmute)"
            } else {
                "unmuted"
            }
        );
    }

    /// `I`: the enhanced-lighting enhancement (the persisted
    /// `enhanced_lighting` option, default on; `--dynamic-lighting` /
    /// `--no-dynamic-lighting` force it per session). NON-RETAIL: the
    /// field path has no light source, so OFF is the faithful
    /// pixel-identical render; ON shades under the scene mood with the
    /// derived lamp lights, emissive surfaces and their glow.
    /// Pure renderer state - no world/sim effect, replays unaffected.
    fn key_enhanced_lighting(&mut self) {
        self.dynamic_lighting = self.options_state.enhanced_lighting;
        if let Some(r) = self.win.renderer.as_ref() {
            r.set_dynamic_lighting(self.dynamic_lighting);
        }
        log::info!(
            "render: enhanced lighting {}",
            if self.dynamic_lighting {
                "ON (enhancement - not retail)"
            } else {
                "off (faithful baked shading)"
            }
        );
    }

    /// `F8`: the time of day enhanced lighting lights scenes under
    /// (auto -> day -> dusk -> night), cycled by the shared edit. Persisted;
    /// inert while the enhancement is off.
    fn key_time_of_day(&mut self) {
        log::info!(
            "render: time of day {}{}",
            self.options_state.lighting_time_of_day,
            if self.dynamic_lighting {
                ""
            } else {
                " (inert until enhanced lighting is on - press I)"
            }
        );
    }

    /// `Y`: toggle the shadows the dynamic-lighting enhancement's point
    /// lights cast (the `--no-dyn-shadows` flag's runtime twin). Shadows
    /// only - the lamps keep lighting the scene, unshadowed, which is
    /// what the play page's "Lamp shadows" box does too. Only visible
    /// while dynamic lighting (`I`) is on. The persisted
    /// `OptionsState::light_shadows`, flipped by the shared edit and pushed
    /// into the renderer by the options apply.
    /// `F10`: toggle the bloom enhancement. The persisted
    /// `OptionsState::bloom`, flipped by the shared edit and pushed into the
    /// renderer by the options apply; pure renderer state.
    fn key_bloom(&mut self) {
        log::info!(
            "render: bloom {}",
            if self.options_state.bloom {
                "ON"
            } else {
                "off"
            }
        );
    }

    fn key_light_shadows(&mut self) {
        log::info!(
            "render: scene light-source shadows {}{}",
            if self.options_state.light_shadows {
                "ON"
            } else {
                "off"
            },
            if self.dynamic_lighting {
                ""
            } else {
                " (inert until dynamic lighting is on - press I)"
            }
        );
    }

    /// `F4`: toggle the camera-occlusion fade enhancement (the
    /// `--no-occlusion-fade` flag's runtime twin). Default ON: field
    /// walls / roofs / props between the camera and the player dissolve
    /// to a screen-door dither around the character so it stays
    /// visible. Pure renderer state - no world/sim effect, replays
    /// unaffected. OFF is the retail behaviour (authored framing only).
    ///
    /// Not `D`, and for a sharper reason than `F2`-not-`V`: `D` is a
    /// **pad-bindable key name** (`KEY_NAME_DOM_CODES` carries it, and the
    /// browser layout spends it on Right), so a player who binds
    /// `D = Right` on the desktop build could not walk right - the hotkey
    /// arm consumes the event and returns before the pad mapping runs. The
    /// shared table's own test holds every hotkey off the bindable set.
    ///
    /// The persisted `OptionsState::occlusion_fade` (the page's
    /// "See-through walls" box), flipped by the shared edit and pushed into
    /// the renderer by the options apply.
    fn key_occlusion_fade(&mut self) {
        log::info!(
            "render: camera-occlusion fade {}",
            if self.options_state.occlusion_fade {
                "ON (see-through walls - enhancement)"
            } else {
                "off (retail authored framing)"
            }
        );
    }

    /// `F9`: the volumetric ground-fog enhancement (the
    /// `--no-volumetric-fog` flag's runtime twin, the play page's "Ground
    /// fog" box). Default **off** while the look matures
    /// (`OptionsState::volumetric_fog` defaults to `false`); on, misty / night
    /// scenes carry a drifting mist bank the characters part, and
    /// `--no-volumetric-fog` pins it off. The setting is the engine's option
    /// (`OptionsState::volumetric_fog` -> `WorldToggles::volumetric_fog`,
    /// pushed by the shared knob step); off resets the bank and draws the
    /// frame without it.
    fn key_volumetric_fog(&mut self) {
        log::info!(
            "render: volumetric ground fog {}",
            if self.options_state.volumetric_fog {
                "ON (enhancement - not retail)"
            } else {
                "off (retail fog puffs only)"
            }
        );
    }

    /// `F6`: cheat - fill the party's AP (Spirit) gauge (the play page's
    /// Cheats -> Max AP). F-keys, like `F4`, because nothing can be bound
    /// to them.
    fn key_cheat_max_ap(&mut self) {
        let line = self
            .session
            .host
            .world
            .apply_cheat(&legaia_engine_cheats::cheats::PlayerCheat::MaxAp, None);
        log::info!("cheat: {line}");
    }

    /// `F7`: cheat - random encounters on / off (the play page's "No
    /// encounters", `--no-live-loop` at launch).
    fn key_cheat_encounters(&mut self) {
        let world = &mut self.session.host.world;
        let on = !world.toggles.live_gameplay_loop;
        world.apply_cheat(
            &legaia_engine_cheats::cheats::PlayerCheat::RandomEncounters(on),
            None,
        );
        // The choice outlives this scene: the live-loop arming a card
        // Load / New Game / door entry runs (`World::arm_live_loop`
        // through `field_live_opts`) re-raised the roll, so `F7` off
        // lasted only until the next Load. The play page's "No
        // encounters" is a session flag its every entry reads
        // (`LegaiaRuntime::live_battles`); this is the same flag here.
        self.field_live_opts.live_loop = on;
        log::info!("cheat: random encounters {}", if on { "ON" } else { "off" });
    }

    /// `F1`: toggle the shell's diagnostic text rows (scene / frame /
    /// mesh count, the camera + audio status line, the world-map camera
    /// readout, the no-encounter hint). Off by default - retail draws no
    /// text there, and the seat is the party readout's
    /// (`field_party_hud_draws`). `LEGAIA_DIAG_HUD` starts them on, the
    /// same variable the battle diagnostics read.
    fn key_diag_rows(&mut self) {
        self.diag_rows = !self.diag_rows;
        log::info!(
            "hud: diagnostic rows {}",
            if self.diag_rows { "ON" } else { "off" }
        );
    }

    /// `F3`: toggle the field camera between the retail follow view
    /// (savestate-pinned pitch/yaw/H, player-anchored - the
    /// faithful framing) and the wide debug orbit vantage (better
    /// for eyeballing scene completeness). Moved off `C` for the same
    /// reason `F2` is not `V`: `C` is Triangle.
    fn key_debug_camera(&mut self) {
        self.field_debug_camera = !self.field_debug_camera;
        log::info!(
            "camera: field vantage = {}",
            if self.field_debug_camera {
                "debug orbit"
            } else {
                "retail follow"
            }
        );
    }

    /// `T`: the field camera-distance preset (retail -> far -> farther),
    /// cycled by the shared edit and pushed onto the session camera by the
    /// knob step. A pure framing knob on the follow / debug-orbit cameras -
    /// it never feeds the world simulation. Persisted; the interactive
    /// default is `far`.
    fn key_camera_distance(&mut self) {
        log::info!(
            "camera: distance = {} (T cycles)",
            self.options_state.camera_distance.label()
        );
    }

    /// `R`: precise movement (opt-in, NON-RETAIL): free-angle
    /// camera-relative locomotion instead of retail's 4/8-way
    /// quantisation - key diagonals walk true 45-degree vectors and an
    /// analog stick's angle passes through continuously. Persisted;
    /// default off (the faithful remap).
    fn key_precise_movement(&mut self) {
        log::info!(
            "movement: precise {}",
            if self.options_state.precise_movement {
                "ON (free-angle - not retail)"
            } else {
                "off (retail 8-way remap)"
            }
        );
    }
}
