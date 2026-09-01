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
use legaia_asset::scene_gltf::{SceneInstance, SceneMesh};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

/// Placement transforms of one animated prop: `(translation, rot_y radians)`
/// per placed instance, translation already in the export frame.
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
    /// Meshes that carry baked VDF morph targets (the ambient vertex-morph
    /// bake - Rim Elm's shoreline). `0` = the scene has no armed pulse.
    pub morph_mesh_count: usize,
    /// Loop length of the baked `vdf_pulse` weights animation, in seconds
    /// (`0.0` when no animation was baked).
    pub morph_loop_seconds: f32,
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
    index: &ProtIndex,
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
            morph_targets: Vec::new(),
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
                    morph_targets: Vec::new(),
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

    // --- Ambient VDF morph bake (Rim Elm's shoreline going in and out). ---
    // Recreate the engine's scene-entry pulse (`crate::vdf_pulse` - the same
    // arming the play hosts run, with its self-guard for retail-armed
    // scenes), turn each lane's full-weight deltas into a glTF morph target
    // on the affected world meshes, and sample the envelope for exactly one
    // loop period as a `weights` animation.
    let weights_anim = bake_vdf_pulse_anim(index, scene, a, &handles, &mut meshes);
    let morph_mesh_count = meshes
        .iter()
        .filter(|m| !m.morph_targets.is_empty())
        .count();
    let morph_loop_seconds = weights_anim
        .as_ref()
        .and_then(|w| w.times.last().copied())
        .unwrap_or(0.0);

    let glb = legaia_asset::scene_gltf::build_scene_glb_animated(
        &a.name,
        &meshes,
        &instances,
        &a.res.vram,
        weights_anim.as_ref(),
    )
    .unwrap_or_default();
    Ok(WorldGlb {
        glb,
        mesh_count: meshes.len(),
        instance_count: instances.len(),
        sky_hidden,
        spawn,
        ground_quads,
        morph_mesh_count,
        morph_loop_seconds,
    })
}

/// The ambient game-tick rate the pulse envelope advances at (the retail
/// 30 Hz field tick) - the time base of the baked weights animation.
const PULSE_TICK_HZ: f32 = 30.0;

