//! Effect-bundle container - the format used by `data\battle\efect.dat`
//! (PROT 0872) and a sibling sub-section of `etmd.dat` (PROT 0873).
//!
//! ## Format
//!
//! A 4-byte magic word followed by two header u32s (constants in every
//! observed file) and a fixed 28-entry ascending offset table:
//!
//! ```text
//! [file start]
//!   ...preamble (variable size; 0xFF-padding allocator slot table in 0872,
//!      arbitrary preceding data in 0873)...
//!   [u32 LE = MAGIC = 0x02018B0C]
//!   [u32 LE = HEADER_A = 0x0000001D]   ; constant across all observed bundles
//!   [u32 LE = HEADER_B = 0x0000001E]   ; constant across all observed bundles
//!   [28 × u32 LE - offset table, identical across all observed bundles]
//!   [asset region - packed Legaia TMD primitive groups + TIM textures]
//! [file end]
//! ```
//!
//! The 28 offsets are the **same values** in every known bundle (`0x17F4`
//! through `0x404D`). They are abstract sub-record offsets - not file
//! offsets - describing a fixed effect-record schema that the runtime fills
//! in with bundle-specific bytes. This mirrors the field-pack pattern in
//! [`crate::field_pack`].
//!
//! ## What this gives us
//!
//! - Reliable detection (`detect`) gated on magic + the strict schema. The
//!   magic alone is too short to be safe; the constant header words plus
//!   the strict ascending table make false positives effectively impossible.
//! - Boundary information: where the magic sits, where the asset region
//!   begins, and the per-slot record sizes (`offset[i+1] - offset[i]`).
//! - A per-PROT-entry classifier so downstream tooling can route effect
//!   bundles through this parser instead of falling through to the
//!   stage-geometry / TIM-pack heuristics.
//!
//! ## What this doesn't (yet) do
//!
//! - Interpret the asset region. The first asset record after the offset
//!   table is a single Legaia TMD (magic `0x80000002`) covering many
//!   primitive groups; the 28 schema offsets index into its primitive
//!   data. Walking individual effect primitives requires the per-mode
//!   descriptor table at `DAT_8007326c` - see `legaia_tmd::legaia_prims`.
//! - Map each schema slot to a named effect. The runtime correspondence
//!   (effect ID → slot index) lives in code that hasn't been traced yet.

use serde::Serialize;

/// Magic word that introduces the bundle header.
pub const MAGIC: u32 = 0x0201_8B0C;

/// First header u32 immediately after the magic. Constant in all observed
/// effect bundles (PROT 0872 and 0873).
pub const HEADER_A: u32 = 0x0000_001D;

/// Second header u32. Constant in all observed effect bundles. The
/// `HEADER_B == HEADER_A + 1` relationship is a property of the format,
/// not a coincidence - both values are the same across every bundle.
pub const HEADER_B: u32 = 0x0000_001E;

/// Number of u32 entries in the offset table.
pub const RECORD_COUNT: usize = 28;

/// Size of the offset table in bytes.
pub const TABLE_SIZE: usize = RECORD_COUNT * 4;

/// First value in the offset table (= start of the first abstract record).
pub const SCHEMA_FIRST: u32 = 0x0000_17F4;

/// Last value in the offset table (= start of the 28th abstract record).
pub const SCHEMA_LAST: u32 = 0x0000_404D;

/// Legaia TMD magic (= `0x80000002`). Used to detect TMDs inside the asset
/// region. Mirrors the constant in `crates/tmd`.
const TMD_MAGIC: u32 = 0x8000_0002;

/// PSX TIM magic at file offset 0 (= `0x00000010`).
const TIM_MAGIC: u32 = 0x0000_0010;

/// Constant header values observed in EVERY master TMD across every effect
/// bundle: 382 verts, 760 normals, 760 primitives. The data inside differs
/// per-bundle but the structural counts are universal.
pub const MASTER_TMD_NVERTS: u32 = 382;
pub const MASTER_TMD_NPRIMS: u32 = 760;
pub const MASTER_TMD_NNORMALS: u32 = 760;

/// Parsed location and slot layout of an effect bundle inside a buffer.
#[derive(Debug, Clone, Serialize)]
pub struct EffectBundle {
    /// File offset of the 4-byte magic word.
    pub magic_offset: usize,
    /// File offset of the first byte of the offset table (= magic + 12).
    pub table_offset: usize,
    /// File offset immediately after the offset table - first byte of the
    /// asset region (TMD + TIMs).
    pub assets_start: usize,
    /// Total file size, for convenience when reporting.
    pub file_size: usize,
    /// Header u32 immediately after the magic.
    pub header_a: u32,
    /// Header u32 immediately after `header_a`.
    pub header_b: u32,
    /// 28 abstract record slots, each `(offset, size)`. Sizes are derived
    /// from `offset[i+1] - offset[i]`; the last slot's size is unknown and
    /// reported as `None`.
    pub slots: Vec<SchemaSlot>,
    /// Detected sub-assets in the asset region - one master TMD plus zero or
    /// more sub-effect TMDs and zero or more PSX TIMs. Populated by `detect`.
    pub assets: AssetRegion,
}

