//! Scene-bundle loader: locates the per-scene asset table inside a loaded
//! [`crate::scene::Scene`] and walks the descriptors into typed sub-assets.
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
}
