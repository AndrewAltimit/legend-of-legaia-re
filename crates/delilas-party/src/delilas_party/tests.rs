use super::*;
use legaia_art::queue::Character;

#[test]
fn move_mode_round_trips_and_defaults_to_hybrid() {
    assert_eq!(DelilasMoveMode::default(), DelilasMoveMode::Hybrid);
    for m in [DelilasMoveMode::Hybrid, DelilasMoveMode::Delilas] {
        assert_eq!(m.to_string().parse::<DelilasMoveMode>().unwrap(), m);
    }
    assert_eq!(
        "  DELILAS ".parse::<DelilasMoveMode>().unwrap(),
        DelilasMoveMode::Delilas
    );
    assert!("purist".parse::<DelilasMoveMode>().is_err());
}

/// The Super trigger table is static, so the row set each character's
/// Supers depend on is too - and it is the thing a blank would cost.
#[test]
fn super_critical_rows_come_from_the_trigger_table() {
    // Vahn's Tri-Somersault chains arts 0x27, 0x1F, 0x27, so rows
    // 0x17 and 0x0F must both be in the set.
    let vahn = super_critical_rows(Character::Vahn);
    assert!(vahn.contains(&0x17) && vahn.contains(&0x0F));
    assert_eq!(vahn.len(), 8);
    assert_eq!(super_critical_rows(Character::Noa).len(), 10);
    assert_eq!(super_critical_rows(Character::Gala).len(), 8);
    // Every row is a real bank row above the matcher's start.
    for ch in [Character::Vahn, Character::Noa, Character::Gala] {
        for row in super_critical_rows(ch) {
            assert!(row > MIRACLE_BANK_ROW, "{ch:?}: row {row}");
        }
    }
}

#[test]
fn retained_rows_hold_the_miracle_the_host_and_the_innate_block() {
    // Vahn's shape on the USA disc: 33 bank records, innate cap 3,
    // the signature hosted on row 12.
    let keep = retained_bank_rows(Character::Vahn, 3, 12, 33);
    assert!(keep.contains(&MIRACLE_BANK_ROW), "the Miracle row");
    assert!(keep.contains(&12), "the signature host");
    // Ids 1..=3 are the script-granted Hyper block.
    for row in 12..=14 {
        assert!(keep.contains(&row), "innate row {row}");
    }
    // Rows 16 / 19 / 20 are ordinary arts no Super names.
    for row in [16, 19, 20] {
        assert!(!keep.contains(&row), "row {row} should be hidden");
    }
    assert!(keep.iter().all(|&r| r < 33), "no row past the bank");
    // A cap of 0 keeps only the Miracle, the host and the Supers.
    let tight = retained_bank_rows(Character::Vahn, 0, 12, 33);
    assert!(tight.len() < keep.len());
    assert!(tight.contains(&MIRACLE_BANK_ROW) && tight.contains(&12));
}

#[test]
fn every_sibling_has_a_label_per_swing_clip() {
    // The archive carries at most four swings per sibling, and the
    // menu field they are written into is seven bytes at its
    // tightest.
    for sib in [Sibling::Gi, Sibling::Che, Sibling::Lu] {
        let labels = swing_labels(sib);
        assert!(labels.len() >= 4, "{sib:?}: only {} label(s)", labels.len());
        for l in labels {
            assert!(
                l.len() <= LABEL_MAX,
                "{sib:?}: {l:?} will not fit a {LABEL_MAX}-byte field"
            );
            assert!(l.starts_with(sib.display_name()), "{sib:?}: {l:?}");
        }
    }
}
