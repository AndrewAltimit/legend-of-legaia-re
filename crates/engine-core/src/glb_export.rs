//! Native **world-export composition**: bake an [`AssembledScene`] into the
//! textured `.glb` + JSON manifest set the `legaia-engine export-glb`
//! subcommand writes for Unity / VRChat world building (see
//! `docs/tooling/vrchat-world-export.md`).
//!
//! Three artifact families per scene, matching retail's own layering:
//!
//! - the **world glb**: ground heightfield + terrain tiles + placed objects,
//!   every draw instanced at its resolved world transform with its coplanar
//!   lift, bound placements posed at **frame 0** of their object-bind clip
//!   (the native play-window's cheap static path);
//! - **NPC glbs**: one animated `.glb` per catalogued MAN placement, the
//!   scene TMD posed by its ANM record with every bone-count-matching clip in
//!   the scene bundle baked as a named glTF animation - spawn transforms go
//!   in the manifest, not the file, so a world builder places them;
//! - **animated-prop glbs**: each distinct `(env mesh, clip)` pair whose
//!   bind clip has more than one frame (windmill sails, doors), exported
//!   object-local with the clip, plus every placement transform in the
//!   manifest. The world glb keeps their frame-0 static twins, so these are
//!   opt-in upgrades, not required parts.
//!
//! Everything composes through the same kernels the browser pages render
//! with ([`crate::scene_assembly`], [`crate::npc_catalog`],
//! `legaia_asset::scene_gltf` / `character_gltf`), so the exported files
//! match the on-screen viewers. The output contains Sony-derived asset data:
//! it is user-generated, local, and must never be committed or redistributed.

use crate::npc_catalog::{NpcCatalog, scene_anm_bundle};
use crate::scene::{ProtIndex, Scene};
use crate::scene_assembly::{
    AssembledScene, build_hybrid_env_mesh, build_hybrid_env_mesh_posed, draw_rot_y_radians,
    draw_translation, is_sky_mesh,
};
use legaia_asset::character_gltf::{CharacterClip, build_character_glb_hybrid};
use legaia_asset::player_anm::PlayerAnmBundle;
use legaia_asset::scene_gltf::{SceneInstance, SceneMesh, build_scene_glb};
use serde_json::{Value, json};
use std::collections::HashMap;

/// Placement transforms of one animated prop: `(translation, rot_y radians)`
/// per placed instance, already in the export frame.
type PropInstances = Vec<([f32; 3], f32)>;

/// Field/duel clip playback rate baked into exported animation timelines -
/// the observed retail field animator cadence (see the characters page's
/// export, `web-viewer::character`).
const CLIP_FPS: f32 = 14.0;

/// Export tuning.
#[derive(Debug, Clone, Copy)]
pub struct GlbExportOptions {
    /// Uniform scale applied to every instance transform (translations and
    /// mesh scale alike), so `1.0` exports raw PSX world units and e.g.
    /// `1.0 / 64.0` makes one 128-unit walk tile two glTF meters - about
    /// right for VRChat player scale.
    pub scale: f32,
    /// Keep the sky-backdrop shells ([`is_sky_mesh`]) in the world glb. The
    /// site viewers hide them (they read as a giant intersecting plane from
    /// outside); inside a VR world they can serve as the horizon dressing.
    pub include_sky: bool,
}

impl Default for GlbExportOptions {
    fn default() -> Self {
        Self {
            scale: 1.0,
            include_sky: false,
        }
    }
}

/// A baked world glb plus the composition stats a caller reports.
pub struct WorldGlb {
    pub glb: Vec<u8>,
    pub mesh_count: usize,
    pub instance_count: usize,
    /// Draws dropped by the sky heuristic (0 when `include_sky`).
    pub sky_hidden: usize,
    /// Suggested player spawn in the export frame (already scaled): the
    /// component-wise median of the placed objects - the village centre, not
    /// the map-grid centre (see the field-scene page's VR spawn note).
    pub spawn: [f32; 3],
    /// Ground heightfield quad count (0 = scene had no walk floor grid).
    pub ground_quads: usize,
}

