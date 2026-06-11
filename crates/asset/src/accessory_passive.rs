//! Accessory ("Goods") passive-effect table parser (`DAT_800752C3` index byte
//! + the passive name/description table at `0x8007625C`).
//!
//! Every accessory grants a **passive effect** while equipped - max-HP/MP
//! percent boosts, stat percent boosts, status-nullify guards, elemental
//! guards, AP/encounter/loot modifiers. The static data side is a **64-slot
//! passive-effect index space** (`0x00..=0x3F`): each index is one effect, and
//! an equipped item's index becomes a **bit position** in the per-character
//! 4x`u32` ability bitfield at char record `+0xF4..+0x103` (the same word the
//! engine's MP-cost ability bits `0x10`/`0x20` live in - see
//! `legaia_engine_vm::battle_formulas::mp_cost_after_ability_bits`).
//!
//! ## Indexing (Ghidra-traced)
//!
//! From the per-frame stat aggregator `FUN_80042558`
//! (`ghidra/scripts/funcs/80042558.txt`), which walks each active party
//! member's eight equipment-slot bytes at char `+0x196`:
//!
//! ```text
//! kind = item_table[id].byte(+0)                  // DAT_80074368[id*0xC]
//! if kind == 1: index = equip_bonus[sub].byte(+5) // DAT_80074F6D[sub*8]
//! if kind == 2: index = descriptor[sub].byte(+3)  // DAT_800752C3[sub*4]
//! if index < 0x40:
//!     char[+0xF4 + (index>>5)*4] |= 1 << (index & 0x1F)
//! ```
//!
//! `0x40`+ is the **no-passive sentinel**: every retail equipment row carries
//! `+5 = 0x40` and every consumable descriptor carries `+3 = 0x41` (the bytes
//! previously documented as "constant `0x40`" / "constant `0x41` (`'A'`)
//! marker" in [`crate::equip_stats`] / [`crate::item_effect`]). Only
//! accessory + quest-item descriptor rows carry a live index, so in retail
//! the bitfield is driven purely by accessories.
//!
//! `FUN_80042558` then ORs all three members' bitfields into the global
//! 4x`u32` ability mask at `DAT_80074358` (bit-tested by `FUN_800431D0`),
//! which is how the party-wide passives (gold/XP boosts, encounter rate,
//! escape modifiers) are consumed.
//!
//! ## Stat-boost magnitudes (pinned from `FUN_80042558`)
//!
//! The percent stat boosts are applied **inline** in the aggregator when it
//! rebuilds the effective stat block (`+0x104..`) from the base stats
//! (`+0x11C..`); there is no separate magnitude table:
//!
//! | Index | Effect | Arithmetic |
//! |---|---|---|
//! | `0x00` | max HP +10% | `+ base/10` |
//! | `0x01` | max HP +25% | `+ base>>2` |
//! | `0x02` | max MP +10% | `+ base/10` |
//! | `0x03` | max MP +25% | `+ base>>2` |
//! | `0x06` | ATK +20% | `+ base/5` |
//! | `0x07` | UDF +20% | `+ base/5` |
//! | `0x08` | LDF +20% | `+ base/5` |
//! | `0x09` | UDF & LDF +20% | `+ base/5` each |
//! | `0x0A` | SPD +20% | `+ base/5` |
//! | `0x0B` | INT +20% | `+ base/5` |
//! | `0x0C` | AGL +20% | `+ base/5` (AGL capped at `0x118` = 280) |
//!
//! The remaining indices are point-of-use flags: the consumer tests the
//! bitfield bit where the mechanic lives (MP cost at cast time, status/
//! elemental guards in the battle damage path, encounter rate in the field
//! step roll, loot in the battle-end reward resolver, ...).
//!
//! ## Passive name/description table (`0x8007625C`)
//!
//! A static 64-record table gives each index its menu name + effect
//! description (the text the Goods menu shows), plus a scope word:
//!
//! | Offset | Type | Field |
//! |---|---|---|
//! | `+0` | u32 | scope: `1` = party-wide (one wearer benefits the party), `0` = wearer-only |
//! | `+4` | u32 | pointer to the effect **name** (e.g. `"HP Boost 1"`) |
//! | `+8` | u32 | pointer to the effect **description** (e.g. `"Increase max HP 10%"`; `0x7C` `'|'` = line break) |
//!
//! Read by the menu overlay's Goods detail panel `FUN_801D0F1C`
//! (`ghidra/scripts/funcs/overlay_menu_801d0f1c.txt`, both pointers) and the
//! static description resolver `FUN_80034250`
//! (`ghidra/scripts/funcs/80034250.txt`, `+8`), each indexing by the same
//! descriptor-`+3` byte with the same `< 0x40` gate.
//!
//! ## Provenance + parser
//!
//! Indexing traced from `ghidra/scripts/funcs/80042558.txt` (static
//! `SCUS_942.54`, no overlay needed). The resolver mirrors the same
//! `t_addr -> file-offset` map as [`crate::item_names`] /
//! [`crate::equip_stats`]. The disc-gated `accessory_passive_real` test pins
//! the per-accessory indices + table text against the real executable, and
//! `legaia-gamedata`'s `accessory_passives_vs_disc` cross-validates every
//! curated accessory effect class against its decoded index.

