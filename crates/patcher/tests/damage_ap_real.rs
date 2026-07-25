//! Disc-gated tests for the **enemy-damage AP** tuning (see
//! `legaia_patcher::damage_ap`): rewriting the spirit-gauge fill scale
//! (retail: 100 AP per 100% of max HP lost) and, for a negative setting, the
//! accrual tail and the two "spirit gain up" arms - in **both** copies of the
//! kernel, `FUN_801DDB30` (magic / summon / special hits) and `FUN_801EC3E4`
//! (ordinary physical hits). Patching only the first leaves regular enemy
//! swings running stock, which is indistinguishable from the slider doing
//! nothing, so the two-copy coverage is what these tests exist to pin.
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

/// Every word either copy rewrites, with the retail word it replaces.
/// Independently transcribed here from the disassembly so the disc - not the
/// module's own constants - is what confirms the placement.
///
/// Copy A is `FUN_801DDB30` (damage in `v1`, defender in `s1`, pct in `a1`);
/// copy B is `FUN_801EC3E4` (damage in `a0`, defender in `a1`, pct in `a2`).
const SITES: [(u32, u32); 31] = [
    // ---- copy A: the x100 shift/add chain (0x801DE1D8 between them is the
    // max-HP load and is not a site) ----
    (0x801D_E1C8, 0x0003_1040), // sll   v0,v1,0x1
    (0x801D_E1CC, 0x0043_1021), // addu  v0,v0,v1
    (0x801D_E1D0, 0x0002_10C0), // sll   v0,v0,0x3
    (0x801D_E1D4, 0x0043_1021), // addu  v0,v0,v1
    (0x801D_E1DC, 0x0002_1080), // sll   v0,v0,0x2
    // copy A: min-1 floor, bonus-arm guards, accrual + clamp tail
    (0x801D_E1F8, 0x2C45_0001), // sltiu a1,v0,0x1
    (0x801D_E248, 0x1040_0005), // beq   v0,zero,0x801de260
    (0x801D_E290, 0x1040_0009), // beq   v0,zero,0x801de2b8
    (0x801D_E2C0, 0x0045_1021), // addu  v0,v0,a1
    (0x801D_E2C4, 0xA622_0170), // sh    v0,0x170(s1)
    (0x801D_E2C8, 0x3042_FFFF), // andi  v0,v0,0xffff
    (0x801D_E2CC, 0x2C42_0065), // sltiu v0,v0,0x65
    (0x801D_E2D0, 0x1440_0002), // bne   v0,zero,0x801de2dc
    (0x801D_E2D4, 0x2402_0064), // li    v0,0x64
    (0x801D_E2D8, 0xA622_0170), // sh    v0,0x170(s1)
    // ---- copy B: the same chain, first word in a branch delay slot ----
    (0x801E_DB78, 0x0004_1040), // sll   v0,a0,0x1   (delay slot)
    (0x801E_DB80, 0x0044_1021), // addu  v0,v0,a0    (branch join)
    (0x801E_DB84, 0x0002_10C0), // sll   v0,v0,0x3
    (0x801E_DB8C, 0x0044_1021), // addu  v0,v0,a0
    (0x801E_DB94, 0x0002_1080), // sll   v0,v0,0x2
    // copy B: min-1 floor, bonus-arm guards, accrual + clamp tail
    (0x801E_DBB0, 0x2C43_0001), // sltiu v1,v0,0x1
    (0x801E_DC00, 0x1040_0005), // beq   v0,zero,0x801edc18
    (0x801E_DC48, 0x1040_000C), // beq   v0,zero,0x801edc7c
    (0x801E_DCA0, 0x0043_1021), // addu  v0,v0,v1
    (0x801E_DCA4, 0xA4A2_0170), // sh    v0,0x170(a1)
    (0x801E_DCA8, 0x8C84_0000), // lw    a0,0x0(a0)
    (0x801E_DCAC, 0x0000_0000), // nop
    (0x801E_DCB0, 0x9482_0170), // lhu   v0,0x170(a0)
    (0x801E_DCB4, 0x0000_0000), // nop
    (0x801E_DCB8, 0x2C42_0065), // sltiu v0,v0,0x65
    (0x801E_DCBC, 0x1440_0004), // bne   v0,zero,0x801edcd0
];

