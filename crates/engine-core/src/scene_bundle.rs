//! Scene-bundle loader: locates the per-scene asset table inside a loaded
//! [`crate::scene::Scene`] and walks the descriptors into typed sub-assets.
//!
//! PORT: FUN_8001F7C0, FUN_8001F05C
//!
//! Mirrors the runtime field-loader chain in [`docs/subsystems/asset-loader.md`]:
//! `FUN_8001F7C0` reads the scene-name path, opens the bundle entry, and the
//! dispatcher at `FUN_8001F05C` walks the 7 descriptors. The retail engine
//! decompressed the payload region into a per-scene working buffer and then
//! resolved each descriptor's `data_offset` within that buffer; for engines
//! we expose just the typed payload slices.
//!
//! The descriptor table format is documented in
//! [`docs/formats/scene-bundles.md`] under `scene_asset_table` and
//! `scene_scripted_asset_table`.

use anyhow::Result;
use legaia_asset::categorize::Class;
use legaia_asset::{
    AssetType, Descriptor, scene_asset_table::SceneAssetTable,
    scene_scripted_asset_table::SceneScriptedAssetTable,
};

use crate::scene::{Scene, SceneEntry};

/// Where the scene's 7-descriptor asset table lives within a PROT entry.
///
/// `Plain` = a `Class::SceneAssetTable` entry whose file starts directly
/// with the table (lead at byte 0).
///
/// `Scripted` = a `Class::SceneScriptedAssetTable` entry whose file starts
/// with the prescript and the table sits at a 0x800-aligned offset past
/// the records.
#[derive(Debug, Clone)]
pub enum BundleSource<'a> {
    Plain {
        entry: &'a SceneEntry,
        table: SceneAssetTable,
    },
    Scripted {
        entry: &'a SceneEntry,
        info: SceneScriptedAssetTable,
        table: SceneAssetTable,
    },
}

impl<'a> BundleSource<'a> {
    /// PROT index of the entry the bundle came from.
    pub fn entry_idx(&self) -> u32 {
        match self {
            BundleSource::Plain { entry, .. } => entry.idx,
            BundleSource::Scripted { entry, .. } => entry.idx,
        }
    }

    /// The 7 descriptors with their packed `(type_byte, size, data_offset)`.
    pub fn descriptors(&self) -> [Descriptor; 7] {
        let table = match self {
            BundleSource::Plain { table, .. } => table,
            BundleSource::Scripted { table, .. } => table,
        };
        let mut out = [Descriptor {
            type_byte: 0,
            size: 0,
            data_offset: 0,
        }; 7];
        for (i, d) in table.descriptors.iter().enumerate() {
            out[i] = Descriptor {
                type_byte: d.type_byte,
                size: d.size,
                data_offset: d.data_offset,
            };
        }
        out
    }

    /// Byte offset within the entry where the descriptor table starts.
    /// `0` for `Plain`, the 0x800-aligned offset for `Scripted`.
    pub fn table_offset(&self) -> usize {
        match self {
            BundleSource::Plain { .. } => 0,
            BundleSource::Scripted { info, .. } => info.asset_table_offset,
        }
    }

    /// Raw entry bytes.
    pub fn bytes(&self) -> &[u8] {
        match self {
            BundleSource::Plain { entry, .. } => &entry.bytes,
            BundleSource::Scripted { entry, .. } => &entry.bytes,
        }
    }
}

/// Find the scene's 7-descriptor asset table inside a loaded scene.
///
/// Walks the scene entries in CDNAME order and returns the first
/// `SceneAssetTable` or `SceneScriptedAssetTable` entry whose detector
/// returns a valid table. Returns `None` for scenes that don't carry an
/// asset bundle (e.g. title-screen scenes that are pure asset bundles
/// without a per-scene descriptor table).
pub fn find_bundle<'a>(scene: &'a Scene) -> Option<BundleSource<'a>> {
    for entry in &scene.entries {
        match entry.class {
            Class::SceneAssetTable => {
                if let Some(t) = legaia_asset::scene_asset_table::detect(&entry.bytes) {
                    return Some(BundleSource::Plain { entry, table: t });
                }
            }
            Class::SceneScriptedAssetTable => {
                if let Some(info) = legaia_asset::scene_scripted_asset_table::detect(&entry.bytes)
                    && let Some(table) = legaia_asset::scene_asset_table::detect(
                        &entry.bytes[info.asset_table_offset..],
                    )
                {
                    return Some(BundleSource::Scripted { entry, info, table });
                }
            }
            _ => {}
        }
    }
    None
}