/// RAM address of the passive name/description table.
pub const PASSIVE_TABLE_VA: u32 = 0x8007_625C;
/// Per-record stride of the passive name/description table.
pub const PASSIVE_STRIDE: usize = 0xC;
/// Number of passive-effect indices (`0x00..=0x3F`).
pub const PASSIVE_COUNT: usize = 0x40;
/// First sentinel value: an index byte `>= 0x40` grants no passive.
pub const NO_PASSIVE: u8 = 0x40;

/// Item property record base (`DAT_80074368`): `+0` kind, `+1` subtype.
const ITEM_TABLE_BASE_VA: u32 = 0x8007_4368;
/// Item property record stride.
const ITEM_RECORD_STRIDE: u32 = 0x0C;
/// Number of item ids.
const ITEM_COUNT: usize = 256;
/// Item-effect descriptor base (`DAT_800752C0`); the passive index for
/// `kind == 2` items is the `+3` byte.
const DESCRIPTOR_BASE_VA: u32 = 0x8007_52C0;
/// Descriptor record count (subtypes `0x00..=0x81`).
const DESCRIPTOR_COUNT: usize = 130;
/// Equip stat-bonus base (`DAT_80074F68`); the passive index for `kind == 1`
/// items is the `+5` byte (retail rows all carry the `0x40` sentinel).
const EQUIP_BONUS_BASE_VA: u32 = 0x8007_4F68;
/// Equip stat-bonus record stride.
const EQUIP_BONUS_STRIDE: u32 = 8;

/// A stat targeted by one of the aggregator-applied percent boosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoostedStat {
    /// Max HP (effective slot char `+0x104`, base `+0x11C`).
    MaxHp,
    /// Max MP (effective slot char `+0x108`, base `+0x11E`).
    MaxMp,
    /// Attack (`+0x112` / base `+0x124`).
    Attack,
    /// Upper defense (`+0x114` / base `+0x126`).
    DefenseUp,
    /// Lower defense (`+0x116` / base `+0x128`).
    DefenseDown,
    /// Speed (`+0x118` / base `+0x12A`).
    Speed,
    /// Intelligence (`+0x11A` / base `+0x12C`).
    Intelligence,
    /// Agility (`+0x110` / base `+0x122`, capped at 280).
    Agility,
}

/// The stat boosts `FUN_80042558` applies for a passive index, as
/// `(stat, divisor)` pairs: the effective stat gains `base / divisor`
/// (divisor `10` = +10%, `4` = +25%, `5` = +20%). Empty for indices the
/// aggregator does not apply inline (point-of-use flags).
pub fn stat_boosts(index: u8) -> &'static [(BoostedStat, u32)] {
    use BoostedStat::*;
    match index {
        0x00 => &[(MaxHp, 10)],
        0x01 => &[(MaxHp, 4)],
        0x02 => &[(MaxMp, 10)],
        0x03 => &[(MaxMp, 4)],
        0x06 => &[(Attack, 5)],
        0x07 => &[(DefenseUp, 5)],
        0x08 => &[(DefenseDown, 5)],
        0x09 => &[(DefenseUp, 5), (DefenseDown, 5)],
        0x0A => &[(Speed, 5)],
        0x0B => &[(Intelligence, 5)],
        0x0C => &[(Agility, 5)],
        _ => &[],
    }
}

/// The `(word, bit)` position a passive index occupies in the per-character
/// `+0xF4` ability bitfield (and the global `DAT_80074358` mask):
/// `word = index >> 5`, `bit = index & 0x1F`.
pub fn bit_location(index: u8) -> (usize, u32) {
    ((index >> 5) as usize, 1 << (index & 0x1F))
}

