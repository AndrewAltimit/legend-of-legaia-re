//! Per-frame field presentation decisions both play hosts take before they
//! draw - the ones that need the engine's world and a render kernel at once,
//! which is why they live here rather than in `engine-core` or `engine-ui`.

use legaia_engine_core::scene::{SceneHost, is_world_map_scene};
use legaia_engine_core::world::SceneMode;
use legaia_engine_ui::scene_lighting::{
    PropLights, ScenePointLight, nearest_lights, place_prop_lights,
};

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
