//! Equipment-aware battle stat aggregator.
//!
//! PORT: FUN_80042558
//!
//! Port of the per-frame stat resolver in
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
//!   bridges from [`legaia_engine_battle_vm::status_effects::StatusKind`].
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

use legaia_engine_battle_vm::status_effects::StatusKind;

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
/// `legaia_game_tables::equip_stats`; the `+0` byte is the INT bonus, `+4` the SPD
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
/// Retail scales the per-strike combat *rolls* by the poison status bits in
/// `FUN_801DD864` (bit 1 Venom → ×9/10, then bit 2 Toxic → ×7/10; the exact
/// kernel is `legaia_engine_battle_vm::battle_formulas::apply_status_weaken`). This
/// resolver mirrors those magnitudes at the stat line instead, which is
/// equivalent for the linear roll terms. The other entries (Confuse accuracy,
/// immobilised evasion, Curse) are engine models, exposed for override.
///
/// REF: FUN_801DD864 (the bit-1 / bit-2 roll scales this resolver mirrors;
/// the exact kernel port is `legaia_engine_battle_vm::battle_formulas::apply_status_weaken`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatusModifiers {
    /// Multiplier applied to ATK when the actor is Toxic. Default `0.7`
    /// (the `FUN_801DD864` bit-2 roll scale).
    pub toxic_atk_mult: f32,
    /// Multiplier applied to both defenses (UDF + LDF) when the actor is
    /// Toxic - retail scales the defender's roll by the same status bits, so
    /// the deadly poison weakens defense too. Default `0.7`.
    pub toxic_def_mult: f32,
    /// Multiplier applied to ATK when the actor is Venomed. Default `0.9`
    /// (the `FUN_801DD864` bit-1 roll scale). Stacks with Toxic when both
    /// are set, matching the sequential retail scale (×9/10 then ×7/10).
    pub venom_atk_mult: f32,
    /// Multiplier applied to both defenses (UDF + LDF) when the actor is
    /// Venomed. Default `0.9`.
    pub venom_def_mult: f32,
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
            toxic_atk_mult: 0.7,
            toxic_def_mult: 0.7,
            venom_atk_mult: 0.9,
            venom_def_mult: 0.9,
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
/// the port-side equivalent, with one deliberate difference: it walks all
/// eight `record.equip` bytes, because the same body is also the battle-side
/// aggregator `FUN_80042558`, which walks eight. On retail data the tail is
/// inert here - the three Goods bytes are item-class `2` ids, absent from
/// [`EquipmentTable`], which indexes class-`1` ids only - and the menu's own
/// five-slot walk (`slti a2, 5` at `0x801CF744`) is `menu_stat_block` in
/// `pause_screens`, which zeroes that tail. It looks each id up in [`EquipmentTable`] (analogue
/// of the `0x80074F68` bonus row), and accumulates the modifiers into
/// [`BattleStats`]. Accuracy / evasion are derived from AGL upstream and are
/// not equipment-fed, so the equipment loop leaves them alone. It also folds
/// status-effect multipliers, which the SCUS pass leaves to the battle-side
/// kernels in `FUN_801EC3E4`. (The town-overlay alias at the same address -
/// emitter ramp-actor allocator - is a separate function; see
/// `docs/reference/functions.md`.)
pub fn compute_battle_stats(
    record: &StatRecord,
    table: &EquipmentTable,
    statuses: &[StatusKind],
    modifiers: &StatusModifiers,
) -> BattleStats {
    compute_battle_stats_inner(record, table, None, statuses, modifiers)
}

/// [`compute_battle_stats`] plus the accessory ("Goods") passive-effect arms
/// of the retail aggregator.
///
/// PORT: FUN_80042558
///
/// Mirrors the two passive arms of `FUN_80042558`
/// (`ghidra/scripts/funcs/80042558.txt`) exactly:
///
/// 1. **Bit resolution** - every equipped item's passive index (`< 0x40`)
///    becomes a bit in the resolved ability mask
///    ([`crate::accessory_passives::AccessoryPassives::bits_for_equipment`]),
///    OR'd into [`BattleStats::abilities`] bytes `0..16` (the engine mirror of
///    the record `+0xF4` 4×u32 bitfield).
/// 2. **Percent stat rebuild** - for each set boost index the matching stat
///    line gains `base / divisor` (`legaia_game_tables::accessory_passive::stat_boosts`:
///    divisor `10` = +10%, `4` = +25%, `5` = +20%), where `base` is the
///    **base** stat (`StatRecord::base_*` - retail reads the `+0x11C..` base
///    window, *not* the post-equipment value, so the percent magnitude is
///    independent of flat equipment bonuses) with unsigned truncating
///    division (retail's `mulhi 0xCCCCCCCD >> 3` = `/10`, `>> 2` = `/4`,
///    `mulhi >> 2` = `/5`). The Agility boost (`0x0C`) feeds both AGL-derived
///    lines (`acc` / `eva`), matching the retail effective-AGL slot serving
///    both rolls.
///
/// After the boosts the retail clamp block applies: ATK / UDF / LDF / SPD /
/// INT cap at `999`, the AGL-derived lines at `0x118` (= 280). (Retail also
/// caps max HP at 9999 / max MP at 999; [`BattleStats`] carries no HP / MP
/// lines - `World::seed_party_battle_stats` applies the max-HP boost + cap to
/// the live battle actor.)
///
/// The boosts derive from the **wearer's own** equipment bits only, exactly
/// as retail (the global party mask at `DAT_80074358` is OR'd *after* the
/// stat rebuild and only feeds point-of-use party-wide consumers).
pub fn compute_battle_stats_with_passives(
    record: &StatRecord,
    table: &EquipmentTable,
    passives: &crate::accessory_passives::AccessoryPassives,
    statuses: &[StatusKind],
    modifiers: &StatusModifiers,
) -> BattleStats {
    compute_battle_stats_inner(record, table, Some(passives), statuses, modifiers)
}

