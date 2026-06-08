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
    /// Base accuracy (hit rate stat - derived from AGL, not equipment-fed).
    pub base_accuracy: u16,
    /// Base evasion / agility (derived from AGL, not equipment-fed).
    pub base_evasion: u16,
    /// Base speed (`SPD`) - turn-order stat, equipment-boosted by footwear.
    pub base_spd: u16,
    /// Base intelligence (`INT`) - magic stat, equipment-boosted by head gear.
    pub base_int: u16,
    /// Currently-equipped item ids in the 8 equipment slots.
    pub equip: [u8; 8],
}

/// Per-item modifier table entry. Each equipment item adds these
/// values onto the character's resolved [`BattleStats`].
///
/// The retail equipment stat-bonus table (`DAT_80074F68`) modifies exactly
/// these five stats - `ATK` / `UDF` / `LDF` / `SPD` / `INT` (see
/// `legaia_asset::equip_stats`; the `+0` byte is the INT bonus, `+4` the SPD
/// bonus). Equipment never touches the derived accuracy / evasion lines, so
/// those are not represented here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ItemModifier {
    pub atk: i16,
    pub udf: i16,
    pub ldf: i16,
    /// Speed (`SPD`) bonus - footwear's `+4` byte.
    pub spd: i16,
    /// Intelligence (`INT`) bonus - head gear's `+0` byte.
    pub int: i16,
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
/// the same `FUN_80042558` pass - Toxic reduces ATK by 1/8, Confuse
/// drops accuracy in half, etc. These values are baked into the
/// resolver and exposed for engines that want to override them. (Per the
/// Legaia wiki, Toxic also lowers DEFENSE, not just ATK; that DEF penalty is
/// not modelled here yet - only the ATK multiplier is applied.)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatusModifiers {
    /// Multiplier applied to ATK when the actor is Toxic. Default `0.875`.
    pub toxic_atk_mult: f32,
    /// Multiplier applied to accuracy when the actor is Confuse. `0.5`.
    pub confuse_acc_mult: f32,
    /// Multiplier applied to evasion when Numb / Sleep / Stone / Faint.
    /// `0.0` - these statuses make the actor a sitting duck.
    pub immobilised_eva_mult: f32,
    /// Multiplier applied to MP cost when Curse. The retail engine
    /// blocks magic outright; this is exposed for engines that prefer
    /// "magic costs more" semantics. Default `f32::INFINITY` - a host
    /// that wants a hard block reads [`BattleStats::magic_blocked`].
    pub curse_mp_mult: f32,
}

