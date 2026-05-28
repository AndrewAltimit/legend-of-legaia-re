//! Equipment-aware battle stat aggregator.
//!
//! PORT: FUN_80042558
//!
//! Clean-room port of the per-frame stat resolver in
//! `ghidra/scripts/funcs/80042558.txt` (`FUN_80042558`). The retail
//! routine reads the active character record's base stat block, walks
//! the 8 equipment slots, ORs equipment ability flags into the global
//! 4×u32 mask at `0x80074358..0x80074368`, sums equipment stat
//! modifiers into the live `BattleActor`, and returns the resolved
//! attack / defense / accuracy / evasion values the strike resolver
//! consumes.
//!
//! ## What this module owns
//!
//! - The [`BattleStats`] resolved value type.
//! - The pure function [`compute_battle_stats`] that reads a
//!   [`StatRecord`] + [`EquipmentTable`] and returns [`BattleStats`].
//! - The [`EquipmentTable`] catalog used to look up per-item modifiers.
//!   Engines populate it once at startup (typically from the equipment
//!   table extracted from the SCUS data section).
//! - Status-effect modifiers fold in via [`StatusModifiers`], which
//!   bridges from [`legaia_engine_vm::status_effects::StatusKind`].
//!
//! ## What this module does NOT own
//!
//! - The retail 4×u32 ability mask at `0x80074358..0x80074368`.
//!   Engines aggregate that themselves through [`BattleStats::abilities`]
//!   ORed across every party member.
//! - The character-record byte layout - `legaia_save::CharacterRecord`
//!   exposes the relevant fields. This module is layout-agnostic.
//!
//! REF: FUN_801EC3E4

use legaia_engine_vm::status_effects::StatusKind;

/// Stat record consumed by the aggregator. Fields mirror the relevant
/// halfwords on the character record (see `docs/subsystems/battle.md`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatRecord {
    /// Base attack power (weapon-independent - character's "STR").
    pub base_attack: u16,
    /// Base upper-defense factor (UDF).
    pub base_udf: u16,
    /// Base lower-defense factor (LDF).
    pub base_ldf: u16,
    /// Base accuracy (hit rate stat).
    pub base_accuracy: u16,
    /// Base evasion / agility.
    pub base_evasion: u16,
    /// Currently-equipped item ids in the 8 equipment slots.
    pub equip: [u8; 8],
}

/// Per-item modifier table entry. Each equipment item adds these
/// values onto the character's resolved [`BattleStats`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ItemModifier {
    pub atk: i16,
    pub udf: i16,
    pub ldf: i16,
    pub acc: i16,
    pub eva: i16,
    /// Ability bits OR'd into the resolved [`BattleStats::abilities`].
    /// 32 bytes = 256 bits, matching the runtime mask shape.
    pub ability_bits: [u8; 32],
}

/// Catalogue mapping equipment-item id → [`ItemModifier`].
#[derive(Debug, Default, Clone)]
pub struct EquipmentTable {
    entries: std::collections::HashMap<u8, ItemModifier>,
}

impl EquipmentTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an item id with a modifier. Replaces any prior entry.
    pub fn set(&mut self, id: u8, m: ItemModifier) {
        self.entries.insert(id, m);
    }

    pub fn get(&self, id: u8) -> Option<&ItemModifier> {
        self.entries.get(&id)
    }

    /// Total registered item count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Status-effect modifiers folded into the resolved stats.
///
/// The retail engine applies a per-status delta to the stat lines in
/// the same `FUN_80042558` pass - Burned reduces ATK by 1/8, Confused
/// drops accuracy in half, etc. These values are baked into the
/// resolver and exposed for engines that want to override them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatusModifiers {
    /// Multiplier applied to ATK when the actor is Burned. Default `0.875`.
    pub burned_atk_mult: f32,
    /// Multiplier applied to accuracy when the actor is Confused. `0.5`.
    pub confused_acc_mult: f32,
    /// Multiplier applied to evasion when Asleep / Stunned / Petrified.
    /// `0.0` - these statuses make the actor a sitting duck.
    pub immobilised_eva_mult: f32,
    /// Multiplier applied to MP cost when Silenced. The retail engine
    /// blocks magic outright; this is exposed for engines that prefer
    /// "magic costs more" semantics. Default `f32::INFINITY` - a host
    /// that wants a hard block reads [`BattleStats::magic_blocked`].
    pub silenced_mp_mult: f32,
}

