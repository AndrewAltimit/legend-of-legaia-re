//! Disc-gated: the VDF vertex-morph chain from scene entry to moving
//! vertices, on the real disc bytes.
//!
//! Two arms:
//!
//! 1. **Retail arming** - rikuroa's entry-ambient install (P1 arg 0 ->
//!    record 1) spawns the flag-gated morph records 69/70 (`model_sel`
//!    63/64 = pack slots 58/59) once system flags 0x281/0x282 are set;
//!    each runs move-VM op `0x0A`, the ambient tick's ramp envelope
//!    (`FUN_80020740` bridge) moves their lane weights, and
//!    `World::current_morph_deltas` yields time-varying deltas. With the
//!    flags clear nothing arms - the faithful negative.
//! 2. **jou entry pulse** (enhancement) - jou's entry tree arms no morph
//!    lanes in any state (the pack is cutscene-armed in retail), so the
//!    host installs the scene-entry VDF pulse over the 17-sub-entry pack;
//!    the flesh-ground meshes' deltas move across ticks and the dirty set
//!    names drawn pack slots.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;

use legaia_engine_core::field_env;
use legaia_engine_core::man_field_scripts::ambient_effect_installs;
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

/// Spawn a scene's ambient tree + VDF buffer into a bare `World`, the way
/// `SceneHost::enter_field_scene` does, and return it with the env-pack
/// object vertex counts. `flags` are set **before** the ambient spawn so
/// flag-gated installer branches see them (the spawn-time first run).
fn ambient_world(
    root: &std::path::Path,
    name: &str,
    flags: &[u16],
) -> Option<(World, Vec<Vec<usize>>)> {
    let index = ProtIndex::open_extracted(root).expect("prot index");
    let scene = Scene::load(&index, name).expect("load scene");
    let scripts = scene.find_event_scripts()?;
    let stager_bytes = scripts.bytes.to_vec();
    let man_bytes = scene.field_man_payload(&index).ok()??;
    let man_file = legaia_asset::man_section::parse(&man_bytes).ok()?;

    let mut world = World {
        frame_step: 2,
        ..Default::default()
    };
    world.install_field_stagers(&stager_bytes);
    world.set_vdf_buffer(legaia_engine_core::scene_bundle::find_vdf_buffer(&scene));
    for &f in flags {
        world.system_flag_set(f);
    }
    for arg in ambient_effect_installs(&man_file, &man_bytes) {
        world.spawn_ambient_record(arg as usize + 1, [0, 0, 0]);
    }

    let shared: Vec<Scene> = Vec::new();
    let shared_refs: Vec<&Scene> = shared.iter().collect();
    let (res, _) =
        legaia_engine_core::scene_resources::SceneResources::build_targeted_with_options(
            &scene,
            &shared_refs,
            legaia_engine_core::scene_resources::BuildOptions {
                kind: legaia_engine_core::scene_resources::SceneLoadKind::Field,
                upload_all_tims: false,
                system_ui: None,
            },
        )
        .ok()?;
    let env = field_env::env_pack_tmd_indices(&scene, &res);
    let pack_objects: Vec<Vec<usize>> = env
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
    Some((world, pack_objects))
}

