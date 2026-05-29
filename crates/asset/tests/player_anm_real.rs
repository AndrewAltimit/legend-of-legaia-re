//! Disc-gated regression pinning the player-ANM source.
//!
//! For each scene whose first sub-entry was identified by byte-equality
//! against the runtime `DAT_8007B7C8` buffer in a live save state, we
//! parse the player-LZS container, find the type-0x05 ("MOVE") section,
//! LZS-decode it, and verify [`legaia_asset::player_anm::parse`] returns a
//! bundle whose record count + total decoded size match the pinned values.
//! Skips when `LEGAIA_DISC_BIN` is unset.

use std::path::PathBuf;

use legaia_asset::player_anm;

/// One row of the per-scene player-ANM pin table.
struct Pin {
    /// PROT entry index in the per-scene cluster.
    prot_index: u32,
    /// Short label for assertions.
    label: &'static str,
    /// `parse_player_lzs` descriptor count for this entry.
    descriptor_count: usize,
    /// Expected record count from the disc container header.
    expected_record_count: u32,
    /// Expected LZS-decoded size (matches the descriptor's claimed size).
    expected_decoded_bytes: usize,
}

const PINS: &[Pin] = &[
    Pin {
        prot_index: 4,
        label: "town01",
        descriptor_count: 6,
        expected_record_count: 69,
        expected_decoded_bytes: 96_448,
    },
    Pin {
        prot_index: 13,
        label: "town0b",
        descriptor_count: 6,
        expected_record_count: 69,
        expected_decoded_bytes: 91_784,
    },
    Pin {
        prot_index: 183,
        label: "balden",
        descriptor_count: 6,
        expected_record_count: 72,
        expected_decoded_bytes: 71_604,
    },
    Pin {
        prot_index: 408,
        label: "bubu1",
        descriptor_count: 6,
        expected_record_count: 70,
        expected_decoded_bytes: 87_844,
    },
    Pin {
        prot_index: 1203,
        label: "other5 (battle-form player anim)",
        descriptor_count: 6,
        expected_record_count: 30,
        expected_decoded_bytes: 87_684,
    },
];

fn prot_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?
        .to_path_buf();
    let p = repo.join("extracted").join("PROT");
    p.is_dir().then_some(p)
}

fn locate_entry(dir: &PathBuf, prot_index: u32) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let name = e.file_name();
        let s = name.to_string_lossy();
        if s.starts_with(&format!("{prot_index:04}_")) && s.ends_with(".BIN") {
            return Some(e.path());
        }
    }
    None
}

#[test]
fn pinned_scenes_decode() {
    let Some(dir) = prot_dir() else {
        eprintln!("LEGAIA_DISC_BIN or extracted/PROT not available; skipping");
        return;
    };
    for pin in PINS {
        let path = locate_entry(&dir, pin.prot_index)
            .unwrap_or_else(|| panic!("PROT {:04} not found", pin.prot_index));
        let bytes = std::fs::read(&path).expect("read PROT entry");
        let bundles = player_anm::find_in_entry(&bytes, pin.descriptor_count);
        assert_eq!(
            bundles.len(),
            1,
            "PROT {:04} ({}): expected exactly 1 player ANM bundle, found {}",
            pin.prot_index,
            pin.label,
            bundles.len()
        );
        let bundle = &bundles[0];
        assert_eq!(
            bundle.record_count, pin.expected_record_count,
            "PROT {:04} ({}): record count",
            pin.prot_index, pin.label
        );
        assert_eq!(
            bundle.decoded.len(),
            pin.expected_decoded_bytes,
            "PROT {:04} ({}): decoded byte count",
            pin.prot_index,
            pin.label
        );
        // Every record's marker_1 should be 0x080C (the canonical ANM tag),
        // and the per-record size invariant `16 + 8 * (a & 0xFF) * b` must
        // hold byte-exact for every record across every pinned scene.
        for i in 0..bundle.record_count as usize {
            assert_eq!(
                bundle.record_marker_1(i),
                Some(player_anm::ANM_MARKER_1),
                "PROT {:04} ({}) record {i}: marker_1 mismatch",
                pin.prot_index,
                pin.label
            );
            // Size invariant: record(i) returns Ok iff size formula matches.
            let _ = bundle.record(i).unwrap_or_else(|e| {
                panic!(
                    "PROT {:04} ({}) record {i}: size invariant failed: {e}",
                    pin.prot_index, pin.label
                )
            });
        }
    }
}
