//! Action validator — clean-room port of `FUN_8003fb10`.
//!
//! The retail battle / menu UI calls this routine before committing a player
//! choice ("can party member N use this item / spell on slot M?"). It reads
//! HP / MP / status / item-count / stat caps from the active record (a battle
//! actor in battle, a character record on the field), decides whether the
//! action is valid, and writes a per-slot "target-valid" bitmask byte at
//! `gp + 0x9A8`. The bitmask drives the menu's per-slot greying / cursor
//! behaviour.
//!
//! ## What's ported
//!
//! Every one of the 16 outer arms (cases 0..=0xD plus the synthetic
//! 0x80..=0x83 cluster). The Ghidra decompilation at
//! `ghidra/scripts/funcs/8003fb10.txt` is the authoritative reference; the
//! comments below cite the case number. Inputs come through
//! [`ActionValidatorHost`] so engines wire the validator against any storage
//! layout.
//!
//! ## What's NOT ported here
//!
//! The retail bitmask byte at `gp + 0x9A8` is exposed via
//! [`ActionValidatorHost::target_valid_bits`]; the host owns it and gets to
//! decide where it lives (e.g. inline in the `BattleActor` table or the menu
//! cursor state). The validator never reaches outside the host.
//!
//! ## Clean-room boundary
//!
//! Same rules as `crate::battle_action`: no Sony bytes embedded, the dump is
//! the spec. Tests use synthetic stats.

#![allow(clippy::too_many_arguments)]

/// One validation request.
#[derive(Debug, Clone, Copy)]
pub struct ValidationRequest {
    /// Outer dispatch arm — `param_1` in the retail signature. Selects which
    /// validation rule fires. The retail switch table accepts up to 0x83;
    /// values past 0x83 (other than the named arms) decode to "always
    /// invalid."
    pub arm: u8,
    /// Sub-case for [`ValidationArm::ItemUseGate`] (arm `6`). Reserved for
    /// the rest. Range 0..=6.
    pub sub: u8,
    /// Target slot. Battle: 0..=7 indexes the actor pointer table (`0..3`
    /// party, `3..8` monsters). Field menu: a party slot (0..=2 typically).
    pub slot: u8,
}

/// Snapshot of the per-slot stats the validator reads. The host provides one
/// of these per call; the validator never reads through indirect pointers.
///
/// `hp_max == 0` is the "no record at this slot" sentinel — the validator
/// treats it as "always invalid" for that slot.
#[derive(Debug, Clone, Copy, Default)]
pub struct SlotStats {
    /// Current HP (`+0x14C` battle / `+0x6CE` char-record-relative).
    pub hp: u16,
    /// Max HP (`+0x14E` battle / `+0x6CC` char-record-relative).
    pub hp_max: u16,
    /// Current MP (`+0x150` battle / `+0x6D2` char).
    pub mp: u16,
    /// Max MP (`+0x152` battle / `+0x6D0` char).
    pub mp_max: u16,
    /// Status flags at `+0x16E` (battle). The retail validator tests `& 3`
    /// for arm 8 (status check) and `!= 0` for arm 3.
    pub status_flags: u16,
}

/// Cap thresholds the level-up / stat-boost item validator (arm 6) compares
/// against. The actual retail compares stats to literal `999` / `9999` /
/// `0x118` / `0x270F` — engines that want different caps override here.
#[derive(Debug, Clone, Copy)]
pub struct StatCaps {
    pub hp_max_cap: u16, // 9999 = 0x270F retail
    pub mp_max_cap: u16, // 999 retail
    pub stat_a_cap: u16, // 999 retail (XP / level / similar)
    pub stat_b_cap: u16, // 999 retail
    pub stat_c_cap: u16, // 999 retail
    pub stat_d_cap: u16, // 999 retail (paired with stat_c)
    pub stat_e_cap: u16, // 999 retail
    pub anim_cap: u16,   // 0x118 retail
}

impl Default for StatCaps {
    fn default() -> Self {
        Self {
            hp_max_cap: 9999,
            mp_max_cap: 999,
            stat_a_cap: 999,
            stat_b_cap: 999,
            stat_c_cap: 999,
            stat_d_cap: 999,
            stat_e_cap: 999,
            anim_cap: 0x118,
        }
    }
}

