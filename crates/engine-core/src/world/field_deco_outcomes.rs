//! Each decoration primitive priced by what its leaf does with it, under
//! the derived field frame cost (`WorldToggles::field_cost_prim_outcomes`).
//!
//! The decoration pass `FUN_801F7088` hands each drawn cell's pack mesh to
//! the prim dispatcher `FUN_80043390` with the colour word
//! `*(0x8007BB48) | (+0x1E != 0 ? 0x40000000) | (+0x12 & 0x800 ? 0x10000000)`
//! (`0x801F7898..0x801F78C8`) and `a2 = 0`. A non-zero top byte raises
//! `a2` and so picks the depth-cued handler set (`0x800433C0..0x800433CC`,
//! `0x80043494`), and `0x40000000` the off-screen test inside it; no
//! decoration word carries `0x08000000`, so the winding mask stays the
//! single-sided `0xFFFFFFFF` (`0x80043520..0x80043540`). Each leaf then
//! transforms the primitive (`RTPT`), tests its winding (`NCLIP`), its
//! depth (`OTZ` under the floor `0x1F80037E`) and, where it runs one, the
//! off-screen test `FUN_80044798`, in the leaf's own order, and costs what
//! the path it took costs (`field_frame_units::FIELD_UNIT_CYCLES_X10`, the
//! `prim k<kind> <bank> <outcome>` rows). The pooled per-kind figure
//! (`DECORATION_PRIM_CYCLES`) averages those outcomes over the ten profiled
//! states, which a view that rejects far more than they did overprices:
//! `conc3_ceremony_mist_pillar` drops `224` of its `240` kind-15 quads.
//!
//! A camera-facing cell (`+0x12 & 0x380`, rebuilt off the camera) keeps
//! the pooled figure.
//!
//! REF: FUN_801F7088, FUN_80043390, FUN_80044798

use legaia_render_kernels::gte::{FieldGteView, GteMat3};

/// One decoration primitive: its corners into the mesh's flattened
/// vertices, its topology and the dispatcher's kind (`flags >> 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecoPrim {
    pub v: [u16; 4],
    pub quad: bool,
    pub kind: u8,
}

/// A pack mesh's primitives as the decoration pass's leaves walk them, its
/// objects' vertices flattened (each primitive indexes its own object's).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecoGeom {
    pub verts: Vec<[i16; 3]>,
    pub prims: Vec<DecoPrim>,
}

/// The field's ordering-table shift (`0x1F8003A4`) and near floor
/// (`0x1F80037E`), as the catalogued field states hold them: `ZSF3 = 0x555
/// >> 3`, `ZSF4 = 0x400 >> 3`, floor `0x10`.
const ZSF3: i64 = 0x555 >> 3;
const ZSF4: i64 = 0x400 >> 3;
const NEAR_FLOOR: i64 = 0x10;

/// How a primitive leaves its leaf.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// Dropped before any off-screen test: the winding, or the near floor.
    Rejected,
    /// Dropped by the off-screen test.
    RejectedOff,
    /// Linked, with or without the off-screen test on the way.
    Linked,
    LinkedOff,
}

/// Cycles a primitive costs by `(bank, kind, outcome)`, x 10: the
/// `prim k<kind> <bank> <outcome>` rows of the field-units table. `None`
/// where no profiled state took that path (the caller prices the pooled
/// figure there).
fn outcome_cycles_x10(cued: bool, kind: u8, o: Outcome) -> Option<i64> {
    use Outcome::*;
    let v = match (cued, kind, o) {
        (false, 12, Linked) => 1201,
        (false, 12, Rejected) => 734,
        (false, 13, Linked) => 1423,
        (false, 13, Rejected) => 978,
        (false, 14, Linked) => 1253,
        (false, 14, Rejected) => 663,
        (false, 15, Linked) => 1591,
        (false, 15, Rejected) => 965,
        (false, 16, Linked) => 1392,
        (false, 16, Rejected) => 727,
        (false, 17, Linked) => 1672,
        (false, 17, Rejected) => 969,
        (false, 18, Linked) => 1513,
        (false, 18, Rejected) => 722,
        (false, 19, Linked) => 1836,
        (false, 19, Rejected) => 967,
        (true, 12, Linked) => 1573,
        (true, 12, Rejected) => 743,
        (true, 13, Linked) => 1796,
        (true, 13, Rejected) => 975,
        (true, 14, LinkedOff) => 2642,
        (true, 14, Rejected) => 672,
        // No field state took it; the battle leaf table's off-screen
        // reject (`0x80044FDC`, `226`).
        (true, 14, RejectedOff) => 2260,
        (true, 15, LinkedOff) => 3040,
        (true, 15, Rejected) => 962,
        (true, 15, RejectedOff) => 2839,
        (true, 16, LinkedOff) => 2686,
        (true, 16, Rejected) => 724,
        (true, 16, RejectedOff) => 2390,
        (true, 17, LinkedOff) => 3022,
        (true, 17, Rejected) => 966,
        (true, 17, RejectedOff) => 2165,
        (true, 18, LinkedOff) => 2812,
        (true, 18, Rejected) => 808,
        (true, 18, RejectedOff) => 2475,
        (true, 19, Linked) => 2499,
        (true, 19, LinkedOff) => 3306,
        (true, 19, Rejected) => 961,
        (true, 19, RejectedOff) => 2096,
        _ => return None,
    };
    Some(v)
}

