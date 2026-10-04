//! The play page's **enhanced lighting** - the browser twin of the native
//! window's `I` / `F8` lighting, fed from the same engine-side source of
//! truth (`legaia_engine_ui::scene_lighting`).
//!
//! # This is NOT retail
//!
//! Retail field rendering has no light source; the faithful render is the
//! baked `texel * colour / 128` shading, and with the enhancement off the
//! page's shaders return it untouched. Everything here is presentation.
//!
//! # What is shared, what is per host
//!
//! * The derivation rules - which prims glow ([`scene_lighting`]'s blend +
//!   curated rules), how samples cluster into lights, the nearest-to-player
//!   pick, the glow sprites - are the one kernel both hosts call.
//! * The mood a frame is lit under is [`TimeOfDay::mood`] over the persisted
//!   [`OptionsState::lighting_time_of_day`] on both hosts.
//! * The input draw list is each host's own scene assembly: here the
//!   `.MAP` placement + terrain draws of [`crate::play::FieldRender`] (the
//!   shared `field_env` resolution) and the MAN actor catalog
//!   ([`crate::field_actors`]), instanced through the same
//!   [`scene_lighting::placement_model`] / live-anchor kernels.
//!
//! The point lights' shadow maps are GPU work over each host's own draw
//! list: the page renders them in `site/js/webgl-tmd.js`
//! (`_renderLightShadows`) from the light positions this packet carries (see
//! `docs/tooling/host-drift.md`).
//!
//! [`OptionsState::lighting_time_of_day`]: legaia_engine_core::options::OptionsState::lighting_time_of_day

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::world::SceneMode;
use legaia_engine_ui::scene_lighting::{self as sl, PropLights, ScenePointLight, TimeOfDay};
use wasm_bindgen::prelude::*;

/// The scene's derived lights, built once per scene entry.
#[derive(Default)]
pub(crate) struct SceneLightCache {
    /// The scene the cache was derived for.
    pub scene: String,
    /// The static (`.MAP` placement + terrain) lights, world space.
    pub statics: Vec<ScenePointLight>,
    /// The MAN actor props' light sets at their spawn anchors.
    pub props: Vec<PropLights>,
}

impl LegaiaRuntime {
    fn scene_name(&self) -> String {
        self.scene_host
            .as_ref()
            .and_then(|h| h.scene.as_ref())
            .map(|s| s.name.clone())
            .unwrap_or_default()
    }

    /// The mood this frame is lit under: the persisted time of day over the
    /// running scene (the native redraw's call).
    pub(crate) fn lighting_mood(&self) -> sl::LightingMood {
        TimeOfDay::from_name(&self.options_state.lighting_time_of_day)
            .unwrap_or_default()
            .mood(&self.scene_name())
    }

