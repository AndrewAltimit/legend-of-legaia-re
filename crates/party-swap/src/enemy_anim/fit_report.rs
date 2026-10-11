//! The diagnostic affine-fit report: per-part least-squares fit of the swapped
//! mesh against the monster's, with the small matrix kernels it needs.

use super::super::*;
use super::bake_ctx::monster_bake_ctx;
use winpose::{mmul, transpose};

// ---------------------------------------------------------------------------
// Bake-parity instrument: per-part exact affine fit.

/// Per-part affine fit of the enemy-side bake, `source geometry posed at
/// the (normalized) player rest` → `baked geometry posed at the monster
/// rest`, polar-decomposed. Gap/centroid metrics are structurally blind
/// to per-part roll; this is the instrument that sees it.
#[derive(Debug, Clone)]
pub struct PartAffineFit {
    pub part: usize,
    /// Whether the part is a chain terminal (head / hand / foot).
    pub terminal: bool,
    /// Rotation angle (degrees) between the fitted rotation and the
    /// whole-rig ideal (body re-face + minimal swing onto the target
    /// bone), computed from the rest PIVOTS alone - independent of the
    /// bake's own frame machinery. For terminals the ideal is the
    /// authored world orientation carried over unchanged, measured
    /// against the RAW (un-normalized) rest.
    pub excess_deg: f32,
    /// Principal scales of the fitted linear map, descending.
    pub principal_scales: [f32; 3],
    /// RMS non-affine residual over the part's RMS radius.
    pub residual: f32,
}

pub(super) fn mat_angle_deg(m: &[[f32; 3]; 3]) -> f32 {
    let tr = (m[0][0] + m[1][1] + m[2][2]).clamp(-1.0, 3.0);
    (((tr - 1.0) / 2.0).clamp(-1.0, 1.0)).acos().to_degrees()
}

/// 3x3 symmetric Jacobi eigenvalues (descending).
fn sym_eigenvalues(a: [[f32; 3]; 3]) -> [f32; 3] {
    let mut m = a;
    for _ in 0..32 {
        // Largest off-diagonal.
        let (mut p, mut q, mut big) = (0usize, 1usize, m[0][1].abs());
        for &(i, j) in &[(0usize, 2usize), (1, 2)] {
            if m[i][j].abs() > big {
                big = m[i][j].abs();
                p = i;
                q = j;
            }
        }
        if big < 1e-7 {
            break;
        }
        let theta = 0.5 * (2.0 * m[p][q]).atan2(m[q][q] - m[p][p] + 1e-20);
        let (s, c) = theta.sin_cos();
        let mut r = [[0.0f32; 3]; 3];
        for (i, row) in r.iter_mut().enumerate() {
            row[i] = 1.0;
        }
        r[p][p] = c;
        r[q][q] = c;
        r[p][q] = s;
        r[q][p] = -s;
        // m = r^T m r
        let mut t = [[0.0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                t[i][j] = (0..3).map(|k| r[k][i] * m[k][j]).sum();
            }
        }
        let mut m2 = [[0.0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                m2[i][j] = (0..3).map(|k| t[i][k] * r[k][j]).sum();
            }
        }
        m = m2;
    }
    let mut ev = [m[0][0], m[1][1], m[2][2]];
    ev.sort_by(|a, b| b.total_cmp(a));
    ev
}

fn mat3_inv(m: [[f32; 3]; 3]) -> Option<[[f32; 3]; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv_det = 1.0 / det;
    let mut inv = [[0.0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let (a, b, c, d) = (
                m[(j + 1) % 3][(i + 1) % 3],
                m[(j + 2) % 3][(i + 2) % 3],
                m[(j + 1) % 3][(i + 2) % 3],
                m[(j + 2) % 3][(i + 1) % 3],
            );
            inv[i][j] = (a * b - c * d) * inv_det;
        }
    }
    Some(inv)
}

