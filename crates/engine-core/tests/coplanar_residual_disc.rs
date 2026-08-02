//! Disc-gated: the coplanar z-fight mitigation stack actually resolves the
//! coincidences it exists for, measured on the final geometry.
//!
//! Rebuilds a field scene's world-space triangle soup exactly as the hosts
//! draw it - per-TMD `legaia_tmd::mesh::resolve_hybrid` (double-sided flags +
//! intra-mesh decal nudges), yaw-rotated `EnvDraw` instancing, cross-draw
//! `coplanar_draws::coplanar_draw_offsets` lifts - then scans for coplanar
//! overlapping triangle pairs that SURVIVE all of it. Every survivor is a
//! view-angle-dependent z-fight in every depth-tested host.
//!
//! Pairs that cannot shimmer are excluded: double-sided-flagged copies (the
//! fragment shaders discard the away-facing copy) and vertex-identical
//! triangles (bit-identical depth interpolation - stable overdraw).
//!
//! The assertion pins the scenes that regressed before: koin6 (the reported
//! inn floor, clean) and koin4 (one sub-100-area wall sliver its detection
//! floor still misses - bounded, not zero). The full-corpus sweep is the
//! diagnostic mode: `DIAG_ALL=1 cargo test -p legaia-engine-core --release
//! --test coplanar_residual_disc -- --nocapture` prints per-scene survivor
//! tables; known residual classes live with the open threads in
//! `docs/reference/open-rev-eng-threads.md`.
//!
//! Skips when `LEGAIA_DISC_BIN` is unset (disc-gated convention).

use std::collections::HashMap;
use std::path::PathBuf;

use legaia_engine_core::coplanar_draws;
use legaia_engine_core::field_env::{self, EnvDraw};
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

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn dominant_axis(n: [f32; 3]) -> usize {
    let a = [n[0].abs(), n[1].abs(), n[2].abs()];
    if a[0] >= a[1] && a[0] >= a[2] {
        0
    } else if a[1] >= a[2] {
        1
    } else {
        2
    }
}
fn project2(v: [f32; 3], ax: usize) -> [f32; 2] {
    match ax {
        0 => [v[1], v[2]],
        1 => [v[0], v[2]],
        _ => [v[0], v[1]],
    }
}
fn poly_area2(p: &[[f32; 2]]) -> f32 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let j = (i + 1) % p.len();
        a += p[i][0] * p[j][1] - p[j][0] * p[i][1];
    }
    a.abs() * 0.5
}
/// Sutherland-Hodgman clip of `subject` by the convex triangle `clip`.
fn clip_tri_tri(mut subject: Vec<[f32; 2]>, clip: &[[f32; 2]; 3]) -> Vec<[f32; 2]> {
    let mut c = *clip;
    let signed =
        (c[1][0] - c[0][0]) * (c[2][1] - c[0][1]) - (c[2][0] - c[0][0]) * (c[1][1] - c[0][1]);
    if signed < 0.0 {
        c.swap(1, 2);
    }
    for e in 0..3 {
        let a = c[e];
        let b = c[(e + 1) % 3];
        if subject.is_empty() {
            return subject;
        }
        let inside =
            |p: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) >= 0.0;
        let isect = |p: [f32; 2], q: [f32; 2]| -> [f32; 2] {
            let r = [q[0] - p[0], q[1] - p[1]];
            let s = [b[0] - a[0], b[1] - a[1]];
            let denom = r[0] * s[1] - r[1] * s[0];
            if denom.abs() < 1e-9 {
                return p;
            }
            let t = ((a[0] - p[0]) * s[1] - (a[1] - p[1]) * s[0]) / denom;
            [p[0] + t * r[0], p[1] + t * r[1]]
        };
        let mut out: Vec<[f32; 2]> = Vec::new();
        for i in 0..subject.len() {
            let cur = subject[i];
            let prev = subject[(i + subject.len() - 1) % subject.len()];
            let (ci, pi) = (inside(cur), inside(prev));
            if ci {
                if !pi {
                    out.push(isect(prev, cur));
                }
                out.push(cur);
            } else if pi {
                out.push(isect(prev, cur));
            }
        }
        subject = out;
    }
    subject
}

struct WTri {
    /// Index into the combined terrain-then-placements draw list.
    draw: usize,
    res_tmd: usize,
    colour_half: bool,
    v: [[f32; 3]; 3],
    n: [f32; 3],
    d: f32,
    area: f32,
    p2: [[f32; 2]; 3],
}

