//! Disc-gated: the **object bind** of a placed field object, and the frame-0
//! rest pose it puts a multi-object prop into.
//!
//! `FUN_8003A55C` does not just stamp a record's mesh at a tile. For each
//! placed record it first resolves a **bind** - the kind-1 tile trigger sitting
//! on the record's footprint-anchor tile, and through it a MAN partition-0
//! record - and that bind decides two things:
//!
//! 1. **Whether the object exists.** No bind at the anchor tile -> the tile is
//!    skipped and no actor is ever created. `town01` has six such placements.
//! 2. **How the object is drawn.** The bind record's header
//!    (`[u8 n][n*2 name bytes][u8 anim_id]`) ends in an animation id, which
//!    `FUN_8003A55C` stores at `actor+0x5C`. A nonzero one makes the per-actor
//!    anim tick (`FUN_800204f8`) bind the scene ANM record into `actor+0x4C`
//!    and flip the actor to draw kind `1`, whose walker (`FUN_8001b964`) poses
//!    every TMD object by that clip's per-bone rigid transform - and refuses to
//!    draw at all unless bone count == object count. Zero leaves the actor at
//!    draw kind `5`, which draws the objects raw.
//!
//! Rim Elm's cupboard (object id `230`, env mesh `15`) is the case that makes
//! the difference visible: three TMD objects (cabinet + two doors), the doors
//! authored about their own hinges, so drawn raw they hang inside the cabinet
//! and below the floor. Its bind names anim `2`, whose 3-bone / 30-frame clip
//! is the door swing; **frame 0 is the closed state**.
//!
//! Every fact here is cross-checked against a live Rim Elm capture's actor list
//! (`mei_house_inside`): the bound placements are exactly the actors present,
//! and each actor's `+0x5C` equals the anim id the bind resolves.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` are missing (disc-gated).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_asset::field_objects::FLAG_PLACED;
use legaia_engine_core::field_env;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};

/// Rim Elm's two scene variants (they share one `.MAP`).
const TOWN_SCENES: &[&str] = &["town01", "town0c"];

/// The searchable cupboard: object-record id, its env-pack mesh, and the anim
/// id its bind names - all three read back from a live `town01` actor.
const CUPBOARD_OBJ: u16 = 230;
const CUPBOARD_MESH: u16 = 15;
const CUPBOARD_ANIM: u8 = 2;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn gate() -> Option<Arc<ProtIndex>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing");
        None
    })?;
    Some(Arc::new(
        ProtIndex::open_extracted(&extracted).expect("open prot index"),
    ))
}

fn scene_resources(index: &Arc<ProtIndex>, scene: &Scene) -> SceneResources {
    let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
        .iter()
        .filter_map(|n| Scene::load(index, n).ok())
        .collect();
    let shared_refs: Vec<&Scene> = shared.iter().collect();
    SceneResources::build_targeted_with_options(
        scene,
        &shared_refs,
        BuildOptions {
            kind: SceneLoadKind::Field,
            upload_all_tims: true,
            system_ui: None,
        },
    )
    .expect("scene resources")
    .0
}

/// The bind gate: an unbound placement is not an actor. Retail's `town01` actor
/// list holds one `obj456` (the bound one, anchor `(95, 54)`) and four `obj230`
/// cupboards, not the six and five the raw `.MAP` sweep yields.
#[test]
fn unbound_placements_do_not_spawn() {
    let Some(index) = gate() else { return };
    for name in TOWN_SCENES {
        let scene = Scene::load(&index, name).expect("load scene");
        let res = scene_resources(&index, &scene);
        let env = field_env::env_pack_tmd_indices(&scene, &res);
        let placements = scene
            .field_object_placements(&index)
            .expect("placements")
            .expect("field map");
        let binds = scene
            .field_object_binds(&index)
            .expect("binds")
            .expect("field map + man");
        assert!(!binds.is_empty(), "{name}: no object binds resolved");

        let (bound, dropped) =
            field_env::resolve_placed_env_draws(&env, &placements, None, Some(&binds));
        let unbound: Vec<_> = dropped
            .iter()
            .filter(|d| matches!(d, field_env::EnvDrawDrop::Unbound { .. }))
            .collect();
        assert!(
            !unbound.is_empty(),
            "{name}: the bind gate dropped nothing - the anchor-tile lookup is not resolving"
        );
        // Every surviving draw must have a bind (that is what `bound` means),
        // and the gate must not have eaten the map (a broken lookup would drop
        // everything).
        assert!(
            bound.len() > unbound.len(),
            "{name}: {} bound vs {} unbound - the gate dropped too much",
            bound.len(),
            unbound.len()
        );

        // The cupboard: five `.MAP` cells reference it, but only the four with
        // a bind become actors (matching the live capture).
        let cupboards: Vec<_> = placements
            .iter()
            .filter(|p| p.obj_idx == CUPBOARD_OBJ)
            .collect();
        assert_eq!(cupboards.len(), 5, "{name}: cupboard cell count");
        let spawned = cupboards
            .iter()
            .filter(|p| binds.contains_key(&(p.anchor_col, p.anchor_row)))
            .count();
        assert_eq!(
            spawned, 4,
            "{name}: exactly the four bound cupboards spawn (the live actor list has four)"
        );
    }
}

