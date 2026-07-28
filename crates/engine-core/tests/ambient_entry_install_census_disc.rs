//! Disc-gated: the scene-entry ambient-install census
//! (`man_field_scripts::scene_entry_ambient_installs`, PORT of retail's
//! placement spawn-prologue pre-run `FUN_8003A1E4`).
//!
//! Two halves, and the second is the one that matters:
//!
//!  - **town0e spawns its VDF morph tree.** Its installer is not a dedicated
//!    effect-actor script - it is the second instruction of a fully scripted,
//!    dialogue-bearing placement - so the older shape-filtered census
//!    (`ambient_effect_installs`) reported the scene as having no ambient
//!    install at all and the morph tree never spawned in the engine, even
//!    though the disc installs it.
//!  - **Nothing that used to be found is lost.** The prologue-slice census is
//!    a strict widening: every scene the shape filter found carriers for gets
//!    the identical list back. That is the assertion that catches an
//!    over-widened rule, since a rule loose enough to admit records the VM
//!    never reaches would also start disagreeing with the scenes whose
//!    carriers are already known-correct.
//!
//! Skip-pass when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;

use legaia_engine_core::field_env;
use legaia_engine_core::man_field_scripts::{
    ambient_effect_installs, scene_entry_ambient_installs,
};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::World;

fn extracted_root() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// A scene's decoded MAN plus its prescript stager bundle, or `None` when the
/// scene carries neither.
fn scene_parts(index: &ProtIndex, name: &str) -> Option<(Scene, Vec<u8>, Vec<u8>)> {
    let scene = Scene::load(index, name).ok()?;
    let stager_bytes = scene.find_event_scripts()?.bytes.to_vec();
    let man_bytes = scene.field_man_payload(index).ok()??;
    Some((scene, stager_bytes, man_bytes))
}

/// town0e's Sol village hut-fire morph tree - the wiring gap this census
/// closes. The install is `34 30 00` sitting as the **second** instruction of
/// partition-1 placement 29, ahead of the placement's own `SysFlag.Test` park
/// gate, so retail's spawn-prologue slice runs it whichever way the flag
/// reads.
#[test]
fn town0e_morph_tree_spawns_from_a_placed_actors_prologue_or_skip() {
    let Some(root) = extracted_root() else { return };
    let index = ProtIndex::open_extracted(&root).expect("prot index");
    let Some((scene, stager_bytes, man_bytes)) = scene_parts(&index, "town0e") else {
        panic!("town0e carries a MAN + prescript bundle");
    };
    let man_file = legaia_asset::man_section::parse(&man_bytes).expect("parse MAN");

    // The gap, stated as a contrast: the shape filter sees nothing here.
    assert!(
        ambient_effect_installs(&man_file, &man_bytes).is_empty(),
        "town0e has no PURE effect-actor script (that is the whole point)"
    );
    assert_eq!(
        scene_entry_ambient_installs(&man_file, &man_bytes),
        vec![0],
        "the prologue slice reaches placement 29's install"
    );

    let mut world = World {
        frame_step: 2,
        ..Default::default()
    };
    world.install_field_stagers(&stager_bytes);
    world.set_vdf_buffer(legaia_engine_core::scene_bundle::find_vdf_buffer(&scene));
    for arg in scene_entry_ambient_installs(&man_file, &man_bytes) {
        world.spawn_ambient_record(arg as usize + 1, [0, 0, 0]);
    }
    assert_eq!(world.ambient_fx.len(), 7, "town0e's record-1 fan-out");

    // The morph carrier: stager record 11's mesh part binds env-pack slot 113
    // (`model_sel 118 - 5`) and its op-`0x0A` arms lanes 10 / 11.
    let parts = world.ambient_morph_parts();
    let morph = parts.first().expect("town0e arms a retail morph carrier");
    assert_eq!(parts.len(), 1, "one armed morph carrier");
    assert_eq!(morph.pack_slot, 113, "env-pack slot the mesh part binds");
    assert_eq!(
        morph.lanes.iter().map(|&(i, _)| i).collect::<Vec<_>>(),
        vec![10, 11],
        "op-0x0A lane sub-entry indices"
    );

    // The deltas actually move, and the dirty set names the slot so a render
    // surface rebuilds that mesh.
    let (_, pack_objects) = env_pack_objects(&scene);
    let n_verts = pack_objects[113][0];
    let mut dirty = false;
    let mut nonzero = false;
    let mut peaks = 0u32;
    let mut was_zero = true;
    for _ in 0..240 {
        world.tick_ambient_fx();
        if world
            .take_morph_dirty_slots()
            .iter()
            .any(|&(s, _)| s == 113)
        {
            dirty = true;
        }
        if let Some(d) = world.current_morph_deltas(113, 0, n_verts)
            && d.iter().any(|v| v != &[0, 0, 0])
        {
            nonzero = true;
        }
        let w = world
            .ambient_morph_parts()
            .first()
            .and_then(|p| p.lanes.iter().map(|&(_, w)| w).max())
            .unwrap_or(0);
        if w == 0 {
            was_zero = true;
        } else if was_zero {
            peaks += 1;
            was_zero = false;
        }
    }
    assert!(nonzero, "envelope-weighted deltas become nonzero");
    assert!(dirty, "the dirty set names the morphing pack slot");
    // town0e's record sets envelope flag `0x1000` - recycle, not hold - so the
    // pulse runs over and over rather than swelling once and stopping.
    assert!(
        peaks >= 2,
        "the 0x1000 envelope recycles the pulse ({peaks} peak(s) in 240 ticks)"
    );
}