/// Well-known passive indices, named after the on-disc effect-name strings
/// (`0x8007625C` record `+4`). The full 64-slot map lives in
/// `docs/formats/accessory-passive-table.md`.
pub mod index {
    /// "HP Boost 1" - max HP +10% (Life Ring, Mei's Pendant).
    pub const HP_BOOST_1: u8 = 0x00;
    /// "HP Boost 2" - max HP +25% (Life Armband, Minea's Ring).
    pub const HP_BOOST_2: u8 = 0x01;
    /// "MP Boost 1" - max MP +10% (Magic Ring, Yuma's Ring).
    pub const MP_BOOST_1: u8 = 0x02;
    /// "MP Boost 2" - max MP +25% (Magic Armband).
    pub const MP_BOOST_2: u8 = 0x03;
    /// "MP Used Down 1" - consume 25% less MP (Spirit Jewel).
    pub const MP_USED_DOWN_1: u8 = 0x04;
    /// "MP Used Down 2" - consume 50% less MP (Spirit Talisman).
    pub const MP_USED_DOWN_2: u8 = 0x05;
    /// "Steal Attack" - steal items when attacking (Evil God Icon).
    pub const STEAL_ATTACK: u8 = 0x10;
    /// "Master Guard" - nullify all abnormal status (Wonder Amulet).
    pub const MASTER_GUARD: u8 = 0x1C;
    /// "Earth Guard" - first of the seven elemental guards
    /// (Earth `0x1D`, Water `0x1E`, Fire `0x1F`, Wind `0x20`,
    /// Thunder `0x21`, Light `0x22`, Dark `0x23`).
    pub const ELEMENTAL_GUARD_FIRST: u8 = 0x1D;
    /// "All Guard" - defense against all elements (Rainbow Jewel).
    pub const ALL_GUARD: u8 = 0x24;
}

/// One record of the passive name/description table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassiveRecord {
    /// Raw scope word (`+0`).
    pub scope_raw: u32,
    /// Effect name (`+4` pointer), e.g. `"HP Boost 1"`.
    pub name: Option<String>,
    /// Effect description (`+8` pointer); `'|'` is the retail line break.
    pub description: Option<String>,
}

impl PassiveRecord {
    /// `true` when one equipped copy benefits the whole party (scope `1`):
    /// the battle-end / encounter / escape modifiers.
    pub fn party_wide(&self) -> bool {
        self.scope_raw == 1
    }
}

/// PSX-EXE `t_addr` -> file-offset resolver (same shape as
/// [`crate::item_names`] / [`crate::equip_stats`]).
struct ExeMap {
    t_addr: u32,
    t_size: u32,
}

impl ExeMap {
    fn parse(scus: &[u8]) -> Option<Self> {
        if scus.len() < 0x800 || &scus[0..8] != b"PS-X EXE" {
            return None;
        }
        let t_addr = u32::from_le_bytes(scus[0x18..0x1C].try_into().ok()?);
        let t_size = u32::from_le_bytes(scus[0x1C..0x20].try_into().ok()?);
        Some(Self { t_addr, t_size })
    }

    fn off(&self, va: u32) -> Option<usize> {
        if va < self.t_addr || va >= self.t_addr.checked_add(self.t_size)? {
            return None;
        }
        Some((va - self.t_addr) as usize + 0x800)
    }
}

