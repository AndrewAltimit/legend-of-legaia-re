//! End-to-end validation suite. Runs the per-crate library APIs in the same
//! sequence the `legaia-extract` binary does, then asserts pinned invariants
//! about counts, sizes, and sample hashes. Catches regressions in any layer
//! that change the extraction outcome on the NA disc.
//!
//! Set `LEGAIA_DISC_BIN` to the absolute path of a Mode2/2352 .bin to enable
//! these tests. Without it, tests print a one-line skip notice and pass -
//! mirroring the convention in `crates/iso/tests/disc_pipeline.rs`.

use std::path::PathBuf;

use legaia_asset::{AssetType, categorize, parse_streaming};
use legaia_iso::iso9660;
use legaia_iso::raw::{RawDisc, USER_DATA_SIZE};
use legaia_prot::archive::Archive;

// ============================================================================
// Pinned baselines (NA SCUS-94254, project author's dump)
// ============================================================================

/// Total entries we expect from PROT.DAT.
const EXPECTED_PROT_ENTRIES: usize = 1232;

/// Class breakdown from `categorize::classify` over every PROT entry's
/// **full on-disc footprint** (indexed payload + any trailing-overlay
/// sectors the boot loader reads past the TOC-indexed end — see
/// `docs/subsystems/boot.md`). Order doesn't matter; the test asserts each
/// `(class_name, count)` pair.
///
/// Re-pinned after the prot crate's `size_sectors = max(indexed, footprint)`
/// fix that surfaces trailing-overlay content (e.g. PROT 899's trailing
/// 60 sectors are the title-screen overlay code). Many entries shifted
/// class as their trailing sectors changed the byte histogram or extended
/// the data-shape past the previous detector boundaries.
const EXPECTED_CLASS_COUNTS: &[(&str, usize)] = &[
    // `battle_data_pack` = the player battle files (retail `battle_data`
    // block, extraction 0863..0866 = PLAYER1..4). The realigned
    // `[id, offset, size]` table frame accepts all four, including
    // Terra's 0866 all-default (`id = 0`) table.
    ("battle_data_pack", 4),
    ("data_field_streaming", 34),
    // `field_pack` 2 → 1: one of the two entries (PROT 4) leads with a
    // count=6 scene-asset table at offset 0 and only carries a field-pack
    // *region* deeper in the file. The offset-0 scene-table shape is the
    // authoritative outer classification (same precedence as v12-over-
    // fieldpack), so it now lands in `scene_asset_table`. PROT 5 remains
    // the sole pure field_pack.
    ("field_pack", 1),
    // `lzs_container` 42 → 35: the count=6 scene-asset-table variant
    // (town01/town0c-class MAN bundles) now claims 7 entries that were
    // coincidental strict-LZS matches before the more specific schema ran.
    // 35 → 34: Terra's player file (extraction 0866) moved to
    // `battle_data_pack` once the realigned table frame accepted its
    // all-default descriptor table.
    ("lzs_container", 34),
    ("mips_overlay", 22),
    ("monster_sound_bank", 1),
    // `mostly_zeros` dropped (101 → 70) because many zero-padded entries
    // gained non-zero trailing-overlay content that shifts them out of the
    // dominant-zero bucket.
    ("mostly_zeros", 70),
    // `overlay_data_blob` (27 → 27 — unchanged; trailing-overlay bytes are
    // mostly code or already-classified data, not the mixed-text shape).
    ("overlay_data_blob", 27),
    ("overlay_ptr_table", 42),
    ("pochi_filler", 265),
    // `scene_asset_table` 80 → 88: the detector now also accepts the
    // count=6 header variant used by the early standalone towns (first
    // descriptor anchored at 0x38, MAN at descriptor index 1/2). Eight
    // entries (PROT 4, 13, 22, 183, 348, 742, 1196, 1229) shifted in — one
    // from `field_pack`, seven from `lzs_container`.
    ("scene_asset_table", 88),
    // `scene_tmd_stream` jumped (148 → 182) as 34 entries' trailing-overlay
    // bytes happened to fit the streaming-with-bare-TMD shape.
    ("scene_tmd_stream", 182),
    ("scene_vab_stream", 217),
    ("scene_v12_table", 97),
    ("scene_scripted_asset_table", 79),
    ("scene_event_scripts", 21),
    ("tim_pack", 7),
    // `vab_multi_bank` matches the level_up multi-bank archive. One PROT entry.
    ("vab_multi_bank", 1),
    // `zero_sector_high_entropy` covers files with leading zeros + high-
    // entropy body. Four PROT entries.
    ("zero_sector_high_entropy", 4),
    // Residual buckets. Trailing-overlay MIPS code doesn't fit any PROT-
    // format detector, so some entries land here (~8.4 MiB total
    // unclassified by extended coverage, vs 1.9 MiB by indexed coverage —
    // see categorize_coverage.rs for the split).
    ("unknown_high_entropy", 1),
    ("unknown_low_entropy", 29),
    ("unknown_other", 6),
];

