//! Disc-gated: the piecewise `music_01` bank map
//! ([`music_labels::prot_entry_for_sound_test_index`]) must point every
//! audio-bearing sound-test row at an extraction PROT entry that actually
//! carries a SEQ, and must leave the dev-leftover / test rows (and the
//! 2-entry gap at extraction 1056/1057) empty. Guards against the +2 base
//! skew that mislabelled the whole low range. Skips when `extracted/PROT/`
//! is missing.

use std::path::PathBuf;

use legaia_engine_core::music_labels as ml;

fn extracted_prot() -> Option<PathBuf> {
    for p in [
        "extracted/PROT",
        "../extracted/PROT",
        "../../extracted/PROT",
    ] {
        let d = PathBuf::from(p);
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

/// Rows the curated source flags as dev leftovers / test files with no
/// shipping audio (M13 flute, M117, MPIANO, LEVELUP, the "A" test, and the
/// two Alundra/Wild-Arms placeholders are still SEQ-bearing, so only the
/// genuinely-empty ones are listed here).
const NO_AUDIO_ROWS: &[u32] = &[76, 77, 78, 79, 80];

fn entry_has_seq(prot: &std::path::Path, entry: u32) -> Option<bool> {
    let rd = std::fs::read_dir(prot).ok()?;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(&format!("{entry:04}_")) {
            let bytes = std::fs::read(e.path()).ok()?;
            return Some(bytes.windows(4).any(|w| w == b"pQES"));
        }
    }
    None
}

#[test]
fn piecewise_map_matches_on_disc_seq_residency() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing - run `legaia-extract` first");
        return;
    };

    let mut checked = 0;
    for i in 0..ml::MUSIC_TRACK_COUNT {
        let entry = ml::prot_entry_for_sound_test_index(i).expect("row in range");
        let Some(has_seq) = entry_has_seq(&prot, entry) else {
            eprintln!("[skip] extraction entry {entry} (row {i}) not found");
            return;
        };
        let expect_audio = !NO_AUDIO_ROWS.contains(&i);
        assert_eq!(
            has_seq, expect_audio,
            "sound-test row {i} -> extraction {entry}: expected audio={expect_audio}, has_seq={has_seq}"
        );
        checked += 1;
    }
    assert_eq!(checked, ml::MUSIC_TRACK_COUNT);

    // The 2-entry gap must not be claimed as a sound-test track.
    assert_eq!(ml::sound_test_index_for_prot_entry(1056), None);
    assert_eq!(ml::sound_test_index_for_prot_entry(1057), None);
    eprintln!("[music-bank] {checked} rows verified against on-disc SEQ residency");
}