/// Read a NUL-terminated MES-style string: strip `0x01` icon escapes and
/// `0xCE XX` colour controls, keep printable ASCII, trim whitespace.
fn read_text(scus: &[u8], map: &ExeMap, va: u32) -> Option<String> {
    let start = map.off(va)?;
    let mut out = String::new();
    let mut i = start;
    while i < scus.len() {
        let b = scus[i];
        if b == 0 {
            break;
        }
        if b == 0x01 {
            i += 1;
            continue;
        }
        if b == 0xCE {
            i += 2;
            continue;
        }
        if (0x20..0x7F).contains(&b) {
            out.push(b as char);
        }
        i += 1;
    }
    let trimmed = out.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// The resolved accessory passive-effect table: per item id the kind +
/// subtype, the per-subtype / per-equip-row passive index bytes, and the 64
/// passive name/description records.
#[derive(Debug, Clone)]
pub struct AccessoryPassiveTable {
    /// `kind[id]` - item property table `+0` byte.
    kind: Vec<u8>,
    /// `subtype[id]` - item property table `+1` byte.
    subtype: Vec<u8>,
    /// `descriptor[sub].byte(+3)` for subtypes `0x00..DESCRIPTOR_COUNT`.
    descriptor_passive: Vec<u8>,
    /// `equip_bonus[sub].byte(+5)` for the equip rows the equippable ids reach.
    equip_passive: Vec<u8>,
    /// Passive records `0x00..=0x3F`.
    records: Vec<PassiveRecord>,
}

impl AccessoryPassiveTable {
    /// Resolve the tables out of a `SCUS_942.54` image. Returns `None` if the
    /// input isn't a PS-X EXE or a table falls outside the data segment.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let map = ExeMap::parse(scus)?;

        let mut kind = Vec::with_capacity(ITEM_COUNT);
        let mut subtype = Vec::with_capacity(ITEM_COUNT);
        for id in 0..ITEM_COUNT {
            let base = map.off(ITEM_TABLE_BASE_VA + (id as u32) * ITEM_RECORD_STRIDE)?;
            kind.push(*scus.get(base)?);
            subtype.push(*scus.get(base + 1)?);
        }

        let mut descriptor_passive = Vec::with_capacity(DESCRIPTOR_COUNT);
        for sub in 0..DESCRIPTOR_COUNT {
            let off = map.off(DESCRIPTOR_BASE_VA + (sub as u32) * 4)?;
            descriptor_passive.push(*scus.get(off + 3)?);
        }

        // Equip rows: only the rows equippable ids reach (same extent rule as
        // `equip_stats`).
        let max_equip = kind
            .iter()
            .zip(&subtype)
            .filter(|(k, _)| **k == 1)
            .map(|(_, s)| *s as usize)
            .max()
            .unwrap_or(0);
        let mut equip_passive = Vec::with_capacity(max_equip + 1);
        for row in 0..=max_equip {
            let off = map.off(EQUIP_BONUS_BASE_VA + (row as u32) * EQUIP_BONUS_STRIDE)?;
            equip_passive.push(*scus.get(off + 5)?);
        }

        let mut records = Vec::with_capacity(PASSIVE_COUNT);
        for idx in 0..PASSIVE_COUNT {
            let off = map.off(PASSIVE_TABLE_VA + (idx * PASSIVE_STRIDE) as u32)?;
            let rec = scus.get(off..off + PASSIVE_STRIDE)?;
            let scope_raw = u32::from_le_bytes(rec[0..4].try_into().ok()?);
            let name_ptr = u32::from_le_bytes(rec[4..8].try_into().ok()?);
            let desc_ptr = u32::from_le_bytes(rec[8..12].try_into().ok()?);
            records.push(PassiveRecord {
                scope_raw,
                name: read_text(scus, &map, name_ptr),
                description: read_text(scus, &map, desc_ptr),
            });
        }

        Some(Self {
            kind,
            subtype,
            descriptor_passive,
            equip_passive,
            records,
        })
    }

    /// The passive-effect index an equipped item id grants, mirroring the
    /// `FUN_80042558` gate exactly: `kind == 1` reads the equip-bonus `+5`
    /// byte, `kind == 2` the descriptor `+3` byte; values `>= 0x40` (the
    /// sentinels) yield `None`.
    pub fn passive_index(&self, id: u8) -> Option<u8> {
        let sub = self.subtype[id as usize] as usize;
        let raw = match self.kind[id as usize] {
            1 => *self.equip_passive.get(sub)?,
            2 => *self.descriptor_passive.get(sub)?,
            _ => return None,
        };
        (raw < NO_PASSIVE).then_some(raw)
    }

    /// The passive record for an index `0x00..=0x3F`.
    pub fn record(&self, index: u8) -> Option<&PassiveRecord> {
        self.records.get(index as usize)
    }

    /// The `(index, record)` pair for an item id, or `None` if the item
    /// grants no passive.
    pub fn passive(&self, id: u8) -> Option<(u8, &PassiveRecord)> {
        let idx = self.passive_index(id)?;
        Some((idx, self.records.get(idx as usize)?))
    }

    /// Number of passive records parsed (always [`PASSIVE_COUNT`]).
    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    /// Raw equip-bonus `+5` bytes (index = equip subtype). In retail every
    /// row is the `0x40` sentinel - no equipment grants a passive; the
    /// `kind == 1` arm of `FUN_80042558` is latent.
    pub fn equip_passive_bytes(&self) -> &[u8] {
        &self.equip_passive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T_ADDR: u32 = 0x8001_0000;

    /// Build a synthetic PS-X EXE holding the four tables + strings.
    fn synth_scus() -> Vec<u8> {
        let t_size = 0x6_8000u32;
        let mut img = vec![0u8; 0x800 + t_size as usize];
        img[0..8].copy_from_slice(b"PS-X EXE");
        img[0x18..0x1C].copy_from_slice(&T_ADDR.to_le_bytes());
        img[0x1C..0x20].copy_from_slice(&t_size.to_le_bytes());
        let off = |va: u32| (va - T_ADDR) as usize + 0x800;

        // Item 0xC0: kind 2, subtype 0x40 (an accessory).
        let it = off(ITEM_TABLE_BASE_VA) + 0xC0 * 0xC;
        img[it] = 2;
        img[it + 1] = 0x40;
        // Item 0x22: kind 1, subtype 0x21 (a weapon, sentinel +5).
        let it = off(ITEM_TABLE_BASE_VA) + 0x22 * 0xC;
        img[it] = 1;
        img[it + 1] = 0x21;
        // Item 0x77: kind 2, subtype 0x00 (a consumable, sentinel +3).
        let it = off(ITEM_TABLE_BASE_VA) + 0x77 * 0xC;
        img[it] = 2;
        img[it + 1] = 0x00;

        // Descriptors: sub 0x40 -> passive 0x00; sub 0x00 -> sentinel 0x41.
        let d = off(DESCRIPTOR_BASE_VA);
        img[d + 0x40 * 4 + 3] = 0x00;
        img[d + 3] = 0x41;
        // Equip row 0x21: +5 sentinel.
        let e = off(EQUIP_BONUS_BASE_VA) + 0x21 * 8;
        img[e + 5] = 0x40;

        // Passive record 0: scope 0, name + description strings.
        let name_va = 0x8006_F000u32;
        let desc_va = 0x8006_F010u32;
        let p = off(PASSIVE_TABLE_VA);
        img[p..p + 4].copy_from_slice(&0u32.to_le_bytes());
        img[p + 4..p + 8].copy_from_slice(&name_va.to_le_bytes());
        img[p + 8..p + 12].copy_from_slice(&desc_va.to_le_bytes());
        img[off(name_va)..off(name_va) + 10].copy_from_slice(b"HP Boost 1");
        let d = b"Increase max HP 10%";
        img[off(desc_va)..off(desc_va) + d.len()].copy_from_slice(d);
        // Passive record 1: party-wide scope, no strings.
        let p1 = p + PASSIVE_STRIDE;
        img[p1..p1 + 4].copy_from_slice(&1u32.to_le_bytes());
        img
    }

    #[test]
    fn accessory_resolves_descriptor_plus_3() {
        let scus = synth_scus();
        let t = AccessoryPassiveTable::from_scus(&scus).unwrap();
        assert_eq!(t.passive_index(0xC0), Some(0x00));
        let (idx, rec) = t.passive(0xC0).unwrap();
        assert_eq!(idx, 0x00);
        assert_eq!(rec.name.as_deref(), Some("HP Boost 1"));
        assert_eq!(rec.description.as_deref(), Some("Increase max HP 10%"));
        assert!(!rec.party_wide());
        assert!(t.record(0x01).unwrap().party_wide());
    }

    #[test]
    fn sentinels_grant_no_passive() {
        let scus = synth_scus();
        let t = AccessoryPassiveTable::from_scus(&scus).unwrap();
        // Weapon: equip +5 == 0x40 sentinel.
        assert_eq!(t.passive_index(0x22), None);
        // Consumable: descriptor +3 == 0x41 sentinel.
        assert_eq!(t.passive_index(0x77), None);
        // kind 0 (no item).
        assert_eq!(t.passive_index(0x00), None);
    }

    #[test]
    fn stat_boost_arithmetic_matches_fun_80042558() {
        use BoostedStat::*;
        assert_eq!(stat_boosts(0x00), &[(MaxHp, 10)]);
        assert_eq!(stat_boosts(0x01), &[(MaxHp, 4)]);
        assert_eq!(stat_boosts(0x09), &[(DefenseUp, 5), (DefenseDown, 5)]);
        assert_eq!(stat_boosts(0x0C), &[(Agility, 5)]);
        // MP-cost reductions are cast-time flags, not aggregator boosts.
        assert!(stat_boosts(0x04).is_empty());
        assert!(stat_boosts(0x10).is_empty());
    }

    #[test]
    fn bit_location_splits_word_and_bit() {
        assert_eq!(bit_location(0x00), (0, 0x1));
        assert_eq!(bit_location(0x04), (0, 0x10)); // MP Used Down 1
        assert_eq!(bit_location(0x05), (0, 0x20)); // MP Used Down 2
        assert_eq!(bit_location(0x1F), (0, 0x8000_0000));
        assert_eq!(bit_location(0x20), (1, 0x1));
        assert_eq!(bit_location(0x3F), (1, 0x8000_0000));
    }

    #[test]
    fn non_psx_exe_returns_none() {
        assert!(AccessoryPassiveTable::from_scus(b"not an exe").is_none());
        assert!(AccessoryPassiveTable::from_scus(&[0u8; 0x900]).is_none());
    }
}
