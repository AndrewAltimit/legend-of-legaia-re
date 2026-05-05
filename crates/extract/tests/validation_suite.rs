//! End-to-end validation suite. Runs the per-crate library APIs in the same
//! sequence the `legaia-extract` binary does, then asserts pinned invariants
//! about counts, sizes, and sample hashes. Catches regressions in any layer
//! that change the extraction outcome on the NA disc.
//!
//! Set `LEGAIA_DISC_BIN` to the absolute path of a Mode2/2352 .bin to enable
//! these tests. Without it, tests print a one-line skip notice and pass —
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

/// Class breakdown from `categorize::classify` over every PROT entry.
/// Order doesn't matter — the test asserts each `(class_name, count)` pair.
///
/// Re-pinned 2026-05 after the prot crate's TOC math fix. Earlier values
/// (effect_bundle=503, field_pack=124, stage_geometry=561, etc.) were all
/// artifacts of misextraction; ~80% of the previous classifications saw
/// duplicate bytes due to `start_lba = toc[p+5] - toc[p+2]` reading the
/// wrong offset. Corrected math (`start_lba = toc[p+2]`) yields the values
/// below, which pass the runtime cross-check (entries 873/871/872/877/888/891
/// byte-match the live battle save's loaded buffers).
const EXPECTED_CLASS_COUNTS: &[(&str, usize)] = &[
    ("all_zeros", 1),
    ("data_field_streaming", 26),
    // Added 2026-05-05: sister of `data_field_streaming` — leading chunks parse
    // cleanly (all known types, all magic-OK) but the final chunk's declared
    // `size` walks past EOF without a terminator. The runtime extends the chunk
    // via streaming DMA continuation rather than a literal terminator on disc.
    // Promoted 3 entries from `unknown_other` (`0157_rikuroa`, `0228_station`,
    // `0373_taiku` — scene streams with 2-3 leading chunks then a partial
    // MOVE/VDF chunk) and 1 from `unknown_low_entropy` (`1205_other5` — a 6-
    // leading-chunk stream with a partial TIM tail).
    ("data_field_truncated", 4),
    ("effect_bundle", 1),
    // 2026-05-04: dropped 4 → 3 after the scene_v12_table detector promoted
    // `0002_gameover_data.BIN` (v12 header at offset 0; field_pack magic at
    // 0x39800 was a coincidental embedded region).
    // 2026-05-05: dropped 3 → 2 after the scene_event_scripts detector took
    // `0003_town01.BIN` (prescript shape at offset 0; the previous field_pack
    // hit was on a deeper magic occurrence inside the prescript records).
    ("field_pack", 2),
    // 2026-05-04: dropped 70 → 44 after the scene_asset_table detector promoted
    // 26 entries that previously matched `n=1` only (a coincidental first-
    // descriptor match). Those 26 are now classed `scene_asset_table` along
    // with 54 sibling entries that didn't pass the LZS-decode gate.
    // 2026-05-05: dropped 44 → 42 after `scene_event_scripts` promoted 2
    // entries whose prescript shape + 50%-FFFF-opener rate is a much more
    // specific signal than "happens to LZS-decode for some descriptors".
    ("lzs_container", 42),
    // Added 2026-05-04: MIPS overlay-code detector recognises `addiu sp, sp, -X`
    // followed by a plausible MIPS prologue continuation (`sw`, `addiu`, `lui`,
    // `lw`, R-type). All 22 matches are in the `0901..=0969_xxx_dat` cluster —
    // small overlay code blobs (14-37 KB plus one 163 KB outlier) that load
    // into the runtime overlay window. Promoted 21 from `unknown_other` and
    // 1 from `unknown_low_entropy`.
    ("mips_overlay", 22),
    ("mostly_zeros", 29),
    // Added 2026-05-04: sister of `mips_overlay` — same kind of overlay code
    // blob, but the first chunk is a 4–64 entry pointer table instead of an
    // immediate `addiu sp, sp, -X` prologue. Each pointer u32 lies in the
    // `0x801C0000..=0x80200000` overlay window. 42 matches across the
    // `0900..=0968_xxx_dat` cluster (some monotonic function-entry tables,
    // some switch dispatch tables with repeating handlers, some preceded by
    // a string title for dance/music subsystems). Promoted 41 from
    // `unknown_other` (138 → 97) and 1 from `unknown_low_entropy` (75 → 74).
    ("overlay_ptr_table", 42),
    ("pochi_filler", 265),
    // Added 2026-05-04: strict 7-asset descriptor table — leads with
    // `07 00 00 00`, then 7 descriptor pairs covering the canonical
    // `(TimList, Tmd, Man, Mes, Move, Anm, Vdf)` asset sequence. The
    // descriptor offsets past the first are runtime-buffer offsets, not
    // file-relative byte offsets — the detector accepts up to 16 MB. Moves
    // 26 entries from `lzs_container`, 43 from `unknown_high_entropy`, and
    // 11 from `unknown_other`.
    ("scene_asset_table", 80),
    ("scene_tmd_stream", 148),
    // Added 2026-05-04: VAB-prefixed scene-stream detector recognizes the
    // `[chunk0 type=0x00, size=N][VABp magic at +4]` pattern shared by 217
    // PROT entries (the `vab_01` cluster + scattered scene blocks). Moves
    // 216 entries out of `unknown_other` (385 → 169) and 1 from
    // `unknown_low_entropy` (77 → 76); `unknown_high_entropy` is unchanged.
    ("scene_vab_stream", 217),
    // Added 2026-05-04: strict 8-word v12 header
    // `[N+4, 0x12, 0, 0x14, ?, N, 0, N+2]` matches 97 scene-named PROT entries
    // (one per scene). Format meaning unconfirmed (likely per-scene navmesh /
    // collision / event-trigger). Moves 95 from `unknown_high_entropy`
    // (219 → 124), 1 from `unknown_other` (169 → 168), 1 from `field_pack`
    // (4 → 3 — `0002_gameover_data.BIN` had v12 at offset 0).
    ("scene_v12_table", 97),
    // Added 2026-05-05: composite shape — `[u16 prescript][bodies][pad][canonical
    // 7-asset scene_asset_table]`. The leading prescript carries scene-event
    // bytecode (likely field-VM frames); the asset table at the next 0x800
    // sector boundary holds the standard scene bundle. Promoted ~64 entries
    // from `unknown_high_entropy` (81 → 17) and ~13 from `unknown_other`
    // (95 → 82 in disc-mode).
    ("scene_scripted_asset_table", 79),
    // Added 2026-05-05: sister of `scene_scripted_asset_table` — same
    // `[u16 count][u16 offsets[count]]` prescript shape, but no canonical
    // 7-asset table at the next sector boundary. Frame-opener gate
    // (>= 50% of records lead with the field-VM `0xFFFF 0x0000` sentinel)
    // keeps it zero-false-positive. 20 entries: 5 from `unknown_high_entropy`,
    // 14 from `unknown_other`, 1 reclaimed from a coincidental `field_pack`
    // false positive (`0003_town01.BIN`).
    ("scene_event_scripts", 20),
    ("tim_pack", 7),
    // Added 2026-05-05: TMD-size-prefix detector — sister of `scene_tmd_stream`
    // for the *truncated* case (`prefix_size > on-disc len`). On-disc file is
    // a prefix of a logical TMD whose remainder is supplied at runtime. Promoted
    // 34 entries from `unknown_other` (95 → 61 in disc-mode).
    ("tmd_size_prefix", 34),
    // 2026-05-05: dropped 81 → 17 after `scene_scripted_asset_table` promoted
    // ~64 entries; further dropped 17 → 12 after `scene_event_scripts` took
    // 5 (0318/0337/0399/0587/0646).
    ("unknown_high_entropy", 12),
    // 2026-05-05: dropped 74 → 73 after `data_field_truncated` claimed
    // `1205_other5` (a 6-chunk streaming buffer with a partial TIM tail
    // that previously hid in the low-entropy bucket because the chunk-0
    // header word `0x00008220` masked as zero-leading).
    ("unknown_low_entropy", 73),
    // 2026-05-05: dropped 95 → 50 in working dir after `tmd_size_prefix` (34)
    // and `scene_scripted_asset_table` (~13). Further dropped 50 → 34 after
    // `scene_event_scripts` took ~14 entries (large town/scene bundles whose
    // prescript holds field-VM event scripts but whose post-prescript payload
    // isn't a canonical asset table). Then dropped 34 → 31 after
    // `data_field_truncated` claimed `0157_rikuroa`, `0228_station`,
    // `0373_taiku` (scene streams with a partial MOVE/VDF tail). The disc-
    // mode count is 31 (vs. 33 in working dir) because `categorize.json`
    // and `manifest.json` are working-dir artifacts, not real PROT entries.
    ("unknown_other", 31),
];

