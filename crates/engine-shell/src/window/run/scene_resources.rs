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
    let (mut res, _stats) = SceneResources::build_targeted_with_options(
        s,
        &shared_refs,
        BuildOptions {
            kind: load_kind,
            upload_all_tims: true,
            system_ui: system_ui.as_deref(),
        },
    )?;
    // Field-character atlas upload (PROT 0874 §2, the `FUN_800198e0`
    // chain): entries 1/2/3 are the Vahn/Noa/Gala atlas pages whose
    // palettes live as **flat strips** on CLUT row 478. The generic TIM
    // scan uploads the pages but places these CLUTs as declared rects
    // (rows 478..481 col 0), so the meshes sample an unpopulated row and
    // the VRAM filter drops them - the invisible-player symptom. Retail
    // field load uploads the pack with strip semantics; replicate that.
    // The rest of the section is layered *under* the build below, never
    // written over it.
    match session
        .host
        .index
        .entry_bytes(legaia_actor_formats::field_char_textures::PROT_ENTRY_INDEX)
        .and_then(|b| legaia_actor_formats::field_char_textures::parse(&b))
    {
        Ok(mut pack) => {
            // Entries 1/2/3 only (the character atlas pages). The pack's
            // other entries land on pages the town env meshes sample -
            // uploading them here drops ~26 scene meshes to the filter.
            pack.textures.retain(|t| (1..=3).contains(&t.index));
            pack.upload_to_vram(&mut res.vram, false);
            log::info!(
                "play-window: field char atlas uploaded ({} TIMs, strip CLUTs)",
                pack.textures.len()
            );
        }
        Err(err) => {
            log::warn!("play-window: field char atlas upload skipped: {err:#}");
        }
    }
    // The whole effect-texture pool of that section (`etim`: the
    // `(448, 0)` page, the `fb_y = 256` pages, CLUT strips on rows 473 /
    // 475 / 478) is resident in retail field and world-map VRAM, and it
    // is where the field fog sheets sample: texture page `0x27` is
    // `(448, 0)`, the wisps sit at rows `0x40..0x6F` of it, CLUT `0x7640`
    // is `(0, 473)`. Every PCSX-Redux field / world-map state in the
    // library holds those cells byte-exact. Retail loads the pool before
    // the scene, so a scene TIM on an overlapping rect wins (`dolk`'s
    // `(448, 0)` page keeps its own texels in retail) - the boot-resident
    // underlay order `SceneResources` uses for the system-UI bundle.
    // Without it the fog quads sampled all-zero texels and the shader
    // discarded every fragment. See `docs/subsystems/field-ambient-fx.md`.
    let mut effect_pool = legaia_tim::Vram::new();
    match legaia_engine_scene::scene::upload_effect_textures_into_vram(
        &session.host.index,
        &mut effect_pool,
        true,
    ) {
        Ok(n) => {
            res.vram.underlay(&effect_pool);
            log::info!("play-window: effect-texture pool underlaid ({n} TIMs)");
        }
        Err(err) => {
            log::warn!("play-window: effect-texture pool underlay skipped: {err:#}");
        }
    }
    Ok(res)
}
