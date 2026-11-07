//! The export frame's handedness, on synthetic bytes: the world glb and an
//! animated prop placed from its manifest numbers land on the retail placement
//! seen through one rotation, for a record with all three angles set.

use super::*;
use legaia_actor_formats::gltf_color::glb_probe;
use legaia_engine_scene::field_env::{EnvDraw, FloorAnchor};
use legaia_tmd::mesh::VramMesh;
use psx_tim::Vram;

const VERTS: [[f32; 3]; 4] = [
    [30.0, -12.0, 5.0],
    [-7.0, 40.0, 22.0],
    [64.0, 0.0, -18.0],
    [1.0, 2.0, 3.0],
];
const SCALE: f32 = 1.0 / 128.0;

fn tilted_draw() -> EnvDraw {
    EnvDraw {
        far_corner_order: false,
        env_slot: 0,
        res_tmd: 0,
        world_x: 2744,
        world_y: -192,
        world_z: 1958,
        rot_y: 0x6A0,
        rot_x: 0x155,
        rot_z: 0xF40,
        anim_id: 1,
        anchor: (0, 0),
        floor: FloorAnchor {
            corners: None,
            nibble: None,
        },
        cell: (0, 0),
        cull_radius: 0,
        view_skip: 0,
    }
}

fn qrot(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let t = cross([x, y, z], v).map(|c| 2.0 * c);
    let ut = cross([x, y, z], t);
    [
        v[0] + w * t[0] + ut[0],
        v[1] + w * t[1] + ut[1],
        v[2] + w * t[2] + ut[2],
    ]
}

/// The retail placement of `v`, through the export frame and scale.
fn retail_truth(d: &EnvDraw, v: [f32; 3]) -> [f32; 3] {
    let r = d.rotate(v);
    let p = [
        d.world_x as f32 + r[0],
        d.world_y as f32 + r[1],
        d.world_z as f32 + r[2],
    ];
    [p[0] * SCALE, -p[1] * SCALE, -p[2] * SCALE]
}

fn node_array<const N: usize>(n: &Value, key: &str, default: [f32; N]) -> [f32; N] {
    match n[key].as_array() {
        Some(a) => std::array::from_fn(|i| a[i].as_f64().unwrap() as f32),
        None => default,
    }
}

fn assert_close(a: [f32; 3], b: [f32; 3], what: &str) {
    for k in 0..3 {
        assert!((a[k] - b[k]).abs() < 1e-3, "{what}: {a:?} vs {b:?}");
    }
}

fn world_glb(d: &EnvDraw) -> Vec<u8> {
    let t = draw_translation([d.world_x as f32, d.world_y as f32, d.world_z as f32]);
    let mesh = SceneMesh {
        name: "twin".into(),
        positions: VERTS.iter().flatten().copied().collect(),
        uvs: vec![0; 8],
        cba_tsb: vec![0, 5, 0, 5, 0, 5, 0, 5],
        indices: vec![0, 1, 2, 1, 3, 2],
        flat_rgba: Vec::new(),
        morph_targets: Vec::new(),
    };
    let inst = SceneInstance {
        mesh: 0,
        translation: t.map(|c| c * SCALE),
        rot_y: draw_rot_y_radians(d.rot_y),
        rotation: Some(draw_rotation_quat(d)),
        scale: SCALE,
    };
    legaia_actor_formats::scene_gltf::build_scene_glb("w", &[mesh], &[inst], &Vram::new()).unwrap()
}

/// A pitched, rolled and yawed record: every world glb vertex lands on the
/// retail placement seen through the export frame.
#[test]
fn world_vertices_land_on_the_retail_placement() {
    let d = tilted_draw();
    let glb = world_glb(&d);
    let (root, bin) = glb_probe::split(&glb).unwrap();
    let node = &root["nodes"][0];
    let t: [f32; 3] = node_array(node, "translation", [0.0; 3]);
    let q: [f32; 4] = node_array(node, "rotation", [0.0, 0.0, 0.0, 1.0]);
    let s: [f32; 3] = node_array(node, "scale", [1.0; 3]);
    let acc = root["meshes"][0]["primitives"][0]["attributes"]["POSITION"]
        .as_u64()
        .unwrap() as usize;
    let baked = glb_probe::floats(&root, bin, acc).unwrap();
    for (v, p) in VERTS.iter().zip(&baked) {
        let r = qrot(q, [p[0] * s[0], p[1] * s[1], p[2] * s[2]]);
        let got = [r[0] + t[0], r[1] + t[1], r[2] + t[2]];
        assert_close(got, retail_truth(&d, *v), "world vertex");
    }
}

