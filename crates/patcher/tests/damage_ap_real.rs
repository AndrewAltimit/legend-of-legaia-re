//! Disc-gated tests for the **enemy-damage AP** tuning (see
//! `legaia_patcher::damage_ap`): rewriting the battle damage finisher's
//! spirit-gauge fill scale (retail: 100 AP per 100% of max HP lost) and, for
//! a negative setting, its accrual tail and the two "spirit gain up" arms.
//!
//! These apply the edit to a scratch copy of the real disc and assert, off
//! the patched image, that:
//!   * the baseline holds the stock `x100` shift/add chain and reads back as
//!     100 - and, critically, that every word the negative form rewrites is
//!     the retail word this feature transcribed from the disassembly (the
//!     synthetic unit tests cannot check that, since they build their overlay
//!     from the same constants);
//!   * a positive setting rewrites the scale and nothing else;
//!   * `0` also drops retail's `max(pct, 1)` floor, so damage really grants
//!     no AP;
//!   * a negative setting flips the accrual to a floored subtract and
//!     neutralizes both bonus arms;
//!   * the edit is byte-deterministic, idempotent, and re-targetable across
//!     the sign, and a round-trip back to 100 restores the stock words.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. The patched image
//! lives only in memory.

use legaia_iso::iso9660::read_file_in_image;
use legaia_patcher::apply;
use legaia_patcher::damage_ap::{
    BATTLE_ACTION_OVERLAY_PROT_INDEX, OVERLAY_BASE_VA, RETAIL_DAMAGE_AP, current, plan,
};
use legaia_patcher::disc::DiscPatcher;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn entry_word(entry: &[u8], va: u32) -> u32 {
    let off = (va - OVERLAY_BASE_VA) as usize;
    u32::from_le_bytes(entry[off..off + 4].try_into().unwrap())
}

/// The `x100` scale chain: `d*2 + d = d*3`, `<<3 = d*24`, `+d = d*25`,
/// `<<2 = d*100`. `0x801DE1D8` in the middle is the max-HP load and is not a
/// site.
const SCALE: [(u32, u32); 5] = [
    (0x801D_E1C8, 0x0003_1040), // sll  v0,v1,0x1
    (0x801D_E1CC, 0x0043_1021), // addu v0,v0,v1
    (0x801D_E1D0, 0x0002_10C0), // sll  v0,v0,0x3
    (0x801D_E1D4, 0x0043_1021), // addu v0,v0,v1
    (0x801D_E1DC, 0x0002_1080), // sll  v0,v0,0x2
];

/// The min-1 floor (`a1 = max(pct, 1)`) and the words only a negative setting
/// rewrites: the two bonus-arm guards and the accrual + clamp tail.
const SIGNED_SITES: [(u32, u32); 9] = [
    (0x801D_E1F8, 0x2C45_0001), // sltiu a1,v0,0x1        (the min-1 floor)
    (0x801D_E248, 0x1040_0005), // beq   v0,zero,0x801de260 (gain up 2 guard)
    (0x801D_E290, 0x1040_0009), // beq   v0,zero,0x801de2b8 (gain up 1 guard)
    (0x801D_E2C0, 0x0045_1021), // addu  v0,v0,a1         (the accrual)
    (0x801D_E2C4, 0xA622_0170), // sh    v0,0x170(s1)
    (0x801D_E2C8, 0x3042_FFFF), // andi  v0,v0,0xffff
    (0x801D_E2CC, 0x2C42_0065), // sltiu v0,v0,0x65
    (0x801D_E2D0, 0x1440_0002), // bne   v0,zero,0x801de2dc
    (0x801D_E2D4, 0x2402_0064), // li    v0,0x64
];

const SCALE_ORI: u32 = 0x801D_E1C8;
const FLOOR_VA: u32 = 0x801D_E1F8;
const ACCRUAL_VA: u32 = 0x801D_E2C0;

fn owned_offsets() -> Vec<usize> {
    SCALE
        .iter()
        .chain(SIGNED_SITES.iter())
        .map(|(va, _)| (va - OVERLAY_BASE_VA) as usize)
        // 0x801DE2D8 is the last tail word; it is only rewritten by the drain.
        .chain(std::iter::once((0x801D_E2D8 - OVERLAY_BASE_VA) as usize))
        .collect()
}

fn assert_surgical(before: &[u8], after: &[u8]) {
    let owned = owned_offsets();
    for (i, (a, b)) in before.iter().zip(after.iter()).enumerate() {
        if a != b {
            assert!(
                owned.iter().any(|off| (*off..*off + 4).contains(&i)),
                "changed byte +{i:#x} lies outside the damage-AP site words"
            );
        }
    }
}

