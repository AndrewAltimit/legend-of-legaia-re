//! Hand-frame calibration between two player battle files: the rigid
//! transform that takes a held item authored in one character's arm-bone
//! frames onto the other's.
//!
//! A held-item record re-authors the three arm bones (upper arm, forearm,
//! hand) in each bone's **own local frame**, and the three skeletons do not
//! share those frames: the same Short Sword runs along `-Y` in Vahn's hand
//! frame, `-Z` in Noa's and `(0, +0.5, +0.85)` in Gala's, and the wrist
//! origins sit differently along the shaft. A transplant that copies the
//! donor's coordinates verbatim therefore hands the new owner a blade
//! rotated by up to a right angle (`examples/hand_frame_fit.rs` is the
//! measurement).
//!
//! The calibration set is the disc itself: every weapon both files carry
//! (the knives, Short Sword, claws, clubs and axes are `any`-owner items,
//! each file holding its own record authored for its own hand). Per
//! section channel `k` (the `k`th attached bone - the same physical bone
//! on every file), each shared weapon is cut out of both files
//! ([`weapon_fuse::weapon_fusion_record`]) and fitted:
//!
//! - rotation from the item's principal frame in each file (shaft axis,
//!   then the widest axis across it), the roll sign about the shaft being
//!   ambiguous on a thin blade and undefined on a round club - so each
//!   weapon offers two candidates and the set picks the sign by consensus,
//!   seeded by the weapon whose two candidates differ most in residual;
//! - translation by aligning the shaft's far tip (the welded Vahn grips
//!   reach into the fist where a separate Noa grip stops, so the wrist
//!   end is not comparable; the tip of the same weapon is), refined by a
//!   translation-only ICP against the target's own placement.
//!
//! The per-weapon transforms are averaged (rotation through the polar
//! factor of the mean matrix) into one transform per channel, with the
//! mean nearest-point residual as its quality. A channel no shared weapon
//! occupies in both files gets no fit, and a transplant drops what it
//! would have carried there rather than guess.
//!
//! One transform per channel is not one transform per hand: a character
//! holds different **classes** of weapon differently. Noa's clubs run along
//! `(+0.77, +0.33, +0.54)` in her hand frame while her blades run along
//! `-Z` - a club is swung, a blade is pointed - so a calibration pooled
//! over every class averages two grips and lands a sword between them.
//! [`fit_for`] therefore calibrates on the shared weapons of the
//! transplanted item's own [`WeaponClass`] (blades from the knives and
//! Short Sword, claws from the claws, clubs and axes from the clubs and
//! axes), and pools every class only when the class has too few shared
//! weapons on a channel to vote.

#![allow(clippy::needless_range_loop, clippy::type_complexity)]

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use legaia_tmd::encode::ModelObject;

use crate::battle_data_pack::BattleDataPack;
use crate::equip_transplant::{find_weapon_record, section_clut_cols, weapon_section};
use crate::party_swap::weapon_fuse::{BareFrame, weapon_fusion_record};

/// 3-vector.
pub type V3 = [f64; 3];
/// Row-major 3x3.
pub type Mat3 = [[f64; 3]; 3];

/// Item ids the calibration draws on: the held-item range shared across
/// files (Ra-Seru forms below it are living arms, not items).
pub const CALIBRATION_IDS: std::ops::RangeInclusive<u32> = 0x22..=0x33;

/// How a held weapon is gripped - the calibration pool a transplant draws
/// from. Ids outside the three retail runs (the Astral Sword, the Ra-Seru
/// Blade) are classed by what they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponClass {
    /// Knives and swords: `0x22..=0x27`, the Ra-Seru Blade `0x1B`, the
    /// Astral Sword `0xBA`.
    Blade,
    /// Gloves and claws: `0x28..=0x2D`.
    Claw,
    /// Clubs, maces and axes: `0x2E..=0x33`, Gala's Mace `0x20` / Ra-Seru
    /// Club `0x21`.
    Club,
}

impl WeaponClass {
    pub fn of(id: u32) -> Option<WeaponClass> {
        match id {
            0x1B | 0x22..=0x27 | 0xBA => Some(WeaponClass::Blade),
            0x1C..=0x1F | 0x28..=0x2D => Some(WeaponClass::Claw),
            0x20 | 0x21 | 0x2E..=0x33 => Some(WeaponClass::Club),
            _ => None,
        }
    }
}

