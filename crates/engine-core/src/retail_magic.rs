//! Retail player Seru-magic table, pinned from `SCUS_942.54`.
//!
//! The battle-action SM resolves a cast's MP cost and spell name from a
//! static 12-byte-stride table in the executable. Two interleaved base
//! addresses view the same records (the SM reads them through different
//! pointers):
//!
//! - **stats base** `DAT_800754C8` (`+id*0xC`): `[cat u8][sub u8]
//!   [target u8][mp u8][anim u8][3 × pad][name_ptr u32]`. Entry `+3` is the
//!   MP cost; `+0` is a class byte (`'c'` = `0x63` marks capture-class
//!   spells). See [`docs/subsystems/battle-action.md`] states `0x28` / `0x3C`.
//! - **name-pointer base** `DAT_800754D0` (`+id*0xC`) is the same stride
//!   shifted +8, so the SM's "name lookup" reads the `name_ptr` field.
//!
//! The `target` byte decodes as a 2-bit shape: bit `0x40` = targets enemies
//! (else allies), bit `0x20` = all (else single) - so `0x44` = one enemy,
//! `0x64` = all enemies, `0x06` = one ally, `0x26` = all allies.
//!
//! Spell ids `0x81..=0x8b` are the **player Seru-magic** block: 11 named
//! summon spells whose `anim` ids run sequentially (`0x25..=0x2f`). Their MP
//! costs cross-validate exactly against the curated `data/gamedata/magic.toml`
//! (public walkthrough data), and id `0x81` = Gimard matches the save-state
//! pin in [`crate::capture_observations::seru_capture`]. Id `0x80` ("Flip
//! Frog") is the boundary entry just below the block (`mp`/`anim` both 0) and
//! is not part of the sequential player set. The lower ids `0x00..=0x24` are
//! the elemental enemy-attack tiers (names composed via the MES substitution
//! table, not inline), and `0x25..=0x7f` are the monster / capture-class
//! spells - neither is reproduced here.
//!
//! Per-spell base **power** is *not* in this table (retail derives damage
//! from the caster's magic stat × a separate per-spell multiplier); the
//! catalog below uses MP-scaled placeholders for the damage figure pending an
//! overlay capture of the multiplier table.

use crate::spells::{SpellCatalog, SpellDef, SpellEffect, SpellElement, SpellTarget};

/// One pinned retail spell record (the fields this crate can source from the
/// static SCUS table + the public gamedata cross-reference).
#[derive(Debug, Clone, Copy)]
pub struct RetailSpell {
    /// Real binary spell id (index into the SCUS spell table).
    pub id: u8,
    /// Display name, read from the table's `name_ptr`.
    pub name: &'static str,
    /// Element (from the gamedata cross-reference; the table encodes it only
    /// as a MES name-colour prefix).
    pub element: SpellElement,
    /// MP cost (table `+3`), byte-exact against retail.
    pub mp: u8,
    /// Target shape (decoded from the table's `target` byte).
    pub target: SpellTarget,
}

/// Player Seru-magic block, spell ids `0x81..=0x8b`. Order matches ascending
/// id (= ascending `anim`). MP + target are byte-exact from `SCUS_942.54`;
/// element is the gamedata cross-reference.
pub const SERU_MAGIC: &[RetailSpell] = &[
    RetailSpell {
        id: 0x81,
        name: "Gimard",
        element: SpellElement::Fire,
        mp: 10,
        target: SpellTarget::OneEnemy,
    },
    RetailSpell {
        id: 0x82,
        name: "Theeder",
        element: SpellElement::Thunder,
        mp: 24,
        target: SpellTarget::OneEnemy,
    },
    RetailSpell {
        id: 0x83,
        name: "Vera",
        element: SpellElement::Light,
        mp: 6,
        target: SpellTarget::OneAlly,
    },
    RetailSpell {
        id: 0x84,
        name: "Gizam",
        element: SpellElement::Water,
        mp: 28,
        target: SpellTarget::AllEnemies,
    },
    RetailSpell {
        id: 0x85,
        name: "Nighto",
        element: SpellElement::Dark,
        mp: 13,
        target: SpellTarget::OneEnemy,
    },
    RetailSpell {
        id: 0x86,
        name: "Zenoir",
        element: SpellElement::Fire,
        mp: 36,
        target: SpellTarget::OneEnemy,
    },
    RetailSpell {
        id: 0x87,
        name: "Viguro",
        element: SpellElement::Thunder,
        mp: 64,
        target: SpellTarget::AllEnemies,
    },
    RetailSpell {
        id: 0x88,
        name: "Swordie",
        element: SpellElement::Wind,
        mp: 32,
        target: SpellTarget::OneEnemy,
    },
    RetailSpell {
        id: 0x89,
        name: "Orb",
        element: SpellElement::Light,
        mp: 18,
        target: SpellTarget::AllAllies,
    },
    RetailSpell {
        id: 0x8a,
        name: "Freed",
        element: SpellElement::Water,
        mp: 40,
        target: SpellTarget::AllEnemies,
    },
    RetailSpell {
        id: 0x8b,
        name: "Nova",
        element: SpellElement::Wind,
        mp: 48,
        target: SpellTarget::OneEnemy,
    },
];