/// Catalog of what's inside the asset region (the bytes after the offset
/// table). Populated by walking the region and validating each magic hit.
#[derive(Debug, Clone, Serialize, Default)]
pub struct AssetRegion {
    /// File offsets of every validated Legaia TMD in the asset region.
    /// `tmds[0]` is the *master* TMD - always at `assets_start`, always with
    /// (382 verts, 760 normals, 760 prims). The remainder are sub-effect
    /// TMDs of variable size; the format reserves up to 28 slots (HEADER_A
    /// = 1 master + 28 sub = 29) but bundles can fill fewer.
    pub tmds: Vec<usize>,
    /// File offsets of every validated PSX TIM in the asset region. Variable
    /// per bundle (0..~9 in observed data).
    pub tims: Vec<usize>,
}

/// One abstract record slot from the offset table.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct SchemaSlot {
    /// Offset of this record in the schema's abstract coordinate space.
    /// **Not** a file offset.
    pub offset: u32,
    /// Size of this record (`offset[i+1] - offset[i]`); `None` for the last
    /// slot, whose size depends on per-bundle asset-region layout that we
    /// don't yet decode.
    pub size: Option<u32>,
}

impl EffectBundle {
    /// File-offset range of the preamble (everything before the magic).
    /// In 0872 this is the 0xFF-init allocator slot table; in 0873 it's
    /// the etmd.dat content that precedes the embedded effect bundle.
    pub fn preamble_range(&self) -> (usize, usize) {
        (0, self.magic_offset)
    }

    /// File-offset range of the asset region (TMD + TIMs after the table).
    pub fn assets_range(&self) -> (usize, usize) {
        (self.assets_start, self.file_size)
    }
}

/// Look for an effect bundle in `buf`. Returns the first match, scanning
/// forward from offset 0.
///
/// Detection criteria (all must hold):
/// 1. `MAGIC` (LE) appears at some offset `m`.
/// 2. `HEADER_A` and `HEADER_B` follow at `m+4` and `m+8`.
/// 3. The 28 ascending u32 LE values fit in `buf` after the headers.
/// 4. They are strictly ascending.
/// 5. `slots[0] == SCHEMA_FIRST` and `slots[27] == SCHEMA_LAST`.
///
/// The combination of magic + constant headers + strict-ascending shape +
/// boundary anchors is specific enough that incidental hits are vanishingly
/// unlikely - the same approach that makes [`crate::field_pack::detect`]
/// false-positive-free.
pub fn detect(buf: &[u8]) -> Option<EffectBundle> {
    let magic_bytes = MAGIC.to_le_bytes();
    let mut search_from = 0usize;
    while let Some(rel) = find_subslice(&buf[search_from..], &magic_bytes) {
        let magic_offset = search_from + rel;
        if let Some(eb) = parse_at(buf, magic_offset) {
            return Some(eb);
        }
        search_from = magic_offset + 1;
    }
    None
}

fn parse_at(buf: &[u8], magic_offset: usize) -> Option<EffectBundle> {
    let header_a_off = magic_offset + 4;
    let header_b_off = magic_offset + 8;
    let table_offset = magic_offset + 12;
    let assets_start = table_offset + TABLE_SIZE;
    if assets_start > buf.len() {
        return None;
    }

    let header_a = u32::from_le_bytes(buf[header_a_off..header_a_off + 4].try_into().unwrap());
    let header_b = u32::from_le_bytes(buf[header_b_off..header_b_off + 4].try_into().unwrap());
    if header_a != HEADER_A || header_b != HEADER_B {
        return None;
    }

    let mut slots = Vec::with_capacity(RECORD_COUNT);
    let mut prev: Option<u32> = None;
    for i in 0..RECORD_COUNT {
        let p = table_offset + i * 4;
        let v = u32::from_le_bytes(buf[p..p + 4].try_into().unwrap());
        if let Some(prev_v) = prev
            && v <= prev_v
        {
            return None;
        }
        prev = Some(v);
        slots.push(SchemaSlot {
            offset: v,
            size: None,
        });
    }
    if slots[0].offset != SCHEMA_FIRST || slots[RECORD_COUNT - 1].offset != SCHEMA_LAST {
        return None;
    }
    for i in 0..RECORD_COUNT - 1 {
        let next = slots[i + 1].offset;
        let cur = slots[i].offset;
        slots[i].size = Some(next - cur);
    }

    let assets = scan_asset_region(buf, assets_start);

    Some(EffectBundle {
        magic_offset,
        table_offset,
        assets_start,
        file_size: buf.len(),
        header_a,
        header_b,
        slots,
        assets,
    })
}