fn rot_y(v: [f32; 3], s: f32, c: f32) -> [f32; 3] {
    [c * v[0] + s * v[2], v[1], -s * v[0] + c * v[2]]
}

#[test]
fn coplanar_mitigation_leaves_no_fighting_pairs() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    if std::env::var_os("DIAG_ALL").is_some() {
        // Diagnostic sweep: print every field scene's survivors, assert
        // nothing (the corpus has a documented small-area tail).
        let cdname =
            legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
        let mut names: Vec<String> = cdname.values().cloned().collect();
        names.sort();
        names.dedup();
        for name in &names {
            if legaia_engine_core::scene::is_world_map_scene(name) {
                continue;
            }
            diag_scene(&index, name);
        }
        return;
    }
    // koin6: the user-reported inn floor. Every coplanar pair must resolve.
    let (groups, area) = diag_scene(&index, "koin6");
    assert_eq!(
        groups, 0,
        "koin6 has {groups} surviving coplanar overlap groups ({area:.0} area)"
    );
    // koin4: one sub-100-area wall sliver sits below the cross-draw pass's
    // plane-cluster detection floor. Bound it so a regression that re-opens
    // the large floor/wall fights (12k+ area before the per-family lifts)
    // cannot hide behind the known sliver.
    let (_, area) = diag_scene(&index, "koin4");
    assert!(
        area < 500.0,
        "koin4 residual coplanar overlap grew to {area:.0} area"
    );
}

