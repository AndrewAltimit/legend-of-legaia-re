//! The play window's render-side [`SceneResources`] build for the host's
//! currently loaded scene.

use super::*;

/// Build the play-window's render-side [`SceneResources`] for the host's
/// currently loaded scene: the shared blocks (`init_data` + `player_data`)
/// stay resident, the load kind mirrors the host's `enter_field_scene`
/// selection (WorldMap for `map\d\d`, Field otherwise), the boot-resident
/// system-UI bundle (raw PROT TOC entries 0/1 - the row-510/511 strip
/// CLUTs + the `(960,256)` menu-glyph atlas the town env meshes sample)
/// layers under the build via [`BuildOptions::system_ui`], and the field
/// character atlas is layered on. Used both for the initial scene at
/// window boot and to REBUILD the render state after a door transition
/// (`SceneTickEvent::SceneEntered`) swaps the host's scene.
pub(in crate::window) fn build_window_scene_resources(
    session: &NativeSession,
) -> Result<SceneResources> {
    let s = session
        .host
        .scene
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no scene loaded on the host"))?;
    // Load the shared blocks (`init_data` + `player_data`) so the
    // player TMD + shared UI atlas stay resident across field
    // transitions, then build with the targeted VRAM-upload
    // heuristic. Without this every prim sampled non-uploaded
    // VRAM regions and the filter dropped 100% of the mesh.
    let shared_scenes = crate::host_setup::load_shared_scenes(&session.host.index, |name, e| {
        log::warn!("play-window: shared block '{name}' not loaded: {e:#}");
    });
    let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();
    // Field-load model (matches retail FUN_8001F7C0 + the engine's
    // `enter_field_scene`): `SceneLoadKind::Field` skips the battle-
    // character `scene_tmd_stream` meshes, and the TMD scan now pulls
    // the town's environment geometry out of the scene_asset_table's
    // LZS-packed mesh pack (previously invisible to the raw scanner,
    // which left the field with a single stray battle mesh). Upload
    // every TIM, as retail's field loader DMAs the whole atlas - the
    // town meshes sample texture pages across all of VRAM, so a
    // render-targeted upload drops most of their prims.
    // World-map scenes (`map\d\d`) draw the kingdom-bundle slot-1
    // landmark pack, not the generic field sweep. Mirror the host's
    // `enter_field_scene` kind selection so the rendered meshes match the
    // gameplay-side resources (otherwise the window draws the Field-mode
    // 2-mesh fallback while the host loaded the full 40-TMD pack).
    let load_kind = if legaia_engine_scene::scene::is_world_map_scene(&s.name) {
        SceneLoadKind::WorldMap
    } else {
        SceneLoadKind::Field
    };
    // Boot-resident system-UI bundle (raw PROT TOC entries 0/1): the
    // pre-pass layers it under the scene build - image pages at their
    // declared rects, CLUTs as `FUN_800198E0` flat strips (rows 510/511
    // etc). Soft-fails to None (the affected prims just drop).
    let system_ui = match session.host.index.system_ui_bundle() {
        Ok(b) => Some(b),
        Err(err) => {
            log::warn!("play-window: system-UI bundle parse skipped: {err:#}");
            None
        }
    };
    // A pack map's entry already made this build (the same scene, override
    // and options; `SceneHost::expansion_entry_build`), most often on a
    // worker before the player reached the exit: start from it.
    let mut res = match session.host.expansion_entry_build() {
        Some(b) => (**b).clone(),
        None => {
            SceneResources::build_targeted_with_options(
                s,
                &shared_refs,
                BuildOptions {
                    kind: load_kind,
                    upload_all_tims: true,
                    system_ui: system_ui.as_deref(),
                },
            )?
            .0
        }
    };
    // The field's resident texture pool (`etim`: the `(448, 0)` page, the
    // `fb_y = 256` pages, CLUT strips on rows 473 / 475 / 478), layered
    // under the build by the step the scene host runs for the browser page
    // (`underlay_field_texture_pool`). It carries the fog sheets' texels
    // (texture page `0x27` is `(448, 0)`, CLUT `0x7640` is `(0, 473)`) and
    // the field-character atlas with its row-478 palette strips; every
    // PCSX-Redux field / world-map state in the library holds those cells.
    // Retail loads the pool before the scene, so a scene TIM on an
    // overlapping rect wins. This build used to write the character atlas
    // *over* the scene as well, which replaced the palettes a scene ships
    // for the party on row 478 (`bubu2`) with the pack's.
    // See `docs/subsystems/field-ambient-fx.md`.
    match legaia_engine_scene::scene::underlay_field_texture_pool(
        &session.host.index,
        &mut res.vram,
    ) {
        Ok(n) => log::info!("play-window: field texture pool underlaid ({n} TIMs)"),
        Err(err) => log::warn!("play-window: field texture pool underlay skipped: {err:#}"),
    }
    Ok(res)
}
