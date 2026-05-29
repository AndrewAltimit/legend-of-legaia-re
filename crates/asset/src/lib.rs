//! Legaia asset descriptor + dispatcher.
//!
//! PORT: FUN_8001F05C, FUN_8001A8B0
//!
//! The game's loader (`FUN_8001f05c` in `SCUS_942.54`) takes a buffer plus a
//! single u32 packing `(type << 24) | (size & 0xFFFFFF)` and dispatches to a
//! type-specific handler. Each asset can be either LZS-compressed (the common
//! case - handled by `FUN_8001a55c`) or stored raw (handled by `FUN_8001a8b0`,
//! which is essentially a memcpy with the same `(size, src, dst)` shape).
//!
//! This crate provides:
//! - [`AssetType`] - the enum of known asset categories.
//! - [`Descriptor`] - `(type, size, data_offset)` parsed from the on-disc form.
//! - [`decode`] - apply a [`Descriptor`] + [`DecodeMode`] to a buffer.
//! - [`parse_player_lzs`] - parse a `player.lzs`-style container header.

use anyhow::{Result, bail};
use serde::Serialize;

pub mod anm_detect;
pub mod battle_char_pack;
pub mod battle_char_palette;
pub mod battle_data_pack;
pub mod befect_cluster;
pub mod categorize;
pub mod character_pack;
pub mod cutscene_text;
pub mod data_field_truncated;
pub mod effect_bundle;
pub mod field_objects;
pub mod field_pack;
pub mod init_pak;
pub mod item_names;
pub mod kingdom_bundle;
pub mod man_section;
pub mod menu_glyph_atlas;
pub mod mips_overlay;
pub mod monster_archive;
pub mod monster_gltf;
pub mod new_game;
pub mod overlay_ptr_table;
pub mod pack;
pub mod player_anm;
pub mod scene_asset_table;
pub mod scene_event_scripts;
pub mod scene_scripted_asset_table;
pub mod scene_tmd_stream;
pub mod scene_v12_table;
pub mod scene_vab_stream;
pub mod spell_names;
pub mod stage_geom;
pub mod str_fmv_table;
pub mod tim_catalog;
pub mod tim_deep_catalog;
pub mod tim_labels;
pub mod tim_scan;
pub mod title_pak;
pub mod tmd_scan;
pub mod tmd_size_prefix;
pub mod vab_multi_bank;
pub mod world_map_overlay;
pub mod worldmap_menu;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AssetType {
    /// Single TIM texture.
    Tim,
    /// List/pack of multiple TIMs (`Tim_Malloc_Err` branch).
    TimList,
    /// TMD mesh (one or many submeshes).
    Tmd,
    /// Unknown - tagged "man" by malloc-error string.
    Man,
    /// Unknown - tagged "mes" (likely message text).
    Mes,
    /// "move" - animation/move data.
    Move,
    /// "anm" - animation.
    Anm,
    /// VDF - Legaia-specific vector/animation file.
    Vdf,
    /// "sin" - unknown.
    Sin,
    /// Second TMD branch (`tmd_malloc_err2`); single submesh.
    Tmd2,
    /// "move2" - variant of move data.
    Move2,
    /// Sentinel/flag returns (cases 0xA, 0xF, 0x14 in the dispatcher).
    /// All three exit the dispatcher immediately with `(case << 8)` as the
    /// return value: 0xA00 / 0xF00 / 0x1400. No malloc, no decompress, no
    /// register. Data bytes (if any) are walker-skipped, not parsed.
    Flag(u8),
    /// Unknown type byte.
    Unknown(u8),
}

