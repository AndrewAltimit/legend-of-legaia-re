//! Per-frame field presentation decisions both play hosts take before they
//! draw - the ones that need the engine's world and a render kernel at once,
//! which is why they live here rather than in `engine-core` or `engine-ui`.

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::scene::is_world_map_scene;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_render_kernels::scene_lighting::{
    LightingMood, PropLights, ScenePointLight, TimeOfDay, fog_tint, nearest_lights,
    place_prop_lights,
};

/// The mood enhanced lighting lights this frame under: the persisted time
/// of day (`time_of_day`, an options-file name - an unknown one reads as the
/// default) over the running scene and the scripted grade its world holds
/// (`World::held_scene_grade`). `host` is `None` before a scene runs, which
/// grades the time of day alone over no scene name.
///
/// Both play hosts light, tint the mist bank and size the bloom from this
/// one answer; each used to spell the scene-name and grade lookups itself.
pub fn lighting_mood(host: Option<&SceneHost>, time_of_day: &str) -> LightingMood {
    let scene = host
        .and_then(|h| h.scene.as_ref())
        .map(|s| s.name.as_str())
        .unwrap_or("");
    let grade = host.and_then(|h| h.world.held_scene_grade());
    TimeOfDay::from_name(time_of_day)
        .unwrap_or_default()
        .mood_graded(scene, grade)
}

/// The volumetric fog bank's colour this frame: the bank's own `color`, or,
/// with enhanced lighting on, that colour in the scene's mood light
/// (`scene_lighting::fog_tint`), so the mist follows the time of day on
/// both hosts.
pub fn fog_volume_color(color: [f32; 3], enhanced_lighting: bool, mood: &LightingMood) -> [f32; 3] {
    if enhanced_lighting {
        fog_tint(color, mood)
    } else {
        color
    }
}

/// This frame's derived scene point lights for the enhanced-lighting layer,
/// picked nearest the player, with the focus they were picked against -
/// `None` when the layer does not light this frame.
///
/// The layer is field free-roam only: battle, the world map (by mode, or a
/// world-map scene still in field mode) and any screen that owns the frame
/// (`screen_owned`: the boot UI, or a menu-overlay screen with the field
/// faded out, `MenuRuntime::covers_field`) light nothing, so the halos never
/// glow through the black behind a shop's windows. `statics` are the scene's
/// placement and terrain lights; `props` the MAN actor props' sets, each
/// moved to its actor's live anchor (`World::field_npc_live_anchor`, the
/// anchor the NPC draw uses). The focus is the player actor, the origin when
/// none is seated.
///
/// The native window and the browser play page each spelled this gate out,
/// and the page's lacked the screen term, so a shop's black backdrop carried
/// the scene's candle halos there.
pub fn field_scene_lights(
    host: &SceneHost,
    screen_owned: bool,
    statics: &[ScenePointLight],
    props: &[PropLights],
) -> Option<(Vec<ScenePointLight>, [f32; 3])> {
    let w = &host.world;
    if screen_owned
        || w.mode != SceneMode::Field
        || host
            .scene
            .as_ref()
            .is_some_and(|s| is_world_map_scene(&s.name))
        || (statics.is_empty() && props.is_empty())
    {
        return None;
    }
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
    let mut all = statics.to_vec();
    all.extend(place_prop_lights(props, |slot, spawn| {
        w.field_npc_live_anchor(slot, spawn)
    }));
    Some((nearest_lights(&all, focus), focus))
}

/// The frame's clear colour, through the shared
/// `battle_stage_clear::scene_clear` selector, with the screen term decided
/// here: a boot UI screen (`boot_ui`, the host's) or a menu-overlay screen
/// with the field faded out (`MenuRuntime::covers_field`) clears retail
/// black, a stage battle clears the stage's black, and every other frame
/// clears to the field's own `r0 / g0 / b0` (`World::frame_clear_rgb`).
///
/// The play page used to pass no screen term at all, so a shop opened in a
/// scene whose op `4C 13` sets a clear colour (`teien`'s sea blue) sat on
/// that colour there and on black natively. Retail's shop runs with the
/// field overlay swapped out and the frame black (`MenuRuntime::covers_field`).
pub fn frame_clear_color(
    world: &legaia_engine_core::world::World,
    boot_ui: bool,
    menu: &legaia_engine_menu_runtime::menu_runtime::MenuRuntime,
    stage_battle: bool,
) -> [f32; 4] {
    legaia_engine_ui::battle_stage_clear::scene_clear(
        boot_ui || menu.covers_field(),
        stage_battle,
        world.frame_clear_rgb(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With no scene running, the mood is the time of day's alone, and an
    /// unknown option name reads as the default (`Auto`).
    #[test]
    fn mood_without_a_scene_is_the_time_of_day_alone() {
        assert_eq!(
            lighting_mood(None, "night"),
            TimeOfDay::Night.mood_graded("", None)
        );
        assert_eq!(
            lighting_mood(None, "not-a-time"),
            TimeOfDay::Auto.mood_graded("", None)
        );
    }

    /// Enhanced lighting off leaves the bank's colour as authored; on, it is
    /// the mood's tint of it.
    #[test]
    fn fog_colour_tints_only_under_enhanced_lighting() {
        let mood = TimeOfDay::Night.mood_graded("", None);
        let c = [0.5, 0.6, 0.7];
        assert_eq!(fog_volume_color(c, false, &mood), c);
        assert_eq!(fog_volume_color(c, true, &mood), fog_tint(c, &mood));
    }
}
