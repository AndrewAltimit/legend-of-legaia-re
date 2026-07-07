//! Per-scene typed asset snapshot - the runtime view of a CDNAME block once
//! it's been loaded by [`crate::scene::Scene::load`].
//!
//! Bridges the gap between the on-disc per-CDNAME-block layout and the
//! runtime asset chain documented in [`docs/subsystems/asset-loader.md`]:
//! `FUN_8001F7C0` and `FUN_800255B8` build paths under
//! `DATA\FIELD\<scene>\` and `h:\PROT\FIELD\<scene>\` to fetch six file
//! types per scene (TIM list, TMDs, MES, MOVE, ANM, VDF). Descriptors
//! 1..=6 inside the scene-asset table carry runtime-buffer offsets that
//! don't address bytes in the on-disc PROT entry, so this module produces
//! the typed view by sweeping every entry in the scene's CDNAME block
//! rather than relying on those offsets.
//!
//! Engines build a [`SceneAssets`] once per scene transition and query it
//! through the [`crate::scene::SceneHost`] for the duration of the scene.
//!
//! PORT: FUN_8001F7C0, FUN_800255B8
//! REF: FUN_80026B4C

use legaia_asset::categorize::Class;
use legaia_asset::{tim_scan, tmd_scan};
use legaia_mes::{Format as MesFormat, RECORD_MARKER};

use crate::scene::{Scene, SceneEntry};
use crate::scene_bundle;

/// One TIM image the scene exposes for VRAM upload. Mirrors the runtime
/// asset-loader's "load every TIM in this scene before drawing TMDs"
/// pre-pass - the asset chain pulls every TIM in the scene's CDNAME block
/// into VRAM up front so cross-entry CLUT references resolve.
#[derive(Debug, Clone, Copy)]
pub struct SceneTim {
    pub entry_idx: u32,
    /// Byte offset of the TIM's file magic within the entry's bytes.
    pub offset: usize,
    pub byte_len: usize,
    pub width: u32,
    pub height: u32,
    pub bpp: u32,
    pub has_clut: bool,
}

/// One TMD model the scene exposes. The retail asset chain registers each
/// TMD via `FUN_80026B4C` into the per-scene mesh pointer table at
/// `0x8007C018 + idx*4`; this module surfaces every TMD-magic hit in the
/// scene's entries so engines can index by `(entry_idx, offset)` until the
/// runtime registration order is reverse-engineered.
#[derive(Debug, Clone, Copy)]
pub struct SceneTmd {
    pub entry_idx: u32,
    pub offset: usize,
    pub byte_len: usize,
    pub n_obj: u32,
}

/// MES dialog container resolved out of one of the scene's PROT entries.
/// Holds an owned copy of the entry bytes plus the parsed offset table /
/// record markers so [`SceneAssets::mes_message_bytes`] can resolve a
/// `text_id` to a bytecode slice without re-parsing.
///
/// Two formats coexist (see [`docs/formats/mes.md`]):
/// - [`MesFormat::Compact`] - 0x404 magic + 16-byte runtime header at
///   `0x28` + offset table from `0x62..0xC8` (3-byte little-endian
///   offsets). Bytecode lives past the table at `0xC8`.
/// - [`MesFormat::Records`] - variable-stride records marked by
///   `0x44 0x78` (per-record byte counts inferred from neighbouring
///   markers).
#[derive(Debug, Clone)]
pub struct SceneMes {
    pub entry_idx: u32,
    /// Byte offset where the MES blob starts within `bytes`. Always 0 for
    /// entries whose entire body is the MES container.
    pub offset: usize,
    /// Owned blob bytes (the slice the offset table indexes into).
    pub bytes: Vec<u8>,
    pub format: MesFormat,
    /// `Compact` only - 3-byte LE offset table from `0x62..0xC8`,
    /// rebuilt as 32-bit values. `None` for `Records`.
    pub offset_table: Option<Vec<u32>>,
    /// `Records` only - record-start offsets (where each `0x44 0x78`
    /// marker lives). Empty for `Compact`.
    pub record_offsets: Vec<usize>,
}

impl SceneMes {
    /// Resolve `text_id` to the bytecode start within [`SceneMes::bytes`].
    /// Returns `None` if the id is past the offset table or out of range
    /// for the records vector.
    pub fn message_offset(&self, text_id: u16) -> Option<usize> {
        match self.format {
            MesFormat::Compact => {
                let table = self.offset_table.as_ref()?;
                let raw = *table.get(text_id as usize)?;
                Some(raw as usize)
            }
            MesFormat::Records => self.record_offsets.get(text_id as usize).copied(),
        }
    }

