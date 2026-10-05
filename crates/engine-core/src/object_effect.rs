//! The **object-effect parameter table** at `0x80083FF8` and the per-actor
//! clip it stages for a raised `actor[+0x42]`.
//!
//! An actor whose `+0x42` is non-zero draws through the far arm of
//! `FUN_8001ADA4` / `FUN_8001B964`: `FUN_8001C204` composes a working
//! transform from row `+0x42 - 1` of this table, and `FUN_8002735C` clips
//! each primitive against the row's two bound words before it projects it
//! (`FUN_80027F00`, interpolating through `FUN_80029724`). Field-VM `4C C2`
//! raises the halfword on placed actors in nine scenes, and each of those
//! scenes' prescript writes row 0 through move-VM ext `0x17..0x1A` first.
//!
//! What the disassembly pins:
//!
//! * **The row.** Stride `0x14`: three rotation angles at `+0` / `+2` /
//!   `+4` and the two bounds at `+0x10` (`lo`) / `+0x12` (`hi`). The boot
//!   init `FUN_8001D424` seeds row 0 as angles `0`, bounds `(-100, -20)`
//!   (`0x8001D618..0x8001D648`) and row 1 at `+0x14` / `+0x26`; the battle
//!   scene setup `FUN_80055B6C` re-seeds row 0 as angles `0`, `lo = -0x7FFF`,
//!   `hi = 0` (`0x80055DDC..0x80055DF8`).
//! * **The space.** `FUN_8001ADA4` stores the identity (`FUN_8003D178`, rot
//!   and zero translation) at `0x1F8002F4` (`0x8001B230..0x8001B238`) and
//!   scales it by the render scale `+0x72` with the actor matrix.
//!   `FUN_8001C204` loads it, post-multiplies `RotMatrixZ(row+4)`,
//!   `RotMatrixY(row+2)`, `RotMatrixX(row+0)` (`FUN_8004638C` / `629C` /
//!   `61A4`), runs the actor position `+0x14` through it as the translation,
//!   then post-multiplies the actor Euler. So a vertex's effect-space point
//!   is `s * Rrow * (Ractor * v + pos)` - **world** space turned by the row,
//!   not view space.
//! * **The clip.** `FUN_80027F00` drops a vertex whose effect-space `Y`
//!   (record `+4`, the `MVMVA` through `0x1F800314`) is below `lo`
//!   (`[0x1F800314]+0x6C` = `0x1F800380`) or above `hi` (`+0x6E`), and
//!   synthesises the crossing on each edge; the kept geometry is projected
//!   through the actor's ordinary matrix. So the visible part of the mesh is
//!   the slab `lo <= y_eff <= hi`.
//!
//! The port draws the same slab with a per-draw fragment discard rather than
//! retail's per-edge polygon clip, which keeps the same pixels.

/// Rows the table holds that the port tracks. Every shipped write names row
/// `0`; row 1 is boot-seeded and read by no shipped `4C C2`.
pub const OBJECT_EFFECT_ROWS: usize = 8;

/// One row: `[ang_x (+0), ang_y (+2), ang_z (+4), lo (+0x10), hi (+0x12)]`.
pub type ObjectEffectRow = [i16; 5];

/// Row 0 as the boot init `FUN_8001D424` seeds it.
pub const BOOT_ROW0: ObjectEffectRow = [0, 0, 0, -100, -20];
/// Row 1 as the boot init seeds it (`+0x14 = -0x400`, `+0x26 = 0x1388`).
pub const BOOT_ROW1: ObjectEffectRow = [-0x400, 0, 0, 0, 0x1388];
/// Row 0 as the battle scene setup `FUN_80055B6C` re-seeds it.
pub const BATTLE_ROW0: ObjectEffectRow = [0, 0, 0, -0x7FFF, 0];

/// The table (`0x80083FF8`, stride `0x14`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectEffectTable {
    rows: [ObjectEffectRow; OBJECT_EFFECT_ROWS],
}

impl Default for ObjectEffectTable {
    fn default() -> Self {
        Self::boot()
    }
}

impl ObjectEffectTable {
    /// The table as the boot init leaves it.
    pub fn boot() -> Self {
        let mut rows = [[0; 5]; OBJECT_EFFECT_ROWS];
        rows[0] = BOOT_ROW0;
        rows[1] = BOOT_ROW1;
        Self { rows }
    }

    /// The battle scene setup's re-seed of row 0.
    pub fn reseed_for_battle(&mut self) {
        self.rows[0] = BATTLE_ROW0;
    }