/// Fewer shared weapons of the item's class than this on a channel, and
/// the calibration pools every class instead.
const MIN_CLASS_VOTES: usize = 2;

/// ICP refinement passes for the translation.
const ICP_ITERS: usize = 12;
/// A calibration weapon farther than this from the consensus rotation
/// does not vote in the final average.
const OUTLIER_DEG: f64 = 25.0;

/// `R v + t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rigid {
    pub r: Mat3,
    pub t: V3,
}

impl Rigid {
    pub const IDENTITY: Rigid = Rigid {
        r: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        t: [0.0; 3],
    };

    pub fn apply(&self, v: V3) -> V3 {
        [
            dot(self.r[0], v) + self.t[0],
            dot(self.r[1], v) + self.t[1],
            dot(self.r[2], v) + self.t[2],
        ]
    }

    /// Rotation angle in degrees.
    pub fn angle_deg(&self) -> f64 {
        let tr = self.r[0][0] + self.r[1][1] + self.r[2][2];
        ((tr - 1.0) / 2.0).clamp(-1.0, 1.0).acos().to_degrees()
    }

    /// Move every vertex of `obj` (i16 GTE space, rounded and clamped).
    pub fn apply_object(&self, obj: &mut ModelObject) {
        for v in &mut obj.vertices {
            let p = self.apply([v[0] as f64, v[1] as f64, v[2] as f64]);
            for k in 0..3 {
                v[k] = p[k].round().clamp(i16::MIN as f64, i16::MAX as f64) as i16;
            }
        }
    }
}

/// One channel's calibrated transform.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelFit {
    pub xf: Rigid,
    /// Mean nearest-point residual of the calibration weapons after the
    /// averaged transform, in GTE units.
    pub rms: f64,
    /// The weapons that contributed.
    pub weapons: Vec<u32>,
}

/// The donor -> target calibration for a held section.
#[derive(Debug, Clone, PartialEq)]
pub struct HandFrameFit {
    /// Donor section bones in channel order.
    pub donor_bones: Vec<u8>,
    /// Target section bones in channel order.
    pub target_bones: Vec<u8>,
    /// Per channel `k`, the fit or `None` when nothing calibrates it.
    pub channels: Vec<Option<ChannelFit>>,
}

impl HandFrameFit {
    /// The fit for donor bone `bone`, if calibrated.
    pub fn for_donor_bone(&self, bone: u8) -> Option<&ChannelFit> {
        let k = self.donor_bones.iter().position(|b| *b == bone)?;
        self.channels.get(k)?.as_ref()
    }
}

/// A file opened for cutting.
struct Side<'a> {
    file: &'a [u8],
    pack: &'a BattleDataPack,
    slot: usize,
    bare: BareFrame,
    sec: usize,
    cols: Vec<u16>,
    bones: Vec<u8>,
}

impl<'a> Side<'a> {
    fn open(file: &'a [u8], pack: &'a BattleDataPack, slot: usize) -> Result<Self> {
        let sec = weapon_section(pack).context("file has no weapon section")?;
        let cols = section_clut_cols(file, pack, sec)?;
        let bare = BareFrame::new(file, pack).context("bare assembly")?;
        let bones = crate::equip_transplant::section_bones(file, pack, sec)?;
        Ok(Side {
            file,
            pack,
            slot,
            bare,
            sec,
            cols,
            bones,
        })
    }

    /// The item cut's referenced vertices per channel index.
    fn cut(&self, id: u32) -> Option<BTreeMap<usize, Vec<V3>>> {
        find_weapon_record(self.pack, id)?;
        let (per_channel, _) = weapon_fusion_record(
            self.file, self.pack, &self.bare, self.slot, self.sec, id, &self.cols,
        )
        .ok()??;
        let mut out = BTreeMap::new();
        for (bone, obj) in per_channel {
            let k = self.bones.iter().position(|b| *b == bone)?;
            let mut used = vec![false; obj.vertices.len()];
            for g in &obj.groups {
                for p in &g.prims {
                    for &v in &p.vertices {
                        if let Some(u) = used.get_mut(v as usize) {
                            *u = true;
                        }
                    }
                }
            }
            let pts: Vec<V3> = obj
                .vertices
                .iter()
                .zip(&used)
                .filter(|(_, u)| **u)
                .map(|(v, _)| [v[0] as f64, v[1] as f64, v[2] as f64])
                .collect();
            if pts.len() >= 4 {
                out.insert(k, pts);
            }
        }
        Some(out)
    }
}