    /// Borrow the bytecode slice starting at `text_id`'s offset. Slice runs
    /// to the buffer end - the iterator stops at the first
    /// [`legaia_mes::Token::EndOfMessage`].
    pub fn message_bytes(&self, text_id: u16) -> Option<&[u8]> {
        let off = self.message_offset(text_id)?;
        self.bytes.get(off..)
    }

    /// Number of messages - table length for `Compact`, marker count for
    /// `Records`.
    pub fn message_count(&self) -> usize {
        match self.format {
            MesFormat::Compact => self.offset_table.as_ref().map_or(0, Vec::len),
            MesFormat::Records => self.record_offsets.len(),
        }
    }
}

/// Typed snapshot of every asset the engine cares about after entering a
/// scene. Built once per scene transition by
/// [`crate::scene::SceneHost::enter_field_scene`] (or directly by engines
/// that drive their own scene loop) and queried by the per-VM Host impls,
/// the renderer, and the audio mixer.
///
/// All fields are owned: holding a `SceneAssets` across a subsequent scene
/// load is safe (the next load builds a fresh snapshot).
#[derive(Debug, Default, Clone)]
pub struct SceneAssets {
    pub scene_name: String,
    /// PROT-entry range `[start, end)` of the CDNAME block.
    pub block_range: (u32, u32),
    /// Decoded descriptor-0 payload (`TIM_LIST`) - LZS-decompressed bytes
    /// from the scene's bundle entry. `None` if the scene has no bundle
    /// or the decode failed.
    pub tim_list_decoded: Option<Vec<u8>>,
    /// Every TIM the scene exposes, across all entries (raw bytes - does
    /// not include TIMs that live inside the LZS-decoded `tim_list_decoded`
    /// buffer; engines that need those run [`legaia_asset::tim_scan`] over
    /// the decoded blob).
    pub tims: Vec<SceneTim>,
    /// Every TMD in the scene's entries.
    pub tmds: Vec<SceneTmd>,
    /// Best-fit MES container - the first `Compact`-magic hit wins; if none
    /// is found, the largest `Records`-format candidate is kept (Records
    /// detection has no fixed magic, so it can false-positive on entropy
    /// data - preferring Compact avoids that).
    pub mes: Option<SceneMes>,
    /// PROT entries the scene block tags as `Class::SeqContainer` - BGM
    /// source bytes, addressable by id via the scene's [`block_range`].
    pub seq_entries: Vec<u32>,
    /// PROT entries that carry a SEQ blob at a non-zero offset (typically
    /// inside a `scene_vab_stream` chunk-header wrapper). Each tuple is
    /// `(prot_idx, byte_offset_of_pqes_magic)`. Most retail scene BGM
    /// resolves through this list rather than `seq_entries` (raw SEQ at
    /// offset 0 is rare in the disc layout).
    pub seq_in_stream_entries: Vec<(u32, usize)>,
    /// PROT entries with VAB headers (sound banks).
    pub vab_entries: Vec<u32>,
    /// Per-record event-script bytecode. Each entry is one record's bytes
    /// verbatim (no frame-divider sentinel stripping - that happens in
    /// [`crate::world::World::load_field_record`]).
    pub event_records: Vec<Vec<u8>>,
}

