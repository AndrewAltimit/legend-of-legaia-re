//! Synthetic-host tests for the battle-action validator port
//! (`FUN_8003FB10` -> `battle_action::validate_action`). Exercises every
//! dispatch arm's gate branches and the per-arm validity-bitmask write
//! discipline (clear-then-set / whole-byte overwrite / untouched).
//!
//! REF: FUN_8003FB10

use legaia_engine_vm::battle_action::{
    ActionValidatorHost, RecordStat, SlotResources, item_count_gate, validate_action,
};
use std::collections::HashMap;

/// Synthetic validator host: fixed slot quads, per-slot status words,
/// per-(slot, stat) record stats, and scripted flag gates.
#[derive(Default)]
struct Host {
    in_battle: bool,
    slots: Vec<SlotResources>,
    status: HashMap<u8, u16>,
    stats: HashMap<(u8, u8), u16>,
    party: Vec<u8>,
    engine_flags: u32,
    system_flags: Vec<u16>,
    inventory_count: i32,
}

fn stat_key(stat: RecordStat) -> u8 {
    match stat {
        RecordStat::HpMax => 0,
        RecordStat::MpMax => 1,
        RecordStat::Agl => 2,
        RecordStat::Atk => 3,
        RecordStat::Udf => 4,
        RecordStat::Ldf => 5,
        RecordStat::Spd => 6,
        RecordStat::Int => 7,
    }
}

impl Host {
    /// Every record stat pinned at its cap (nothing raisable) unless a
    /// test overrides a specific `(slot, stat)`.
    fn with_capped_stats(mut self, slot: u8) -> Self {
        for (stat, cap) in [
            (RecordStat::HpMax, 9999),
            (RecordStat::MpMax, 999),
            (RecordStat::Agl, 0x118),
            (RecordStat::Atk, 999),
            (RecordStat::Udf, 999),
            (RecordStat::Ldf, 999),
            (RecordStat::Spd, 999),
            (RecordStat::Int, 999),
        ] {
            self.stats.insert((slot, stat_key(stat)), cap);
        }
        self
    }
    fn set_stat(&mut self, slot: u8, stat: RecordStat, v: u16) {
        self.stats.insert((slot, stat_key(stat)), v);
    }
}

impl ActionValidatorHost for Host {
    fn in_battle(&self) -> bool {
        self.in_battle
    }
    fn slot_resources(&self, slot: u8) -> Option<SlotResources> {
        self.slots.get(slot as usize).copied()
    }
    fn status_word(&self, slot: u8) -> u16 {
        self.status.get(&slot).copied().unwrap_or(0)
    }
    fn record_stat(&self, slot: u8, stat: RecordStat) -> u16 {
        self.stats
            .get(&(slot, stat_key(stat)))
            .copied()
            .unwrap_or(0)
    }
    fn party_count(&self) -> u8 {
        self.party.len() as u8
    }
    fn party_member_slot(&self, index: u8) -> u8 {
        self.party.get(index as usize).copied().unwrap_or(0)
    }
    fn engine_flag_word(&self) -> u32 {
        self.engine_flags
    }
    fn system_flag(&self, idx: u16) -> bool {
        self.system_flags.contains(&idx)
    }
    fn inventory_count(&self) -> i32 {
        self.inventory_count
    }
}

fn quad(hp: u16, hp_max: u16, mp: u16, mp_max: u16) -> SlotResources {
    SlotResources {
        hp,
        hp_max,
        mp,
        mp_max,
    }
}

// ---- arm 0x00: heal target (alive AND hp < hp_max) ----

#[test]
fn arm0_injured_alive_slot_is_valid_and_sets_bit() {
    let mut h = Host {
        slots: vec![quad(50, 100, 0, 0)],
        ..Default::default()
    };
    let mut bits = 0u8;
    assert!(validate_action(&mut h, 0x00, 0, 0, &mut bits));
    assert_eq!(bits, 0b1);
}

