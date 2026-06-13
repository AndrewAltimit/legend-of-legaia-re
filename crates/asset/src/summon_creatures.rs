//! Player Seru-magic summon → namesake `battle_data` creature map, recovered
//! from the disc by mesh identity.
//!
//! A player summon renders its namesake creature through the ordinary rigid-TRS
//! battle draw (the move-VM stager is the spawn/effect side-channel, not the
//! visual — see [`crate::summon_overlay`] and the open-RE thread on the summon
//! visual). Each summon's creature is installed from the `summon.dat` group's
//! actor-record slot (`legaia_asset::summon_readef`), and that slot's Legaia TMD
//! is **byte-identical** to a record in the monster archive (PROT 867,
//! [`crate::monster_archive`]). Matching every summon group's actor-record TMD
//! against the archive by longest-common-prefix recovers the full map below.
//!
//! Two blocks resolve to archive creatures with byte-identical meshes:
//!
//! - **Base block** `0x81..=0x8B` (Gimard … Nova) — the eleven first-tier
//!   Seru-magic summons.
//! - **Evolved-Seru block** `0x8C..=0x95` (Gola Gola … Gilium) — the second-tier
//!   summons. This pins the two legs that had no mid-cast capture state,
//!   `0x90` → Kemaro and `0x91` → Spoon, by exact mesh identity.
//!
//! The **high block** `0x99..=0xA0` (Evil-Seru / Sim-Seru / Ra-Seru summons:
//! Juggernaut, Palma, Mule, Horn, Jedo, Meta, Terra, Ozma) does **not** byte-
//! match any archive record — those summons carry a **bespoke mesh** in the
//! `summon.dat` group's raw CLUT+texture+part-pool slot (the third slot of the
//! four-slot big-summon groups), not a reused enemy body. The disc-gated
//! `summon_creature_tmd_map_real` oracle asserts both facts: byte-identity for
//! `0x81..=0x95`, and no archive byte-match for `0x99..=0xA0`.

/// One summon spell's namesake `battle_data` creature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummonCreature {
    /// Seru-magic spell id (`actor[+0x1DF]`, `0x81..=0x95`).
    pub spell_id: u8,
    /// 1-based monster id in the PROT 867 archive whose mesh this summon reuses.
    pub creature_id: u16,
    /// Archive record name for that creature.
    pub name: &'static str,
}

/// Base + evolved-Seru summon → creature map, byte-validated against the disc by
/// `summon_creature_tmd_map_real`. The high block `0x99..=0xA0` is intentionally
/// absent (bespoke summon meshes — see the module docs).
pub const SUMMON_CREATURES: &[SummonCreature] = &[
    // Base block 0x81..=0x8B.
    SummonCreature {
        spell_id: 0x81,
        creature_id: 10,
        name: "Gimard",
    },
    SummonCreature {
        spell_id: 0x82,
        creature_id: 25,
        name: "Theeder",
    },
    SummonCreature {
        spell_id: 0x83,
        creature_id: 28,
        name: "Vera",
    },
    SummonCreature {
        spell_id: 0x84,
        creature_id: 55,
        name: "Gizam",
    },
    SummonCreature {
        spell_id: 0x85,
        creature_id: 49,
        name: "Nighto",
    },
    SummonCreature {
        spell_id: 0x86,
        creature_id: 64,
        name: "Zenoir",
    },
    SummonCreature {
        spell_id: 0x87,
        creature_id: 74,
        name: "Viguro",
    },
    SummonCreature {
        spell_id: 0x88,
        creature_id: 86,
        name: "Swordie",
    },
    SummonCreature {
        spell_id: 0x89,
        creature_id: 83,
        name: "Orb",
    },
    SummonCreature {
        spell_id: 0x8A,
        creature_id: 92,
        name: "Freed",
    },
    SummonCreature {
        spell_id: 0x8B,
        creature_id: 95,
        name: "Nova",
    },
    // Evolved-Seru block 0x8C..=0x95.
    SummonCreature {
        spell_id: 0x8C,
        creature_id: 98,
        name: "Gola Gola",
    },
    SummonCreature {
        spell_id: 0x8D,
        creature_id: 101,
        name: "Mushura",
    },
    SummonCreature {
        spell_id: 0x8E,
        creature_id: 80,
        name: "Aluru",
    },
    SummonCreature {
        spell_id: 0x8F,
        creature_id: 141,
        name: "Barra",
    },
    SummonCreature {
        spell_id: 0x90,
        creature_id: 144,
        name: "Kemaro",
    },
    SummonCreature {
        spell_id: 0x91,
        creature_id: 147,
        name: "Spoon",
    },
    SummonCreature {
        spell_id: 0x92,
        creature_id: 150,
        name: "Slippery",
    },
    SummonCreature {
        spell_id: 0x93,
        creature_id: 153,
        name: "Iota",
    },
    SummonCreature {
        spell_id: 0x94,
        creature_id: 156,
        name: "Puera",
    },
    SummonCreature {
        spell_id: 0x95,
        creature_id: 159,
        name: "Gilium",
    },
];

/// The map entry for `spell_id`, or `None` if it is not a base/evolved summon
/// (the high block `0x99..=0xA0` is bespoke and intentionally excluded).
pub fn creature_for_spell(spell_id: u8) -> Option<&'static SummonCreature> {
    SUMMON_CREATURES.iter().find(|c| c.spell_id == spell_id)
}

/// In-`summon.dat` actor-record slot index for a summon spell id: the last slot
/// of the spell's group (3-slot groups for `0x81..=0x99`, 4-slot for the
/// big-summon `0x9A..=0xA0`). Mirrors `summon_readef::stream_target`'s group
/// layout; the actor record is always the group's final slot.
pub fn actor_record_slot_index(spell_id: u8) -> usize {
    if (0x81..=0x99).contains(&spell_id) {
        3 * (spell_id as usize - 0x81) + 2
    } else {
        78 + 4 * (spell_id as usize - 0x9A)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_is_contiguous_and_complete() {
        // Exactly 0x81..=0x95, in order, no gaps.
        let expected: Vec<u8> = (0x81..=0x95).collect();
        let got: Vec<u8> = SUMMON_CREATURES.iter().map(|c| c.spell_id).collect();
        assert_eq!(got, expected);
        assert!(creature_for_spell(0x80).is_none());
        assert!(creature_for_spell(0x96).is_none());
        assert!(creature_for_spell(0x99).is_none()); // high block is bespoke
        assert_eq!(creature_for_spell(0x90).unwrap().name, "Kemaro");
        assert_eq!(creature_for_spell(0x91).unwrap().name, "Spoon");
    }

    #[test]
    fn slot_index_layout() {
        assert_eq!(actor_record_slot_index(0x81), 2);
        assert_eq!(actor_record_slot_index(0x8C), 35);
        assert_eq!(actor_record_slot_index(0x99), 74);
        assert_eq!(actor_record_slot_index(0x9A), 78);
        assert_eq!(actor_record_slot_index(0xA0), 102);
    }
}