/// Number of PROT entries that pass the strict streaming-format filter
/// (terminator + ≥2 chunks + all known types + magic OK).
const EXPECTED_STREAM_HITS: usize = 26;

/// Total sub-assets across all streaming hits, counting both singles
/// (TIM 0x00 / TMD2 0x09 / MOVE2 0x0B → 1 each) and packs (TimList 0x01
/// / Tmd 0x02 → expanded via pack walker). Post-fix the 26 hits are all
/// in the `_other5` cluster and consist of single-asset chunks: 16 TIM +
/// 19 TMD2 + 14 MOVE2 = 49.
const EXPECTED_TOTAL_SUBASSETS: usize = 49;

/// One pinned PROT entry's size, used as a quick sanity check that the TOC
/// math hasn't drifted.
const PINNED_ENTRY: (u32, u64) = (148, 172_032); // entry 148 = retock

/// Number of PROT entries that strict-validate as real LZS containers
/// (the strict check requires no section-input-overrun and a minimum decoded
/// total of [`MIN_REAL_DECODE_BYTES`]).
const EXPECTED_LZS_CONTAINERS_STRICT: usize = 33;

/// Constant matching `lzs-decode`'s MIN_REAL_DECODE_BYTES — kept in sync
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
            // TMD2 0x09, MOVE2 0x0B — each = 1 sub-asset) and pack chunks
            // (TimList 0x01, Tmd 0x02 — expanded via pack walker).
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

    // ---- 4b. LZS container scan (Epic 2.1: verification at scale)
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
    // test that mismatched the entry's actual on-disc shape — pre-fix,
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