/// The two copies' scale-factor words, min-1 floors and accrual heads.
const SCALE_ORI: [u32; 2] = [0x801D_E1C8, 0x801E_DB78];
const FLOOR_VA: [u32; 2] = [0x801D_E1F8, 0x801E_DBB0];
const ACCRUAL_VA: [u32; 2] = [0x801D_E2C0, 0x801E_DCA0];
const FLOOR_NONE: [u32; 2] = [0x0000_2821, 0x0000_1821];
const ACCRUAL_DRAIN: [u32; 2] = [0x0045_1023, 0x0043_1023];

fn owned_offsets() -> Vec<usize> {
    SITES
        .iter()
        .map(|(va, _)| (va - OVERLAY_BASE_VA) as usize)
        // Copy B's tail runs two words past its last stock-checked word; both
        // are only rewritten by the drain.
        .chain([0x801E_DCC4, 0x801E_DCC8].map(|va| (va - OVERLAY_BASE_VA) as usize))
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
    for (va, want) in SITES {
        assert_eq!(
            entry_word(&overlay, va),
            want,
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
    assert_eq!(current(&after).unwrap(), 200);
    for i in 0..2 {
        assert_eq!(
            entry_word(&after, SCALE_ORI[i]),
            0x3402_0000 | 200,
            "copy {i} scale"
        );
        // The accrual and the min-1 floor stay retail for a non-zero positive.
        let stock = |va: u32| SITES.iter().find(|(v, _)| *v == va).unwrap().1;
        assert_eq!(entry_word(&after, ACCRUAL_VA[i]), stock(ACCRUAL_VA[i]));
        assert_eq!(entry_word(&after, FLOOR_VA[i]), stock(FLOOR_VA[i]));
    }
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
    for i in 0..2 {
        assert_eq!(entry_word(&after, SCALE_ORI[i]), 0x3402_0000);
        assert_eq!(
            entry_word(&after, FLOOR_VA[i]),
            FLOOR_NONE[i],
            "copy {i}: a `move rX,zero` replaces retail's max(pct, 1)"
        );
    }
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
    for i in 0..2 {
        assert_eq!(
            entry_word(&after, SCALE_ORI[i]),
            0x3402_0000 | 50,
            "copy {i}: the magnitude drives the scale"
        );
        assert_eq!(
            entry_word(&after, ACCRUAL_VA[i]),
            ACCRUAL_DRAIN[i],
            "copy {i}: a `subu` replaces the accrual"
        );
    }
    // Copy A's floored store.
    assert_eq!(entry_word(&after, 0x801D_E2C4), 0x0441_0002, "A bgez floor");
    assert_eq!(
        entry_word(&after, 0x801D_E2CC),
        0x0000_1021,
        "A move v0,zero"
    );
    assert_eq!(entry_word(&after, 0x801D_E2D0), 0xA622_0170, "A the store");
    // Copy B's floored store.
    assert_eq!(entry_word(&after, 0x801E_DCA4), 0x0441_0002, "B bgez floor");
    assert_eq!(
        entry_word(&after, 0x801E_DCAC),
        0x0000_1021,
        "B move v0,zero"
    );
    assert_eq!(entry_word(&after, 0x801E_DCB0), 0xA4A2_0170, "B the store");
    // All four "spirit gain up" guards become unconditional (`beq zero,zero`).
    assert_eq!(entry_word(&after, 0x801D_E248), 0x1000_0005);
    assert_eq!(entry_word(&after, 0x801D_E290), 0x1000_0009);
    assert_eq!(entry_word(&after, 0x801E_DC00), 0x1000_0005);
    assert_eq!(entry_word(&after, 0x801E_DC48), 0x1000_000C);
    // The word the tail leaves live for the join at 0x801EDCD0 survives.
    assert_eq!(
        entry_word(&after, 0x801E_DCC0),
        0x3C02_8008,
        "lui v0,0x8008"
    );
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