#[test]
fn baseline_holds_the_stock_damage_fill() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let overlay = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read battle-action overlay");
    for (va, want) in SCALE.iter().chain(SIGNED_SITES.iter()) {
        assert_eq!(
            entry_word(&overlay, *va),
            *want,
            "stock word at {va:#x} is not what the damage-AP rewrite expects"
        );
    }
    assert_eq!(
        current(&overlay).expect("recognized build"),
        RETAIL_DAMAGE_AP
    );
    // Planning retail against retail is a no-op.
    assert_eq!(plan(&overlay, RETAIL_DAMAGE_AP).unwrap(), None);
}

#[test]
fn positive_value_rewrites_the_scale_only() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let before = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read overlay before");

    let report = apply::apply_damage_ap(&mut patcher, 200).expect("apply damage AP");
    assert!(report.changed);
    assert_eq!(report.previous, RETAIL_DAMAGE_AP);

    let after = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read overlay after");
    assert_eq!(entry_word(&after, SCALE_ORI), 0x3402_0000 | 200);
    assert_eq!(current(&after).unwrap(), 200);
    // The accrual and the min-1 floor stay retail for any non-zero positive.
    assert_eq!(entry_word(&after, ACCRUAL_VA), SIGNED_SITES[3].1);
    assert_eq!(entry_word(&after, FLOOR_VA), SIGNED_SITES[0].1);
    assert_surgical(&before, &after);

    read_file_in_image(patcher.image(), "SCUS_942.54")
        .expect("patched image re-reads (sectors stay valid)");
}

#[test]
fn zero_removes_the_min_one_floor() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    apply::apply_damage_ap(&mut patcher, 0).expect("apply damage AP 0");
    let after = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read overlay after");
    assert_eq!(current(&after).unwrap(), 0);
    assert_eq!(entry_word(&after, SCALE_ORI), 0x3402_0000);
    assert_eq!(
        entry_word(&after, FLOOR_VA),
        0x0000_2821,
        "`move a1,zero` replaces retail's max(pct, 1)"
    );
}

#[test]
fn negative_value_flips_the_accrual_and_kills_the_bonus_arms() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let before = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read overlay before");

    let report = apply::apply_damage_ap(&mut patcher, -50).expect("apply drain");
    assert!(report.changed);
    assert_eq!(report.previous, RETAIL_DAMAGE_AP);

    let after = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read overlay after");
    assert_eq!(current(&after).unwrap(), -50);
    assert_eq!(
        entry_word(&after, SCALE_ORI),
        0x3402_0000 | 50,
        "the magnitude drives the scale"
    );
    assert_eq!(
        entry_word(&after, ACCRUAL_VA),
        0x0045_1023,
        "`subu v0,v0,a1` replaces the accrual"
    );
    assert_eq!(entry_word(&after, 0x801D_E2C4), 0x0441_0002, "bgez floor");
    assert_eq!(entry_word(&after, 0x801D_E2CC), 0x0000_1021, "move v0,zero");
    assert_eq!(entry_word(&after, 0x801D_E2D0), 0xA622_0170, "the store");
    // Both "spirit gain up" guards become unconditional (`beq zero,zero`).
    assert_eq!(entry_word(&after, 0x801D_E248), 0x1000_0005);
    assert_eq!(entry_word(&after, 0x801D_E290), 0x1000_0009);
    assert_surgical(&before, &after);

    read_file_in_image(patcher.image(), "SCUS_942.54")
        .expect("patched image re-reads (sectors stay valid)");
}

#[test]
fn edit_is_deterministic_idempotent_and_retargetable_across_the_sign() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut a = DiscPatcher::open(disc.clone()).expect("open a");
    let mut b = DiscPatcher::open(disc.clone()).expect("open b");
    apply::apply_damage_ap(&mut a, -75).unwrap();
    apply::apply_damage_ap(&mut b, -75).unwrap();
    assert_eq!(
        a.image(),
        b.image(),
        "the damage-AP edit yields a byte-identical patched image"
    );
    let again = apply::apply_damage_ap(&mut a, -75).expect("re-apply is accepted");
    assert!(!again.changed);
    assert_eq!(again.previous, -75);

    let pristine = DiscPatcher::open(disc)
        .unwrap()
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .unwrap();
    let rep = apply::apply_damage_ap(&mut a, 25).expect("retarget -75 -> 25");
    assert!(rep.changed);
    assert_eq!(rep.previous, -75);
    let rep = apply::apply_damage_ap(&mut a, RETAIL_DAMAGE_AP).expect("restore retail");
    assert!(rep.changed);
    assert_eq!(rep.previous, 25);
    assert_eq!(
        a.read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX).unwrap(),
        pristine,
        "restoring 100 returns the overlay entry to stock bytes"
    );
}
