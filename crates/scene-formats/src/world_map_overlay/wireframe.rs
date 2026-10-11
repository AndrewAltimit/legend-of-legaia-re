//! The wireframe-era views of a slot-4 bank: top-down lines, per-record
//! points, 3D segments and the translation path, with their bounds helpers.
//! Viewer aids over the byte view, not a claim about what the bank is.

use super::*;

/// A line segment in the X-Z (top-down) plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WireframeLine {
    pub body_index: u8,
    pub group_index: u16,
    pub x0: i16,
    pub z0: i16,
    pub x1: i16,
    pub z1: i16,
}

/// Which axis of a [`Slot4Record`] to project onto the 2D output plane.
///
/// Slot 4 stores each record as `(x, y, z, attr)`. The dev-menu top-view
/// renderer was originally assumed to consume `(x, z)` (top-down), but
/// several bodies (Drake body 9 / 11 with `count_a x count_b` y-ranges
/// comparable to the x/z scale) show coherent silhouettes only in the
/// `(x, y)` or `(y, z)` projections, suggesting slot 4 holds heterogeneous
/// per-body object-local meshes rather than a single top-down map. Letting
/// the caller pick the axis pair unblocks visual exploration without
/// committing to a topology guess.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn pick(self, r: &Slot4Record) -> i16 {
        match self {
            Axis::X => r.x,
            Axis::Y => r.y,
            Axis::Z => r.z,
        }
    }
}

/// How to interpret the per-body record layout when emitting polylines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolylineMode {
    /// One polyline per group, walking the `count_a` records in order.
    /// (Original "row-major" interpretation.)
    RowMajor,
    /// One polyline per record-slot, walking that slot's value across
    /// every group. The result is `count_a` polylines of `count_b`
    /// vertices each. Matches the slot-4 body 12 layout where record
    /// X-values are fixed per record-slot and Z varies per group -
    /// the records trace `count_a` parallel "scan lines" across the
    /// continent.
    ColumnMajor,
    /// Pair-wise: every two consecutive records form one line segment.
    /// With `count_a = 10` that's 5 segments per group. Hypothesis for
    /// the slot-4 contour bodies (12/13): each group is `count_a / 2`
    /// independent edge pairs, not a chained polyline.
    PairWise,
    /// Quad-mesh grid: each body is a `count_a` x `count_b` vertex grid
    /// (record `[g * count_a + k]` is the `(k, g)` cell). Draws both
    /// row edges (`(k, g) -> (k + 1, g)`) and column edges
    /// (`(k, g) -> (k, g + 1)`). Matches the slot-4 body-12 layout
    /// where consecutive groups share fixed X-bands across `count_a / 2`
    /// vertex pairs and Y varies as terrain elevation - i.e. the body
    /// tests the (falsified) coarse-heightfield-mesh reading.
    Grid,
}

/// Top-down wireframe-line emission options.
#[derive(Clone, Debug)]
pub struct WireframeOptions {
    /// Skip groups where every record is `(0, 0, 0, *)` (or `(0, 0, 0, 0)`
    /// after attr is stripped). Many bodies have placeholder zero-records
    /// padding the table.
    pub strip_zero_records: bool,
    /// Skip a body where every group is byte-identical to the first
    /// (Drake body 8 = 3 identical groups, reserved/padding).
    pub skip_identical_group_bodies: bool,
    /// Close each polyline back to its first vertex. False for the
    /// degenerate-polyline bodies (3, 4, 14 in Drake), true for the
    /// contour bodies.
    pub close_polylines: bool,
    /// Polyline layout interpretation. See [`PolylineMode`].
    pub mode: PolylineMode,
    /// 2D-projection axis pair: `(horizontal, vertical)` axis of the
    /// output plane. Default is `(X, Z)` (top-down view). See [`Axis`].
    pub axes: (Axis, Axis),
}

