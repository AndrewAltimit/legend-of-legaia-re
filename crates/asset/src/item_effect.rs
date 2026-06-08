//! Item-effect descriptor table parser (`DAT_800752C0` in `SCUS_942.54`).
//!
//! This is the static table the item-use code reads to decide *what kind* of
//! effect a consumable has (heal HP, restore MP, cure status, revive, raise a
//! stat, field escape, ...), its target shape (single ally vs. whole party),
//! and where it may be used (field menu vs. battle). It is a sibling of the
//! [item-name table](crate::item_names) (`DAT_80074368`) and the
//! [spell table](../../docs/formats/spell-table.md) - the three are contiguous
//! static `SCUS_942.54` data (`item-name @ 0x80074368`, this table
//! `@ 0x800752C0`, spell table `@ 0x800754C8`, which is exactly where this
//! table ends).
//!
//! ## What this table is and is NOT
//!
//! It holds the effect *class + tier + flags*, **not** the literal restore
//! amounts. "Healing Leaf restores 200 HP" is split: this table says
//! `(class = heal-HP, tier = 0)`, and the `(class, tier) -> 200` mapping is a
//! `switch` inside the (overlay-resident, undumped) item-use *apply* handler.
//! So the numeric heal/restore amounts are **not** recoverable from this table
//! alone - the engine keeps curated walkthrough amounts for those (see
//! `legaia_engine_core::items`). What this table *does* give faithfully is the
//! effect class, the per-class tier selector, the all-party flag, and the
//! field-vs-battle usability flags.
//!
//! ## Indexing (Ghidra-traced)
//!
//! The lookup is **double-indirected by item id -> subtype -> descriptor**.
//! From `FUN_8003043c` (`ghidra/scripts/funcs/8003043c.txt`):
//!
//! ```text
//! subtype    = item_name_table[id].byte(+1)          // DAT_80074369[id*0xC]
//! descriptor = (&DAT_800752C0)[subtype * 4]          // stride-4 record
//! arm        = descriptor[+0]   // effect class / action-validator arm
//! tier       = descriptor[+1]   // per-class sub-case selector
//! flags      = descriptor[+2]   // 0x80 base | 0x20 all-party | 0x04 battle | 0x02 field
//! marker     = descriptor[+3]   // 0x41 ('A') consumable-effect marker
//! ```
//!
//! The same subtype byte feeds the field item-menu list builder
//! (`FUN_80030628`), which is where the `0x02`/`0x04` usability bits are read
//! (two menu contexts gate on `flags & 2` and `flags & 4` respectively), and
//! the all-party bit `0x20` is read in `FUN_8003043c` itself (`& 0x20` selects
//! the all-targets validator call).
//!
//! ## Record layout (4 bytes, stride `0x4`)
//!
//! | Offset | Type | Field |
//! |---|---|---|
//! | `+0` | u8 | effect **class** (action-validator arm) - see [`ItemEffectCategory`] |
//! | `+1` | u8 | **tier** / sub-case (per-class selector; e.g. heal-HP 0/1/2 = 200/800/max) |
//! | `+2` | u8 | **flags**: `0x80` base, `0x20` all-party, `0x04` battle-usable, `0x02` field-usable |
//! | `+3` | u8 | constant `0x41` (`'A'`) marker across consumable-effect rows |
//!
//! The table spans subtypes `0x00..=0x81` (130 records, `0x208` bytes) and
//! ends exactly at the spell table (`0x800754C8`).
//!
//! ## Provenance
//!
//! Indexing + flag reads traced from `ghidra/scripts/funcs/8003043c.txt` and
//! `ghidra/scripts/funcs/80030628.txt`. Class labels validated against the
//! on-disc item *description* strings (item record `+8` pointer): e.g. class 0
//! items read "Recover NHP. Ally.", class 1 "...All allies.", class 2
//! "Recover NMP.", class 4 "Restore life.", class 128 "Teleport out of
//! dungeons.". The `legaia_asset::item_effect` resolver mirrors the same
//! `t_addr -> file-offset` map as [`crate::item_names`].

