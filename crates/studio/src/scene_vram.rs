//! A model's textures in the context of its scene: the VRAM image the engine
//! builds when it loads the entry's CDNAME block.
//!
//! Most scene meshes sample texture pages and CLUTs that another entry of the
//! same block uploads (the scene's TIM pack, the row-479 NPC palettes, the
//! boot-resident system-UI bundle), so the entry's own TIMs are not enough.
//! This is `legaia_engine_scene::scene_resources::SceneResources` built the
//! way the play host builds a field: every TIM of the block plus the shared
//! `init_data` / `player_data` blocks, the system-UI bundle layered under,
//! [`SceneLoadKind::Field`] (or `WorldMap` for the kingdom maps). An entry
//! the field loader never uploads - a battle backdrop shell
//! (`scene_tmd_stream`) - takes the battle load kind instead.

use anyhow::{Context, Result};
use legaia_disc_audit::categorize::Class;
use legaia_engine_scene::scene::{Scene, is_world_map_scene};
use legaia_engine_scene::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};

use crate::Library;

/// A scene's VRAM image, as the engine builds it.
pub struct SceneVram {
    /// The CDNAME block that was loaded (the retail block of the entry).
    pub scene: String,
    pub kind: SceneLoadKind,
    pub vram: legaia_tim::Vram,
    /// TIMs uploaded, scene-local and shared.
    pub tims: usize,
    pub shared_tims: usize,
}

/// The load kind the engine would use for an entry of `class` in `scene`.
pub fn load_kind(scene: &str, class: Class) -> SceneLoadKind {
    if class == Class::SceneTmdStream {
        SceneLoadKind::Battle
    } else if is_world_map_scene(scene) {
        SceneLoadKind::WorldMap
    } else {
        SceneLoadKind::Field
    }
}

/// Build the VRAM of the scene that owns PROT entry `index`.
pub fn build(lib: &Library, index: u32, class: Class) -> Result<SceneVram> {
    let name = lib
        .retail_block(index)
        .context("this entry has no CDNAME block")?
        .to_string();
    let scene = Scene::load(&lib.index, &name).with_context(|| format!("loading scene {name}"))?;
    let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
        .iter()
        .filter_map(|n| Scene::load(&lib.index, n).ok())
        .collect();
    let shared_refs: Vec<&Scene> = shared.iter().collect();
    let system_ui = lib.index.system_ui_bundle().ok();
    let kind = load_kind(&name, class);
    let (res, _stats) = SceneResources::build_targeted_with_options(
        &scene,
        &shared_refs,
        BuildOptions {
            kind,
            upload_all_tims: true,
            system_ui: system_ui.as_deref(),
        },
    )
    .with_context(|| format!("building scene {name}"))?;
    Ok(SceneVram {
        scene: name,
        kind,
        vram: res.vram,
        tims: res.tim_count,
        shared_tims: res.shared_tim_count,
    })
}
