//! Disc-gated oracle for the Tactical-Art damage-power editor
//! (`legaia_patcher::arts_power` + `apply::set_arts_power`).
//!
//! Pins the on-disc power bytes (`record0 +0x24` in the player battle files) for
//! known arts on the real disc, applies a power edit on a scratch copy, and
//! confirms it is surgical (only the targeted power bytes change), the touched
//! player-file sector stays EDC/ECC-valid, the patched `record0` re-decodes to
//! the new values, re-applying is a no-op, an absent combo is refused, and a
//! fixed edit is byte-deterministic. Skips + passes without `LEGAIA_DISC_BIN`.

use legaia_art::queue::{Character, Command};
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_SIZE};
use legaia_patcher::apply;
use legaia_patcher::arts_power::{self, labeled_art_powers, parse_combo, player_entry_index};
use legaia_patcher::disc::DiscPatcher;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn scus(patcher: &DiscPatcher) -> Vec<u8> {
    legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54").expect("SCUS")
}

fn power_of(patcher: &DiscPatcher, ch: Character, combo: &[Command]) -> Vec<u8> {
    let entry = patcher.read_entry(player_entry_index(ch)).unwrap();
    let list = labeled_art_powers(&scus(patcher), &entry, ch).unwrap();
    list.into_iter()
        .find(|a| a.combo == combo)
        .unwrap_or_else(|| panic!("art with combo not found"))
        .power
}

#[test]
fn baseline_power_bytes_match_the_documented_encoding() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open");

    // Vahn's Burning Flare (R D L D L) is a 4-hit art whose power bytes decode to
    // the documented ascending LDF/UDF tiers.
    let bf = parse_combo("RDLDL").unwrap();
    assert_eq!(
        power_of(&patcher, Character::Vahn, &bf),
        vec![0x1D, 0x19, 0x1F, 0x1A]
    );
    // (v-0xC)%5 -> MULT, (v-0xC)%10<5 -> UDF else LDF.
    assert_eq!(arts_power::power_tier(0x1D), Some((false, 20))); // LDF x20
    assert_eq!(arts_power::power_tier(0x1A), Some((true, 28))); // UDF x28

    // Vahn's Somersault (U D U) is a single weak hit.
    let som = parse_combo("UDU").unwrap();
    assert_eq!(power_of(&patcher, Character::Vahn, &som), vec![0x18]);

    // Gala's Miracle Art (Biron Rage) carries no `+0x24` damage byte (spirit).
    let entry = patcher
        .read_entry(player_entry_index(Character::Gala))
        .unwrap();
    let gala = labeled_art_powers(&scus(&patcher), &entry, Character::Gala).unwrap();
    let miracle = gala.iter().find(|a| a.is_miracle).unwrap();
    assert!(
        miracle.power.is_empty(),
        "Gala's Miracle has no damage power byte"
    );
}

#[test]
fn power_edit_is_surgical_edc_valid_and_reparses() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open");
    let scus = scus(&patcher);
    let combo = parse_combo("RDLDL").unwrap(); // Vahn's Burning Flare

    let index = player_entry_index(Character::Vahn);
    let before_entry = patcher.read_entry(index).unwrap();
    let before = labeled_art_powers(&scus, &before_entry, Character::Vahn).unwrap();

    // Power the art down: every hit -> tier 0x0C (UDF x12).
    let report = apply::set_arts_power(&mut patcher, &[(combo.clone(), 0x0C)]).expect("apply");
    assert_eq!(report.edits.len(), 1, "exactly one record edited");
    assert_eq!(report.edits[0].character, Character::Vahn);
    assert_eq!(report.edits[0].old_power, vec![0x1D, 0x19, 0x1F, 0x1A]);
    assert_eq!(report.edits[0].new_power, vec![0x0C, 0x0C, 0x0C, 0x0C]);

    // The patched record0 re-decodes: only Burning Flare's power changed; every
    // other Vahn art keeps its bytes.
    let after_entry = patcher.read_entry(index).unwrap();
    let after = labeled_art_powers(&scus, &after_entry, Character::Vahn).unwrap();
    assert_eq!(before.len(), after.len());
    for (b, a) in before.iter().zip(after.iter()) {
        assert_eq!(b.combo, a.combo);
        if a.combo == combo {
            assert_eq!(a.power, vec![0x0C, 0x0C, 0x0C, 0x0C], "target powered down");
        } else {
            assert_eq!(
                b.power,
                a.power,
                "non-target art {:?} unchanged",
                a.combo_str()
            );
        }
    }

    // Image size unchanged; the touched player-file sector(s) stay EDC/ECC-valid.
    assert_eq!(patcher.image().len() % SECTOR_SIZE, 0);
    let lba = patcher.entry_disc_lba(index).unwrap() as usize;
    let footprint = patcher.entry_footprint(index).unwrap() as usize;
    let sectors = footprint.div_ceil(USER_DATA_SIZE);
    let img = patcher.image();
    for s in 0..sectors {
        let sb = (lba + s) * SECTOR_SIZE;
        assert!(
            legaia_iso::write::mode2_form1_sector_is_valid(&img[sb..sb + SECTOR_SIZE]),
            "player-file sector {s} must stay EDC/ECC-valid after the power edit"
        );
    }
}

#[test]
fn reapply_is_noop_and_absent_combo_refused() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open");
    let combo = parse_combo("LLRR").unwrap(); // Noa's Frost Breath

    let r1 = apply::set_arts_power(&mut patcher, &[(combo.clone(), 0x10)]).expect("apply");
    assert!(!r1.edits.is_empty());
    // Re-setting to the same value writes nothing.
    let r2 = apply::set_arts_power(&mut patcher, &[(combo, 0x10)]).expect("apply");
    assert!(r2.edits.is_empty(), "re-applying the same power is a no-op");

    // A combo no art uses is refused (typo / wrong-disc guard).
    let bogus = parse_combo("UUUUUUUUU").unwrap();
    assert!(apply::set_arts_power(&mut patcher, &[(bogus, 0x10)]).is_err());
}

#[test]
fn power_edit_is_deterministic() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let combo = parse_combo("RDLDL").unwrap();
    let mut a = DiscPatcher::open(disc.clone()).expect("open");
    let mut b = DiscPatcher::open(disc).expect("open");
    apply::set_arts_power(&mut a, &[(combo.clone(), 0x0C)]).unwrap();
    apply::set_arts_power(&mut b, &[(combo, 0x0C)]).unwrap();
    assert!(
        a.image() == b.image(),
        "same edit must reproduce the image byte-for-byte"
    );
}