#[test]
fn arm0_dead_slot_is_invalid_and_clears_prior_bit() {
    let mut h = Host {
        slots: vec![quad(0, 100, 0, 0)],
        ..Default::default()
    };
    let mut bits = 0b1;
    assert!(!validate_action(&mut h, 0x00, 0, 0, &mut bits));
    assert_eq!(bits, 0, "arm 0 clears the slot bit before testing");
}

#[test]
fn arm0_full_hp_slot_is_invalid() {
    let mut h = Host {
        slots: vec![quad(100, 100, 0, 0)],
        ..Default::default()
    };
    let mut bits = 0b1;
    assert!(!validate_action(&mut h, 0x00, 0, 0, &mut bits));
    assert_eq!(bits, 0);
}

// ---- arm 0x01: party walk ----

#[test]
fn arm1_sets_one_bit_per_injured_party_member() {
    let mut h = Host {
        slots: vec![
            quad(10, 100, 0, 0),
            quad(100, 100, 0, 0),
            quad(30, 100, 0, 0),
        ],
        party: vec![0, 1, 2],
        ..Default::default()
    };
    let mut bits = 0xFF;
    assert!(validate_action(&mut h, 0x01, 0, 0, &mut bits));
    // Byte zeroed first, then bits 0 and 2 set (slot 1 is at full HP).
    assert_eq!(bits, 0b101);
}

#[test]
fn arm1_empty_party_zeroes_byte_and_returns_invalid() {
    let mut h = Host::default();
    let mut bits = 0xFF;
    assert!(!validate_action(&mut h, 0x01, 0, 0, &mut bits));
    assert_eq!(bits, 0);
}

#[test]
fn arm1_walks_via_party_member_slot_indirection() {
    // Party of one member sitting in slot 2 (the DAT_80084598 indirection).
    let mut h = Host {
        slots: vec![
            quad(100, 100, 0, 0),
            quad(100, 100, 0, 0),
            quad(1, 100, 0, 0),
        ],
        party: vec![2],
        ..Default::default()
    };
    let mut bits = 0;
    assert!(validate_action(&mut h, 0x01, 0, 0, &mut bits));
    assert_eq!(bits, 0b100);
}

// ---- arm 0x02: MP-restore target ----

#[test]
fn arm2_alive_with_missing_mp_is_valid() {
    let mut h = Host {
        slots: vec![quad(1, 1, 5, 30)],
        ..Default::default()
    };
    let mut bits = 0;
    assert!(validate_action(&mut h, 0x02, 0, 0, &mut bits));
    assert_eq!(bits, 0b1);
}

#[test]
fn arm2_full_mp_or_dead_is_invalid() {
    let mut h = Host {
        slots: vec![quad(1, 1, 30, 30), quad(0, 1, 5, 30)],
        ..Default::default()
    };
    let mut bits = 0b11;
    assert!(!validate_action(&mut h, 0x02, 0, 0, &mut bits));
    assert!(!validate_action(&mut h, 0x02, 0, 1, &mut bits));
    assert_eq!(bits, 0, "both slot bits cleared");
}

// ---- arm 0x03: status presence, mode-split bit discipline ----

#[test]
fn arm3_battle_branch_returns_status_without_touching_bits() {
    let mut h = Host {
        in_battle: true,
        slots: vec![quad(1, 1, 0, 0)],
        status: [(0u8, 0x40u16)].into_iter().collect(),
        ..Default::default()
    };
    let mut bits = 0b1010;
    assert!(validate_action(&mut h, 0x03, 0, 0, &mut bits));
    assert_eq!(bits, 0b1010, "battle arm 3 never writes the bit byte");
    h.status.insert(0, 0);
    assert!(!validate_action(&mut h, 0x03, 0, 0, &mut bits));
    assert_eq!(bits, 0b1010);
}

#[test]
fn arm3_field_branch_clears_then_sets_bit() {
    let mut h = Host {
        slots: vec![quad(1, 1, 0, 0)],
        status: [(0u8, 0x40u16)].into_iter().collect(),
        ..Default::default()
    };
    let mut bits = 0;
    assert!(validate_action(&mut h, 0x03, 0, 0, &mut bits));
    assert_eq!(bits, 0b1);
    h.status.insert(0, 0);
    assert!(!validate_action(&mut h, 0x03, 0, 0, &mut bits));
    assert_eq!(bits, 0);
}