impl Default for StatusModifiers {
    fn default() -> Self {
        Self {
            burned_atk_mult: 0.875,
            confused_acc_mult: 0.5,
            immobilised_eva_mult: 0.0,
            silenced_mp_mult: f32::INFINITY,
        }
    }
}

/// Resolved per-actor battle stats - the inputs the strike resolver
/// reads each turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BattleStats {
    pub atk: u16,
    pub udf: u16,
    pub ldf: u16,
    pub acc: u16,
    pub eva: u16,
    /// 256-bit ability mask. Equipment + character record contribute.
    pub abilities: [u8; 32],
    /// `true` if Magic actions should be filtered out by the validator.
    pub magic_blocked: bool,
    /// `true` if the actor cannot act this turn (Asleep / Stunned /
    /// Petrified). The action validator should treat the slot as
    /// "select-only".
    pub action_blocked: bool,
}

impl BattleStats {
    /// `true` if the resolved abilities mask has bit `bit_idx` set
    /// (range 0..=255). Mirrors the retail
    /// `((mask[bit / 8] >> (bit % 8)) & 1) != 0` test.
    pub fn has_ability(&self, bit_idx: u16) -> bool {
        let i = (bit_idx / 8) as usize;
        let m = (bit_idx % 8) as u8;
        if i >= self.abilities.len() {
            return false;
        }
        (self.abilities[i] >> m) & 1 != 0
    }
}

fn or_assign_bits(dst: &mut [u8; 32], src: &[u8; 32]) {
    for i in 0..32 {
        dst[i] |= src[i];
    }
}

fn add_clamped(base: u16, delta: i16) -> u16 {
    let v = base as i32 + delta as i32;
    v.clamp(0, u16::MAX as i32) as u16
}

fn mul_clamp(value: u16, mult: f32) -> u16 {
    let v = (value as f32 * mult).round();
    v.clamp(0.0, u16::MAX as f32) as u16
}