#[test]
fn rikuroa_flag_gated_entry_tree_arms_retail_morph_lanes_or_skip() {
    let Some(root) = extracted_root() else { return };

    // Faithful negative: with system flags 0x281/0x282 clear, record 1's
    // ext-0x14 gates skip the morph-record spawns entirely.
    let Some((bare, pack_objects)) = ambient_world(&root, "rikuroa", &[]) else {
        panic!("rikuroa ambient world builds");
    };
    assert!(
        bare.ambient_morph_parts().is_empty(),
        "flags clear -> no morph carriers spawn"
    );
    // And the entry pulse must stand aside anyway: the stager table
    // carries op-0x0A records, so retail owns this scene's morphs.
    {
        let mut bare = bare;
        assert!(
            !bare.install_entry_vdf_pulse(&pack_objects),
            "entry pulse stands aside where any stager record arms morphs"
        );
    }

    // With the gates open, records 69/70 spawn: model_sel 63/64 = pack
    // slots 58/59, lane indices [9, A, B] / [D, E, F] (op 0x0A operands).
    let Some((mut world, pack_objects)) = ambient_world(&root, "rikuroa", &[0x281, 0x282]) else {
        panic!("rikuroa ambient world builds");
    };
    let parts = world.ambient_morph_parts();
    let mut slots: Vec<usize> = parts.iter().map(|p| p.pack_slot).collect();
    slots.sort_unstable();
    slots.dedup();
    assert_eq!(slots, vec![58, 59], "rikuroa retail morph carriers");
    let sac = parts
        .iter()
        .find(|p| p.pack_slot == 58)
        .expect("slot-58 part");
    assert_eq!(
        sac.lanes.iter().map(|&(i, _)| i).collect::<Vec<_>>(),
        vec![0x09, 0x0A, 0x0B, 0x0C],
        "record 69's lane indices (op 0x0A count = 4)"
    );

    // Envelope: record 54/69 sets HOLD (`op 0x32 [0x400]`), so the sac
    // swells to full weight and stays there - deltas ramp to nonzero and
    // the dirty set surfaces the slot while the weights move.
    let n_verts = pack_objects[58][0];
    assert_eq!(n_verts, 43, "slot-58 generator-sac vertex count");
    let mut seen_nonzero = false;
    let mut dirty = false;
    for _ in 0..64 {
        world.tick_ambient_fx();
        if world.take_morph_dirty_slots().iter().any(|&(s, _)| s == 58) {
            dirty = true;
        }
        if let Some(d) = world.current_morph_deltas(58, 0, n_verts)
            && d.iter().any(|v| v != &[0, 0, 0])
        {
            seen_nonzero = true;
        }
    }
    assert!(seen_nonzero, "envelope-weighted deltas become nonzero");
    assert!(dirty, "the dirty set names the morphing pack slot");
    // At HOLD the staged deltas equal the authored full-weight deltas of
    // the lanes' sub-entries summed - the swollen rest state.
    let lanes: Vec<(u8, u16)> = sac.lanes.iter().map(|&(i, _)| (i, 0x1000)).collect();
    let full = world.morph_deltas_for(&lanes, 0, n_verts);
    let held = world
        .current_morph_deltas(58, 0, n_verts)
        .expect("held deltas");
    assert_eq!(held, full, "HOLD keeps every lane at peak weight");
}

#[test]
fn jou_entry_pulse_moves_the_flesh_ground_deltas_or_skip() {
    let Some(root) = extracted_root() else { return };
    let Some((mut world, pack_objects)) = ambient_world(&root, "jou", &[]) else {
        panic!("jou ambient world builds");
    };

    // jou's entry tree arms NO retail morph lanes (the 17 sub-entries are
    // cutscene-armed) - the pinned negative that motivates the pulse.
    assert!(
        world.ambient_morph_parts().is_empty(),
        "jou has no retail entry-armed morph parts"
    );

    // The enhancement pulse installs over the 17-sub-entry pack and
    // targets drawn flesh-ground meshes (the v102 / v112 / v213 family).
    assert!(
        world.install_entry_vdf_pulse(&pack_objects),
        "jou entry pulse installs"
    );

    let mut dirty_all: std::collections::BTreeSet<(usize, u32)> = Default::default();
    let mut moving_slot: Option<(usize, u32, usize)> = None;
    let mut changed = false;
    let mut prev: Option<Vec<[i16; 3]>> = None;
    for _ in 0..600 {
        world.tick_ambient_fx();
        for pair in world.take_morph_dirty_slots() {
            dirty_all.insert(pair);
        }
        let (slot, group, n) = *moving_slot.get_or_insert_with(|| {
            // First dirty slot becomes the tracked mesh.
            let &(s, g) = dirty_all.iter().next().unwrap_or(&(4, 0));
            (s, g, pack_objects[s][g as usize])
        });
        if let Some(d) = world.current_morph_deltas(slot, group, n) {
            if let Some(p) = &prev
                && *p != d
                && d.iter().any(|v| v != &[0, 0, 0])
            {
                changed = true;
            }
            prev = Some(d);
        }
    }
    assert!(
        dirty_all.len() >= 4,
        "the pulse touches several pack meshes: {dirty_all:?}"
    );
    // The 102-vertex ground pieces (pack slots 4/5) are prime jou targets.
    assert!(
        dirty_all
            .iter()
            .any(|&(s, _)| pack_objects[s].first().is_some_and(|&n| n >= 75)),
        "a real flesh-ground mesh is targeted: {dirty_all:?}"
    );
    assert!(changed, "the tracked mesh's deltas move across ticks");
}
