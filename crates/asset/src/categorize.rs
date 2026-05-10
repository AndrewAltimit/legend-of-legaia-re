//! Bulk classification of unknown PROT entries.
//!
//! Tries every known parser against a buffer; if none match, falls back to
//! statistical features (entropy, byte distribution, leading-zero run).
//!
//! The point isn't to get every entry right - it's to shrink the
//! "uncategorized" pile so we can see clusters worth reversing next.

use serde::Serialize;

use crate::{AssetType, parse_player_lzs, parse_streaming};

/// Top-level classification result for one file.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    /// 0-byte file.
    Empty,
    /// < 32 bytes.
    Tiny,
    /// All bytes are 0x00.
    AllZeros,
    /// At least 95% of bytes are 0x00 (and at least one byte isn't).
    /// Distinct from [`Class::AllZeros`] because the non-zero bytes might
    /// signal a small terminator, count, or footer that's worth noting.
    /// Captures the "this PROT slot is reserved but never populated" shape.
    MostlyZeros,
    /// All bytes are the same non-zero value.
    ConstantByte,
    /// Starts with the PSX TIM magic (0x10).
    TimPassthrough,
    /// Parses as a DATA_FIELD streaming container (FUN_8002541c 0x14 branch).
    DataFieldStreaming,
    /// Sister of [`Class::DataFieldStreaming`] - leading chunks parse cleanly
    /// (all known types, all magic-OK) but the final chunk's declared `size`
    /// walks past EOF without a terminator. Real PROT entries (`0157_rikuroa`,
    /// `0228_station`, `0373_taiku`) carry a per-scene secondary table whose
    /// declared size exceeds the on-disc body - the runtime extends the chunk
    /// via streaming DMA continuation rather than a literal terminator.
    /// See [`crate::data_field_truncated`].
    DataFieldTruncated,
    /// Matches the standalone TIM-pack heuristic (`byte[3]==0x01 && byte[2]<0x10`).
    /// See `crates/prot/src/timpack.rs`.
    TimPack,
    /// Parses as a player.lzs-style descriptor container at some count
    /// (1, 2, 3, 4, 8, 16) and at least one descriptor decodes via LZS.
    LzsContainer,
    /// Contains a stage-geometry table (12-byte fixed prefix + 8-byte
    /// payload at 20-byte stride). See [`crate::stage_geom`].
    StageGeometry,
    /// Field-pack container - 4-byte magic + 97-entry schema followed by
    /// packed TIMs and TMDs. See [`crate::field_pack`].
    FieldPack,
    /// Effect-bundle container - magic `0x02018B0C` + constant header words +
    /// 28-entry schema followed by packed TMD primitive groups + TIMs.
    /// See [`crate::effect_bundle`].
    EffectBundle,
    /// PsyQ SEQ sequenced-music file - leads with the 4-byte ASCII magic
    /// `pQES` (0x70 0x51 0x45 0x53) followed by a 9-byte header. Drives the
    /// SsAPI sequencer at runtime. See [`docs/formats/seq.md`].
    SeqContainer,
    /// Legaia ANM animation pack - `[u32 count][u32 offset[count]][records...]`
    /// where every record's `+4..+6` u16 equals `0x080C` (the per-record
    /// marker_1). 8/8 hits across the title + town overlay corpus carry the
    /// marker, so the detector is zero-false-positive against random data.
    /// See [`docs/formats/anm.md`].
    AnmContainer,
    /// `[u32 size][bare TMD][streaming chunks]` - a streaming-format variant
    /// where the first asset is a Legaia TMD without a typed chunk header.
    /// See [`crate::scene_tmd_stream`].
    SceneTmdStream,
    /// `[u32 claimed_total][TMD magic][TMD flags=0][nobj]` - a TMD-fronted
    /// resource where the prefix u32 claims a total size *larger than the
    /// on-disc bytes*. The on-disc file is a prefix of a logical TMD whose
    /// remainder is supplied by the runtime (streaming tail elsewhere or
    /// zero-fill). Sister to [`Class::SceneTmdStream`] - captures the
    /// truncated subset that detector intentionally rejects.
    /// See [`crate::tmd_size_prefix`].
    TmdSizePrefix,
    /// `[u32 chunk0_header (type=0x00, size=N)][VABp sound bank][...]` -
    /// a streaming-format variant where the leading chunk is a Sony VAB
    /// instrument bank instead of a TMD. The single largest distributed
    /// VAB carrier in the corpus (200+ entries). See [`crate::scene_vab_stream`].
    SceneVabStream,
    /// Strict 8-word v12 header - `[N+4, 0x12, 0, 0x14, ?, N, 0, N+2]` - used
    /// by 97 scene-named PROT entries (one per scene). Format meaning open;
    /// likely candidates are per-scene navmesh / collision / event-trigger
    /// data. See [`crate::scene_v12_table`].
    SceneV12Table,
    /// Canonical 7-asset scene bundle - leads with `07 00 00 00`, then 7
    /// descriptor pairs (`(type<<24)|size, data_offset`) covering the
    /// `(TimList, Tmd, Man, Mes, Move, Anm, Vdf)` asset sequence. 80 PROT
    /// entries match. See [`crate::scene_asset_table`].
    SceneAssetTable,
    /// Composite shape: `[u16 count][u16 offsets[count]][record bodies]
    /// [zero pad to next 0x800 sector][canonical scene_asset_table]`. The
    /// leading prescript carries scene-event-script bytecode (likely
    /// field-VM frames) and the asset table at the next sector boundary
    /// holds the standard 7-asset scene bundle. 77 PROT entries match.
    /// See [`crate::scene_scripted_asset_table`].
    SceneScriptedAssetTable,
    /// `[u16 count][u16 offsets[count]][record bodies]` - same prescript
    /// shape as [`Class::SceneScriptedAssetTable`] but the post-prescript
    /// payload is **not** a canonical scene-asset-table. Detected when at
    /// least 50 % of records open with the field-VM frame sentinel
    /// `0xFFFF 0x0000`. ~20 PROT entries match. See
    /// [`crate::scene_event_scripts`].
    SceneEventScripts,
    /// MIPS code blob - the static disc copy of a runtime overlay. Leads
    /// with `addiu sp, sp, -X` (a function prologue) and a plausible MIPS
    /// follow-up instruction. 22 PROT entries match - all in the `0901..=0969`
    /// `xxx_dat` cluster. See [`crate::mips_overlay`].
    MipsOverlay,
    /// Sister cluster to [`Class::MipsOverlay`] - MIPS overlay code blob
    /// that leads with a function/jump-table header (4-64 consecutive u32
    /// values, each in the `0x801C0000..=0x801FFFFF` overlay window) instead
    /// of a `addiu sp, sp, -X` prologue. 42 PROT entries match - all in the
    /// `0900..=0968` `xxx_dat` cluster. See [`crate::overlay_ptr_table`].
    OverlayPtrTable,
    /// "pochi"-fill placeholder: the first 1926 bytes are the ASCII pattern
    /// `pochipochipochi...\r\n` (Japanese dev fill, "ポチ" = generic dog name)
    /// with `0x1A` (DOS EOF) at offset `0x786`. Marks an unused / reserved PROT
    /// slot. Found at consistent offsets within each scene CDNAME block -
    /// scenes reserve N asset slots but only fill some, leaving the rest as
    /// dev-fill. Distinct from data: post-prefix bytes are scratch / leftover.
    PochiFiller,
    /// Multi-bank VAB archive - `[u32 reserved=0][u32 count][u32 sector_nums[count]]`
    /// with VABp magic at `sector_nums[0] * 0x800 + 4`. Covers the level_up
    /// cluster's multi-bank sound archive (206 VABp entries).
    /// See [`crate::vab_multi_bank`].
    VabMultiBank,
    /// Monster / actor SPU sound bank - `[u32 format=2][u16 spu_addrs[256]][ADPCM...]`.
    /// All 256 u16 address-table entries have bit 15 set (>= 0x8000 = active slot).
    /// Sourced from `h:\mpack\monster.snd` loaded by `FUN_8003E104` at battle start.
    MonsterSoundBank,
    /// File with >= 2 sectors (512 bytes) of leading zeros followed by a
    /// high-entropy body (>= 7.0 bits/byte). Characteristic of cutscene / XA
    /// audio files where the leading sector(s) are zeroed out on disc.
    ZeroSectorHighEntropy,
    /// Mid-entropy data blob with >= 18 % printable ASCII content.
    /// Covers overlay string tables, text data dumps, and mixed game data
    /// where the format is not yet identified but readable text is present.
    OverlayDataBlob,
    /// High entropy (>= 7.5 bits/byte). Likely already-compressed or encrypted.
    UnknownHighEntropy,
    /// Low entropy (< 4 bits/byte). Tabular data, sparse vectors, padding.
    UnknownLowEntropy,
    /// Mid-entropy and otherwise unidentified.
    UnknownOther,
}