/// RAM address of the effect-descriptor table (`DAT_800752C0`).
pub const TABLE_VA: u32 = 0x8007_52C0;
/// Per-subtype stride in bytes.
pub const RECORD_STRIDE: usize = 4;
/// Number of descriptor records (subtypes `0x00..=0x81`); the table ends at the
/// spell table (`0x800754C8`).
pub const RECORD_COUNT: usize = 130;

/// Item-name record base (`DAT_80074368`); the **subtype** byte is at `+1`.
const ITEM_TABLE_BASE_VA: u32 = 0x8007_4368;
/// Item-name record stride.
const ITEM_RECORD_STRIDE: u32 = 0x0C;
/// Number of item ids.
const ITEM_COUNT: usize = 256;

/// Flag bit: set on every populated descriptor (base / "has an effect").
pub const FLAG_BASE: u8 = 0x80;
/// Flag bit: the effect applies to the **whole party** (all-targets validator).
pub const FLAG_ALL_PARTY: u8 = 0x20;
/// Flag bit: the item is usable from the **battle** item menu.
pub const FLAG_BATTLE_USABLE: u8 = 0x04;
/// Flag bit: the item is usable from the **field** item menu.
pub const FLAG_FIELD_USABLE: u8 = 0x02;

/// RAM address of the **heal-amount table** the apply handler `FUN_800402F4`
/// (jump table `0x80014FA0`, indexed by descriptor class) reads to size a
/// restore. Two contiguous `u16[4]` sub-tables, tier-indexed (`base + tier*2`),
/// only tiers `0..=2` read; tier `3+` falls through to the character-relative
/// Seru-heal path (the `0x80084140`-based per-character spell tables) instead:
///
/// - `+0x00` (this VA): **HP** restore caps — `[200, 800, 9999, 0]`. The single-
///   target HP heal (class `0`) and the all-party HP heal (class `1`) both read
///   it; tier `2` (`9999`) is an effective full restore.
/// - `+0x08` (`0x80076564`): **MP** restore caps — `[50, 200, 20, 0]`, read by
///   the MP heal (class `2`).
///
/// Each restore is `min(max - current, table[tier])` (deficit-clamped). Pinned
/// from `ghidra/scripts/funcs/800402f4.txt` (HP arm `0x800404b8`, MP arm
/// `0x80040dc0`); the amounts are **static `SCUS_942.54` data**, not an
/// overlay-resident immediate switch.
pub const HEAL_AMOUNT_TABLE_VA: u32 = 0x8007_655C;
/// Byte offset from [`HEAL_AMOUNT_TABLE_VA`] to the MP sub-table.
pub const MP_SUBTABLE_OFFSET: u32 = 0x08;
/// Number of tier entries read per heal sub-table (tier `0..=2`; tier `3+` is
/// character-relative, not a flat amount).
pub const HEAL_TIER_COUNT: usize = 3;

/// PSX-EXE `t_addr` -> file-offset resolver (`SCUS_942.54` loads its data
/// segment at `t_addr` from file offset `0x800`). Same shape as the resolver in
/// [`crate::item_names`].
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

/// The validated effect-class buckets. The raw class byte is always available
/// on [`ItemEffect::class`]; this is the engine-relevant categorisation, with
/// every variant grounded in an on-disc item description (see module docs).
/// Niche classes that differ only by a parameter the class byte encodes
/// directly (the three arts books, the two summon flutes) collapse to one
/// variant each - read [`ItemEffect::class`] for the exact byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemEffectCategory {
    /// Class 0: restore HP to one ally ("Recover NHP. Ally."). Tier selects the
    /// amount (0/1/2 = 200/800/max). Note: key items with no usable effect also
    /// funnel to class 0 - gate on [`ItemEffect::is_usable_consumable`].
    HealHp,
    /// Class 1: restore HP to the whole party ("...All allies.").
    HealHpAllParty,
    /// Class 2: restore MP to one ally ("Recover NMP. Ally.").
    HealMp,
    /// Class 3: cure all status ("Cure all status. Ally.").
    CureAllStatus,
    /// Class 4: revive a fallen ally ("Restore life. Ally.").
    Revive,
    /// Class 5: extend the action gauge for one battle ("Fury Boost").
    ActionGaugeExtend,
    /// Class 6: permanently raise a stat ("All stats +4." / "Increase all
    /// stats."). Tier selects which stat.
    StatUp,
    /// Class 7: temporary one-battle stat buff ("Increase X for one battle.").
    /// Tier selects which stat.
    StatBuffOneBattle,
    /// Class 8: cure a single status ("Cure Venom. Ally.").
    CureStatus,
    /// Classes 11/12/13: arts book (Fire/Wind/Thunder "Book of Hyper Arts.").
    /// The class byte encodes the element; the tier encodes the book level.
    ArtsBook,
    /// Classes 126/127: summon flute ("Flute that calls the X monster.").
    SummonFlute,
    /// Class 128: field-only escape from a dungeon ("Teleport out of dungeons.").
    FieldEscapeDungeon,
    /// Class 129: field-only warp to a city ("Teleport to another city.").
    FieldWarpCity,
    /// Class 130: reduce the encounter rate ("Incense").
    ReduceEncounter,
    /// Any class byte not in the validated set above.
    Other(u8),
}

