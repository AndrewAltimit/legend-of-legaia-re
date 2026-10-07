//! A **per-draw census** of a host's frame, keyed the way a retail display
//! list is: by texture family (`CLUT`, `tpage`), with each family's screen
//! bounds, triangle count and mean corner colour.
//!
//! The retail side of the comparison needs no emulator: a save state's RAM
//! holds the frame's packets on its ordering tables, and
//! `mednafen-state display-list --json` (or `scripts/mednafen/display-list.py`
//! for a PCSX-Redux state) prints them with their CLUT, tpage, screen bounds
//! and colour. This module is the engine's side - the hosts keep a
//! [`MeshCensus`] beside every uploaded mesh while the diagnostic is on, and
//! fold each frame's draws into [`FamilyRow`]s through the draw's own
//! view-projection - so `scripts/ci/draw-family-diff.py` can say which
//! families one side draws and the other does not, where on screen, and how
//! bright. It is a diagnostic: nothing in the game path reads it.

use std::collections::BTreeMap;

/// One triangle of an uploaded mesh, as the census sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CensusTri {
    /// The PSX `CLUT` word (`cba`) of the triangle's first corner, with the
    /// engine's double-sided pair flag (bit 15) masked off - a packet's CLUT
    /// word never sets it.
    pub cba: u16,
    /// The PSX `tpage` word (`tsb`), with the engine's own high flag bits
    /// masked off (`& 0x01FF`) so it compares with a packet's tpage.
    pub tsb: u16,
    /// The prim's semi-transparency enable (the engine-packed ABE, `tsb`
    /// bit 15) - a packet's command bit 1.
    pub semi: bool,
    /// Mesh-space corners.
    pub pos: [[f32; 3]; 3],
    /// Mean corner colour word (`0x80` = neutral).
    pub color: [u8; 3],
}

/// Every triangle of one uploaded mesh.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshCensus {
    pub tris: Vec<CensusTri>,
}

impl MeshCensus {
    /// Build a census from a mesh's upload arrays (the
    /// `legaia_tmd::mesh::VramMesh` fields).
    pub fn from_mesh(
        positions: &[[f32; 3]],
        cba_tsb: &[[u16; 2]],
        colors: &[[u8; 3]],
        indices: &[u32],
    ) -> Self {
        let tris = indices
            .as_chunks::<3>()
            .0
            .iter()
            .filter_map(|t| {
                let [a, b, c] = t.map(|i| i as usize);
                let p = [*positions.get(a)?, *positions.get(b)?, *positions.get(c)?];
                let [cba, tsb] = *cba_tsb.get(a)?;
                let col = [a, b, c].map(|i| colors.get(i).copied().unwrap_or([0x80; 3]));
                let mean = [0, 1, 2].map(|k| {
                    ((u32::from(col[0][k]) + u32::from(col[1][k]) + u32::from(col[2][k])) / 3) as u8
                });
                Some(CensusTri {
                    cba: cba & 0x7FFF,
                    tsb: tsb & 0x01FF,
                    semi: tsb & 0x8000 != 0,
                    pos: p,
                    color: mean,
                })
            })
            .collect();
        Self { tris }
    }
}

/// One texture family of a frame.
#[derive(Debug, Clone, PartialEq)]
pub struct FamilyRow {
    pub cba: u16,
    pub tsb: u16,
    /// Triangles with at least one corner in front of the eye and inside the
    /// screen rectangle.
    pub tris: u32,
    /// Screen bounds `[x0, y0, x1, y1]` on the 320 x 240 stage.
    pub bounds: [f32; 4],
    /// Mean colour word over those triangles.
    pub color: [f32; 3],
    /// Of [`Self::tris`], how many wind clockwise on screen (a negative
    /// signed area with `y` down). A family whose count splits between the two
    /// windings is drawing back faces a GTE `NCLIP` pass would drop.
    pub clockwise: u32,
    /// Of [`Self::tris`], how many carry the semi-transparency enable.
    pub semi: u32,
    /// Clip-space depth range `[min, max]` of the on-stage corners
    /// (`z / w`, before the renderer's reversed-Z): a family past `1.0` or
    /// below `0.0` is clipped away by the rasteriser however it counts here.
    pub depth: [f32; 2],
}

impl FamilyRow {
    fn empty(cba: u16, tsb: u16) -> Self {
        Self {
            cba,
            tsb,
            tris: 0,
            bounds: [f32::MAX, f32::MAX, f32::MIN, f32::MIN],
            color: [0.0; 3],
            clockwise: 0,
            semi: 0,
            depth: [f32::MAX, f32::MIN],
        }
    }
}

