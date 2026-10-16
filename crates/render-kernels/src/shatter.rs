//! The shatter renderer: a body whose draw word `+0x7A` is non-zero breaks
//! into its own triangles, each spun about its centroid and pushed away
//! from a burst centre - `FUN_80029888` (per object) and `FUN_80029DD8`
//! (per primitive), `docs/subsystems/renderer.md`, "The shatter renderer".
//!
//! Retail reaches it from the field object draw (`FUN_8001B964`, the op
//! `43 07` deformation) and the battle body draw (`FUN_80048A08`, which
//! copies the actor's `+0x1BA` into the node's `+0x7A`). Both hand it
//! `a3 = (row[+8] << 16) | +0x7A`, `row` the 12-byte record at
//! `0x8007BE60 + node[+0x6D] * 12` ([`ShatterRow`]).
//!
//! The kernel works in object-local integer space the way the GTE does:
//! [`ShatterObject::triangle`] takes one triangle's local vertices and
//! returns where they draw, still in the object's local frame, so a host
//! applies its own object pose after it. The mesh helpers
//! ([`shatter_local_triangles`], [`shatter_vram_mesh`],
//! [`shatter_color_mesh`]) run it over a whole local mesh and de-index the
//! result: neighbouring triangles share no vertex once they come apart.
//!
//! What it leaves to the host: the packet colours (the shatter path runs
//! the same `DPCS` far-colour cue as the plain dispatcher, which the hosts'
//! per-draw cue already carries), and the backface rule - the shatter path
//! issues no `NCLIP`, so a shattering body draws both sides
//! ([`SHATTER_NCLIP`]).

use crate::battle_intro::euler_rot_psx;
use crate::gte::GteMat3;

/// Flag `0x10000`: no spin - `F` is the identity (`0x80029928`).
pub const FLAG_NO_SPIN: u32 = 0x0001_0000;
/// Flag `0x20000`: no outward push - the triangles turn in place
/// (`0x80029FD4`).
pub const FLAG_NO_PUSH: u32 = 0x0002_0000;
/// Flag `0x40000`: shrink each triangle by `0x1000 - v` (`0x80029960`).
pub const FLAG_SHRINK: u32 = 0x0004_0000;
/// Flag `0x80000`: sweep along view Y.
pub const FLAG_SWEEP_Y: u32 = 0x0008_0000;
/// Flag `0x100000`: sweep along view X.
pub const FLAG_SWEEP_X: u32 = 0x0010_0000;
/// Flag `0x200000`: sweep along view Z.
pub const FLAG_SWEEP_Z: u32 = 0x0020_0000;
/// The flags that leave the default arm (`lui v0,0x3a` at `0x80029F6C`).
const NON_DEFAULT: u32 = FLAG_NO_PUSH | FLAG_SWEEP_X | FLAG_SWEEP_Y | FLAG_SWEEP_Z;
/// The three sweep flags (`lui v0,0x38` at `0x80029FCC`).
const SWEEP: u32 = FLAG_SWEEP_X | FLAG_SWEEP_Y | FLAG_SWEEP_Z;

/// The NCLIP word (`Renderer::set_backface_cull` encoding) a shattering
/// body draws under: `FUN_80029DD8` runs `RTPT` and emits the packet with no
/// `NCLIP` between, so both sides of every triangle draw.
pub const SHATTER_NCLIP: u32 = 0;

/// One row of the deformation table at `0x8007BE60` (12-byte stride): the
/// three halfwords of the burst offset `off`, the sweep front halfword `h3`
/// and the flag word. The field's op `43 07` writes it; the battle rows read
/// zero in every capture.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShatterRow {
    /// `+0..+4` - added to every vertex (`0x80029B3C..0x80029B88`); `-off`
    /// is the burst centre.
    pub off: [i16; 3],
    /// `+6` - the sweep front's start (`lh v0,0x46(s2)` at `0x8002A070`).
    pub h3: i16,
    /// `+8` - the flag word, shifted into the draw word's high half.
    pub flags: u32,
}