// ---- arm 0x04: dead target (revive) ----

#[test]
fn arm4_dead_slot_is_valid_alive_is_not() {
    let mut h = Host {
        slots: vec![quad(0, 100, 0, 0), quad(1, 100, 0, 0)],
        ..Default::default()
    };
    let mut bits = 0;
    assert!(validate_action(&mut h, 0x04, 0, 0, &mut bits));
    assert_eq!(bits, 0b1);
    assert!(!validate_action(&mut h, 0x04, 0, 1, &mut bits));
    assert_eq!(bits, 0b1, "slot 1 bit cleared (was clear), slot 0 kept");
}

// ---- arms 0x05 / 0x07: alive ----

#[test]
fn arm5_and_arm7_validate_alive_slots() {
    for arm in [0x05u8, 0x07] {
        let mut h = Host {
            slots: vec![quad(1, 1, 0, 0), quad(0, 1, 0, 0)],
            ..Default::default()
        };
        let mut bits = 0b10;
        assert!(validate_action(&mut h, arm, 0, 0, &mut bits));
        assert!(!validate_action(&mut h, arm, 0, 1, &mut bits));
        assert_eq!(bits, 0b01, "arm {arm:#x}: alive bit set, dead bit cleared");
    }
}

// ---- arm 0x06: stat-cap walker ----

#[test]
fn arm6_dead_slot_is_invalid_regardless_of_stats() {
    let mut h = Host {
        slots: vec![quad(0, 1, 0, 0)],
        ..Default::default()
    };
    // Default stats are 0 (all below cap) - liveness still gates.
    let mut bits = 0b1;
    assert!(!validate_action(&mut h, 0x06, 0, 0, &mut bits));
    assert_eq!(bits, 0);
}

#[test]
fn arm6_subcase_stat_below_cap_boundaries() {
    // (sub_case, stat, cap) triples for the single-stat sub-cases.
    let cases = [
        (0u8, RecordStat::HpMax, 9999u16),
        (1, RecordStat::Atk, 999),
        (3, RecordStat::Spd, 999),
        (4, RecordStat::Int, 999),
        (5, RecordStat::MpMax, 999),
    ];
    for (sub, stat, cap) in cases {
        let mut h = Host {
            slots: vec![quad(1, 1, 0, 0)],
            ..Default::default()
        }
        .with_capped_stats(0);
        let mut bits = 0;
        // At the cap: invalid (retail is a strict `<` compare).
        assert!(
            !validate_action(&mut h, 0x06, sub, 0, &mut bits),
            "sub {sub}: at-cap stat must be invalid"
        );
        assert_eq!(bits, 0);
        // One below the cap: valid.
        h.set_stat(0, stat, cap - 1);
        assert!(
            validate_action(&mut h, 0x06, sub, 0, &mut bits),
            "sub {sub}: below-cap stat must be valid"
        );
        assert_eq!(bits, 0b1);
    }
}

#[test]
fn arm6_subcase2_is_the_udf_ldf_pair() {
    let mut h = Host {
        slots: vec![quad(1, 1, 0, 0)],
        ..Default::default()
    }
    .with_capped_stats(0);
    let mut bits = 0;
    assert!(!validate_action(&mut h, 0x06, 2, 0, &mut bits));
    // Either half of the pair below cap validates.
    h.set_stat(0, RecordStat::Ldf, 998);
    assert!(validate_action(&mut h, 0x06, 2, 0, &mut bits));
    h.set_stat(0, RecordStat::Ldf, 999);
    h.set_stat(0, RecordStat::Udf, 998);
    assert!(validate_action(&mut h, 0x06, 2, 0, &mut bits));
}