/// Frame-`frame` bone offsets of scene ANM record `anim_id - 1`, under
/// retail's count-equality contract (`FUN_8001b964` refuses to draw when the
/// mesh chain and the clip disagree). `None` = pose nothing.
fn frame_bone_offsets(
    bundle: &PlayerAnmBundle,
    anim_id: u8,
    nobj: usize,
    frame: usize,
) -> Option<Vec<([i16; 3], [i16; 3])>> {
    let rec_idx = (anim_id as usize).checked_sub(1)?;
    let rec = bundle.record(rec_idx).ok()?;
    let bones = rec.bone_count as usize;
    if bones != nobj {
        return None;
    }
    let f = frame.min((rec.frame_count as usize).saturating_sub(1));
    Some(
        (0..bones)
            .map(|b| match bundle.bone_transform(rec_idx, f, b) {
                Some(t) => (
                    [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                    [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                ),
                None => ([0; 3], [0; 3]),
            })
            .collect(),
    )
}

/// Bake the assembled scene's static map into one `.glb`: ground + terrain
/// tiles + placed objects (bound placements posed at clip frame 0), the same
/// draw list the field-scene page renders and exports, with the site's
/// placement-transform conventions baked in. `Err` only on structural
/// failure; a scene with no drawable geometry returns an empty `glb`.
pub fn export_world_glb(
    scene: &Scene,
    a: &AssembledScene,
    opts: &GlbExportOptions,
) -> Result<WorldGlb, String> {
    let bundle = scene_anm_bundle(scene);
    let mut meshes: Vec<SceneMesh> = Vec::new();
    let mut instances: Vec<SceneInstance> = Vec::new();
    let mut sky_hidden = 0usize;
    let mut ground_quads = 0usize;

    if let Some(hf) = &a.ground {
        ground_quads = hf.quad_count();
        // Ground sinks below the env pack's authored floor art (see
        // `coplanar_draws::GROUND_SINK`), same as the page's ground stream.
        let mut positions = Vec::with_capacity(hf.positions.len() * 3);
        for p in &hf.positions {
            positions.push(p[0]);
            positions.push(p[1] + crate::coplanar_draws::GROUND_SINK);
            positions.push(p[2]);
        }
        let mut uvs = Vec::with_capacity(hf.uvs.len() * 2);
        for uv in &hf.uvs {
            uvs.extend_from_slice(uv);
        }
        let mut cba_tsb = Vec::with_capacity(hf.cba_tsb.len() * 2);
        for ct in &hf.cba_tsb {
            cba_tsb.extend_from_slice(ct);
        }
        meshes.push(SceneMesh {
            name: "ground".to_string(),
            positions,
            uvs,
            cba_tsb,
            indices: hf.indices.clone(),
            flat_rgba: Vec::new(),
        });
        instances.push(SceneInstance {
            mesh: 0,
            translation: [0.0; 3],
            rot_y: 0.0,
            scale: opts.scale,
        });
    }

    // Terrain first, then placed objects - the native draw sequence. A mesh
    // is registered once per (env slot, posing clip) and instanced.
    let mut handles: HashMap<(usize, u8), Option<usize>> = HashMap::new();
    let mut spawn_src: Vec<[f32; 3]> = Vec::new();
    for (layer, draws) in [(0usize, &a.terrain), (1, &a.placements)] {
        for d in draws {
            let key = (d.env_slot, d.anim_id);
            let handle = *handles.entry(key).or_insert_with(|| {
                let rtmd = &a.res.tmds[d.res_tmd];
                let offsets = (d.anim_id != 0)
                    .then(|| {
                        bundle.as_ref().and_then(|b| {
                            frame_bone_offsets(b, d.anim_id, rtmd.tmd.objects.len(), 0)
                        })
                    })
                    .flatten();
                let (mesh, flat) = match &offsets {
                    Some(o) => build_hybrid_env_mesh_posed(rtmd, o),
                    None => build_hybrid_env_mesh(rtmd, &a.res.vram),
                };
                if mesh.positions.is_empty() || mesh.indices.is_empty() {
                    return None;
                }
                if !opts.include_sky && is_sky_mesh(&mesh) {
                    // Marker: a registered-but-sky mesh. Encoded as usize::MAX
                    // so per-draw accounting below still counts each hidden
                    // instance.
                    return Some(usize::MAX);
                }
                let mut positions = Vec::with_capacity(mesh.positions.len() * 3);
                for p in &mesh.positions {
                    positions.extend_from_slice(p);
                }
                let mut uvs = Vec::with_capacity(mesh.uvs.len() * 2);
                for uv in &mesh.uvs {
                    uvs.extend_from_slice(uv);
                }
                let mut cba_tsb = Vec::with_capacity(mesh.cba_tsb.len() * 2);
                for ct in &mesh.cba_tsb {
                    cba_tsb.extend_from_slice(ct);
                }
                meshes.push(SceneMesh {
                    name: match d.anim_id {
                        0 => format!("mesh_{}", d.env_slot),
                        anim => format!("mesh_{}_anim{}", d.env_slot, anim),
                    },
                    positions,
                    uvs,
                    cba_tsb,
                    indices: mesh.indices.clone(),
                    flat_rgba: flat,
                });
                Some(meshes.len() - 1)
            });
            let Some(mi) = handle else { continue };
            if mi == usize::MAX {
                sky_hidden += 1;
                continue;
            }
            let t = draw_translation(a.draw_world_position(d));
            let t = [t[0] * opts.scale, t[1] * opts.scale, t[2] * opts.scale];
            if layer == 1 {
                spawn_src.push(t);
            }
            instances.push(SceneInstance {
                mesh: mi,
                translation: t,
                rot_y: draw_rot_y_radians(d.rot_y),
                scale: opts.scale,
            });
        }
    }

    // Spawn = component-wise median of the placed objects (a town usually
    // occupies a corner of its 128x128-tile grid, so the AABB centre lands on
    // empty ground; the median lands in the village).
    let median = |axis: usize, src: &[[f32; 3]]| -> f32 {
        if src.is_empty() {
            return 0.0;
        }
        let mut vals: Vec<f32> = src.iter().map(|p| p[axis]).collect();
        vals.sort_by(f32::total_cmp);
        vals[(vals.len() - 1) / 2]
    };
    let spawn = [
        median(0, &spawn_src),
        median(1, &spawn_src),
        median(2, &spawn_src),
    ];

    let glb = build_scene_glb(&a.name, &meshes, &instances, &a.res.vram).unwrap_or_default();
    Ok(WorldGlb {
        glb,
        mesh_count: meshes.len(),
        instance_count: instances.len(),
        sky_hidden,
        spawn,
        ground_quads,
    })
}

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

/// Slug a dialog label into a filename fragment.
fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().take(24) {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if (c == ' ' || c == '-' || c == '_') && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Bake every catalogued (non-special) NPC into an animated `.glb`: the scene
/// TMD in the scene's field VRAM, its spawn clip first plus every other
/// bone-count-matching clip in the scene ANM bundle as extra takes. Entries
/// whose mesh produces no triangles are skipped.
pub fn export_npc_glbs(scene: &Scene, a: &AssembledScene, catalog: &NpcCatalog) -> Vec<NpcGlb> {
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
        let mut anims: Vec<(String, legaia_asset::monster_archive::MonsterAnimation)> = Vec::new();
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
            glb,
            clips: anims.into_iter().map(|(n, _)| n).collect(),
        });
    }
    out
}