/// The draw word both callers build: `(int)(short)+0x7A | row[+8] << 16`
/// (`lh v0,0x7a(s0)` / `sll a3,a3,0x10` / `or` at `0x80048FB8..0x80048FC0`).
/// A negative `+0x7A` sign-extends over the flag half, as retail's does.
pub fn shatter_word(value: i16, row: &ShatterRow) -> u32 {
    (value as i32 as u32) | (row.flags << 16)
}

/// How the sweep arms measure a triangle against the front: retail runs
/// `MVMVA` on the centroid through the object's own GTE matrix - rotation
/// `rot`, translation `tr` (the object's translation before the burst
/// offset is folded in) - and subtracts the node's view translation
/// `node_t` (`+0x2C..+0x34`, staged at `0x1F8003BC`). A host without the
/// retail view matrix passes [`SweepFrame::MODEL`], which measures along the
/// object's own axes from its origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepFrame {
    /// The object's GTE rotation `M`.
    pub rot: GteMat3,
    /// The object's GTE translation `T`.
    pub tr: [i32; 3],
    /// The node's view translation `+0x2C..+0x34`.
    pub node_t: [i32; 3],
}

impl SweepFrame {
    /// The object's own axes from its origin.
    pub const MODEL: Self = Self {
        rot: GteMat3::IDENTITY,
        tr: [0; 3],
        node_t: [0; 3],
    };
}

/// `ScaleMatrix` (`FUN_8005B4E8`): every column `c` times `s[c]`, `>> 12`,
/// kept to a halfword.
fn scale_matrix(m: &GteMat3, s: i32) -> GteMat3 {
    let mut out = *m;
    for row in out.m.iter_mut() {
        for e in row.iter_mut() {
            *e = (i32::from(*e).wrapping_mul(s) >> 12) as i16;
        }
    }
    out
}

/// `MVMVA` with `sf = 1` over `TR` (`cop2 0x480012`): `TR + (R * v) >> 12`
/// in the 32-bit `MAC` registers.
fn mvmva(rot: &GteMat3, tr: [i32; 3], v: [i16; 3]) -> [i32; 3] {
    std::array::from_fn(|r| {
        let sum: i64 = (0..3)
            .map(|c| i64::from(rot.m[r][c]) * i64::from(v[c]))
            .sum();
        (((i64::from(tr[r]) << 12) + sum) >> 12) as i32
    })
}

/// One object's shatter state: what `FUN_80029888` prepares once per object
/// and what `FUN_80029DD8` carries from one primitive to the next in the
/// scratchpad matrix `0x1F800314`.
#[derive(Debug, Clone)]
pub struct ShatterObject {
    word: u32,
    off: [i16; 3],
    h3: i16,
    /// The scratchpad matrix `0x1F800314`'s rotation half. The default and
    /// no-push arms reuse what the prologue built; the sweep arms rebuild it
    /// per primitive, and a past-the-front primitive under
    /// [`FLAG_NO_SPIN`] scales whatever the previous one left there.
    cur: GteMat3,
    sweep: SweepFrame,
}

impl ShatterObject {
    /// `FUN_80029888`'s prologue (`0x80029888..0x80029990`): `F =
    /// RotMatrix(v << 4, 0, v << 4)` (the angle kept to a halfword, the
    /// identity under [`FLAG_NO_SPIN`]), scaled by `0x1000 - v` under
    /// [`FLAG_SHRINK`].
    ///
    /// PORT: FUN_80029888
    pub fn new(word: u32, row: &ShatterRow, sweep: SweepFrame) -> Self {
        let angle = if word & FLAG_NO_SPIN != 0 {
            0
        } else {
            (word << 4) as u16 as i16
        };
        let mut f = euler_rot_psx((angle, 0, angle));
        if word & FLAG_SHRINK != 0 {
            f = scale_matrix(&f, 0x1000 - (word & 0xFFFF) as i32);
        }
        Self {
            word,
            off: row.off,
            h3: row.h3,
            cur: f,
            sweep,
        }
    }

    /// The burst offset this object adds to its vertices.
    pub fn off(&self) -> [i16; 3] {
        self.off
    }

