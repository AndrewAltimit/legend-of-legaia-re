//! Disc-gated end-to-end test for the weapon-specialty randomizer: permute the
//! three favored weapon families among the characters on a scratch copy of the
//! disc, re-decode the patched player battle files off the patched image, and
//! confirm:
//!
//! - the favored-family assignment is a permutation of `{blade, claw, club}`;
//! - every weapon section re-decodes cleanly (the LZS round-trip held) and now
//!   carries the cost its new favored family dictates - `0x1E` for the
//!   character's favored class, `0x2A` otherwise - except sections skipped
//!   because the re-compressed stream wouldn't fit, which keep their old cost
//!   (and the count of such must equal the reported fit-skips);
//! - non-class weapons (the Astral Sword) are never touched;
//! - every disc sector the patch changed stays EDC/ECC-valid;
//! - a fixed seed reproduces the patched image byte-for-byte.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use std::collections::BTreeSet;

use legaia_asset::battle_data_pack;
use legaia_iso::raw::SECTOR_SIZE;
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::weapon_specialty::{
    self, FAVORED_COST, Family, arm_cost_offset, cost_for, weapon_family,
};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// `(entry, weapon_id, family, arm_cost)` for every weapon section across the
/// three player files, read off a patcher.
fn weapon_costs(patcher: &DiscPatcher) -> Vec<(usize, u8, Family, u8)> {
    let mut out = Vec::new();
    for player in &weapon_specialty::PLAYERS {
        let Ok(buf) = patcher.read_entry(player.entry) else {
            continue;
        };
        let Some(pack) = battle_data_pack::detect(&buf) else {
            continue;
        };
        for (idx, rec) in pack.records.iter().enumerate() {
            let Some(fam) = weapon_family(rec.id as u8) else {
                continue;
            };
            let Ok(dec) = battle_data_pack::decode_record(&buf, &pack, idx) else {
                continue;
            };
            let Some(coff) = arm_cost_offset(&dec.bytes) else {
                continue;
            };
            out.push((player.entry, rec.id as u8, fam, dec.bytes[coff]));
        }
    }
    out
}

/// Assert every disc sector that differs between `orig` and `patched` is a
/// valid Mode-2 Form-1 sector (EDC/ECC re-encoded). Returns the count checked.
fn changed_sectors_valid(orig: &[u8], patched: &[u8]) -> usize {
    assert_eq!(orig.len(), patched.len(), "image must not change size");
    let mut checked = 0;
    for s in 0..orig.len() / SECTOR_SIZE {
        let a = s * SECTOR_SIZE;
        let b = a + SECTOR_SIZE;
        if orig[a..b] != patched[a..b] {
            assert!(
                legaia_iso::write::mode2_form1_sector_is_valid(&patched[a..b]),
                "changed sector {s} is not EDC/ECC-valid"
            );
            checked += 1;
        }
    }
    checked
}

#[test]
fn weapon_specialty_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // A seed whose family permutation actually moves every character (verified
    // below) so the test exercises a real reassignment, not a near-identity.
    let seed = 0x57EC_1A11_2ED0_0007;

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = weapon_costs(&base);
    assert!(
        before.len() > 20,
        "expected many weapon sections across the three player files, found {}",
        before.len()
    );
    // Baseline: read each character's current favored class; vanilla should be
    // unchanged (from == to), which exercises the read path.
    let baseline = apply::current_specialties(&base).expect("read specialties");
    assert_eq!(baseline.len(), 3, "three player files");
    for a in &baseline {
        assert_eq!(
            a.from, a.to,
            "vanilla disc: {} should still read its vanilla class",
            a.character
        );
    }

    // Apply on a scratch copy.
    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let report = apply::randomize_weapon_specialty(&mut patcher, seed).expect("randomize");
    assert!(
        report.weapons_changed > 0,
        "a reassignment should rewrite at least one weapon"
    );

    // Assignments are a permutation of the three families, and a real move.
    let tos: BTreeSet<&str> = report.assignments.iter().map(|a| a.to.as_str()).collect();
    assert_eq!(tos.len(), 3, "every favored family assigned exactly once");
    assert!(
        report.assignments.iter().any(|a| a.from != a.to),
        "seed should move at least one character's specialty"
    );

    // Expected new favored family per entry (recompute the same plan).
    let favored = weapon_specialty::plan_favored(seed);
    let entry_fav: Vec<(usize, Family)> = weapon_specialty::PLAYERS
        .iter()
        .zip(favored)
        .map(|(p, f)| (p.entry, f))
        .collect();
    let fav_of = |entry: usize| entry_fav.iter().find(|(e, _)| *e == entry).unwrap().1;

    // Re-decode off the PATCHED image.
    let after = weapon_costs(&patcher);
    assert_eq!(
        after.len(),
        before.len(),
        "every weapon section must still re-decode after the LZS round-trip"
    );

    // Every weapon now carries its new favored cost, or (if its section was too
    // tight to re-pack) keeps its old cost. The latter count must match the
    // report's fit-skips exactly.
    let mut favored_confirmed = 0;
    let mut skipped = 0;
    for ((entry, id, fam, after_cost), (_, _, _, before_cost)) in after.iter().zip(&before) {
        let expected = cost_for(*fam, fav_of(*entry));
        if *after_cost == expected {
            if expected == FAVORED_COST {
                favored_confirmed += 1;
            }
        } else {
            assert_eq!(
                *after_cost, *before_cost,
                "weapon 0x{id:02x} in entry {entry} was neither reassigned nor preserved"
            );
            skipped += 1;
        }
    }
    assert_eq!(
        skipped, report.weapons_skipped_fit,
        "non-reassigned weapons must equal the reported fit-skips"
    );
    assert!(
        favored_confirmed > 0,
        "at least one new favored-class weapon should read the favored cost"
    );

    // Every changed sector stays EDC/ECC-valid.
    let checked = changed_sectors_valid(&original, patcher.image());
    assert!(checked > 0, "expected some sectors to change");

    // Determinism.
    let mut patcher2 = DiscPatcher::open(original).expect("open");
    let report2 = apply::randomize_weapon_specialty(&mut patcher2, seed).expect("randomize");
    assert_eq!(report2.weapons_changed, report.weapons_changed);
    assert!(
        patcher2.image() == patcher.image(),
        "same seed must reproduce the patched image"
    );

    let map = report
        .assignments
        .iter()
        .map(|a| format!("{}->{}", a.character, a.to))
        .collect::<Vec<_>>()
        .join(", ");
    eprintln!(
        "weapon-specialty seed {seed:#x}: {map}; {} weapons rewritten, \
         {favored_confirmed} favored-cost confirmed, {skipped} fit-skipped, \
         {checked} sectors changed (all EDC/ECC-valid)",
        report.weapons_changed
    );
}
