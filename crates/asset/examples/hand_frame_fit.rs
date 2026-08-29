//! How does the same weapon sit in two characters' hands?
//!
//! For every weapon id two player files both carry (the `any` weapons:
//! knives, short sword, claws, clubs, axes), cut the item out of each file
//! (`weapon_fuse::weapon_fusion_record`, hand-bone channel) and compare
//! its placement in the two hand-local frames: vertex counts, centroid,
//! principal axis, and - when the meshes are the same vertex list - the
//! rigid (Kabsch) fit taking the donor's placement onto the target's with
//! its residual. Reads `extracted/PROT/086{3,4,5}*.BIN`.
//!
//!     cargo run --release -p legaia-asset --example hand_frame_fit
#![allow(clippy::needless_range_loop)]

use legaia_asset::battle_data_pack;
use legaia_asset::equip_transplant::{section_clut_cols, weapon_section};
use legaia_asset::party_swap::weapon_fuse::{BareFrame, weapon_fusion_record};
use std::collections::BTreeMap;

type V = [f64; 3];

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: V) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V) -> V {
    let n = norm(a).max(1e-9);
    [a[0] / n, a[1] / n, a[2] / n]
}

fn centroid(p: &[V]) -> V {
    let n = p.len().max(1) as f64;
    let mut c = [0.0; 3];
    for v in p {
        for k in 0..3 {
            c[k] += v[k] / n;
        }
    }
    c
}

/// Principal axis by power iteration on the covariance.
fn principal_axis(p: &[V]) -> V {
    let c = centroid(p);
    let mut cov = [[0.0f64; 3]; 3];
    for v in p {
        let d = sub(*v, c);
        for i in 0..3 {
            for j in 0..3 {
                cov[i][j] += d[i] * d[j];
            }
        }
    }
    let mut x = [1.0, 0.7, 0.3];
    for _ in 0..100 {
        let y = [dot(cov[0], x), dot(cov[1], x), dot(cov[2], x)];
        x = unit(y);
    }
    // Sign: point away from the origin (the wrist) along the shaft.
    if dot(x, c) < 0.0 {
        [-x[0], -x[1], -x[2]]
    } else {
        x
    }
}

/// Kabsch: rotation R (row-major) + translation t with `R a + t ~ b`.
/// Returns (R, t, rms residual, angle degrees).
fn kabsch(a: &[V], b: &[V]) -> ([[f64; 3]; 3], V, f64, f64) {
    let ca = centroid(a);
    let cb = centroid(b);
    let mut h = [[0.0f64; 3]; 3];
    for (pa, pb) in a.iter().zip(b) {
        let da = sub(*pa, ca);
        let db = sub(*pb, cb);
        for i in 0..3 {
            for j in 0..3 {
                h[i][j] += da[i] * db[j];
            }
        }
    }
    // SVD of a 3x3 via Jacobi on H^T H (good enough for a report).
    let r = polar_rotation(h);
    let t = sub(cb, [dot(r[0], ca), dot(r[1], ca), dot(r[2], ca)]);
    let mut ss = 0.0;
    for (pa, pb) in a.iter().zip(b) {
        let m = [
            dot(r[0], *pa) + t[0],
            dot(r[1], *pa) + t[1],
            dot(r[2], *pa) + t[2],
        ];
        ss += dot(sub(m, *pb), sub(m, *pb));
    }
    let rms = (ss / a.len().max(1) as f64).sqrt();
    let tr = r[0][0] + r[1][1] + r[2][2];
    let ang = ((tr - 1.0) / 2.0).clamp(-1.0, 1.0).acos().to_degrees();
    (r, t, rms, ang)
}