/// Compute resolved [`BattleStats`] from a base [`StatRecord`], the
/// equipment catalog, and the set of active status kinds. Pure
/// function - does not mutate any input.
///
/// PORT: FUN_801CF650 (menu overlay variant)
///
/// `FUN_801CF650` in the menu overlay (`overlay_menu_801cf650.txt`) is
/// retail's equipment-stat aggregator for the menu / status / equipment
/// subscreens: it walks the 5 equipment bytes at `char_record + 0x196`, for
/// each non-zero slot looks up the item entry at stride `0xC` from the item
/// table (`0x8007433C`), gates on `entry[0] == 1` (equippable type), reads
/// the stat-bonus row at `entry[1] * 8` from `0x8007EF68`, and accumulates
/// into the menu's stat-display globals (`DAT_801EF08C/090/094/098/09C` —
/// STR / INT / DEF / LUCK / …). This function is the clean-room equivalent:
/// it consumes the same five equipment ids (`record.equip`), looks each up
/// in the engine's [`EquipmentTable`] (analogue of the `0x8007EF68` bonus
/// row), and accumulates the modifiers into [`BattleStats`]. It also folds
/// status-effect multipliers, which the SCUS pass leaves to the battle-side
/// kernels in `FUN_801EC3E4`. (The town-overlay alias at the same address —
/// emitter ramp-actor allocator — is a separate function; see
/// `docs/reference/functions.md`.)
pub fn compute_battle_stats(
    record: &StatRecord,
    table: &EquipmentTable,
    statuses: &[StatusKind],
    modifiers: &StatusModifiers,
) -> BattleStats {
    let mut stats = BattleStats {
        atk: record.base_attack,
        udf: record.base_udf,
        ldf: record.base_ldf,
        acc: record.base_accuracy,
        eva: record.base_evasion,
        abilities: [0u8; 32],
        magic_blocked: false,
        action_blocked: false,
    };
    // Walk equipment slots, sum modifiers + OR ability bits.
    for &id in record.equip.iter() {
        if id == 0 {
            continue;
        }
        if let Some(m) = table.get(id) {
            stats.atk = add_clamped(stats.atk, m.atk);
            stats.udf = add_clamped(stats.udf, m.udf);
            stats.ldf = add_clamped(stats.ldf, m.ldf);
            stats.acc = add_clamped(stats.acc, m.acc);
            stats.eva = add_clamped(stats.eva, m.eva);
            or_assign_bits(&mut stats.abilities, &m.ability_bits);
        }
    }
    // Fold status-effect modifiers.
    for &k in statuses {
        match k {
            StatusKind::Burned => {
                stats.atk = mul_clamp(stats.atk, modifiers.burned_atk_mult);
            }
            StatusKind::Confused => {
                stats.acc = mul_clamp(stats.acc, modifiers.confused_acc_mult);
            }
            StatusKind::Asleep | StatusKind::Stunned | StatusKind::Petrified => {
                stats.eva = mul_clamp(stats.eva, modifiers.immobilised_eva_mult);
                stats.action_blocked = true;
            }
            StatusKind::Silenced => {
                stats.magic_blocked = true;
            }
            _ => {}
        }
    }
    if statuses.iter().any(|s| matches!(s, StatusKind::Petrified)) {
        stats.magic_blocked = true;
    }
    stats
}