/// Calibrate `donor -> target` over every shared weapon in
/// [`CALIBRATION_IDS`].
pub fn fit(
    donor_file: &[u8],
    donor_pack: &BattleDataPack,
    donor_slot: usize,
    target_file: &[u8],
    target_pack: &BattleDataPack,
    target_slot: usize,
) -> Result<HandFrameFit> {
    fit_excluding(
        donor_file,
        donor_pack,
        donor_slot,
        target_file,
        target_pack,
        target_slot,
        &[],
    )
}

/// Calibrate `donor -> target` for carrying `item_id` over: the shared
/// weapons of its class, every class where the class is too thin.
pub fn fit_for(
    donor_file: &[u8],
    donor_pack: &BattleDataPack,
    donor_slot: usize,
    target_file: &[u8],
    target_pack: &BattleDataPack,
    target_slot: usize,
    item_id: u32,
) -> Result<HandFrameFit> {
    fit_class_excluding(
        donor_file,
        donor_pack,
        donor_slot,
        target_file,
        target_pack,
        target_slot,
        WeaponClass::of(item_id),
        &[],
    )
}

/// [`fit`] without the weapons in `exclude` (leave-one-out validation).
pub fn fit_excluding(
    donor_file: &[u8],
    donor_pack: &BattleDataPack,
    donor_slot: usize,
    target_file: &[u8],
    target_pack: &BattleDataPack,
    target_slot: usize,
    exclude: &[u32],
) -> Result<HandFrameFit> {
    fit_class_excluding(
        donor_file,
        donor_pack,
        donor_slot,
        target_file,
        target_pack,
        target_slot,
        None,
        exclude,
    )
}

/// The general form: `class` restricts the calibration pool (pooled when
/// `None`, or when the class is too thin on a channel); `exclude` holds
/// weapons out.
#[allow(clippy::too_many_arguments)]
pub fn fit_class_excluding(
    donor_file: &[u8],
    donor_pack: &BattleDataPack,
    donor_slot: usize,
    target_file: &[u8],
    target_pack: &BattleDataPack,
    target_slot: usize,
    class: Option<WeaponClass>,
    exclude: &[u32],
) -> Result<HandFrameFit> {
    let d = Side::open(donor_file, donor_pack, donor_slot).context("donor")?;
    let t = Side::open(target_file, target_pack, target_slot).context("target")?;
    if d.bones.len() != t.bones.len() {
        bail!(
            "donor section attaches {} bones, target {} - not the same arm",
            d.bones.len(),
            t.bones.len()
        );
    }
    // Per channel: (weapon, the two roll candidates with their residuals).
    let mut per_channel: Vec<Vec<(u32, [(Rigid, f64); 2])>> = vec![Vec::new(); d.bones.len()];
    for id in CALIBRATION_IDS {
        if exclude.contains(&id) {
            continue;
        }
        let (Some(cd), Some(ct)) = (d.cut(id), t.cut(id)) else {
            continue;
        };
        for (k, pd) in &cd {
            let Some(pt) = ct.get(k) else { continue };
            if let Some(cands) = fit_pair(pd, pt) {
                per_channel[*k].push((id, cands));
            }
        }
    }
    let channels = per_channel
        .into_iter()
        .map(|cands| {
            if let Some(class) = class {
                let mine: Vec<_> = cands
                    .iter()
                    .filter(|(id, _)| WeaponClass::of(*id) == Some(class))
                    .cloned()
                    .collect();
                if mine.len() >= MIN_CLASS_VOTES {
                    return consensus(&mine);
                }
            }
            consensus(&cands)
        })
        .collect();
    Ok(HandFrameFit {
        donor_bones: d.bones,
        target_bones: t.bones,
        channels,
    })
}