#[test]
fn arm6_subcase6_checks_every_stat_including_agl() {
    let mut h = Host {
        slots: vec![quad(1, 1, 0, 0)],
        ..Default::default()
    }
    .with_capped_stats(0);
    let mut bits = 0;
    assert!(
        !validate_action(&mut h, 0x06, 6, 0, &mut bits),
        "everything at cap: nothing raisable"
    );
    // AGL is only tested by sub-case 6 (cap 0x118 = 280).
    h.set_stat(0, RecordStat::Agl, 0x117);
    assert!(validate_action(&mut h, 0x06, 6, 0, &mut bits));
    assert_eq!(bits, 0b1);
}

#[test]
fn arm6_unknown_subcase_is_invalid() {
    let mut h = Host {
        slots: vec![quad(1, 1, 0, 0)],
        ..Default::default()
    };
    let mut bits = 0b1;
    assert!(!validate_action(&mut h, 0x06, 7, 0, &mut bits));
    assert_eq!(bits, 0, "bit still cleared by the arm-6 entry");
}

// ---- arm 0x08: status bits 0-1 ----

#[test]
fn arm8_dead_slot_leaves_bits_untouched() {
    let mut h = Host {
        slots: vec![quad(0, 1, 0, 0)],
        ..Default::default()
    };
    let mut bits = 0b101;
    assert!(!validate_action(&mut h, 0x08, 0, 0, &mut bits));
    assert_eq!(bits, 0b101, "dead early-out precedes every bit write");
}

#[test]
fn arm8_battle_masks_status_with_3_and_skips_bit_write() {
    let mut h = Host {
        in_battle: true,
        slots: vec![quad(1, 1, 0, 0)],
        status: [(0u8, 0x0002u16)].into_iter().collect(),
        ..Default::default()
    };
    let mut bits = 0b100;
    assert!(validate_action(&mut h, 0x08, 0, 0, &mut bits));
    assert_eq!(bits, 0b100);
    h.status.insert(0, 0x0004); // bit outside the & 3 mask
    assert!(!validate_action(&mut h, 0x08, 0, 0, &mut bits));
}

#[test]
fn arm8_field_keeps_the_sign_extension_quirk() {
    let mut h = Host {
        slots: vec![quad(1, 1, 0, 0)],
        ..Default::default()
    };
    let mut bits = 0;
    // Bits 0-1 validate.
    h.status.insert(0, 0x0001);
    assert!(validate_action(&mut h, 0x08, 0, 0, &mut bits));
    assert_eq!(bits, 0b1);
    // Bit 15 set: the retail `lh` sign-extends, so `& 0xFFFF0003` is
    // non-zero even with bits 0-1 clear.
    h.status.insert(0, 0x8000);
    assert!(validate_action(&mut h, 0x08, 0, 0, &mut bits));
    // Any other status bit alone does not validate.
    h.status.insert(0, 0x0040);
    assert!(!validate_action(&mut h, 0x08, 0, 0, &mut bits));
    assert_eq!(bits, 0);
}

// ---- arms 0x09 / 0x0A: force-valid ----

#[test]
fn arm9_and_arm_a_overwrite_byte_with_7() {
    for arm in [0x09u8, 0x0A] {
        let mut h = Host::default();
        let mut bits = 0xF0;
        assert!(validate_action(&mut h, arm, 0, 0, &mut bits));
        assert_eq!(bits, 7, "arm {arm:#x} forces the whole byte to 7");
    }
}

// ---- arms 0x0B..=0x0D: exact-slot match ----

#[test]
fn arm_b_to_d_match_only_their_own_slot() {
    for arm in [0x0Bu8, 0x0C, 0x0D] {
        let expect_slot = arm - 0x0B;
        for slot in 0..3u8 {
            let mut h = Host::default();
            let mut bits = 0xFF;
            let ok = validate_action(&mut h, arm, 0, slot, &mut bits);
            if slot == expect_slot {
                assert!(ok);
                assert_eq!(bits, 1 << slot, "whole byte becomes the slot mask");
            } else {
                assert!(!ok);
                assert_eq!(bits, 0xFF, "mismatch leaves the byte untouched");
            }
        }
    }
}

// ---- arms 0x80 / 0x81: out-of-battle flag gates ----