impl Default for WireframeOptions {
    fn default() -> Self {
        Self {
            strip_zero_records: true,
            skip_identical_group_bodies: true,
            // Slot-4 groups are OPEN polylines (the inspection default; scan
            // lines), not closed polygons. Closing them adds a spurious
            // diagonal from the last vertex back to the first that
            // visually scrambles the rendered shape. Keep them open by
            // default - callers that want a closed polygon can flip
            // this to `true`.
            close_polylines: false,
            // RowMajor is what the live WebGL view uses today. Both
            // mode variants are visually noisy because the per-group
            // record layout isn't a simple polyline (see body 12: each
            // group is 5 (left, right) edge pairs under one (falsified) reading,
            // not a 10-vertex polyline). Use [`record_points`] for a
            // topology-free dump.
            mode: PolylineMode::RowMajor,
            // (X, Z) preserves the historical top-down behaviour.
            axes: (Axis::X, Axis::Z),
        }
    }
}

/// Emit a top-down (X-Z) wireframe-line list from a parsed slot 4.
///
/// Polyline construction follows [`WireframeOptions::mode`]:
///   - `RowMajor` walks each group's `count_a` records in order;
///   - `ColumnMajor` walks each record-slot's value across all groups.
///
/// Zero records are stripped per [`WireframeOptions::strip_zero_records`].
pub fn top_down_lines(slot: &KingdomSlot4, opts: &WireframeOptions) -> Vec<WireframeLine> {
    let mut out = Vec::new();
    for body in &slot.bodies {
        if opts.skip_identical_group_bodies && body_groups_identical(body) {
            continue;
        }
        let ca = body.count_a as usize;
        let cb = body.count_b as usize;
        match opts.mode {
            PolylineMode::RowMajor => {
                if ca < 2 {
                    continue;
                }
                for (g, group) in body.groups().enumerate() {
                    emit_polyline(&mut out, body.index as u8, g as u16, group, opts);
                }
            }
            PolylineMode::ColumnMajor => {
                if cb < 2 {
                    continue;
                }
                // For each record-slot k in [0..ca), gather its value
                // across all groups [0..cb) and emit as one polyline.
                for k in 0..ca {
                    let strand: Vec<Slot4Record> =
                        (0..cb).map(|g| body.records[g * ca + k]).collect();
                    emit_polyline(&mut out, body.index as u8, k as u16, &strand, opts);
                }
            }
            PolylineMode::PairWise => {
                if ca < 2 {
                    continue;
                }
                let (ah, av) = opts.axes;
                for (g, group) in body.groups().enumerate() {
                    for pair in group.as_chunks::<2>().0 {
                        let a = pair[0];
                        let b = pair[1];
                        if opts.strip_zero_records
                            && a.x == 0
                            && a.y == 0
                            && a.z == 0
                            && b.x == 0
                            && b.y == 0
                            && b.z == 0
                        {
                            continue;
                        }
                        let (a0, b0) = (ah.pick(&a), av.pick(&a));
                        let (a1, b1) = (ah.pick(&b), av.pick(&b));
                        if a0 == a1 && b0 == b1 {
                            continue;
                        }
                        out.push(WireframeLine {
                            body_index: body.index as u8,
                            group_index: g as u16,
                            x0: a0,
                            z0: b0,
                            x1: a1,
                            z1: b1,
                        });
                    }
                }
            }
            PolylineMode::Grid => {
                if ca < 2 && cb < 2 {
                    continue;
                }
                let (ah, av) = opts.axes;
                let is_zero = |r: &Slot4Record| r.x == 0 && r.y == 0 && r.z == 0;
                let push =
                    |out: &mut Vec<WireframeLine>, g: usize, a: Slot4Record, b: Slot4Record| {
                        if opts.strip_zero_records && (is_zero(&a) || is_zero(&b)) {
                            return;
                        }
                        let (a0, b0) = (ah.pick(&a), av.pick(&a));
                        let (a1, b1) = (ah.pick(&b), av.pick(&b));
                        if a0 == a1 && b0 == b1 {
                            return;
                        }
                        out.push(WireframeLine {
                            body_index: body.index as u8,
                            group_index: g as u16,
                            x0: a0,
                            z0: b0,
                            x1: a1,
                            z1: b1,
                        });
                    };
                // Row edges: (k, g) -> (k+1, g)
                if ca >= 2 {
                    for g in 0..cb {
                        for k in 0..ca - 1 {
                            let a = body.records[g * ca + k];
                            let b = body.records[g * ca + k + 1];
                            push(&mut out, g, a, b);
                        }
                    }
                }
                // Column edges: (k, g) -> (k, g+1)
                if cb >= 2 {
                    for g in 0..cb - 1 {
                        for k in 0..ca {
                            let a = body.records[g * ca + k];
                            let b = body.records[(g + 1) * ca + k];
                            push(&mut out, g, a, b);
                        }
                    }
                }
            }
        }
    }
    out
}

