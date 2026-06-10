//! Disc-gated regression for [`legaia_asset::battle_char_palette`].
//!
//! Drives the full `FUN_80052FA0` decode+assembly against the on-disc Vahn
//! battle record (extraction PROT `0863`, raw TOC `0x361` = the `PLAYER1`
//! file; see `docs/formats/cdname.md` § numbering space). The historical
//! `0861` reading matched the same record through the two 1-sector stub
//! entries preceding it — their `2 × 0x800` bytes are exactly the `+0x1000`
//! "pochi header" the old assert documented. The
//! band colours themselves are Sony data, so this asserts only the structure
//! (band bases + counts) plus an FNV-1a digest of the colours — never the raw
//! palette bytes. Skips when `LEGAIA_DISC_BIN` is unset so CI works without
//! redistributing disc data.

use legaia_asset::battle_char_palette::{find_record0, parse_record};
use std::path::PathBuf;

/// Vahn's three effective bands (base, colour count). band@0x00 = record0's
/// CLUT B, 0x40 = sub0's trailing CLUT, 0x70 = sub4's trailing CLUT.
const EXPECTED_BANDS: [(u16, usize); 3] = [(0x00, 32), (0x40, 48), (0x70, 32)];

/// FNV-1a-64 of the bands sorted by base: for each band, `base` (u16-LE),
/// `count` (u16-LE), then the colours (u16-LE). Pins byte-exact extraction
/// without committing the palette itself.
const EXPECTED_DIGEST: u64 = 0x68CC_D40B_E368_E0B9;

fn locate_player_file_0863() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let prot = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("extracted")
        .join("PROT");
    if !prot.is_dir() {
        return None;
    }
    std::fs::read_dir(&prot)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|s| s.starts_with("0863_") && s.ends_with(".BIN"))
        })
}

fn fnv1a64(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[test]
fn vahn_battle_palette_from_disc() {
    let Some(path) = locate_player_file_0863() else {
        eprintln!("LEGAIA_DISC_BIN or extracted/PROT not available; skipping");
        return;
    };
    let file = std::fs::read(&path).expect("read PROT 0863");

    let rec0 = find_record0(&file).expect("locate record0 in the player file");
    assert_eq!(rec0, 0, "record0 leads the player file (entry-aligned)");

    let pal = parse_record(&file, rec0).expect("parse battle palette");

    // Drop count-0 no-ops are already filtered; keep the last band per base.
    let mut bands: Vec<_> = pal.bands.iter().collect();
    bands.sort_by_key(|b| b.base);
    let got: Vec<(u16, usize)> = bands.iter().map(|b| (b.base, b.colors.len())).collect();
    assert_eq!(got, EXPECTED_BANDS, "Vahn battle CLUT band layout");

    let mut h = 0xcbf29ce484222325u64;
    for b in &bands {
        h = fnv1a64(h, &b.base.to_le_bytes());
        h = fnv1a64(h, &(b.colors.len() as u16).to_le_bytes());
        for &c in &b.colors {
            h = fnv1a64(h, &c.to_le_bytes());
        }
    }
    assert_eq!(
        h, EXPECTED_DIGEST,
        "Vahn battle palette colours (byte-exact)"
    );

    // STP transform: non-zero colours get bit 15, zero stays zero.
    for b in &bands {
        for (&disc, &vram) in b.colors.iter().zip(b.vram_words().iter()) {
            let want = if disc != 0 { disc | 0x8000 } else { 0 };
            assert_eq!(vram, want);
        }
    }
}
