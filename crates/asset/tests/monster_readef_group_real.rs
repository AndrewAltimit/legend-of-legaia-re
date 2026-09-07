//! Disc-gated census of the monster record's readef animation-group byte
//! (`+0x1C`) - the byte the per-turn initiative scheduler `FUN_801DABA4`
//! turns into the side-band streaming applier's base slot
//! (`base = 3 * readef_group`, `overlay_battle_action_801daba4.txt`
//! `0x801db098` / `0x801db0c8`).
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.
//!
//! What this catches:
//! - `+0x1C` stops parsing as the group index (a value escapes the
//!   `readef.DAT` group space `0..=25`, i.e. its base escapes the file's 78
//!   slots).
//! - The retail group census changes - in particular the three groups
//!   `19..=21` that no record names, which is the closing evidence for
//!   `docs/formats/summon-readef.md`'s open question about those groups.

use legaia_asset::monster_archive;
use std::path::PathBuf;

/// `readef.DAT` ships 78 slots of `0x10800`, i.e. 26 three-slot groups.
const READEF_GROUPS: u8 = 26;

fn entry_867() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let f = PathBuf::from(p).join("0867_battle_data.BIN");
        if f.is_file() {
            return std::fs::read(f).ok();
        }
    }
    None
}

#[test]
fn every_record_names_a_real_readef_group_and_none_names_19_to_21() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };
    let recs = monster_archive::records(&entry).expect("archive walk");
    assert!(
        recs.len() > 150,
        "expected the full roster, got {}",
        recs.len()
    );

    let mut census = [0usize; READEF_GROUPS as usize];
    for r in &recs {
        assert!(
            r.readef_group < READEF_GROUPS,
            "id {} ({}) names readef group {} - base 0x{:02X} is past the file's 78 slots",
            r.id,
            r.name,
            r.readef_group,
            3 * u16::from(r.readef_group)
        );
        census[r.readef_group as usize] += 1;
    }

    // The applier's second-upload gate (`FUN_801F12D0` stage 4) excludes
    // bases 0x37..=0x41, and the three groups inside that hole - 19, 20, 21
    // (bases 0x39 / 0x3C / 0x3F) - are exactly the ones whose `base+1` slot
    // holds a duplicate actor record instead of a texture page. No retail
    // monster record names them, so nothing streams those slots through the
    // per-turn seed.
    for g in [19usize, 20, 21] {
        assert_eq!(
            census[g], 0,
            "readef group {g} is named by {} record(s)",
            census[g]
        );
    }
    // Group 0 is the default: the majority of the roster carries no dedicated
    // effect bank.
    assert!(
        census[0] > recs.len() / 2,
        "group 0 should be the roster default, got {}",
        census[0]
    );

    for (g, n) in census.iter().enumerate() {
        eprintln!("[readef] group {g:2} base 0x{:02X}  {n:3} record(s)", 3 * g);
    }
}