    /// One triangle through `FUN_80029DD8`: `verts` are the object-local
    /// vertices, the result is where they draw in the same frame.
    ///
    /// The vertices move by `off` first (`lhu` / `addu` / `sh`, kept to a
    /// halfword); the centroid `c` is their sum `* 0x555 >> 12`; then:
    ///
    /// - default arm: `TR = c + (c * v) >> 7` with the prologue's `F`;
    /// - [`FLAG_NO_PUSH`] alone: `TR = c`;
    /// - a sweep flag: the centroid is measured along the view axis against
    ///   the front at `h3 - v` (`h3 + v` when `h3 <= 0`); a triangle past it
    ///   is pushed and spun by its own distance `d` past the front, the rest
    ///   draw intact.
    ///
    /// Each vertex is then `TR + F * (p - c) >> 12` (`MVMVA` at
    /// `0x8002A208` / `0x8002A274`), stored to a halfword, and handed back
    /// less `off` - the frame the object's translation `T - M * off` draws
    /// it in.
    ///
    /// PORT: FUN_80029DD8
    pub fn triangle(&mut self, verts: [[i16; 3]; 3]) -> [[i32; 3]; 3] {
        let off = self.off;
        let p: [[i16; 3]; 3] =
            std::array::from_fn(|i| std::array::from_fn(|a| verts[i][a].wrapping_add(off[a])));
        let c: [i32; 3] = std::array::from_fn(|a| {
            let s = i32::from(p[0][a]) + i32::from(p[1][a]) + i32::from(p[2][a]);
            s.wrapping_mul(0x555) >> 12
        });
        let word = self.word;
        let v = (word & 0xFFFF) as i32;
        let tr = if word & NON_DEFAULT == 0 {
            push(c, v)
        } else if word & SWEEP == 0 {
            c
        } else {
            self.sweep_arm(c, v)
        };
        std::array::from_fn(|i| {
            let sv: [i16; 3] = std::array::from_fn(|a| p[i][a].wrapping_sub(c[a] as i16));
            let out = mvmva(&self.cur, tr, sv);
            std::array::from_fn(|a| i32::from(out[a] as i16) - i32::from(off[a]))
        })
    }

    /// The sweep arms (`0x80029FF0..0x8002A1B8`): measure, then either come
    /// apart by `d` or draw intact with the identity.
    fn sweep_arm(&mut self, c: [i32; 3], v: i32) -> [i32; 3] {
        let sw = self.sweep;
        // The object's translation with the burst offset folded in
        // (`T - M * off`, `0x800298E4..0x8002991C`), then the centroid
        // through it (`FUN_8003D344` at `0x8002A000`).
        let m_off = mvmva(&sw.rot, [0; 3], self.off);
        let tr_obj: [i32; 3] = std::array::from_fn(|a| sw.tr[a].wrapping_sub(m_off[a]));
        let cv = mvmva(&sw.rot, tr_obj, c.map(|x| x as i16));
        let mut proj = 0i32;
        let word = self.word;
        if word & FLAG_SWEEP_X != 0 {
            proj = cv[0].wrapping_sub(sw.node_t[0]);
        }
        if word & FLAG_SWEEP_Y != 0 {
            proj = cv[1].wrapping_sub(sw.node_t[1]);
        }
        if word & FLAG_SWEEP_Z != 0 {
            proj = cv[2].wrapping_sub(sw.node_t[2]);
        }
        let h3 = i32::from(self.h3);
        let (past, d) = if h3 > 0 {
            let front = h3 - v;
            (front < proj, front.wrapping_sub(proj))
        } else {
            let front = h3 + v;
            (proj < front, proj.wrapping_sub(front))
        };
        let d = d.wrapping_abs();
        if !past {
            self.cur = euler_rot_psx((0, 0, 0));
            return c;
        }
        let tr = push(c, d);
        if word & FLAG_NO_SPIN == 0 {
            self.cur = euler_rot_psx(((d << 4) as i16, (d << 2) as i16, (d << 4) as i16));
        }
        if word & FLAG_SHRINK != 0 {
            let s = (0x1000 - d * 8).max(0);
            self.cur = scale_matrix(&self.cur, s);
        }
        tr
    }
}