/// Number of PROT entries that pass the strict streaming-format filter
/// (terminator + ≥2 chunks + all known types + magic OK). Bumped from 26
/// → 34 after the size-math fix surfaced 8 entries whose trailing-overlay
/// bytes complete a streaming-format suffix.
const EXPECTED_STREAM_HITS: usize = 34;

/// Total sub-assets across all streaming hits. Jumped from 49 → 583 after
/// the size-math fix: the 8 new streaming hits include TimList/Tmd PACK
/// chunks (not just single-asset chunks), and the pack walkers expand each
/// into multiple sub-assets.
const EXPECTED_TOTAL_SUBASSETS: usize = 583;

/// One pinned PROT entry's size, used as a quick sanity check that the TOC
/// math hasn't drifted.
const PINNED_ENTRY: (u32, u64) = (148, 172_032); // entry 148 = retock

/// Number of PROT entries that strict-validate as real LZS containers
/// (the strict check requires no section-input-overrun and a minimum decoded
/// total of [`MIN_REAL_DECODE_BYTES`]). Jumped from 33 → 113 after the
/// size-math fix: many entries' trailing-overlay tails extend the LZS-
/// descriptor walk past its previous truncation point, satisfying the
/// strict decode check.
const EXPECTED_LZS_CONTAINERS_STRICT: usize = 113;

/// Constant matching `lzs-decode`'s MIN_REAL_DECODE_BYTES - kept in sync
/// to prove the validation suite checks the same thing the audit tool does.
const MIN_REAL_DECODE_BYTES: usize = 256;

// ============================================================================

fn disc_bin_path() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)
}

fn skip_or<T>(val: Option<T>, msg: &str) -> Option<T> {
    if val.is_none() {
        eprintln!("[skip] {}", msg);
    }
    val
}

