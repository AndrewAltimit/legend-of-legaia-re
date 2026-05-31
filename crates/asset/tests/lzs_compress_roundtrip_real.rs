//! Disc-gated corpus round-trip for the LZS encoder ([`legaia_lzs::compress`]).
//!
//! The retail game ships only an LZS *decoder*; `compress` is our re-packer for
//! editing assets. Its correctness criterion is that every real decompressed
//! asset round-trips: `decompress(compress(x), x.len()) == x`. Driving it
//! against real game data exercises literals, long back-references,
//! self-overlapping RLE runs and the 4 KB window boundary far more thoroughly
//! than synthetic input.
//!
//! Reads the per-entry files the extractor writes under `extracted/PROT/`;
//! skips and passes when that directory is absent, like the other disc-gated
//! tests, so CI runs without redistributing Sony data.

use legaia_asset::monster_archive::SLOT_STRIDE;

fn prot_dir() -> Option<std::path::PathBuf> {
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let d = std::path::PathBuf::from(p);
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

/// `decompress(compress(x), x.len())` must reproduce `x` exactly.
fn lzs_roundtrips(data: &[u8]) -> bool {
    let packed = legaia_lzs::compress(data);
    matches!(legaia_lzs::decompress(&packed, data.len()), Ok(u) if u == data)
}

// Drives the encoder against the decompressed monster records (PROT 867): a
// large, structurally diverse corpus (TMD + texture pools + stat/spell tables).
#[test]
fn lzs_compress_roundtrips_monster_records() {
    let Some(dir) = prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let entry = std::fs::read(dir.join("0867_battle_data.BIN")).expect("read monster archive");
    let slots = entry.len() / SLOT_STRIDE;

    let mut orig_total = 0usize;
    let mut packed_total = 0usize;
    let mut tested = 0usize;
    for i in 0..slots {
        let slot = &entry[i * SLOT_STRIDE..((i + 1) * SLOT_STRIDE).min(entry.len())];
        if slot.len() < 4 {
            continue;
        }
        let declared = u32::from_le_bytes(slot[0..4].try_into().unwrap()) as usize;
        if declared == 0 {
            continue; // filler slot
        }
        let Ok(decoded) = legaia_lzs::decompress(&slot[4..], declared) else {
            continue;
        };
        assert!(
            lzs_roundtrips(&decoded),
            "monster slot {i} failed to round-trip ({} bytes)",
            decoded.len()
        );
        orig_total += decoded.len();
        packed_total += legaia_lzs::compress(&decoded).len();
        tested += 1;
    }
    assert!(tested > 100, "expected many decoded records, got {tested}");
    // Real structured data must actually compress, not merely round-trip; an
    // all-literals encoder would inflate by ~12.5%. Guards against a regression
    // to literal-only output.
    assert!(
        packed_total < orig_total,
        "encoder did not compress monster corpus: {packed_total} >= {orig_total}"
    );
    eprintln!(
        "monster corpus: {tested} records, {orig_total} -> {packed_total} ({:.1}%)",
        100.0 * packed_total as f64 / orig_total as f64
    );
}

// Round-trips LZS *container*-format sections (the other major on-disc LZS
// shape) across a sample of PROT entries. Sampling bounds the per-entry walk.
#[test]
fn lzs_compress_roundtrips_container_sections() {
    let Some(dir) = prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .expect("read PROT dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "BIN"))
        .collect();
    entries.sort();

    let mut sections = 0usize;
    for path in entries.iter().step_by(17) {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(decoded) = legaia_lzs::decompress_container_strict(&bytes) else {
            continue;
        };
        for sec in &decoded {
            if sec.is_empty() {
                continue;
            }
            assert!(
                lzs_roundtrips(sec),
                "container section in {:?} failed to round-trip ({} bytes)",
                path.file_name(),
                sec.len()
            );
            sections += 1;
        }
    }
    eprintln!("container sweep: {sections} sections round-tripped");
    assert!(sections > 0, "expected at least one container section");
}