/// `c + (c * k) >> 7` per lane - the outward push (`mult` / `mflo` keeps the
/// low word of the product, then `sra 7`).
fn push(c: [i32; 3], k: i32) -> [i32; 3] {
    std::array::from_fn(|a| c[a].wrapping_add(c[a].wrapping_mul(k) >> 7))
}

/// What one shattering body draws with: its draw word, its row and the
/// sweep frame for each object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShatterInput {
    /// [`shatter_word`]: `+0x7A` in the low half, the row's flags above.
    pub word: u32,
    /// The deformation-table row the node's `+0x6D` names.
    pub row: ShatterRow,
}

impl ShatterInput {
    /// The input for draw value `value` (`+0x7A`) over `row`; `None` when
    /// the value is zero, which keeps the body on the plain dispatcher.
    pub fn new(value: i16, row: ShatterRow) -> Option<Self> {
        (value != 0).then(|| Self {
            word: shatter_word(value, &row),
            row,
        })
    }
}

/// Run every triangle of a local mesh through the shatter, in index order:
/// `positions` are object-local, `object_ids` names each vertex's object, and
/// a new [`ShatterObject`] starts whenever a triangle's object differs from
/// the previous one's (retail calls `FUN_80029888` once per object). Returns
/// one position per index - the de-indexed, shattered local corners - so
/// corner `k` keeps the attributes of vertex `indices[k]`.
pub fn shatter_local_triangles(
    positions: &[[f32; 3]],
    indices: &[u32],
    object_ids: &[u32],
    input: &ShatterInput,
    sweep_of: impl Fn(u32) -> SweepFrame,
) -> Vec<[f32; 3]> {
    let mut out = Vec::with_capacity(indices.len());
    let mut obj: Option<(u32, ShatterObject)> = None;
    let q = |p: [f32; 3]| -> [i16; 3] { p.map(|x| x.round().clamp(-32768.0, 32767.0) as i16) };
    for tri in indices.as_chunks::<3>().0 {
        let at = |i: u32| positions.get(i as usize).copied().unwrap_or([0.0; 3]);
        let id = object_ids.get(tri[0] as usize).copied().unwrap_or(0);
        if obj.as_ref().is_none_or(|(o, _)| *o != id) {
            obj = Some((id, ShatterObject::new(input.word, &input.row, sweep_of(id))));
        }
        let (_, so) = obj.as_mut().expect("seeded above");
        let t = so.triangle([q(at(tri[0])), q(at(tri[1])), q(at(tri[2]))]);
        out.extend(t.iter().map(|v| v.map(|x| x as f32)));
    }
    out
}

/// One object's pose for [`shatter_vram_mesh`] / [`shatter_color_mesh`]:
/// `(translation, rotation)` as the posed mesh builders take it.
pub type BonePose = ([i16; 3], [i16; 3]);

/// Pose one local corner of object `o` the way the posed builders do:
/// `R * v + T` (`rotate`) or `v + T`; an object past the end of `bones` stays
/// at its local position.
fn pose_corner(p: [f32; 3], o: u32, bones: &[BonePose], rotate: bool) -> [f32; 3] {
    const A2R: f32 = std::f32::consts::TAU / 4096.0;
    let Some((t, r)) = bones.get(o as usize) else {
        return p;
    };
    let q = if rotate {
        let (sx, cx) = (f32::from(r[0]) * A2R).sin_cos();
        let (sy, cy) = (f32::from(r[1]) * A2R).sin_cos();
        let (sz, cz) = (f32::from(r[2]) * A2R).sin_cos();
        legaia_tmd::mesh::rot_zyx(p, cx, sx, cy, sy, cz, sz)
    } else {
        p
    };
    [
        q[0] + f32::from(t[0]),
        q[1] + f32::from(t[1]),
        q[2] + f32::from(t[2]),
    ]
}