/// Per-descriptor extraction: descriptor metadata plus the file-relative
/// payload range. `payload_start` and `payload_end` are byte offsets into
/// the entry's raw bytes; engines slice `entry.bytes[start..end]` and pass
/// to the per-type decoder ([`legaia_lzs::decompress`] for compressed
/// payloads, raw copy otherwise).
#[derive(Debug, Clone)]
pub struct ExtractedDescriptor {
    /// Asset type (canonical mapping from the dispatcher table).
    pub asset_type: AssetType,
    /// `(type_byte, size, data_offset)` straight from the descriptor.
    pub descriptor: Descriptor,
    /// Position 0..6 in the table.
    pub index: usize,
    /// Byte offset into the entry's raw bytes where the payload starts.
    pub payload_start: usize,
    /// Byte offset where the payload ends (`start + size`).
    pub payload_end: usize,
}

impl ExtractedDescriptor {
    /// Slice the payload bytes from the entry. Returns `None` if the
    /// derived range falls outside the entry buffer.
    pub fn payload<'a>(&self, entry_bytes: &'a [u8]) -> Option<&'a [u8]> {
        entry_bytes.get(self.payload_start..self.payload_end)
    }
}

/// Walk the bundle's 7 descriptors. Descriptor 0 carries an authoritative
/// file-relative `data_offset` (always `0x40` past the table header); the
/// other six carry runtime-buffer offsets that don't address bytes within
/// the on-disc file.
///
/// The result is the descriptor metadata for all seven plus, for descriptor
/// 0 only, a file-relative byte range pointing at its LZS-compressed
/// payload. Decompress with [`extract_descriptor_0_lzs`] to materialise the
/// bytes.
///
/// Descriptors 1..6 are surfaced for completeness but `payload_start` /
/// `payload_end` are zeroed - the retail loader resolves them inside its
/// per-scene working buffer that the on-disc bytes don't fully populate
/// (see the asset-loader subsystem doc). Engines that need those payloads
/// drive the streaming loader chain (`tim.dat` / `move.mdt`) instead of
/// reading them from this entry.
pub fn walk_descriptors(bundle: &BundleSource) -> Vec<ExtractedDescriptor> {
    let descriptors = bundle.descriptors();
    let table_offset = bundle.table_offset();

    let mut out = Vec::with_capacity(7);
    for (i, d) in descriptors.iter().enumerate() {
        let asset_type = d.asset_type();
        let (payload_start, payload_end) = if i == 0 {
            let start = (table_offset as u32 + d.data_offset) as usize;
            // The runtime LZS-decompresses the payload at runtime - its
            // on-disc length isn't the descriptor.size; that's the
            // post-decompression size. We expose `start` only.
            (start, start)
        } else {
            (0, 0)
        };
        out.push(ExtractedDescriptor {
            asset_type,
            descriptor: *d,
            index: i,
            payload_start,
            payload_end,
        });
    }
    out
}

/// LZS-decompress descriptor 0's payload (the `TIM_LIST` in canonical
/// scenes). Returns the decompressed bytes plus the number of input bytes
/// the decoder consumed.
///
/// Descriptor 0 is the only descriptor with a reliably file-resident
/// payload - the dispatcher LZS-decodes it in place, then walks the
/// per-mesh descriptor chain in the resulting buffer. See
/// [`docs/formats/asset-type.md`] for the LZS-vs-raw decision.
///
/// Errors when:
///  - `data_offset` is past the bundle bytes,
///  - the LZS decoder hits a malformed stream,
///  - the decoded length doesn't match `descriptor.size`.
pub fn extract_descriptor_0_lzs(bundle: &BundleSource) -> Result<(Vec<u8>, usize)> {
    let descriptors = bundle.descriptors();
    let d = descriptors[0];
    let table_offset = bundle.table_offset();
    let payload_start = (table_offset as u32 + d.data_offset) as usize;
    let bytes = bundle.bytes();
    if payload_start >= bytes.len() {
        return Err(anyhow::anyhow!(
            "descriptor 0 payload starts past entry end ({} >= {})",
            payload_start,
            bytes.len()
        ));
    }
    let body = &bytes[payload_start..];
    let (decoded, consumed) = legaia_lzs::decompress_tracked(body, d.size as usize)?;
    Ok((decoded, consumed))
}