fn compute_battle_stats_inner(
    record: &StatRecord,
    table: &EquipmentTable,
    passives: Option<&crate::accessory_passives::AccessoryPassives>,
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
    // Resolve the accessory passive bits up front (retail zeroes the +0xF4
    // bitfield and re-derives it inside the same equipment walk).
    let passive_words = passives.map(|p| p.bits_for_equipment(&record.equip));
    if let Some(words) = &passive_words {
        for (w, word) in words.iter().enumerate() {
            for b in 0..4 {
                stats.abilities[w * 4 + b] |= (word >> (8 * b)) as u8;
            }
        }
    }
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
    // Percent stat boosts from the wearer's own passive bits, then the retail
    // clamp block. The magnitude reads the BASE stat (truncating unsigned
    // division), exactly as `FUN_80042558` - see
    // [`compute_battle_stats_with_passives`].
    if let Some(words) = &passive_words {
        use legaia_game_tables::accessory_passive::{BoostedStat, stat_boosts};
        for idx in 0u8..legaia_game_tables::accessory_passive::NO_PASSIVE {
            let (w, mask) = legaia_game_tables::accessory_passive::bit_location(idx);
            if words[w] & mask == 0 {
                continue;
            }
            for &(stat, div) in stat_boosts(idx) {
                let div = div as u16;
                match stat {
                    BoostedStat::Attack => {
                        stats.atk = stats.atk.saturating_add(record.base_attack / div);
                    }
                    BoostedStat::DefenseUp => {
                        stats.udf = stats.udf.saturating_add(record.base_udf / div);
                    }
                    BoostedStat::DefenseDown => {
                        stats.ldf = stats.ldf.saturating_add(record.base_ldf / div);
                    }
                    BoostedStat::Speed => {
                        stats.spd = stats.spd.saturating_add(record.base_spd / div);
                    }
                    BoostedStat::Intelligence => {
                        stats.int = stats.int.saturating_add(record.base_int / div);
                    }
                    // The retail effective-AGL slot (+0x110) serves both the
                    // hit and dodge rolls; the engine splits it into acc/eva,
                    // so the AGL boost feeds both lines.
                    BoostedStat::Agility => {
                        stats.acc = stats.acc.saturating_add(record.base_accuracy / div);
                        stats.eva = stats.eva.saturating_add(record.base_evasion / div);
                    }
                    // No HP / MP lines on BattleStats; the max-HP boost is
                    // applied to the live battle actor by
                    // `World::seed_party_battle_stats`.
                    BoostedStat::MaxHp | BoostedStat::MaxMp => {}
                }
            }
        }
        // Retail clamp block (`FUN_80042558` tail): the five equipment-fed
        // lines cap at 999; the AGL-derived lines at 0x118 = 280.
        stats.atk = stats.atk.min(999);
        stats.udf = stats.udf.min(999);
        stats.ldf = stats.ldf.min(999);
        stats.spd = stats.spd.min(999);
        stats.int = stats.int.min(999);
        stats.acc = stats.acc.min(280);
        stats.eva = stats.eva.min(280);
    }
    // Fold status-effect modifiers.
    for &k in statuses {
        match k {
            StatusKind::Toxic => {
                stats.atk = mul_clamp(stats.atk, modifiers.toxic_atk_mult);
                // Toxic also drops defense (both UDF and LDF).
                stats.udf = mul_clamp(stats.udf, modifiers.toxic_def_mult);
                stats.ldf = mul_clamp(stats.ldf, modifiers.toxic_def_mult);
            }
            StatusKind::Venom => {
                stats.atk = mul_clamp(stats.atk, modifiers.venom_atk_mult);
                stats.udf = mul_clamp(stats.udf, modifiers.venom_def_mult);
                stats.ldf = mul_clamp(stats.ldf, modifiers.venom_def_mult);
            }
            // Rot disables a limb's attack command; it carries no stat fold.
            StatusKind::Rot => {}
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
mod tests;