/// Least-squares affine fit `w_dst ≈ A * w_src + b`, then polar
/// decomposition of `A` (Higham iteration).
pub(super) fn affine_fit(
    src: &[[f32; 3]],
    dst: &[[f32; 3]],
) -> Option<([[f32; 3]; 3], f32, [f32; 3])> {
    let n = src.len().min(dst.len());
    if n < 4 {
        return None;
    }
    let mean = |pts: &[[f32; 3]]| {
        let mut m = [0.0f32; 3];
        for p in pts.iter().take(n) {
            for k in 0..3 {
                m[k] += p[k];
            }
        }
        [m[0] / n as f32, m[1] / n as f32, m[2] / n as f32]
    };
    let (ms, md) = (mean(src), mean(dst));
    let mut cov_ds = [[0.0f32; 3]; 3]; // dst x src
    let mut cov_ss = [[0.0f32; 3]; 3];
    for i in 0..n {
        let s = vsub(src[i], ms);
        let d = vsub(dst[i], md);
        for r in 0..3 {
            for c in 0..3 {
                cov_ds[r][c] += d[r] * s[c];
                cov_ss[r][c] += s[r] * s[c];
            }
        }
    }
    // Regularize a flat part.
    let trace = cov_ss[0][0] + cov_ss[1][1] + cov_ss[2][2];
    for (k, row) in cov_ss.iter_mut().enumerate() {
        row[k] += trace * 1e-6 + 1e-6;
    }
    let a = mmul(&cov_ds, &mat3_inv(cov_ss)?);
    // Residual.
    let mut se = 0.0f32;
    let mut rad = 0.0f32;
    for i in 0..n {
        let s = vsub(src[i], ms);
        let d = vsub(dst[i], md);
        let p = apply(&a, s);
        let e = vsub(d, p);
        se += vdot(e, e);
        rad += vdot(d, d);
    }
    let residual = (se / n as f32).sqrt() / (rad / n as f32).sqrt().max(1.0);
    // Polar rotation via Higham: R <- (R + R^-T)/2.
    let mut r = a;
    for _ in 0..24 {
        let Some(rinv) = mat3_inv(r) else { break };
        let rinv_t = transpose(&rinv);
        let mut next = [[0.0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                next[i][j] = 0.5 * (r[i][j] + rinv_t[i][j]);
            }
        }
        r = next;
    }
    // Principal scales: sqrt of eigenvalues of A^T A.
    let ata = mmul(&transpose(&a), &a);
    let ev = sym_eigenvalues(ata);
    let scales = [
        ev[0].max(0.0).sqrt(),
        ev[1].max(0.0).sqrt(),
        ev[2].max(0.0).sqrt(),
    ];
    Some((r, residual, scales))
}

/// The whole-rig ideal rotation for canonical part `c`: read the source
/// bone in the source body frame, rebuild it in the destination body
/// frame (re-face), then swing minimally onto the destination bone.
/// Pure function of the rest pivots - shares no code with the bake's
/// frame machinery beyond the two public axis helpers.
fn ideal_rotation(
    src_pivots: &[[f32; 3]],
    dst_pivots: &[[f32; 3]],
    c: usize,
) -> Option<[[f32; 3]; 3]> {
    let axes = |pivots: &[[f32; 3]]| -> [[f32; 3]; 3] {
        let unit = |v: [f32; 3], f: [f32; 3]| {
            let l = vnorm(v);
            if l < 1e-3 {
                f
            } else {
                [v[0] / l, v[1] / l, v[2] / l]
            }
        };
        let up = unit(vsub(pivots[0], pivots[2]), [0.0, -1.0, 0.0]);
        let lat = vsub(pivots[6], pivots[3]);
        let d = vdot(lat, up);
        let lat = unit(
            [lat[0] - up[0] * d, lat[1] - up[1] * d, lat[2] - up[2] * d],
            [0.0, 0.0, 1.0],
        );
        [up, lat, unit(vcross(up, lat), [1.0, 0.0, 0.0])]
    };
    let (bs, bd) = (axes(src_pivots), axes(dst_pivots));
    // R_reface: world -> world, source body frame onto destination's.
    let mut reface = [[0.0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            reface[i][j] = (0..3).map(|k| bd[k][i] * bs[k][j]).sum();
        }
    }
    let child = CANONICAL_CHILD[c]?;
    let bone = |pivots: &[[f32; 3]], from: usize, to: usize| -> Option<[f32; 3]> {
        let b = vsub(pivots[to], pivots[from]);
        let l = vnorm(b);
        (l >= 2.0).then(|| [b[0] / l, b[1] / l, b[2] / l])
    };
    let xs = bone(src_pivots, c, child)?;
    let xd = bone(dst_pivots, c, child)?;
    let refaced = apply(&reface, xs);
    // Rodrigues swing refaced -> xd.
    let v = vcross(refaced, xd);
    let cos = vdot(refaced, xd).clamp(-1.0, 1.0);
    let s = vnorm(v);
    let ident = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    if s < 1e-6 {
        return (cos > 0.0).then_some(reface);
    }
    let k = [v[0] / s, v[1] / s, v[2] / s];
    let kx = [[0.0, -k[2], k[1]], [k[2], 0.0, -k[0]], [-k[1], k[0], 0.0]];
    let (st, ct) = s.atan2(cos).sin_cos();
    let mut swing = ident;
    for i in 0..3 {
        for j in 0..3 {
            let kk: f32 = (0..3).map(|q| kx[i][q] * kx[q][j]).sum();
            swing[i][j] = ident[i][j] + st * kx[i][j] + (1.0 - ct) * kk;
        }
    }
    Some(mmul(&swing, &reface))
}