/// Emit one polyline (consecutive pairs as line segments) into `out`,
/// honoring strip-zero and close-polyline options. Pulled out so both
/// `RowMajor` and `ColumnMajor` can share the same emission logic.
fn emit_polyline(
    out: &mut Vec<WireframeLine>,
    body_index: u8,
    group_index: u16,
    records: &[Slot4Record],
    opts: &WireframeOptions,
) {
    let (ah, av) = opts.axes;
    let mut pts: Vec<(i16, i16)> = records
        .iter()
        .filter(|r| !(opts.strip_zero_records && r.x == 0 && r.y == 0 && r.z == 0))
        .map(|r| (ah.pick(r), av.pick(r)))
        .collect();
    if pts.len() < 2 {
        return;
    }
    if opts.close_polylines && pts.first() != pts.last() {
        pts.push(pts[0]);
    }
    for w in pts.windows(2) {
        let (x0, z0) = w[0];
        let (x1, z1) = w[1];
        if x0 == x1 && z0 == z1 {
            continue;
        }
        out.push(WireframeLine {
            body_index,
            group_index,
            x0,
            z0,
            x1,
            z1,
        });
    }
}

/// Emit one point per non-zero record across every body, tagged with
/// its body index (for color routing) and group index. No polyline
/// topology is assumed - the caller decides how to render each point.
/// Useful for the disc-vs-RAM validation viewer where any imposed
/// polyline interpretation risks hiding format bugs.
pub fn record_points(slot: &KingdomSlot4, opts: &WireframeOptions) -> Vec<(u8, u16, i16, i16)> {
    let mut out = Vec::new();
    let (ah, av) = opts.axes;
    for body in &slot.bodies {
        if opts.skip_identical_group_bodies && body_groups_identical(body) {
            continue;
        }
        let ca = body.count_a as usize;
        for (g, group) in body.groups().enumerate() {
            for (k, r) in group.iter().enumerate() {
                if opts.strip_zero_records && r.x == 0 && r.y == 0 && r.z == 0 {
                    continue;
                }
                let _ = k;
                let _ = ca;
                out.push((body.index as u8, g as u16, ah.pick(r), av.pick(r)));
            }
        }
    }
    out
}

/// One 3D wireframe segment: both endpoints in the body's object-local
/// model space, tagged with the source `body_index` and `kind` so a
/// renderer can colour by body class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment3d {
    pub body_index: u8,
    pub kind: u16,
    pub a: [i16; 3],
    pub b: [i16; 3],
}

/// Emit a full-3D wireframe segment list from a parsed slot 4, preserving
/// the model-space `(x, y, z)` of every endpoint (unlike [`top_down_lines`],
/// which flattens to a 2D axis pair).
///
/// Topology is the historical group-polyline convention
/// ([`PolylineMode::RowMajor`]): consecutive entries within one frame form a
/// polyline. **These are not coordinates.** The `(x, y, z)` fields are raw
/// `i16` slices that straddle the entry's packed 12-bit nibble boundaries, so
/// the output is a byte-inspection plot, not geometry. Use
/// [`translation_path_segments`] for the decoded curve.
/// Honors [`WireframeOptions::strip_zero_records`],
/// [`WireframeOptions::skip_identical_group_bodies`], and
/// [`WireframeOptions::close_polylines`]; [`WireframeOptions::axes`] and
/// [`WireframeOptions::mode`] are ignored (the full 3D coordinate is always
/// kept and the layout is always row-major).
pub fn wireframe_segments_3d(slot: &KingdomSlot4, opts: &WireframeOptions) -> Vec<Segment3d> {
    let is_zero = |r: &Slot4Record| r.x == 0 && r.y == 0 && r.z == 0;
    let mut out = Vec::new();
    for body in &slot.bodies {
        if opts.skip_identical_group_bodies && body_groups_identical(body) {
            continue;
        }
        if (body.count_a as usize) < 2 {
            continue;
        }
        for group in body.groups() {
            let mut pts: Vec<[i16; 3]> = group
                .iter()
                .filter(|r| !(opts.strip_zero_records && is_zero(r)))
                .map(|r| [r.x, r.y, r.z])
                .collect();
            if pts.len() < 2 {
                continue;
            }
            if opts.close_polylines && pts.first() != pts.last() {
                pts.push(pts[0]);
            }
            for w in pts.windows(2) {
                if w[0] == w[1] {
                    continue;
                }
                out.push(Segment3d {
                    body_index: body.index as u8,
                    kind: body.kind,
                    a: w[0],
                    b: w[1],
                });
            }
        }
    }
    out
}