/// Materialise the scene's `Asset(0x05) = Move` payload as a flat byte
/// blob suitable for installing as the MOVE pool root (retail
/// `_DAT_8007B888`). Mirrors the per-scene `move.mdt` install documented
/// in [`docs/formats/mdt.md`]: when a field scene loads, descriptor 4 of
/// the `scene_asset_table` bundle is the per-area move-table that
/// `FUN_800204F8` reads via [`legaia_engine_vm::move_buffer`].
///
/// Each descriptor in the scene asset table is its own independently
/// LZS-compressed stream. `data_offset` is the file-relative byte
/// position of that stream and `size` is the **decompressed** payload
/// size that the dispatcher passes to [`legaia_lzs::decompress`]. So
/// the Move payload is `LZS.decode(entry_bytes[desc[4].data_offset..],
/// desc[4].size)` directly.
///
/// `entry_bytes` is the **full on-disc footprint** of the bundle entry
/// (from [`legaia_prot::archive::Archive::read_entry`] / the
/// `entry_bytes_extended` accessor on `ProtIndex`), not the indexed
/// sub-region. Several scene_asset_table entries (e.g. `0588_juui1`)
/// have descriptor offsets that fall past the TOC-indexed end and into
/// the trailing-overlay sectors; those offsets are valid against the
/// extended footprint. See [`docs/formats/prot.md`] §"Trailing-overlay
/// sectors".
///
/// Returns `Ok(None)` for:
///  - Bundles whose descriptor table doesn't carry a Move slot
///    (the `(1, 2, 3, 4, 6, 7, 0x14)` skip-Move variant; 1/80 entries).
///  - Bundles whose Move descriptor has zero size.
///  - Bundles where the LZS-decoded payload doesn't validate as a
///    `legaia_mdt::MoveBuffer` (via
///    [`legaia_mdt::MoveBuffer::looks_like_move_buffer`], not the strict
///    `fitness` score - see that method's doc).
///
/// Returns `Err` only for genuinely malformed inputs (data offset past
/// entry end, LZS decoder fails on the bytes). The "no Move table for
/// this scene" case is `Ok(None)` so callers can branch on `Option`
/// rather than catching errors.
pub fn extract_move_payload(bundle: &BundleSource, entry_bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    let descriptors = bundle.descriptors();
    let Some(move_desc) = descriptors
        .iter()
        .find(|d| matches!(d.asset_type(), AssetType::Move))
        .copied()
    else {
        return Ok(None);
    };
    if move_desc.size == 0 || move_desc.data_offset == 0 {
        return Ok(None);
    }

    let table_offset = bundle.table_offset();
    let payload_start = table_offset + move_desc.data_offset as usize;
    if payload_start >= entry_bytes.len() {
        return Err(anyhow::anyhow!(
            "Move descriptor offset 0x{:X} past entry end ({}b)",
            payload_start,
            entry_bytes.len()
        ));
    }
    let body = &entry_bytes[payload_start..];

    let (decoded, _consumed) = legaia_lzs::decompress_tracked(body, move_desc.size as usize)?;
    if decoded.len() != move_desc.size as usize {
        return Ok(None);
    }
    if !move_payload_looks_valid(&decoded) {
        return Ok(None);
    }
    Ok(Some(decoded))
}

/// Predicate used by [`extract_move_payload`] to gate installation.
///
/// Thin wrapper around [`legaia_mdt::MoveBuffer::looks_like_move_buffer`];
/// see that method's doc for why the strict
/// [`legaia_mdt::MoveBuffer::fitness`] check is false-negative on real
/// per-scene Move data.
fn move_payload_looks_valid(buf: &[u8]) -> bool {
    legaia_mdt::MoveBuffer::parse(buf)
        .map(|mb| mb.looks_like_move_buffer())
        .unwrap_or(false)
}