/// One exported animated prop: a distinct `(env mesh, clip)` pair whose
/// object-bind clip really moves (more than one frame), object-local with
/// the clip baked, plus every placement transform (already in the export
/// frame, scaled).
pub struct PropGlb {
    pub env_slot: usize,
    pub anim_id: u8,
    pub file_stem: String,
    pub glb: Vec<u8>,
    pub frame_count: u16,
    /// `(translation, rot_y radians)` per placed instance.
    pub instances: PropInstances,
}

/// Export the scene's animated placed props (windmill sails, doors, gates):
/// every distinct `(env_slot, anim_id)` placement pair whose clip has more
/// than one frame and matches the mesh's object count. The world glb keeps
/// their frame-0 static twins - a world builder swaps these in where the
/// motion is wanted.
pub fn export_animated_prop_glbs(
    scene: &Scene,
    a: &AssembledScene,
    opts: &GlbExportOptions,
) -> Vec<PropGlb> {
    let Some(bundle) = scene_anm_bundle(scene) else {
        return Vec::new();
    };
    let mut by_key: HashMap<(usize, u8), PropInstances> = HashMap::new();
    let mut key_order: Vec<(usize, u8)> = Vec::new();
    for d in &a.placements {
        if d.anim_id == 0 {
            continue;
        }
        let key = (d.env_slot, d.anim_id);
        let t = draw_translation(a.draw_world_position(d));
        let t = [t[0] * opts.scale, t[1] * opts.scale, t[2] * opts.scale];
        let entry = by_key.entry(key).or_default();
        if entry.is_empty() {
            key_order.push(key);
        }
        entry.push((t, draw_rot_y_radians(d.rot_y)));
    }
    let mut out = Vec::new();
    for key in key_order {
        let (env_slot, anim_id) = key;
        let rec_idx = (anim_id as usize) - 1;
        let Ok(rec) = bundle.record(rec_idx) else {
            continue;
        };
        if rec.frame_count <= 1 {
            continue;
        }
        let Some(&res_tmd) = a.env_tmds.get(env_slot) else {
            continue;
        };
        let rtmd = &a.res.tmds[res_tmd];
        if rec.bone_count as usize != rtmd.tmd.objects.len() {
            continue;
        }
        let (mesh, object_ids, shading) =
            legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&rtmd.tmd, &rtmd.raw);
        if mesh.positions.is_empty() || mesh.indices.is_empty() {
            continue;
        }
        let Some(anim) = bundle.record_to_monster_animation(rec_idx) else {
            continue;
        };
        let stem = format!("prop_{env_slot}_anim{anim_id}");
        let clips = [CharacterClip {
            name: format!("record_{rec_idx}"),
            fps: CLIP_FPS,
            anim: &anim,
        }];
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
        out.push(PropGlb {
            env_slot,
            anim_id,
            file_stem: stem,
            glb,
            frame_count: rec.frame_count,
            instances: by_key.remove(&key).unwrap_or_default(),
        });
    }
    out
}

