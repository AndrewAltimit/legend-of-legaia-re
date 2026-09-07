//! Slot-4 container parser. Each body is one **animation clip**.
//!
//! Format reference: [`docs/formats/world-map-overlay.md`].
//!
//! Slot 4 of each kingdom bundle (PROT entries 0086 / 0245 / 0392) is an
//! ordinary asset-type-`0x05` ("MOVE") ANM container - the same shape every
//! field scene carries - holding the world-map scene's actor animation clips.
//! It is **not** a mesh library, not terrain and not 2D contours; the geometry
//! an animated actor poses is a `DAT_8007C018` pool TMD chosen by the actor,
//! and slot 4 supplies only the per-frame transform of each of that mesh's
//! objects.
//!
//! Layout:
//!
//! ```text
//! [u32 count]
//! [u32 byte_offsets[count]]   ; absolute byte offsets into the decoded payload
//! [body 0 ..]
//! [body k: u8 part_count, u8 flags, u16 frame_count,
//!          u16 marker = 0x080C, u16 rate,
//!          entry[frame_count * part_count],   ; 8 bytes each, FRAME-major
//!          8-byte zero trailer]
//! ```
//!
//! One entry is a rigid transform - three packed **12-bit signed**
//! translation components in bytes 0..4 plus three **8-bit** rotation angles
//! in bytes 5..7 (`angle = byte << 4`, 4096 = one turn). Byte 4's high nibble
//! is reserved and is zero in every entry on the disc. Decoder:
//! `FUN_8001BE80`, called per part from the animated-actor renderer
//! `FUN_8001B964`, which resolves the frame as
//! `(i16)actor[+0x68] >> 4` and the entry as
//! `body + 8 + (frame * part_count + part) * 8`.
//!
//! `flags` bit 0 enables sub-frame interpolation against the next frame's
//! entry (weight = `actor[+0x68] & 0xF`), and `rate` (1 / 2 / 4) is the
//! divisor in the clip-cursor step. The clip an actor plays is selected
//! 1-based: `record = base + *(u32*)(base + (actor[+0x5C] & 0x3FF) * 4)` in
//! `FUN_800204F8`, out of the type-`0x05` buffer pointer `_DAT_8007B888`.
//!
//! NOTE: two historical readings of these bytes are **falsified** and their
//! scaffolding survives below only as byte-inspection aids. (1) That each
//! body's groups are polylines whose top-down (X-Z) projection traces
//! continent coastlines. (2) That each 8-byte entry is a GTE vertex
//! `(i16 x, y, z, attr)` indexed by an unpinned "cluster-A command stream" -
//! no such stream exists, the entry's real field boundaries fall on nibbles
//! rather than `i16`s, and the `attr` half is the Y/Z rotation pair, read
//! every frame. The `top_down_*` / `Wireframe*` / `record_points` helpers plot
//! raw `i16` field pairs that straddle those boundaries: they are useful for
//! diffing bytes, never for geometry. [`translation_path_segments`] is the
//! decoded curve.

/// One 8-byte slot-4 entry, kept in its historical four-`i16` view.
///
/// The four fields are a **byte view**, not a semantic one: the real entry is
/// three packed 12-bit translations plus three 8-bit angles, so `x` mixes the
/// translation X low byte with the translation Y low byte, and `attr` is the
/// `(rotY, rotZ)` pair. Use [`Slot4Record::transform`] for the decoded values;
/// these fields exist so a caller can round-trip the bytes and so the
/// wireframe-era helpers keep compiling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Slot4Record {
    /// Bytes 0..2 as `i16` - translation X low byte | translation Y low byte.
    pub x: i16,
    /// Bytes 2..4 as `i16` - the packed X/Y high nibbles | translation Z low
    /// byte.
    pub y: i16,
    /// Bytes 4..6 as `i16` - the Z high nibble | rotation X.
    pub z: i16,
    /// Bytes 6..8 as `i16` - rotation Y | rotation Z. Named `attr` when it was
    /// read as a non-coordinate 4th vertex field; it is neither unused nor a
    /// coordinate.
    pub attr: i16,
}