impl SceneAssets {
    /// Build a snapshot from a loaded `Scene`. Sweeps every entry in the
    /// CDNAME block once, runs the TIM / TMD scanners, picks the best MES
    /// container, and surfaces SEQ + VAB entries by class.
    pub fn build(scene: &Scene) -> Self {
        let mut tims = Vec::new();
        let mut tmds = Vec::new();
        let mut seq_entries = Vec::new();
        let mut seq_in_stream_entries = Vec::new();
        let mut vab_entries = Vec::new();

        for entry in &scene.entries {
            for hit in tim_scan::scan_buffer(&entry.bytes) {
                tims.push(SceneTim {
                    entry_idx: entry.idx,
                    offset: hit.offset,
                    byte_len: hit.byte_len,
                    width: hit.width,
                    height: hit.height,
                    bpp: hit.bpp,
                    has_clut: hit.has_clut,
                });
            }
            for hit in tmd_scan::scan_buffer(&entry.bytes) {
                tmds.push(SceneTmd {
                    entry_idx: entry.idx,
                    offset: hit.offset,
                    byte_len: hit.byte_len,
                    n_obj: hit.n_obj,
                });
            }
            match entry.class {
                Class::SeqContainer => seq_entries.push(entry.idx),
                Class::SceneVabStream => vab_entries.push(entry.idx),
                _ => {}
            }
            // Search for pQES magic past offset 0 - most retail SEQ data
            // lives inside `scene_vab_stream` chunk wrappers, not at the
            // entry start. Entries already classified as `SeqContainer`
            // are tracked separately above.
            if entry.class != Class::SeqContainer
                && let Some(off) = find_seq_magic(&entry.bytes)
            {
                seq_in_stream_entries.push((entry.idx, off));
            }
        }

        let mes = pick_best_mes(scene);

        let tim_list_decoded = scene_bundle::find_bundle(scene)
            .and_then(|b| scene_bundle::extract_descriptor_0_lzs(&b).ok())
            .map(|(bytes, _)| bytes);

        let event_records = scene
            .find_event_scripts()
            .map(|scripts| {
                (0..scripts.len())
                    .filter_map(|i| scripts.record(i).map(<[u8]>::to_vec))
                    .collect()
            })
            .unwrap_or_default();

        Self {
            scene_name: scene.name.clone(),
            block_range: (scene.start, scene.end),
            tim_list_decoded,
            tims,
            tmds,
            mes,
            seq_entries,
            seq_in_stream_entries,
            vab_entries,
            event_records,
        }
    }

    /// Resolve `text_id` to the bytecode slice an MES interpreter can step
    /// through. `None` when the scene has no MES container or the id is
    /// out of range.
    pub fn mes_message_bytes(&self, text_id: u16) -> Option<&[u8]> {
        self.mes.as_ref()?.message_bytes(text_id)
    }

    /// Resolve a BGM id to a SEQ-bearing PROT entry's index. Mirrors the
    /// retail [`docs/subsystems/script-vm.md`] BGM lookup: scene-local ids
    /// (`< 2000`) live at `raw_define + 6 + id` in the raw-TOC frame =
    /// `block_range.0 + 8 + id` here ([`Scene`] windows are in the
    /// extraction frame, retail block first entry = the `.MAP`; the raw
    /// define is `block_range.0 + 2`). Ids `>= 2000` live in the global pool
    /// (not modeled here). Checks both standalone SEQ entries and the
    /// `scene_vab_stream`-wrapped form (where SEQ data lives after a 4-byte
    /// chunk header). Absolute slots pinned by the audio oracles.
    pub fn bgm_seq_entry(&self, bgm_id: u16) -> Option<u32> {
        if bgm_id >= 2000 {
            return None;
        }
        let target = self.block_range.0 + 8 + bgm_id as u32;
        if self.seq_entries.contains(&target)
            || self
                .seq_in_stream_entries
                .iter()
                .any(|(idx, _)| *idx == target)
        {
            Some(target)
        } else {
            None
        }
    }

    /// If the resolved BGM entry is wrapped (chunk-header in front of the
    /// SEQ data), return the byte offset where the `pQES` magic begins.
    /// Returns `0` for raw SEQ entries, `None` if `bgm_id` doesn't resolve.
    pub fn bgm_seq_offset(&self, bgm_id: u16) -> Option<usize> {
        let target = self.bgm_seq_entry(bgm_id)?;
        if self.seq_entries.contains(&target) {
            return Some(0);
        }
        self.seq_in_stream_entries
            .iter()
            .find(|(idx, _)| *idx == target)
            .map(|(_, off)| *off)
    }
}

/// Search a buffer for the `pQES` magic past offset 0 (i.e. wrapped in a
/// chunk-header container). Returns the offset of the first `p` byte, or
/// `None` if no pQES sub-string lives past offset 0.
///
/// Legaia SEQ data uses a u32 BE version field (rather than the u16 BE
/// PsyQ-doc form), so the validation reads 4 reserved/version bytes
/// before the PPQN word - see [`legaia_seq::parse_header`] for the
/// canonical reader.
fn find_seq_magic(buf: &[u8]) -> Option<usize> {
    const MAGIC: &[u8; 4] = b"pQES";
    if buf.len() < MAGIC.len() + 1 {
        return None;
    }
    // Cap the scan to a sensible budget - SEQ-wrapped entries are
    // typically small (< 256 KB). Anything past 4 MB is almost certainly
    // not a real wrapper.
    let scan_end = buf.len().min(4 * 1024 * 1024);
    for i in 1..scan_end.saturating_sub(MAGIC.len()) {
        if &buf[i..i + MAGIC.len()] == MAGIC && validate_seq_header(&buf[i..]) {
            return Some(i);
        }
    }
    None
}