#[test]
fn validation_suite_full_pipeline() {
    let Some(bin) = skip_or(disc_bin_path(), "LEGAIA_DISC_BIN unset; skipping") else {
        return;
    };
    if !bin.exists() {
        panic!("LEGAIA_DISC_BIN={} does not exist", bin.display());
    }

    // ---- 1. Disc walk: file count + presence of PROT.DAT
    let mut disc = RawDisc::open(&bin).expect("open disc");
    let vol = iso9660::read_volume(&mut disc).expect("read volume");
    let files = iso9660::walk_files(&mut disc, &vol.root).expect("walk");
    assert!(
        files
            .iter()
            .any(|(p, _)| p.eq_ignore_ascii_case("PROT.DAT")),
        "PROT.DAT missing from disc walk"
    );

    // ---- 2. Extract PROT.DAT to a temp file so we can open it via Archive
    let tmp = std::env::temp_dir().join(format!("legaia-validation-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).expect("create tmp");
    let prot_path = tmp.join("PROT.DAT");
    let (_, prot_entry) = files
        .iter()
        .find(|(p, _)| p.eq_ignore_ascii_case("PROT.DAT"))
        .expect("PROT.DAT entry");
    let mut buf = Vec::new();
    let n = prot_entry.size.div_ceil(USER_DATA_SIZE as u32);
    disc.read_user_data(prot_entry.lba, n, &mut buf)
        .expect("read PROT.DAT");
    buf.truncate(prot_entry.size as usize);
    std::fs::write(&prot_path, &buf).expect("write PROT.DAT");

    // ---- 3. Open archive: assert entry count + pinned entry size
    let mut archive = Archive::open(&prot_path).expect("open PROT.DAT");
    assert_eq!(
        archive.entries.len(),
        EXPECTED_PROT_ENTRIES,
        "PROT entry count drift"
    );
    let pinned = archive
        .entries
        .iter()
        .find(|e| e.index == PINNED_ENTRY.0)
        .expect("pinned entry missing")
        .clone();
    assert_eq!(
        pinned.size_bytes, PINNED_ENTRY.1,
        "pinned entry {} size drift: expected {}, got {}",
        PINNED_ENTRY.0, PINNED_ENTRY.1, pinned.size_bytes
    );

    // ---- 4. Categorize: count each class
    let mut class_counts: std::collections::HashMap<&'static str, usize> =
        std::collections::HashMap::new();
    let mut entry_buf = Vec::new();
    let mut stream_hits = 0usize;
    let mut total_subassets = 0usize;
    let entries = archive.entries.clone();
    for entry in &entries {
        archive
            .read_entry(entry, &mut entry_buf)
            .expect("read entry");
        let report = categorize::classify(&entry_buf);
        *class_counts.entry(report.class.name()).or_insert(0) += 1;

        // Streaming-format check (mirrors classifier's stricter detector
        // path; counted independently for cross-validation).
        if let Ok(s) = parse_streaming(&entry_buf, 4096)
            && s.terminated
            && s.all_known_types
            && s.all_magic_ok
            && s.chunks.len() >= 2
        {
            stream_hits += 1;
            // Count sub-assets across both single-asset chunks (TIM 0x00,
            // TMD2 0x09, MOVE2 0x0B - each = 1 sub-asset) and pack chunks
            // (TimList 0x01, Tmd 0x02 - expanded via pack walker).
            for chunk in &s.chunks {
                let t = AssetType::from_byte(chunk.type_byte);
                match t {
                    AssetType::Tim | AssetType::Tmd2 | AssetType::Move2 => {
                        total_subassets += 1;
                    }
                    AssetType::TimList | AssetType::Tmd => {
                        let data_start = chunk.header_offset + 4;
                        let data_end = data_start + chunk.size as usize;
                        if data_end > entry_buf.len() {
                            continue;
                        }
                        let chunk_data = &entry_buf[data_start..data_end];
                        if let Ok(items) = legaia_asset::pack::extract_pack(chunk_data) {
                            total_subassets += items.len();
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Assert pinned class counts.
    let total_seen: usize = class_counts.values().sum();
    assert_eq!(
        total_seen, EXPECTED_PROT_ENTRIES,
        "categorize total mismatch"
    );
    for (name, want) in EXPECTED_CLASS_COUNTS {
        let got = class_counts.get(name).copied().unwrap_or(0);
        assert_eq!(
            got, *want,
            "class {} count drift: expected {}, got {}; full breakdown: {:?}",
            name, want, got, class_counts
        );
    }

    // Assert streaming hits.
    assert_eq!(
        stream_hits, EXPECTED_STREAM_HITS,
        "streaming-format hit count drift"
    );

    // Assert sub-asset total.
    assert_eq!(
        total_subassets, EXPECTED_TOTAL_SUBASSETS,
        "total sub-asset count from streaming hits drifted"
    );

    // ---- 4b. LZS container scan (verification at scale)
    let mut lzs_strict_hits = 0usize;
    for entry in &entries {
        archive
            .read_entry(entry, &mut entry_buf)
            .expect("read entry");
        let Ok(decoded) = legaia_lzs::decompress_container_strict(&entry_buf) else {
            continue;
        };
        let total: usize = decoded.iter().map(|d| d.len()).sum();
        if total >= MIN_REAL_DECODE_BYTES {
            lzs_strict_hits += 1;
        }
    }
    assert_eq!(
        lzs_strict_hits, EXPECTED_LZS_CONTAINERS_STRICT,
        "strict LZS container count drifted: expected {}, got {}",
        EXPECTED_LZS_CONTAINERS_STRICT, lzs_strict_hits
    );

    // ---- 5. Smoke-test the scene_tmd_stream detector on entry 148 (retock).
    //
    // 0148_retock is a `scene_tmd_stream` entry: `[u32 size][bare TMD][stream]`.
    // Validate (a) the detector fires, (b) the leading TMD parses end-to-end via
    // the regular `legaia_tmd::parse` API, and (c) the streaming tail produces at
    // least one valid chunk header. Replaces an earlier "expand TIM pack" smoke
    // test that mismatched the entry's actual on-disc shape - pre-fix,
    // categorize.json-driven assumptions for entry 148 ascribed it to standard
    // DATA_FIELD streaming with a TIM_LIST pack chunk, but post-fix scan-stream
    // shows entry 148's TIM_LIST chunk holds a *single* TIM (not a pack).
    archive
        .read_entry(&pinned, &mut entry_buf)
        .expect("re-read");
    let scene = legaia_asset::scene_tmd_stream::detect(&entry_buf)
        .expect("retock should detect as scene_tmd_stream");
    assert!(
        scene.tmd_nobj >= 1 && scene.tmd_nobj <= 16,
        "leading TMD nobj out of expected range: {}",
        scene.tmd_nobj
    );
    let tmd_bytes = &entry_buf[scene.tmd_range()];
    let tmd = legaia_tmd::parse(tmd_bytes).expect("parse leading bare TMD via legaia_tmd::parse");
    assert_eq!(
        tmd.objects.len() as u32,
        scene.tmd_nobj,
        "TMD object count mismatch between detector and parser"
    );
    assert!(
        !scene.tail_chunks.is_empty(),
        "scene_tmd_stream tail should have at least one chunk"
    );
    let first_tail = &scene.tail_chunks[0];
    assert!(
        matches!(
            AssetType::from_byte(0).name().chars().next().unwrap_or('?'),
            'T' | 'M' | 'A' | 'V' | 'S' | 'F' | 'U'
        ),
        "AssetType name lookup smoke check"
    );
    assert!(
        !matches!(first_tail.asset_type, AssetType::Unknown(_)),
        "first tail chunk should have a known asset type, got {:?}",
        first_tail.asset_type
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&tmp);
}