/// Widening, not replacement: for every scene on the disc the prologue-slice
/// census returns a superset of the shape-filtered one, and for every scene
/// the shape filter found anything in, the two agree **exactly**.
///
/// This is the over-widening guard. The shape filter's carriers are the ones
/// already pinned by `field_ambient_fx_disc` / `ambient_mode4_scroll_disc` /
/// `vdf_morph_ambient_disc`, so a rule that started admitting records the VM
/// never reaches at entry would show up here as a disagreement on a scene
/// whose answer is known.
#[test]
fn entry_census_is_a_strict_widening_of_the_shape_filter_or_skip() {
    let Some(root) = extracted_root() else { return };
    let index = ProtIndex::open_extracted(&root).expect("prot index");

    let mut checked = 0usize;
    let mut with_pure = 0usize;
    let mut gained: Vec<String> = Vec::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        let Ok(Some(man_bytes)) = scene.field_man_payload(&index) else {
            continue;
        };
        let Ok(man_file) = legaia_asset::man_section::parse(&man_bytes) else {
            continue;
        };
        checked += 1;
        let pure = ambient_effect_installs(&man_file, &man_bytes);
        let entry = scene_entry_ambient_installs(&man_file, &man_bytes);
        for id in &pure {
            assert!(
                entry.contains(id),
                "{name}: shape-filtered install {id} dropped by the entry census \
                 (pure {pure:?}, entry {entry:?})"
            );
        }
        if !pure.is_empty() {
            with_pure += 1;
            assert_eq!(
                entry, pure,
                "{name}: a scene whose carriers were already correct must not change"
            );
        } else if !entry.is_empty() {
            gained.push(name.clone());
        }
        // No scene installs the same stager twice at entry - a widening that
        // double-spawned a tree would double its on-screen effect.
        let mut sorted = entry.clone();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(
            before,
            sorted.len(),
            "{name}: duplicate entry install {entry:?}"
        );
    }

    assert!(checked > 90, "surveyed {checked} scenes");
    assert_eq!(with_pure, 22, "scenes the shape filter already covered");

    // The scenes the widening adds, pinned so the set is reviewable rather
    // than merely "more". Every one carries the install in a placement's
    // unconditional entry prologue (`25` open, then the `34 30 xx`).
    assert_eq!(
        gained,
        vec![
            "izumi", "vell", "bylon", "dolk", "dolk2", "garmel", "keikoku", "jiji", "stone",
            "balden", "conc", "ropeway", "dohaty", "station", "map02", "jagaroom", "tunnelc",
            "balden2", "concnow", "map03", "bubu2", "uru", "uru2", "kor3", "koin2", "koin4",
            "juui1", "conc2", "nilboa", "nilboa2", "jouina", "jouinc", "jouind", "chitei2",
            "town0e", "opdeene", "opstati", "opurud", "koin1b", "edlast", "edkorout",
        ],
        "scenes whose ambient tree the entry census recovers"
    );
}