    /// Row `index`, if the port tracks it.
    pub fn row(&self, index: i16) -> Option<ObjectEffectRow> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.rows.get(i))
            .copied()
    }

    /// Ext `0x17` / `0x18` / `0x1A`: store five halfwords.
    pub fn write(&mut self, index: i16, values: ObjectEffectRow) {
        if let Some(r) = usize::try_from(index)
            .ok()
            .and_then(|i| self.rows.get_mut(i))
        {
            *r = values;
        }
    }

    /// Ext `0x19`: add five halfwords (`addu` + `sh`, so 16-bit wrap).
    pub fn add(&mut self, index: i16, deltas: ObjectEffectRow) {
        if let Some(r) = usize::try_from(index)
            .ok()
            .and_then(|i| self.rows.get_mut(i))
        {
            for (v, d) in r.iter_mut().zip(deltas) {
                *v = v.wrapping_add(d);
            }
        }
    }

    /// The clip an actor with `+0x42 = kind` draws under, or `None` while the
    /// gate is down (`kind == 0`) or names a row the port does not track.
    /// `sin` / `cos` are the retail trig LUT pair (`0x1000` entries each).
    pub fn clip_for(&self, kind: u16, sin: &[i16], cos: &[i16]) -> Option<EffectClip> {
        let row = self.row(i16::try_from(kind).ok()?.checked_sub(1)?)?;
        Some(EffectClip::from_row(row, sin, cos))
    }
}

/// The clip one actor draws under: effect-space `Y = scale * (n . world)`
/// kept inside `[lo, hi]`, with `n` the second row of the row rotation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectClip {
    /// Row 1 of `Rz(row+4) * Ry(row+2) * Rx(row+0)`, unit length.
    pub n: [f32; 3],
    /// `+0x10`.
    pub lo: f32,
    /// `+0x12`.
    pub hi: f32,
}

/// Retail trig sample in `1.0 = 0x1000` units (`angle & 0xFFF`).
fn trig(lut: &[i16], angle: i16) -> f32 {
    let i = (angle as u16 & 0xFFF) as usize;
    f32::from(lut.get(i).copied().unwrap_or(0)) / 4096.0
}

impl EffectClip {
    /// The clip a row stages. `RotMatrixZ` is `[[c,-s,0],[s,c,0],[0,0,1]]`,
    /// `RotMatrixY` `[[c,0,s],[0,1,0],[-s,0,c]]`, `RotMatrixX`
    /// `[[1,0,0],[0,c,-s],[0,s,c]]` (the columns `FUN_8004638C` /
    /// `FUN_8004629C` / `FUN_800461A4` feed `MVMVA`), composed as
    /// `Rz * Ry * Rx`; only that product's row 1 reaches the clip.
    pub fn from_row(row: ObjectEffectRow, sin: &[i16], cos: &[i16]) -> Self {
        let (sx, cx) = (trig(sin, row[0]), trig(cos, row[0]));
        let (sy, cy) = (trig(sin, row[1]), trig(cos, row[1]));
        let (sz, cz) = (trig(sin, row[2]), trig(cos, row[2]));
        // Row 1 of Rz = (sz, cz, 0); times Ry = (sz*cy, cz, sz*sy);
        // times Rx.
        let a = [sz * cy, cz, sz * sy];
        let n = [a[0], a[1] * cx + a[2] * sx, -a[1] * sx + a[2] * cx];
        Self {
            n,
            lo: f32::from(row[3]),
            hi: f32::from(row[4]),
        }
    }

    /// The same slab in a draw's **mesh** space, for a host shader that only
    /// sees mesh-space positions.
    ///
    /// `model_rows` are the draw's model matrix rows (`world = rows . [p, 1]`)
    /// and `actor_scale` the actor's render scale `+0x72` in `1.0 = 0x1000`
    /// units. Retail measures `s * n . (Ractor * v + pos)`; a port draw places
    /// the same vertex at `A * p + t` with `A` already carrying its own
    /// scale `s_A`, so the effect value is `(s / s_A) * n . (A p) + s * n . t`
    /// - the returned `m . p` against `[lo, hi]` shifted by the constant.
    pub fn in_mesh_space(&self, model_rows: [[f32; 4]; 3], actor_scale: f32) -> MeshClip {
        let col0 = [model_rows[0][0], model_rows[1][0], model_rows[2][0]];
        let s_a = (col0[0] * col0[0] + col0[1] * col0[1] + col0[2] * col0[2])
            .sqrt()
            .max(1e-6);
        let k = actor_scale / s_a;
        let mut m = [0.0f32; 3];
        for (j, mj) in m.iter_mut().enumerate() {
            *mj = k
                * (self.n[0] * model_rows[0][j]
                    + self.n[1] * model_rows[1][j]
                    + self.n[2] * model_rows[2][j]);
        }
        let nt = self.n[0] * model_rows[0][3]
            + self.n[1] * model_rows[1][3]
            + self.n[2] * model_rows[2][3];
        let c = actor_scale * nt;
        MeshClip {
            m,
            lo: self.lo - c,
            hi: self.hi - c,
        }
    }
}