/// Pick one candidate per weapon by consensus, average, and measure.
///
/// Every seed (each weapon, either sign) proposes an assignment: each
/// weapon takes the candidate nearest the seed, the mean rotation is
/// re-derived and the assignment re-picked until stable. The assignment
/// with the tightest rotation spread wins; weapons farther than
/// [`OUTLIER_DEG`] from its mean (round clubs with no roll, cuts that
/// differ in what they claim) are dropped before the final average.
fn consensus(cands: &[(u32, [(Rigid, f64); 2])]) -> Option<ChannelFit> {
    if cands.is_empty() {
        return None;
    }
    let mean_of = |picks: &[usize]| -> Mat3 {
        let mut m = [[0.0f64; 3]; 3];
        for (c, &pick) in cands.iter().zip(picks) {
            for i in 0..3 {
                for j in 0..3 {
                    m[i][j] += c.1[pick].0.r[i][j];
                }
            }
        }
        nearest_rotation(m)
    };
    let pick_nearest = |r: Mat3| -> Vec<usize> {
        cands
            .iter()
            .map(|c| {
                if rotation_gap(c.1[0].0.r, r) <= rotation_gap(c.1[1].0.r, r) {
                    0
                } else {
                    1
                }
            })
            .collect()
    };
    let spread = |picks: &[usize], r: Mat3| -> f64 {
        cands
            .iter()
            .zip(picks)
            .map(|(c, &p)| rotation_gap(c.1[p].0.r, r))
            .sum::<f64>()
            / cands.len() as f64
    };
    let mut best: Option<(f64, Vec<usize>, Mat3)> = None;
    for seed in cands {
        for sign in 0..2 {
            let mut picks = pick_nearest(seed.1[sign].0.r);
            let mut r = mean_of(&picks);
            for _ in 0..6 {
                let next = pick_nearest(r);
                if next == picks {
                    break;
                }
                picks = next;
                r = mean_of(&picks);
            }
            let sp = spread(&picks, r);
            if best.as_ref().is_none_or(|b| sp < b.0) {
                best = Some((sp, picks, r));
            }
        }
    }
    let (_, picks, mut r) = best?;
    // Outliers out, then the final average over what agrees.
    let mut keep: Vec<usize> = (0..cands.len())
        .filter(|&i| rotation_gap(cands[i].1[picks[i]].0.r, r).to_degrees() <= OUTLIER_DEG)
        .collect();
    if keep.len() < 2 {
        keep = (0..cands.len()).collect();
    }
    let mut m = [[0.0f64; 3]; 3];
    let mut t_sum = [0.0f64; 3];
    let mut rms_sum = 0.0;
    let mut weapons = Vec::new();
    for &i in &keep {
        let (xf, rms) = cands[i].1[picks[i]];
        for a in 0..3 {
            for b in 0..3 {
                m[a][b] += xf.r[a][b];
            }
            t_sum[a] += xf.t[a];
        }
        rms_sum += rms;
        weapons.push(cands[i].0);
    }
    let n = keep.len() as f64;
    r = nearest_rotation(m);
    for v in t_sum.iter_mut() {
        *v /= n;
    }
    Some(ChannelFit {
        xf: Rigid { r, t: t_sum },
        rms: rms_sum / n,
        weapons,
    })
}

/// Angle (radians) between two rotations.
fn rotation_gap(a: Mat3, b: Mat3) -> f64 {
    // trace(a^T b)
    let mut tr = 0.0;
    for i in 0..3 {
        for j in 0..3 {
            tr += a[i][j] * b[i][j];
        }
    }
    ((tr - 1.0) / 2.0).clamp(-1.0, 1.0).acos()
}

