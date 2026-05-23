//! PSX TMD (3D mesh) parser.
//!
//! TMD is Sony's PlayStation 3D model format. Reference: Sony PSX SDK,
//! `libgte` and `libgs` documentation.
//!
//! File layout:
//!
//! ```text
//! Header (12 bytes):
//!   u32 id        // typically 0x00000041; bit 31 (FLIST_BIT) selects
//!                 // pointer addressing mode. Legaia uses 0x80000002.
//!   u32 flags     // usually 0
//!   u32 nobj      // number of objects
//!
//! Object table (28 bytes per object × nobj):
//!   u32 vert_top
//!   u32 n_vert
//!   u32 normal_top
//!   u32 n_normal
//!   u32 prim_top
//!   u32 n_primitive
//!   i32 scale     // signed log2 scale
//!
//! Then the per-object data sections (vertices, normals, primitives) live
//! at the offsets given by the *_top pointers.
//! ```
//!
//! When `FLIST_BIT` is set in `id`, all `*_top` values are byte offsets
//! from the end of the header (i.e., from the start of the object table).
//! When unset, pointers are absolute RAM addresses (used after the runtime
//! patches them in place - irrelevant for static parsing).
//!
//! Vertices and normals are `SVECTOR { i16 x, y, z, pad }` = 8 bytes each.
//!
//! ## TMD vs TMD2 (Legaia dispatcher types 0x02 and 0x09)
//!
//! Legaia's asset dispatcher (`FUN_8001f05c`) routes both `TMD` (type 0x02)
//! and `TMD2` (type 0x09) through the per-submesh validator `FUN_80026b4c`,
//! whose first instruction loads `*buffer` and compares against `0x80000002`
//! (a TMD `id` with `FLIST_BIT` set). The difference is wrapping:
//!
//! - `TMD` (case 2): the loaded payload is a *pack* - `[count, off0, off1, …]`
//!   followed by `count` independent TMD blobs at the listed offsets. The
//!   dispatcher iterates and calls `FUN_80026b4c(buffer + off_i, 0)` per blob.
//!   See [`legaia_prot::timpack`] for the analogous TIM-pack format and
//!   `crates/asset::pack` for the actual TMD-pack walker we use.
//!
//! - `TMD2` (case 9): the loaded payload IS a single bare TMD - the dispatcher
//!   calls `FUN_80026b4c(buffer, 0)` exactly once. **No pack header.** Pass
//!   the raw payload bytes directly to [`parse`].
//!
//! Every TMD we've extracted from in-the-wild streaming-format containers has
//! `nobj == 1`, so [`parse`] is exercised against the TMD2-shape today even
//! though the wrapping route differs.
//!
//! Primitives have a 4-byte header `{ u8 olen, u8 ilen, u8 flag, u8 mode }`
//! followed by `ilen × 4` bytes of primitive data. The `mode` byte encodes
//! topology + shading + texturing flags. We don't decode primitive bodies
//! yet - just walk the headers to validate structure.

use anyhow::{Result, bail};
use serde::Serialize;

pub mod descriptor;
pub mod legaia_prim_probe;
pub mod legaia_prims;
pub mod mesh;
pub mod vram_targeted;

/// FLIST_BIT - when set in `id`, pointers are relative to header end.
pub const FLIST_BIT: u32 = 0x8000_0000;

/// Header size in bytes.
pub const HEADER_SIZE: usize = 12;
/// Object table entry size in bytes.
pub const OBJECT_SIZE: usize = 28;
/// Vertex / normal struct size in bytes.
pub const VECTOR_SIZE: usize = 8;
/// Primitive header size in bytes (olen + ilen + flag + mode).
pub const PRIM_HEADER_SIZE: usize = 4;