/// [`EffectClip`] in one draw's mesh space: keep `lo <= m . p <= hi`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshClip {
    pub m: [f32; 3],
    pub lo: f32,
    pub hi: f32,
}

impl MeshClip {
    /// Whether mesh-space point `p` survives the clip.
    pub fn keeps(&self, p: [f32; 3]) -> bool {
        let y = self.m[0] * p[0] + self.m[1] * p[1] + self.m[2] * p[2];
        y >= self.lo && y <= self.hi
    }

    /// The six floats a host shader reads, `[m0, m1, m2, lo, hi, 1]`.
    pub fn shader_floats(&self) -> [f32; 6] {
        [self.m[0], self.m[1], self.m[2], self.lo, self.hi, 1.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luts() -> (Vec<i16>, Vec<i16>) {
        let sin: Vec<i16> = (0..0x1000)
            .map(|i| ((i as f64 * std::f64::consts::TAU / 4096.0).sin() * 4096.0) as i16)
            .collect();
        let cos: Vec<i16> = (0..0x1000)
            .map(|i| ((i as f64 * std::f64::consts::TAU / 4096.0).cos() * 4096.0) as i16)
            .collect();
        (sin, cos)
    }

    #[test]
    fn boot_and_battle_seeds() {
        let mut t = ObjectEffectTable::boot();
        assert_eq!(t.row(0), Some(BOOT_ROW0));
        t.reseed_for_battle();
        assert_eq!(t.row(0), Some([0, 0, 0, -0x7FFF, 0]));
        assert_eq!(t.row(-1), None);
    }

    #[test]
    fn add_wraps_and_kind_zero_is_no_clip() {
        let (s, c) = luts();
        let mut t = ObjectEffectTable::boot();
        t.add(0, [1, 2, 3, 4, 5]);
        assert_eq!(t.row(0), Some([1, 2, 3, -96, -15]));
        assert!(t.clip_for(0, &s, &c).is_none(), "+0x42 = 0 draws unclipped");
        assert!(t.clip_for(1, &s, &c).is_some());
    }

    /// Zero angles: the clip is world `Y` (Y-down), garmel's `0x17` row
    /// keeps everything between `-4096` and the floor at `0`.
    #[test]
    fn a_flat_row_clips_world_y() {
        let (s, c) = luts();
        let clip = EffectClip::from_row([0, 0, 0, -4096, 0], &s, &c);
        assert_eq!(clip.n, [0.0, 1.0, 0.0]);
        // An unscaled actor standing at y = -50: mesh y = -100 is world -150.
        let rows = [
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, -50.0],
            [0.0, 0.0, 1.0, 7.0],
        ];
        let mc = clip.in_mesh_space(rows, 1.0);
        assert!(mc.keeps([0.0, -100.0, 0.0]));
        assert!(!mc.keeps([0.0, 60.0, 0.0]), "below the floor is clipped");
    }

    /// The `0x1A` yaw seat (`+4 = 0x400`): row 1 of `Rz(90)*Ry(yaw)` is
    /// `(cos yaw, 0, sin yaw)`, a vertical wipe plane.
    #[test]
    fn a_yaw_seat_is_a_vertical_plane() {
        let (s, c) = luts();
        let clip = EffectClip::from_row([0, 0x400, 0x400, -4096, 0], &s, &c);
        assert!(clip.n[0].abs() < 1e-3);
        assert!(clip.n[1].abs() < 1e-3);
        assert!((clip.n[2] - 1.0).abs() < 1e-3);
    }

    /// A render-scaled actor: retail scales the position term too.
    #[test]
    fn render_scale_shifts_the_bound_by_the_position() {
        let (s, c) = luts();
        let clip = EffectClip::from_row([0, 0, 0, -100, 100], &s, &c);
        let rows = [
            [1.25, 0.0, 0.0, 0.0],
            [0.0, 1.25, 0.0, -40.0],
            [0.0, 0.0, 1.25, 0.0],
        ];
        let mc = clip.in_mesh_space(rows, 1.25);
        // effect y = 1.25 * (p.y + -40) for unscaled mesh point p.
        assert!(mc.keeps([0.0, 0.0, 0.0])); // 1.25 * -40 = -50
        assert!(mc.keeps([0.0, 119.0, 0.0])); // 1.25 * 79 = 98.75
        assert!(!mc.keeps([0.0, 121.0, 0.0])); // 1.25 * 81 = 101.25
    }
}