/// Recreate the scene-entry VDF pulse and bake it: morph targets onto the
/// world meshes of every targeted env slot (one target per envelope lane, the
/// lane's deltas applied at full weight), plus the sampled lane weights over
/// exactly one envelope period. `None` when the scene arms no pulse (no VDF
/// pack, retail owns the morphs, or nothing fits).
fn bake_vdf_pulse_anim(
    index: &ProtIndex,
    scene: &Scene,
    a: &AssembledScene,
    handles: &HashMap<(usize, u8), Option<usize>>,
    meshes: &mut [SceneMesh],
) -> Option<legaia_asset::scene_gltf::SceneWeightsAnim> {
    use legaia_asset::scene_gltf::{SceneMorphTarget, SceneWeightsAnim};

    // The same setup `SceneHost::enter_field_scene` runs before arming.
    let mut w = crate::world::World {
        frame_step: 2,
        ..Default::default()
    };
    if let Some(scripts) = scene.find_event_scripts() {
        w.install_field_stagers(scripts.bytes);
    }
    w.set_vdf_buffer(crate::scene_bundle::find_vdf_buffer(scene));
    if let Ok(Some(man)) = scene.field_man_payload(index)
        && let Ok(mf) = legaia_asset::man_section::parse(&man)
    {
        for arg in crate::man_field_scripts::scene_entry_ambient_installs(&mf, &man) {
            w.spawn_ambient_record(arg as usize + 1, [0, 0, 0]);
        }
    }
    let pack_objects: Vec<Vec<usize>> = a
        .env_tmds
        .iter()
        .map(|&ti| {
            a.res.tmds[ti]
                .tmd
                .objects
                .iter()
                .map(|o| o.vertices.len())
                .collect()
        })
        .collect();
    if !w.install_entry_vdf_pulse(&pack_objects) {
        return None;
    }

    // Per targeted env slot: the lanes that touch it (each lane = one morph
    // target) and the groups each lane moves.
    let all_targets = w.entry_vdf_pulse.as_ref()?.all_targets();
    let mut slot_lanes: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    let mut lane_groups: BTreeMap<(usize, u8), Vec<u32>> = BTreeMap::new();
    for &(slot, group) in &all_targets {
        for (lane, _) in w.entry_vdf_pulse.as_ref()?.lanes_for(slot, group) {
            let lanes = slot_lanes.entry(slot).or_default();
            if !lanes.contains(&lane) {
                lanes.push(lane);
            }
            let groups = lane_groups.entry((slot, lane)).or_default();
            if !groups.contains(&group) {
                groups.push(group);
            }
        }
    }
    for lanes in slot_lanes.values_mut() {
        lanes.sort_unstable();
    }

    // Build the morph targets: rebuild each slot's mesh with one lane's
    // deltas at full weight and diff the position stream (the rebuild keeps
    // vertex order - `with_group_deltas` only moves positions).
    let mut tracked_meshes: Vec<(usize, Vec<u8>)> = Vec::new();
    for (&slot, lanes) in &slot_lanes {
        // Only the static (anim 0) world mesh morphs; a slot that never
        // reached the world glb (unplaced twin, sky) has no mesh to morph.
        let Some(&Some(mi)) = handles.get(&(slot, 0)) else {
            continue;
        };
        if mi == usize::MAX {
            continue;
        }
        let Some(&res_tmd) = a.env_tmds.get(slot) else {
            continue;
        };
        let rtmd = &a.res.tmds[res_tmd];
        let mut targets: Vec<SceneMorphTarget> = Vec::new();
        for &lane in lanes {
            let mut morphed = rtmd.clone();
            for &group in lane_groups.get(&(slot, lane)).into_iter().flatten() {
                let n_verts = morphed
                    .tmd
                    .objects
                    .get(group as usize)
                    .map_or(0, |o| o.vertices.len());
                let deltas = w.morph_deltas_for(&[(lane, 0x1000)], group, n_verts);
                morphed = morphed.with_group_deltas(group, &deltas);
            }
            let (mesh, _) = build_hybrid_env_mesh(&morphed, &a.res.vram);
            let base = &meshes[mi].positions;
            if mesh.positions.len() * 3 != base.len() {
                continue; // rebuild changed shape - refuse a desynced target
            }
            let mut deltas_flat = Vec::with_capacity(base.len());
            for (vi, p) in mesh.positions.iter().enumerate() {
                deltas_flat.push(p[0] - base[vi * 3]);
                deltas_flat.push(p[1] - base[vi * 3 + 1]);
                deltas_flat.push(p[2] - base[vi * 3 + 2]);
            }
            targets.push(SceneMorphTarget {
                name: format!("vdf_lane_{lane}"),
                deltas: deltas_flat,
            });
        }
        if !targets.is_empty() {
            meshes[mi].morph_targets = targets;
            tracked_meshes.push((mi, lanes.clone()));
        }
    }
    if tracked_meshes.is_empty() {
        return None;
    }

    // Sample the envelope for one full period (fingerprint recurrence),
    // closing the loop with a final key equal to the first.
    const MAX_TICKS: usize = 3600;
    let sample_lanes = |w: &crate::world::World, lanes: &[u8]| -> Vec<f32> {
        let (weights, _, _) = w
            .entry_vdf_pulse
            .as_ref()
            .map(|p| p.phase_fingerprint())
            .unwrap_or_default();
        lanes
            .iter()
            .map(|&l| {
                weights
                    .get(l as usize)
                    .map_or(0.0, |&v| f32::from(v.min(0x1000)) / 4096.0)
            })
            .collect()
    };
    let start = w.entry_vdf_pulse.as_ref()?.phase_fingerprint();
    let mut times: Vec<f32> = Vec::new();
    let mut tracks: BTreeMap<usize, Vec<f32>> = BTreeMap::new();
    for tick in 0..=MAX_TICKS {
        times.push(tick as f32 / PULSE_TICK_HZ);
        for (mi, lanes) in &tracked_meshes {
            tracks
                .entry(*mi)
                .or_default()
                .extend(sample_lanes(&w, lanes));
        }
        w.tick_ambient_fx();
        if w.entry_vdf_pulse.as_ref()?.phase_fingerprint() == start {
            // The state has returned to key 0's - one more key with those
            // weights closes the loop seamlessly under LINEAR interpolation.
            times.push((tick + 1) as f32 / PULSE_TICK_HZ);
            for (mi, lanes) in &tracked_meshes {
                tracks
                    .entry(*mi)
                    .or_default()
                    .extend(sample_lanes(&w, lanes));
            }
            break;
        }
    }
    Some(SceneWeightsAnim {
        name: "vdf_pulse".to_string(),
        times,
        tracks,
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
        // The manifest contract is "rot_y_radians is applied the way the
        // world glb instances are": the glb node negates the page param
        // (see `scene_gltf::quat_y`), so the manifest carries the negation
        // too - which lands on the authored retail yaw as a plain positive
        // rotation about +Y. Keeps builder-placed props aligned with their
        // baked frame-0 twins in any importer.
        entry.push((t, -draw_rot_y_radians(d.rot_y)));
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

/// Traversal data of one scene, ready for the manifest: the intra-scene
/// teleports a walk-in world needs to make doorways *work*, plus the
/// scene-change portal sites a multi-scene build can wire up.
#[derive(Default)]
pub struct TraversalExport {
    /// Intra-scene teleports (both retail families - see
    /// [`export_scene_traversal`]), as manifest-ready JSON entries.
    pub teleports: Vec<Value>,
    /// Gate-1 walk-on portal sites whose record runs a `0x3F` named scene
    /// change (a town exit / overworld entrance) - useful when several
    /// exported scenes share one world.
    pub portals: Vec<Value>,
}

/// The facing direction of engine heading `h` (12-bit, `0` faces +Z) as a
/// unit XZ vector - unchanged by the export frame's Y re-sign
/// ([`draw_translation`] touches only Y), so it is valid in both.
fn heading_dir_xz(h: i16) -> [f32; 2] {
    let th = (h as f32) / 4096.0 * std::f32::consts::TAU;
    [th.sin(), th.cos()]
}

/// Recover both retail doorway families of a scene as manifest entries (the
/// same disc structures the play hosts dispatch - see
/// `docs/subsystems/field-locomotion.md` § intra-scene doorways):
///
/// - **map doors** (`.MAP` kind-0 intra-scene-teleport table): a plain tile
///   carries a destination; crossing onto it repositions the player. This is
///   where most house *exits* live.
/// - **script doors** (`.MAP` object gate-0 kind-1 binds): touching the
///   object's contact box runs its MAN record, whose taken arm teleports the
///   player channel. The arm is resolved here against the **cold-entry**
///   story-flag state (all clear - the state a fresh walk-in world models);
///   records whose cold arm spawns a cutscene instead of teleporting are
///   skipped.
///
/// Positions are in the export frame (scaled, [`draw_translation`]-signed),
/// with destination heights re-sampled off the floor model exactly as the
/// engine re-seats an arrival. `facing_dir` is the arrival facing as a unit
/// XZ direction (`null` = keep the walked-in facing).
///
/// **Trigger boxes are built for a physical player capsule, not retail's
/// point-player.** Retail dispatches both families on a 2D distance check
/// with no height test, so a literal transcription fails two ways a 3D
/// engine's capsule exposes: a contact tile can sample a different floor
/// *layer* than the approach ground walks on (Rim Elm's cave mouth: contact
/// floor 4 m below the village ground crossing above it), and a recessed
/// door's contact centre can sit deeper into an alcove than a capsule can
/// follow (Vahn's door: the walkable channel is narrower than a player
/// capsule; retail's point slides down it). Hence each trigger's vertical
/// span is the min..max of the floor sampled on rings around the contact
/// (padded a step down / a body height up), and script-door horizontal
/// half-extents carry a capsule allowance over retail's `0x50`.
pub fn export_scene_traversal(
    index: &ProtIndex,
    scene: &Scene,
    floor: &FloorSampler,
    opts: &GlbExportOptions,
) -> TraversalExport {
    let s = opts.scale;
    let pt = |x: f32, y: f32, z: f32| -> [f32; 3] {
        let t = draw_translation([x, y, z]);
        [t[0] * s, t[1] * s, t[2] * s]
    };
    let floor_pt = |wx: i16, wz: i16| -> [f32; 3] {
        let y = floor.height(wx as i32, wz as i32);
        pt(wx as f32, y as f32, wz as f32)
    };
    // Trigger vertical envelope: sample the floor at the contact and on
    // 8-direction rings out to two tiles, and return `(position, half_y)`
    // such that the box spans `lowest - 0x20 .. highest + 0x90` PSX units
    // (half a step below, a body height above) - so the box overlaps the
    // player wherever the *approach* ground actually is, exactly as
    // retail's height-blind 2D check behaves. Retail frame: +Y is DOWN,
    // so "lowest surface" is the numeric max.
    let floor_box = |wx: i16, wz: i16| -> ([f32; 3], f32) {
        let (cx, cz) = (wx as i32, wz as i32);
        let mut lo = floor.height(cx, cz); // numeric min = highest surface
        let mut hi = lo;
        for r in [64, 128] {
            for (dx, dz) in [
                (0, -1),
                (1, -1),
                (1, 0),
                (1, 1),
                (0, 1),
                (-1, 1),
                (-1, 0),
                (-1, -1),
            ] {
                let h = floor.height(cx + dx * r, cz + dz * r);
                lo = lo.min(h);
                hi = hi.max(h);
            }
        }
        let bottom = hi + 0x20; // retail +Y down: below the lowest floor
        let top = lo - 0x90; // above the highest floor
        let half_y = (bottom - top) as f32 / 2.0 * s;
        (pt(wx as f32, bottom as f32, wz as f32), half_y)
    };
    let mut out = TraversalExport::default();

    // --- Map doors (kind-0 intra-scene teleports). ---
    if let Ok((primary, fallback)) = scene.field_intra_scene_teleports(index) {
        let mut seen: Vec<(u8, u8)> = Vec::new();
        for t in primary.iter().chain(fallback.iter()) {
            if seen.contains(&(t.tile_x, t.tile_z)) {
                continue; // primary block wins, as in the trigger dispatch
            }
            seen.push((t.tile_x, t.tile_z));
            let (tx, tz) = (
                i16::from(t.tile_x) * 128 + 0x40,
                i16::from(t.tile_z) * 128 + 0x40,
            );
            let (dx, dz) = t.dest_world();
            let (tpos, thalf_y) = floor_box(tx, tz);
            out.teleports.push(json!({
                "kind": "map",
                "trigger": {
                    "position": tpos,
                    // One collision tile (the kind-0 dispatch is an
                    // exact tile compare); vertical span from the local
                    // floor envelope (see `floor_box`).
                    "half_extents": [64.0 * s, thalf_y, 64.0 * s],
                },
                "destination": { "position": floor_pt(dx, dz) },
                "facing_dir": Value::Null,
                "record": Value::Null,
            }));
        }
    }

    // --- Script doors (object walk-touch binds). ---
    let map_bytes = scene
        .field_map_index(index)
        .and_then(|i| index.entry_bytes_extended(i).ok());
    let triggers = scene.field_tile_triggers(index).ok().map(|(p, f)| {
        let mut t = p;
        t.extend(f);
        t
    });
    let man = scene.field_man_payload(index).ok().flatten();
    if let (Some(map), Some(triggers), Some(man)) = (map_bytes, triggers, man.as_ref())
        && let Ok(mf) = legaia_asset::man_section::parse(man)
    {
        for bind in crate::man_field_scripts::object_walk_touch_binds(&map, &triggers, &mf, man) {
            // Re-resolve the arm the record takes with every story flag
            // clear - the cold-entry state a fresh world models. A record
            // whose cold arm is a cutscene spawn (or a minigame warp) is
            // not a plain doorway; leave it out rather than guess.
            let cold =
                crate::man_field_scripts::resolve_walk_touch_event(&mf, man, bind.record, &|_| {
                    false
                });
            let Some(crate::man_field_scripts::WalkTouchEvent::PlayerMoveTo {
                world_x,
                world_z,
                facing,
            }) = cold
            else {
                continue;
            };
            let (cx, cz) = bind.contact;
            // Retail's `0x50` contact half-extent measured a POINT player;
            // a capsule stopped at the mouth of a recessed doorway (Vahn's
            // door: the walkable channel is narrower than a capsule) needs
            // the face to come out and meet it - widen by half a tile.
            let half = (crate::field_regions::MAP_OBJECT_CONTACT_HALF + 32) as f32;
            let (tpos, thalf_y) = floor_box(cx, cz);
            out.teleports.push(json!({
                "kind": "script",
                "trigger": {
                    "position": tpos,
                    "half_extents": [half * s, thalf_y, half * s],
                },
                "destination": { "position": floor_pt(world_x, world_z) },
                "facing_dir": facing.map(heading_dir_xz),
                "record": bind.record,
            }));
        }

        // --- Scene-change portal sites (gate-1 walk-on -> 0x3F). ---
        let half = |b: u8| -> f32 {
            f32::from(b & 0x7F) * 128.0 + if b & 0x80 != 0 { 128.0 } else { 64.0 }
        };
        for site in crate::man_field_scripts::overworld_portal_sites(&mf, man, &triggers) {
            let (tx, tz) = (
                i16::from(site.overworld_x) * 128 + 0x40,
                i16::from(site.overworld_z) * 128 + 0x40,
            );
            // Arrival facing: the 0x3F trailing dir byte through the retail
            // compass table (`dir & 7` eighths of the 12-bit circle).
            let facing = heading_dir_xz(i16::from(site.dir & 7) * 0x200);
            let (tpos, thalf_y) = floor_box(tx, tz);
            out.portals.push(json!({
                "target_scene": site.scene_name,
                "trigger": {
                    "position": tpos,
                    "half_extents": [64.0 * s, thalf_y, 64.0 * s],
                },
                // Arrival point in the TARGET scene's export frame (XZ only,
                // scaled; sample the destination scene's floor for Y when
                // wiring a multi-scene world).
                "entry_xz": [half(site.entry_x) * s, half(site.entry_z) * s],
                "facing_dir": facing,
                "record": site.record,
                "conditional": site.conditional.as_ref().map(|c| json!({
                    "flag": c.flag,
                    "target_scene": c.scene_name,
                    "entry_xz": [half(c.entry_x) * s, half(c.entry_z) * s],
                })),
            }));
        }
    }
    out
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
    traversal: &TraversalExport,
) -> Value {
    // A prop instance standing on a doorway-teleport trigger is a door mesh
    // (its clip is the swing retail's door record plays); one standing on a
    // scene-change portal band is a gate leaf. A builder swaps either's
    // free-running loop for open-on-approach. Trigger proximity is the
    // structural join: retail parks the door placement on its own doorway
    // tile (town01 doors sit within ~136 PSX units of their trigger; the
    // nearest non-door animated prop is ~530 away), and it covers both
    // teleport families where a script-tile anchor join structurally cannot
    // (map doors never had trigger records).
    let near_teleport = |t: &[f32; 3]| -> bool {
        let thresh = 256.0 * opts.scale; // two collision tiles
        traversal.teleports.iter().any(|tp| {
            let Some(p) = tp["trigger"]["position"].as_array() else {
                return false;
            };
            let (Some(px), Some(pz)) = (p[0].as_f64(), p[2].as_f64()) else {
                return false;
            };
            let (dx, dz) = (px as f32 - t[0], pz as f32 - t[2]);
            dx * dx + dz * dz < thresh * thresh
        })
    };
    let near_portal = |t: &[f32; 3]| -> Option<usize> {
        let thresh = 384.0 * opts.scale; // 3 collision tiles
        let mut best: Option<(f32, usize)> = None;
        for (i, tp) in traversal.portals.iter().enumerate() {
            let p = tp["trigger"]["position"].as_array()?;
            let (dx, dz) = (p[0].as_f64()? as f32 - t[0], p[2].as_f64()? as f32 - t[2]);
            let d = dx * dx + dz * dz;
            if d < thresh * thresh && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    };
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
                    // Standing on a doorway-teleport trigger says this
                    // placement's clip is a door record's swing - open it
                    // on approach, don't loop.
                    "is_door": near_teleport(t),
                    // Index into `scene_portals` when this instance stands
                    // on an exit band (a town-gate leaf).
                    "near_portal": near_portal(t),
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
            "morph_meshes": world.morph_mesh_count,
        },
        // The world glb's baked ambient vertex-morph clip (Rim Elm's
        // shoreline): play it looping on the world instance.
        "world_anim": (world.morph_loop_seconds > 0.0).then(|| json!({
            "clip": "vdf_pulse",
            "loop_seconds": world.morph_loop_seconds,
        })),
        "npcs": npc_dir_entries,
        "doors": door_entries,
        "animated_props": prop_entries,
        "teleports": traversal.teleports.clone(),
        "scene_portals": traversal.portals.clone(),
    })
}

// ---------------------------------------------------------------------------
// Equipment item export (`export-glb --items`)
// ---------------------------------------------------------------------------

/// One exported equipment item: the two `.glb` flavours the characters page
/// offers per record (the item **alone** with its grip repaired, and the
/// record-keeping palette cut **with its host limb**), plus the manifest
/// facts a consumer needs to trust the cut.
pub struct ItemGlb {
    /// Character label (`Vahn` / `Noa` / `Gala` / `Terra`).
    pub character: &'static str,
    pub cslot: usize,
    /// Player-file section index (order differs per character).
    pub section: usize,
    /// Human section label derived from the SCUS equipment stat table.
    pub section_label: String,
    /// Equipment id in the player file's descriptor table.
    pub id: u32,
    /// SCUS item-table display name, when the executable was readable.
    pub name: Option<String>,
    /// Suggested relative file stem (`vahn/s2_005_fire-blade`).
    pub file_stem: String,
    /// The item-alone glb (empty when the cut kept nothing).
    pub glb_alone: Vec<u8>,
    /// The item + host-limb glb (empty when the section contributed none).
    pub glb_with_limb: Vec<u8>,
    /// How the palette cut classed the record (`own-object` / `separate` /
    /// `welded` / `fused`).
    pub class: String,
    /// `false` = the grip is open on the record itself (the shaft inside the
    /// closed fist was never modelled).
    pub complete: bool,
    /// How the item-alone cut decided (`colour-diff` / `identity` / `whole` /
    /// `palette`).
    pub mode: String,
    /// Whether a committed `equip-isolation.toml` rule touched the record.
    pub curated: bool,
    /// Grip-repair bridges the alone mesh carries (0 = none needed/found).
    pub bridges: usize,
    /// Clips baked into both files (action bank + weapon swings).
    pub clip_count: usize,
}

/// Every equippable record of every player battle file, exported.
pub struct ItemsExport {
    pub items: Vec<ItemGlb>,
    /// Tolerated decode degradations, labelled by character/loadout.
    pub notes: Vec<String>,
}

fn item_file_stem(character: &str, section: usize, id: u32, name: Option<&str>) -> String {
    let slug: String = name
        .unwrap_or("item")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!(
        "{}/s{}_{:03}_{}",
        character.to_lowercase(),
        section,
        id,
        slug
    )
}

/// Assemble each character wearing exactly one item per equippable record
/// and bake both per-item `.glb` flavours through the shared loadout kernel
/// (`legaia_asset::battle_char_assembly::loadout` - the same code the
/// browser characters page runs). `scus` (the `SCUS_942.54` bytes) supplies
/// item display names and section labels; without it both fall back to ids.
///
/// Arts clips are not baked here - an item file carries the action bank and
/// the equipment-spliced weapon swings, which is what moves a held piece.
pub fn export_equipment_item_glbs(
    index: &ProtIndex,
    scus: Option<&[u8]>,
) -> Result<ItemsExport, String> {
    use legaia_asset::battle_char_assembly as bca;
    use legaia_asset::battle_char_assembly::loadout;

    let names = scus.and_then(legaia_asset::item_names::ItemNameTable::from_scus);
    let stats = scus.and_then(legaia_asset::equip_stats::EquipStatTable::from_scus);
    let mut items = Vec::new();
    let mut notes = Vec::new();
    for (cslot, character) in loadout::CHARACTER_LABELS.iter().enumerate() {
        let prot_index = loadout::PLAYER_FILE_BASE + cslot as u32;
        let raw = index
            .entry_bytes(prot_index)
            .map_err(|e| format!("player file (PROT {prot_index}): {e}"))?;
        let pack = legaia_asset::battle_data_pack::parse(&raw)
            .map_err(|e| format!("player file {prot_index}: {e}"))?;
        let cats = loadout::section_catalog(&pack);
        let labels = loadout::section_labels(&cats, stats.as_ref(), cslot);
        for (section, cat) in cats.iter().enumerate() {
            for &id in &cat.ids {
                let Ok(id_u8) = u8::try_from(id) else {
                    notes.push(format!("{character} s{section} id {id}: out of u8 range"));
                    continue;
                };
                let mut equipped = [0u8; bca::SECTION_COUNT];
                equipped[section] = id_u8;
                let c = match loadout::build(&raw, cslot, equipped, false, &[]) {
                    Ok(c) => c,
                    Err(why) => {
                        notes.push(format!("{character} s{section} id {id}: {why}"));
                        continue;
                    }
                };
                notes.extend(c.notes.iter().map(|n| format!("{character}: {n}")));
                let Some(it) = c.items.iter().find(|i| i.section == section) else {
                    notes.push(format!(
                        "{character} s{section} id {id}: section contributed no geometry"
                    ));
                    continue;
                };
                let name = u8::try_from(it.id)
                    .ok()
                    .and_then(|i| names.as_ref()?.name(i))
                    .map(str::to_string);
                items.push(ItemGlb {
                    character,
                    cslot,
                    section,
                    section_label: labels[section].clone(),
                    id: it.id,
                    file_stem: item_file_stem(character, section, it.id, name.as_deref()),
                    name,
                    glb_alone: loadout::item_only_glb(&c, section, names.as_ref()),
                    glb_with_limb: loadout::item_glb(&c, section, names.as_ref()),
                    class: it.partition.class.tag().to_string(),
                    complete: it.partition.class.is_complete(),
                    mode: it.isolation.mode.tag().to_string(),
                    curated: it.isolation.curated,
                    bridges: it.alone.bridges.len(),
                    clip_count: c.clips.len(),
                });
            }
        }
    }
    Ok(ItemsExport { items, notes })
}

/// The `items/manifest.json` document for one [`ItemsExport`].
pub fn items_manifest(e: &ItemsExport) -> Value {
    json!({
        "generator": "legaia-engine export-glb --items",
        "source": "player battle files data\\battle\\PLAYER1..4 (extraction PROT 863..866)",
        "conventions": {
            "units": "raw PSX model units (same as the site's character downloads) - scale instances by your world export's `scale`",
            "clips": "each glb carries the character's battle action bank + the weapon's spliced direction swings; clip 0 frame 0 is the rest pose",
            "alone_vs_with_limb": "the `_alone` file is the opinionated item-alone cut (grip repaired); `_with_limb` is the exact palette cut with its ground-truth host limb",
        },
        "items": e.items.iter().map(|i| json!({
            "character": i.character,
            "slot": i.cslot,
            "section": i.section,
            "section_label": i.section_label,
            "id": i.id,
            "name": i.name,
            "alone": (!i.glb_alone.is_empty()).then(|| format!("{}_alone.glb", i.file_stem)),
            "with_limb": (!i.glb_with_limb.is_empty()).then(|| format!("{}_with_limb.glb", i.file_stem)),
            "class": i.class,
            "complete": i.complete,
            "isolation_mode": i.mode,
            "curated": i.curated,
            "grip_bridges": i.bridges,
            "clips": i.clip_count,
        })).collect::<Vec<_>>(),
    })
}
