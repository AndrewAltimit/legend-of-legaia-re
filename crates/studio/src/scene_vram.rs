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
//! (`scene_tmd_stream`) - takes the battle load kind instead. Over the
//! build go the layers the native play host adds ([`add_host_layers`]): the
//! field-character atlas and the effect-texture pool.

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
    /// The host layers added over the build ([`add_host_layers`]).
    pub layers: Vec<String>,
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
    let mut vram = res.vram;
    let layers = add_host_layers(lib, kind, &mut vram);
    Ok(SceneVram {
        scene: name,
        kind,
        vram,
        tims: res.tim_count,
        shared_tims: res.shared_tim_count,
        layers,
    })
}

/// The two layers the native play host adds over `SceneResources` for a
/// field or world-map scene (`engine-shell` `build_window_scene_resources`),
/// so the party's field models and the effect sprites a scene draws sample
/// populated VRAM:
///
/// - the **field-character atlas**, PROT 874 section 2 entries 1..=3 (Vahn,
///   Noa and Gala's pages), uploaded with the flat-strip CLUT semantics of
///   `FUN_800198E0` (`legaia_actor_formats::field_char_textures`);
/// - the **effect-texture pool** (`etim`, the `(448, 0)` page and the
///   `fb_y = 256` pages with their CLUT strips), laid *under* the scene,
///   since retail loads it before the scene and a scene TIM on an
///   overlapping rect wins.
///
/// A battle load (a backdrop shell) takes only the effect pool, which stays
/// resident through battle. Returns one line per layer for the UI; a layer
/// that does not load is reported and skipped.
pub fn add_host_layers(
    lib: &Library,
    kind: SceneLoadKind,
    vram: &mut legaia_tim::Vram,
) -> Vec<String> {
    use legaia_actor_formats::field_char_textures as fct;
    let mut layers = Vec::new();
    if kind != SceneLoadKind::Battle {
        match lib
            .index
            .entry_bytes(fct::PROT_ENTRY_INDEX)
            .and_then(|b| fct::parse(&b))
        {
            Ok(mut pack) => {
                pack.textures.retain(|t| (1..=3).contains(&t.index));
                pack.upload_to_vram(vram, false);
                layers.push(format!(
                    "field-character atlas ({} pages)",
                    pack.textures.len()
                ));
            }
            Err(e) => layers.push(format!("field-character atlas skipped: {e:#}")),
        }
    }
    let mut pool = legaia_tim::Vram::new();
    match legaia_engine_scene::scene::upload_effect_textures_into_vram(&lib.index, &mut pool, true)
    {
        Ok(n) => {
            vram.underlay(&pool);
            layers.push(format!("effect-texture pool ({n} TIMs, underlaid)"));
        }
        Err(e) => layers.push(format!("effect-texture pool skipped: {e:#}")),
    }
    layers
}