/// The prop file placed from the manifest's own numbers, with no mirror
/// anywhere, lands on the same points as its baked world twin: the file's
/// root node rotation and the world's bake are the same frame.
#[test]
fn prop_placed_from_manifest_numbers_lands_on_its_twin() {
    let d = tilted_draw();
    let mesh = VramMesh {
        positions: VERTS.to_vec(),
        uvs: vec![[0, 0]; 4],
        cba_tsb: vec![[0, 5]; 4],
        indices: vec![0, 1, 2, 1, 3, 2],
        colors: vec![[0x80; 3]; 4],
        normals: Vec::new(),
    };
    let glb = build_character_glb_hybrid("prop", &mesh, &[0, 0, 0, 0], &Vram::new(), &[], None)
        .expect("prop glb");
    let glb = scale_glb_scene_roots(glb, SCALE);
    let (root, bin) = glb_probe::split(&glb).unwrap();
    let nodes = root["nodes"].as_array().unwrap();
    let file_root = nodes.iter().find(|n| n["children"].is_array()).unwrap();
    let root_q: [f32; 4] = node_array(file_root, "rotation", [0.0, 0.0, 0.0, 1.0]);
    let root_s: [f32; 3] = node_array(file_root, "scale", [1.0; 3]);
    let mesh_node = nodes.iter().find(|n| n["mesh"].is_u64()).unwrap();
    let acc =
        root["meshes"][mesh_node["mesh"].as_u64().unwrap() as usize]["primitives"][0]["attributes"]
            ["POSITION"]
            .as_u64()
            .unwrap() as usize;
    let file_verts = glb_probe::floats(&root, bin, acc).unwrap();

    // The manifest's numbers for this instance.
    let t = export_translation([d.world_x as f32, d.world_y as f32, d.world_z as f32])
        .map(|c| c * SCALE);
    let q = export_rotation(&d);

    let placed: Vec<[f32; 3]> = file_verts
        .iter()
        .map(|p| {
            let rooted = qrot(
                root_q,
                [p[0] * root_s[0], p[1] * root_s[1], p[2] * root_s[2]],
            );
            let r = qrot(q, rooted);
            [r[0] + t[0], r[1] + t[1], r[2] + t[2]]
        })
        .collect();
    for v in VERTS {
        let want = retail_truth(&d, v);
        let best = placed
            .iter()
            .map(|p| {
                (0..3)
                    .map(|k| (p[k] - want[k]).abs())
                    .fold(0.0f32, f32::max)
            })
            .fold(f32::MAX, f32::min);
        assert!(best < 1e-3, "{v:?}: nearest placed vertex is {best} away");
    }
}

/// A yaw-only record: the manifest's legacy yaw field is its full rotation.
#[test]
fn yaw_field_matches_the_rotation_for_a_yaw_only_record() {
    let mut d = tilted_draw();
    (d.rot_x, d.rot_z) = (0, 0);
    let q = export_rotation(&d);
    let h = export_yaw_radians(d.rot_y) / 2.0;
    let want = [0.0, h.sin(), 0.0, h.cos()];
    let dot: f32 = q.iter().zip(want).map(|(a, b)| a * b).sum();
    assert!((dot.abs() - 1.0).abs() < 1e-5, "{q:?} vs {want:?}");
}

/// Heading `0` faces retail `+Z`, which is the export frame's `-Z`.
#[test]
fn facing_direction_is_in_the_export_frame() {
    let d = traversal::heading_dir_xz(0);
    assert!(d[0].abs() < 1e-6 && (d[1] + 1.0).abs() < 1e-6, "{d:?}");
    let east = traversal::heading_dir_xz(0x400);
    assert!((east[0] - 1.0).abs() < 1e-6, "{east:?}");
}