/// Look up a pinned retail spell by its real id.
pub fn get(id: u8) -> Option<&'static RetailSpell> {
    SERU_MAGIC.iter().find(|s| s.id == id)
}

/// Build a [`SpellDef`] for one [`RetailSpell`]. Ally-target light spells are
/// modelled as heals (Vera / Orb); everything else is elemental damage. The
/// damage figure is an MP-scaled placeholder (see the module docs).
fn spell_def_for(s: &RetailSpell) -> SpellDef {
    let effect = match s.target {
        SpellTarget::OneAlly => SpellEffect::Heal {
            amount: (s.mp as u16) * 8,
        },
        SpellTarget::AllAllies => SpellEffect::HealAll {
            amount: (s.mp as u16) * 6,
        },
        _ => SpellEffect::Damage {
            base_power: (s.mp as u16) * 2,
            element: s.element,
        },
    };
    SpellDef {
        id: s.id,
        name: s.name.into(),
        mp_cost: s.mp,
        element: s.element,
        target: s.target,
        effect,
        // anim id == real table anim (0x25 + block index), kept aligned so a
        // future anim-table port can drive the same trigger.
        anim_id: 0x25 + (s.id - 0x81),
    }
}

/// A spell catalog covering the real player Seru-magic ids on top of the
/// [`SpellCatalog::vanilla`] demo entries. The real ids (`0x81..=0x8b`) don't
/// collide with the placeholder range (`0x10..=0x51`), so this is a clean
/// union: a boot save or capture that uses a real id resolves to the correct
/// name, while the legacy demo ids still work.
pub fn retail_seru_magic_catalog() -> SpellCatalog {
    let mut c = SpellCatalog::vanilla();
    for s in SERU_MAGIC {
        c.insert(spell_def_for(s));
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_observations::seru_capture;

    #[test]
    fn pinned_mp_costs_match_gamedata_cross_reference() {
        // These MP values are byte-exact from SCUS_942.54 and were validated
        // against data/gamedata/magic.toml when the table was decoded. Pin
        // them here so a regression in the table is caught without the disc.
        let expect: &[(u8, &str, u8)] = &[
            (0x81, "Gimard", 10),
            (0x82, "Theeder", 24),
            (0x83, "Vera", 6),
            (0x84, "Gizam", 28),
            (0x85, "Nighto", 13),
            (0x86, "Zenoir", 36),
            (0x87, "Viguro", 64),
            (0x88, "Swordie", 32),
            (0x89, "Orb", 18),
            (0x8a, "Freed", 40),
            (0x8b, "Nova", 48),
        ];
        for &(id, name, mp) in expect {
            let s = get(id).unwrap_or_else(|| panic!("missing spell {id:#04x}"));
            assert_eq!(s.name, name, "name for {id:#04x}");
            assert_eq!(s.mp, mp, "mp for {name}");
        }
        assert_eq!(SERU_MAGIC.len(), expect.len());
    }

    #[test]
    fn gimard_id_matches_the_save_state_pin() {
        // The Gimard before/after savestate pin recorded spell id 0x81 in
        // Vahn's record; the SCUS spell table names that id "Gimard" (fire).
        assert_eq!(seru_capture::GIMARD_SPELL_ID, 0x81);
        let gimard = get(seru_capture::GIMARD_SPELL_ID).expect("Gimard pinned");
        assert_eq!(gimard.name, "Gimard");
        assert_eq!(gimard.element, SpellElement::Fire);
    }

    #[test]
    fn retail_catalog_resolves_real_ids_and_keeps_vanilla() {
        let c = retail_seru_magic_catalog();
        // Real id resolves with its real name.
        assert_eq!(c.get(0x81).map(|d| d.name.as_str()), Some("Gimard"));
        assert_eq!(c.get(0x87).map(|d| d.name.as_str()), Some("Viguro"));
        // Ally-target spells are heals; enemy-target are damage.
        assert!(matches!(
            c.get(0x83).map(|d| &d.effect),
            Some(SpellEffect::Heal { .. })
        ));
        assert!(matches!(
            c.get(0x89).map(|d| &d.effect),
            Some(SpellEffect::HealAll { .. })
        ));
        assert!(matches!(
            c.get(0x81).map(|d| &d.effect),
            Some(SpellEffect::Damage { .. })
        ));
        // Legacy demo id still present (no regression).
        assert!(c.get(0x20).is_some());
    }
}