/// Walk the asset region and return every validated TMD/TIM offset.
///
/// Validation: TMD requires nobjs in `1..=64` and `obj[0].scale == 0x00808080`
/// (the Legaia-specific scale signature). TIM requires `pmode <= 4` in the
/// flag word. Both checks are strict enough that random bytes don't match.
fn scan_asset_region(buf: &[u8], lo: usize) -> AssetRegion {
    let mut tmds = Vec::new();
    let mut tims = Vec::new();
    let mut pos = lo;
    while pos + 4 <= buf.len() {
        let magic = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap());
        if magic == TMD_MAGIC && is_real_tmd(buf, pos) {
            tmds.push(pos);
        } else if magic == TIM_MAGIC && is_real_tim(buf, pos) {
            tims.push(pos);
        }
        pos += 4;
    }
    AssetRegion { tmds, tims }
}

fn is_real_tmd(buf: &[u8], off: usize) -> bool {
    if off + 12 + 28 > buf.len() {
        return false;
    }
    let nobjs = u32::from_le_bytes(buf[off + 8..off + 12].try_into().unwrap());
    if !(1..=64).contains(&nobjs) {
        return false;
    }
    // obj[0].scale lives at offset 0x18 within the 28-byte object record,
    // which begins at off+12. Must equal 0x00808080 for Legaia TMDs.
    let scale_off = off + 12 + 0x18;
    if scale_off + 4 > buf.len() {
        return false;
    }
    let scale = u32::from_le_bytes(buf[scale_off..scale_off + 4].try_into().unwrap());
    scale == 0x0080_8080
}