impl Default for StatusModifiers {
    fn default() -> Self {
        Self {
            toxic_atk_mult: 0.875,
            confuse_acc_mult: 0.5,
            immobilised_eva_mult: 0.0,
            curse_mp_mult: f32::INFINITY,
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
    /// Resolved speed (`SPD`) - base + equipment (footwear). Feeds turn order.
    pub spd: u16,
    /// Resolved intelligence (`INT`) - base + equipment (head gear).
    pub int: u16,
    /// Derived accuracy. Comes from AGL upstream; equipment does not feed it.
    pub acc: u16,
    /// Derived evasion. Comes from AGL upstream; equipment does not feed it.
    pub eva: u16,
    /// 256-bit ability mask. Equipment + character record contribute.
    pub abilities: [u8; 32],
    /// `true` if Magic actions should be filtered out by the validator.
    pub magic_blocked: bool,
    /// `true` if the actor cannot act this turn (Numb / Sleep / Stone /
    /// Faint). The action validator should treat the slot as
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
/// the stat-bonus row at `entry[1] * 8` from `0x80074F68`, and accumulates
/// into the menu's stat-display globals (`DAT_801EF08C/090/094/098/09C`). Those
/// five accumulators are pre-loaded by `FUN_801CF5D0` from the character
/// record's `ATK / UDF / LDF / SPD / INT` halfwords (`+0x112/0x114/0x116/0x118/
/// 0x11A`), so the five equipment bytes target `ATK / UDF / LDF / SPD / INT`
/// respectively (the `+0` byte is INT, the `+4` byte is SPD). This function is
/// the clean-room equivalent: it consumes the same five equipment ids
/// (`record.equip`), looks each up in the engine's [`EquipmentTable`] (analogue
/// of the `0x80074F68` bonus row), and accumulates the modifiers into
/// [`BattleStats`]. Accuracy / evasion are derived from AGL upstream and are
/// not equipment-fed, so the equipment loop leaves them alone. It also folds
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
        spd: record.base_spd,
        int: record.base_int,
        acc: record.base_accuracy,
        eva: record.base_evasion,
        abilities: [0u8; 32],
        magic_blocked: false,
        action_blocked: false,
    };
    // Walk equipment slots, sum the five equipment-fed stats (ATK/UDF/LDF/SPD/
    // INT) + OR ability bits. Accuracy/evasion are derived from AGL and are not
    // touched by equipment.
    for &id in record.equip.iter() {
        if id == 0 {
            continue;
        }
        if let Some(m) = table.get(id) {
            stats.atk = add_clamped(stats.atk, m.atk);
            stats.udf = add_clamped(stats.udf, m.udf);
            stats.ldf = add_clamped(stats.ldf, m.ldf);
            stats.spd = add_clamped(stats.spd, m.spd);
            stats.int = add_clamped(stats.int, m.int);
            or_assign_bits(&mut stats.abilities, &m.ability_bits);
        }
    }
    // Fold status-effect modifiers.
    for &k in statuses {
        match k {
            StatusKind::Toxic => {
                stats.atk = mul_clamp(stats.atk, modifiers.toxic_atk_mult);
            }
            StatusKind::Confuse => {
                stats.acc = mul_clamp(stats.acc, modifiers.confuse_acc_mult);
            }
            StatusKind::Numb | StatusKind::Sleep | StatusKind::Stone | StatusKind::Faint => {
                stats.eva = mul_clamp(stats.eva, modifiers.immobilised_eva_mult);
                stats.action_blocked = true;
            }
            StatusKind::Curse => {
                stats.magic_blocked = true;
            }
            _ => {}
        }
    }
    if statuses.iter().any(|s| matches!(s, StatusKind::Faint)) {
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
            base_spd: 40,
            base_int: 25,
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
        assert_eq!(s.spd, 40);
        assert_eq!(s.int, 25);
        assert!(!s.action_blocked);
        assert!(!s.magic_blocked);
    }

    #[test]
    fn equipment_spd_and_int_add_but_not_acc_eva() {
        let mut t = EquipmentTable::new();
        // Footwear SPD bonus in slot id 1, head-gear INT bonus in slot id 2.
        t.set(
            1,
            ItemModifier {
                spd: 5,
                ..ItemModifier::default()
            },
        );
        t.set(
            2,
            ItemModifier {
                int: 8,
                ..ItemModifier::default()
            },
        );
        let s = compute_battle_stats_default(&record(), &t, &[]);
        assert_eq!(s.spd, 45); // 40 + 5
        assert_eq!(s.int, 33); // 25 + 8
        // Equipment never moves the derived accuracy / evasion lines.
        assert_eq!(s.acc, 90);
        assert_eq!(s.eva, 30);
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
            compute_battle_stats_default(&record(), &EquipmentTable::new(), &[StatusKind::Toxic]);
        assert_eq!(s.atk, 88); // 100 * 0.875 = 87.5 -> 88 (rounded)
    }

    #[test]
    fn confuse_halves_accuracy() {
        let s =
            compute_battle_stats_default(&record(), &EquipmentTable::new(), &[StatusKind::Confuse]);
        assert_eq!(s.acc, 45);
    }

    #[test]
    fn sleep_zeros_evasion_and_blocks_actions() {
        let s =
            compute_battle_stats_default(&record(), &EquipmentTable::new(), &[StatusKind::Sleep]);
        assert_eq!(s.eva, 0);
        assert!(s.action_blocked);
    }

    #[test]
    fn silence_blocks_magic_only() {
        let s =
            compute_battle_stats_default(&record(), &EquipmentTable::new(), &[StatusKind::Curse]);
        assert!(s.magic_blocked);
        assert!(!s.action_blocked);
    }

    #[test]
    fn petrify_blocks_both_magic_and_actions() {
        let s =
            compute_battle_stats_default(&record(), &EquipmentTable::new(), &[StatusKind::Faint]);
        assert!(s.magic_blocked);
        assert!(s.action_blocked);
    }

    #[test]
    fn equipment_and_status_compose() {
        let mut t = EquipmentTable::new();
        t.set(1, weapon(40));
        t.set(2, armor(20, 25));
        let s = compute_battle_stats_default(&record(), &t, &[StatusKind::Toxic]);
        // Atk: 100 + 40 = 140; Toxic -> 140 * 0.875 = 122.5 -> 123 (rounded).
        assert_eq!(s.atk, 123);
        // UDF: 50 + 20 = 70; LDF: 60 + 25 = 85.
        assert_eq!(s.udf, 70);
        assert_eq!(s.ldf, 85);
    }

    #[test]
    fn custom_modifiers_let_engines_tune_severity() {
        let mods = StatusModifiers {
            toxic_atk_mult: 0.5, // Brutal toxic
            ..StatusModifiers::default()
        };
        let s = compute_battle_stats(
            &record(),
            &EquipmentTable::new(),
            &[StatusKind::Toxic],
            &mods,
        );
        assert_eq!(s.atk, 50);
    }

    #[test]
    fn action_blocked_takes_priority_when_multi_status() {
        let s = compute_battle_stats_default(
            &record(),
            &EquipmentTable::new(),
            &[StatusKind::Sleep, StatusKind::Toxic, StatusKind::Confuse],
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
