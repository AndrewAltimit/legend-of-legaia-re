//! The world placement manifest written beside a scene's `.glb` files.

use super::*;

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
                // Seamless loop vs one-shot swing (see [`PropGlb::cyclic`]):
                // a builder free-loops cyclic clips and plays the rest once
                // on approach, even when no teleport/portal tags the
                // instance as a door (interior doors, cupboards, drawers).
                "cyclic": p.cyclic,
                "instances": p.instances.iter().map(|(t, r, q)| json!({
                    "position": t,
                    "rot_y_radians": r,
                    // The placement's full rotation (all three authored
                    // angles), the world glb twin node's own quaternion.
                    // `rot_y_radians` is its yaw alone - exact only when
                    // the record carries no pitch or roll.
                    "rotation_xyzw": q,
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
            "rotation": "rotation_xyzw is the instance's full glTF rotation, the world glb instance node's own; rot_y_radians is its yaw alone (exact only for records with no pitch or roll), about +Y",
            // NPC / prop glbs carry `scale` baked on their root node, so a
            // file dragged into a scene is already world-sized; a builder
            // placing instances applies only the handedness mirror (a
            // manifest without this flag is an older raw-PSX-units export
            // whose instances need the full scale).
            "npc_prop_units": "scaled",
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