/// Best-effort SEQ header validation: accepts either the u16-version PsyQ
/// shape (synthetic test data) or the u32-version Legaia shape (every
/// real disc SEQ examined). The two shapes differ only in whether the
/// PPQN word lives at +6 or +8.
fn validate_seq_header(buf: &[u8]) -> bool {
    if buf.len() < 15 {
        return false;
    }
    // u32 BE version path - the Legaia shape.
    let v32 = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let ppqn32 = u16::from_be_bytes([buf[8], buf[9]]);
    let tempo32_nz = buf[10] != 0 || buf[11] != 0 || buf[12] != 0;
    if v32 == 1 && ppqn32 > 0 && tempo32_nz {
        return true;
    }
    // u16 BE version fallback - synthetic / standard PsyQ shape.
    let v16 = u16::from_be_bytes([buf[4], buf[5]]);
    let ppqn16 = u16::from_be_bytes([buf[6], buf[7]]);
    let tempo16_nz = buf[8] != 0 || buf[9] != 0 || buf[10] != 0;
    v16 == 1 && ppqn16 > 0 && tempo16_nz
}

fn pick_best_mes(scene: &Scene) -> Option<SceneMes> {
    let mut compact_hit: Option<SceneMes> = None;
    let mut best_records: Option<SceneMes> = None;
    for entry in &scene.entries {
        if let Some(mes) = try_extract_mes(entry) {
            match mes.format {
                MesFormat::Compact => {
                    if compact_hit.is_none() {
                        compact_hit = Some(mes);
                    }
                }
                MesFormat::Records => {
                    if best_records
                        .as_ref()
                        .is_none_or(|b| b.record_offsets.len() < mes.record_offsets.len())
                    {
                        best_records = Some(mes);
                    }
                }
            }
        }
    }
    compact_hit.or(best_records)
}

fn try_extract_mes(entry: &SceneEntry) -> Option<SceneMes> {
    let bytes = entry.bytes.as_ref();
    let blob = legaia_mes::parse(bytes).ok()?;
    let format = blob.format;
    // Records detector matches any 2-byte 0x44 0x78 in the buffer - too
    // permissive for the scene-sweep use case unless we apply a minimum.
    // Require at least 4 marker hits before considering a Records-format
    // candidate (most real MES containers carry dozens).
    if format == MesFormat::Records {
        let count = blob.records.as_ref().map_or(0, Vec::len);
        if count < 4 {
            return None;
        }
    }
    let offset_table = blob.offset_table;
    let record_offsets = blob
        .records
        .map(|recs| recs.iter().map(|r| r.offset).collect())
        .unwrap_or_default();
    Some(SceneMes {
        entry_idx: entry.idx,
        offset: 0,
        bytes: bytes.to_vec(),
        format,
        offset_table,
        record_offsets,
    })
}

