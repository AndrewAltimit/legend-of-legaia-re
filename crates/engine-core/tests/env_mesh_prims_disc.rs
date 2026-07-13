//! Disc-gated: every field/town scene's environment-mesh pack decodes with all
//! of its primitives intact.
//!
//! The env packs are the one place the *light-source-lit* textured descriptor
//! rows (rows 0/1 of `DAT_8007326C`, group flags `0x10..=0x17`) appear in bulk:
//! a town's houses are lit meshes, each prim laid out as `[12-byte texture
//! block][vertex indices][normal indices]`. Their **quad** vertex-index offset
//! is the one place the two retail renderers disagree, and `FUN_8002735c`'s
//! `byte4 + 2` fallback lands the read inside the trailing normal block: the
//! indices then exceed the object's vertex count, the mesh builders drop those
//! prims, and the house renders as a shredded pile (Rim Elm object 137 lost 105
//! of its 163 prims).
//!
//! "Indices are in range" is too weak an invariant on its own - a wrong offset
//! that happens to land on small normal indices still decodes - so this also
//! scores the offset geometrically: an authored quad is planar (the PSX draws
//! it as two triangles, so a handful are genuinely bent, but the population is
//! flat), and a misread offset scrambles the vertices and blows the
//! out-of-plane distance up by two orders of magnitude. The decoded offset is
//! checked against every alternative the prim layout could support, so this
//! stays honest without hard-coding the answer.
//!
//! Skips when `LEGAIA_DISC_BIN` is unset (disc-gated convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::field_env;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};
use legaia_tmd::descriptor::Descriptor;
use legaia_tmd::legaia_prims;

/// Candidate vertex-index byte offsets for a lit textured quad. `12` is what
/// the descriptor resolves; the others are where the retail renderers' quad
/// arithmetic would land (`FUN_80029888` -> 14 for row 0, `FUN_8002735c` ->
/// 16 / 18).
const CANDIDATE_OFFSETS: [usize; 4] = [12, 14, 16, 18];

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Largest distance of any vertex past the third from the plane through the
/// first three. `0` for a degenerate (collinear) face - no plane to measure.
fn out_of_plane(vs: &[[f64; 3]]) -> f64 {
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let e1 = sub(vs[1], vs[0]);
    let e2 = sub(vs[2], vs[0]);
    let n = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len < 1e-6 {
        return 0.0;
    }
    vs[3..]
        .iter()
        .map(|&v| {
            let d = sub(v, vs[0]);
            ((n[0] * d[0] + n[1] * d[1] + n[2] * d[2]) / len).abs()
        })
        .fold(0.0f64, f64::max)
}

/// Per-candidate-offset score over the lit-quad population.
#[derive(Default, Clone, Copy)]
struct Score {
    out_of_range: usize,
    planar_sum: f64,
    planar_n: usize,
}

impl Score {
    fn mean_out_of_plane(&self) -> f64 {
        if self.planar_n == 0 {
            f64::INFINITY
        } else {
            self.planar_sum / self.planar_n as f64
        }
    }
}

