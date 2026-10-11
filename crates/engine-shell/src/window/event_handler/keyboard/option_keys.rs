//! The option keys: audio mute, the render enhancements, the two cheats,
//! the diagnostic rows and the camera / movement knobs.

use super::super::super::*;

impl PlayWindowApp {
    /// `F2` / `I` / `F8` / `Y` / `F4` / `F9` / `F6` / `F7` / `F1` / `F3` / `T` / `R`: toggles and cycles. `true` when one took the key.
    pub(super) fn option_key(&mut self, code: KeyCode, state: ElementState) -> bool {
        self.key_audio_mute(code, state)
            || self.key_enhanced_lighting(code, state)
            || self.key_time_of_day(code, state)
            || self.key_light_shadows(code, state)
            || self.key_occlusion_fade(code, state)
            || self.key_volumetric_fog(code, state)
            || self.key_cheat_max_ap(code, state)
            || self.key_cheat_encounters(code, state)
            || self.key_diag_rows(code, state)
            || self.key_debug_camera(code, state)
            || self.key_camera_distance(code, state)
            || self.key_precise_movement(code, state)
    }

    /// `F2`: master audio mute toggle. Flips the engine-only `muted`
    /// options knob, pushes it into the mixer's master gate (output
    /// silenced; sequencer + SPU keep ticking so unmute stays in sync),
    /// and persists it to the options config file. The HUD status line
    /// reflects the state ("audio muted").
    ///
    /// It used to be `V`, which is **Square** in the engine's default pad
    /// table - so the native window had no Square at all, and the dance
    /// minigame's third judged button, the fishing reel B and the
    /// three-actor talk's leader switch were all unpressable.
    fn key_audio_mute(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::F2) && state == ElementState::Pressed {
            self.options_state.muted = !self.options_state.muted;
            self.persist_and_apply_options();
            log::info!(
                "audio: {}",
                if self.options_state.muted {
                    "muted (F2 to unmute)"
                } else {
                    "unmuted"
                }
            );
            return true;
        }
        false
    }

    /// `I`: toggle the enhanced-lighting enhancement (the persisted
    /// `enhanced_lighting` option, default on; `--dynamic-lighting` /
    /// `--no-dynamic-lighting` force it per session). NON-RETAIL: the
    /// field path has no light source, so OFF is the faithful
    /// pixel-identical render; ON shades under the scene mood with the
    /// derived lamp lights, emissive surfaces and their glow.
    /// Pure renderer state - no world/sim effect, replays unaffected.
    fn key_enhanced_lighting(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::KeyI) && state == ElementState::Pressed {
            self.dynamic_lighting = !self.dynamic_lighting;
            if let Some(r) = self.win.renderer.as_ref() {
                r.set_dynamic_lighting(self.dynamic_lighting);
            }
            self.options_state.enhanced_lighting = self.dynamic_lighting;
            self.persist_and_apply_options();
            log::info!(
                "render: enhanced lighting {}",
                if self.dynamic_lighting {
                    "ON (enhancement - not retail)"
                } else {
                    "off (faithful baked shading)"
                }
            );
            return true;
        }
        false
    }

    /// `F8`: cycle the time of day enhanced lighting lights scenes under
    /// (auto -> day -> dusk -> night). Persisted; inert while the
    /// enhancement is off.
    fn key_time_of_day(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::F8) && state == ElementState::Pressed {
            use legaia_render_kernels::scene_lighting::TimeOfDay;
            let next = TimeOfDay::from_name(&self.options_state.lighting_time_of_day)
                .unwrap_or_default()
                .next();
            self.options_state.lighting_time_of_day = next.name().to_string();
            self.persist_and_apply_options();
            log::info!(
                "render: time of day {}{}",
                next.name(),
                if self.dynamic_lighting {
                    ""
                } else {
                    " (inert until enhanced lighting is on - press I)"
                }
            );
            return true;
        }
        false
    }

    /// `Y`: toggle the shadows the dynamic-lighting enhancement's point
    /// lights cast (the `--no-dyn-shadows` flag's runtime twin). Shadows
    /// only - the lamps keep lighting the scene, unshadowed, which is
    /// what the play page's "Lamp shadows" box does too. Only visible
    /// while dynamic lighting (`I`) is on. Pure renderer state, like `I`.
    fn key_light_shadows(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::KeyY) && state == ElementState::Pressed {
            self.dyn_shadows = !self.dyn_shadows;
            if let Some(r) = self.win.renderer.as_ref() {
                r.set_dyn_shadows(self.dyn_shadows);
            }
            log::info!(
                "render: scene light-source shadows {}{}",
                if self.dyn_shadows { "ON" } else { "off" },
                if self.dynamic_lighting {
                    ""
                } else {
                    " (inert until dynamic lighting is on - press I)"
                }
            );
            return true;
        }
        false
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
    /// `D = Right` on the desktop build could not walk right - this arm
    /// consumes the event and returns before the pad mapping runs. `F4`
    /// has no entry in the shell's binding parser, so nothing can ever be
    /// bound to it.
    fn key_occlusion_fade(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::F4) && state == ElementState::Pressed {
            self.occlusion_fade = !self.occlusion_fade;
            if let Some(r) = self.win.renderer.as_ref() {
                r.set_occlusion_fade(self.occlusion_fade);
            }
            log::info!(
                "render: camera-occlusion fade {}",
                if self.occlusion_fade {
                    "ON (see-through walls - enhancement)"
                } else {
                    "off (retail authored framing)"
                }
            );
            return true;
        }
        false
    }

    /// `F9`: toggle the volumetric ground-fog enhancement (the
    /// `--no-volumetric-fog` flag's runtime twin, the play page's "Ground
    /// fog" box). Default ON: misty / night scenes carry a drifting mist
    /// bank the characters part. The setting is the engine's option
    /// (`OptionsState::volumetric_fog` -> `WorldToggles::volumetric_fog`),
    /// persisted like `R`; off resets the bank and draws the frame
    /// without it. Like `F4`, an F-key so no pad binding can shadow it.
    fn key_volumetric_fog(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::F9) && state == ElementState::Pressed {
            let on = !self.options_state.volumetric_fog;
            self.options_state.volumetric_fog = on;
            self.session.host.world.toggles.volumetric_fog = on;
            self.persist_and_apply_options();
            log::info!(
                "render: volumetric ground fog {}",
                if on {
                    "ON (enhancement - not retail)"
                } else {
                    "off (retail fog puffs only)"
                }
            );
            return true;
        }
        false
    }

    /// `F6`: cheat - fill the party's AP (Spirit) gauge (the play page's
    /// Cheats -> Max AP). `F7`: cheat - random encounters on / off (the
    /// play page's "No encounters", `--no-live-loop` at launch). F-keys,
    /// like `F4`, because nothing can be bound to them.
    fn key_cheat_max_ap(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::F6) && state == ElementState::Pressed {
            let line = self
                .session
                .host
                .world
                .apply_cheat(&legaia_engine_core::cheats::PlayerCheat::MaxAp, None);
            log::info!("cheat: {line}");
            return true;
        }
        false
    }

    /// `F7`: cheat - random encounters on / off. See
    /// [`Self::key_cheat_max_ap`] for why the cheats sit on F-keys.
    fn key_cheat_encounters(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::F7) && state == ElementState::Pressed {
            let world = &mut self.session.host.world;
            let on = !world.toggles.live_gameplay_loop;
            world.apply_cheat(
                &legaia_engine_core::cheats::PlayerCheat::RandomEncounters(on),
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
            return true;
        }
        false
    }

    /// `F1`: toggle the shell's diagnostic text rows (scene / frame /
    /// mesh count, the camera + audio status line, the world-map camera
    /// readout, the no-encounter hint). Off by default - retail draws no
    /// text there, and the seat is the party readout's
    /// (`field_party_hud_draws`). `LEGAIA_DIAG_HUD` starts them on, the
    /// same variable the battle diagnostics read.
    fn key_diag_rows(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::F1) && state == ElementState::Pressed {
            self.diag_rows = !self.diag_rows;
            log::info!(
                "hud: diagnostic rows {}",
                if self.diag_rows { "ON" } else { "off" }
            );
            return true;
        }
        false
    }

    /// `F3`: toggle the field camera between the retail follow view
    /// (savestate-pinned pitch/yaw/H, player-anchored - the
    /// faithful framing) and the wide debug orbit vantage (better
    /// for eyeballing scene completeness). Moved off `C` for the same
    /// reason `F2` is not `V`: `C` is Triangle.
    fn key_debug_camera(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::F3)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            self.field_debug_camera = !self.field_debug_camera;
            log::info!(
                "camera: field vantage = {}",
                if self.field_debug_camera {
                    "debug orbit"
                } else {
                    "retail follow"
                }
            );
            return true;
        }
        false
    }

    /// `T`: cycle the field camera-distance preset (retail -> far ->
    /// farther). A pure framing knob on the follow / debug-orbit
    /// cameras - it never feeds the world simulation. Persisted to the
    /// options config file; the interactive default is `far`.
    fn key_camera_distance(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::KeyT)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            let next = self.options_state.cycle_camera_distance();
            self.session.camera.distance = next;
            self.persist_and_apply_options();
            log::info!("camera: distance = {} (T cycles)", next.label());
            return true;
        }
        false
    }

    /// `R`: toggle precise movement (opt-in, NON-RETAIL): free-angle
    /// camera-relative locomotion instead of retail's 4/8-way
    /// quantisation - key diagonals walk true 45-degree vectors and an
    /// analog stick's angle passes through continuously. Persisted to
    /// the options config file; default off (the faithful remap).
    fn key_precise_movement(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::KeyR)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            let on = !self.options_state.precise_movement;
            self.options_state.precise_movement = on;
            self.session.host.world.locomotion.precise_movement = on;
            self.persist_and_apply_options();
            log::info!(
                "movement: precise {}",
                if on {
                    "ON (free-angle - not retail)"
                } else {
                    "off (retail 8-way remap)"
                }
            );
            return true;
        }
        false
    }
}