/// Closest rotation to H^T (the Kabsch optimum) via iterated polar
/// decomposition: R <- (R + R^-T)/2 from R0 = H^T, then det fix.
fn polar_rotation(h: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut r = [
        [h[0][0], h[1][0], h[2][0]],
        [h[0][1], h[1][1], h[2][1]],
        [h[0][2], h[1][2], h[2][2]],
    ];
    for _ in 0..60 {
        let inv_t = match inverse_t(r) {
            Some(m) => m,
            None => break,
        };
        for i in 0..3 {
            for j in 0..3 {
                r[i][j] = 0.5 * (r[i][j] + inv_t[i][j]);
            }
        }
    }
    let d = det(r);
    if d < 0.0 {
        // Reflection: flip the axis of least variance (the third row).
        for j in 0..3 {
            r[2][j] = -r[2][j];
        }
    }
    r
}

fn det(m: [[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn inverse_t(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let d = det(m);
    if d.abs() < 1e-12 {
        return None;
    }
    let c = |i: usize, j: usize| -> f64 {
        let r = [(i + 1) % 3, (i + 2) % 3];
        let s = [(j + 1) % 3, (j + 2) % 3];
        m[r[0]][s[0]] * m[r[1]][s[1]] - m[r[0]][s[1]] * m[r[1]][s[0]]
    };
    // inverse-transpose = cofactor / det
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = c(i, j) / d;
        }
    }
    Some(out)
}

fn euler_zyx(r: [[f64; 3]; 3]) -> (f64, f64, f64) {
    let sy = (-r[2][0]).clamp(-1.0, 1.0);
    let y = sy.asin();
    let x = r[2][1].atan2(r[2][2]);
    let z = r[1][0].atan2(r[0][0]);
    (x.to_degrees(), y.to_degrees(), z.to_degrees())
}

struct File {
    name: &'static str,
    slot: usize,
    bytes: Vec<u8>,
    pack: battle_data_pack::BattleDataPack,
    bare: BareFrame,
    sec: usize,
    cols: Vec<u16>,
}

fn cut(f: &File, id: u32) -> Option<BTreeMap<u8, Vec<V>>> {
    let (per_channel, _) =
        weapon_fusion_record(&f.bytes, &f.pack, &f.bare, f.slot, f.sec, id, &f.cols).ok()??;
    let mut out = BTreeMap::new();
    for (bone, obj) in per_channel {
        // Only vertices a prim references.
        let mut used = vec![false; obj.vertices.len()];
        for g in &obj.groups {
            for p in &g.prims {
                for &v in &p.vertices {
                    used[v as usize] = true;
                }
            }
        }
        let pts: Vec<V> = obj
            .vertices
            .iter()
            .zip(&used)
            .filter(|(_, u)| **u)
            .map(|(v, _)| [v[0] as f64, v[1] as f64, v[2] as f64])
            .collect();
        out.insert(bone, pts);
    }
    Some(out)
}

fn main() {
    let files: Vec<File> = [
        ("Vahn", 0usize, "0863_edstati3.BIN"),
        ("Noa", 1, "0864_edstati3.BIN"),
        ("Gala", 2, "0865_battle_data.BIN"),
    ]
    .into_iter()
    .map(|(name, slot, fname)| {
        let bytes = std::fs::read(format!("extracted/PROT/{fname}")).expect("player file");
        let pack = battle_data_pack::parse(&bytes).expect("parse");
        let bare = BareFrame::new(&bytes, &pack).expect("bare");
        let sec = weapon_section(&pack).expect("weapon section");
        let cols = section_clut_cols(&bytes, &pack, sec).expect("cols");
        File {
            name,
            slot,
            bytes,
            pack,
            bare,
            sec,
            cols,
        }
    })
    .collect();

    // Channels of the Vahn-only blades (what a transplant has to carry).
    for id in [0x1Bu32, 0x25, 0x26, 0x27, 0xBA] {
        if let Some(c) = cut(&files[0], id) {
            let chans: Vec<String> = c
                .iter()
                .map(|(b, p)| format!("bone {b}: {} verts", p.len()))
                .collect();
            println!("Vahn {id:#04x}: {}", chans.join(", "));
        }
    }
    // Frame fit from the shared blades: per file, an orthonormal frame
    // (shaft, width, normal) + grip point per blade; the pair transform is
    // R = F_t F_d^T, t = grip_t - R grip_d, averaged over the training
    // blades, then checked on the held-out one (both roll signs).
    let blades = [0x22u32, 0x23, 0x24];
    for (d, t) in [(0usize, 1usize), (0, 2), (1, 2)] {
        for held in blades {
            let train: Vec<u32> = blades.iter().copied().filter(|&b| b != held).collect();
            let frames = |f: &File, id: u32, flip: bool| -> Option<([[f64; 3]; 3], V)> {
                let c = cut(f, id)?;
                let (_, pts) = c.iter().next_back()?;
                let shaft = principal_axis(pts);
                let cen = centroid(pts);
                // Width: principal axis of the residual after removing the shaft.
                let resid: Vec<V> = pts
                    .iter()
                    .map(|p| {
                        let d = sub(*p, cen);
                        let k = dot(d, shaft);
                        [
                            d[0] - k * shaft[0],
                            d[1] - k * shaft[1],
                            d[2] - k * shaft[2],
                        ]
                    })
                    .collect();
                let mut width = principal_axis(
                    &resid
                        .iter()
                        .map(|r| {
                            [
                                r[0] + 1000.0 * shaft[0],
                                r[1] + 1000.0 * shaft[1],
                                r[2] + 1000.0 * shaft[2],
                            ]
                        })
                        .collect::<Vec<_>>(),
                );
                // remove any shaft component, renormalise
                let k = dot(width, shaft);
                width = unit([
                    width[0] - k * shaft[0],
                    width[1] - k * shaft[1],
                    width[2] - k * shaft[2],
                ]);
                if flip {
                    width = [-width[0], -width[1], -width[2]];
                }
                let normal = [
                    shaft[1] * width[2] - shaft[2] * width[1],
                    shaft[2] * width[0] - shaft[0] * width[2],
                    shaft[0] * width[1] - shaft[1] * width[0],
                ];
                // Grip: the vertex nearest the wrist origin.
                let grip = *pts
                    .iter()
                    .min_by(|a, b| norm(**a).partial_cmp(&norm(**b)).unwrap())?;
                Some(([shaft, width, normal], grip))
            };
            for flip in [false, true] {
                // Average R over training blades (as matrices; fine for near-identical rotations).
                let mut racc = [[0.0f64; 3]; 3];
                let mut tacc = [0.0f64; 3];
                let mut n = 0.0;
                for &b in &train {
                    let (Some((fd, gd)), Some((ft, gt))) =
                        (frames(&files[d], b, false), frames(&files[t], b, flip))
                    else {
                        continue;
                    };
                    // R = Ft^T-as-columns * Fd-as-rows : R = sum_k ft_k ⊗ fd_k
                    let mut r = [[0.0; 3]; 3];
                    for k in 0..3 {
                        for i in 0..3 {
                            for j in 0..3 {
                                r[i][j] += ft[k][i] * fd[k][j];
                            }
                        }
                    }
                    let rg = [dot(r[0], gd), dot(r[1], gd), dot(r[2], gd)];
                    for i in 0..3 {
                        for j in 0..3 {
                            racc[i][j] += r[i][j];
                        }
                        tacc[i] += gt[i] - rg[i];
                    }
                    n += 1.0;
                }
                if n == 0.0 {
                    continue;
                }
                for i in 0..3 {
                    for j in 0..3 {
                        racc[i][j] /= n;
                    }
                    tacc[i] /= n;
                }
                let r = polar_rotation([
                    [racc[0][0], racc[1][0], racc[2][0]],
                    [racc[0][1], racc[1][1], racc[2][1]],
                    [racc[0][2], racc[1][2], racc[2][2]],
                ]);
                // Apply to the donor's held-out blade; compare to the target's own.
                let (Some(cd), Some(ct)) = (cut(&files[d], held), cut(&files[t], held)) else {
                    continue;
                };
                let pd = cd.iter().next_back().unwrap().1;
                let pt = ct.iter().next_back().unwrap().1;
                let moved: Vec<V> = pd
                    .iter()
                    .map(|p| {
                        [
                            dot(r[0], *p) + tacc[0],
                            dot(r[1], *p) + tacc[1],
                            dot(r[2], *p) + tacc[2],
                        ]
                    })
                    .collect();
                let (ex, ey, ez) = euler_zyx(r);
                let axm = principal_axis(&moved);
                let axt = principal_axis(pt);
                let cm = centroid(&moved);
                let ctt = centroid(pt);
                // Nearest-point residual (meshes differ in vertex count).
                let mut acc = 0.0;
                for m in &moved {
                    let best = pt
                        .iter()
                        .map(|q| norm(sub(*m, *q)))
                        .fold(f64::MAX, f64::min);
                    acc += best * best;
                }
                println!(
                    "  {}->{} held-out {held:#04x} flip={flip}: R xyz ({:+6.1},{:+6.1},{:+6.1}) t ({:+6.1},{:+6.1},{:+6.1}) | axis dot {:+.3} centroid gap {:5.1} nearest-pt rms {:5.1}",
                    files[d].name,
                    files[t].name,
                    ex,
                    ey,
                    ez,
                    tacc[0],
                    tacc[1],
                    tacc[2],
                    dot(axm, axt),
                    norm(sub(cm, ctt)),
                    (acc / moved.len() as f64).sqrt()
                );
            }
        }
    }
    let ids: Vec<u32> = (0x22..=0x24).collect();
    for id in ids {
        let cuts: Vec<Option<BTreeMap<u8, Vec<V>>>> = files.iter().map(|f| cut(f, id)).collect();
        let have: Vec<usize> = (0..3).filter(|&i| cuts[i].is_some()).collect();
        if have.len() < 2 {
            continue;
        }
        println!("== weapon {id:#04x}");
        for &i in &have {
            let c = cuts[i].as_ref().unwrap();
            for (bone, pts) in c {
                let cen = centroid(pts);
                let ax = principal_axis(pts);
                println!(
                    "  {:<4} bone {:>2}: {:>4} verts  centroid ({:7.1},{:7.1},{:7.1})  axis ({:+.2},{:+.2},{:+.2})  reach {:6.1}",
                    files[i].name,
                    bone,
                    pts.len(),
                    cen[0],
                    cen[1],
                    cen[2],
                    ax[0],
                    ax[1],
                    ax[2],
                    norm(cen)
                );
            }
        }
        // Pairwise fit on the hand channel (last bone of the section).
        for a in 0..have.len() {
            for b in (a + 1)..have.len() {
                let (fa, fb) = (have[a], have[b]);
                let (ca, cb) = (cuts[fa].as_ref().unwrap(), cuts[fb].as_ref().unwrap());
                let (ha, hb) = (
                    ca.iter().next_back().unwrap(),
                    cb.iter().next_back().unwrap(),
                );
                if ha.1.len() != hb.1.len() || ha.1.len() < 4 {
                    println!(
                        "  {}->{}: hand meshes differ ({} vs {} verts) - no direct fit",
                        files[fa].name,
                        files[fb].name,
                        ha.1.len(),
                        hb.1.len()
                    );
                    continue;
                }
                let (r, t, rms, ang) = kabsch(ha.1, hb.1);
                let (ex, ey, ez) = euler_zyx(r);
                println!(
                    "  {}->{}: rigid fit rms {:5.1}  rotation {:5.1} deg (xyz {:+6.1} {:+6.1} {:+6.1})  t ({:+6.1},{:+6.1},{:+6.1})",
                    files[fa].name, files[fb].name, rms, ang, ex, ey, ez, t[0], t[1], t[2]
                );
            }
        }
    }
}