/// Index every TIM that the scene exposes via the `TimList` descriptor
/// or as scattered `Class::SceneVabStream` / `Class::SceneTmdStream`
/// neighbours.
///
/// The asset-loader chain pulls **every** TIM in the scene's CDNAME block
/// into VRAM before any TMD is rendered - that's what binds CLUTs that
/// scatter across PROT entries (see `docs/subsystems/asset-loader.md`
/// CLUT-data scattering section).
///
/// Returns one `(entry_idx, tim_offset_in_entry)` pair per TIM the
/// engine should upload. Engines slice `entry.bytes[tim_offset..]`,
/// hand it to [`legaia_tim::parse`], and upload the image + CLUT to
/// the software VRAM at the TIM's framebuffer coordinates.
///
/// Scope: scans every entry in `scene.entries`, runs the TIM detector at
/// every byte offset (cheap - TIM magic is `0x10` + four-byte header).
pub fn scene_tim_layout(scene: &Scene) -> Result<Vec<TimLocation>> {
    let mut out = Vec::new();
    for entry in &scene.entries {
        for hit in legaia_asset::tim_scan::scan_buffer(&entry.bytes) {
            out.push(TimLocation {
                entry_idx: entry.idx,
                offset: hit.offset,
                kind: TimKind::Raw,
                width: hit.width,
                height: hit.height,
                bpp: hit.bpp,
                has_clut: hit.has_clut,
                byte_len: hit.byte_len,
            });
        }
    }
    Ok(out)
}

/// One TIM the scene exposes for VRAM upload. Mirrors the runtime
/// asset-loader's "load every TIM in this scene before drawing TMDs"
/// pre-pass.
///
/// The framebuffer coordinates aren't surfaced here - they live inside
/// the TIM header proper. Engines parse with [`legaia_tim::parse`] at
/// `entry_bytes[offset..]` to get `fb_x` / `fb_y` / `clut_fb_x` /
/// `clut_fb_y` for the VRAM upload.
#[derive(Debug, Clone, Copy)]
pub struct TimLocation {
    pub entry_idx: u32,
    pub offset: usize,
    pub kind: TimKind,
    pub width: u32,
    pub height: u32,
    pub bpp: u32,
    pub has_clut: bool,
    pub byte_len: usize,
}

/// Where the TIM was found - raw entry bytes vs. a post-LZS slice.
/// Currently we only emit `Raw`; LZS sub-paths can be added later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimKind {
    Raw,
    Lzs,
}

/// Convenience: collect per-type descriptor counts (e.g. for diagnostic
/// overlays). Returns `[(type_name, count), ...]` over the 7 descriptors.
pub fn descriptor_type_summary(bundle: &BundleSource) -> Vec<(&'static str, usize)> {
    let mut counts: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    for d in bundle.descriptors().iter() {
        *counts.entry(d.asset_type().name()).or_insert(0) += 1;
    }
    counts.into_iter().collect()
}

/// Per-record event-script ranges for a scripted bundle.
#[derive(Debug, Clone)]
pub struct ScriptedEventRecords {
    /// PROT entry index that carries the prescript.
    pub entry_idx: u32,
    /// `(start, end)` byte ranges per record, in the same order the
    /// runtime field VM dispatches them.
    pub ranges: Vec<(usize, usize)>,
}

impl ScriptedEventRecords {
    pub fn len(&self) -> usize {
        self.ranges.len()
    }
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }
}

/// Find the **lead** scripted-asset-table entry's per-record event
/// scripts. Convenience wrapper for engines that want to skip directly to
/// the field-VM bytecode without going through `Scene::find_event_scripts`.
///
/// Returns `None` if the bundle isn't a scripted table or has no records.
pub fn scripted_event_record_ranges(bundle: &BundleSource) -> Option<ScriptedEventRecords> {
    if let BundleSource::Scripted { entry, .. } = bundle {
        let ranges = legaia_asset::scene_scripted_asset_table::record_ranges(&entry.bytes)?;
        if !ranges.is_empty() {
            return Some(ScriptedEventRecords {
                entry_idx: entry.idx,
                ranges,
            });
        }
    }
    None
}

