//! One animated `.glb` per stream-animated prop of a scene.

use super::*;

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
    /// `true` when retail leaves this prop's clip **free-running** - the bind
    /// record's spawn pass keeps the template's looping playback flags
    /// ([`legaia_engine_scene::field_env::prop_spawn_free_runs`]: the windmill, whose sails
    /// spin forever under the unconditional anim tick) - so a builder should
    /// free-loop it. `false` for a prop whose spawn pass parks the clip held
    /// (a door/cupboard swing, a drawer slide): retail advances those only
    /// while the bind record's script runs, so a builder should trigger the
    /// clip once instead (town01: every untagged looping-door report traced
    /// to a `cyclic == false` clip). When no bind resolves, falls back to the
    /// keyframe test [`anim_is_cyclic`] - which alone is NOT sufficient: the
    /// windmill's 0..179-degree spin ends displaced from frame 0 (its
    /// four-blade symmetry is what makes the loop seamless), so the keyframe
    /// test misreads retail's one always-running prop as a one-shot.
    pub cyclic: bool,
    /// `(translation, rot_y radians, rotation quaternion)` per placed
    /// instance.
    pub instances: PropInstances,
}

/// Does the clip's last keyframe return to its first? **Fallback only** - the
/// primary `cyclic` verdict is the retail bind program (see
/// [`PropGlb::cyclic`]; this shape test calls the windmill's symmetric spin a
/// one-shot). Rotation compared
/// per-axis in retail's `0..4096` angle space with wraparound (a full turn is
/// cyclic), translation in TMD model units. Thresholds: 5 degrees / 4 units -
/// wide enough for keyframe rounding, far under the smallest real swing
/// (town01's shallowest one-shot ends 77 degrees out; its drawer slide ends
/// 27 units out; true loops measure exactly 0).
fn anim_is_cyclic(anim: &legaia_actor_formats::monster_archive::MonsterAnimation) -> bool {
    let (Some(first), Some(last)) = (anim.frames.first(), anim.frames.last()) else {
        return true;
    };
    let wrap = |a: u16, b: u16| -> u32 {
        let d = u32::from(a.wrapping_sub(b) & 0xFFF);
        d.min(4096 - d)
    };
    let tr = |a: i16, b: i16| -> u32 { (i32::from(a) - i32::from(b)).unsigned_abs() };
    first.iter().zip(last.iter()).all(|(f, l)| {
        wrap(f.rx, l.rx) <= 57
            && wrap(f.ry, l.ry) <= 57
            && wrap(f.rz, l.rz) <= 57
            && tr(f.tx, l.tx) <= 4
            && tr(f.ty, l.ty) <= 4
            && tr(f.tz, l.tz) <= 4
    })
}

/// Export the scene's animated placed props (windmill sails, doors, gates):
/// every distinct `(env_slot, anim_id)` placement pair whose clip has more
/// than one frame and matches the mesh's object count. The world glb keeps
/// their frame-0 static twins - a world builder swaps these in where the
/// motion is wanted.
pub fn export_animated_prop_glbs(
    index: &ProtIndex,
    scene: &Scene,
    a: &AssembledScene,
    opts: &GlbExportOptions,
) -> Vec<PropGlb> {
    let Some(bundle) = scene_anm_bundle(scene) else {
        return Vec::new();
    };
    // The retail free-run verdict per placement anchor: does the bind
    // record's spawn pass leave the clip looping (windmill) or parked
    // (door)? See [`PropGlb::cyclic`].
    let man = scene.field_man_payload(index).ok().flatten();
    let man_file = man
        .as_ref()
        .and_then(|m| legaia_scene_formats::man_section::parse(m).ok());
    let binds = scene
        .field_object_binds(index)
        .ok()
        .flatten()
        .unwrap_or_default();
    let free_runs = |anchor: &(u8, u8)| -> Option<bool> {
        let (Some(man), Some(mf)) = (man.as_ref(), man_file.as_ref()) else {
            return None;
        };
        let bind = binds.get(anchor)?;
        Some(legaia_engine_scene::field_env::prop_spawn_free_runs(
            mf,
            man,
            bind.record as usize,
        ))
    };
    let mut by_key: HashMap<(usize, u8), PropInstances> = HashMap::new();
    let mut anchors_by_key: HashMap<(usize, u8), Vec<(u8, u8)>> = HashMap::new();
    let mut key_order: Vec<(usize, u8)> = Vec::new();
    for d in &a.placements {
        if d.anim_id == 0 {
            continue;
        }
        let key = (d.env_slot, d.anim_id);
        anchors_by_key.entry(key).or_default().push(d.anchor);
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
        entry.push((t, -draw_rot_y_radians(d.rot_y), draw_rotation_quat(d)));
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
        // Retail verdict when any instance's bind resolves (all instances of
        // a key share the record shape; conservative AND if they differ);
        // keyframe-shape fallback otherwise.
        let retail: Vec<bool> = anchors_by_key
            .remove(&key)
            .unwrap_or_default()
            .iter()
            .filter_map(free_runs)
            .collect();
        let cyclic = if retail.is_empty() {
            anim_is_cyclic(&anim)
        } else {
            retail.iter().all(|&v| v)
        };
        out.push(PropGlb {
            env_slot,
            anim_id,
            file_stem: stem,
            glb: scale_glb_scene_roots(glb, opts.scale),
            frame_count: rec.frame_count,
            cyclic,
            instances: by_key.remove(&key).unwrap_or_default(),
        });
    }
    out
}
