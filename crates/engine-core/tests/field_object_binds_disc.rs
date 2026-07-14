//! Disc-gated: the **object bind** of a placed field object - which of retail's
//! two placed-object sweeps owns it, and the frame-0 rest pose the bind's clip
//! puts a multi-object prop into.
//!
//! Retail creates a placed record's actor from one of two sweeps, and the
//! anchor tile's `CELL_BIND_OWNED` (`0x400`) bit says which:
//!
//! 1. **`FUN_8003A55C`** (scene init, whole grid) resolves the record's **bind** -
//!    the kind-1 tile trigger on its footprint-anchor tile, and through it a MAN
//!    partition-0 record - and skips the record when there is none.
//! 2. **`FUN_801D7B50`** (field overlay, sub-area window rebuild) does no bind
//!    lookup at all; its only extra gate is that same `0x400` bit
//!    (`801d7ccc: andi v0,v0,0x400` -> skip). So it creates exactly the records
//!    the init sweep left behind - unscripted and unposed.
//!
//! The two sets are complementary on the disc, so **every placed record draws**;
//! reading the bind as a *spawn gate* culls the second sweep's objects, which in
//! Rim Elm means the cavern shell (record `168`, env mesh `72`) disappears and
//! the cave renders as a black hole.
//!
//! What the bind *does* decide is the pose. Its record header
//! (`[u8 n][n*2 name bytes][u8 anim_id]`) ends in an animation id, which
//! `FUN_8003A55C` stores at `actor+0x5C`. A nonzero one makes the per-actor anim
//! tick (`FUN_800204f8`) bind the scene ANM record into `actor+0x4C` and flip the
//! actor to draw kind `1`, whose walker (`FUN_8001b964`) poses every TMD object by
//! that clip's per-bone rigid transform - and refuses to draw at all unless bone
//! count == object count. Zero leaves the actor at draw kind `5`, which draws the
//! objects raw.
//!
//! Rim Elm's cupboard (object id `230`, env mesh `15`) makes the pose visible:
//! three TMD objects (cabinet + two doors), the doors authored about their own
//! hinges, so drawn raw they hang inside the cabinet and below the floor. Its
//! bind names anim `2`, whose 3-bone / 30-frame clip is the door swing; **frame 0
//! is the closed state**.
//!
//! Cross-checked against a live Rim Elm capture's actor list: the *init* sweep's
//! actors are exactly the bound placements (37 of `town01`'s 46), and each
//! actor's `+0x5C` equals the anim id its bind resolves.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` are missing (disc-gated).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_asset::field_objects::{CELL_BIND_OWNED, FLAG_PLACED};
use legaia_engine_core::field_env;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};

/// Rim Elm's two scene variants (they share one `.MAP`).
const TOWN_SCENES: &[&str] = &["town01", "town0c"];

/// Scenes the two-sweep partition is checked over: Rim Elm, a casino town, and
/// the Drake overworld - the bind / `0x400` complementarity is not a town quirk.
const SWEEP_SCENES: &[&str] = &["town01", "town0c", "koin3", "map01"];

/// The searchable cupboard: object-record id, its env-pack mesh, and the anim
/// id its bind names - all three read back from a live `town01` actor.
const CUPBOARD_OBJ: u16 = 230;
const CUPBOARD_MESH: u16 = 15;
const CUPBOARD_ANIM: u8 = 2;

/// Rim Elm's cavern shell: the placed record with no bind, and the env mesh it
/// stamps (a ~3100 x 4000-unit round chamber + entry corridor).
const CAVERN_OBJ: u16 = 168;
const CAVERN_MESH: u16 = 72;

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

/// The `0x400` anchor-tile bit and the object bind are **complementary**: every
/// placed record has exactly one of them, so exactly one of retail's two sweeps
/// creates it and nothing is ever culled.
///
/// This is what makes the bind a *pose* selector and not a spawn gate. It holds
/// over field scenes, town scenes and the overworld alike.
#[test]
fn every_placed_record_belongs_to_exactly_one_sweep() {
    let Some(index) = gate() else { return };
    for name in SWEEP_SCENES {
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

        let (mut init_sweep, mut window_sweep) = (0usize, 0usize);
        for p in &placements {
            let bound = binds.contains_key(&(p.anchor_col, p.anchor_row));
            let bind_owned = p.anchor_cell & CELL_BIND_OWNED != 0;
            assert_eq!(
                bound, bind_owned,
                "{name}: obj{} at ({}, {}): the anchor tile's CELL_BIND_OWNED bit must agree \
                 with the bind - it is what keeps FUN_8003A55C and FUN_801D7B50 disjoint",
                p.obj_idx, p.col, p.row
            );
            if bound {
                init_sweep += 1;
            } else {
                window_sweep += 1;
            }
        }
        assert!(init_sweep > 0, "{name}: no init-sweep placements");
        assert_eq!(
            init_sweep + window_sweep,
            placements.len(),
            "{name}: the two sweeps must tile the placement set"
        );

        // ...and therefore the resolver drops nothing: every placed record is
        // an actor, hence a draw.
        let (draws, dropped) =
            field_env::resolve_placed_env_draws(&env, &placements, None, Some(&binds));
        assert!(
            !dropped
                .iter()
                .any(|d| matches!(d, field_env::EnvDrawDrop::Unbound { .. })),
            "{name}: a placed record was culled - retail's window sweep would have spawned it"
        );
        assert_eq!(
            draws.len(),
            placements.len(),
            "{name}: every placed record must draw"
        );
        // Only the init sweep's actors carry a script/clip; the window sweep
        // does no bind lookup, so its actors have `anim_id == 0`.
        assert!(
            draws.iter().filter(|d| d.anim_id != 0).count() <= init_sweep,
            "{name}: an unbound draw claimed an animation id"
        );
    }
}