/// Convenience wrapper using [`StatusModifiers::default`].
pub fn compute_battle_stats_default(
    record: &StatRecord,
    table: &EquipmentTable,
    statuses: &[StatusKind],
) -> BattleStats {
    compute_battle_stats(record, table, statuses, &StatusModifiers::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> StatRecord {
        StatRecord {
            base_attack: 100,
            base_udf: 50,
            base_ldf: 60,
            base_accuracy: 90,
            base_evasion: 30,
            equip: [1, 2, 0, 0, 0, 0, 0, 0],
        }
    }

    fn weapon(atk: i16) -> ItemModifier {
        ItemModifier {
            atk,
            ..ItemModifier::default()
        }
    }

    fn armor(udf: i16, ldf: i16) -> ItemModifier {
        ItemModifier {
            udf,
            ldf,
            ..ItemModifier::default()
        }
    }

    #[test]
    fn empty_equip_returns_base_stats() {
        let mut r = record();
        r.equip = [0; 8];
        let table = EquipmentTable::new();
        let s = compute_battle_stats_default(&r, &table, &[]);
        assert_eq!(s.atk, 100);
        assert_eq!(s.udf, 50);
        assert_eq!(s.ldf, 60);
        assert_eq!(s.acc, 90);
        assert_eq!(s.eva, 30);
        assert!(!s.action_blocked);
        assert!(!s.magic_blocked);
    }

    #[test]
    fn equipment_atk_adds() {
        let mut t = EquipmentTable::new();
        t.set(1, weapon(20));
        let s = compute_battle_stats_default(&record(), &t, &[]);
        assert_eq!(s.atk, 120);
    }

    #[test]
    fn equipment_negative_modifier_clamps_at_zero() {
        let mut t = EquipmentTable::new();
        t.set(1, weapon(-200));
        let s = compute_battle_stats_default(&record(), &t, &[]);
        assert_eq!(s.atk, 0);
    }

    #[test]
    fn ability_bits_or_into_mask() {
        let mut a = ItemModifier::default();
        a.ability_bits[0] = 0x05; // bits 0 and 2
        let mut b = ItemModifier::default();
        b.ability_bits[0] = 0x02; // bit 1
        b.ability_bits[3] = 0x80; // high bit of byte 3 = bit 31
        let mut t = EquipmentTable::new();
        t.set(1, a);
        t.set(2, b);
        let s = compute_battle_stats_default(&record(), &t, &[]);
        assert_eq!(s.abilities[0], 0x07);
        assert_eq!(s.abilities[3], 0x80);
        assert!(s.has_ability(0));
        assert!(s.has_ability(1));
        assert!(s.has_ability(2));
        assert!(s.has_ability(31));
        assert!(!s.has_ability(3));
    }

    #[test]
    fn missing_equipment_id_is_silently_ignored() {
        let mut t = EquipmentTable::new();
        t.set(99, weapon(50));
        let s = compute_battle_stats_default(&record(), &t, &[]);
        assert_eq!(s.atk, 100);
    }

    #[test]
    fn burn_reduces_attack() {
        let s =
            compute_battle_stats_default(&record(), &EquipmentTable::new(), &[StatusKind::Burned]);
        assert_eq!(s.atk, 88); // 100 * 0.875 = 87.5 -> 88 (rounded)
    }

    #[test]
    fn confuse_halves_accuracy() {
        let s = compute_battle_stats_default(
            &record(),
            &EquipmentTable::new(),
            &[StatusKind::Confused],
        );
        assert_eq!(s.acc, 45);
    }

    #[test]
    fn sleep_zeros_evasion_and_blocks_actions() {
        let s =
            compute_battle_stats_default(&record(), &EquipmentTable::new(), &[StatusKind::Asleep]);
        assert_eq!(s.eva, 0);
        assert!(s.action_blocked);
    }

    #[test]
    fn silence_blocks_magic_only() {
        let s = compute_battle_stats_default(
            &record(),
            &EquipmentTable::new(),
            &[StatusKind::Silenced],
        );
        assert!(s.magic_blocked);
        assert!(!s.action_blocked);
    }

    #[test]
    fn petrify_blocks_both_magic_and_actions() {
        let s = compute_battle_stats_default(
            &record(),
            &EquipmentTable::new(),
            &[StatusKind::Petrified],
        );
        assert!(s.magic_blocked);
        assert!(s.action_blocked);
    }

    #[test]
    fn equipment_and_status_compose() {
        let mut t = EquipmentTable::new();
        t.set(1, weapon(40));
        t.set(2, armor(20, 25));
        let s = compute_battle_stats_default(&record(), &t, &[StatusKind::Burned]);
        // Atk: 100 + 40 = 140; Burned -> 140 * 0.875 = 122.5 -> 123 (rounded).
        assert_eq!(s.atk, 123);
        // UDF: 50 + 20 = 70; LDF: 60 + 25 = 85.
        assert_eq!(s.udf, 70);
        assert_eq!(s.ldf, 85);
    }

    #[test]
    fn custom_modifiers_let_engines_tune_severity() {
        let mods = StatusModifiers {
            burned_atk_mult: 0.5, // Brutal burn
            ..StatusModifiers::default()
        };
        let s = compute_battle_stats(
            &record(),
            &EquipmentTable::new(),
            &[StatusKind::Burned],
            &mods,
        );
        assert_eq!(s.atk, 50);
    }

    #[test]
    fn action_blocked_takes_priority_when_multi_status() {
        let s = compute_battle_stats_default(
            &record(),
            &EquipmentTable::new(),
            &[StatusKind::Asleep, StatusKind::Burned, StatusKind::Confused],
        );
        assert!(s.action_blocked);
        assert_eq!(s.eva, 0);
        // Atk + accuracy still applied.
        assert_eq!(s.atk, 88);
        assert_eq!(s.acc, 45);
    }

    #[test]
    fn equipment_table_len_and_is_empty_track() {
        let mut t = EquipmentTable::new();
        assert!(t.is_empty());
        assert_eq!(t.len(), 0);
        t.set(1, weapon(10));
        assert_eq!(t.len(), 1);
        assert!(!t.is_empty());
    }
}