/// The census is an **under**-approximation, deliberately: a flag-gated
/// install deeper in a record is left out rather than guessed at. Pinned so
/// the conservative half of the rule is as reviewable as the permissive half.
#[test]
fn flag_gated_installs_past_the_unconditional_prefix_are_left_out_or_skip() {
    let Some(root) = extracted_root() else { return };
    let index = ProtIndex::open_extracted(&root).expect("prot index");

    // `nilboa` P1[3]: `25 / SysFlag.Clear / CFlag.Set / INSTALL 0 /
    // SysFlag.Test 1110 / INSTALL 2` - the second install is behind the test.
    let Some((_, _, man_bytes)) = scene_parts(&index, "nilboa") else {
        panic!("nilboa loads");
    };
    let man_file = legaia_asset::man_section::parse(&man_bytes).expect("parse MAN");
    assert_eq!(
        scene_entry_ambient_installs(&man_file, &man_bytes),
        vec![0],
        "nilboa: the flag-gated second install is not claimed"
    );

    // `suimon` P1[4] opens on two `SysFlag.Test`s before its two installs, so
    // neither is unconditionally reached and the scene reports none.
    let Some((_, _, man_bytes)) = scene_parts(&index, "suimon") else {
        panic!("suimon loads");
    };
    let man_file = legaia_asset::man_section::parse(&man_bytes).expect("parse MAN");
    assert!(
        scene_entry_ambient_installs(&man_file, &man_bytes).is_empty(),
        "suimon: both installs sit behind entry flag tests"
    );

    // `edkorout` P1[15] carries two installs split by a `0x21` nop - retail's
    // frame-slice break - so only the first lands in the load slice.
    let Some((_, _, man_bytes)) = scene_parts(&index, "edkorout") else {
        panic!("edkorout loads");
    };
    let man_file = legaia_asset::man_section::parse(&man_bytes).expect("parse MAN");
    assert_eq!(
        scene_entry_ambient_installs(&man_file, &man_bytes),
        vec![0],
        "edkorout: the post-`0x21` install is a later slice, not the load one"
    );
}

/// The env-pack TMD object vertex counts for a scene, indexed by pack slot.
fn env_pack_objects(scene: &Scene) -> (usize, Vec<Vec<usize>>) {
    let shared: Vec<Scene> = Vec::new();
    let shared_refs: Vec<&Scene> = shared.iter().collect();
    let (res, _) =
        legaia_engine_core::scene_resources::SceneResources::build_targeted_with_options(
            scene,
            &shared_refs,
            legaia_engine_core::scene_resources::BuildOptions {
                kind: legaia_engine_core::scene_resources::SceneLoadKind::Field,
                upload_all_tims: false,
                system_ui: None,
            },
        )
        .expect("scene resources");
    let env = field_env::env_pack_tmd_indices(scene, &res);
    let objects: Vec<Vec<usize>> = env
        .iter()
        .map(|&ti| {
            res.tmds[ti]
                .tmd
                .objects
                .iter()
                .map(|o| o.vertices.len())
                .collect()
        })
        .collect();
    (env.len(), objects)
}