/// Run the enemy-side bake and fit each part's produced geometry against
/// its source, reporting excess rotation / principal scales / residual.
/// Disc-gated tests assert bounds on this - the instrument that catches
/// the roll-defect class the gap metrics were blind to.
pub fn monsterize_fit_report(
    player_file: &[u8],
    rig: &PlayerRig,
    archive_entry: &[u8],
    target_id: u16,
) -> Result<Vec<PartAffineFit>> {
    let ctx = monster_bake_ctx(player_file, rig, archive_entry, target_id)?;
    let mut out = Vec::with_capacity(CANONICAL_PARTS);
    #[allow(clippy::needless_range_loop)]
    for c in 0..CANONICAL_PARTS {
        let ch = rig.channel_for_canonical[c] as usize;
        let terminal = CANONICAL_CHILD[c].is_none();
        // Source world: pre-bake geometry posed at the player rest. For
        // non-terminals the normalized and raw rests agree; terminals are
        // measured against the RAW rest so the fit checks the authored
        // orientation really is carried over.
        let pose = if terminal {
            ctx.rest_raw[ch]
        } else {
            ctx.rest[ch]
        };
        let ms = rot_matrix(&pose);
        let src_world: Vec<[f32; 3]> = ctx.objects[c]
            .vertices
            .iter()
            .map(|v| {
                let w = apply(&ms, [v[0] as f32, v[1] as f32, v[2] as f32]);
                [
                    w[0] + pose.tx as f32,
                    w[1] + pose.ty as f32,
                    w[2] + pose.tz as f32,
                ]
            })
            .collect();
        // Baked world: run the bake on a clone, pose at the monster rest.
        let mut baked = ctx.objects[c].clone();
        let pb = pivot_bake_params(&ctx.src_frames[c], &ctx.dst_frames[c], ctx.radial);
        bake_object_pivot(
            &mut baked,
            &ctx.rest[ch],
            ctx.src_pivots[c],
            &ctx.target_rest[c],
            &pb,
        )?;
        let md = rot_matrix(&ctx.target_rest[c]);
        let tp = &ctx.target_rest[c];
        let dst_world: Vec<[f32; 3]> = baked
            .vertices
            .iter()
            .map(|v| {
                let w = apply(&md, [v[0] as f32, v[1] as f32, v[2] as f32]);
                [
                    w[0] + tp.tx as f32,
                    w[1] + tp.ty as f32,
                    w[2] + tp.tz as f32,
                ]
            })
            .collect();
        let Some((r_fit, residual, scales)) = affine_fit(&src_world, &dst_world) else {
            continue;
        };
        let ideal = if terminal {
            // Normalization carries the authored world orientation over
            // unchanged: the ideal is the identity.
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        } else {
            match ideal_rotation(&ctx.src_pivots, &ctx.dst_pivots, c) {
                Some(m) => m,
                None => continue,
            }
        };
        let excess = mmul(&r_fit, &transpose(&ideal));
        out.push(PartAffineFit {
            part: c,
            terminal,
            excess_deg: mat_angle_deg(&excess),
            principal_scales: scales,
            residual,
        });
    }
    Ok(out)
}