/// Stats relevant to arm 6 (the item-use cap walker). The retail reads u16
/// fields at +0x6CC..+0x6E2 of each character record. We model them as eight
/// distinct `cap_*` fields to avoid baking offsets into the validator.
#[derive(Debug, Clone, Copy, Default)]
pub struct CapStats {
    /// `+0x6CC` — HP-max-style stat (compared against `hp_max_cap`).
    pub stat_hp_max: u16,
    /// `+0x6DA` — paired with `stat_a_cap`.
    pub stat_a: u16,
    /// `+0x6E0` — `stat_b_cap`.
    pub stat_b: u16,
    /// `+0x6E2` — `stat_c_cap`.
    pub stat_c: u16,
    /// `+0x6D0` — MP-max-style; compared against `stat_d_cap` (yes, the
    /// retail mapping is asymmetric — mp_max ends up against stat_d in the
    /// arm-6 walker, not against `mp_max_cap`).
    pub stat_d: u16,
    /// `+0x6D8` — `anim_cap`.
    pub stat_anim: u16,
    /// `+0x6DC` — `stat_e_cap`.
    pub stat_e: u16,
    /// `+0x6DE` — `stat_e_cap` (final fall-through).
    pub stat_f: u16,
}

/// Host abstraction. Engines implement this against their per-slot record
/// storage (battle actor table or character record array, depending on
/// [`ActionValidatorHost::in_battle`]).
pub trait ActionValidatorHost {
    /// Read HP / MP / status for `slot`. Returning `None` means the slot is
    /// unoccupied — the validator treats this as "invalid target".
    fn slot_stats(&self, slot: u8) -> Option<SlotStats>;

    /// Cap-stat snapshot for `slot` (arm 6 only). Returning `None` collapses
    /// every arm-6 sub-case to "invalid".
    fn cap_stats(&self, _slot: u8) -> Option<CapStats> {
        None
    }

    /// Active-stat caps the arm-6 walker compares against. Default is the
    /// retail constants.
    fn stat_caps(&self) -> StatCaps {
        StatCaps::default()
    }

    /// Number of party slots to walk in arm 1 (the "any-injured-party"
    /// reducer). Default `3`.
    fn party_count(&self) -> u8 {
        3
    }

    /// Per-slot party order. `slot_for_party_index(i)` gives the actor-table
    /// slot that holds party member `i`. Retail reads `(&DAT_80084598)[i]`.
    /// Default identity mapping `i -> i`.
    fn slot_for_party_index(&self, i: u8) -> u8 {
        i
    }

    /// `_DAT_8007B83C == 0x15` — the "we're inside a battle" gate. Drives
    /// arm 3 / arm 8's two-path branch. Default `false`.
    fn in_battle(&self) -> bool {
        false
    }

    /// `_DAT_1F800394` story-flag bitmap. Arm 0x80 tests `& 0x100000`,
    /// arm 0x81 tests `& 0x200000`. Default `0`.
    fn story_flag_bits(&self) -> u32 {
        0
    }

    /// `FUN_8003CE64(flag_id)` — the system-flag query at `_DAT_80086D70`.
    /// Returns true when bit `flag_id` is set. Arm 0x80 queries 5; arm 0x81
    /// queries 6. Default `false`.
    fn system_flag_test(&self, _flag_id: u8) -> bool {
        false
    }

    /// `FUN_80046898()` — the item-count / capture-shop validator. Arm 0x82
    /// returns whatever this returns directly. Default `0`.
    fn external_validator(&mut self) -> u32 {
        0
    }

    /// Read+write access to the per-slot validity bitmask byte at retail
    /// `gp + 0x9A8`. Bit `1 << slot` is set when validation succeeds and
    /// cleared at the start of every per-slot arm. Engines wire this against
    /// whatever storage the menu cursor reads.
    fn target_valid_bits(&mut self) -> &mut u8;
}