#[test]
fn env_pack_prims_decode_in_range_and_planar() {
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
    let system_ui = index.system_ui_bundle().ok();

    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut scene_names: Vec<String> = cdname.values().cloned().collect();
    scene_names.sort();
    scene_names.dedup();

    let mut scenes = 0usize;
    let mut prims = 0usize;
    let mut oob_prims = 0usize;
    let mut lit_quads = 0usize;
    let mut plane_err: Vec<f64> = Vec::new();
    let mut scores = [Score::default(); CANDIDATE_OFFSETS.len()];

    for name in &scene_names {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        let Ok((res, _)) = SceneResources::build_targeted_with_options(
            &scene,
            &shared_refs,
            BuildOptions {
                kind: SceneLoadKind::Field,
                upload_all_tims: true,
                system_ui: system_ui.as_deref(),
            },
        ) else {
            continue;
        };
        let env_tmds = field_env::env_pack_tmd_indices(&scene, &res);
        if env_tmds.is_empty() {
            continue;
        }
        scenes += 1;

        for (slot, &ti) in env_tmds.iter().enumerate() {
            let t = &res.tmds[ti];
            for (oi, o) in t.tmd.objects.iter().enumerate() {
                let vertex_at = |raw: u16| -> Option<[f64; 3]> {
                    let v = o.vertices.get(usize::from(raw / 8))?;
                    Some([f64::from(v.x), f64::from(v.y), f64::from(v.z)])
                };
                let groups = legaia_prims::iter_groups_lenient(
                    &t.raw,
                    o.primitives_byte_offset,
                    o.primitives_byte_size,
                );
                for g in &groups {
                    let Some(d) = Descriptor::for_flags(g.header.flags) else {
                        continue;
                    };
                    let stride = g.header.prim_stride();
                    // Rows 0/1 are the light-source-lit textured rows.
                    let lit_quad = d.packet_shape.is_textured() && d.table_row <= 1 && d.is_quad;

                    for p in &g.prims {
                        prims += 1;
                        let idx = p.vertex_indices();
                        if idx.is_empty() {
                            continue;
                        }
                        if idx.iter().any(|&i| u32::from(i) >= o.header.n_vert) {
                            oob_prims += 1;
                            eprintln!(
                                "[env-mesh] out-of-range prim: {name} slot {slot} obj {oi} \
                                 flags {:#06x} idx {idx:?} (n_vert {})",
                                g.header.flags, o.header.n_vert
                            );
                            continue;
                        }
                        if !lit_quad {
                            continue;
                        }
                        lit_quads += 1;

                        // Score the decoded offset against every alternative.
                        for (score, &off) in scores.iter_mut().zip(CANDIDATE_OFFSETS.iter()) {
                            let vs: Option<Vec<[f64; 3]>> = (off + 8 <= stride)
                                .then(|| {
                                    (0..4)
                                        .map(|k| {
                                            let a = p.bytes_offset + off + k * 2;
                                            let raw = u16::from_le_bytes([t.raw[a], t.raw[a + 1]]);
                                            vertex_at(raw)
                                        })
                                        .collect()
                                })
                                .flatten();
                            match vs {
                                Some(vs) => {
                                    score.planar_sum += out_of_plane(&vs);
                                    score.planar_n += 1;
                                }
                                None => score.out_of_range += 1,
                            }
                        }
                        let vs: Vec<[f64; 3]> =
                            idx.iter().filter_map(|&i| vertex_at(i * 8)).collect();
                        plane_err.push(out_of_plane(&vs));
                    }
                }
            }
        }
    }

    plane_err.sort_by(|a, b| a.total_cmp(b));
    let pct = |q: f64| plane_err[((plane_err.len() - 1) as f64 * q) as usize];
    eprintln!("[env-mesh] {scenes} scenes, {prims} prims, {lit_quads} lit textured quads");
    eprintln!(
        "[env-mesh] lit-quad out-of-plane: median {:.2} p90 {:.2} max {:.2}",
        pct(0.5),
        pct(0.9),
        pct(1.0)
    );
    for (score, off) in scores.iter().zip(CANDIDATE_OFFSETS.iter()) {
        eprintln!(
            "[env-mesh] candidate vertex offset {off:2}: {} out-of-range, mean out-of-plane {:.2}",
            score.out_of_range,
            score.mean_out_of_plane()
        );
    }

    assert!(scenes > 50, "only {scenes} scenes had an env pack");
    assert!(
        lit_quads > 1000,
        "only {lit_quads} lit textured quads swept - the row-0/1 quad offset is barely covered"
    );
    assert_eq!(
        oob_prims, 0,
        "{oob_prims} env-pack prims reference out-of-range vertex indices - a per-row \
         vertex-index offset is reading the normal-index block"
    );

    // The decoded offset is CANDIDATE_OFFSETS[0]; it must beat every rival
    // outright: no out-of-range reads, and a flat population where the others
    // scramble the quad.
    let chosen = scores[0];
    assert_eq!(chosen.out_of_range, 0);
    for (score, off) in scores.iter().zip(CANDIDATE_OFFSETS.iter()).skip(1) {
        assert!(
            chosen.mean_out_of_plane() * 10.0 < score.mean_out_of_plane(),
            "lit-quad vertex offset {} is not decisively flatter than the alternative {off} \
             ({:.2} vs {:.2} mean out-of-plane) - the row-0/1 quad offset needs re-pinning",
            CANDIDATE_OFFSETS[0],
            chosen.mean_out_of_plane(),
            score.mean_out_of_plane()
        );
    }
}