/// The bind's anim id and the retail count-equality contract: every bound
/// placement that names an animation resolves a scene-ANM record whose bone
/// count equals its mesh's TMD-object count, and every *multi-object* placed
/// mesh names one (an unposed multi-object prop is the bug).
#[test]
fn bound_multi_object_props_resolve_a_matching_pose_clip() {
    let Some(index) = gate() else { return };
    for name in TOWN_SCENES {
        let scene = Scene::load(&index, name).expect("load scene");
        let res = scene_resources(&index, &scene);
        let env = field_env::env_pack_tmd_indices(&scene, &res);
        let placements = scene
            .field_object_placements(&index)
            .expect("placements")
            .expect("field map");
        let binds = scene
            .field_object_binds(&index)
            .expect("binds")
            .expect("field map + man");
        let bundle = scene
            .entries
            .iter()
            .find_map(|e| {
                [3usize, 5, 6, 7]
                    .into_iter()
                    .find_map(|d| legaia_asset::player_anm::find_in_entry(&e.bytes, d).pop())
            })
            .expect("scene ANM bundle");

        let (draws, _) = field_env::resolve_placed_env_draws(&env, &placements, None, Some(&binds));
        let mut posed = 0usize;
        for d in &draws {
            let objects = res.tmds[d.res_tmd].tmd.objects.len();
            if d.anim_id == 0 {
                assert_eq!(
                    objects, 1,
                    "{name}: env mesh {} has {objects} TMD objects but its bind names no \
                     animation - a multi-object prop drawn raw is exactly the cupboard bug",
                    d.env_slot
                );
                continue;
            }
            let rec = bundle
                .record((d.anim_id - 1) as usize)
                .unwrap_or_else(|e| panic!("{name}: anim {} has no record: {e}", d.anim_id));
            assert_eq!(
                rec.bone_count as usize, objects,
                "{name}: anim {} has {} bones but env mesh {} has {objects} objects - \
                 retail's `FUN_8001b964` refuses to draw a mismatched pair",
                d.anim_id, rec.bone_count, d.env_slot
            );
            assert!(rec.frame_count > 1, "{name}: anim {} is a still", d.anim_id);
            posed += 1;
        }
        assert!(posed >= 5, "{name}: only {posed} posed props");
    }
}

