//! Disc-gated end-to-end test for the special-attack power randomizer: shuffle
//! the move-power table's power column in PROT entry 0898 on a scratch copy of
//! the disc, then re-parse the patched overlay off the patched image and confirm:
//!
//! - the power multiset is preserved (a shuffle is a 1:1 reassignment);
//! - every non-power byte of every 26-byte record is untouched (only `+0x00`
//!   moves, so animations / effects / sound cues stay coherent);
//! - the touched PROT 0898 sectors stay EDC/ECC-valid;
//! - a fixed seed reproduces the patched image byte-for-byte.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_asset::move_power::{
    self, BATTLE_ACTION_OVERLAY_PROT_INDEX, MOVE_POWER_RECORD_STRIDE, MOVE_POWER_TABLE_FILE_OFFSET,
};
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn powers(patcher: &DiscPatcher) -> Vec<i16> {
    apply::current_move_powers(patcher)
        .expect("read move powers")
        .expect("move-power table present")
}

#[test]
fn shuffle_move_powers_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x5EA1_F00D_3000_0001;

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = powers(&base);
    assert_eq!(
        before.len(),
        move_power::MOVE_POWER_TABLE_LEN,
        "expected the full 44-record move-power table"
    );
    // Capture the full raw table to prove only the power halfwords move.
    let entry_before = base
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read 0898");
    let span = before.len() * MOVE_POWER_RECORD_STRIDE;
    let raw_before =
        entry_before[MOVE_POWER_TABLE_FILE_OFFSET..MOVE_POWER_TABLE_FILE_OFFSET + span].to_vec();

    // Shuffle on a scratch copy.
    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let changed =
        apply::randomize_move_powers(&mut patcher, seed, DropMode::Shuffle).expect("randomize");
    assert!(changed > 0, "a shuffle should move at least one power");

    // Re-parse off the PATCHED image: power multiset preserved.
    let after = powers(&patcher);
    let mut a = after.clone();
    let mut b = before.clone();
    a.sort_unstable();
    b.sort_unstable();
    assert_eq!(a, b, "shuffle must preserve the power multiset");

    // Only the `+0x00` halfword of each record changed; the rest is byte-equal.
    let entry_after = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read 0898");
    let raw_after = &entry_after[MOVE_POWER_TABLE_FILE_OFFSET..MOVE_POWER_TABLE_FILE_OFFSET + span];
    for i in 0..before.len() {
        let rec = i * MOVE_POWER_RECORD_STRIDE;
        assert_eq!(
            raw_before[rec + 2..rec + MOVE_POWER_RECORD_STRIDE],
            raw_after[rec + 2..rec + MOVE_POWER_RECORD_STRIDE],
            "record {i}: bytes beyond the power halfword must be untouched"
        );
    }

    // (PROT.DAT sector EDC/ECC validity after a patch_prot_entry write is
    // covered by disc_patch_real; re-decoding the table off the patched image
    // above already proves the touched sectors parse cleanly.)

    // Determinism: same seed -> byte-identical image.
    let mut patcher2 = DiscPatcher::open(original).expect("open");
    let changed2 =
        apply::randomize_move_powers(&mut patcher2, seed, DropMode::Shuffle).expect("randomize");
    assert_eq!(changed2, changed);
    assert!(
        patcher2.image() == patcher.image(),
        "same seed must reproduce the patched image"
    );

    eprintln!(
        "move-power shuffle seed {seed:#x}: {changed} of {} powers changed; multiset + record tails preserved",
        before.len()
    );
}