/// The body-to-view rotation [`bone_sweep_frame`] takes for a frame measured
/// along the body's own axes.
pub const BODY_AXES: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// The sweep frame of object `o` posed by `bones`, measured the way retail
/// measures it: the centroid through the object's own GTE matrix less the
/// node's view translation (`0x8002A000`), i.e. the triangle's offset from
/// the body root along a **view** axis. `view` is the body-to-view linear
/// map retail's object matrix carries above the object's pose - the battle
/// world scale, the camera rotation and the body's facing
/// (`legaia_engine_core::World::battle_shatter_view`), or [`BODY_AXES`] to
/// measure along the body's own axes. The pose is the posed builders'
/// `Rz * Ry * Rx` (the identity without `rotate`) and its translation; an
/// object past the end of `bones` is measured unposed.
pub fn bone_sweep_frame(
    bones: &[BonePose],
    o: u32,
    rotate: bool,
    view: &[[f32; 3]; 3],
) -> SweepFrame {
    const A2R: f32 = std::f32::consts::TAU / 4096.0;
    let (t, r) = bones.get(o as usize).copied().unwrap_or(([0; 3], [0; 3]));
    let pose = |e: [f32; 3]| -> [f32; 3] {
        if !rotate {
            return e;
        }
        let (sx, cx) = (f32::from(r[0]) * A2R).sin_cos();
        let (sy, cy) = (f32::from(r[1]) * A2R).sin_cos();
        let (sz, cz) = (f32::from(r[2]) * A2R).sin_cos();
        legaia_tmd::mesh::rot_zyx(e, cx, sx, cy, sy, cz, sz)
    };
    let apply = |v: [f32; 3]| -> [f32; 3] {
        std::array::from_fn(|row| (0..3).map(|k| view[row][k] * v[k]).sum())
    };
    let q = |x: f32| x.round().clamp(-32768.0, 32767.0);
    // Column `c` is the image of axis `c` under `view * pose`.
    let mut m = [[0i16; 3]; 3];
    for c in 0..3 {
        let mut e = [0.0f32; 3];
        e[c] = 1.0;
        for (row, x) in apply(pose(e)).iter().enumerate() {
            m[row][c] = q(x * 4096.0) as i16;
        }
    }
    let tr = apply(t.map(f32::from));
    SweepFrame {
        rot: GteMat3 { m },
        tr: tr.map(|x| q(x) as i32),
        node_t: [0; 3],
    }
}

/// The shattered, posed, de-indexed copy of a local textured mesh (built by
/// `legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids`): every triangle
/// through [`shatter_local_triangles`], then its object's pose. Each corner
/// keeps its source vertex's UV, CBA/TSB and colour. Normals are left at the
/// zero sentinel - a shard has no smooth neighbour. `sweep_of` is each
/// object's sweep frame: [`bone_sweep_frame`] measures it in its pose,
/// [`SweepFrame::MODEL`] along its own local axes.
pub fn shatter_vram_mesh(
    local: &legaia_tmd::mesh::VramMesh,
    object_ids: &[u32],
    input: &ShatterInput,
    bones: &[BonePose],
    rotate: bool,
    sweep_of: impl Fn(u32) -> SweepFrame,
) -> legaia_tmd::mesh::VramMesh {
    let pos = shatter_local_triangles(
        &local.positions,
        &local.indices,
        object_ids,
        input,
        sweep_of,
    );
    let pick = |k: usize| local.indices[k] as usize;
    let n = local.indices.len();
    legaia_tmd::mesh::VramMesh {
        positions: (0..n)
            .map(|k| {
                let o = object_ids.get(pick(k)).copied().unwrap_or(0);
                pose_corner(pos[k], o, bones, rotate)
            })
            .collect(),
        uvs: (0..n).map(|k| local.uvs[pick(k)]).collect(),
        cba_tsb: (0..n).map(|k| local.cba_tsb[pick(k)]).collect(),
        colors: (0..n).map(|k| local.colors[pick(k)]).collect(),
        normals: vec![[0.0; 3]; n],
        indices: (0..n as u32).collect(),
    }
}