/// Build one scene's post-mitigation world soup and report the surviving
/// coplanar overlapping pairs. Returns `(survivor_groups, total_overlap_area)`.
fn diag_scene(index: &ProtIndex, name: &str) -> (usize, f32) {
    let Ok(scene) = Scene::load(index, name) else {
        return (0, 0.0);
    };
    eprintln!("=== scene {name} ===");
    let mut shared_scenes: Vec<Scene> = Vec::new();
    for n in FIELD_SHARED_BLOCKS {
        if let Ok(s) = Scene::load(index, n) {
            shared_scenes.push(s);
        }
    }
    let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();
    let system_ui = index.system_ui_bundle().ok();
    let (res, _stats) = SceneResources::build_targeted_with_options(
        &scene,
        &shared_refs,
        BuildOptions {
            kind: SceneLoadKind::Field,
            upload_all_tims: true,
            system_ui: system_ui.as_deref(),
        },
    )
    .expect("scene resources");

    // The same assembly the hosts run (web `build_field_render` / native
    // `resolve_field_*`): placements + FLAG_PLACED-filtered terrain, lifted
    // by the shared cross-draw kernel over the combined list.
    let env_tmds = field_env::env_pack_tmd_indices(&scene, &res);
    let floor_lut = scene.field_floor_height_lut(index).ok().flatten();
    let placement_records = scene
        .field_object_placements(index)
        .ok()
        .flatten()
        .unwrap_or_default();
    let terrain_records: Vec<_> = scene
        .field_terrain_tiles(index)
        .ok()
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.flags & legaia_asset::field_objects::FLAG_PLACED == 0)
        .collect();
    let (placements, _) = field_env::resolve_env_draws(&env_tmds, &placement_records, floor_lut);
    let (terrain, _) = field_env::resolve_env_draws(&env_tmds, &terrain_records, floor_lut);
    let mut combined: Vec<EnvDraw> = Vec::with_capacity(terrain.len() + placements.len());
    combined.extend_from_slice(&terrain);
    combined.extend_from_slice(&placements);
    let planes = coplanar_draws::draw_plane_summaries(&combined, &res);
    let offsets = coplanar_draws::coplanar_draw_offsets(&combined, &planes);
    eprintln!(
        "  draws: {} terrain + {} placements; {} cross-draw lifts",
        terrain.len(),
        placements.len(),
        offsets.len()
    );

    // Per-res_tmd hybrid meshes, post resolve_hybrid, halves kept separate
    // so each half's double-sided flag channel stays readable.
    struct Hybrid {
        vmesh: legaia_tmd::mesh::VramMesh,
        cmesh: legaia_tmd::mesh::ColorMesh,
    }
    let mut hybrids: HashMap<usize, Hybrid> = HashMap::new();
    for d in &combined {
        if hybrids.contains_key(&d.res_tmd) {
            continue;
        }
        let Some(rt) = res.tmds.get(d.res_tmd) else {
            continue;
        };
        let mut vmesh = rt.build_filtered_vram_mesh(&res.vram);
        let mut cmesh = legaia_tmd::mesh::tmd_to_color_mesh(&rt.tmd, &rt.raw);
        legaia_tmd::mesh::resolve_hybrid(&mut vmesh, &mut cmesh);
        hybrids.insert(d.res_tmd, Hybrid { vmesh, cmesh });
    }

    // World soup with per-draw yaw + translation + coplanar lift applied.
    let mut soup: Vec<WTri> = Vec::new();
    for (di, d) in combined.iter().enumerate() {
        let Some(h) = hybrids.get(&d.res_tmd) else {
            continue;
        };
        let off = offsets.get(d).copied().unwrap_or([0.0; 3]);
        let ang = f32::from(d.rot_y & 0x0FFF) * (std::f32::consts::TAU / 4096.0);
        let (s, c) = ang.sin_cos();
        let t = [
            d.world_x as f32 + off[0],
            d.world_y as f32 + off[1],
            d.world_z as f32 + off[2],
        ];
        let mut add = |positions: &[[f32; 3]],
                       indices: &[u32],
                       ds_flag: &dyn Fn(u32) -> bool,
                       colour_half: bool| {
            for tri in indices.chunks_exact(3) {
                // Double-sided-pair copies are resolved by the shaders'
                // facing discard, not by geometry - not a residual fight.
                if ds_flag(tri[0]) {
                    continue;
                }
                let v: Vec<[f32; 3]> = tri
                    .iter()
                    .map(|&i| {
                        let w = rot_y(positions[i as usize], s, c);
                        [w[0] + t[0], w[1] + t[1], w[2] + t[2]]
                    })
                    .collect();
                let cr = cross(sub(v[1], v[0]), sub(v[2], v[0]));
                let len = dot(cr, cr).sqrt();
                if len < 1e-6 {
                    continue;
                }
                let area = len * 0.5;
                if area < 1.0 {
                    continue;
                }
                let mut n = [cr[0] / len, cr[1] / len, cr[2] / len];
                let ax = dominant_axis(n);
                if n[ax] < 0.0 {
                    n = [-n[0], -n[1], -n[2]];
                }
                let dd = dot(n, v[0]);
                soup.push(WTri {
                    draw: di,
                    res_tmd: d.res_tmd,
                    colour_half,
                    v: [v[0], v[1], v[2]],
                    n,
                    d: dd,
                    area,
                    p2: [project2(v[0], ax), project2(v[1], ax), project2(v[2], ax)],
                });
            }
        };
        let vm = &h.vmesh;
        let cm = &h.cmesh;
        add(
            &vm.positions,
            &vm.indices,
            &|i| vm.cba_tsb[i as usize][0] & legaia_tmd::mesh::CBA_DOUBLE_SIDED_BIT != 0,
            false,
        );
        add(
            &cm.positions,
            &cm.indices,
            &|i| cm.blend[i as usize] & legaia_tmd::mesh::BLEND_DOUBLE_SIDED_BIT != 0,
            true,
        );
    }
    // The walk-ground heightfield layer, sunk exactly as the render sites
    // sink it (`GROUND_SINK`). Before the sink, koin6's entire ground grid
    // sat on the env floor slabs' plane - the class this layer's inclusion
    // pins.
    if let Ok(Some(hf)) = scene.walk_heightfield(index) {
        const GROUND_DRAW: usize = usize::MAX;
        for tri in hf.indices.chunks_exact(3) {
            let v: Vec<[f32; 3]> = tri
                .iter()
                .map(|&i| {
                    let p = hf.positions[i as usize];
                    [p[0], p[1] + coplanar_draws::GROUND_SINK, p[2]]
                })
                .collect();
            let cr = cross(sub(v[1], v[0]), sub(v[2], v[0]));
            let len = dot(cr, cr).sqrt();
            if len < 1e-6 {
                continue;
            }
            let area = len * 0.5;
            if area < 1.0 {
                continue;
            }
            let mut n = [cr[0] / len, cr[1] / len, cr[2] / len];
            let ax = dominant_axis(n);
            if n[ax] < 0.0 {
                n = [-n[0], -n[1], -n[2]];
            }
            let dd = dot(n, v[0]);
            soup.push(WTri {
                draw: GROUND_DRAW,
                res_tmd: GROUND_DRAW,
                colour_half: false,
                v: [v[0], v[1], v[2]],
                n,
                d: dd,
                area,
                p2: [project2(v[0], ax), project2(v[1], ax), project2(v[2], ax)],
            });
        }
    }

    // Bucket by quantized plane; scan for surviving coincident overlaps.
    let mut buckets: HashMap<(i32, i32, i32, i64), Vec<usize>> = HashMap::new();
    for (i, r) in soup.iter().enumerate() {
        let base = (
            (r.n[0] * 512.0).round() as i32,
            (r.n[1] * 512.0).round() as i32,
            (r.n[2] * 512.0).round() as i32,
        );
        for dd in -1..=1i64 {
            buckets
                .entry((base.0, base.1, base.2, (r.d / 2.0).floor() as i64 + dd))
                .or_default()
                .push(i);
        }
    }
    // survivor key: (draw_a, res_a, half_a, draw_b, res_b, half_b)
    #[allow(clippy::type_complexity)]
    let mut survivors: HashMap<
        (usize, usize, bool, usize, usize, bool),
        (usize, f32, [f32; 3], [f32; 3]),
    > = HashMap::new();
    let mut seen: std::collections::HashSet<(usize, usize)> = Default::default();
    for members in buckets.values() {
        for xi in 0..members.len() {
            for yi in (xi + 1)..members.len() {
                let (i, j) = (members[xi].min(members[yi]), members[xi].max(members[yi]));
                if i == j || seen.contains(&(i, j)) {
                    continue;
                }
                let (a, b) = (&soup[i], &soup[j]);
                if dot(a.n, b.n) < 0.9999 || (a.d - b.d).abs() > 0.05 {
                    continue;
                }
                // Vertex-identical triangles (same three world positions in
                // any order) interpolate to bit-identical depths: the depth
                // test resolves them deterministically (first drawn wins) -
                // stable overdraw, not shimmer.
                let same_verts = a.v.iter().all(|va| {
                    b.v.iter().any(|vb| {
                        (va[0] - vb[0]).abs() < 1e-3
                            && (va[1] - vb[1]).abs() < 1e-3
                            && (va[2] - vb[2]).abs() < 1e-3
                    })
                });
                if same_verts {
                    continue;
                }
                let clipped = clip_tri_tri(a.p2.to_vec(), &b.p2);
                let ov = if clipped.len() < 3 {
                    0.0
                } else {
                    poly_area2(&clipped)
                };
                if ov < 4.0 || ov < 0.02 * a.area.min(b.area) {
                    continue;
                }
                seen.insert((i, j));
                let key = (
                    a.draw,
                    a.res_tmd,
                    a.colour_half,
                    b.draw,
                    b.res_tmd,
                    b.colour_half,
                );
                let e = survivors.entry(key).or_insert((0, 0.0, a.v[0], a.n));
                e.0 += 1;
                e.1 += ov;
            }
        }
    }
    let mut rows: Vec<_> = survivors.into_iter().collect();
    rows.sort_by(|a, b| {
        b.1.1
            .partial_cmp(&a.1.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let total_area: f32 = rows.iter().map(|(_, (_, a, _, _))| a).sum();
    eprintln!("  surviving coplanar overlap groups: {}", rows.len());
    for ((da, ra, ha, db, rb, hb), (pairs, area, at, n)) in rows.iter().take(30) {
        let half = |h: bool| if h { "colour" } else { "tex" };
        let dinfo = |di: usize| {
            let Some(d) = combined.get(di) else {
                return "ground-heightfield".to_string();
            };
            format!(
                "draw{di}[res{} @({},{},{}) rot{}]",
                d.res_tmd, d.world_x, d.world_y, d.world_z, d.rot_y
            )
        };
        eprintln!(
            "    {} {} <-> {} {} : {pairs} pairs, {area:.0} overlap area, near ({:.0},{:.0},{:.0}) n=({:.3},{:.3},{:.3})  [res {ra} vs {rb}]",
            dinfo(*da),
            half(*ha),
            dinfo(*db),
            half(*hb),
            at[0],
            at[1],
            at[2],
            n[0],
            n[1],
            n[2],
        );
    }
    (rows.len(), total_area)
}