#[test]
fn arm80_gates_on_mode_engine_word_and_system_flag() {
    // In battle: invalid.
    let mut h = Host {
        in_battle: true,
        ..Default::default()
    };
    let mut bits = 0;
    assert!(!validate_action(&mut h, 0x80, 0, 0, &mut bits));
    // Out of battle, all clear: valid.
    h.in_battle = false;
    assert!(validate_action(&mut h, 0x80, 0, 0, &mut bits));
    // Engine flag-word bit 0x100000 blocks.
    h.engine_flags = 0x0010_0000;
    assert!(!validate_action(&mut h, 0x80, 0, 0, &mut bits));
    // System flag 5 blocks.
    h.engine_flags = 0;
    h.system_flags = vec![5];
    assert!(!validate_action(&mut h, 0x80, 0, 0, &mut bits));
    // Flag 6 is arm 0x81's flag, not arm 0x80's.
    h.system_flags = vec![6];
    assert!(validate_action(&mut h, 0x80, 0, 0, &mut bits));
}

#[test]
fn arm81_uses_bit_200000_and_flag_6() {
    let mut h = Host::default();
    let mut bits = 0;
    assert!(validate_action(&mut h, 0x81, 0, 0, &mut bits));
    h.engine_flags = 0x0020_0000;
    assert!(!validate_action(&mut h, 0x81, 0, 0, &mut bits));
    h.engine_flags = 0x0010_0000; // arm 0x80's bit - ignored here
    assert!(validate_action(&mut h, 0x81, 0, 0, &mut bits));
    h.system_flags = vec![6];
    assert!(!validate_action(&mut h, 0x81, 0, 0, &mut bits));
}

// ---- arm 0x82: external out-of-battle gate ----

#[test]
fn arm82_runs_the_inventory_count_gate_out_of_battle_only() {
    let mut h = Host::default(); // count 0 - room in the bag
    let mut bits = 0;
    assert!(validate_action(&mut h, 0x82, 0, 0, &mut bits));
    // At the 0xE0 (224) cap: no room.
    h.inventory_count = 0xE0;
    assert!(!validate_action(&mut h, 0x82, 0, 0, &mut bits));
    // One below the cap: room (strict signed `<`).
    h.inventory_count = 0xDF;
    assert!(validate_action(&mut h, 0x82, 0, 0, &mut bits));
    // In battle the arm is invalid regardless of count.
    h.in_battle = true;
    assert!(!validate_action(&mut h, 0x82, 0, 0, &mut bits));
}

#[test]
fn item_count_gate_matches_the_leaf_compare() {
    assert!(item_count_gate(0));
    assert!(item_count_gate(0xDF));
    assert!(!item_count_gate(0xE0));
    assert!(!item_count_gate(0x100));
    // Signed compare: a negative word passes (retail `slti`).
    assert!(item_count_gate(-1));
}

// ---- arm 0x83 + unhandled arms ----

#[test]
fn arm83_is_always_valid_and_unhandled_arms_are_not() {
    let mut h = Host::default();
    let mut bits = 0xAA;
    assert!(validate_action(&mut h, 0x83, 0, 0, &mut bits));
    assert_eq!(bits, 0xAA, "arm 0x83 never writes the bit byte");
    for arm in [0x0Eu8, 0x40, 0x7F, 0x84, 0xFF] {
        assert!(
            !validate_action(&mut h, arm, 0, 0, &mut bits),
            "arm {arm:#x} is an unhandled jump-table slot"
        );
        assert_eq!(bits, 0xAA);
    }
}

// ---- slot-mask truncation ----

#[test]
fn slot_masks_above_7_truncate_to_zero_bit_writes() {
    // Retail computes `(byte)(1 << slot)` - slots 8..=31 truncate to a
    // zero mask, so the "set bit" write is a no-op. Slot 8 has no host
    // resources here either (None -> dead quad), so arm 5 is invalid.
    let mut h = Host {
        slots: vec![quad(1, 1, 0, 0)],
        ..Default::default()
    };
    let mut bits = 0xFF;
    assert!(!validate_action(&mut h, 0x05, 0, 8, &mut bits));
    assert_eq!(bits, 0xFF, "mask 0: the clear write is a no-op too");
}
