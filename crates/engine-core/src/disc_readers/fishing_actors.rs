//! The fishing venue's actors (rod, lure, line, catch bursts) live in
//! `legaia_engine_fishing::fishing_actors`. This file keeps the one piece
//! that reads a loaded `Scene`: gathering the rod meshes and the bend out of
//! the venue.

use legaia_engine_fishing::fishing_actors::*;

/// Lift the three rods and the bend out of a loaded venue scene: the rod
/// models off the scene's op-`0x0E` model bank, the bend off its type-7 VDF
/// buffer. `None` when not one rod model resolves.
pub fn rod_mesh_from_scene(scene: &legaia_engine_scene::scene::Scene) -> Option<RodMesh> {
    let bank = legaia_engine_scene::model_bank::SceneModelBank::build(scene);
    let models = std::array::from_fn(|r| bank.tmd_bytes(scene, ROD_MODEL_BASE + r as i16));
    let bend = legaia_engine_scene::scene_bundle::find_vdf_buffer(scene).and_then(|buf| {
        legaia_engine_scene::scene_bundle::vdf_entry(&buf, ROD_BEND_VDF_ENTRY).map(<[u8]>::to_vec)
    });
    RodMesh::from_models(models, bend)
}