impl Class {
    pub fn name(&self) -> &'static str {
        match self {
            Class::Empty => "empty",
            Class::Tiny => "tiny",
            Class::AllZeros => "all_zeros",
            Class::MostlyZeros => "mostly_zeros",
            Class::ConstantByte => "constant_byte",
            Class::TimPassthrough => "tim_passthrough",
            Class::DataFieldStreaming => "data_field_streaming",
            Class::DataFieldTruncated => "data_field_truncated",
            Class::TimPack => "tim_pack",
            Class::LzsContainer => "lzs_container",
            Class::StageGeometry => "stage_geometry",
            Class::FieldPack => "field_pack",
            Class::EffectBundle => "effect_bundle",
            Class::SeqContainer => "seq_container",
            Class::AnmContainer => "anm_container",
            Class::SceneTmdStream => "scene_tmd_stream",
            Class::TmdSizePrefix => "tmd_size_prefix",
            Class::SceneVabStream => "scene_vab_stream",
            Class::SceneV12Table => "scene_v12_table",
            Class::SceneAssetTable => "scene_asset_table",
            Class::SceneScriptedAssetTable => "scene_scripted_asset_table",
            Class::SceneEventScripts => "scene_event_scripts",
            Class::MipsOverlay => "mips_overlay",
            Class::OverlayPtrTable => "overlay_ptr_table",
            Class::PochiFiller => "pochi_filler",
            Class::VabMultiBank => "vab_multi_bank",
            Class::MonsterSoundBank => "monster_sound_bank",
            Class::ZeroSectorHighEntropy => "zero_sector_high_entropy",
            Class::OverlayDataBlob => "overlay_data_blob",
            Class::UnknownHighEntropy => "unknown_high_entropy",
            Class::UnknownLowEntropy => "unknown_low_entropy",
            Class::UnknownOther => "unknown_other",
        }
    }
}