/// The two roll-sign candidates taking `d` (donor placement) onto `t`
/// (target placement of the same weapon), each `(transform, residual)`.
fn fit_pair(d: &[V3], t: &[V3]) -> Option<[(Rigid, f64); 2]> {
    let fd = frame_of(d)?;
    let mut out = Vec::with_capacity(2);
    for flip in [false, true] {
        let mut ft = frame_of(t)?;
        if flip {
            ft.width = neg(ft.width);
            ft.normal = neg(ft.normal);
        }
        // R = sum_k ft_k (x) fd_k  maps fd axes onto ft axes.
        let mut r = [[0.0f64; 3]; 3];
        for (a, b) in [
            (fd.shaft, ft.shaft),
            (fd.width, ft.width),
            (fd.normal, ft.normal),
        ] {
            for i in 0..3 {
                for j in 0..3 {
                    r[i][j] += b[i] * a[j];
                }
            }
        }
        let r = nearest_rotation(r);
        let mut xf = Rigid { r, t: [0.0; 3] };
        // Tip alignment, then translation-only ICP.
        let tip_d = xf.apply(fd.tip);
        xf.t = sub(ft.tip, tip_d);
        let mut rms = 0.0;
        for _ in 0..ICP_ITERS {
            let moved: Vec<V3> = d.iter().map(|p| xf.apply(*p)).collect();
            let mut shift = [0.0f64; 3];
            let mut ss = 0.0;
            for m in &moved {
                let q = nearest(*m, t);
                let e = sub(q, *m);
                for k in 0..3 {
                    shift[k] += e[k] / moved.len() as f64;
                }
                ss += dot(e, e);
            }
            rms = (ss / moved.len() as f64).sqrt();
            for k in 0..3 {
                xf.t[k] += shift[k];
            }
        }
        out.push((xf, rms));
    }
    Some([out[0], out[1]])
}

/// Principal frame of an item cut: shaft (away from the wrist origin),
/// width (widest direction across the shaft), normal, and the shaft's far
/// tip.
struct Frame {
    shaft: V3,
    width: V3,
    normal: V3,
    tip: V3,
}

fn frame_of(p: &[V3]) -> Option<Frame> {
    if p.len() < 4 {
        return None;
    }
    let c = centroid(p);
    let mut shaft = principal_axis(p, c)?;
    if dot(shaft, c) < 0.0 {
        shaft = neg(shaft);
    }
    // Residual across the shaft.
    let resid: Vec<V3> = p
        .iter()
        .map(|v| {
            let d = sub(*v, c);
            let k = dot(d, shaft);
            [
                d[0] - k * shaft[0],
                d[1] - k * shaft[1],
                d[2] - k * shaft[2],
            ]
        })
        .collect();
    let mut width = principal_axis(&resid, [0.0; 3]).unwrap_or_else(|| any_perpendicular(shaft));
    let k = dot(width, shaft);
    width = unit([
        width[0] - k * shaft[0],
        width[1] - k * shaft[1],
        width[2] - k * shaft[2],
    ]);
    if norm(width) < 0.5 {
        width = any_perpendicular(shaft);
    }
    let normal = cross(shaft, width);
    // Far tip: the vertex of greatest projection on the shaft, averaged
    // with its near neighbours so one stray vertex does not steer.
    let mut proj: Vec<(f64, V3)> = p.iter().map(|v| (dot(*v, shaft), *v)).collect();
    proj.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let take = (p.len() / 10).clamp(1, 6);
    let tip = centroid(&proj[..take].iter().map(|x| x.1).collect::<Vec<_>>());
    Some(Frame {
        shaft,
        width,
        normal,
        tip,
    })
}

fn nearest(m: V3, t: &[V3]) -> V3 {
    let mut best = t[0];
    let mut bd = f64::MAX;
    for q in t {
        let d = norm(sub(*q, m));
        if d < bd {
            bd = d;
            best = *q;
        }
    }
    best
}

/// Principal axis of `p` about `c` (power iteration).
fn principal_axis(p: &[V3], c: V3) -> Option<V3> {
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
    for _ in 0..200 {
        let y = [dot(cov[0], x), dot(cov[1], x), dot(cov[2], x)];
        let n = norm(y);
        if n < 1e-9 {
            return None;
        }
        x = [y[0] / n, y[1] / n, y[2] / n];
    }
    Some(x)
}