#[derive(Debug, Clone, Serialize)]
pub struct Header {
    pub id: u32,
    pub flags: u32,
    pub nobj: u32,
    pub flist_bit_set: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObjectHeader {
    pub vert_top: u32,
    pub n_vert: u32,
    pub normal_top: u32,
    pub n_normal: u32,
    pub prim_top: u32,
    pub n_primitive: u32,
    pub scale: i32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Vector {
    pub x: i16,
    pub y: i16,
    pub z: i16,
    pub _pad: i16,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct PrimHeader {
    pub olen: u8,
    pub ilen: u8,
    pub flag: u8,
    pub mode: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct Object {
    pub header: ObjectHeader,
    pub vertices: Vec<Vector>,
    pub normals: Vec<Vector>,
    /// Primitive section as a raw byte slice (start..end bounds within the
    /// TMD buffer). We don't iterate primitives because Legaia's primitive
    /// layout diverges from PSX SDK; you'd need a Legaia-specific walker.
    pub primitives_byte_offset: usize,
    pub primitives_byte_size: usize,
    /// What the object header claims n_primitive is (informational).
    pub claimed_n_primitive: u32,
    /// Best-effort primitive iteration via the standard PSX SDK header
    /// `(olen, ilen, flag, mode)`. Will be `Some(Ok(...))` if it walks
    /// cleanly, `Some(Err(...))` if it fails partway, or `None` if we
    /// chose not to attempt iteration.
    pub primitives_psx_walk: Option<std::result::Result<Vec<PrimHeader>, String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Tmd {
    pub header: Header,
    pub objects: Vec<Object>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParseStats {
    pub total_vertices: usize,
    pub total_normals: usize,
    pub total_primitives: usize,
    pub total_bytes_consumed: usize,
}

impl Tmd {
    pub fn stats(&self) -> ParseStats {
        let mut s = ParseStats {
            total_vertices: 0,
            total_normals: 0,
            total_primitives: 0,
            total_bytes_consumed: 0,
        };
        for o in &self.objects {
            s.total_vertices += o.vertices.len();
            s.total_normals += o.normals.len();
            s.total_primitives += o.claimed_n_primitive as usize;
            s.total_bytes_consumed += o.vertices.len() * VECTOR_SIZE
                + o.normals.len() * VECTOR_SIZE
                + o.primitives_byte_size;
        }
        s
    }
}

fn read_u32_le(buf: &[u8], off: usize) -> Result<u32> {
    if off + 4 > buf.len() {
        bail!("read_u32 at {} past buffer end ({})", off, buf.len());
    }
    Ok(u32::from_le_bytes(buf[off..off + 4].try_into().unwrap()))
}

fn read_i32_le(buf: &[u8], off: usize) -> Result<i32> {
    Ok(read_u32_le(buf, off)? as i32)
}

fn read_i16_le(buf: &[u8], off: usize) -> Result<i16> {
    if off + 2 > buf.len() {
        bail!("read_i16 at {} past buffer end ({})", off, buf.len());
    }
    Ok(i16::from_le_bytes(buf[off..off + 2].try_into().unwrap()))
}

fn read_vector(buf: &[u8], off: usize) -> Result<Vector> {
    Ok(Vector {
        x: read_i16_le(buf, off)?,
        y: read_i16_le(buf, off + 2)?,
        z: read_i16_le(buf, off + 4)?,
        _pad: read_i16_le(buf, off + 6)?,
    })
}

pub fn parse(buf: &[u8]) -> Result<Tmd> {
    if buf.len() < HEADER_SIZE {
        bail!("buffer too small for TMD header ({} bytes)", buf.len());
    }
    let id = read_u32_le(buf, 0)?;
    let flags = read_u32_le(buf, 4)?;
    let nobj = read_u32_le(buf, 8)?;
    let flist_bit_set = (id & FLIST_BIT) != 0;
    let header = Header {
        id,
        flags,
        nobj,
        flist_bit_set,
    };

    if !flist_bit_set {
        bail!(
            "TMD has FLIST_BIT clear (id=0x{:08X}); pointers are absolute RAM \
             addresses and require runtime fixup. Static parsing not supported.",
            id
        );
    }
    if nobj == 0 {
        return Ok(Tmd {
            header,
            objects: Vec::new(),
        });
    }
    if nobj > 1024 {
        bail!("implausible object count {}", nobj);
    }

    let nobj = nobj as usize;
    let table_end = HEADER_SIZE + nobj * OBJECT_SIZE;
    if table_end > buf.len() {
        bail!(
            "object table ({} entries, ends at {}) overruns buffer ({} bytes)",
            nobj,
            table_end,
            buf.len()
        );
    }

    // With FLIST_BIT, all *_top are byte offsets from `table_end_base`
    // (the end of the 12-byte header, i.e. the start of the object table).
    // Note: this is the typical interpretation; some refs say "end of
    // object table" - testing on real Legaia TMDs confirms it's end-of-header.
    let ptr_base: usize = HEADER_SIZE;

    let mut objects = Vec::with_capacity(nobj);
    for i in 0..nobj {
        let off = HEADER_SIZE + i * OBJECT_SIZE;
        let vert_top = read_u32_le(buf, off)?;
        let n_vert = read_u32_le(buf, off + 4)?;
        let normal_top = read_u32_le(buf, off + 8)?;
        let n_normal = read_u32_le(buf, off + 12)?;
        let prim_top = read_u32_le(buf, off + 16)?;
        let n_primitive = read_u32_le(buf, off + 20)?;
        let scale = read_i32_le(buf, off + 24)?;
        let oh = ObjectHeader {
            vert_top,
            n_vert,
            normal_top,
            n_normal,
            prim_top,
            n_primitive,
            scale,
        };

        // Resolve and read vertices. Validate the byte range with checked
        // arithmetic BEFORE allocating: `n_vert` is an unverified u32 from the
        // buffer, so a bogus value (e.g. a non-TMD candidate that happened to
        // pass the magic check) would otherwise overflow `Vec::with_capacity`
        // (capacity-overflow panic) or, on 32-bit targets, overflow the
        // `* VECTOR_SIZE` byte-count entirely. The scanner relies on `parse`
        // returning `Err` for junk, never panicking.
        let mut vertices = Vec::new();
        if n_vert > 0 {
            let start = ptr_base
                .checked_add(vert_top as usize)
                .filter(|&s| s <= buf.len());
            let range = start.and_then(|s| {
                (n_vert as usize)
                    .checked_mul(VECTOR_SIZE)
                    .and_then(|n| s.checked_add(n))
                    .filter(|&e| e <= buf.len())
                    .map(|_| s)
            });
            let Some(start) = range else {
                bail!(
                    "object {} vertices (top {}, count {}) overruns buffer ({})",
                    i,
                    vert_top,
                    n_vert,
                    buf.len()
                );
            };
            vertices.reserve(n_vert as usize);
            for v in 0..n_vert as usize {
                vertices.push(read_vector(buf, start + v * VECTOR_SIZE)?);
            }
        }

        // Normals (same shape as vertices; same checked-before-alloc guard).
        let mut normals = Vec::new();
        if n_normal > 0 {
            let start = ptr_base
                .checked_add(normal_top as usize)
                .filter(|&s| s <= buf.len());
            let range = start.and_then(|s| {
                (n_normal as usize)
                    .checked_mul(VECTOR_SIZE)
                    .and_then(|n| s.checked_add(n))
                    .filter(|&e| e <= buf.len())
                    .map(|_| s)
            });
            let Some(start) = range else {
                bail!(
                    "object {} normals (top {}, count {}) overruns buffer ({})",
                    i,
                    normal_top,
                    n_normal,
                    buf.len()
                );
            };
            normals.reserve(n_normal as usize);
            for v in 0..n_normal as usize {
                normals.push(read_vector(buf, start + v * VECTOR_SIZE)?);
            }
        }

        // Compute the primitive section's byte range without iterating it.
        // The section starts at `ptr_base + prim_top` and ends where the
        // first per-object data section begins (vertices or normals,
        // whichever comes first AND is non-empty). For Legaia the primitive
        // layout is custom and we don't try to walk it here.
        let prim_start = ptr_base + prim_top as usize;
        let mut prim_end_candidates: Vec<usize> = Vec::new();
        if n_vert > 0 {
            prim_end_candidates.push(ptr_base + vert_top as usize);
        }
        if n_normal > 0 {
            prim_end_candidates.push(ptr_base + normal_top as usize);
        }
        prim_end_candidates.push(buf.len());
        let prim_end = *prim_end_candidates.iter().min().unwrap();
        let primitives_byte_size = prim_end.saturating_sub(prim_start);

        // Best-effort PSX-SDK-style walk so we can flag whether the
        // standard format applies.
        let primitives_psx_walk = if n_primitive > 0 && primitives_byte_size > 0 {
            Some(walk_psx_primitives(
                buf,
                prim_start,
                prim_end,
                n_primitive as usize,
            ))
        } else {
            None
        };

        objects.push(Object {
            header: oh,
            vertices,
            normals,
            primitives_byte_offset: prim_start,
            primitives_byte_size,
            claimed_n_primitive: n_primitive,
            primitives_psx_walk,
        });
    }

    Ok(Tmd { header, objects })
}

/// Try to walk primitives using the standard PSX SDK header layout
/// `(olen, ilen, flag, mode)` where body = `ilen * 4` bytes. Returns the
/// list of headers if it walks cleanly to exactly `n_primitive` items
/// without overrunning the section. This is for diagnostic purposes.
fn walk_psx_primitives(
    buf: &[u8],
    start: usize,
    end: usize,
    n_primitive: usize,
) -> std::result::Result<Vec<PrimHeader>, String> {
    // No `with_capacity(n_primitive)`: `n_primitive` is an unverified u32 from
    // the object header, so a junk value would overflow the allocation. The
    // loop bails as soon as `pos` overruns the section, so growth is bounded
    // by the real section size regardless.
    let mut out = Vec::new();
    let mut pos = start;
    for p in 0..n_primitive {
        if pos + PRIM_HEADER_SIZE > end {
            return Err(format!(
                "primitive {} header at {} past section end {}",
                p, pos, end
            ));
        }
        let ph = PrimHeader {
            olen: buf[pos],
            ilen: buf[pos + 1],
            flag: buf[pos + 2],
            mode: buf[pos + 3],
        };
        let body_bytes = (ph.ilen as usize) * 4;
        if pos + PRIM_HEADER_SIZE + body_bytes > end {
            return Err(format!(
                "primitive {} body ({}b, mode=0x{:02X}) at {} overruns section end {}",
                p,
                body_bytes,
                ph.mode,
                pos + PRIM_HEADER_SIZE,
                end
            ));
        }
        out.push(ph);
        pos += PRIM_HEADER_SIZE + body_bytes;
    }
    if pos != end {
        return Err(format!(
            "walked {} prims but consumed {}b of {}b section ({} unused)",
            n_primitive,
            pos - start,
            end - start,
            end - pos
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tiny synthetic TMD: 1 object with 2 vertices, 0 normals,
    /// 1 primitive (4-byte header, 8-byte body).
    fn synth_tmd() -> Vec<u8> {
        let mut buf = Vec::new();
        // Header: id with FLIST_BIT, flags=0, nobj=1
        buf.extend_from_slice(&(0x8000_0041u32).to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        // Object table: 28 bytes. Pointers are offsets from header-end.
        // Plan layout (offsets relative to byte 12 = header-end):
        //   prim_top    = 28           (right after object table)
        //   vert_top    = 28 + 12 = 40 (after primitive header+body)
        //   normal_top  = 40 + 16 = 56 (after 2 vertices * 8B)
        // n_vert=2, n_normal=0, n_prim=1
        buf.extend_from_slice(&40u32.to_le_bytes()); // vert_top
        buf.extend_from_slice(&2u32.to_le_bytes()); // n_vert
        buf.extend_from_slice(&56u32.to_le_bytes()); // normal_top
        buf.extend_from_slice(&0u32.to_le_bytes()); // n_normal
        buf.extend_from_slice(&28u32.to_le_bytes()); // prim_top
        buf.extend_from_slice(&1u32.to_le_bytes()); // n_primitive
        buf.extend_from_slice(&0i32.to_le_bytes()); // scale
        // Primitive header: olen=4, ilen=2, flag=0, mode=0x20
        buf.extend_from_slice(&[4, 2, 0, 0x20]);
        // Primitive body: 2 words = 8 bytes
        buf.extend_from_slice(&[0; 8]);
        // Vertex 0: (1,2,3)
        buf.extend_from_slice(&1i16.to_le_bytes());
        buf.extend_from_slice(&2i16.to_le_bytes());
        buf.extend_from_slice(&3i16.to_le_bytes());
        buf.extend_from_slice(&0i16.to_le_bytes());
        // Vertex 1: (-1,-2,-3)
        buf.extend_from_slice(&(-1i16).to_le_bytes());
        buf.extend_from_slice(&(-2i16).to_le_bytes());
        buf.extend_from_slice(&(-3i16).to_le_bytes());
        buf.extend_from_slice(&0i16.to_le_bytes());
        buf
    }

    #[test]
    fn parses_synthetic_tmd() {
        let buf = synth_tmd();
        let tmd = parse(&buf).unwrap();
        assert!(tmd.header.flist_bit_set);
        assert_eq!(tmd.header.nobj, 1);
        assert_eq!(tmd.objects.len(), 1);
        let o = &tmd.objects[0];
        assert_eq!(o.vertices.len(), 2);
        assert_eq!(o.vertices[0].x, 1);
        assert_eq!(o.vertices[1].x, -1);
        assert_eq!(o.normals.len(), 0);
        assert_eq!(o.claimed_n_primitive, 1);
        // PSX-format walk should succeed on this synthetic file.
        let walk = o.primitives_psx_walk.as_ref().unwrap();
        let prims = walk.as_ref().unwrap();
        assert_eq!(prims.len(), 1);
        assert_eq!(prims[0].ilen, 2);
        // Primitive section size = 4 (header) + 8 (body) = 12 bytes.
        assert_eq!(o.primitives_byte_size, 12);
    }

    #[test]
    fn rejects_no_flist_bit() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x0000_0041u32.to_le_bytes()); // no FLIST_BIT
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn empty_object_count_is_ok() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x8000_0041u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        let tmd = parse(&buf).unwrap();
        assert_eq!(tmd.objects.len(), 0);
    }

    #[test]
    fn rejects_implausible_nobj() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x8000_0041u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse(&buf).is_err());
    }

    /// A candidate that passes the header magic but carries a junk
    /// `n_vert` / `n_primitive` (e.g. a non-TMD blob the scanner probed)
    /// must return `Err`, never panic with a capacity overflow. The asset
    /// TMD scanner calls `parse` on every magic hit across whole PROT
    /// entries (including LZS-decompressed sections), so a panic here takes
    /// down the disc load / web viewer.
    #[test]
    fn rejects_bogus_lengths_without_panicking() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x8000_0002u32.to_le_bytes()); // id (magic)
        buf.extend_from_slice(&0u32.to_le_bytes()); // flags
        buf.extend_from_slice(&1u32.to_le_bytes()); // nobj
        // One object whose counts are absurd u32s.
        buf.extend_from_slice(&40u32.to_le_bytes()); // vert_top
        buf.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // n_vert (junk)
        buf.extend_from_slice(&56u32.to_le_bytes()); // normal_top
        buf.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // n_normal (junk)
        buf.extend_from_slice(&28u32.to_le_bytes()); // prim_top
        buf.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // n_primitive (junk)
        buf.extend_from_slice(&0i32.to_le_bytes()); // scale
        buf.resize(buf.len() + 64, 0);
        assert!(parse(&buf).is_err());
    }

    // --- Additional panic-hardening regression tests ---------------------
    //
    // `parse` is already hardened (checked-before-alloc on n_vert / n_normal /
    // n_primitive). These confirm the remaining attacker-controlled entry
    // shapes - empty / 1-byte / truncated-header input and a valid-magic blob
    // with a garbage body - all return `Err` without panicking.

    #[test]
    fn empty_input_is_err_not_panic() {
        assert!(parse(&[]).is_err());
    }

    #[test]
    fn one_byte_input_is_err_not_panic() {
        assert!(parse(&[0x02]).is_err());
    }

    #[test]
    fn truncated_header_is_err_not_panic() {
        // Magic present (FLIST set) but fewer than HEADER_SIZE bytes.
        assert!(parse(&0x8000_0002u32.to_le_bytes()).is_err());
    }

    #[test]
    fn valid_magic_object_table_overruns_buffer_is_err_not_panic() {
        // Header claims 5 objects but no object table bytes follow.
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x8000_0002u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&5u32.to_le_bytes());
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn valid_magic_garbage_body_is_err_or_bounded_ok_not_panic() {
        // Valid TMD magic + nobj=1, then an object table of all-0xFF (absurd
        // counts AND offsets). Must not panic.
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x8000_0002u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend(std::iter::repeat_n(0xFFu8, OBJECT_SIZE));
        buf.extend(std::iter::repeat_n(0u8, 64));
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn iter_groups_rejects_oob_section_without_panic() {
        use crate::legaia_prims::{iter_groups, iter_groups_lenient};
        let buf = vec![0u8; 16];
        // Section start/size that runs past the buffer.
        assert!(iter_groups(&buf, 8, 1_000_000).is_err());
        // Lenient sibling returns empty rather than panicking.
        assert!(iter_groups_lenient(&buf, 8, 1_000_000).is_empty());
        // Overflowing section bounds.
        assert!(iter_groups(&buf, usize::MAX, 8).is_err());
        assert!(iter_groups_lenient(&buf, usize::MAX, 8).is_empty());
    }

    #[test]
    fn iter_groups_junk_body_does_not_panic() {
        use crate::legaia_prims::{iter_groups, iter_groups_lenient};
        // 64 bytes of 0xFF interpreted as a primitive section: huge counts /
        // strides must be caught by the overrun checks, not panic.
        let buf = vec![0xFFu8; 64];
        let _ = iter_groups(&buf, 0, buf.len());
        let _ = iter_groups_lenient(&buf, 0, buf.len());
    }

    /// TMD2 dispatch (asset type 0x09) hands the payload to FUN_80026b4c
    /// directly, which validates `*buffer == 0x80000002`. This test confirms
    /// `parse` accepts that exact id (Legaia's chosen TMD id) so a TMD2
    /// payload - i.e., a single bare TMD blob - round-trips cleanly through
    /// the same parser we use for TMD-pack contents.
    #[test]
    fn parses_legaia_tmd2_id() {
        // Same shape as synth_tmd, but with id = 0x80000002 instead of 0x80000041.
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x8000_0002u32.to_le_bytes()); // Legaia TMD id
        buf.extend_from_slice(&0u32.to_le_bytes()); // flags
        buf.extend_from_slice(&1u32.to_le_bytes()); // nobj
        // Object table: one object, prims-then-verts, no normals.
        buf.extend_from_slice(&40u32.to_le_bytes()); // vert_top
        buf.extend_from_slice(&2u32.to_le_bytes()); // n_vert
        buf.extend_from_slice(&56u32.to_le_bytes()); // normal_top (unused)
        buf.extend_from_slice(&0u32.to_le_bytes()); // n_normal
        buf.extend_from_slice(&28u32.to_le_bytes()); // prim_top
        buf.extend_from_slice(&1u32.to_le_bytes()); // n_primitive
        buf.extend_from_slice(&0i32.to_le_bytes()); // scale
        // Primitive header (4B) + body (8B) = 12B
        buf.extend_from_slice(&[4, 2, 0, 0x20]);
        buf.extend_from_slice(&[0; 8]);
        // Vertices
        for v in [(1i16, 2i16, 3i16), (-1, -2, -3)] {
            buf.extend_from_slice(&v.0.to_le_bytes());
            buf.extend_from_slice(&v.1.to_le_bytes());
            buf.extend_from_slice(&v.2.to_le_bytes());
            buf.extend_from_slice(&0i16.to_le_bytes());
        }

        let tmd = parse(&buf).unwrap();
        assert_eq!(tmd.header.id, 0x8000_0002);
        assert!(tmd.header.flist_bit_set);
        assert_eq!(tmd.header.nobj, 1);
        assert_eq!(tmd.objects.len(), 1);
        assert_eq!(tmd.objects[0].vertices.len(), 2);
        assert_eq!(tmd.objects[0].vertices[0].x, 1);
    }

    /// `parse` then the mesh builders on pseudo-random bytes (some forced to
    /// carry the TMD magic) must never panic. The asset TMD scanner + web
    /// viewer probe every magic hit, and the mesh builders run on whatever
    /// `parse` returns - a vertex-index or prim-section panic there takes
    /// down the disc load.
    #[test]
    fn parse_and_mesh_build_never_panic_on_random_bytes() {
        use crate::mesh::{tmd_to_textured_mesh, tmd_to_vram_mesh};
        for seed in 0u64..400 {
            let mut x = seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(0xABCD);
            let n = (seed % 512) as usize;
            let mut buf = Vec::with_capacity(n);
            for _ in 0..n {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                buf.push(x as u8);
            }
            // Force the Legaia TMD magic so `parse` enters the body walk.
            if buf.len() >= 4 {
                buf[0..4].copy_from_slice(&0x8000_0002u32.to_le_bytes());
            }
            if let Ok(tmd) = parse(&buf) {
                // The mesh builders must not panic on whatever objects /
                // vertex counts / prim sections `parse` accepted.
                let _ = tmd_to_textured_mesh(&tmd, &buf);
                let _ = tmd_to_vram_mesh(&tmd, &buf);
            }
        }
    }
}