/// A floor-height sampler over the scene's `.MAP` collision grid, floor LUT
/// and elevation overrides - the same retail model the play hosts run
/// (`World::sample_field_floor_height`, `FUN_80019278`), so manifest spawn
/// heights match where the engine grounds an actor.
pub struct FloorSampler {
    world: Box<crate::world::World>,
}

impl FloorSampler {
    /// Build from the scene's on-disc geometry; every input is optional (a
    /// missing grid just samples 0, same as retail before the install).
    pub fn build(index: &ProtIndex, scene: &Scene) -> Self {
        let mut world = Box::<crate::world::World>::default();
        if let Ok(Some(grid)) = scene.field_collision_grid(index) {
            world.load_field_collision_grid(&grid);
        }
        if let Some(idx) = scene.field_map_index(index)
            && let Ok(bytes) = index.entry_bytes_extended(idx)
        {
            world.load_field_object_cells(
                bytes
                    .get(legaia_asset::field_objects::OBJECT_GRID_OFFSET..)
                    .unwrap_or_default(),
            );
            world.load_field_elevation_overrides(
                bytes
                    .get(crate::field_regions::MAP_REGION_BLOCK_OFFSET..)
                    .unwrap_or_default(),
                bytes
                    .get(crate::field_regions::MAP_TRIGGER_FALLBACK_OFFSET..)
                    .unwrap_or_default(),
            );
        }
        if let Ok(Some(lut)) = scene.field_floor_height_lut(index) {
            // The MAN header stores POSITIVE tiers; every runtime consumer
            // assumes the negated (PSX Y-down) copy (`FUN_8003AEB0`).
            world.field_floor_height_lut = lut.map(|v| v.wrapping_neg());
        }
        Self { world }
    }