/// Detect format-only - bypasses [`legaia_mes::parse`] for the cheaper
/// pre-check used by [`pick_best_mes`].
#[allow(dead_code)]
fn looks_like_records(buf: &[u8]) -> bool {
    let mut count = 0usize;
    let mut i = 0usize;
    while i + 2 <= buf.len() {
        if buf[i..i + 2] == RECORD_MARKER {
            count += 1;
            if count >= 4 {
                return true;
            }
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Scene, SceneEntry};
    use std::sync::Arc;

    /// Build a synthetic Compact-MES blob: header + minimal 16-bit body
    /// region, plus a 1-entry offset table pointing at byte 0xC8.
    fn synth_compact_mes() -> Vec<u8> {
        let mut buf = vec![0u8; 0x100];
        // Magic: 0x00 0x00 0x04 0x04 (LE = 0x04040000) - but the format
        // detector wants u32_le == 0x404 i.e. bytes `0x04 0x04 0x00 0x00`.
        buf[0..4].copy_from_slice(&0x404u32.to_le_bytes());
        // Three-byte LE offset at 0x62: 0xC8 0x00 0x00 → entry 0 = 0xC8.
        buf[0x62] = 0xC8;
        buf[0x63] = 0x00;
        buf[0x64] = 0x00;
        // Bytecode: a single glyph then EndOfMessage.
        buf[0xC8] = 0x40; // glyph
        buf[0xC9] = 0x00; // end-of-message
        buf
    }

    fn make_scene(entries: Vec<SceneEntry>) -> Scene {
        Scene {
            name: "test".into(),
            start: 100,
            end: 100 + entries.len() as u32,
            entries,
        }
    }

    #[test]
    fn build_indexes_compact_mes() {
        let mes_bytes = synth_compact_mes();
        let scene = make_scene(vec![SceneEntry {
            idx: 105,
            class: Class::UnknownOther,
            bytes: Arc::new(mes_bytes),
        }]);
        let assets = SceneAssets::build(&scene);
        let mes = assets.mes.as_ref().expect("MES should be detected");
        assert_eq!(mes.format, MesFormat::Compact);
        assert_eq!(mes.entry_idx, 105);
        let table = mes.offset_table.as_ref().unwrap();
        assert_eq!(table[0], 0xC8);
        let bytes = assets.mes_message_bytes(0).unwrap();
        assert_eq!(bytes[0], 0x40);
    }

    #[test]
    fn build_collects_block_range() {
        let scene = make_scene(vec![SceneEntry {
            idx: 100,
            class: Class::Empty,
            bytes: Arc::new(vec![]),
        }]);
        let assets = SceneAssets::build(&scene);
        assert_eq!(assets.block_range, (100, 101));
        assert_eq!(assets.scene_name, "test");
    }

    #[test]
    fn mes_message_bytes_returns_none_past_offset_table() {
        let mes_bytes = synth_compact_mes();
        let scene = make_scene(vec![SceneEntry {
            idx: 100,
            class: Class::UnknownOther,
            bytes: Arc::new(mes_bytes),
        }]);
        let assets = SceneAssets::build(&scene);
        // Offset table covers 0x62..0xC8 - 102 bytes / 3 = 34 entries.
        // Indexing past 34 falls off the end of the table.
        assert!(assets.mes_message_bytes(34).is_none());
        assert!(assets.mes_message_bytes(100).is_none());
    }

    #[test]
    fn rejects_records_with_too_few_markers() {
        // Only 2 markers - below the 4-marker minimum, should not be
        // accepted as a Records MES.
        let mut buf = vec![0u8; 256];
        buf[10] = 0x44;
        buf[11] = 0x78;
        buf[100] = 0x44;
        buf[101] = 0x78;
        let scene = make_scene(vec![SceneEntry {
            idx: 100,
            class: Class::UnknownOther,
            bytes: Arc::new(buf),
        }]);
        let assets = SceneAssets::build(&scene);
        assert!(assets.mes.is_none(), "should reject low-marker Records");
    }

    #[test]
    fn bgm_seq_entry_uses_block_offset_math() {
        let scene = Scene {
            name: "t".into(),
            start: 100,
            end: 200,
            entries: vec![SceneEntry {
                idx: 113,
                class: Class::SeqContainer,
                // pQES + version + ppqn + tempo (just enough to classify).
                bytes: Arc::new(make_pqes_bytes()),
            }],
        };
        let assets = SceneAssets::build(&scene);
        // BGM id 5 → block_start (100) + 8 + 5 = 113 (raw_define+6+id frame).
        assert_eq!(assets.bgm_seq_entry(5), Some(113));
        // BGM id past the available SEQ entries returns None.
        assert_eq!(assets.bgm_seq_entry(0), None);
    }

    #[test]
    fn bgm_global_pool_returns_none() {
        let assets = SceneAssets::default();
        assert!(assets.bgm_seq_entry(2000).is_none());
        assert!(assets.bgm_seq_entry(3000).is_none());
    }

    fn make_pqes_bytes() -> Vec<u8> {
        // pQES + version 1 BE + ppqn 480 BE + tempo 500_000 us + 4/4 + EOT.
        let mut buf = b"pQES".to_vec();
        buf.extend_from_slice(&[0, 1]); // version 1 BE
        buf.extend_from_slice(&[0x01, 0xE0]); // ppqn 480 BE
        buf.extend_from_slice(&[0x07, 0xA1, 0x20]); // tempo 500_000 us
        buf.extend_from_slice(&[0x04, 0x02, 0x18, 0x08]); // 4/4
        // Minimum events to satisfy classifier (just delta+EOT).
        buf.push(0x00); // delta
        buf.extend_from_slice(&[0xFF, 0x2F, 0x00]); // EOT
        buf
    }
}