/// Fold `draws` - each a census and its column-major view-projection (the
/// matrix the host hands its renderer) - into texture families on a
/// `width x height` stage. Clip-space `w <= 0` corners are dropped; a
/// triangle counts when any corner lands on the stage.
pub fn family_rows<'a>(
    draws: impl IntoIterator<Item = (&'a MeshCensus, [f32; 16])>,
    width: f32,
    height: f32,
) -> Vec<FamilyRow> {
    // Colour sums ride in `f64` beside each row until the mean is taken.
    let mut acc: BTreeMap<(u16, u16), (FamilyRow, [f64; 3])> = BTreeMap::new();
    for (census, m) in draws {
        for t in &census.tris {
            let mut on = false;
            let mut bb = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            let mut scr = [[0.0f32; 2]; 3];
            let mut behind = false;
            let mut dz = [f32::MAX, f32::MIN];
            for (i, p) in t.pos.into_iter().enumerate() {
                let x = m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12];
                let y = m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13];
                let z = m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14];
                let w = m[3] * p[0] + m[7] * p[1] + m[11] * p[2] + m[15];
                if w <= 1e-6 {
                    behind = true;
                    continue;
                }
                let sx = (x / w * 0.5 + 0.5) * width;
                let sy = (0.5 - y / w * 0.5) * height;
                scr[i] = [sx, sy];
                dz = [dz[0].min(z / w), dz[1].max(z / w)];
                if (0.0..width).contains(&sx) && (0.0..height).contains(&sy) {
                    on = true;
                }
                bb = [bb[0].min(sx), bb[1].min(sy), bb[2].max(sx), bb[3].max(sy)];
            }
            if !on {
                continue;
            }
            let area = (scr[1][0] - scr[0][0]) * (scr[2][1] - scr[0][1])
                - (scr[2][0] - scr[0][0]) * (scr[1][1] - scr[0][1]);
            let (row, sum) = acc
                .entry((t.cba, t.tsb))
                .or_insert_with(|| (FamilyRow::empty(t.cba, t.tsb), [0.0; 3]));
            row.tris += 1;
            if !behind && area < 0.0 {
                row.clockwise += 1;
            }
            if t.semi {
                row.semi += 1;
            }
            row.depth = [row.depth[0].min(dz[0]), row.depth[1].max(dz[1])];
            row.bounds = [
                row.bounds[0].min(bb[0]),
                row.bounds[1].min(bb[1]),
                row.bounds[2].max(bb[2]),
                row.bounds[3].max(bb[3]),
            ];
            for (s, c) in sum.iter_mut().zip(t.color) {
                *s += f64::from(c);
            }
        }
    }
    acc.into_values()
        .map(|(mut row, sum)| {
            row.color = sum.map(|s| (s / f64::from(row.tris.max(1))) as f32);
            row
        })
        .collect()
}

/// [`family_rows`] as JSON lines, one family per line - the shape
/// `scripts/ci/draw-family-diff.py` reads.
pub fn family_rows_jsonl(rows: &[FamilyRow]) -> String {
    rows.iter()
        .map(|r| {
            format!(
                "{{\"clut\":{},\"tpage\":{},\"tris\":{},\"clockwise\":{},\"semi\":{},\"bounds\":[{:.1},{:.1},{:.1},{:.1}],\"color\":[{:.1},{:.1},{:.1}],\"depth\":[{:.4},{:.4}]}}\n",
                r.cba,
                r.tsb,
                r.tris,
                r.clockwise,
                r.semi,
                r.bounds[0],
                r.bounds[1],
                r.bounds[2],
                r.bounds[3],
                r.color[0],
                r.color[1],
                r.color[2],
                r.depth[0],
                r.depth[1]
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A triangle in front of an identity camera lands on the stage under its
    /// own family; one behind the eye does not count.
    #[test]
    fn families_count_on_stage_triangles_only() {
        let front = MeshCensus::from_mesh(
            &[[0.0, 0.0, 0.5], [0.5, 0.0, 0.5], [0.0, 0.5, 0.5]],
            &[[0x7EC1, 0x820B]; 3],
            &[[0x40; 3]; 3],
            &[0, 1, 2],
        );
        let mut ident = [0.0f32; 16];
        ident[0] = 1.0;
        ident[5] = 1.0;
        ident[10] = 1.0;
        ident[15] = 1.0;
        let rows = family_rows([(&front, ident)], 320.0, 240.0);
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (rows[0].cba, rows[0].tsb, rows[0].tris, rows[0].semi),
            (0x7EC1, 0x000B, 1, 1)
        );
        assert_eq!(rows[0].color, [64.0; 3]);
        assert_eq!(rows[0].depth, [0.5, 0.5]);
        let mut behind = ident;
        behind[15] = -1.0;
        assert!(family_rows([(&front, behind)], 320.0, 240.0).is_empty());
    }
}