/// [`shatter_vram_mesh`] for the untextured half
/// (`legaia_tmd::mesh::tmd_to_color_mesh_with_object_ids`).
pub fn shatter_color_mesh(
    local: &legaia_tmd::mesh::ColorMesh,
    object_ids: &[u32],
    input: &ShatterInput,
    bones: &[BonePose],
    rotate: bool,
    sweep_of: impl Fn(u32) -> SweepFrame,
) -> legaia_tmd::mesh::ColorMesh {
    let pos = shatter_local_triangles(
        &local.positions,
        &local.indices,
        object_ids,
        input,
        sweep_of,
    );
    let pick = |k: usize| local.indices[k] as usize;
    let n = local.indices.len();
    legaia_tmd::mesh::ColorMesh {
        positions: (0..n)
            .map(|k| {
                let o = object_ids.get(pick(k)).copied().unwrap_or(0);
                pose_corner(pos[k], o, bones, rotate)
            })
            .collect(),
        colors: (0..n).map(|k| local.colors[pick(k)]).collect(),
        blend: (0..n).map(|k| local.blend[pick(k)]).collect(),
        indices: (0..n as u32).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRI: [[i16; 3]; 3] = [[300, 0, 0], [0, 300, 0], [0, 0, 300]];

    fn row(flags: u32) -> ShatterRow {
        ShatterRow {
            off: [0; 3],
            h3: 0,
            flags,
        }
    }

    /// The draw word: `+0x7A` low, the row's flags shifted up; a negative
    /// value sign-extends over them.
    #[test]
    fn word_packs_value_and_flags() {
        assert_eq!(shatter_word(0x123, &row(4)), 0x0004_0123);
        assert_eq!(shatter_word(-1, &row(4)), 0xFFFF_FFFF);
    }

    /// No spin, value 128: centroid `(900 * 0x555) >> 12 = 299 / 3` -
    /// `409500 >> 12 = 99`; `TR = 99 + (99 * 128 >> 7) = 198`; the first
    /// corner is `TR + (300 - 99, -99, -99)`.
    #[test]
    fn default_arm_pushes_by_v_over_128() {
        let mut o = ShatterObject::new(shatter_word(128, &row(1)), &row(1), SweepFrame::MODEL);
        let t = o.triangle(TRI);
        assert_eq!(t[0], [399, 99, 99]);
        assert_eq!(t[1], [99, 399, 99]);
        assert_eq!(t[2], [99, 99, 399]);
    }

    /// Shrink and no spin at `0x800`: `F` is the identity halved, `TR = 99 +
    /// (99 * 0x800 >> 7) = 1683`, corner 0's x `1683 + (0x800 * 201 >> 12) =
    /// 1783` and its y / z `1683 + (0x800 * -99 >> 12) = 1683 - 50`.
    #[test]
    fn shrink_scales_about_the_centroid() {
        let r = row(5);
        let mut o = ShatterObject::new(shatter_word(0x800, &r), &r, SweepFrame::MODEL);
        let t = o.triangle(TRI);
        assert_eq!(t[0], [1783, 1633, 1633]);
    }

    /// No push (`0x20000`) with no spin: every corner stays put.
    #[test]
    fn no_push_without_spin_is_the_identity() {
        let r = row(3);
        let mut o = ShatterObject::new(shatter_word(0x400, &r), &r, SweepFrame::MODEL);
        assert_eq!(o.triangle(TRI), TRI.map(|v| v.map(i32::from)));
    }

    /// The spin is `RotMatrix(v << 4, 0, v << 4)` about the centroid, so a
    /// full turn (`v << 4 == 0x1000`, `v = 0x100`) with no push leaves the
    /// triangle where it was, and the offset only moves the frame.
    #[test]
    fn spin_is_about_the_centroid_and_off_cancels() {
        let r = ShatterRow {
            off: [10, -32, 64],
            h3: 0,
            flags: 2,
        };
        let mut o = ShatterObject::new(shatter_word(0x100, &r), &r, SweepFrame::MODEL);
        let t = o.triangle(TRI);
        for (got, want) in t.iter().zip(TRI) {
            for a in 0..3 {
                assert!(
                    (got[a] - i32::from(want[a])).abs() <= 1,
                    "{got:?} vs {want:?}"
                );
            }
        }
    }

    /// The push is away from `-off`: with `off` moving the body so the
    /// centroid lands on the origin, nothing is pushed.
    #[test]
    fn burst_centre_is_minus_off() {
        let r = ShatterRow {
            off: [-100, -100, -100],
            h3: 0,
            flags: 1,
        };
        let mut o = ShatterObject::new(shatter_word(0x400, &r), &r, SweepFrame::MODEL);
        // The centroid of (vertex + off) is the origin, so the push is zero.
        assert_eq!(o.triangle(TRI), TRI.map(|v| v.map(i32::from)));
    }

    /// A Y sweep with the front at `h3 - v`: a triangle whose centroid sits
    /// above the front (model frame) draws intact; one past it comes apart.
    #[test]
    fn sweep_front_splits_intact_and_past() {
        let r = ShatterRow {
            off: [0; 3],
            h3: 200,
            flags: (FLAG_SWEEP_Y | FLAG_NO_SPIN) >> 16,
        };
        let w = shatter_word(50, &r);
        let mut o = ShatterObject::new(w, &r, SweepFrame::MODEL);
        // Front at 150: centroid y 99 is not past it.
        assert_eq!(o.triangle(TRI), TRI.map(|v| v.map(i32::from)));
        // A triangle around y = 400 (centroid 399): past the front by
        // d = 249 and pushed out by 249 / 128 of its distance.
        let low = [[300, 400, 0], [0, 400, 0], [0, 400, 300]];
        let t = o.triangle(low);
        assert_eq!(t[1][1], 399 + ((399 * 249) >> 7) + 1);
    }

    /// The mesh helper de-indexes: a quad split `[0,1,2, 1,3,2]` comes back
    /// as six corners and each keeps its source attributes.
    #[test]
    fn mesh_helper_de_indexes() {
        let local = legaia_tmd::mesh::VramMesh {
            positions: vec![
                [0.0, 0.0, 0.0],
                [100.0, 0.0, 0.0],
                [0.0, 100.0, 0.0],
                [100.0, 100.0, 0.0],
            ],
            uvs: vec![[0, 0], [1, 1], [2, 2], [3, 3]],
            cba_tsb: vec![[0, 0]; 4],
            colors: vec![[128; 3]; 4],
            normals: vec![[0.0; 3]; 4],
            indices: vec![0, 1, 2, 1, 3, 2],
        };
        let input = ShatterInput::new(0x80, row(1)).unwrap();
        let m = shatter_vram_mesh(&local, &[0; 4], &input, &[], false, |_| SweepFrame::MODEL);
        assert_eq!(m.positions.len(), 6);
        assert_eq!(m.uvs[3], [1, 1]);
        assert_eq!(m.uvs[4], [3, 3]);
        assert!(ShatterInput::new(0, row(1)).is_none());
    }

    /// A sweep row measures through the body's view map: Freed's row 1
    /// (front `-0x400 + v`, shrink + sweep Y) leaves a triangle `0x100` above
    /// the root intact along the bare body axes, but the battle's 4x world
    /// scale puts it `0x400` up the view, past the front at `v = 0x100`.
    #[test]
    fn the_view_map_scales_the_sweep_measure() {
        let row = ShatterRow {
            off: [0; 3],
            h3: -0x400,
            flags: (FLAG_SHRINK | FLAG_SWEEP_Y) >> 16,
        };
        let input = ShatterInput::new(0x100, row).unwrap();
        let tri = [[-16, -0x100, 0], [16, -0x100, 0], [0, -0x100 + 16, 0]];
        let bones: [BonePose; 1] = [([0; 3], [0; 3])];
        let run = |view: &[[f32; 3]; 3]| {
            let mut o = ShatterObject::new(
                input.word,
                &input.row,
                bone_sweep_frame(&bones, 0, true, view),
            );
            o.triangle(tri)
        };
        let whole = run(&BODY_AXES);
        assert_eq!(whole[0], [-16, -0x100, 0], "below the front: intact");
        let scaled = run(&[[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]]);
        assert_ne!(scaled, whole, "past the front: pushed and shrunk");
    }
}
