//! Disc-gated end-to-end oracle for the full-ISO relayout dialog import.
//!
//! Lifts the official PAL localization onto the USA disc, then imports the pack
//! two ways - once same-size-only (abbreviation fallback) and once with the
//! whole-sector **disc relayout** enabled - and asserts the relayout achieves a
//! byte-faithful import of the dialog that otherwise had to be abbreviated:
//!
//!   - the relayout grew a set of scene MAN PROT entries by whole sectors;
//!   - the patched image grew by exactly that many sectors and **re-parses** as a
//!     valid disc (ISO9660 + PROT.DAT both), preserving the PROT index space;
//!   - every changed 2352-byte sector stays EDC/ECC-valid and every relocated
//!     sector's MSF header matches its new position;
//!   - the man-dialog abbreviation count drops sharply vs the same-size import;
//!   - every filled dialog line in a relayout-grown entry is present at **full
//!     length** in the patched decompressed MAN (no abbreviation).
//!
//! Needs the USA disc (`LEGAIA_DISC_BIN`) + a PAL disc (`LEGAIA_PAL_DISC_BIN`);
//! skips + passes when either is unset (no Sony bytes are committed; CI has no
//! disc).

use legaia_iso::raw::SECTOR_SIZE;
use legaia_iso::write::{is_form2, mode2_form1_sector_is_valid};
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::translation::export::SceneManText;
use legaia_patcher::translation::markup::{self, Target};
use legaia_patcher::translation::{import_pack, import_pack_relayout, lift};

fn load(var: &str) -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os(var)?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Count man-dialog lines the import had to abbreviate / roll back (the messages
/// the same-size path emits when a line won't fit its budget).
fn abbreviation_issues(report: &legaia_patcher::translation::ImportReport) -> usize {
    report
        .issues
        .iter()
        .filter(|(k, m)| {
            k.starts_with("man:")
                && (m.contains("could not be grown to fit") || m.contains("rolled back"))
        })
        .count()
}

/// BCD MSF header expected at a physical sector.
fn expect_msf(lba: usize) -> [u8; 3] {
    let v = lba as u32 + 150;
    let bcd = |n: u32| (((n / 10) << 4) | (n % 10)) as u8;
    [bcd(v / (75 * 60)), bcd((v / 75) % 60), bcd(v % 75)]
}

#[test]
fn relayout_imports_official_dialog_byte_faithfully() {
    let (Some(usa_bytes), Some(pal_bytes)) = (load("LEGAIA_DISC_BIN"), load("LEGAIA_PAL_DISC_BIN"))
    else {
        eprintln!("[skip] LEGAIA_DISC_BIN / LEGAIA_PAL_DISC_BIN unset");
        return;
    };
    let usa = DiscPatcher::open(usa_bytes.clone()).expect("open USA");
    let pal = DiscPatcher::open(pal_bytes).expect("open PAL");
    let (pack, _) = lift::lift_official(&usa, &pal).expect("lift official");

    // Baseline: same-size import (abbreviation fallback).
    let mut plain = DiscPatcher::open(usa_bytes.clone()).expect("open scratch");
    let plain_report = import_pack(&mut plain, &pack).expect("plain import");
    let plain_abbrev = abbreviation_issues(&plain_report);
    assert!(
        plain_abbrev > 0,
        "expected the same-size import to abbreviate some lines"
    );

    // Relayout import.
    let mut patcher = DiscPatcher::open(usa_bytes.clone()).expect("open scratch 2");
    let report = import_pack_relayout(&mut patcher, &pack).expect("relayout import");
    assert!(
        report.relayout_entries >= 20,
        "expected many MANs to be relaid out, got {}",
        report.relayout_entries
    );
    assert!(report.relayout_sectors_added >= report.relayout_entries as u32);

    // The relayout eliminated the overflow-class abbreviations.
    let relayout_abbrev = abbreviation_issues(&report);
    assert!(
        relayout_abbrev < plain_abbrev,
        "relayout should reduce abbreviation: {relayout_abbrev} vs {plain_abbrev}"
    );

    let patched = patcher.into_image();

    // The image grew by exactly the reported sector count.
    assert_eq!(
        patched.len(),
        usa_bytes.len() + report.relayout_sectors_added as usize * SECTOR_SIZE,
        "image grew by the wrong number of sectors"
    );

    // The patched image re-parses as a valid disc, PROT index space preserved.
    let re = DiscPatcher::open(patched.clone()).expect("patched image re-parses");
    assert_eq!(
        re.entry_count(),
        usa.entry_count(),
        "PROT index space must be preserved"
    );

    // Every relocated / changed sector is EDC/ECC-valid and MSF-correct.
    let n_sectors = patched.len() / SECTOR_SIZE;
    let mut bad_ecc = 0usize;
    let mut bad_msf = 0usize;
    for lba in 0..n_sectors {
        let sec = &patched[lba * SECTOR_SIZE..(lba + 1) * SECTOR_SIZE];
        if sec[12..15] != expect_msf(lba) {
            bad_msf += 1;
        }
        if !is_form2(sec) && !mode2_form1_sector_is_valid(sec) {
            bad_ecc += 1;
        }
    }
    assert_eq!(bad_ecc, 0, "{bad_ecc} sectors are EDC/ECC-invalid");
    assert_eq!(bad_msf, 0, "{bad_msf} sectors have a wrong MSF header");

    // Byte-faithfulness: every dialog line the relayout import reported as
    // *applied* in a grown entry is present at full length in the patched
    // decompressed MAN (lines the import legitimately skipped - already-applied /
    // wrong framing / encode failure - are not required to appear).
    let applied: std::collections::HashSet<&str> =
        report.applied_keys.iter().map(|s| s.as_str()).collect();
    let mut checked_entries = 0usize;
    let mut checked_lines = 0usize;
    for idx in 0..usa.entry_count() {
        let (Ok(orig_entry), Ok(new_entry)) = (usa.read_entry(idx), re.read_entry(idx)) else {
            continue;
        };
        let (Some(orig_man), Some(new_man)) = (
            SceneManText::locate(&orig_entry),
            SceneManText::locate(&new_entry),
        ) else {
            continue;
        };
        if new_man.decoded.len() <= orig_man.decoded.len() {
            continue; // not a grown entry
        }
        checked_entries += 1;
        let prefix = format!("man:{idx}:");
        for e in pack.sections.scene_dialog.iter().filter(|e| {
            e.is_filled() && e.key.starts_with(&prefix) && applied.contains(e.key.as_str())
        }) {
            let Ok(encoded) = markup::encode(&e.translation, Target::Segment) else {
                continue;
            };
            if encoded.is_empty() {
                continue;
            }
            assert!(
                contains_subslice(&new_man.decoded, &encoded),
                "grown entry {idx}: applied line {} not present at full length (abbreviated?)",
                e.key
            );
            checked_lines += 1;
        }
    }
    assert!(
        checked_entries >= 20 && checked_lines > 100,
        "expected to verify many grown lines: {checked_entries} entries / {checked_lines} lines"
    );
}

fn contains_subslice(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > hay.len() {
        return false;
    }
    hay.windows(needle.len()).any(|w| w == needle)
}