/// One 4-byte effect descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemEffect {
    /// `+0` effect class / action-validator arm.
    pub class: u8,
    /// `+1` per-class tier / sub-case selector.
    pub tier: u8,
    /// `+2` flag byte (`0x80` base | `0x20` all-party | `0x04` battle | `0x02` field).
    pub flags: u8,
    /// `+3` marker byte (`0x41` `'A'` on consumable-effect rows).
    pub marker: u8,
}

impl ItemEffect {
    /// `true` if the effect targets the whole party.
    pub fn all_party(&self) -> bool {
        self.flags & FLAG_ALL_PARTY != 0
    }

    /// `true` if usable from the field item menu.
    pub fn field_usable(&self) -> bool {
        self.flags & FLAG_FIELD_USABLE != 0
    }

    /// `true` if usable from the battle item menu.
    pub fn battle_usable(&self) -> bool {
        self.flags & FLAG_BATTLE_USABLE != 0
    }

    /// `true` if this is a usable consumable (usable in field and/or battle).
    /// Key items and equipment-as-item rows resolve to a descriptor with
    /// neither usability bit set.
    pub fn is_usable_consumable(&self) -> bool {
        self.field_usable() || self.battle_usable()
    }

    /// Validated effect category for the raw [`Self::class`] byte.
    pub fn category(&self) -> ItemEffectCategory {
        match self.class {
            0 => ItemEffectCategory::HealHp,
            1 => ItemEffectCategory::HealHpAllParty,
            2 => ItemEffectCategory::HealMp,
            3 => ItemEffectCategory::CureAllStatus,
            4 => ItemEffectCategory::Revive,
            5 => ItemEffectCategory::ActionGaugeExtend,
            6 => ItemEffectCategory::StatUp,
            7 => ItemEffectCategory::StatBuffOneBattle,
            8 => ItemEffectCategory::CureStatus,
            11..=13 => ItemEffectCategory::ArtsBook,
            126 | 127 => ItemEffectCategory::SummonFlute,
            128 => ItemEffectCategory::FieldEscapeDungeon,
            129 => ItemEffectCategory::FieldWarpCity,
            130 => ItemEffectCategory::ReduceEncounter,
            other => ItemEffectCategory::Other(other),
        }
    }
}

/// The resolved literal restore amount for a `(class, tier)` consumable, decoded
/// from the [`HEAL_AMOUNT_TABLE_VA`] table the apply handler reads. Only the
/// flat (deficit-clamped) cases are an on-disc amount; the rest carry the
/// mechanism so a consumer doesn't mistake them for a fixed number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreAmount {
    /// HP restore of up to `amount`, clamped to the target's HP deficit (class
    /// `0` single, class `1` all-party). `9999` (tier 2) is an effective full
    /// restore.
    Hp(u16),
    /// MP restore of up to `amount`, clamped to the MP deficit (class `2`).
    Mp(u16),
    /// A heal whose magnitude is **character-relative**, not a flat table value:
    /// the higher-tier HP heals (tier `3+`) scale off the per-character
    /// `0x80084140` Seru-heal tables, and revive (class `4`) restores
    /// `max_hp*0.4 + rand()%(max_hp/8)` (tier `0`) or full HP (tier `> 0`).
    CharRelative,
}

