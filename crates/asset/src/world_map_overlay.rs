//! Slot-4 (world-map overlay outlines) parser.
//!
//! Format reference: [`docs/formats/world-map-overlay.md`].
//!
//! Slot 4 of each kingdom bundle (PROT entries 0085 / 0244 / 0391) is the
//! dev-menu top-view wireframe / coastline data - not the textured ground
//! tiles. Layout:
//!
//! ```text
//! [u32 count]
//! [u32 byte_offsets[count]]   ; absolute byte offsets into the decoded payload
//! [body 0 ..]
//! [body k: u8 count_a, u8 flag_a, u8 count_b, u8 flag_b,
//!          u16 marker = 0x080C, u16 kind,
//!          record[count_a * count_b] of (i16 x, i16 y, i16 z, i16 attr),
//!          8-byte trailer]
//! ```
//!
//! Each of the `count_b` groups in a body is a polyline of `count_a` vertices.
//! The top-down (X-Z) projection of those polylines traces continent
//! coastlines (Drake body 12), the ±32 K world boundary frame (Drake body
//! 13), and lower-resolution inner contours.

/// One vertex of a slot-4 polyline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Slot4Record {
    pub x: i16,
    pub y: i16,
    pub z: i16,
    /// Per-record attribute byte. Always 0 for body 4; up to 214 distinct
    /// values in Drake's body 12 (coastline). Probably packs a (tpage, clut)
    /// tag or a zone id; semantic depends on the (still unidentified)
    /// consumer.
    pub attr: i16,
}

/// One sub-body of slot 4.
#[derive(Clone, Debug)]
pub struct Slot4Body {
    /// Body index within the outer pack.
    pub index: usize,
    /// Records per group.
    pub count_a: u8,
    /// Usually 0; observed `1` for Drake body 13 (the boundary frame).
    pub flag_a: u8,
    /// Number of groups.
    pub count_b: u8,
    /// Usually 0.
    pub flag_b: u8,
    /// Constant `0x080C` across every Drake body. Treated as a magic check.
    pub marker: u16,
    /// Body kind. Observed values: 1, 2, 4. Semantic not yet pinned to a
    /// draw routine.
    pub kind: u16,
    /// `count_a * count_b` vertex records, laid out group-major
    /// (group g's records start at `g * count_a`).
    pub records: Vec<Slot4Record>,
}

impl Slot4Body {
    /// Iterate the body's polyline groups. Each group is a slice of
    /// `count_a` records.
    pub fn groups(&self) -> impl Iterator<Item = &[Slot4Record]> {
        let ca = self.count_a as usize;
        let cb = self.count_b as usize;
        (0..cb).map(move |g| &self.records[g * ca..(g + 1) * ca])
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
}

impl Default for WireframeOptions {
    fn default() -> Self {
        Self {
            strip_zero_records: true,
            skip_identical_group_bodies: true,
            close_polylines: true,
        }
    }
}

/// Emit a top-down (X-Z) wireframe-line list from a parsed slot 4.
///
/// One line per consecutive pair of vertices within each group, plus
/// (optionally) one closing segment back to the first vertex. Zero
/// records are stripped per [`WireframeOptions::strip_zero_records`].
pub fn top_down_lines(slot: &KingdomSlot4, opts: &WireframeOptions) -> Vec<WireframeLine> {
    let mut out = Vec::new();
    for body in &slot.bodies {
        if opts.skip_identical_group_bodies && body_groups_identical(body) {
            continue;
        }
        let ca = body.count_a as usize;
        if ca < 2 {
            // Single-vertex groups can't draw a line; skip the body.
            continue;
        }
        for (g, group) in body.groups().enumerate() {
            let mut pts: Vec<(i16, i16)> = group
                .iter()
                .filter(|r| !(opts.strip_zero_records && r.x == 0 && r.y == 0 && r.z == 0))
                .map(|r| (r.x, r.z))
                .collect();
            if pts.len() < 2 {
                continue;
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
                    body_index: body.index as u8,
                    group_index: g as u16,
                    x0,
                    z0,
                    x1,
                    z1,
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
    let mut xmin = i16::MAX;
    let mut zmin = i16::MAX;
    let mut xmax = i16::MIN;
    let mut zmax = i16::MIN;
    let mut any = false;
    for b in &slot.bodies {
        for r in &b.records {
            if r.x == 0 && r.y == 0 && r.z == 0 {
                continue;
            }
            any = true;
            xmin = xmin.min(r.x);
            zmin = zmin.min(r.z);
            xmax = xmax.max(r.x);
            zmax = zmax.max(r.z);
        }
    }
    if any {
        Some((xmin, zmin, xmax, zmax))
    } else {
        None
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
        let lines = top_down_lines(&slot, &WireframeOptions::default());
        // 3 records: (0,0,0), (100,0,200), (50,0,300). After stripping the
        // zero record we have 2 points; close back to first; one line each
        // way = 2 segments.
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0].x0, lines[0].z0), (100, 200));
        assert_eq!((lines[0].x1, lines[0].z1), (50, 300));
        assert_eq!((lines[1].x0, lines[1].z0), (50, 300));
        assert_eq!((lines[1].x1, lines[1].z1), (100, 200));
    }

    #[test]
    fn xz_bounds_skips_zeros() {
        let buf = synthetic_one_body();
        let slot = parse(&buf).unwrap();
        let b = xz_bounds(&slot).unwrap();
        assert_eq!(b, (50, 200, 100, 300));
    }
}
