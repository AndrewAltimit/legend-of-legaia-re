//! The scene's world `.glb`: static environment, ground, and the baked VDF pulse animation.

use super::*;

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
    /// Ground heightfield quads with at least one triangle exported (0 =
    /// scene had no walk floor grid).
    pub ground_quads: usize,
    /// Meshes that carry baked VDF morph targets (the ambient vertex-morph
    /// bake - Rim Elm's shoreline). `0` = the scene has no armed pulse.
    pub morph_mesh_count: usize,
    /// Loop length of the baked `vdf_pulse` weights animation, in seconds
    /// (`0.0` when no animation was baked).
    pub morph_loop_seconds: f32,
}

/// A walk-ground cell's horizontal run (one 128-unit tile), the steepness
/// bound past which a far-bucket ground triangle is a sheet, not a ramp.
const GROUND_CELL_RUN: f32 = 128.0;

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
        // A glb has no ordering table to sink a sloped far-bucket cell under
        // the cliff in front of it (`field_ground::flat_refs`). A gentle
        // ramp still reads right depth-tested, but a face rising more than a
        // cell's run stands as a stray sheet of ground texture retail never
        // shows (town01's cell (30, 38)). Judged per TRIANGLE: such a cell
        // is often one flat half and one vertical half, and dropping the
        // whole quad holes the floor where the flat half was.
        let refs = legaia_engine_scene::field_ground::flat_refs(hf, &hf.positions);
        let sheet = |far: bool, t: &[u32]| {
            let ys = t.iter().map(|&i| hf.positions[i as usize][1]);
            far && ys.clone().fold(f32::MIN, f32::max) - ys.fold(f32::MAX, f32::min)
                > GROUND_CELL_RUN
        };
        let mut indices: Vec<u32> = Vec::with_capacity(hf.indices.len());
        for q in hf.indices.as_chunks::<6>().0 {
            let far = refs
                .get(q[0] as usize)
                .is_some_and(legaia_engine_scene::field_ground::is_far_bucket_ref);
            let before = indices.len();
            for t in q.as_chunks::<3>().0.iter().filter(|t| !sheet(far, *t)) {
                indices.extend_from_slice(t);
            }
            ground_quads += usize::from(indices.len() > before);
        }
        // Ground sinks below the env pack's authored floor art (see
        // `coplanar_draws::GROUND_SINK`), same as the page's ground stream.
        let mut positions = Vec::with_capacity(hf.positions.len() * 3);
        for p in &hf.positions {
            positions.push(p[0]);
            positions.push(p[1] + legaia_engine_scene::coplanar_draws::GROUND_SINK);
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
            indices,
            flat_rgba: Vec::new(),
            morph_targets: Vec::new(),
        });
        instances.push(SceneInstance {
            mesh: 0,
            translation: [0.0; 3],
            rot_y: 0.0,
            rotation: None,
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
                // All three authored angles, not the yaw alone: the
                // placement kernel the play hosts draw with.
                rotation: Some(draw_rotation_quat(d)),
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
    // Recreate the engine's scene-entry pulse (`legaia_engine_field::vdf_pulse` - the same
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

    let glb = legaia_actor_formats::scene_gltf::build_scene_glb_animated(
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
) -> Option<legaia_actor_formats::scene_gltf::SceneWeightsAnim> {
    use legaia_actor_formats::scene_gltf::{SceneMorphTarget, SceneWeightsAnim};

    // The same setup `SceneHost::enter_field_scene` runs before arming.
    let mut w = legaia_engine_core::world::World {
        clock: legaia_engine_vm::frame_clock::FrameClock {
            frame_step: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    if let Some(scripts) = scene.find_event_scripts() {
        w.install_field_stagers(scripts.bytes);
    }
    w.set_vdf_buffer(legaia_engine_scene::scene_bundle::find_vdf_buffer(scene));
    if let Ok(Some(man)) = scene.field_man_payload(index)
        && let Ok(mf) = legaia_scene_formats::man_section::parse(&man)
    {
        for arg in legaia_engine_field::man_field_scripts::scene_entry_ambient_installs(&mf, &man) {
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
    if !w.install_entry_vdf_pulse(&scene.name, &pack_objects) {
        return None;
    }

    // Per targeted env slot: the lanes that touch it (each lane = one morph
    // target) and the groups each lane moves.
    let all_targets = w.ambient.entry_vdf_pulse.as_ref()?.all_targets();
    let mut slot_lanes: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    let mut lane_groups: BTreeMap<(usize, u8), Vec<u32>> = BTreeMap::new();
    for &(slot, group) in &all_targets {
        for (lane, _) in w.ambient.entry_vdf_pulse.as_ref()?.lanes_for(slot, group) {
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
    let sample_lanes = |w: &legaia_engine_core::world::World, lanes: &[u8]| -> Vec<f32> {
        let (weights, _, _) = w
            .ambient
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
    let start = w.ambient.entry_vdf_pulse.as_ref()?.phase_fingerprint();
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
        if w.ambient.entry_vdf_pulse.as_ref()?.phase_fingerprint() == start {
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