/// The two tier-indexed heal-amount sub-tables (`HP` + `MP`) the apply handler
/// `FUN_800402F4` reads, decoded off the disc. See [`HEAL_AMOUNT_TABLE_VA`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealAmounts {
    /// HP restore cap per tier (`0..=2`); `hp[2]` is the effective full restore.
    pub hp: [u16; HEAL_TIER_COUNT],
    /// MP restore cap per tier (`0..=2`).
    pub mp: [u16; HEAL_TIER_COUNT],
}

impl HealAmounts {
    /// The flat restore for a `(class, tier)`, or `None` when the class isn't a
    /// flat-amount heal (cure/buff/arts-book/flute/field) — read
    /// [`ItemEffect::category`] for those. Class `0`/`1` (single/all-party HP)
    /// read the HP table; class `2` (MP) reads the MP table; tier `3+` and the
    /// revive class resolve to [`RestoreAmount::CharRelative`].
    pub fn resolve(&self, class: u8, tier: u8) -> Option<RestoreAmount> {
        match class {
            0 | 1 => Some(match self.hp.get(tier as usize) {
                Some(&amt) => RestoreAmount::Hp(amt),
                None => RestoreAmount::CharRelative,
            }),
            2 => Some(match self.mp.get(tier as usize) {
                Some(&amt) => RestoreAmount::Mp(amt),
                None => RestoreAmount::CharRelative,
            }),
            4 => Some(RestoreAmount::CharRelative),
            _ => None,
        }
    }
}

/// The resolved item-effect table: per item id, the subtype byte it indexes by
/// and the descriptor that subtype selects.
#[derive(Debug, Clone)]
pub struct ItemEffectTable {
    /// `descriptors[subtype]` for the `RECORD_COUNT` table rows.
    descriptors: Vec<ItemEffect>,
    /// `subtype[id]` - the item-name table `+1` byte, per item id.
    subtype: Vec<u8>,
    /// The tier-indexed heal-amount sub-tables the apply handler reads.
    heal_amounts: HealAmounts,
}

impl ItemEffectTable {
    /// Resolve both tables out of a `SCUS_942.54` image. Returns `None` if the
    /// input isn't a PS-X EXE or the tables fall outside the data segment.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let map = ExeMap::parse(scus)?;

        let mut descriptors = Vec::with_capacity(RECORD_COUNT);
        for st in 0..RECORD_COUNT {
            let off = map.off(TABLE_VA + (st as u32) * RECORD_STRIDE as u32)?;
            let rec = scus.get(off..off + RECORD_STRIDE)?;
            descriptors.push(ItemEffect {
                class: rec[0],
                tier: rec[1],
                flags: rec[2],
                marker: rec[3],
            });
        }

        let mut subtype = Vec::with_capacity(ITEM_COUNT);
        for id in 0..ITEM_COUNT {
            let off = map.off(ITEM_TABLE_BASE_VA + (id as u32) * ITEM_RECORD_STRIDE + 1)?;
            subtype.push(*scus.get(off)?);
        }

        let read_tiers = |base_va: u32| -> Option<[u16; HEAL_TIER_COUNT]> {
            let mut out = [0u16; HEAL_TIER_COUNT];
            for (tier, slot) in out.iter_mut().enumerate() {
                let off = map.off(base_va + (tier as u32) * 2)?;
                *slot = u16::from_le_bytes(scus.get(off..off + 2)?.try_into().ok()?);
            }
            Some(out)
        };
        let heal_amounts = HealAmounts {
            hp: read_tiers(HEAL_AMOUNT_TABLE_VA)?,
            mp: read_tiers(HEAL_AMOUNT_TABLE_VA + MP_SUBTABLE_OFFSET)?,
        };

