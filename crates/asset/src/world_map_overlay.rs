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

mod wireframe;
mod wireframe_raster;

pub use wireframe::*;
pub use wireframe_raster::*;

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
