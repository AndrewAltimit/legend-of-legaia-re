//! Scene questions whose answer needs simulation state: whether a placed
//! field object is drawn against a running `World`'s windowed static-object
//! list, the story-hidden record set a scratch `World` staged at a scene's
//! free-roam visit evaluates, and the full static assembly built on that
//! set. The `World`-free draw resolver and assembly kernel these complete are
//! `legaia_engine_scene::field_env` and `legaia_engine_scene::scene_assembly`.

use legaia_engine_field::field_regions;
use legaia_engine_scene::field_env::PlacedWindowKey;
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_engine_scene::scene_assembly::{AssembledScene, assemble_field_scene_with};

/// Assemble a CDNAME scene's full static map with its story-hidden placed
/// objects filtered at the scene's canonical free-roam visit
/// ([`story_hidden_records_for_scene`]).
pub fn assemble_field_scene(index: &ProtIndex, name: &str) -> Result<AssembledScene, String> {
    assemble_field_scene_with(index, name, &story_hidden_records_for_scene)
}

/// Whether a placed draw is live this frame, against the scene's windowed
/// static-object list - the one kernel both play hosts ask.
///
/// A draw the init sweep owns (`key == None`) is always live. A window-owned
/// one is live when the port draws the whole map (the default -
/// [`legaia_engine_core::world::StaticObjectWindow::retail_windowing`] clear), and under
/// retail windowing only while the list holds a **drawn** actor for it: inside
/// the box the last camera re-centre latched, and with a non-zero draw kind
/// ([`legaia_engine_field::field_regions::WindowSpawn::drawn`]).
// REF: FUN_801D7B50 (the list), FUN_8001ADA4 (the draw-kind range check)
pub fn placed_draw_live(
    key: Option<&PlacedWindowKey>,
    window: &legaia_engine_core::world::StaticObjectWindow,
) -> bool {
    match key {
        None => true,
        Some(_) if !window.retail_windowing => true,
        Some(k) => window.draws(k),
    }
}

/// Evaluate a scene's story-hidden object records **without a live world** -
/// the `field_env::retain_visible_placed_draws` input for hosts that assemble a scene
/// statically (the site's full-map viewer, exporters).
///
/// Spins a scratch [`World`](legaia_engine_core::world::World), stages it at the scene's
/// canonical free-roam visit
/// ([`World::seed_free_roam_story_baseline`](legaia_engine_core::world::World::seed_free_roam_story_baseline)),
/// and pre-runs the object-bind spawn prologues exactly as `enter_field_scene`
/// does (`seed_object_channels` - the bind derivation mirrors the scene-entry
/// host). A scene with no MAN / no bindable objects returns the empty set
/// (nothing filtered).
pub fn story_hidden_records_for_scene(
    scene: &Scene,
    index: &ProtIndex,
) -> std::collections::HashSet<usize> {
    let Ok(Some(man_bytes)) = scene.field_man_payload(index) else {
        return Default::default();
    };
    let Ok(man_file) = legaia_scene_formats::man_section::parse(&man_bytes) else {
        return Default::default();
    };
    let (primary, fallback) = scene
        .field_tile_triggers(index)
        .unwrap_or((Vec::new(), Vec::new()));
    let triggers: Vec<field_regions::TileTrigger> = primary.into_iter().chain(fallback).collect();
    let map_bytes = scene
        .field_map_index(index)
        .and_then(|idx| index.entry_bytes_extended(idx).ok());
    let object_binds: Vec<(usize, (i16, i16))> = match map_bytes.as_deref() {
        Some(map) => {
            legaia_engine_field::man_field_scripts::object_script_bind_seats(map, &triggers)
        }
        None => triggers
            .iter()
            .filter(|t| t.gate == 0)
            .map(|t| {
                (
                    t.record as usize,
                    (
                        i16::from(t.tile_x) * 128 + 0x40,
                        i16::from(t.tile_z) * 128 + 0x40,
                    ),
                )
            })
            .collect(),
    };
    let mut w = legaia_engine_core::world::World::new();
    w.seed_free_roam_story_baseline(&scene.name);
    w.seed_field_channels(&man_file, &man_bytes);
    w.set_object_bind_rots(
        map_bytes
            .as_deref()
            .map(|map| {
                legaia_engine_field::man_field_scripts::object_script_bind_rots(map, &triggers)
            })
            .unwrap_or_default(),
    );
    w.seed_object_channels(&man_file, &man_bytes, &object_binds);
    w.hidden_object_records()
}