impl AssetType {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0x00 => Self::Tim,
            0x01 => Self::TimList,
            0x02 => Self::Tmd,
            0x03 => Self::Man,
            0x04 => Self::Mes,
            0x05 => Self::Move,
            0x06 => Self::Anm,
            0x07 => Self::Vdf,
            0x08 => Self::Sin,
            0x09 => Self::Tmd2,
            0x0B => Self::Move2,
            0x0A | 0x0F | 0x14 => Self::Flag(b),
            _ => Self::Unknown(b),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Tim => "TIM",
            Self::TimList => "TIM_LIST",
            Self::Tmd => "TMD",
            Self::Man => "MAN",
            Self::Mes => "MES",
            Self::Move => "MOVE",
            Self::Anm => "ANM",
            Self::Vdf => "VDF",
            Self::Sin => "SIN",
            Self::Tmd2 => "TMD2",
            Self::Move2 => "MOVE2",
            Self::Flag(_) => "FLAG",
            Self::Unknown(_) => "UNKNOWN",
        }
    }

    /// Whether this type carries actual data (vs. being a flag-only return).
    pub fn has_data(&self) -> bool {
        !matches!(self, Self::Flag(_))
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Descriptor {
    pub type_byte: u8,
    pub size: u32,
    pub data_offset: u32,
}

impl Descriptor {
    /// Parse from the canonical `(type_size, data_offset)` u32 pair.
    pub fn from_pair(type_size: u32, data_offset: u32) -> Self {
        Self {
            type_byte: ((type_size >> 24) & 0xFF) as u8,
            size: type_size & 0x00FF_FFFF,
            data_offset,
        }
    }

    pub fn asset_type(&self) -> AssetType {
        AssetType::from_byte(self.type_byte)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DecodeMode {
    /// LZS-decompress `size` bytes.
    Lzs,
    /// Raw copy `size` bytes (mirrors `FUN_8001a8b0`).
    Raw,
}

pub fn decode(buffer: &[u8], desc: &Descriptor, mode: DecodeMode) -> Result<Vec<u8>> {
    let start = desc.data_offset as usize;
    if start > buffer.len() {
        bail!(
            "data_offset 0x{:X} past buffer end ({}b)",
            start,
            buffer.len()
        );
    }
    let body = &buffer[start..];
    match mode {
        DecodeMode::Lzs => legaia_lzs::decompress(body, desc.size as usize),
        DecodeMode::Raw => {
            let n = desc.size as usize;
            if n > body.len() {
                bail!(
                    "raw size {} exceeds buffer remaining ({}b at offset 0x{:X})",
                    n,
                    body.len(),
                    start
                );
            }
            Ok(body[..n].to_vec())
        }
    }
}

/// Parse a `player.lzs`-style container: `[meta0, meta1, (size0, off0), (size1, off1), ...]`
/// where each `(size, off)` pair is a [`Descriptor`].
///
/// The header has 2 metadata u32s (purpose currently unknown) before the
/// descriptor pairs. `count` is how many descriptors to read; player.lzs uses 3.
pub fn parse_player_lzs(file: &[u8], count: usize) -> Result<Container> {
    if file.len() < 8 + count * 8 {
        bail!(
            "file too small ({}b) for {} descriptors after 8-byte meta",
            file.len(),
            count
        );
    }
    let meta = [
        u32::from_le_bytes(file[0..4].try_into().unwrap()),
        u32::from_le_bytes(file[4..8].try_into().unwrap()),
    ];
    let mut descriptors = Vec::with_capacity(count);
    for i in 0..count {
        let p = 8 + i * 8;
        let type_size = u32::from_le_bytes(file[p..p + 4].try_into().unwrap());
        let off = u32::from_le_bytes(file[p + 4..p + 8].try_into().unwrap());
        descriptors.push(Descriptor::from_pair(type_size, off));
    }
    Ok(Container { meta, descriptors })
}

#[derive(Debug, Serialize)]
pub struct Container {
    pub meta: [u32; 2],
    pub descriptors: Vec<Descriptor>,
}

/// Per-descriptor validation outcome.
#[derive(Debug, Clone, Serialize)]
pub struct DescriptorReport {
    pub index: usize,
    pub type_byte: u8,
    pub type_name: &'static str,
    pub size: u32,
    pub data_offset: u32,
    /// Mode that successfully decoded (if any).
    pub decoded_as: Option<&'static str>,
    /// Decoded length (if any).
    pub decoded_len: Option<usize>,
    /// First 4 bytes of decoded output, formatted hex.
    pub decoded_magic: Option<String>,
    /// True iff `decoded_magic` matches a known magic for this type.
    pub magic_ok: bool,
    /// Reason for failure, if any.
    pub error: Option<String>,
}

/// Whole-container validation outcome.
#[derive(Debug, Serialize)]
pub struct ContainerReport {
    pub count: u32,
    pub layout_ok: bool,
    pub descriptors: Vec<DescriptorReport>,
}

/// "First u32 we expect to see at offset 0 of an asset's data."
///
/// Only single-asset types have a known fixed magic. Pack types (TIM_LIST,
/// TMD, TMD2) start with a u32 count + offset table; the magic for the
/// contained items is inside the pack at runtime-computed offsets, not at
/// offset 0. So we deliberately don't claim a magic for them.
///
/// PSX TIM has `0x00000010` at file offset 0.
fn known_magic(t: AssetType) -> Option<u32> {
    match t {
        AssetType::Tim => Some(0x0000_0010),
        // Pack formats: first u32 is a count, not a magic.
        AssetType::TimList | AssetType::Tmd => None,
        // Bare TMD (case 9 in FUN_8001f05c) has the Legaia TMD magic at offset 0.
        AssetType::Tmd2 => Some(0x8000_0002),
        _ => None,
    }
}

/// Try LZS first, then Raw. Returns the mode that worked along with the bytes.
fn try_decode_either(buffer: &[u8], desc: &Descriptor) -> Result<(&'static str, Vec<u8>)> {
    match decode(buffer, desc, DecodeMode::Lzs) {
        Ok(v) => Ok(("lzs", v)),
        Err(e_lzs) => match decode(buffer, desc, DecodeMode::Raw) {
            Ok(v) => Ok(("raw", v)),
            Err(e_raw) => bail!("lzs={}; raw={}", e_lzs, e_raw),
        },
    }
}

/// Strict validation oracle. Tries to parse `buffer` as a player.lzs-style
/// container with `count` descriptors, then for each descriptor:
/// 1. Checks layout (known type, size & offset within sane bounds).
/// 2. Tries LZS then Raw decode.
/// 3. If a known magic exists for the type, requires it to match.
pub fn validate(buffer: &[u8], count: usize) -> Result<ContainerReport> {
    let c = parse_player_lzs(buffer, count)?;
    let header_end = (8 + count * 8) as u32;

    let mut descriptors = Vec::with_capacity(c.descriptors.len());
    let mut layout_ok = true;

    for (i, d) in c.descriptors.iter().enumerate() {
        let t = d.asset_type();
        let mut report = DescriptorReport {
            index: i,
            type_byte: d.type_byte,
            type_name: t.name(),
            size: d.size,
            data_offset: d.data_offset,
            decoded_as: None,
            decoded_len: None,
            decoded_magic: None,
            magic_ok: false,
            error: None,
        };

        // Skip flag descriptors - they have no data.
        if !t.has_data() {
            descriptors.push(report);
            continue;
        }

        if matches!(t, AssetType::Unknown(_)) {
            report.error = Some(format!("unknown type byte 0x{:02X}", d.type_byte));
            layout_ok = false;
            descriptors.push(report);
            continue;
        }
        if d.size < 32 || d.size > 4 * 1024 * 1024 {
            report.error = Some(format!("size {} out of bounds", d.size));
            layout_ok = false;
            descriptors.push(report);
            continue;
        }
        if d.data_offset < header_end || (d.data_offset as usize) >= buffer.len() {
            report.error = Some(format!(
                "offset 0x{:X} outside buffer [{:#X}, {:#X})",
                d.data_offset,
                header_end,
                buffer.len()
            ));
            layout_ok = false;
            descriptors.push(report);
            continue;
        }

        match try_decode_either(buffer, d) {
            Ok((mode, decoded)) => {
                report.decoded_as = Some(mode);
                report.decoded_len = Some(decoded.len());
                if decoded.len() >= 4 {
                    let m = u32::from_le_bytes(decoded[..4].try_into().unwrap());
                    report.decoded_magic = Some(format!("{:08X}", m));
                    if let Some(want) = known_magic(t) {
                        report.magic_ok = m == want;
                        if !report.magic_ok {
                            report.error = Some(format!(
                                "magic mismatch: got 0x{:08X}, want 0x{:08X}",
                                m, want
                            ));
                        }
                    } else {
                        // No known magic for this type; we just record.
                        report.magic_ok = true;
                    }
                }
            }
            Err(e) => {
                report.error = Some(e.to_string());
            }
        }

        descriptors.push(report);
    }

    Ok(ContainerReport {
        count: count as u32,
        layout_ok,
        descriptors,
    })
}

// ============================================================================
// Streaming format (DATA_FIELD\…)
//
// Reverse-engineered from `FUN_8002541c` 0x14 branch:
//
//   uVar2 = *puVar5;                                  // first u32 = type_size
//   while ((uVar2 & 0xffffff) != 0) {                 // terminator: size==0
//     FUN_8001f05c(puVar5 + 1, *puVar5, 0, 1);        // dispatcher; copy_only=1 (raw)
//     puVar5 = puVar5 + ((uVar2 & 0xffffff) >> 2) + 1; // advance words
//     uVar2 = *puVar5;
//   }
//
// Layout: a sequence of `[u32 type_size, data_bytes...]` chunks where
//   - `type_size = (type << 24) | (size & 0xFFFFFF)`
//   - `data_bytes` immediately follows the header (`size` bytes of raw data)
//   - the next chunk starts at `pos + 4 + ((size >> 2) << 2)` - i.e., size
//     truncated DOWN to a multiple of 4, plus the header. The runtime treats
//     size as already-aligned; if it isn't, the runtime would walk into the
//     "wrong" position and likely crash.
//   - terminator is a u32 with `size & 0xFFFFFF == 0`.
//
// Because copy_only=1 in the dispatcher call, every chunk is uncompressed.
// The chunk's first 4 bytes ARE the asset's first 4 bytes (no LZS step).

/// One chunk in a DATA_FIELD-style streaming buffer.
#[derive(Debug, Clone, Serialize)]
pub struct StreamChunk {
    /// Position in the buffer where the header u32 lives.
    pub header_offset: usize,
    pub type_byte: u8,
    pub type_name: &'static str,
    /// Declared size in bytes (low 24 bits of header).
    pub size: u32,
    /// First 4 bytes of the data, formatted hex (or empty).
    pub magic: String,
    /// True iff `magic` matches a known magic for this type.
    pub magic_ok: bool,
}

#[derive(Debug, Serialize)]
pub struct StreamReport {
    pub chunks: Vec<StreamChunk>,
    /// Did we hit a clean terminator before running out of bytes?
    pub terminated: bool,
    /// Did every chunk have a known type?
    pub all_known_types: bool,
    /// Did every chunk with a known magic pass it?
    pub all_magic_ok: bool,
    /// Bytes consumed (header + data + final terminator).
    pub bytes_consumed: usize,
}

/// Parse a DATA_FIELD-style streaming buffer. Returns `(chunks, &data_slices)`-equivalent
/// info plus a layout report. Stops on terminator or on bounds violation.
///
/// Set `max_chunks` to bound runaway parses. A reasonable cap is 4096.
pub fn parse_streaming(buffer: &[u8], max_chunks: usize) -> Result<StreamReport> {
    let mut chunks = Vec::new();
    let mut pos = 0usize;
    let mut terminated = false;
    let mut all_known_types = true;
    let mut all_magic_ok = true;

    while pos + 4 <= buffer.len() && chunks.len() < max_chunks {
        let header = u32::from_le_bytes(buffer[pos..pos + 4].try_into().unwrap());
        let type_byte = ((header >> 24) & 0xFF) as u8;
        let size = header & 0x00FF_FFFF;

        if size == 0 {
            terminated = true;
            pos += 4;
            break;
        }

        let data_start = pos + 4;
        let data_end = data_start.saturating_add(size as usize);
        if data_end > buffer.len() {
            // Unterminated / past end. Stop without consuming this chunk.
            break;
        }

        let t = AssetType::from_byte(type_byte);
        let mut magic = String::new();
        let mut magic_ok = true;
        if data_end - data_start >= 4 {
            let m = u32::from_le_bytes(buffer[data_start..data_start + 4].try_into().unwrap());
            magic = format!("{:08X}", m);
            if let Some(want) = known_magic(t) {
                magic_ok = m == want;
            }
        }
        if matches!(t, AssetType::Unknown(_)) {
            all_known_types = false;
        }
        if !magic_ok {
            all_magic_ok = false;
        }

        chunks.push(StreamChunk {
            header_offset: pos,
            type_byte,
            type_name: t.name(),
            size,
            magic,
            magic_ok,
        });

        // Advance: header + size_truncated_to_word_boundary.
        // Mirrors the runtime: `(size >> 2) + 1` words = 4 + (size & ~3) bytes.
        let advance = 4 + ((size as usize) & !3);
        pos += advance;
    }

    Ok(StreamReport {
        chunks,
        terminated,
        all_known_types,
        all_magic_ok,
        bytes_consumed: pos,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_packs_type_and_size() {
        let d = Descriptor::from_pair(0x02_123456, 0x40);
        assert_eq!(d.type_byte, 0x02);
        assert_eq!(d.size, 0x123456);
        assert_eq!(d.data_offset, 0x40);
        assert_eq!(d.asset_type(), AssetType::Tmd);
    }

    #[test]
    fn types_round_trip() {
        for b in [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0x0A, 0x0B, 0x0F, 0x14, 0xAA] {
            let t = AssetType::from_byte(b);
            assert!(!t.name().is_empty());
        }
    }

    #[test]
    fn flag_types_recognized() {
        for b in [0x0Au8, 0x0F, 0x14] {
            let t = AssetType::from_byte(b);
            assert!(
                matches!(t, AssetType::Flag(x) if x == b),
                "byte 0x{:02X} should map to Flag, got {:?}",
                b,
                t
            );
            assert!(!t.has_data());
        }
    }

    #[test]
    fn decode_raw_copies_slice() {
        let buf = b"....HELLO WORLD....";
        let d = Descriptor {
            type_byte: 0,
            size: 11,
            data_offset: 4,
        };
        let out = decode(buf, &d, DecodeMode::Raw).unwrap();
        assert_eq!(out, b"HELLO WORLD");
    }

    #[test]
    fn streaming_parses_two_chunks_and_terminator() {
        // chunk 1: type=2 (TMD), size=8, data = [0x41, 0, 0, 0, ...]
        // chunk 2: type=0 (TIM), size=4, data = [0x10, 0, 0, 0]
        // terminator: 0x00000000
        let mut file = Vec::new();
        // chunk 1 header: (2 << 24) | 8 = 0x02000008
        file.extend_from_slice(&0x02_000008u32.to_le_bytes());
        file.extend_from_slice(&[0x41, 0x00, 0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF]);
        // chunk 2 header: (0 << 24) | 4 = 0x00000004
        file.extend_from_slice(&0x00_000004u32.to_le_bytes());
        file.extend_from_slice(&[0x10, 0x00, 0x00, 0x00]);
        // terminator
        file.extend_from_slice(&0u32.to_le_bytes());

        let r = parse_streaming(&file, 16).unwrap();
        assert_eq!(r.chunks.len(), 2);
        assert!(r.terminated);
        assert!(r.all_known_types);
        assert!(r.all_magic_ok);
        assert_eq!(r.chunks[0].type_byte, 0x02);
        assert_eq!(r.chunks[0].size, 8);
        assert_eq!(r.chunks[0].magic, "00000041");
        assert_eq!(r.chunks[1].type_byte, 0x00);
        assert_eq!(r.chunks[1].size, 4);
        assert_eq!(r.chunks[1].magic, "00000010");
    }

    #[test]
    fn streaming_stops_on_unterminated() {
        // Single chunk header claiming size=1024 but no data follows.
        let mut file = Vec::new();
        file.extend_from_slice(&0x00_000400u32.to_le_bytes()); // type=0, size=1024
        file.extend_from_slice(&[0x10, 0x00, 0x00, 0x00, 0xAA, 0xBB]); // only 6 bytes
        let r = parse_streaming(&file, 16).unwrap();
        assert_eq!(r.chunks.len(), 0);
        assert!(!r.terminated);
    }

    #[test]
    fn parse_player_lzs_layout() {
        // 8-byte meta + 2 descriptors
        let mut file = Vec::new();
        file.extend_from_slice(&0u32.to_le_bytes()); // meta[0]
        file.extend_from_slice(&0u32.to_le_bytes()); // meta[1]
        file.extend_from_slice(&0x02_001000u32.to_le_bytes()); // type=2 (TMD), size=0x1000
        file.extend_from_slice(&0x40u32.to_le_bytes()); // offset
        file.extend_from_slice(&0x00_000800u32.to_le_bytes()); // type=0 (TIM), size=0x800
        file.extend_from_slice(&0x80u32.to_le_bytes());

        let c = parse_player_lzs(&file, 2).unwrap();
        assert_eq!(c.descriptors.len(), 2);
        assert_eq!(c.descriptors[0].asset_type(), AssetType::Tmd);
        assert_eq!(c.descriptors[0].size, 0x1000);
        assert_eq!(c.descriptors[1].asset_type(), AssetType::Tim);
        assert_eq!(c.descriptors[1].size, 0x800);
    }
}