fn is_real_tim(buf: &[u8], off: usize) -> bool {
    if off + 8 > buf.len() {
        return false;
    }
    let flag = u32::from_le_bytes(buf[off + 4..off + 8].try_into().unwrap());
    let pmode = flag & 0x7;
    pmode <= 4
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic bundle that satisfies the detector.
    fn synthetic(preamble: usize) -> Vec<u8> {
        let mut buf = vec![0xFFu8; preamble];
        buf.extend_from_slice(&MAGIC.to_le_bytes());
        buf.extend_from_slice(&HEADER_A.to_le_bytes());
        buf.extend_from_slice(&HEADER_B.to_le_bytes());
        // Distribute 28 offsets evenly between SCHEMA_FIRST and SCHEMA_LAST.
        let span = SCHEMA_LAST - SCHEMA_FIRST;
        let step = span / (RECORD_COUNT as u32 - 1);
        for i in 0..RECORD_COUNT {
            let v = if i == 0 {
                SCHEMA_FIRST
            } else if i == RECORD_COUNT - 1 {
                SCHEMA_LAST
            } else {
                SCHEMA_FIRST + step * i as u32
            };
            buf.extend_from_slice(&v.to_le_bytes());
        }
        // Pretend asset region: a bare Legaia TMD magic.
        buf.extend_from_slice(&0x8000_0002u32.to_le_bytes());
        buf.extend_from_slice(&[0xAAu8; 64]);
        buf
    }

    #[test]
    fn detects_synthetic_bundle() {
        let buf = synthetic(1024);
        let eb = detect(&buf).expect("should detect");
        assert_eq!(eb.magic_offset, 1024);
        assert_eq!(eb.table_offset, 1024 + 12);
        assert_eq!(eb.assets_start, 1024 + 12 + TABLE_SIZE);
        assert_eq!(eb.header_a, HEADER_A);
        assert_eq!(eb.header_b, HEADER_B);
        assert_eq!(eb.slots.len(), RECORD_COUNT);
        assert_eq!(eb.slots[0].offset, SCHEMA_FIRST);
        assert_eq!(eb.slots[RECORD_COUNT - 1].offset, SCHEMA_LAST);
        assert!(eb.slots[RECORD_COUNT - 1].size.is_none());
        assert!(eb.slots[0].size.is_some());
    }

    #[test]
    fn rejects_buffer_with_only_magic() {
        let mut buf = vec![0u8; 100];
        buf.extend_from_slice(&MAGIC.to_le_bytes());
        // No headers/table follow.
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_wrong_header_words() {
        let mut buf = vec![0u8; 100];
        buf.extend_from_slice(&MAGIC.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes()); // wrong header_a
        buf.extend_from_slice(&HEADER_B.to_le_bytes());
        for i in 0..RECORD_COUNT {
            let v = SCHEMA_FIRST + i as u32 * 0x100;
            buf.extend_from_slice(&v.to_le_bytes());
        }
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_non_ascending_table() {
        let mut buf = vec![0u8; 100];
        buf.extend_from_slice(&MAGIC.to_le_bytes());
        buf.extend_from_slice(&HEADER_A.to_le_bytes());
        buf.extend_from_slice(&HEADER_B.to_le_bytes());
        buf.extend_from_slice(&SCHEMA_FIRST.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes()); // goes backward
        for _ in 2..RECORD_COUNT {
            buf.extend_from_slice(&0u32.to_le_bytes());
        }
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_wrong_anchors() {
        let mut buf = vec![0u8; 100];
        buf.extend_from_slice(&MAGIC.to_le_bytes());
        buf.extend_from_slice(&HEADER_A.to_le_bytes());
        buf.extend_from_slice(&HEADER_B.to_le_bytes());
        // 28 ascending u32s but boundary values don't match SCHEMA_FIRST/LAST.
        for i in 0..RECORD_COUNT {
            buf.extend_from_slice(&((i as u32 + 1) * 4).to_le_bytes());
        }
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn slot_sizes_sum_to_known_range() {
        let buf = synthetic(0);
        let eb = detect(&buf).unwrap();
        let total: u32 = eb.slots.iter().filter_map(|s| s.size).sum();
        assert_eq!(total, SCHEMA_LAST - SCHEMA_FIRST);
    }

    #[test]
    fn preamble_and_asset_ranges() {
        let buf = synthetic(0x100);
        let eb = detect(&buf).unwrap();
        assert_eq!(eb.preamble_range(), (0, 0x100));
        let (a_start, a_end) = eb.assets_range();
        assert_eq!(a_start, 0x100 + 12 + TABLE_SIZE);
        assert_eq!(a_end, buf.len());
    }

    /// Build an asset region containing one valid Legaia TMD (just enough
    /// structure to satisfy `is_real_tmd`).
    fn synthetic_with_master_tmd() -> Vec<u8> {
        let mut buf = vec![0xFFu8; 0x100];
        buf.extend_from_slice(&MAGIC.to_le_bytes());
        buf.extend_from_slice(&HEADER_A.to_le_bytes());
        buf.extend_from_slice(&HEADER_B.to_le_bytes());
        let span = SCHEMA_LAST - SCHEMA_FIRST;
        let step = span / (RECORD_COUNT as u32 - 1);
        for i in 0..RECORD_COUNT {
            let v = match i {
                0 => SCHEMA_FIRST,
                _ if i == RECORD_COUNT - 1 => SCHEMA_LAST,
                _ => SCHEMA_FIRST + step * i as u32,
            };
            buf.extend_from_slice(&v.to_le_bytes());
        }
        // Master TMD at assets_start: magic + flags + nobjs=1 + obj[0]
        // (verts_off, n_verts, normals_off, n_normals, prims_off, n_prims, scale)
        buf.extend_from_slice(&TMD_MAGIC.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes()); // flags
        buf.extend_from_slice(&1u32.to_le_bytes()); // nobjs
        // obj[0] - values don't matter for validation except scale
        for _ in 0..6 {
            buf.extend_from_slice(&0u32.to_le_bytes());
        }
        buf.extend_from_slice(&0x0080_8080u32.to_le_bytes()); // scale (Legaia-specific)
        // Some filler bytes so we don't overrun reads
        buf.extend_from_slice(&[0u8; 64]);
        buf
    }

    #[test]
    fn detects_master_tmd_in_asset_region() {
        let buf = synthetic_with_master_tmd();
        let eb = detect(&buf).expect("should detect");
        assert_eq!(
            eb.assets.tmds.len(),
            1,
            "expected exactly the master TMD to validate"
        );
        assert_eq!(eb.assets.tmds[0], eb.assets_start);
        assert!(eb.assets.tims.is_empty());
    }

    #[test]
    fn rejects_invalid_tmd_in_asset_region() {
        // synthetic() places only an unparseable 0x80000002 magic; the strict
        // validator (nobjs sane + scale==0x00808080) must reject it.
        let buf = synthetic(0);
        let eb = detect(&buf).unwrap();
        assert!(eb.assets.tmds.is_empty());
        assert!(eb.assets.tims.is_empty());
    }
}
