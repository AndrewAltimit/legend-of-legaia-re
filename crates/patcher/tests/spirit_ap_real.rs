//! Disc-gated tests for the **Spirit AP** tuning (see
//! `legaia_patcher::spirit_ap`): rewriting the battle-action state-`0x50`
//! Spirit accrual immediate (retail 32) plus the three state-`0x46`
//! gauge-widget ramp targets that mirror it.
//!
//! These apply the edit to a scratch copy of the real disc and assert, off
//! the patched image, that:
//!   * the baseline holds the stock immediates (`0x20`/`0x20`/`0x28`/`0x23`
//!     - the recognized US build) and reads back as 32 AP;
//!   * after patching, exactly the four site words changed in the overlay
//!     entry and nothing else anywhere in it;
//!   * the immediates land per the retail boost ratio (`n`, `n`, `n + n/4`,
//!     `n + n/10`) and the patched image still parses;
//!   * the edit is byte-deterministic, idempotent, and re-targetable (a
//!     patched image accepts a different value cleanly, and a round-trip
//!     back to 32 restores the stock words).
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. The patched image
//! lives only in memory.

use legaia_iso::iso9660::read_file_in_image;
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::spirit_ap::{
    BATTLE_ACTION_OVERLAY_PROT_INDEX, GRANT_VA, OVERLAY_BASE_VA, RETAIL_SPIRIT_AP, WIDGET_BASE_VA,
    WIDGET_BOOST1_VA, WIDGET_BOOST2_VA, current, plan,
};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn entry_word(entry: &[u8], va: u32) -> u32 {
    let off = (va - OVERLAY_BASE_VA) as usize;
    u32::from_le_bytes(entry[off..off + 4].try_into().unwrap())
}

const SITE_VAS: [u32; 4] = [GRANT_VA, WIDGET_BASE_VA, WIDGET_BOOST2_VA, WIDGET_BOOST1_VA];

#[test]
fn baseline_holds_the_stock_spirit_accrual() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let overlay = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read battle-action overlay");
    assert_eq!(
        entry_word(&overlay, GRANT_VA),
        0x2402_0020,
        "state-0x50 accrual is `addiu v0,zero,0x20`"
    );
    assert_eq!(entry_word(&overlay, WIDGET_BASE_VA), 0x24A2_0020);
    assert_eq!(entry_word(&overlay, WIDGET_BOOST2_VA), 0x24A2_0028);
    assert_eq!(entry_word(&overlay, WIDGET_BOOST1_VA), 0x24A2_0023);
    assert_eq!(
        current(&overlay).expect("recognized build"),
        RETAIL_SPIRIT_AP
    );
    // Planning retail against retail is a no-op.
    assert_eq!(plan(&overlay, RETAIL_SPIRIT_AP).unwrap(), None);
}

#[test]
fn edit_changes_exactly_the_four_site_words() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let before = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read overlay before");

    let report = apply::apply_spirit_ap(&mut patcher, 100).expect("apply spirit AP");
    assert!(report.changed);
    assert_eq!(report.previous, RETAIL_SPIRIT_AP);

    let after = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read overlay after");
    assert_eq!(entry_word(&after, GRANT_VA), 0x2402_0000 | 100);
    assert_eq!(entry_word(&after, WIDGET_BASE_VA), 0x24A2_0000 | 100);
    assert_eq!(entry_word(&after, WIDGET_BOOST2_VA), 0x24A2_0000 | 125);
    assert_eq!(entry_word(&after, WIDGET_BOOST1_VA), 0x24A2_0000 | 110);
    assert_eq!(current(&after).unwrap(), 100);

    // Surgical: every changed byte lies inside one of the four site words.
    let site_offs: Vec<usize> = SITE_VAS
        .iter()
        .map(|va| (va - OVERLAY_BASE_VA) as usize)
        .collect();
    let changed: Vec<usize> = before
        .iter()
        .zip(after.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    assert!(!changed.is_empty());
    for i in &changed {
        assert!(
            site_offs.iter().any(|off| (*off..*off + 4).contains(i)),
            "changed byte +{i:#x} lies outside the four site words"
        );
    }

    // The patched image still parses: a named-file read walks the ISO
    // structure over re-encoded sectors.
    read_file_in_image(patcher.image(), "SCUS_942.54")
        .expect("patched image re-reads (sectors stay valid)");
}

#[test]
fn edit_is_deterministic_idempotent_and_retargetable() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut a = DiscPatcher::open(disc.clone()).expect("open a");
    let mut b = DiscPatcher::open(disc.clone()).expect("open b");
    apply::apply_spirit_ap(&mut a, 0).unwrap();
    apply::apply_spirit_ap(&mut b, 0).unwrap();
    assert_eq!(
        a.image(),
        b.image(),
        "the spirit-AP edit yields a byte-identical patched image"
    );
    // Re-applying the held value is a no-op.
    let again = apply::apply_spirit_ap(&mut a, 0).expect("re-apply is accepted");
    assert!(!again.changed);
    assert_eq!(again.previous, 0);

    // Re-target the patched image, then round-trip back to retail: the
    // overlay entry returns to its stock bytes.
    let pristine = DiscPatcher::open(disc)
        .unwrap()
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .unwrap();
    let rep = apply::apply_spirit_ap(&mut a, 55).expect("retarget 0 -> 55");
    assert!(rep.changed);
    assert_eq!(rep.previous, 0);
    let rep = apply::apply_spirit_ap(&mut a, RETAIL_SPIRIT_AP).expect("restore retail");
    assert!(rep.changed);
    assert_eq!(rep.previous, 55);
    let restored = a.read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX).unwrap();
    assert_eq!(
        restored, pristine,
        "restoring 32 returns the overlay entry to stock bytes"
    );
}