        Some(Self {
            descriptors,
            subtype,
            heal_amounts,
        })
    }

    /// The subtype byte item `id` indexes the descriptor table by.
    pub fn subtype(&self, id: u8) -> u8 {
        self.subtype[id as usize]
    }

    /// The raw descriptor for a subtype, or `None` if the subtype is past the
    /// table's `RECORD_COUNT` rows.
    pub fn descriptor(&self, subtype: u8) -> Option<ItemEffect> {
        self.descriptors.get(subtype as usize).copied()
    }

    /// The effect descriptor for an item id (resolved via its subtype byte).
    pub fn effect(&self, id: u8) -> Option<ItemEffect> {
        self.descriptor(self.subtype(id))
    }

    /// Number of descriptor rows parsed.
    pub fn record_count(&self) -> usize {
        self.descriptors.len()
    }

    /// The tier-indexed heal-amount sub-tables (HP + MP) the apply handler reads.
    pub fn heal_amounts(&self) -> HealAmounts {
        self.heal_amounts
    }

    /// The literal restore an item id applies, decoded from the heal-amount
    /// table via the item's `(class, tier)` descriptor. `None` for non-heal
    /// effects (cure / buff / arts book / flute / field) or unparsed ids — read
    /// [`Self::effect`] / [`ItemEffect::category`] for those.
    pub fn restore_amount(&self, id: u8) -> Option<RestoreAmount> {
        let eff = self.effect(id)?;
        self.heal_amounts.resolve(eff.class, eff.tier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_mapping_is_exhaustive_over_validated_classes() {
        let mk = |class| ItemEffect {
            class,
            tier: 0,
            flags: FLAG_BASE,
            marker: 0x41,
        };
        assert_eq!(mk(0).category(), ItemEffectCategory::HealHp);
        assert_eq!(mk(2).category(), ItemEffectCategory::HealMp);
        assert_eq!(mk(4).category(), ItemEffectCategory::Revive);
        assert_eq!(mk(8).category(), ItemEffectCategory::CureStatus);
        assert_eq!(mk(12).category(), ItemEffectCategory::ArtsBook);
        assert_eq!(mk(127).category(), ItemEffectCategory::SummonFlute);
        assert_eq!(mk(130).category(), ItemEffectCategory::ReduceEncounter);
        assert_eq!(mk(200).category(), ItemEffectCategory::Other(200));
    }

    #[test]
    fn flag_helpers_decode_bits() {
        // Healing-item flag byte 0x86 = base | field | battle.
        let heal = ItemEffect {
            class: 0,
            tier: 0,
            flags: 0x86,
            marker: 0x41,
        };
        assert!(heal.field_usable());
        assert!(heal.battle_usable());
        assert!(!heal.all_party());
        assert!(heal.is_usable_consumable());

        // All-party heal flag byte 0xA6 = base | all-party | field | battle.
        let party = ItemEffect {
            flags: 0xA6,
            ..heal
        };
        assert!(party.all_party());

        // Key-item flag byte 0x89 = base | 0x08 | 0x01: no field/battle bit.
        let key = ItemEffect {
            flags: 0x89,
            ..heal
        };
        assert!(!key.is_usable_consumable());
    }

    #[test]
    fn heal_amounts_resolve_by_class_and_tier() {
        let amts = HealAmounts {
            hp: [200, 800, 9999],
            mp: [50, 200, 20],
        };
        // Single (0) + all-party (1) HP heals both read the HP table.
        assert_eq!(amts.resolve(0, 0), Some(RestoreAmount::Hp(200)));
        assert_eq!(amts.resolve(0, 2), Some(RestoreAmount::Hp(9999)));
        assert_eq!(amts.resolve(1, 1), Some(RestoreAmount::Hp(800)));
        // MP heal (class 2) reads the MP table.
        assert_eq!(amts.resolve(2, 0), Some(RestoreAmount::Mp(50)));
        // Tier past the flat table (3+) is character-relative, not a number.
        assert_eq!(amts.resolve(0, 3), Some(RestoreAmount::CharRelative));
        // Revive is always character-relative; non-heal classes resolve to None.
        assert_eq!(amts.resolve(4, 0), Some(RestoreAmount::CharRelative));
        assert_eq!(amts.resolve(8, 0), None); // cure-single
        assert_eq!(amts.resolve(130, 0), None); // reduce-encounter
    }
}