fn any_perpendicular(a: V3) -> V3 {
    let cand = if a[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    unit(cross(a, cand))
}

/// Closest rotation to `m` (polar factor), reflection resolved.
pub fn nearest_rotation(m: Mat3) -> Mat3 {
    let mut r = m;
    for _ in 0..80 {
        let Some(inv_t) = inverse_transpose(r) else {
            break;
        };
        for i in 0..3 {
            for j in 0..3 {
                r[i][j] = 0.5 * (r[i][j] + inv_t[i][j]);
            }
        }
    }
    if det(r) < 0.0 {
        for j in 0..3 {
            r[2][j] = -r[2][j];
        }
    }
    r
}

fn det(m: Mat3) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn inverse_transpose(m: Mat3) -> Option<Mat3> {
    let d = det(m);
    if d.abs() < 1e-12 {
        return None;
    }
    let cof = |i: usize, j: usize| -> f64 {
        let r = [(i + 1) % 3, (i + 2) % 3];
        let s = [(j + 1) % 3, (j + 2) % 3];
        m[r[0]][s[0]] * m[r[1]][s[1]] - m[r[0]][s[1]] * m[r[1]][s[0]]
    };
    let mut out = [[0.0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = cof(i, j) / d;
        }
    }
    Some(out)
}

pub fn centroid(p: &[V3]) -> V3 {
    let n = p.len().max(1) as f64;
    let mut c = [0.0; 3];
    for v in p {
        for k in 0..3 {
            c[k] += v[k] / n;
        }
    }
    c
}

pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn neg(a: V3) -> V3 {
    [-a[0], -a[1], -a[2]]
}
fn unit(a: V3) -> V3 {
    let n = norm(a).max(1e-9);
    [a[0] / n, a[1] / n, a[2] / n]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Nearest-point RMS of `moved` against `target` (what a fit reports).
pub fn nearest_rms(moved: &[V3], target: &[V3]) -> f64 {
    if moved.is_empty() || target.is_empty() {
        return f64::NAN;
    }
    let ss: f64 = moved
        .iter()
        .map(|m| {
            let e = sub(nearest(*m, target), *m);
            dot(e, e)
        })
        .sum();
    (ss / moved.len() as f64).sqrt()
}

/// Principal axis of a point set (unit, pointed away from the origin) -
/// the shaft direction a test compares.
pub fn shaft_axis(p: &[V3]) -> Option<V3> {
    frame_of(p).map(|f| f.shaft)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rot_z(deg: f64) -> Mat3 {
        let (s, c) = deg.to_radians().sin_cos();
        [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
    }

    /// A synthetic blade: a thin box along +X with a wider guard.
    fn blade() -> Vec<V3> {
        let mut v = Vec::new();
        for i in 0..12 {
            let x = 10.0 + i as f64 * 6.0;
            for (y, z) in [(-1.0, -3.0), (1.0, -3.0), (-1.0, 3.0), (1.0, 3.0)] {
                v.push([x, y, z]);
            }
        }
        for (y, z) in [(-2.0, -9.0), (2.0, -9.0), (-2.0, 9.0), (2.0, 9.0)] {
            v.push([12.0, y, z]);
        }
        v
    }

    #[test]
    fn fit_pair_recovers_a_known_rigid_move() {
        let d = blade();
        let xf = Rigid {
            r: rot_z(70.0),
            t: [5.0, -12.0, 3.0],
        };
        let t: Vec<V3> = d.iter().map(|p| xf.apply(*p)).collect();
        let cands = fit_pair(&d, &t).unwrap();
        let best = cands
            .iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap();
        assert!(best.1 < 0.5, "residual {}", best.1);
        assert!(rotation_gap(best.0.r, xf.r).to_degrees() < 1.0);
        for k in 0..3 {
            assert!((best.0.t[k] - xf.t[k]).abs() < 0.5, "t {:?}", best.0.t);
        }
    }

    #[test]
    fn consensus_prefers_the_seed_roll_sign() {
        let d = blade();
        let xf = Rigid {
            r: rot_z(-40.0),
            t: [0.0, 8.0, 0.0],
        };
        let t: Vec<V3> = d.iter().map(|p| xf.apply(*p)).collect();
        let c = fit_pair(&d, &t).unwrap();
        // A second "weapon": the same blade, so the same candidates.
        let fit = consensus(&[(0x22, c), (0x23, c)]).unwrap();
        assert!(fit.rms < 0.5, "{}", fit.rms);
        assert_eq!(fit.weapons, vec![0x22, 0x23]);
        assert!(rotation_gap(fit.xf.r, xf.r).to_degrees() < 1.0);
    }

    #[test]
    fn nearest_rotation_projects_a_scaled_rotation() {
        let r = rot_z(33.0);
        let mut m = r;
        for row in m.iter_mut() {
            for v in row.iter_mut() {
                *v *= 2.5;
            }
        }
        let back = nearest_rotation(m);
        assert!(rotation_gap(back, r).to_degrees() < 1e-6);
    }
}