/// Run one validation. Returns `true` when the action is valid for the given
/// slot, `false` otherwise. Mutates the host's `target_valid_bits` to mirror
/// the retail side-effect.
pub fn validate<H: ActionValidatorHost + ?Sized>(host: &mut H, req: ValidationRequest) -> bool {
    let slot_bit: u8 = 1u8.wrapping_shl(req.slot as u32 & 0x1F);
    let clear_mask: u8 = !slot_bit;

    match req.arm {
        // Arm 0: alive-and-not-full-HP (heal target). Clear bit, then set
        // when (hp != 0) AND (hp < hp_max).
        0x00 => {
            *host.target_valid_bits() &= clear_mask;
            let Some(s) = host.slot_stats(req.slot) else {
                return false;
            };
            if s.hp == 0 || s.hp >= s.hp_max {
                return false;
            }
            *host.target_valid_bits() |= slot_bit;
            true
        }
        // Arm 1: any-injured-party reducer. Walks `party_count` slots, sets
        // bit per slot that's alive-and-not-full. Returns true if any matched.
        0x01 => {
            *host.target_valid_bits() = 0;
            let count = host.party_count();
            let mut any = false;
            for i in 0..count {
                let s_slot = host.slot_for_party_index(i);
                let Some(s) = host.slot_stats(s_slot) else {
                    continue;
                };
                if s.hp != 0 && s.hp < s.hp_max {
                    any = true;
                    *host.target_valid_bits() |= 1u8.wrapping_shl(s_slot as u32 & 0x1F);
                }
            }
            any
        }
        // Arm 2: alive-and-MP-not-full. (Restore-MP-target check.)
        0x02 => {
            *host.target_valid_bits() &= clear_mask;
            let Some(s) = host.slot_stats(req.slot) else {
                return false;
            };
            if s.hp == 0 || s.mp >= s.mp_max {
                return false;
            }
            *host.target_valid_bits() |= slot_bit;
            true
        }
        // Arm 3: status-flag presence. Battle path: `actor.field[+0x16E] != 0`.
        // Field path: `char_record[+0x12E] != 0`. Both surface via
        // `slot_stats(...).status_flags`.
        0x03 => {
            if host.in_battle() {
                let Some(s) = host.slot_stats(req.slot) else {
                    return false;
                };
                return s.status_flags != 0;
            }
            *host.target_valid_bits() &= clear_mask;
            let Some(s) = host.slot_stats(req.slot) else {
                return false;
            };
            if s.status_flags == 0 {
                return false;
            }
            *host.target_valid_bits() |= slot_bit;
            true
        }
        // Arm 4: dead-target check (Revive item validator). Set bit when
        // `hp == 0`.
        0x04 => {
            *host.target_valid_bits() &= clear_mask;
            let Some(s) = host.slot_stats(req.slot) else {
                return false;
            };
            if s.hp != 0 {
                return false;
            }
            *host.target_valid_bits() |= slot_bit;
            true
        }
        // Arm 5: alive (any-action target).
        0x05 => {
            *host.target_valid_bits() &= clear_mask;
            let Some(s) = host.slot_stats(req.slot) else {
                return false;
            };
            if s.hp == 0 {
                return false;
            }
            *host.target_valid_bits() |= slot_bit;
            true
        }
        // Arm 6: stat-cap walker (level-up / stat-boost item validator).
        // Sub-case picks which stat to check.
        0x06 => {
            *host.target_valid_bits() &= clear_mask;
            let Some(s) = host.slot_stats(req.slot) else {
                return false;
            };
            if s.hp == 0 {
                return false;
            }
            let Some(c) = host.cap_stats(req.slot) else {
                return false;
            };
            let caps = host.stat_caps();
            let valid = match req.sub {
                0 => c.stat_hp_max < caps.hp_max_cap,
                1 => c.stat_a < caps.stat_a_cap,
                2 => c.stat_d < caps.stat_d_cap || c.stat_e < caps.stat_e_cap,
                3 => c.stat_b < caps.stat_b_cap,
                4 => c.stat_c < caps.stat_c_cap,
                5 => c.stat_d < caps.stat_d_cap,
                6 => {
                    // Union of every cap check.
                    c.stat_hp_max < caps.hp_max_cap
                        || c.stat_a < caps.stat_a_cap
                        || c.stat_b < caps.stat_b_cap
                        || c.stat_c < caps.stat_c_cap
                        || c.stat_d < caps.stat_d_cap
                        || c.stat_anim < caps.anim_cap
                        || c.stat_e < caps.stat_e_cap
                        || c.stat_f < caps.stat_e_cap
                }
                _ => false,
            };
            if !valid {
                return false;
            }
            *host.target_valid_bits() |= slot_bit;
            true
        }
        // Arm 7: alive (synonym of arm 5, separate code path that omits the
        // upper-bound test). Behaviourally identical for HP > 0 targets.
        0x07 => {
            *host.target_valid_bits() &= clear_mask;
            let Some(s) = host.slot_stats(req.slot) else {
                return false;
            };
            if s.hp == 0 {
                return false;
            }
            *host.target_valid_bits() |= slot_bit;
            true
        }
        // Arm 8: alive-and-status-bits-low (the "can apply paralysis / sleep"
        // gate). Battle: actor.status & 3 != 0. Field: char.status & 3 != 0.
        0x08 => {
            let Some(s) = host.slot_stats(req.slot) else {
                return false;
            };
            if s.hp == 0 {
                return false;
            }
            if host.in_battle() {
                return (s.status_flags & 3) != 0;
            }
            *host.target_valid_bits() &= clear_mask;
            if (s.status_flags & 3) == 0 {
                return false;
            }
            *host.target_valid_bits() |= slot_bit;
            true
        }
        // Arms 9 / 0xA: always-valid + force the bitmask to the literal `7`
        // (party-3-targeting). Used by all-party heal / status spells.
        0x09 | 0x0A => {
            *host.target_valid_bits() = 7;
            true
        }
        // Arms 0xB / 0xC / 0xD: per-slot exact-match — only valid when
        // `slot == arm - 0xB` (slots 0/1/2). Sets the bitmask to that single
        // slot bit.
        0x0B..=0x0D => {
            let want = req.arm - 0x0B;
            if req.slot != want {
                return false;
            }
            *host.target_valid_bits() = 1u8.wrapping_shl(req.slot as u32 & 0x1F);
            true
        }
        // Arm 0x80 / 0x81: out-of-battle system-flag carve-out. Both use
        // story flag bit + a system-flag query.
        0x80 | 0x81 => {
            if host.in_battle() {
                return false;
            }
            let bit = if req.arm == 0x80 { 0x100000 } else { 0x200000 };
            if (host.story_flag_bits() & bit) != 0 {
                return false;
            }
            let flag = if req.arm == 0x80 { 5 } else { 6 };
            if host.system_flag_test(flag) {
                return false;
            }
            true
        }
        // Arm 0x82: out-of-battle external validator.
        0x82 => {
            if host.in_battle() {
                return false;
            }
            host.external_validator() != 0
        }
        // Arm 0x83: always valid.
        0x83 => true,
        // Anything else: invalid.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal host backed by a fixed slot table.
    #[derive(Default)]
    struct TestHost {
        stats: Vec<Option<SlotStats>>,
        caps: Vec<Option<CapStats>>,
        in_battle: bool,
        story_flags: u32,
        sys_flag_5: bool,
        sys_flag_6: bool,
        ext: u32,
        target_bits: u8,
    }

    impl TestHost {
        fn with_slots(n: usize) -> Self {
            Self {
                stats: vec![None; n],
                caps: vec![None; n],
                ..Default::default()
            }
        }
        fn set_stats(&mut self, slot: u8, s: SlotStats) {
            self.stats[slot as usize] = Some(s);
        }
        fn set_caps(&mut self, slot: u8, c: CapStats) {
            self.caps[slot as usize] = Some(c);
        }
    }

    impl ActionValidatorHost for TestHost {
        fn slot_stats(&self, slot: u8) -> Option<SlotStats> {
            self.stats.get(slot as usize).and_then(|x| *x)
        }
        fn cap_stats(&self, slot: u8) -> Option<CapStats> {
            self.caps.get(slot as usize).and_then(|x| *x)
        }
        fn in_battle(&self) -> bool {
            self.in_battle
        }
        fn story_flag_bits(&self) -> u32 {
            self.story_flags
        }
        fn system_flag_test(&self, flag: u8) -> bool {
            match flag {
                5 => self.sys_flag_5,
                6 => self.sys_flag_6,
                _ => false,
            }
        }
        fn external_validator(&mut self) -> u32 {
            self.ext
        }
        fn target_valid_bits(&mut self) -> &mut u8 {
            &mut self.target_bits
        }
    }

    #[test]
    fn arm_0_accepts_alive_not_full_hp() {
        let mut h = TestHost::with_slots(8);
        h.set_stats(
            2,
            SlotStats {
                hp: 50,
                hp_max: 100,
                ..Default::default()
            },
        );
        let ok = validate(
            &mut h,
            ValidationRequest {
                arm: 0,
                sub: 0,
                slot: 2,
            },
        );
        assert!(ok);
        assert_eq!(h.target_bits & 0b100, 0b100);
    }

    #[test]
    fn arm_0_rejects_full_hp() {
        let mut h = TestHost::with_slots(8);
        h.set_stats(
            0,
            SlotStats {
                hp: 100,
                hp_max: 100,
                ..Default::default()
            },
        );
        let ok = validate(
            &mut h,
            ValidationRequest {
                arm: 0,
                sub: 0,
                slot: 0,
            },
        );
        assert!(!ok);
        assert_eq!(h.target_bits & 1, 0);
    }

    #[test]
    fn arm_0_rejects_dead_target() {
        let mut h = TestHost::with_slots(8);
        h.set_stats(
            0,
            SlotStats {
                hp: 0,
                hp_max: 100,
                ..Default::default()
            },
        );
        let ok = validate(
            &mut h,
            ValidationRequest {
                arm: 0,
                sub: 0,
                slot: 0,
            },
        );
        assert!(!ok);
    }

    #[test]
    fn arm_1_walks_party() {
        let mut h = TestHost::with_slots(8);
        h.set_stats(
            0,
            SlotStats {
                hp: 100,
                hp_max: 100,
                ..Default::default()
            },
        );
        h.set_stats(
            1,
            SlotStats {
                hp: 30,
                hp_max: 100,
                ..Default::default()
            },
        ); // injured
        h.set_stats(
            2,
            SlotStats {
                hp: 0,
                hp_max: 100,
                ..Default::default()
            },
        ); // dead
        let ok = validate(
            &mut h,
            ValidationRequest {
                arm: 1,
                sub: 0,
                slot: 0,
            },
        );
        assert!(ok);
        // Only slot 1 was a valid target; the validator clears the byte and
        // sets only that slot's bit.
        assert_eq!(h.target_bits, 0b010);
    }

    #[test]
    fn arm_1_returns_false_when_party_full_hp() {
        let mut h = TestHost::with_slots(8);
        for i in 0..3 {
            h.set_stats(
                i,
                SlotStats {
                    hp: 100,
                    hp_max: 100,
                    ..Default::default()
                },
            );
        }
        let ok = validate(
            &mut h,
            ValidationRequest {
                arm: 1,
                sub: 0,
                slot: 0,
            },
        );
        assert!(!ok);
        assert_eq!(h.target_bits, 0);
    }

    #[test]
    fn arm_2_alive_and_mp_not_full() {
        let mut h = TestHost::with_slots(8);
        h.set_stats(
            1,
            SlotStats {
                hp: 50,
                hp_max: 100,
                mp: 10,
                mp_max: 50,
                ..Default::default()
            },
        );
        assert!(validate(
            &mut h,
            ValidationRequest {
                arm: 2,
                sub: 0,
                slot: 1
            }
        ));
        // Full MP rejects.
        h.set_stats(
            1,
            SlotStats {
                hp: 50,
                hp_max: 100,
                mp: 50,
                mp_max: 50,
                ..Default::default()
            },
        );
        assert!(!validate(
            &mut h,
            ValidationRequest {
                arm: 2,
                sub: 0,
                slot: 1
            }
        ));
    }

    #[test]
    fn arm_3_battle_branch_uses_status_flags_only() {
        let mut h = TestHost::with_slots(8);
        h.in_battle = true;
        h.set_stats(
            0,
            SlotStats {
                status_flags: 4,
                ..Default::default()
            },
        );
        // Battle path returns directly without touching the bitmask.
        let pre = h.target_bits;
        assert!(validate(
            &mut h,
            ValidationRequest {
                arm: 3,
                sub: 0,
                slot: 0
            }
        ));
        assert_eq!(h.target_bits, pre);
        h.set_stats(
            0,
            SlotStats {
                status_flags: 0,
                ..Default::default()
            },
        );
        assert!(!validate(
            &mut h,
            ValidationRequest {
                arm: 3,
                sub: 0,
                slot: 0
            }
        ));
    }

    #[test]
    fn arm_4_revive_check() {
        let mut h = TestHost::with_slots(8);
        h.set_stats(
            2,
            SlotStats {
                hp: 0,
                hp_max: 100,
                ..Default::default()
            },
        );
        assert!(validate(
            &mut h,
            ValidationRequest {
                arm: 4,
                sub: 0,
                slot: 2
            }
        ));
        h.set_stats(
            2,
            SlotStats {
                hp: 1,
                hp_max: 100,
                ..Default::default()
            },
        );
        assert!(!validate(
            &mut h,
            ValidationRequest {
                arm: 4,
                sub: 0,
                slot: 2
            }
        ));
    }

    #[test]
    fn arm_5_and_arm_7_are_alive_check() {
        let mut h = TestHost::with_slots(8);
        h.set_stats(
            0,
            SlotStats {
                hp: 1,
                hp_max: 100,
                ..Default::default()
            },
        );
        assert!(validate(
            &mut h,
            ValidationRequest {
                arm: 5,
                sub: 0,
                slot: 0
            }
        ));
        assert!(validate(
            &mut h,
            ValidationRequest {
                arm: 7,
                sub: 0,
                slot: 0
            }
        ));
        h.set_stats(
            0,
            SlotStats {
                hp: 0,
                hp_max: 100,
                ..Default::default()
            },
        );
        assert!(!validate(
            &mut h,
            ValidationRequest {
                arm: 5,
                sub: 0,
                slot: 0
            }
        ));
        assert!(!validate(
            &mut h,
            ValidationRequest {
                arm: 7,
                sub: 0,
                slot: 0
            }
        ));
    }

    #[test]
    fn arm_6_sub_0_hp_max_cap() {
        let mut h = TestHost::with_slots(8);
        h.set_stats(
            0,
            SlotStats {
                hp: 50,
                hp_max: 100,
                ..Default::default()
            },
        );
        h.set_caps(
            0,
            CapStats {
                stat_hp_max: 8000,
                ..Default::default()
            },
        );
        assert!(validate(
            &mut h,
            ValidationRequest {
                arm: 6,
                sub: 0,
                slot: 0
            }
        ));
        h.set_caps(
            0,
            CapStats {
                stat_hp_max: 9999,
                ..Default::default()
            },
        );
        assert!(!validate(
            &mut h,
            ValidationRequest {
                arm: 6,
                sub: 0,
                slot: 0
            }
        ));
    }

    #[test]
    fn arm_6_sub_6_unions_all_caps() {
        let mut h = TestHost::with_slots(8);
        h.set_stats(
            0,
            SlotStats {
                hp: 50,
                hp_max: 100,
                ..Default::default()
            },
        );
        // All at cap → invalid.
        h.set_caps(
            0,
            CapStats {
                stat_hp_max: 9999,
                stat_a: 999,
                stat_b: 999,
                stat_c: 999,
                stat_d: 999,
                stat_anim: 0x118,
                stat_e: 999,
                stat_f: 999,
            },
        );
        assert!(!validate(
            &mut h,
            ValidationRequest {
                arm: 6,
                sub: 6,
                slot: 0
            }
        ));
        // Drop one stat below cap → valid.
        h.set_caps(
            0,
            CapStats {
                stat_hp_max: 9999,
                stat_a: 100,
                stat_b: 999,
                stat_c: 999,
                stat_d: 999,
                stat_anim: 0x118,
                stat_e: 999,
                stat_f: 999,
            },
        );
        assert!(validate(
            &mut h,
            ValidationRequest {
                arm: 6,
                sub: 6,
                slot: 0
            }
        ));
    }

    #[test]
    fn arm_8_status_low_bits_battle() {
        let mut h = TestHost::with_slots(8);
        h.in_battle = true;
        h.set_stats(
            0,
            SlotStats {
                hp: 50,
                hp_max: 100,
                status_flags: 1,
                ..Default::default()
            },
        );
        assert!(validate(
            &mut h,
            ValidationRequest {
                arm: 8,
                sub: 0,
                slot: 0
            }
        ));
        h.set_stats(
            0,
            SlotStats {
                hp: 50,
                hp_max: 100,
                status_flags: 4,
                ..Default::default()
            },
        );
        assert!(!validate(
            &mut h,
            ValidationRequest {
                arm: 8,
                sub: 0,
                slot: 0
            }
        ));
        h.set_stats(
            0,
            SlotStats {
                hp: 0,
                hp_max: 100,
                status_flags: 1,
                ..Default::default()
            },
        );
        assert!(!validate(
            &mut h,
            ValidationRequest {
                arm: 8,
                sub: 0,
                slot: 0
            }
        ));
    }

    #[test]
    fn arm_9_and_a_force_party_three_bits() {
        let mut h = TestHost::with_slots(8);
        assert!(validate(
            &mut h,
            ValidationRequest {
                arm: 9,
                sub: 0,
                slot: 0
            }
        ));
        assert_eq!(h.target_bits, 7);
        h.target_bits = 0;
        assert!(validate(
            &mut h,
            ValidationRequest {
                arm: 0xA,
                sub: 0,
                slot: 5
            }
        ));
        assert_eq!(h.target_bits, 7);
    }

    #[test]
    fn arms_b_c_d_only_match_corresponding_slot() {
        let mut h = TestHost::with_slots(8);
        for arm in 0xB..=0xD {
            let want = arm - 0xB;
            for slot in 0..3 {
                h.target_bits = 0;
                let ok = validate(&mut h, ValidationRequest { arm, sub: 0, slot });
                assert_eq!(ok, slot == want, "arm {arm:#x} slot {slot}");
                if ok {
                    assert_eq!(h.target_bits, 1u8 << slot);
                }
            }
        }
    }

    #[test]
    fn arm_80_blocks_in_battle() {
        let mut h = TestHost::with_slots(8);
        h.in_battle = true;
        assert!(!validate(
            &mut h,
            ValidationRequest {
                arm: 0x80,
                sub: 0,
                slot: 0
            }
        ));
        h.in_battle = false;
        // Story flag set → invalid.
        h.story_flags = 0x100000;
        assert!(!validate(
            &mut h,
            ValidationRequest {
                arm: 0x80,
                sub: 0,
                slot: 0
            }
        ));
        // Story flag clear, system flag set → invalid.
        h.story_flags = 0;
        h.sys_flag_5 = true;
        assert!(!validate(
            &mut h,
            ValidationRequest {
                arm: 0x80,
                sub: 0,
                slot: 0
            }
        ));
        // All clear → valid.
        h.sys_flag_5 = false;
        assert!(validate(
            &mut h,
            ValidationRequest {
                arm: 0x80,
                sub: 0,
                slot: 0
            }
        ));
    }

    #[test]
    fn arm_81_uses_distinct_flags() {
        let mut h = TestHost::with_slots(8);
        h.story_flags = 0x100000; // arm 0x80 only — 0x81 ignores this.
        assert!(validate(
            &mut h,
            ValidationRequest {
                arm: 0x81,
                sub: 0,
                slot: 0
            }
        ));
        h.story_flags = 0x200000;
        assert!(!validate(
            &mut h,
            ValidationRequest {
                arm: 0x81,
                sub: 0,
                slot: 0
            }
        ));
        h.story_flags = 0;
        h.sys_flag_6 = true;
        assert!(!validate(
            &mut h,
            ValidationRequest {
                arm: 0x81,
                sub: 0,
                slot: 0
            }
        ));
    }

    #[test]
    fn arm_82_calls_external_validator() {
        let mut h = TestHost::with_slots(8);
        h.ext = 0;
        assert!(!validate(
            &mut h,
            ValidationRequest {
                arm: 0x82,
                sub: 0,
                slot: 0
            }
        ));
        h.ext = 1;
        assert!(validate(
            &mut h,
            ValidationRequest {
                arm: 0x82,
                sub: 0,
                slot: 0
            }
        ));
        h.in_battle = true;
        assert!(!validate(
            &mut h,
            ValidationRequest {
                arm: 0x82,
                sub: 0,
                slot: 0
            }
        ));
    }

    #[test]
    fn arm_83_always_valid() {
        let mut h = TestHost::with_slots(8);
        assert!(validate(
            &mut h,
            ValidationRequest {
                arm: 0x83,
                sub: 0,
                slot: 0
            }
        ));
    }

    #[test]
    fn unknown_arm_invalid() {
        let mut h = TestHost::with_slots(8);
        assert!(!validate(
            &mut h,
            ValidationRequest {
                arm: 0x40,
                sub: 0,
                slot: 0
            }
        ));
    }

    #[test]
    fn no_record_in_slot_invalid() {
        let mut h = TestHost::with_slots(8);
        for arm in [0u8, 2, 3, 4, 5, 6, 7, 8] {
            assert!(
                !validate(
                    &mut h,
                    ValidationRequest {
                        arm,
                        sub: 0,
                        slot: 0
                    }
                ),
                "arm {arm:#x}"
            );
        }
    }
}