    /// Floor height at a world (x, z), retail frame (PSX +Y down).
    pub fn height(&self, world_x: i32, world_z: i32) -> i32 {
        self.world.sample_field_floor_height(world_x, world_z)
    }
}

/// Compose the per-scene manifest JSON: everything a world builder (or the
/// shipped Unity importer script) needs to place the exported files -
/// transforms in the export frame, file names, labels, and the coordinate /
/// scale conventions spelled out.
#[allow(clippy::too_many_arguments)]
pub fn world_manifest(
    a: &AssembledScene,
    opts: &GlbExportOptions,
    world: &WorldGlb,
    world_file: &str,
    catalog: Option<&NpcCatalog>,
    npcs: &[NpcGlb],
    props: &[PropGlb],
    floor: &FloorSampler,
) -> Value {
    let npc_dir_entries: Vec<Value> = npcs
        .iter()
        .filter_map(|n| {
            let e = catalog?.entries.get(n.entry_index)?;
            let p = &e.placement;
            let y = floor.height(p.world_x as i32, p.world_z as i32);
            let t = draw_translation([p.world_x as f32, y as f32, p.world_z as f32]);
            Some(json!({
                "file": format!("npcs/{}.glb", n.file_stem),
                "kind": e.kind,
                "label": e.dialog,
                "position": [t[0] * opts.scale, t[1] * opts.scale, t[2] * opts.scale],
                "clips": n.clips,
                "conditional": e.conditional,
                "target_map": e.target_map,
                "model_index": p.model_index,
                "anim_id": p.anim_id,
            }))
        })
        .collect();
    let door_entries: Vec<Value> = catalog
        .map(|c| {
            c.entries
                .iter()
                .filter(|e| e.kind == "door")
                .map(|e| {
                    let p = &e.placement;
                    let y = floor.height(p.world_x as i32, p.world_z as i32);
                    let t = draw_translation([p.world_x as f32, y as f32, p.world_z as f32]);
                    json!({
                        "target_map": e.target_map,
                        "position": [t[0] * opts.scale, t[1] * opts.scale, t[2] * opts.scale],
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let prop_entries: Vec<Value> = props
        .iter()
        .map(|p| {
            json!({
                "file": format!("props/{}.glb", p.file_stem),
                "env_slot": p.env_slot,
                "anim_id": p.anim_id,
                "clip_frames": p.frame_count,
                "instances": p.instances.iter().map(|(t, r)| json!({
                    "position": t,
                    "rot_y_radians": r,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "scene": a.name,
        "generator": "legaia-engine export-glb",
        "scale": opts.scale,
        "conventions": {
            "units": "glTF meters = PSX world units * scale; one walk tile = 128 PSX units",
            "axes": "glTF +Y up; geometry keeps the site viewers' mirror-handedness",
            "rotation": "rot_y_radians is about +Y, applied the way the world glb instances are",
        },
        "world_glb": world_file,
        "spawn": { "position": world.spawn },
        "stats": {
            "meshes": world.mesh_count,
            "instances": world.instance_count,
            "sky_draws_hidden": world.sky_hidden,
            "ground_quads": world.ground_quads,
            "placements": a.placements.len(),
            "terrain_tiles": a.terrain.len(),
        },
        "npcs": npc_dir_entries,
        "doors": door_entries,
        "animated_props": prop_entries,
    })
}