/// The cupboard, end to end: its bind names anim `2`; the clip's frame 0 puts
/// both door objects **flush in the cabinet's front opening** (closed), and a
/// later frame swings them out. Unposed - what the engine used to draw - the
/// door objects sit at the cabinet's mid-depth and hang *below the floor*.
#[test]
fn cupboard_frame_zero_is_the_closed_door() {
    let Some(index) = gate() else { return };
    let scene = Scene::load(&index, "town01").expect("load scene");
    let res = scene_resources(&index, &scene);
    let env = field_env::env_pack_tmd_indices(&scene, &res);
    let placements = scene
        .field_object_placements(&index)
        .expect("placements")
        .expect("field map");
    let binds = scene
        .field_object_binds(&index)
        .expect("binds")
        .expect("field map + man");
    let bundle = scene
        .entries
        .iter()
        .find_map(|e| {
            [3usize, 5, 6, 7]
                .into_iter()
                .find_map(|d| legaia_asset::player_anm::find_in_entry(&e.bytes, d).pop())
        })
        .expect("scene ANM bundle");

    let cupboard = placements
        .iter()
        .find(|p| p.obj_idx == CUPBOARD_OBJ && binds.contains_key(&(p.anchor_col, p.anchor_row)))
        .expect("a bound cupboard");
    assert!(cupboard.flags & FLAG_PLACED != 0);
    assert_eq!(cupboard.pack_index, Some(CUPBOARD_MESH));
    let bind = binds[&(cupboard.anchor_col, cupboard.anchor_row)];
    assert_eq!(
        bind.anim_id, CUPBOARD_ANIM,
        "the cupboard's bind names anim {CUPBOARD_ANIM} (live actor +0x5C reads the same)"
    );

    let rt = &res.tmds[env[CUPBOARD_MESH as usize]];
    assert_eq!(rt.tmd.objects.len(), 3, "cabinet + two doors");
    let rec_idx = (bind.anim_id - 1) as usize;
    let rec = bundle.record(rec_idx).expect("cupboard clip");
    assert_eq!(rec.bone_count, 3);
    assert!(rec.frame_count >= 16, "the door swing is a real clip");

    // Object-space AABBs of the three objects, unposed and posed at frame 0.
    let obj_aabb = |oi: usize, offsets: Option<&[([i16; 3], [i16; 3])]>| -> ([f32; 3], [f32; 3]) {
        let mut one = rt.tmd.clone();
        one.objects = vec![rt.tmd.objects[oi].clone()];
        let (vm, cm) = match offsets {
            Some(o) => (
                legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(&one, &rt.raw, &o[oi..oi + 1]),
                legaia_tmd::mesh::tmd_to_color_mesh_posed_rot(&one, &rt.raw, &o[oi..oi + 1]),
            ),
            None => (
                legaia_tmd::mesh::tmd_to_vram_mesh(&one, &rt.raw),
                legaia_tmd::mesh::tmd_to_color_mesh(&one, &rt.raw),
            ),
        };
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for p in vm.positions.iter().chain(cm.positions.iter()) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    };
    let pose = |frame: usize| -> Vec<([i16; 3], [i16; 3])> {
        (0..3)
            .map(|b| match bundle.bone_transform(rec_idx, frame, b) {
                Some(t) => (
                    [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                    [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                ),
                None => ([0; 3], [0; 3]),
            })
            .collect()
    };

    // The cabinet body (object 0, bone 0) is the anchor: its clip transform is
    // identity, so it is the mesh's own frame.
    let f0 = pose(0);
    assert_eq!(
        f0[0],
        ([0, 0, 0], [0, 0, 0]),
        "bone 0 is the static cabinet"
    );
    let (body_lo, body_hi) = obj_aabb(0, None);
    // TMD Y is down: the cabinet stands from its top (`lo`) to the floor at
    // `hi.y == 0`.
    assert!(
        body_hi[1].abs() < 1.0,
        "the cabinet rests on the floor plane (max Y {} ~ 0)",
        body_hi[1]
    );

    for door in 1..3 {
        let (raw_lo, raw_hi) = obj_aabb(door, None);
        let (p0_lo, p0_hi) = obj_aabb(door, Some(&f0));

        // THE BUG: drawn raw, the door hangs *through the floor* (its own
        // hinge-local vertices reach past Y 0) and sits at the cabinet's
        // mid-depth instead of on its front face.
        assert!(
            raw_hi[1] > 8.0,
            "door {door} unposed should sink below the floor (max Y {}) - if it no longer \
             does, the mesh changed and this regression needs re-pinning",
            raw_hi[1]
        );

        // POSED at frame 0: the door is back inside the cabinet's own bounds
        // (allowing the hinge lip a few units of overhang) and no longer below
        // the floor - that is what "closed" looks like.
        assert!(
            p0_hi[1] <= 1.0,
            "door {door} at frame 0 must sit on/above the floor, not through it (max Y {})",
            p0_hi[1]
        );
        for k in 0..3 {
            assert!(
                p0_lo[k] >= body_lo[k] - 8.0 && p0_hi[k] <= body_hi[k] + 8.0,
                "door {door} at frame 0 must lie within the cabinet's bounds on axis {k}: \
                 door [{}, {}] vs body [{}, {}]",
                p0_lo[k],
                p0_hi[k],
                body_lo[k],
                body_hi[k]
            );
        }
        assert!(
            (p0_lo, p0_hi) != (raw_lo, raw_hi),
            "door {door}'s frame-0 transform must actually move it"
        );

        // ...and the clip really is the door OPENING: some later frame swings
        // the door clear of the cabinet's own bounds. (Frame 0 is the only
        // *closed* frame, which is why it is the rest pose.)
        let opens = (1..rec.frame_count as usize).any(|f| {
            let (lo, hi) = obj_aabb(door, Some(&pose(f)));
            (0..3).any(|k| hi[k] > body_hi[k] + 8.0 || lo[k] < body_lo[k] - 8.0)
        });
        assert!(
            opens,
            "door {door}: no frame of anim {} swings it out of the cabinet - \
             the clip is supposed to be the door opening",
            bind.anim_id
        );
    }
}
