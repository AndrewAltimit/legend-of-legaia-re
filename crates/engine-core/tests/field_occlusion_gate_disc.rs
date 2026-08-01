//! Disc-gated: the camera-occlusion fade's visibility gate must not count
//! invisible geometry as an occluder.
//!
//! The concrete fixture is the keikoku canyon corridor: walking up the path
//! at `x = 7392` the gate armed while the character was plainly on screen,
//! blocked by a 96x320 wall quad at `z = 3776` that never renders visibly.
//! This test rebuilds the scene exactly as the hosts do, reproduces the
//! ray-cast with the captured eye/centre, and dissects whatever blocks it.
//!
//! Skips when `LEGAIA_DISC_BIN` is unset (disc-gated convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::field_env;
use legaia_engine_core::field_occlusion::FieldOccluders;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// The captured false-positive ray from the play-window debug run: eye =
/// the follow camera on the canyon path, centre = the player's body centre
/// mid-corridor, plainly visible in the frame.
const EYE: [f32; 3] = [7167.18, -764.11, 2886.46];
const CENTRE: [f32; 3] = [7392.0, -65.0, 3784.0];

#[test]
fn keikoku_corridor_walk_does_not_arm_the_gate() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open prot index"));
    let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
        .iter()
        .filter_map(|n| Scene::load(&index, n).ok())
        .collect();
    let shared_refs: Vec<&Scene> = shared.iter().collect();
    let scene = Scene::load(&index, "keikoku").expect("load keikoku");
    let (res, _) = SceneResources::build_targeted_with_options(
        &scene,
        &shared_refs,
        BuildOptions {
            kind: SceneLoadKind::Field,
            upload_all_tims: true,
            system_ui: index.system_ui_bundle().ok().as_deref(),
        },
    )
    .expect("build keikoku resources");

    // The same static draw lists the hosts feed the kernel.
    let env_tmds = field_env::env_pack_tmd_indices(&scene, &res);
    let floor_lut = scene.field_floor_height_lut(&index).ok().flatten();
    let tiles: Vec<_> = scene
        .field_terrain_tiles(&index)
        .ok()
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.flags & legaia_asset::field_objects::FLAG_PLACED == 0)
        .collect();
    let (terrain, _) = field_env::resolve_env_draws(&env_tmds, &tiles, floor_lut);
    let placements_rec = scene
        .field_object_placements(&index)
        .ok()
        .flatten()
        .unwrap_or_default();
    let binds = scene.field_object_binds(&index).ok().flatten();
    let (placements, _) =
        field_env::resolve_placed_env_draws(&env_tmds, &placements_rec, floor_lut, binds.as_ref());

    let occ = FieldOccluders::build(&[&terrain, &placements], &res);
    assert!(!occ.is_empty(), "keikoku should have occluders");

    if let Some((tri, res_tmd)) = occ.first_hit(EYE, CENTRE) {
        // Dissect: find the draw instance + local prim behind the hit.
        eprintln!("BLOCKED by tri {tri:?} from res_tmd {res_tmd}");
        for d in terrain.iter().chain(placements.iter()) {
            if d.res_tmd != res_tmd {
                continue;
            }
            let t = [d.world_x as f32, d.world_y as f32, d.world_z as f32];
            if (t[0] - tri[0][0]).abs() > 300.0 || (t[2] - tri[0][2]).abs() > 300.0 {
                continue;
            }
            eprintln!(
                "  candidate draw at ({}, {}, {}) rot_y {} env_slot {}",
                d.world_x, d.world_y, d.world_z, d.rot_y, d.env_slot
            );
            let rt = &res.tmds[res_tmd];
            let mesh =
                legaia_tmd::mesh::tmd_to_vram_mesh_filtered(&rt.tmd, &rt.raw, |_, _, _| true);
            // Local-space triangle (rot_y observed 0 for this fixture).
            for (pi, ptri) in mesh.indices.chunks_exact(3).enumerate() {
                let p0 = mesh.positions[ptri[0] as usize];
                let close = |a: [f32; 3], b: [f32; 3]| {
                    (a[0] + t[0] - b[0]).abs() < 1.0
                        && (a[1] + t[1] - b[1]).abs() < 1.0
                        && (a[2] + t[2] - b[2]).abs() < 1.0
                };
                if close(p0, tri[0]) || close(p0, tri[1]) || close(p0, tri[2]) {
                    let v = ptri[0] as usize;
                    let (cba, tsb) = (mesh.cba_tsb[v][0], mesh.cba_tsb[v][1]);
                    let uvs: Vec<(u8, u8)> = ptri
                        .iter()
                        .map(|&i| (mesh.uvs[i as usize][0], mesh.uvs[i as usize][1]))
                        .collect();
                    eprintln!(
                        "  textured prim {pi}: cba {cba:#06x} tsb {tsb:#06x} uvs {uvs:?} \
                         bbox-opacity {:.2}",
                        res.vram.prim_opaque_fraction(cba, tsb, &uvs)
                    );
                }
            }
            let cmesh = legaia_tmd::mesh::tmd_to_color_mesh(&rt.tmd, &rt.raw);
            for ptri in cmesh.indices.chunks_exact(3) {
                let p0 = cmesh.positions[ptri[0] as usize];
                let close = |a: [f32; 3], b: [f32; 3]| {
                    (a[0] + t[0] - b[0]).abs() < 1.0
                        && (a[1] + t[1] - b[1]).abs() < 1.0
                        && (a[2] + t[2] - b[2]).abs() < 1.0
                };
                if close(p0, tri[0]) || close(p0, tri[1]) || close(p0, tri[2]) {
                    eprintln!("  COLOUR-half prim matches the phantom");
                }
            }
        }
        panic!("visible-character ray must not be blocked (see dissection above)");
    }
}
