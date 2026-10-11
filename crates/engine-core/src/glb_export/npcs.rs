//! One animated `.glb` per NPC model of a scene.

use super::*;

/// One exported NPC: the animated `.glb` plus everything a manifest needs to
/// place and label it.
pub struct NpcGlb {
    /// Index into the [`NpcCatalog::entries`] this was baked from.
    pub entry_index: usize,
    /// Suggested file stem (`npc_03_talk_old-man`), unique per scene.
    pub file_stem: String,
    pub glb: Vec<u8>,
    /// Baked clip names, primary (spawn) clip first.
    pub clips: Vec<String>,
}

/// Bake every catalogued (non-special) NPC into an animated `.glb`: the scene
/// TMD in the scene's field VRAM, its spawn clip first plus every other
/// bone-count-matching clip in the scene ANM bundle as extra takes. Entries
/// whose mesh produces no triangles are skipped. Files carry the export
/// scale on their root node ([`scale_glb_scene_roots`]).
pub fn export_npc_glbs(
    scene: &Scene,
    a: &AssembledScene,
    catalog: &NpcCatalog,
    opts: &GlbExportOptions,
) -> Vec<NpcGlb> {
    let bundle = scene_anm_bundle(scene);
    let mut out = Vec::new();
    for (i, e) in catalog.entries.iter().enumerate() {
        if e.special {
            continue;
        }
        let Some(rtmd) = a.res.tmds.get(e.placement.model_index as usize) else {
            continue;
        };
        let (mesh, object_ids, shading) =
            legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&rtmd.tmd, &rtmd.raw);
        if mesh.positions.is_empty() || mesh.indices.is_empty() {
            continue;
        }
        let nobj = rtmd.tmd.objects.len();
        // Spawn clip first (frame 0 = the rest pose a non-autoplaying viewer
        // shows), then the rest of the bundle's matching clips as variety.
        let mut anims: Vec<(
            String,
            legaia_actor_formats::monster_archive::MonsterAnimation,
        )> = Vec::new();
        if let Some(b) = bundle.as_ref() {
            let primary = (e.placement.anim_id as usize).checked_sub(1);
            let mut order: Vec<usize> = Vec::new();
            if let Some(p) = primary {
                order.push(p);
            }
            for rec in 0..b.record_count as usize {
                if Some(rec) != primary {
                    order.push(rec);
                }
            }
            for rec in order {
                let Ok(r) = b.record(rec) else { continue };
                if r.bone_count as usize != nobj {
                    continue;
                }
                if let Some(anim) = b.record_to_monster_animation(rec) {
                    let label = if Some(rec) == primary {
                        format!("spawn_record_{rec}")
                    } else {
                        format!("record_{rec}")
                    };
                    anims.push((label, anim));
                }
            }
        }
        let clips: Vec<CharacterClip<'_>> = anims
            .iter()
            .map(|(name, anim)| CharacterClip {
                name: name.clone(),
                fps: CLIP_FPS,
                anim,
            })
            .collect();
        let mut stem = format!("npc_{i:02}_{}", e.kind);
        if let Some(d) = &e.dialog {
            let s = slug(d);
            if !s.is_empty() {
                stem = format!("{stem}_{s}");
            }
        }
        let Some(glb) = build_character_glb_hybrid(
            &stem,
            &mesh,
            &object_ids,
            &a.res.vram,
            &clips,
            Some(&shading),
        ) else {
            continue;
        };
        out.push(NpcGlb {
            entry_index: i,
            file_stem: stem,
            glb: scale_glb_scene_roots(glb, opts.scale),
            clips: anims.into_iter().map(|(n, _)| n).collect(),
        });
    }
    out
}