    /// Derive (once per scene) the static + prop light sets.
    fn ensure_scene_lights(&mut self) {
        let name = self.scene_name();
        if self.scene_lights.as_ref().is_some_and(|c| c.scene == name) {
            return;
        }
        let mut cache = SceneLightCache {
            scene: name,
            ..Default::default()
        };
        if let (Some(f), Some(res)) = (self.field.as_ref(), self.res()) {
            let mut samples = Vec::new();
            let mut per_mesh: std::collections::HashMap<usize, Vec<sl::EmitterSample>> =
                std::collections::HashMap::new();
            for d in f.terrain.iter().chain(&f.placements) {
                let em = per_mesh.entry(d.res_tmd).or_insert_with(|| {
                    let Some(rtmd) = res.tmds.get(d.res_tmd) else {
                        return Vec::new();
                    };
                    let (mut mesh, flat) =
                        crate::field_scene::build_hybrid_env_mesh(rtmd, &res.vram);
                    let hit = sl::tag_emissive_hybrid(&rtmd.raw, &mut mesh, &flat, &res.vram);
                    let mut em = sl::hybrid_mesh_emitters(
                        &mesh.positions,
                        &mesh.cba_tsb,
                        &mesh.colors,
                        &flat,
                        &mesh.indices,
                        Some((&res.vram, &mesh.uvs)),
                    );
                    if let Some(h) = hit {
                        em.push(sl::curated_mesh_sample(&h));
                    }
                    em
                });
                if em.is_empty() {
                    continue;
                }
                let model = sl::placement_model(
                    [d.world_x as f32, d.world_y as f32, d.world_z as f32],
                    [d.rot_x, d.rot_y, d.rot_z],
                    1.0,
                );
                samples.extend(sl::transform_samples(em, &model));
            }
            cache.statics = sl::cluster_all_scene_lights(&samples);
        }
        if let Some(host) = self.scene_host.as_ref() {
            let banks = crate::field_actors::ActorBanks {
                scene_anm: self.scene_anm.as_ref(),
                locomotion_anm: self.locomotion_anm.as_ref(),
            };
            cache.props = self.actors.prop_light_sets(host, banks);
        }
        self.scene_lights = Some(cache);
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Turn enhanced lighting on / off (the native window's `I`), persisted
    /// with the other options.
    pub fn set_enhanced_lighting(&mut self, on: bool) {
        if self.options_state.enhanced_lighting != on {
            self.options_state.enhanced_lighting = on;
            self.persist_and_apply_options();
        }
    }

    /// Whether enhanced lighting is on - the persisted option the page's
    /// checkbox reflects on load.
    pub fn enhanced_lighting(&self) -> bool {
        self.options_state.enhanced_lighting
    }

    /// Set the time of day (`auto` / `day` / `dusk` / `night` - the native
    /// window's `F8` cycle), persisted. Returns the name now in effect (an
    /// unknown name reads as `auto`).
    pub fn set_lighting_time_of_day(&mut self, name: &str) -> String {
        let t = TimeOfDay::from_name(name).unwrap_or_default();
        self.options_state.lighting_time_of_day = t.name().to_string();
        self.persist_and_apply_options();
        t.name().to_string()
    }

    /// The persisted time-of-day name.
    pub fn lighting_time_of_day(&self) -> String {
        TimeOfDay::from_name(&self.options_state.lighting_time_of_day)
            .unwrap_or_default()
            .name()
            .to_string()
    }

    /// One frame of enhanced lighting for the page, against the camera basis
    /// it draws with (`right` / `up`, retail Y-down frame). Layout (`f32`):
    ///
    /// * `[0..12]` - the mood's three shader words
    ///   ([`sl::LightingMood::uniforms`], enable = 1);
    /// * `[12..16]` - [`sl::LightingMood::window_word`] (the window glow);
    /// * `[16..]` - [`sl::frame_packet`]: the picked lights (nearest the
    ///   player, mood lamp strength folded in) and the glow-sprite quads.
    ///
    /// Lights and glow only on a field frame (not the world map), as the
    /// native window stages them; any other frame carries the mood alone.
    #[allow(clippy::too_many_arguments)] // two 3-vectors, flat for wasm-bindgen
    pub fn play_lighting_frame(
        &mut self,
        rx: f32,
        ry: f32,
        rz: f32,
        ux: f32,
        uy: f32,
        uz: f32,
    ) -> Vec<f32> {
        let mood = self.lighting_mood();
        let mut out: Vec<f32> = mood.uniforms(true).iter().flatten().copied().collect();
        out.extend_from_slice(&mood.window_word());
        let field = self.scene_host.as_ref().is_some_and(|h| {
            h.world.mode == SceneMode::Field
                && !h
                    .scene
                    .as_ref()
                    .is_some_and(|s| legaia_engine_core::scene::is_world_map_scene(&s.name))
        });
        if !field {
            out.extend_from_slice(&[0.0, 0.0]);
            return out;
        }
        self.ensure_scene_lights();
        let (Some(host), Some(cache)) = (self.scene_host.as_ref(), self.scene_lights.as_ref())
        else {
            out.extend_from_slice(&[0.0, 0.0]);
            return out;
        };
        let w = &host.world;
        let focus = w
            .player_actor_slot
            .and_then(|s| w.actors.get(s as usize))
            .map(|a| {
                [
                    a.move_state.world_x as f32,
                    a.move_state.world_y as f32,
                    a.move_state.world_z as f32,
                ]
            })
            .unwrap_or([0.0; 3]);
        let mut all = cache.statics.clone();
        all.extend(sl::place_prop_lights(&cache.props, |slot, spawn| {
            w.field_npc_live_anchor(slot, spawn)
        }));
        out.extend(sl::frame_packet(
            &all,
            focus,
            &mood,
            [rx, ry, rz],
            [ux, uy, uz],
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The packet layout the page parses: sixteen mood words, then the two
    /// counts. A host with no scene answers the mood and zero lights.
    #[test]
    fn frame_layout_without_a_scene() {
        let mut rt = LegaiaRuntime::new();
        let f = rt.play_lighting_frame(1.0, 0.0, 0.0, 0.0, -1.0, 0.0);
        assert_eq!(f.len(), 18);
        assert_eq!(f[3], 1.0, "enable word");
        assert_eq!(&f[16..], &[0.0, 0.0]);
    }
}