/// The leaf's cull shape, by kind and topology (the depth-cued handlers'
/// rules, `battle-stage-camera.md`, "The census"; the opaque handlers keep
/// the same winding and near tests and run no off-screen test).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Leaf {
    /// `NCLIP >= 0`, the near test, then (cued, kind `!= 12`) the
    /// off-screen test.
    Tri,
    /// Kinds `13` / `15` / `17`: kept on the first half's `NCLIP > 0` or the
    /// second's `< 0`, the near test, then (cued, kind `!= 13`) the
    /// off-screen test.
    QuadStrict,
    /// Kind `19`: kept on `>= 0` or the second half's `<= 0`, the off-screen
    /// test under the word's `0x40` bit, then the near test.
    QuadLoose,
}

/// The composed transform of one decoration draw: the camera matrix times
/// the record's `Rx * Ry * Rz` (`FUN_80026988`), its translation the
/// camera's moved by the cell's world position.
pub struct DecoTransform {
    rot: GteMat3,
    tr: [i64; 3],
    h: u16,
    ofx: i32,
    ofy: i32,
}

impl DecoTransform {
    pub fn new(view: &FieldGteView, pos: [i32; 3], angles: [u16; 3]) -> Self {
        let obj = legaia_render_kernels::battle_intro::euler_rot_psx((
            angles[0] as i16,
            angles[1] as i16,
            angles[2] as i16,
        ));
        let rot = view.rot.mul(&obj);
        let tr = std::array::from_fn(|r| {
            let s: i64 = (0..3)
                .map(|c| i64::from(view.rot.m[r][c]) * i64::from(pos[c]))
                .sum();
            i64::from(view.tr[r]) + (s >> 12)
        });
        Self {
            rot,
            tr,
            h: view.h,
            ofx: view.ofx,
            ofy: view.ofy,
        }
    }

    /// `RTPS` of an object-local vertex: the screen pair and `SZ`.
    fn project(&self, v: [i16; 3]) -> (i32, i32, i64) {
        let mac: [i64; 3] = std::array::from_fn(|r| {
            let s: i64 = (0..3)
                .map(|c| i64::from(self.rot.m[r][c]) * i64::from(v[c]))
                .sum();
            ((self.tr[r] << 12) + s) >> 12
        });
        let ir = mac.map(|m| m.clamp(-0x8000, 0x7FFF));
        let sz = mac[2].clamp(0, 0xFFFF);
        let (div, _) = legaia_render_kernels::gte::divide::gte_divide(self.h, sz as u16);
        let screen = |of: i32, ir: i64| {
            (((i64::from(of) << 16) + ir * div) >> 16).clamp(-0x400, 0x3FF) as i32
        };
        (screen(self.ofx, ir[0]), screen(self.ofy, ir[1]), sz)
    }
}