/// One decoded slot-4 entry: the pose of one mesh object in one frame.
///
/// Translations are 12-bit signed model-space units; rotations are PSX angle
/// units (4096 = one full turn), always a multiple of 16 because they are
/// stored as a byte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Slot4Transform {
    pub tx: i16,
    pub ty: i16,
    pub tz: i16,
    pub rx: i16,
    pub ry: i16,
    pub rz: i16,
    /// Byte 4's high nibble - reserved, zero in every entry on the disc, and
    /// never read by the runtime decoder. Parsed so a mismatch is visible.
    pub reserved: u8,
}

impl Slot4Record {
    /// The entry's raw 8 bytes, little-endian, exactly as they sit on disc.
    pub fn raw_bytes(&self) -> [u8; 8] {
        let mut out = [0u8; 8];
        out[0..2].copy_from_slice(&self.x.to_le_bytes());
        out[2..4].copy_from_slice(&self.y.to_le_bytes());
        out[4..6].copy_from_slice(&self.z.to_le_bytes());
        out[6..8].copy_from_slice(&self.attr.to_le_bytes());
        out
    }

    /// Decode the entry the way `FUN_8001BE80` does.
    pub fn transform(&self) -> Slot4Transform {
        let b = self.raw_bytes();
        let s12 = |v: u16| -> i16 {
            if v & 0x800 != 0 {
                (v | 0xF000) as i16
            } else {
                v as i16
            }
        };
        Slot4Transform {
            tx: s12(b[0] as u16 | ((b[2] as u16 & 0x0F) << 8)),
            ty: s12(b[1] as u16 | ((b[2] as u16 & 0xF0) << 4)),
            tz: s12(b[3] as u16 | ((b[4] as u16 & 0x0F) << 8)),
            rx: (b[5] as i16) << 4,
            ry: (b[6] as i16) << 4,
            rz: (b[7] as i16) << 4,
            reserved: b[4] >> 4,
        }
    }
}

/// One sub-body of slot 4 - a single animation clip.
#[derive(Clone, Debug)]
pub struct Slot4Body {
    /// Body index within the outer pack. The clip id an actor plays is this
    /// plus one (`FUN_800204F8` indexes the offset table 1-based).
    pub index: usize,
    /// Header `+0x00`: objects posed per frame. Must equal the `nobj` of the
    /// actor's pool TMD or `FUN_8001B964` skips the whole draw
    /// (`bne v1,a0` at `0x8001BAF0`).
    pub count_a: u8,
    /// Header `+0x01`: flag byte. Bit 0 enables sub-frame interpolation.
    pub flag_a: u8,
    /// Header `+0x02`: clip length in frames (low byte of a `u16`).
    pub count_b: u8,
    /// Header `+0x03`: the frame count's high byte. Zero in every body on the
    /// disc.
    pub flag_b: u8,
    /// Constant `0x080C` - the ANM record marker. Treated as a magic check.
    pub marker: u16,
    /// Header `+0x06`: sub-frame divisor (`1`, `2` or `4`); only its low byte
    /// is read, and only when `flag_a & 1`.
    pub kind: u16,
    /// `part_count * frame_count` entries, laid out **frame-major**: the entry
    /// for `(frame, part)` is at index `frame * part_count + part`.
    pub records: Vec<Slot4Record>,
}

impl Slot4Body {
    /// Iterate the clip's frames. Each frame is a slice of `part_count`
    /// entries.
    ///
    /// (Named `groups` when a "group" was believed to be a polyline; a group
    /// is one frame.)
    pub fn groups(&self) -> impl Iterator<Item = &[Slot4Record]> {
        let ca = self.count_a as usize;
        let cb = self.count_b as usize;
        (0..cb).map(move |g| &self.records[g * ca..(g + 1) * ca])
    }