/// Emit each animated object's **translation path** across its clip: for every
/// body and every part, one polyline through the decoded 12-bit translations
/// of frames `0..frame_count`.
///
/// This is the only geometric reading of slot 4 that is actually in the bytes.
/// The values are object-local offsets in model-space units (the corpus range
/// is about `-541..384`), transformed by the actor's matrix at draw time - so
/// a path is a motion curve relative to the actor origin, not a world-space
/// position. `Segment3d::kind` carries the body's `rate` field, as it does for
/// [`wireframe_segments_3d`]; `body_index` is the body, and the part index is
/// recoverable from the emission order (parts ascend within a body).
///
/// Degenerate clips (a single frame, or a part whose translation never moves)
/// emit nothing.
pub fn translation_path_segments(slot: &KingdomSlot4) -> Vec<Segment3d> {
    let mut out = Vec::new();
    for body in &slot.bodies {
        let parts = body.part_count();
        let frames = body.frame_count();
        if parts == 0 || frames < 2 {
            continue;
        }
        for part in 0..parts {
            let pts: Vec<[i16; 3]> = (0..frames)
                .filter_map(|f| body.entry(f, part))
                .map(|r| {
                    let t = r.transform();
                    [t.tx, t.ty, t.tz]
                })
                .collect();
            for w in pts.windows(2) {
                if w[0] == w[1] {
                    continue;
                }
                out.push(Segment3d {
                    body_index: body.index as u8,
                    kind: body.kind,
                    a: w[0],
                    b: w[1],
                });
            }
        }
    }
    out
}

fn body_groups_identical(body: &Slot4Body) -> bool {
    let cb = body.count_b as usize;
    let ca = body.count_a as usize;
    if cb < 2 || ca == 0 {
        return false;
    }
    let first = &body.records[..ca];
    (1..cb).all(|g| &body.records[g * ca..(g + 1) * ca] == first)
}

/// Top-down (X-Z) bounding box across every record in every body.
/// Returns `(xmin, zmin, xmax, zmax)` or `None` if `slot` is empty.
pub fn xz_bounds(slot: &KingdomSlot4) -> Option<(i16, i16, i16, i16)> {
    axis_bounds(slot, Axis::X, Axis::Z)
}

/// Bounding box on the given axis pair across every record in every body.
/// Returns `(amin, bmin, amax, bmax)` or `None` if `slot` is empty.
pub fn axis_bounds(slot: &KingdomSlot4, ah: Axis, av: Axis) -> Option<(i16, i16, i16, i16)> {
    let mut amin = i16::MAX;
    let mut bmin = i16::MAX;
    let mut amax = i16::MIN;
    let mut bmax = i16::MIN;
    let mut any = false;
    for b in &slot.bodies {
        for r in &b.records {
            if r.x == 0 && r.y == 0 && r.z == 0 {
                continue;
            }
            any = true;
            let a = ah.pick(r);
            let v = av.pick(r);
            amin = amin.min(a);
            bmin = bmin.min(v);
            amax = amax.max(a);
            bmax = bmax.max(v);
        }
    }
    if any {
        Some((amin, bmin, amax, bmax))
    } else {
        None
    }
}

/// Per-axis range `(min, max)` for a single body. Skips zero records.
/// Returns `None` if every record is zero.
pub fn body_axis_range(body: &Slot4Body, axis: Axis) -> Option<(i16, i16)> {
    let mut lo = i16::MAX;
    let mut hi = i16::MIN;
    let mut any = false;
    for r in &body.records {
        if r.x == 0 && r.y == 0 && r.z == 0 {
            continue;
        }
        any = true;
        let v = axis.pick(r);
        lo = lo.min(v);
        hi = hi.max(v);
    }
    if any { Some((lo, hi)) } else { None }
}