/// The cycles `geom` costs through the decoration leaves under `t`, x 10,
/// with the primitives it priced by kind; a primitive with no outcome row
/// is left to the pooled figure (not counted in either).
pub fn price_decoration(
    geom: &DecoGeom,
    t: &DecoTransform,
    cued: bool,
    offtest: bool,
    scratch: &mut Vec<(i32, i32, i64)>,
) -> (i64, [u32; 12]) {
    scratch.clear();
    scratch.extend(geom.verts.iter().map(|&v| t.project(v)));
    let c = &scratch[..];
    let nclip = |a: (i32, i32, i64), b: (i32, i32, i64), d: (i32, i32, i64)| -> i32 {
        let (ax, ay, bx, by, cx, cy) = (
            i64::from(a.0),
            i64::from(a.1),
            i64::from(b.0),
            i64::from(b.1),
            i64::from(d.0),
            i64::from(d.1),
        );
        (ax * by + bx * cy + cx * ay - ax * cy - bx * ay - cx * by) as i32
    };
    let mut cycles = 0i64;
    let mut kinds = [0u32; 12];
    for p in &geom.prims {
        let n = if p.quad { 4 } else { 3 };
        let mut k = [(0i32, 0i32, 0i64); 4];
        let mut whole = true;
        for (i, slot) in k.iter_mut().enumerate().take(n) {
            match c.get(usize::from(p.v[i])) {
                Some(&x) => *slot = x,
                None => whole = false,
            }
        }
        if !whole {
            continue;
        }
        let k = &k[..n];
        let leaf = match (p.quad, p.kind) {
            (false, _) => Leaf::Tri,
            (true, 19) => Leaf::QuadLoose,
            (true, _) => Leaf::QuadStrict,
        };
        let first = nclip(k[0], k[1], k[2]);
        let faced = match leaf {
            Leaf::Tri => first >= 0,
            Leaf::QuadStrict => first > 0 || nclip(k[1], k[2], k[3]) < 0,
            Leaf::QuadLoose => first >= 0 || nclip(k[1], k[2], k[3]) <= 0,
        };
        let sum: i64 = k.iter().map(|x| x.2).sum();
        let otz = if p.quad {
            (ZSF4 * sum) >> 12
        } else {
            (ZSF3 * sum) >> 12
        };
        let near = otz < NEAR_FLOOR;
        let off = || {
            let all = |f: &dyn Fn(&(i32, i32, i64)) -> bool| k.iter().all(f);
            all(&|x| x.1 < 0)
                || all(&|x| x.1 >= 0xF0)
                || all(&|x| x.0 <= 0)
                || all(&|x| x.0 >= 0x140)
        };
        let tested = cued
            && match leaf {
                Leaf::Tri => p.kind != 12,
                Leaf::QuadStrict => p.kind != 13,
                Leaf::QuadLoose => offtest,
            };
        let o = if !faced {
            Outcome::Rejected
        } else if leaf == Leaf::QuadLoose {
            if tested && off() {
                Outcome::RejectedOff
            } else if near {
                Outcome::Rejected
            } else if tested {
                Outcome::LinkedOff
            } else {
                Outcome::Linked
            }
        } else if near {
            Outcome::Rejected
        } else if tested && off() {
            Outcome::RejectedOff
        } else if tested {
            Outcome::LinkedOff
        } else {
            Outcome::Linked
        };
        if let Some(v) = outcome_cycles_x10(cued, p.kind, o) {
            cycles += v;
            kinds[usize::from(p.kind - 8)] += 1;
        }
    }
    (cycles, kinds)
}

impl DecoGeom {
    /// Every object of `tmd`, its vertices flattened in object order and
    /// each primitive re-based into them.
    pub fn from_tmd(tmd: &legaia_tmd::Tmd, raw: &[u8]) -> Self {
        let mut out = Self::default();
        for o in &tmd.objects {
            let base = out.verts.len();
            out.verts.extend(o.vertices.iter().map(|v| [v.x, v.y, v.z]));
            let n_vert = o.header.n_vert;
            for g in legaia_tmd::legaia_prims::iter_groups_lenient(
                raw,
                o.primitives_byte_offset,
                o.primitives_byte_size,
            ) {
                let kind = (g.header.flags >> 1) as u8;
                if !(8..=19).contains(&kind) {
                    continue;
                }
                for p in &g.prims {
                    let idx = p.vertex_indices();
                    if idx.iter().any(|&i| u32::from(i) >= n_vert) || !(3..=4).contains(&idx.len())
                    {
                        continue;
                    }
                    let at = |i: usize| (base + usize::from(idx[i.min(idx.len() - 1)])) as u16;
                    out.prims.push(DecoPrim {
                        v: [at(0), at(1), at(2), at(3)],
                        quad: idx.len() == 4,
                        kind,
                    });
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera at the origin looking down `+Z` (the field base matrix, no
    /// rotation), `H = 256`.
    fn view() -> FieldGteView {
        FieldGteView::from_globals(
            [0, 0, 0],
            [0, 0, 0],
            [0, 0, 0],
            &FieldGteView::field_base(),
            256,
            160,
            120,
        )
    }

    /// One kind-17 quad at depth `z`, wound so it faces the camera when
    /// `front`.
    fn quad(front: bool) -> DecoGeom {
        let v = vec![[-20, -20, 0], [20, -20, 0], [-20, 20, 0], [20, 20, 0]];
        let order = if front { [0, 1, 2, 3] } else { [1, 0, 3, 2] };
        DecoGeom {
            verts: v,
            prims: vec![DecoPrim {
                v: order,
                quad: true,
                kind: 17,
            }],
        }
    }

    #[test]
    fn a_primitive_costs_the_path_its_leaf_takes() {
        let mut scratch = Vec::new();
        let at = |z: i32| DecoTransform::new(&view(), [0, 0, z], [0, 0, 0]);
        // On screen, facing: linked through the tested cued leaf.
        let (c, k) = price_decoration(&quad(true), &at(400), true, true, &mut scratch);
        assert_eq!((c, k[17 - 8]), (3022, 1));
        // Back-facing: rejected before the test.
        let (c, _) = price_decoration(&quad(false), &at(400), true, true, &mut scratch);
        assert_eq!(c, 966);
        // The opaque bank runs no test.
        let (c, _) = price_decoration(&quad(true), &at(400), false, false, &mut scratch);
        assert_eq!(c, 1672);
        // Wholly off to the side: the test drops it.
        let side = DecoTransform::new(&view(), [600, 0, 400], [0, 0, 0]);
        let (c, _) = price_decoration(&quad(true), &side, true, true, &mut scratch);
        assert_eq!(c, 2165);
    }
}