    /// Objects posed per frame (header `+0x00`).
    pub fn part_count(&self) -> usize {
        self.count_a as usize
    }

    /// Clip length in frames (header `+0x02`, with `+0x03` as its high byte).
    pub fn frame_count(&self) -> usize {
        self.count_b as usize | ((self.flag_b as usize) << 8)
    }

    /// Whether the runtime blends between consecutive frames (header `+0x01`
    /// bit 0).
    pub fn interpolates(&self) -> bool {
        self.flag_a & 1 != 0
    }

    /// The clip-cursor sub-frame divisor (header `+0x06`, low byte).
    pub fn subframe_divisor(&self) -> u8 {
        self.kind as u8
    }

    /// The entry for one `(frame, part)` pair, frame-major.
    pub fn entry(&self, frame: usize, part: usize) -> Option<Slot4Record> {
        if part >= self.part_count() || frame >= self.frame_count() {
            return None;
        }
        self.records.get(frame * self.part_count() + part).copied()
    }
}

/// Parsed slot-4 payload.
#[derive(Clone, Debug, Default)]
pub struct KingdomSlot4 {
    pub bodies: Vec<Slot4Body>,
}

/// Error parsing slot 4.
#[derive(Debug)]
pub enum Slot4Error {
    HeaderTooSmall(usize),
    ImplausibleCount(u32),
    BodyOob {
        body: usize,
        offset: usize,
        len: usize,
    },
    BodySizeMismatch {
        body: usize,
        got: usize,
        want: usize,
    },
    BadMarker {
        body: usize,
        marker: u16,
    },
}

impl std::fmt::Display for Slot4Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HeaderTooSmall(n) => write!(f, "buffer too small for header ({n} bytes)"),
            Self::ImplausibleCount(c) => write!(f, "implausible body count {c}"),
            Self::BodyOob { body, offset, len } => {
                write!(f, "body {body}: offset {offset} oob (len {len})")
            }
            Self::BodySizeMismatch { body, got, want } => {
                write!(f, "body {body}: payload size {got}, expected {want}")
            }
            Self::BadMarker { body, marker } => {
                write!(f, "body {body}: marker 0x{marker:04X} != 0x080C")
            }
        }
    }
}

impl std::error::Error for Slot4Error {}

