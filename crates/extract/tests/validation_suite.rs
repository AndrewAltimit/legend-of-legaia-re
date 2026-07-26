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

/// Total entries we expect from PROT.DAT: indices 0..=1232 (1231 = the
/// dance-minigame SFX VAB, 1232 = the last data sector; the zeroed TOC
/// tail rows past 1232 are padding, dropped by the archive's zero-row
/// guard).
const EXPECTED_PROT_ENTRIES: usize = 1233;

/// Class breakdown from `categorize::classify` over every PROT entry's own
/// sectors (`toc[p+3] - toc[p+2]`; see `docs/formats/prot.md`). Order
/// doesn't matter; the test asserts each `(class_name, count)` pair.
///
/// Re-pinned when the entry size was corrected. The reader used to return
/// `max(toc[p+5] - toc[p+3] + 4, footprint)`, and that first term is entry
/// `p`'s two *successors'* sizes plus 4 - so 931 of the 1233 entries were
/// classified over a buffer that ran into their neighbours. Three kinds of
/// movement follow, and each is called out on the affected row below:
/// phantom classes that only ever matched a neighbour's content, entries
/// whose byte histogram was being set by borrowed bytes, and formats whose
/// detector reached past the entry.
const EXPECTED_CLASS_COUNTS: &[(&str, usize)] = &[
    // `battle_data_pack` = the player battle files (retail `battle_data`
    // block, extraction 0863..0866 = PLAYER1..4). The realigned
    // `[id, offset, size]` table frame accepts all four, including
    // Terra's 0866 all-default (`id = 0`) table.
    ("battle_data_pack", 4),
    ("data_field_streaming", 34),
    // `data_field_truncated` 0 → 1: a streaming entry whose chunk walk runs
    // to the end of its own sectors. It used to complete only because the
    // buffer continued into the next entry.
    ("data_field_truncated", 1),
    // `field_pack` 2 → 1: one of the two entries (PROT 4) leads with a
    // count=6 scene-asset table at offset 0 and only carries a field-pack
    // *region* deeper in the file. The offset-0 scene-table shape is the
    // authoritative outer classification (same precedence as v12-over-
    // fieldpack), so it lands in `scene_asset_table`. PROT 5 remains the
    // sole pure field_pack.
    ("field_pack", 1),
    // `lzs_container` 34 → 33: one entry's descriptor walk no longer
    // completes inside its own sectors.
    ("lzs_container", 33),
    // `bse_bank` - the `bse.dat` master sound bank (extraction 888, the loader's
    // raw TOC `0x37A`) plus its uncalled sibling at 1195.
    ("bse_bank", 2),
    // `efect_pack` - the runtime `efect.dat` 2-pack (extraction 0873). One entry.
    ("efect_pack", 1),
    // `field_map` **101** - the per-scene `DATA\FIELD\<scene>.MAP`, slot 0 of
    // every scene block at a fixed `0x12000` bytes, exactly the count
    // `docs/formats/field-map.md` gives.
    //
    // This was briefly pinned at 104 with the reading "three more resolve now
    // that each entry is exactly its own `0x12000`". That was wrong, and the
    // number had been ratcheted to match a detector defect: `0x12000` is a
    // FOOTPRINT, not a signature. 111 entries are that size; the extra three
    // (63, 71, 701) are `scene_tmd_stream` members of their scene blocks that
    // happen to be 36 sectors long, and they passed only because
    // `field_map::detect`'s all-zero-trigger-header escape hatch had no
    // precondition. The hatch is now gated on the condition its own doc always
    // stated (object table + collision grid also zero), so the class is again
    // exactly the block-slot-0 entries.
    ("field_map", 101),
    // `init_pak` - the boot logo/overlay pack (extraction 0895). One entry.
    // Its detector carried a `>= 0x30000` length floor taken from the entry's
    // old over-read size; PROT 0895 is 75 sectors (`0x25800`), so the floor
    // rejected the real file. The floor is now the reach of the last logo TIM.
    ("init_pak", 1),
    ("mips_overlay", 22),
    // `monster_sound_bank` matches **nothing**: its only historical match was
    // `summon.dat` (extraction 893), whose leading `[u32 mode = 2][256-entry
    // CLUT, every colour STP-set]` satisfies the `[u32 format = 2][256 SPU
    // addresses >= 0x8000]` test byte-for-byte. The real `h:\mpack\monster.snd`
    // is extraction 891 (`FUN_8003E104`'s `li v0,0x37d`) and lands in
    // `vab_multi_bank`. Kept pinned at 0 so a detector-order regression that
    // re-steals summon.dat fails here.
    ("monster_sound_bank", 0),
    // `all_zeros` 4: entries that really are empty - honest verdicts about
    // small entries, not lost content.
    ("all_zeros", 4),
    // `mostly_zeros` 16 → **0**. A class named after a byte histogram is a
    // confession that no detector claimed the entry, so a nonzero count here
    // is a worklist rather than a result. These 16, plus the 8
    // `unknown_low_entropy` below, were the 24-entry fallthrough set now
    // claimed by format (23 `scene_event_scripts` + 1 `overlay_data_blob`).
    // Pinned at 0: a reappearance means a format detector regressed, not that
    // the disc grew an empty entry.
    ("mostly_zeros", 0),
    // `overlay_data_blob` 24 → **25**: `0974_other_game.BIN` is an overlay
    // data image like its block-mates 0970-0980 (`"OTHER3 \n"` + printf
    // formats + an `addiu sp,sp,-0x40` prologue at `+0x44`), but
    // `is_overlay_data_image` implemented only half its own stated argument -
    // it demanded an overlay-pointer run, which 0974 has none of.
    ("overlay_data_blob", 25),
    ("overlay_ptr_table", 42),
    // `pochi_filler` - reserved dev filler slots, incl. the final TOC entry
    // (index 1232, the archive's last data sector).
    ("pochi_filler", 266),
    // `scene_asset_table` 88, unchanged - every one of them sits at offset 0
    // of its own entry with every descriptor payload inside it.
    ("scene_asset_table", 88),
    // `scene_tmd_stream` **182**. Three of these (63, 71, 701) were briefly
    // counted as `field_map` because they are exactly `0x12000` bytes and the
    // zero-header escape hatch accepted them; they lead `[u32 size]` then the
    // `0x80000002` TMD magic and are ordinary members of their scene blocks.
    // See the `field_map` note above.
    ("scene_tmd_stream", 182),
    ("scene_vab_stream", 218),
    ("scene_v12_table", 97),
    // `scene_scripted_asset_table` 79 → **0**, and `scene_event_scripts`
    // 21 → 78 as the same entries reclassify by their own content.
    //
    // The "prescript-prefixed asset table" was never a format. Every one of
    // those 79 hits was a table found at a 0x800-aligned offset that is
    // exactly the **next entry's start LBA** - i.e. the neighbour's ordinary
    // offset-0 table, seen through a window that ran past the entry. Reading
    // each entry's own sectors leaves the 88 bare tables untouched and the
    // carrier entries classed as what they are: event-script prescripts.
    // Pinned at 0 so a reader or detector regression that resurrects the
    // phantom fails here (see `crates/asset/tests/scene_asset_table_walk_real.rs`).
    ("scene_scripted_asset_table", 0),
    // `scene_event_scripts` 78 → **101**: one per scene block, at slot 2.
    // The missing 23 were falling through to a byte-histogram class because
    // `categorize`'s prescript detector gated on a frame-opener RATE floor
    // (>= 45 % of records leading `model_sel = -1`) that had been calibrated
    // while the over-read made 79 of these look like
    // `scene_scripted_asset_table`. With that crutch gone the floor dropped a
    // fifth of the format - `geremi` / `tunnela` / `tunnelb` / `edson` score
    // zero, carrying no transform-node record at all. Identity is now the
    // structural shape (`[u16 count][u16 offsets]`, `offsets[0] == 2+2*count`,
    // monotonic, in-bounds, `count >= 2`); the opener rate is a quality signal,
    // not an identity test.
    ("scene_event_scripts", 101),
    // `summon_readef` - `summon.dat` / `readef.DAT` (extraction 893 / 894).
    ("summon_readef", 2),
    ("tim_pack", 7),
    // `vab_multi_bank` matches one PROT entry: extraction 891, the content the
    // CDNAME `monster_se` define points at (`h:\mpack\monster.snd`). The
    // `level_up` in its extraction filename is the +2 label shift.
    ("vab_multi_bank", 1),
    // `zero_sector_high_entropy` 4 → 0: those four entries are leading zeros
    // followed by *nothing* of their own - the high-entropy body belonged to
    // the next entry. They are `all_zeros` above.
    ("zero_sector_high_entropy", 0),
    // Statistical residual buckets - **all four now empty, and that is the
    // point**. `unknown_low_entropy` 8 → 0: these eight, plus the 16
    // `mostly_zeros` above, were the 24-entry fallthrough set, and they are
    // now claimed by format: 23 are `scene_event_scripts` (block slot 2) and
    // one is `overlay_data_blob` (0974). An entry landing in any of these
    // buckets is a detector regression, and the census exists to notice.
    ("unknown_low_entropy", 0),
    ("unknown_high_entropy", 0),
    ("unknown_other", 0),
    ("constant_byte", 0),
];

/// Number of PROT entries that pass the strict streaming-format filter
/// (terminator + ≥2 chunks + all known types + magic OK).
const EXPECTED_STREAM_HITS: usize = 34;

/// Total sub-assets across all streaming hits.
const EXPECTED_TOTAL_SUBASSETS: usize = 583;

/// One pinned PROT entry's size, used as a quick sanity check that the TOC
/// math hasn't drifted. 41 sectors - the sector gap to entry 149.
const PINNED_ENTRY: (u32, u64) = (148, 83_968); // entry 148 = retock

/// Number of PROT entries that strict-validate as real LZS containers
/// (the strict check requires no section-input-overrun and a minimum decoded
/// total of [`MIN_REAL_DECODE_BYTES`]). 113 → 110 with the corrected entry
/// size: three entries' descriptor walks were completing on bytes that
/// belong to the following entry.
const EXPECTED_LZS_CONTAINERS_STRICT: usize = 110;

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