/// Per-file feature dump.
#[derive(Debug, Clone, Serialize)]
pub struct FileReport {
    pub class: Class,
    pub size: usize,
    /// First 16 bytes (or fewer), hex.
    pub head: String,
    /// First u32 LE (None if file < 4 bytes).
    pub first_u32: Option<u32>,
    /// Shannon entropy in bits/byte.
    pub entropy_bits: f32,
    /// Length of leading-zero run.
    pub leading_zeros: usize,
    /// Fraction of bytes equal to 0x00 across the whole buffer (0.0..=1.0).
    /// Useful for downstream filtering even when the class doesn't depend on it.
    pub zero_fraction: f32,
    /// For data_field_streaming: chunk count.
    pub stream_chunks: Option<usize>,
    /// For lzs_container: descriptor count that worked.
    pub lzs_descriptor_count: Option<usize>,
    /// For stage_geometry: number of records found in the largest table.
    pub stage_geom_records: Option<usize>,
    /// For tmd_size_prefix: claimed total in-RAM size from the leading u32.
    pub tmd_size_prefix_total: Option<u32>,
}

/// Classify a single buffer.
pub fn classify(buf: &[u8]) -> FileReport {
    let size = buf.len();
    let head = hex_head(buf, 16);
    let first_u32 = if buf.len() >= 4 {
        Some(u32::from_le_bytes(buf[..4].try_into().unwrap()))
    } else {
        None
    };
    let entropy_bits = entropy(buf);
    let leading_zeros = buf.iter().take_while(|&&b| b == 0).count();
    let zero_count = buf.iter().filter(|&&b| b == 0).count();
    let zero_fraction = if buf.is_empty() {
        0.0
    } else {
        zero_count as f32 / buf.len() as f32
    };

    // Run all detectors; pick the first one that fires in this priority order.
    if size == 0 {
        return mk(
            Class::Empty,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }
    if leading_zeros == size {
        return mk(
            Class::AllZeros,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }
    if size < 32 {
        return mk(
            Class::Tiny,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }
    if buf.iter().all(|&b| b == buf[0]) {
        return mk(
            Class::ConstantByte,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }
    if first_u32 == Some(0x0000_0010) {
        return mk(
            Class::TimPassthrough,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }

    // PsyQ SEQ - `pQES` magic + version `0x0001` + non-zero PPQN + non-zero
    // tempo. Specific 4-byte signature with structural follow-up so a
    // chance-match on the magic alone won't fire.
    if buf.len() >= 13
        && &buf[0..4] == b"pQES"
        && u16::from_be_bytes([buf[4], buf[5]]) == 1
        && u16::from_be_bytes([buf[6], buf[7]]) > 0
        && (buf[8] != 0 || buf[9] != 0 || buf[10] != 0)
    {
        return mk(
            Class::SeqContainer,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }

    // ANM container - strict structural detector that requires every
    // record's `marker_1` u16 to equal 0x080C. Runs early so the more
    // permissive structural heuristics (TimPack / lzs_container) don't
    // claim ANM payloads first.
    if crate::anm_detect::detect(buf).is_some() {
        return mk(
            Class::AnmContainer,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }

    // pochi-fill placeholder slot: ASCII "pochi" repeating up to byte 0x785,
    // then `0x1A` (DOS EOF) at offset 0x786. Cheap to check and very specific.
    if is_pochi_filler(buf) {
        return mk(
            Class::PochiFiller,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }

    // Streaming format: don't accept just 1 chunk (too many false positives).
    if let Ok(r) = parse_streaming(buf, 4096)
        && r.terminated
        && r.all_known_types
        && r.all_magic_ok
        && r.chunks.len() >= 2
    {
        let mut report = mk(
            Class::DataFieldStreaming,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
        report.stream_chunks = Some(r.chunks.len());
        return report;
    }

    // Sister of `data_field_streaming` - leading chunks decode cleanly but
    // the final chunk's declared size walks past EOF without a terminator.
    // Strict structural detector: requires >= 3 leading chunks, all known
    // types and magic-OK, plus a partial trailing chunk with a known type.
    if let Some(t) = crate::data_field_truncated::detect(buf) {
        let mut report = mk(
            Class::DataFieldTruncated,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
        report.stream_chunks = Some(t.leading_chunks);
        return report;
    }

    // MIPS overlay-code blob - leads with `addiu sp, sp, -X`. Specific
    // 4-byte signature with no overlap against any other detector. Runs early
    // so we don't waste time on heavier structural checks.
    if crate::mips_overlay::detect(buf).is_some() {
        return mk(
            Class::MipsOverlay,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }

    // Sister of `mips_overlay` - leads with a 4–64 u32 run, each in the
    // overlay window. Strict structural detector with no overlap against
    // any other class.
    if crate::overlay_ptr_table::detect(buf).is_some() {
        return mk(
            Class::OverlayPtrTable,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }

    // Scripted scene-asset-table - `[u16 prescript][bodies][pad][scene_asset_table]`.
    // Runs before plain `scene_asset_table` because the script-prefixed
    // variant is strictly more specific.
    if crate::scene_scripted_asset_table::detect(buf).is_some() {
        return mk(
            Class::SceneScriptedAssetTable,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }

    // 7-asset scene table - leads with `07 00 00 00`, then 7 descriptor pairs.
    // Strict structural detector (no LZS-decode requirement) so it captures
    // both the LZS-payload and raw-payload variants uniformly. Runs before
    // lzs_container so the 26 entries that previously matched as `n=1`
    // (a coincidental first-descriptor match) get the more specific class.
    if crate::scene_asset_table::detect(buf).is_some() {
        return mk(
            Class::SceneAssetTable,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }

    // Scene event-scripts: same `[u16 count][u16 offsets]` prescript shape as
    // `scene_scripted_asset_table` but with no canonical asset table after.
    // Runs after both scripted-and-asset-table and plain asset-table so the
    // more specific layouts claim their entries first. Frame-opener-rate gate
    // (>= 50% of records start with the field-VM `0xFFFF 0x0000` sentinel)
    // keeps this zero-false-positive against random `[count][offsets]`-shaped
    // data.
    if crate::scene_event_scripts::detect(buf).is_some() {
        return mk(
            Class::SceneEventScripts,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }

    // v12 strict-magic header - `[N+4, 0x12, 0, 0x14, ?, N, 0, N+2]`. This
    // outer-shape header at offset 0 is more authoritative than fieldpack
    // magic that may appear deeper in the file (e.g. `0002_gameover_data.BIN`
    // has v12 at offset 0 and a fieldpack-shaped region at 0x39800).
    if crate::scene_v12_table::detect(buf).is_some() {
        return mk(
            Class::SceneV12Table,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }

    // Field-pack: magic + 97-entry schema. Detect before TimPack /
    // stage_geometry / lzs_container - fieldpack files often satisfy
    // weaker heuristics, so the most-specific signature wins.
    if crate::field_pack::detect(buf).is_some() {
        return mk(
            Class::FieldPack,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }

    // Effect-bundle: same logic as field-pack - strict-schema detector
    // gates this before the weaker heuristics.
    if crate::effect_bundle::detect(buf).is_some() {
        return mk(
            Class::EffectBundle,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }

    // TMD-prefixed scene stream - `[u32 size][bare TMD][streaming chunks]`.
    // Strict structural detector (TMD magic at +4, sane nobj, in-bounds size
    // prefix, walkable streaming tail) - runs before the weaker LZS-container
    // and stage-geometry heuristics so the most-specific schema wins.
    if let Some(s) = crate::scene_tmd_stream::detect(buf) {
        let mut report = mk(
            Class::SceneTmdStream,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
        report.stream_chunks = Some(s.tail_chunks.len());
        return report;
    }

    // TMD-with-size-prefix - sister of `scene_tmd_stream` for the truncated
    // case (claimed_total > on-disc len). Runs immediately after
    // `scene_tmd_stream` so any complete-on-disc TMD-stream files are claimed
    // by the more permissive sister detector first.
    if let Some(t) = crate::tmd_size_prefix::detect(buf) {
        let mut report = mk(
            Class::TmdSizePrefix,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
        report.tmd_size_prefix_total = Some(t.claimed_total);
        return report;
    }

    // VAB-prefixed scene stream - same outer wrapper as scene_tmd_stream, but
    // chunk0 carries a Sony VAB sound bank (`VABp` magic at +4). Strict
    // structural detector validates the magic + version + ps/ts counts.
    if let Some(s) = crate::scene_vab_stream::detect(buf) {
        let mut report = mk(
            Class::SceneVabStream,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
        report.stream_chunks = Some(s.tail_chunks);
        return report;
    }

    // Multi-bank VAB archive - `[u32 reserved=0][u32 count][u32 sector_nums[count]]`
    // with VABp magic at `sector_nums[0] * 0x800 + 4`. Covers the level_up
    // cluster (206 VABp entries). Runs after scene_vab_stream so the more
    // common streaming wrapper claims its entries first.
    if crate::vab_multi_bank::detect(buf).is_some() {
        return mk(
            Class::VabMultiBank,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }

    // Monster / actor SPU sound bank: `[u32 format=2][u16 spu_addrs[256]][ADPCM...]`.
    // All 256 u16 address-table entries have bit 15 set (>= 0x8000 = active slot).
    if first_u32 == Some(2) && buf.len() >= 4 + 256 * 2 {
        let all_high = (0..256usize).all(|i| {
            let p = 4 + i * 2;
            let v = u16::from_le_bytes([buf[p], buf[p + 1]]);
            v >= 0x8000
        });
        if all_high {
            return mk(
                Class::MonsterSoundBank,
                size,
                head,
                first_u32,
                entropy_bits,
                leading_zeros,
                zero_fraction,
            );
        }
    }

    // Large zero-padded header (>= 2 sectors of zeros) with high-entropy body.
    // Typical of cutscene/XA audio files where the leading sector(s) are zeroed.
    if leading_zeros >= 512 && entropy_bits >= 7.0 {
        return mk(
            Class::ZeroSectorHighEntropy,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }

    // Standalone TIM-pack heuristic (`byte[3]==0x01 && byte[2]<0x10`).
    if legaia_prot::timpack::is_tim_pack(buf) {
        return mk(
            Class::TimPack,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }

    // Stage-geometry table (Cluster A successor). One run of >= 4
    // consecutive records is enough - the 12-byte signature is too
    // specific to coincide.
    let geom_tables = crate::stage_geom::scan(buf);
    if let Some(largest) = geom_tables.iter().max_by_key(|t| t.records) {
        let mut report = mk(
            Class::StageGeometry,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
        report.stage_geom_records = Some(largest.records);
        return report;
    }

    // player.lzs-style container: try a handful of descriptor counts. We accept
    // it only if EVERY descriptor decodes via LZS (some by raw is fine too) AND
    // at least one decodes (no zero-descriptor fits).
    for &n in &[1usize, 2, 3, 4, 8, 16] {
        if let Some(_count) = try_lzs_container(buf, n) {
            let mut report = mk(
                Class::LzsContainer,
                size,
                head,
                first_u32,
                entropy_bits,
                leading_zeros,
                zero_fraction,
            );
            report.lzs_descriptor_count = Some(n);
            return report;
        }
    }

    // Mostly-zeros placeholder. Run after structural detectors so a sparse
    // stage-geometry / streaming entry isn't shadowed. The 0.75 threshold
    // catches near-empty PROT slots without sweeping in real (sparse) tables.
    if zero_fraction >= 0.75 {
        return mk(
            Class::MostlyZeros,
            size,
            head,
            first_u32,
            entropy_bits,
            leading_zeros,
            zero_fraction,
        );
    }

    // Mid-entropy data blob with significant printable ASCII content.
    // Covers overlay string tables, text data dumps, and mixed game data where
    // the format is not yet identified but readable text content is present.
    if entropy_bits < 7.0 {
        let visible: f32 = buf
            .iter()
            .filter(|&&b| (0x20..=0x7E).contains(&b) || matches!(b, 9 | 10 | 13))
            .count() as f32
            / size as f32;
        if visible >= 0.18 {
            return mk(
                Class::OverlayDataBlob,
                size,
                head,
                first_u32,
                entropy_bits,
                leading_zeros,
                zero_fraction,
            );
        }
    }

    // Statistical fallback.
    let class = if entropy_bits >= 7.5 {
        Class::UnknownHighEntropy
    } else if entropy_bits < 4.0 {
        Class::UnknownLowEntropy
    } else {
        Class::UnknownOther
    };
    mk(
        class,
        size,
        head,
        first_u32,
        entropy_bits,
        leading_zeros,
        zero_fraction,
    )
}

/// Detects the "pochi-fill" placeholder pattern used in unused PROT slots.
///
/// Layout:
/// - Bytes 0..0x786: ASCII `"pochi"` repeating (lines of 50 chars + CRLF
///   terminator), where 0x786 = 1926 = 37 lines × 52 bytes + 2 bytes ("po").
/// - Byte 0x786: `0x1A` (DOS EOF marker).
/// - Bytes 0x787..end: scratch / leftover data (sometimes non-zero).
///
/// We don't validate the full prefix byte-by-byte - checking the first 5
/// bytes for `"pochi"` plus the magic at 0x786 is enough to be specific
/// (no real format starts with 5 ASCII letters and then has 0x1A at exactly
/// that offset).
fn is_pochi_filler(buf: &[u8]) -> bool {
    buf.len() > 0x786 && buf.starts_with(b"pochi") && buf[0x786] == 0x1A
}

fn try_lzs_container(buf: &[u8], count: usize) -> Option<usize> {
    let header_end = (8 + count * 8) as u32;
    let c = parse_player_lzs(buf, count).ok()?;
    // All descriptors need: known type, sane size, in-bounds offset.
    if !c.descriptors.iter().all(|d| {
        !matches!(d.asset_type(), AssetType::Unknown(_))
            && (32..=4 * 1024 * 1024).contains(&d.size)
            && d.data_offset >= header_end
            && (d.data_offset as usize) < buf.len()
    }) {
        return None;
    }
    // At least one must decode via LZS to count as evidence.
    let any_lzs = c
        .descriptors
        .iter()
        .any(|d| crate::decode(buf, d, crate::DecodeMode::Lzs).is_ok());
    if any_lzs { Some(count) } else { None }
}

fn mk(
    class: Class,
    size: usize,
    head: String,
    first_u32: Option<u32>,
    entropy_bits: f32,
    leading_zeros: usize,
    zero_fraction: f32,
) -> FileReport {
    FileReport {
        class,
        size,
        head,
        first_u32,
        entropy_bits,
        leading_zeros,
        zero_fraction,
        stream_chunks: None,
        lzs_descriptor_count: None,
        stage_geom_records: None,
        tmd_size_prefix_total: None,
    }
}

fn hex_head(buf: &[u8], n: usize) -> String {
    buf.iter()
        .take(n)
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ")
}

fn entropy(buf: &[u8]) -> f32 {
    if buf.is_empty() {
        return 0.0;
    }
    let mut counts = [0u32; 256];
    for &b in buf {
        counts[b as usize] += 1;
    }
    let n = buf.len() as f32;
    let mut h = 0.0f32;
    for c in counts.iter().filter(|&&c| c > 0) {
        let p = (*c as f32) / n;
        h -= p * p.log2();
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_all_zeros() {
        let r = classify(&vec![0u8; 1024]);
        assert_eq!(r.class, Class::AllZeros);
        assert_eq!(r.zero_fraction, 1.0);
    }

    #[test]
    fn detects_mostly_zeros_above_threshold() {
        // 1024 zeros + 10 non-zero bytes = ~99% zeros.
        let mut buf = vec![0u8; 1024];
        for i in 0..10 {
            buf.push(0xAA + i as u8);
        }
        let r = classify(&buf);
        assert_eq!(r.class, Class::MostlyZeros);
        assert!(r.zero_fraction >= 0.75);
    }

    #[test]
    fn does_not_classify_50pct_zeros_as_mostly_zeros() {
        // Sparse-but-real tables (< 75% zeros) must stay below the
        // mostly_zeros threshold so they get reverse-engineered.
        let mut buf = vec![0u8; 1024];
        // 512 non-zero bytes scattered = 50% zeros - below 75% threshold.
        for i in 0..512 {
            buf[i * 2] = 0xAA;
        }
        let r = classify(&buf);
        assert_ne!(r.class, Class::MostlyZeros);
    }

    #[test]
    fn detects_tim_passthrough() {
        let mut buf = vec![0x10u8, 0, 0, 0, 0x08, 0, 0, 0];
        buf.resize(64, 0xAA);
        let r = classify(&buf);
        assert_eq!(r.class, Class::TimPassthrough);
    }

    #[test]
    fn detects_seq_container() {
        // pQES + version 1 BE + ppqn 480 BE + tempo 500_000 us + 4/4
        let mut buf = b"pQES".to_vec();
        buf.extend_from_slice(&[0x00, 0x01]); // version
        buf.extend_from_slice(&[0x01, 0xE0]); // ppqn
        buf.extend_from_slice(&[0x07, 0xA1, 0x20]); // tempo
        buf.push(0x04);
        buf.push(0x02);
        buf.resize(128, 0); // event stream filler
        let r = classify(&buf);
        assert_eq!(r.class, Class::SeqContainer);
    }

    #[test]
    fn rejects_seq_with_wrong_version() {
        let mut buf = b"pQES".to_vec();
        buf.extend_from_slice(&[0x00, 0x02]); // version 2 - not the SsAPI shape
        buf.resize(128, 0);
        let r = classify(&buf);
        assert_ne!(r.class, Class::SeqContainer);
    }

    #[test]
    fn detects_constant_byte() {
        let r = classify(&vec![0xAAu8; 1024]);
        assert_eq!(r.class, Class::ConstantByte);
    }

    #[test]
    fn detects_empty_and_tiny() {
        assert_eq!(classify(&[]).class, Class::Empty);
        assert_eq!(classify(&[1, 2, 3]).class, Class::Tiny);
    }

    #[test]
    fn high_entropy_random_bytes() {
        let buf: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
        let r = classify(&buf);
        // 256 symbols uniform → 8 bits of entropy
        assert!(r.entropy_bits > 7.9);
        assert_eq!(r.class, Class::UnknownHighEntropy);
    }

    #[test]
    fn low_entropy_repetitive() {
        // 4096-byte buffer at ~50% zeros (below the 75% MostlyZeros gate)
        // with a 4-symbol alphabet - low entropy but real content.
        let mut buf = vec![0u8; 4096];
        // Alternate 0x00 / (1,2,3 cycling) every other byte → 50% zeros.
        for i in 0..2048 {
            buf[i * 2] = (i % 3 + 1) as u8;
        }
        let r = classify(&buf);
        assert!(r.entropy_bits < 4.0, "entropy was {}", r.entropy_bits);
        assert!(
            r.zero_fraction < 0.75,
            "zero_fraction was {}",
            r.zero_fraction
        );
        assert_eq!(r.class, Class::UnknownLowEntropy);
    }
}