/// Parse a slot-4 payload (already LZS-decoded).
pub fn parse(decoded: &[u8]) -> Result<KingdomSlot4, Slot4Error> {
    if decoded.len() < 4 {
        return Err(Slot4Error::HeaderTooSmall(decoded.len()));
    }
    let count = u32::from_le_bytes(decoded[0..4].try_into().unwrap());
    // Empirical bound: Drake / Sebucus / Karisto all have count == 15.
    // 256 is a generous ceiling that still excludes corrupted headers.
    if !(1..=256).contains(&count) {
        return Err(Slot4Error::ImplausibleCount(count));
    }
    let count = count as usize;
    let header_bytes = 4 + count * 4;
    if decoded.len() < header_bytes {
        return Err(Slot4Error::HeaderTooSmall(decoded.len()));
    }
    let mut byte_offsets = Vec::with_capacity(count);
    for k in 0..count {
        let off = u32::from_le_bytes(decoded[4 + 4 * k..8 + 4 * k].try_into().unwrap()) as usize;
        byte_offsets.push(off);
    }
    let mut bodies = Vec::with_capacity(count);
    for k in 0..count {
        let start = byte_offsets[k];
        let end = byte_offsets.get(k + 1).copied().unwrap_or(decoded.len());
        if start > decoded.len() || end > decoded.len() || end < start {
            return Err(Slot4Error::BodyOob {
                body: k,
                offset: start,
                len: decoded.len(),
            });
        }
        let body = &decoded[start..end];
        if body.len() < 8 {
            return Err(Slot4Error::BodySizeMismatch {
                body: k,
                got: body.len(),
                want: 8,
            });
        }
        let count_a = body[0];
        let flag_a = body[1];
        let count_b = body[2];
        let flag_b = body[3];
        let marker = u16::from_le_bytes(body[4..6].try_into().unwrap());
        let kind = u16::from_le_bytes(body[6..8].try_into().unwrap());
        if marker != 0x080C {
            return Err(Slot4Error::BadMarker { body: k, marker });
        }
        let n_records = count_a as usize * count_b as usize;
        // Body payload = 8-byte header + n_records * 8-byte records + 8-byte
        // trailer. The 8-byte trailer is always zeroed in Drake.
        let need = 8 + n_records * 8 + 8;
        if body.len() < need {
            return Err(Slot4Error::BodySizeMismatch {
                body: k,
                got: body.len(),
                want: need,
            });
        }
        let mut records = Vec::with_capacity(n_records);
        for r in 0..n_records {
            let off = 8 + r * 8;
            let x = i16::from_le_bytes(body[off..off + 2].try_into().unwrap());
            let y = i16::from_le_bytes(body[off + 2..off + 4].try_into().unwrap());
            let z = i16::from_le_bytes(body[off + 4..off + 6].try_into().unwrap());
            // Bytes 6..8: the (rotY, rotZ) pair. Kept in the historical `attr`
            // slot so the byte view round-trips; decode with
            // `Slot4Record::transform`.
            let attr = i16::from_le_bytes(body[off + 6..off + 8].try_into().unwrap());
            records.push(Slot4Record { x, y, z, attr });
        }
        bodies.push(Slot4Body {
            index: k,
            count_a,
            flag_a,
            count_b,
            flag_b,
            marker,
            kind,
            records,
        });
    }
    Ok(KingdomSlot4 { bodies })
}

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
                    for pair in group.chunks_exact(2) {
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

/// One per-body color, RGBA8. Matches `site/js/webgl-tmd.js::wireframeBodyColor`
/// so the standalone PNG renderer agrees visually with the WebGL viewer.
const PALETTE: &[[u8; 4]] = &[
    [0xF2, 0x8C, 0x59, 0xFF], //  0  amber
    [0xA6, 0xD9, 0x73, 0xFF], //  1  lime
    [0x8C, 0xCC, 0xF2, 0xFF], //  2  sky
    [0xF2, 0x8C, 0xF2, 0xFF], //  3  magenta
    [0xF2, 0xCC, 0x73, 0xFF], //  4  gold
    [0xA6, 0x73, 0xF2, 0xFF], //  5  violet
    [0x73, 0xF2, 0xA6, 0xFF], //  6  mint
    [0xF2, 0x73, 0x8C, 0xFF], //  7  rose
    [0x73, 0xA6, 0xF2, 0xFF], //  8  azure
    [0xD9, 0xD9, 0x73, 0xFF], //  9  olive
    [0x73, 0xF2, 0xF2, 0xFF], // 10  aqua
    [0xF2, 0xA6, 0x73, 0xFF], // 11  apricot
    [0x8C, 0xF2, 0xF2, 0xFF], // 12
    [0xF2, 0xF2, 0xA6, 0xFF], // 13
    [0xA6, 0xF2, 0x8C, 0xFF], // 14  chartreuse
    [0xF2, 0x8C, 0xA6, 0xFF], // 15  blush
];

/// Color for body `i`. Wraps if `i >= PALETTE.len()`.
pub fn body_color(i: usize) -> [u8; 4] {
    PALETTE[i % PALETTE.len()]
}

/// Top-down PNG rasterizer for slot-4 wireframe lines.
///
/// Output coordinate system: `x` increases right, `z` increases down
/// (matches the in-game minimap orientation). The world bounds are
/// computed from `xz_bounds(slot)`; the line set is rasterized into a
/// canvas of `(width, height)` with a configurable margin so labels /
/// dots stay inside.
pub struct WireframeRaster {
    pub width: u32,
    pub height: u32,
    /// Margin in pixels around the world bbox (each edge).
    pub margin: u32,
    /// Background color (RGBA8). Default `#0A0A1A` to match the site.
    pub bg: [u8; 4],
    /// Pixel buffer (RGBA8, row-major, length = width * height * 4).
    pub buf: Vec<u8>,
    /// Cached world-bounds: (xmin, zmin, xmax, zmax). All-zero if the
    /// slot is empty (rendering is a no-op in that case).
    pub world_bounds: (i32, i32, i32, i32),
}

impl WireframeRaster {
    /// Create a new raster initialised to `bg`.
    pub fn new(width: u32, height: u32, margin: u32, bg: [u8; 4]) -> Self {
        let mut buf = Vec::with_capacity((width as usize) * (height as usize) * 4);
        for _ in 0..(width * height) {
            buf.extend_from_slice(&bg);
        }
        Self {
            width,
            height,
            margin,
            bg,
            buf,
            world_bounds: (0, 0, 0, 0),
        }
    }

    /// Compute world bounds from the slot's record set, using the X-Z
    /// projection. Use [`Self::set_bounds_from_axes`] for non-default
    /// projections.
    pub fn set_bounds_from(&mut self, slot: &KingdomSlot4) {
        self.set_bounds_from_axes(slot, Axis::X, Axis::Z);
    }

    /// Compute world bounds from the slot's record set on the given
    /// axis pair. Use when rendering a non-default projection so the
    /// raster's viewport matches the data being plotted.
    pub fn set_bounds_from_axes(&mut self, slot: &KingdomSlot4, ah: Axis, av: Axis) {
        match axis_bounds(slot, ah, av) {
            Some((amin, bmin, amax, bmax)) => {
                self.world_bounds = (amin as i32, bmin as i32, amax as i32, bmax as i32);
            }
            None => self.world_bounds = (0, 0, 1, 1),
        }
    }

    /// Override bounds (when rasterizing several kingdoms at once and you
    /// want them sharing a common camera).
    pub fn set_bounds(&mut self, xmin: i32, zmin: i32, xmax: i32, zmax: i32) {
        self.world_bounds = (xmin, zmin, xmax, zmax);
    }

    /// Convert world (x, z) to pixel (px, py). Letter-boxes inside the
    /// margin so the shorter axis doesn't stretch.
    pub fn world_to_pix(&self, x: i32, z: i32) -> (i32, i32) {
        let (xmin, zmin, xmax, zmax) = self.world_bounds;
        let dx = (xmax - xmin).max(1) as f32;
        let dz = (zmax - zmin).max(1) as f32;
        let aw = (self.width as i32 - 2 * self.margin as i32).max(1) as f32;
        let ah = (self.height as i32 - 2 * self.margin as i32).max(1) as f32;
        // Letterbox: keep aspect ratio.
        let scale = (aw / dx).min(ah / dz);
        let pw = (dx * scale) as i32;
        let ph = (dz * scale) as i32;
        let ox = self.margin as i32 + (aw as i32 - pw) / 2;
        let oy = self.margin as i32 + (ah as i32 - ph) / 2;
        let px = ox + (((x - xmin) as f32) * scale) as i32;
        let py = oy + (((z - zmin) as f32) * scale) as i32;
        (px, py)
    }

    /// Plot a single pixel with alpha blending. Out-of-range coords are
    /// clipped.
    pub fn put_px(&mut self, x: i32, y: i32, color: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let off = ((y as u32 * self.width + x as u32) * 4) as usize;
        let sa = color[3] as u16;
        let inv = 255 - sa;
        for (c, src_c) in color.iter().enumerate().take(3) {
            let dst = self.buf[off + c] as u16;
            let src = *src_c as u16;
            self.buf[off + c] = ((dst * inv + src * sa) / 255) as u8;
        }
        // Keep alpha at 255 (opaque output).
    }

    /// Bresenham line in pixel space, blended.
    pub fn line_pix(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: [u8; 4]) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let mut x = x0;
        let mut y = y0;
        loop {
            self.put_px(x, y, color);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Filled circle of `radius` pixels around (cx, cy).
    pub fn dot_pix(&mut self, cx: i32, cy: i32, radius: i32, color: [u8; 4]) {
        let r2 = radius * radius;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= r2 {
                    self.put_px(cx + dx, cy + dy, color);
                }
            }
        }
    }

    /// Rasterize all wireframe lines, one body at a time. Body color
    /// comes from [`body_color`]; alpha is 220 so overlapping lines
    /// brighten the pixel without fully replacing it.
    ///
    /// When `only_body` is `Some(i)`, only body index `i` is rendered.
    /// Render order is body-major: body 0 first, body N-1 last; the
    /// the highest-index body (12 in Drake) therefore paints on top of the
    /// padded inner contours.
    pub fn draw_wireframe(
        &mut self,
        slot: &KingdomSlot4,
        opts: &WireframeOptions,
        only_body: Option<usize>,
    ) {
        let lines = top_down_lines(slot, opts);
        for l in &lines {
            if let Some(i) = only_body
                && l.body_index as usize != i
            {
                continue;
            }
            let mut c = body_color(l.body_index as usize);
            c[3] = 220;
            let (x0, y0) = self.world_to_pix(l.x0 as i32, l.z0 as i32);
            let (x1, y1) = self.world_to_pix(l.x1 as i32, l.z1 as i32);
            self.line_pix(x0, y0, x1, y1, c);
        }
    }

    /// Render every (non-zero) record as a small dot, colored by body.
    /// Polyline-topology-free; the truest one-shot view of the raw
    /// data. Optional `only_body` mirrors [`Self::draw_wireframe`].
    pub fn draw_points(
        &mut self,
        slot: &KingdomSlot4,
        opts: &WireframeOptions,
        only_body: Option<usize>,
        radius: i32,
    ) {
        let pts = record_points(slot, opts);
        for (body, _group, x, z) in &pts {
            if let Some(i) = only_body
                && (*body as usize) != i
            {
                continue;
            }
            let mut c = body_color(*body as usize);
            c[3] = 255;
            let (px, py) = self.world_to_pix(*x as i32, *z as i32);
            self.dot_pix(px, py, radius, c);
        }
    }

    /// Plot a placement scatter overlay. Each dot is a small filled
    /// circle with a halo so it stays visible against busy wireframes.
    pub fn draw_placements(&mut self, placements: &[(i32, i32)], color: [u8; 4], radius: i32) {
        for &(x, z) in placements {
            let (px, py) = self.world_to_pix(x, z);
            // Dark halo
            self.dot_pix(px, py, radius + 1, [0x00, 0x00, 0x00, 0xC0]);
            // Bright core
            self.dot_pix(px, py, radius, color);
        }
    }

    /// Encode the buffer as a PNG into `out`.
    pub fn encode_png<W: std::io::Write>(&self, out: W) -> std::io::Result<()> {
        let mut enc = png::Encoder::new(out, self.width, self.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc
            .write_header()
            .map_err(|e| std::io::Error::other(format!("png header: {e}")))?;
        writer
            .write_image_data(&self.buf)
            .map_err(|e| std::io::Error::other(format!("png data: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal-valid slot-4 payload: one body, 1 group, 3 records.
    fn synthetic_one_body() -> Vec<u8> {
        let mut buf = Vec::new();
        // count = 1
        buf.extend_from_slice(&1u32.to_le_bytes());
        // offsets[0] = 8 (right after the header)
        buf.extend_from_slice(&8u32.to_le_bytes());
        // body: count_a=3 flag_a=0 count_b=1 flag_b=0 marker=0x080C kind=2
        buf.extend_from_slice(&[3, 0, 1, 0, 0x0C, 0x08, 0x02, 0x00]);
        // records: (0, 0, 0), (100, 0, 200), (50, 0, 300)
        for (x, y, z, a) in [(0, 0, 0, 0), (100, 0, 200, 1), (50, 0, 300, 1)] {
            buf.extend_from_slice(&(x as i16).to_le_bytes());
            buf.extend_from_slice(&(y as i16).to_le_bytes());
            buf.extend_from_slice(&(z as i16).to_le_bytes());
            buf.extend_from_slice(&(a as i16).to_le_bytes());
        }
        // trailer
        buf.extend_from_slice(&[0u8; 8]);
        buf
    }

    #[test]
    fn parse_synthetic_body() {
        let buf = synthetic_one_body();
        let slot = parse(&buf).expect("parse");
        assert_eq!(slot.bodies.len(), 1);
        let b = &slot.bodies[0];
        assert_eq!(b.count_a, 3);
        assert_eq!(b.count_b, 1);
        assert_eq!(b.marker, 0x080C);
        assert_eq!(b.kind, 2);
        assert_eq!(b.records.len(), 3);
        assert_eq!(b.records[1].x, 100);
        assert_eq!(b.records[2].z, 300);
    }

    #[test]
    fn rejects_bad_marker() {
        let mut buf = synthetic_one_body();
        buf[8 + 4] = 0x00; // clobber marker low byte
        match parse(&buf) {
            Err(Slot4Error::BadMarker { body, .. }) => assert_eq!(body, 0),
            other => panic!("expected BadMarker, got {other:?}"),
        }
    }

    #[test]
    fn top_down_lines_strip_zeros() {
        let buf = synthetic_one_body();
        let slot = parse(&buf).unwrap();
        // Default options leave polylines open: 3 records → strip 1 zero
        // → 2 vertices → 1 line segment (no closing diagonal).
        let lines = top_down_lines(&slot, &WireframeOptions::default());
        assert_eq!(lines.len(), 1);
        assert_eq!((lines[0].x0, lines[0].z0), (100, 200));
        assert_eq!((lines[0].x1, lines[0].z1), (50, 300));

        // Closed-polyline mode adds the back-edge.
        let closed = top_down_lines(
            &slot,
            &WireframeOptions {
                close_polylines: true,
                ..WireframeOptions::default()
            },
        );
        assert_eq!(closed.len(), 2);
        assert_eq!((closed[1].x0, closed[1].z0), (50, 300));
        assert_eq!((closed[1].x1, closed[1].z1), (100, 200));
    }

    #[test]
    fn xz_bounds_skips_zeros() {
        let buf = synthetic_one_body();
        let slot = parse(&buf).unwrap();
        let b = xz_bounds(&slot).unwrap();
        assert_eq!(b, (50, 200, 100, 300));
    }

    #[test]
    fn wireframe_segments_3d_keep_full_coords() {
        let buf = synthetic_one_body();
        let slot = parse(&buf).unwrap();
        // 3 records → strip the (0,0,0) record → 2 vertices → 1 segment.
        let segs = wireframe_segments_3d(&slot, &WireframeOptions::default());
        assert_eq!(segs.len(), 1);
        let s = segs[0];
        assert_eq!(s.body_index, 0);
        assert_eq!(s.kind, 2);
        // Full 3D endpoints are preserved (top_down_lines would flatten Y).
        assert_eq!(s.a, [100, 0, 200]);
        assert_eq!(s.b, [50, 0, 300]);

        // Closing the polyline adds the back-edge in 3D too.
        let closed = wireframe_segments_3d(
            &slot,
            &WireframeOptions {
                close_polylines: true,
                ..WireframeOptions::default()
            },
        );
        assert_eq!(closed.len(), 2);
        assert_eq!(closed[1].a, [50, 0, 300]);
        assert_eq!(closed[1].b, [100, 0, 200]);
    }

    /// Two frames x one part, with hand-packed entries covering the sign
    /// extension, the shared nibble byte and the reserved high nibble.
    fn synthetic_clip(entries: &[[u8; 8]], parts: u8, frames: u8, flags: u8, rate: u16) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&8u32.to_le_bytes());
        buf.push(parts);
        buf.push(flags);
        buf.push(frames);
        buf.push(0);
        buf.extend_from_slice(&0x080Cu16.to_le_bytes());
        buf.extend_from_slice(&rate.to_le_bytes());
        for e in entries {
            buf.extend_from_slice(e);
        }
        buf.extend_from_slice(&[0u8; 8]);
        buf
    }

    #[test]
    fn transform_decodes_packed_fields() {
        // tx = 0x123 -> 291, ty = -0x001 (0xFFF) -> -1, tz = -0x800 -> -2048.
        //   byte0 = 0x23 (tx lo), byte1 = 0xFF (ty lo),
        //   byte2 = (ty hi << 4) | tx hi = (0xF << 4) | 0x1 = 0xF1
        //   byte3 = 0x00 (tz lo), byte4 = (reserved << 4) | tz hi = 0x08
        //   bytes 5..7 = rotation bytes.
        let e = [0x23, 0xFF, 0xF1, 0x00, 0x08, 0x01, 0x10, 0xFF];
        let buf = synthetic_clip(&[e], 1, 1, 0, 2);
        let slot = parse(&buf).unwrap();
        let t = slot.bodies[0].records[0].transform();
        assert_eq!((t.tx, t.ty, t.tz), (0x123, -1, -2048));
        assert_eq!((t.rx, t.ry, t.rz), (0x010, 0x100, 0xFF0));
        assert_eq!(t.reserved, 0);
        // The byte view round-trips.
        assert_eq!(slot.bodies[0].records[0].raw_bytes(), e);
    }

    #[test]
    fn header_accessors_and_frame_major_entries() {
        // 2 parts x 2 frames; entry (f, p) carries tx = f*10 + p.
        let mk = |tx: u8| [tx, 0, 0, 0, 0, 0, 0, 0];
        let entries = [mk(0), mk(1), mk(10), mk(11)];
        let buf = synthetic_clip(&entries, 2, 2, 1, 4);
        let slot = parse(&buf).unwrap();
        let b = &slot.bodies[0];
        assert_eq!(b.part_count(), 2);
        assert_eq!(b.frame_count(), 2);
        assert!(b.interpolates());
        assert_eq!(b.subframe_divisor(), 4);
        assert_eq!(b.entry(0, 1).unwrap().transform().tx, 1);
        assert_eq!(b.entry(1, 0).unwrap().transform().tx, 10);
        assert_eq!(b.entry(1, 1).unwrap().transform().tx, 11);
        assert!(b.entry(2, 0).is_none());
        assert!(b.entry(0, 2).is_none());
    }

    #[test]
    fn translation_paths_follow_one_part_across_frames() {
        let mk = |tx: u8| [tx, 0, 0, 0, 0, 0, 0, 0];
        // part 0 moves 0 -> 10 -> 20; part 1 never moves.
        let entries = [mk(0), mk(5), mk(10), mk(5), mk(20), mk(5)];
        let buf = synthetic_clip(&entries, 2, 3, 0, 1);
        let slot = parse(&buf).unwrap();
        let segs = translation_path_segments(&slot);
        // Part 0 emits two segments; part 1 is stationary and emits none.
        assert_eq!(segs.len(), 2);
        assert_eq!((segs[0].a[0], segs[0].b[0]), (0, 10));
        assert_eq!((segs[1].a[0], segs[1].b[0]), (10, 20));
    }
}