/// Scan the scene's entries for the first asset-type-0x07 (`VDF` /
/// `set_mime`) streaming chunk and return its body bytes.
///
/// The VDF buffer is the retail `DAT_8007B7DC` install target the
/// asset-dispatcher case 7 builds; the body bytes are the
/// `[u32 count][u32 byte_offsets[count]][bodies]` layout the field-VM
/// `0x4C 0xD8` opcode resolves via [`crate::world::World::vdf_record_bytes`].
///
/// Returns `None` when no VDF chunk is reachable from the scene. Some
/// scenes (utility / cutscene / world-map) carry no VDF data; that's
/// not an error.
///
/// **Heuristic note:** picks the *first* VDF chunk found in CDNAME
/// order. Of 124 scenes in the retail corpus only 8 carry VDF chunks,
/// and each carries exactly one - so the "first" choice matches retail
/// behaviour. If a future PROT layout ever surfaces multiple VDF chunks
/// per scene, this needs to be revisited.
pub fn find_vdf_buffer(scene: &Scene) -> Option<Vec<u8>> {
    for entry in &scene.entries {
        let Ok(report) = legaia_asset::parse_streaming(&entry.bytes, 4096) else {
            continue;
        };
        for c in &report.chunks {
            if matches!(AssetType::from_byte(c.type_byte), AssetType::Vdf) {
                let body_start = c.header_offset + 4;
                let body_end = body_start + c.size as usize;
                if body_end <= entry.bytes.len() {
                    return Some(entry.bytes[body_start..body_end].to_vec());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn synth_scene_asset_table_bytes(types: [u8; 7], total_size: usize) -> Vec<u8> {
        // Mirror the helper inside legaia_asset::scene_asset_table tests but
        // produce real packed payloads after the descriptor block so our
        // sequential walker has bytes to read.
        let header_end = 0x40u32;
        let mut buf = Vec::with_capacity(total_size);
        buf.extend_from_slice(&7u32.to_le_bytes()); // count
        buf.extend_from_slice(&0u32.to_le_bytes()); // meta1
        let payload_size: u32 = 0x100;
        let mut data_off = header_end;
        for &t in &types {
            let type_size = ((t as u32) << 24) | payload_size;
            buf.extend_from_slice(&type_size.to_le_bytes());
            buf.extend_from_slice(&data_off.to_le_bytes());
            data_off += payload_size;
        }
        // Pad / pack payloads - 7 * 0x100 = 0x700 bytes after the header.
        buf.resize(header_end as usize + (7 * payload_size as usize), 0);
        // Tail-pad to the requested total_size for entropy coverage.
        if total_size > buf.len() {
            buf.resize(total_size, 0);
        }
        buf
    }

    fn make_scene_with_plain_bundle(entry_bytes: Vec<u8>) -> Scene {
        Scene {
            name: "test".into(),
            start: 0,
            end: 1,
            entries: vec![SceneEntry {
                idx: 100,
                class: Class::SceneAssetTable,
                bytes: Arc::new(entry_bytes),
            }],
        }
    }

    #[test]
    fn find_bundle_locates_plain_scene_asset_table() {
        let bytes = synth_scene_asset_table_bytes([1, 2, 3, 4, 5, 6, 7], 0x1000);
        let scene = make_scene_with_plain_bundle(bytes);
        let bundle = find_bundle(&scene).expect("plain bundle should be found");
        assert_eq!(bundle.entry_idx(), 100);
        assert_eq!(bundle.table_offset(), 0);
        let descs = bundle.descriptors();
        assert_eq!(descs[0].asset_type(), AssetType::TimList);
        assert_eq!(descs[6].asset_type(), AssetType::Vdf);
    }

    #[test]
    fn walk_descriptors_emits_seven_with_authoritative_first() {
        let bytes = synth_scene_asset_table_bytes([1, 2, 3, 4, 5, 6, 7], 0x800);
        let scene = make_scene_with_plain_bundle(bytes);
        let bundle = find_bundle(&scene).unwrap();
        let xs = walk_descriptors(&bundle);
        assert_eq!(xs.len(), 7);
        // Descriptor 0 has a real file-relative offset.
        assert_eq!(xs[0].payload_start, 0x40);
        // Descriptors 1..6 carry runtime-buffer offsets - payload range is
        // zeroed.
        for (i, x) in xs.iter().enumerate().skip(1) {
            assert_eq!(x.payload_start, 0, "desc[{i}] should have no file range");
            assert_eq!(x.payload_end, 0, "desc[{i}] should have no file range");
        }
    }

    #[test]
    fn descriptor_type_summary_counts_by_type_name() {
        let bytes = synth_scene_asset_table_bytes([1, 2, 3, 4, 5, 6, 7], 0x800);
        let scene = make_scene_with_plain_bundle(bytes);
        let bundle = find_bundle(&scene).unwrap();
        let summary = descriptor_type_summary(&bundle);
        // Each canonical type appears exactly once.
        let names: Vec<&str> = summary.iter().map(|(n, _)| *n).collect();
        for expected in ["TIM_LIST", "TMD", "MAN", "MES", "MOVE", "ANM", "VDF"] {
            assert!(
                names.contains(&expected),
                "missing {expected} in summary {summary:?}"
            );
        }
    }

    #[test]
    fn find_bundle_returns_none_for_scene_with_no_asset_table() {
        let scene = Scene {
            name: "empty".into(),
            start: 0,
            end: 1,
            entries: vec![SceneEntry {
                idx: 0,
                class: Class::Empty,
                bytes: Arc::new(vec![]),
            }],
        };
        assert!(find_bundle(&scene).is_none());
    }

    #[test]
    fn scripted_event_record_ranges_returns_none_for_plain_bundles() {
        let bytes = synth_scene_asset_table_bytes([1, 2, 3, 4, 5, 6, 7], 0x800);
        let scene = make_scene_with_plain_bundle(bytes);
        let bundle = find_bundle(&scene).unwrap();
        assert!(scripted_event_record_ranges(&bundle).is_none());
    }

    /// LZS-encode `input` as a literal-only stream: every input byte
    /// becomes a literal under an all-ones control byte. Decoding via
    /// `legaia_lzs::decompress(.., input.len())` yields `input` verbatim.
    /// Used only by the test synth helpers since the production code
    /// path never produces LZS bytes - the decoder consumes retail
    /// streams that an external encoder produced.
    fn lzs_encode_literals(input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len() + input.len().div_ceil(8));
        for chunk in input.chunks(8) {
            let mut control: u8 = 0;
            for i in 0..chunk.len() {
                control |= 1 << i;
            }
            out.push(control);
            out.extend_from_slice(chunk);
        }
        out
    }

    /// Build a synthetic Move buffer (offset-table layout) whose id 7
    /// maps to a single record past the 4 KB offset-table region.
    fn synth_move_buffer() -> Vec<u8> {
        let size: usize = 0x1100; // 4 KB table + 256 B record region
        let id: usize = 7;
        let record_off: u32 = 0x1000;
        let mut buf = vec![0u8; size];
        buf[id * 4..id * 4 + 4].copy_from_slice(&record_off.to_le_bytes());
        buf[record_off as usize + 2] = 8; // max_position_x16 low
        buf[record_off as usize + 6] = 1; // divisor
        buf
    }

    /// Build a synthetic `scene_asset_table` PROT entry where each
    /// descriptor's `data_offset` points at a per-descriptor LZS stream
    /// (matching the on-disc layout that `extract_move_payload`
    /// consumes). Descriptor 4 carries a Move buffer that
    /// `legaia_mdt::MoveBuffer::parse` accepts with positive fitness.
    fn synth_scene_with_valid_move_payload() -> Vec<u8> {
        let header_end: u32 = 0x40;
        let types: [u8; 7] = [1, 2, 3, 4, 5, 6, 7];
        // Each descriptor's "size" is the decompressed payload size.
        // Tiny non-Move sizes are fine - the production extractor only
        // reads descriptor 4.
        let small_size: u32 = 0x10;
        let move_buffer = synth_move_buffer();
        let move_size: u32 = move_buffer.len() as u32;
        let sizes: [u32; 7] = [
            small_size, small_size, small_size, small_size, move_size, small_size, small_size,
        ];

        // Empty (zero-length) LZS streams aren't decodable, so each
        // non-Move slot still needs a literal stream of `size` bytes.
        let small_zeroes = vec![0u8; small_size as usize];
        let small_encoded = lzs_encode_literals(&small_zeroes);
        let move_encoded = lzs_encode_literals(&move_buffer);

        // Compute file-relative offsets for each descriptor's LZS stream.
        let mut offsets = [0u32; 7];
        let mut cursor = header_end;
        for (i, slot) in offsets.iter_mut().enumerate() {
            *slot = cursor;
            cursor += if i == 4 {
                move_encoded.len() as u32
            } else {
                small_encoded.len() as u32
            };
        }
        let total = cursor as usize;

        // Assemble.
        let mut buf = Vec::with_capacity(total);
        buf.extend_from_slice(&7u32.to_le_bytes()); // count
        buf.extend_from_slice(&0u32.to_le_bytes()); // meta1
        for ((t, sz), off) in types.iter().zip(sizes.iter()).zip(offsets.iter()) {
            let type_size = ((*t as u32) << 24) | *sz;
            buf.extend_from_slice(&type_size.to_le_bytes());
            buf.extend_from_slice(&off.to_le_bytes());
        }
        // Pad to header_end.
        buf.resize(header_end as usize, 0);
        // Append per-descriptor LZS streams in offset order.
        for i in 0..7 {
            if i == 4 {
                buf.extend_from_slice(&move_encoded);
            } else {
                buf.extend_from_slice(&small_encoded);
            }
        }
        debug_assert_eq!(buf.len(), total);
        buf
    }

    #[test]
    fn extract_move_payload_returns_slice_when_move_slot_present() {
        let bytes = synth_scene_with_valid_move_payload();
        let scene = make_scene_with_plain_bundle(bytes.clone());
        let bundle = find_bundle(&scene).expect("bundle present");
        let payload = extract_move_payload(&bundle, &bytes)
            .expect("no error")
            .expect("Move slot present");
        // The Move descriptor in the synth carries 0x1100 bytes.
        assert_eq!(payload.len(), 0x1100);
        let mb = legaia_mdt::MoveBuffer::parse(&payload).unwrap();
        assert!(
            mb.fitness() > 0,
            "synthetic Move buffer should validate; got fitness {} bogus {}",
            mb.fitness(),
            mb.bogus_offsets
        );
        assert_eq!(mb.used_slots.len(), 1);
        assert_eq!(mb.used_slots[0].move_id, 7);
        assert_eq!(mb.used_slots[0].raw_offset, 0x1000);
    }

    #[test]
    fn extract_move_payload_returns_none_when_move_slot_absent() {
        // `(1, 2, 3, 4, 6, 7, 0x14)` is the skip-Move variant (1/80 in corpus).
        let bytes = synth_scene_asset_table_bytes([1, 2, 3, 4, 6, 7, 0x14], 0x800);
        let scene = make_scene_with_plain_bundle(bytes.clone());
        let bundle = find_bundle(&scene).expect("bundle present");
        assert!(extract_move_payload(&bundle, &bytes).unwrap().is_none());
    }

    #[test]
    fn extract_move_payload_returns_none_for_unrecoverable_garbage() {
        // Default zero-payload synthetic: the Move descriptor's
        // `data_offset` points into a region of zeros, which LZS-decodes
        // to zeros and parses to a `MoveBuffer` with fitness 0. The
        // extractor should reject it rather than installing garbage.
        let bytes = synth_scene_asset_table_bytes([1, 2, 3, 4, 5, 6, 7], 0x800);
        let scene = make_scene_with_plain_bundle(bytes.clone());
        let bundle = find_bundle(&scene).expect("bundle present");
        assert!(extract_move_payload(&bundle, &bytes).unwrap().is_none());
    }
}