/// Rim Elm's cave: the cavern shell is a placed record with **no** bind
/// (`FUN_801D7B50`'s), so treating the bind as a spawn gate deletes the whole
/// interior and the cave renders as a black hole. It is a large mesh - a round
/// chamber with an entry corridor - not a prop, which is what makes its loss so
/// visible.
#[test]
fn rim_elm_cavern_shell_draws() {
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

        let cave = placements
            .iter()
            .find(|p| p.obj_idx == CAVERN_OBJ)
            .unwrap_or_else(|| panic!("{name}: no cavern-shell placement"));
        assert_eq!(cave.pack_index, Some(CAVERN_MESH));
        assert!(
            !binds.contains_key(&(cave.anchor_col, cave.anchor_row)),
            "{name}: the cavern shell is the window sweep's - it has no bind"
        );
        assert_eq!(
            cave.anchor_cell & CELL_BIND_OWNED,
            0,
            "{name}: ...and therefore no CELL_BIND_OWNED bit"
        );

        let (draws, _) = field_env::resolve_placed_env_draws(&env, &placements, None, Some(&binds));
        let drawn = draws
            .iter()
            .find(|d| d.env_slot == CAVERN_MESH as usize)
            .unwrap_or_else(|| panic!("{name}: the cavern shell is not drawn - the cave is empty"));
        assert_eq!(drawn.anim_id, 0, "{name}: the window sweep binds no clip");

        // It really is the cave, not a prop: a chamber tens of tiles across.
        let rt = &res.tmds[env[CAVERN_MESH as usize]];
        let vm = legaia_tmd::mesh::tmd_to_vram_mesh(&rt.tmd, &rt.raw);
        let (lo, hi) = vm.aabb();
        assert!(
            hi[0] - lo[0] > 2000.0 && hi[2] - lo[2] > 2000.0,
            "{name}: cavern shell is only {} x {} units - wrong mesh?",
            hi[0] - lo[0],
            hi[2] - lo[2]
        );
    }
}

/// The cupboard: five `.MAP` cells reference it. Four are bound (the init sweep's,
/// posed by anim `2` - the live capture has exactly four cupboard actors); the
/// fifth is the window sweep's, and it draws too, just unposed.
#[test]
fn cupboard_cells_split_across_the_two_sweeps() {
    let Some(index) = gate() else { return };
    for name in TOWN_SCENES {
        let scene = Scene::load(&index, name).expect("load scene");
        let placements = scene
            .field_object_placements(&index)
            .expect("placements")
            .expect("field map");
        let binds = scene
            .field_object_binds(&index)
            .expect("binds")
            .expect("field map + man");

        let cupboards: Vec<_> = placements
            .iter()
            .filter(|p| p.obj_idx == CUPBOARD_OBJ)
            .collect();
        assert_eq!(cupboards.len(), 5, "{name}: cupboard cell count");
        let posed = cupboards
            .iter()
            .filter_map(|p| binds.get(&(p.anchor_col, p.anchor_row)))
            .filter(|b| b.anim_id == CUPBOARD_ANIM)
            .count();
        assert_eq!(
            posed, 4,
            "{name}: four cupboards are the init sweep's, posed by the door-swing clip"
        );
    }
}

/// The bind's anim id and the retail count-equality contract: every *bound*
/// placement that names an animation resolves a scene-ANM record whose bone
/// count equals its mesh's TMD-object count, and every **bound** multi-object
/// mesh names one (an unposed multi-object *bound* prop is the cupboard bug).
///
/// The window sweep's placements are exempt: `FUN_801D7B50` does no bind lookup,
/// so its actors have no clip and draw their objects raw - which is what retail
/// does with them, multi-object or not.
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
            let bound = binds.contains_key(&d.anchor);
            if d.anim_id == 0 {
                assert!(
                    !bound || objects == 1,
                    "{name}: env mesh {} has {objects} TMD objects but its bind names no \
                     animation - a multi-object BOUND prop drawn raw is exactly the cupboard bug",
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
