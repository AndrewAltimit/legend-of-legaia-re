//! Disc-gated regression test: walk every retail `scene_asset_table` bundle,
//! LZS-decode its MAN sub-asset, and assert the multi-section parser pins
//! every section's offset+length without overrunning the buffer.
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.
//!
//! What this catches:
//! - The MAN header parser regresses (a header byte being misread breaks the
//!   N0/N1/N2 / u24[0x28] decode and one of the section pointers ends up
//!   past the buffer).
//! - The section-chain walker drops the +3 length-prefix accounting (next
//!   section pointer ends up off by 3 bytes).
//! - The "section 5 is always a zero terminator" invariant breaks (= a
//!   retail scene has a 6th non-trivial section we don't know about, or
//!   our section count is wrong).
//! - The encounter-section interior decoder rejects a stride combination
//!   the runtime accepts.

use legaia_asset::man_section;
use legaia_asset::scene_asset_table;
use std::path::PathBuf;

fn extracted_prot() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ];
    candidates.into_iter().find(|p| p.is_dir())
}

#[test]
fn man_section_parses_every_retail_scene_bundle() {
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&prot)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("BIN"))
        .collect();
    entries.sort();

    let mut man_hits = 0usize;
    let mut encounter_hits = 0usize;
    let mut total_formations = 0usize;
    let mut total_regions = 0usize;

    for path in &entries {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        // Must be a scene_asset_table bundle.
        let Some(table) = scene_asset_table::detect(&bytes) else {
            continue;
        };
        // Must carry a MAN descriptor.
        let Some(man_desc) = table.descriptors.iter().find(|d| d.type_byte == 0x03) else {
            continue;
        };
        let start = man_desc.data_offset as usize;
        if start >= bytes.len() {
            continue;
        }
        let body = &bytes[start..];
        let Ok((man_bytes, _consumed)) =
            legaia_lzs::decompress_tracked(body, man_desc.size as usize)
        else {
            continue;
        };
        // Some entries' MAN descriptors point past the indexed footprint and
        // require the extended-footprint read; skip those silently rather
        // than flagging the test - this oracle is about parser correctness
        // on the bundles whose MAN bytes we can recover from the per-entry
        // dump.
        if man_bytes.len() as u32 != man_desc.size {
            continue;
        }

        let man = man_section::parse(&man_bytes)
            .unwrap_or_else(|e| panic!("MAN parse failed on {}: {e}", path.display()));
        man_hits += 1;

        // All 5 active sections plus the terminator fit in the buffer.
        for (i, s) in man.sections.iter().enumerate() {
            assert!(
                s.end_offset() <= man_bytes.len(),
                "section {i} of {} runs past MAN end ({:X}..{:X} > {})",
                path.display(),
                s.offset,
                s.end_offset(),
                man_bytes.len(),
            );
        }
        // Section 5 must be a zero-length terminator.
        assert!(
            man.terminator().is_terminator(),
            "section 5 of {} is not a zero terminator (len=0x{:X})",
            path.display(),
            man.terminator().length,
        );

        // Encounter section interior parses too.
        let s0_body = man
            .encounter_section_body(&man_bytes)
            .expect("encounter section body in bounds");
        let es = man_section::parse_encounter_section(s0_body).unwrap_or_else(|e| {
            panic!("encounter section parse failed on {}: {e}", path.display())
        });
        encounter_hits += 1;
        total_formations += es.formation_count as usize;
        total_regions += es.region_count as usize;

        // Strides match the runtime: the FUN_8003A110 reader hard-codes
        // these three slots (formation/condition/region) in the control
        // block.
        assert!(
            es.formation_stride >= 4,
            "{}: formation_stride={} < 4 (no room for count+ids)",
            path.display(),
            es.formation_stride,
        );
        assert!(
            es.region_stride >= 8,
            "{}: region_stride={} < 8 (no room for AABB+rate+range)",
            path.display(),
            es.region_stride,
        );

        // Every formation row must parse (count <= 4).
        for (i, f) in man_section::formation_records(s0_body, &es).enumerate() {
            assert!(
                f.is_some(),
                "{}: formation {i} failed to parse (stride {} too small for declared count)",
                path.display(),
                es.formation_stride,
            );
        }

        // Every region row must parse (>= 8 bytes per row).
        for (i, r) in man_section::region_records(s0_body, &es).enumerate() {
            assert!(
                r.is_some(),
                "{}: region {i} failed to parse",
                path.display(),
            );
        }
    }

    // Floor matches the documented corpus: 80 scene_asset_table bundles,
    // every one carrying a MAN descriptor whose payload fits in the indexed
    // entry. A few may be skipped if the descriptor offset spills past the
    // indexed footprint; the floor is intentionally generous.
    assert!(
        man_hits >= 70,
        "expected ≥ 70 MAN-bearing scenes, found {man_hits}"
    );
    assert_eq!(
        encounter_hits, man_hits,
        "every MAN-bearing scene should have a parseable encounter section"
    );

    eprintln!(
        "[man_section_corpus] {} scenes parsed, {} total formations, {} total regions",
        man_hits, total_formations, total_regions
    );
}
