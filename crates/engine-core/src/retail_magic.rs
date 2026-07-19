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
//! Per-spell base **power** is *not* in this table, and there is no separate
//! static multiplier array to capture. The player Seru block all share
//! `cat = 0x32 / sub = 0`, and the cast-begin state (`0x28`) reads only MP +
//! capture flag + name; the per-summon effect (and its damage) is dispatched by
//! `(id - 0x81)` through `PTR_801f6734` in state `0x29`, i.e. it lives in the
//! battle effect scripts, not a scalar table. The static atk/def kernel
//! `FUN_801ec3e4` is melee/arts-only (gated on an action-queue head in
//! `0xC..=0x1F`). See `docs/formats/spell-table.md` for the full trace. The
//! `base_power` figures below are therefore explicit MP-scaled placeholders.
//!
//! What the magnitude actually *is* has been traced: a damage summon's HP delta
//! is the caster/summon-state-derived roll in `FUN_801dd0ac` (`attacker_slot ==
//! 7`), scaled by element affinity + status bits + the caster's magic-power byte
//! (`FUN_801dd864`) and finalized by `FUN_801ddb30`. The bounded, state-free
//! pieces of that chain are ported as pure kernels in
//! [`legaia_engine_vm::battle_formulas`] (`summon_attacker_roll` /
//! `summon_defender_roll` / `summon_predamage` / `heal_summon_amount` and the
//! `apply_*` scale helpers). They are **not yet wired** into a live battle here:
//! the engine's spell path still uses [`SpellEffect`]'s MP-scaled `base_power`,
//! and the faithful roll needs a live battle-actor context (both actors' AGL/HP/
//! defense/status, the affinity matrix, and the caster magic-power byte) plus
//! the `FUN_801ddb30` finisher, which mutates ~20 battle globals. When a
//! player-driven summon consumer needs real numbers, feed those stats into the
//! `battle_formulas` kernels rather than the placeholder below.

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

/// Build a [`SpellDef`] for one [`RetailSpell`] with explicit `mp` / `target`
/// (so the pinned and disc-sourced paths share the same effect mapping).
/// Ally-target light spells are modelled as heals (Vera / Orb); everything else
/// is elemental damage. The damage figure is an MP-scaled placeholder (see the
/// module docs).
fn spell_def_with(s: &RetailSpell, mp: u8, target: SpellTarget) -> SpellDef {
    let effect = match target {
        SpellTarget::OneAlly => SpellEffect::Heal {
            amount: (mp as u16) * 8,
        },
        SpellTarget::AllAllies => SpellEffect::HealAll {
            amount: (mp as u16) * 6,
        },
        _ => SpellEffect::Damage {
            base_power: (mp as u16) * 2,
            element: s.element,
        },
    };
    SpellDef {
        id: s.id,
        name: s.name.into(),
        mp_cost: mp,
        element: s.element,
        target,
        effect,
        // anim id == real table anim (0x25 + block index), kept aligned so a
        // future anim-table port can drive the same trigger.
        anim_id: 0x25 + (s.id - 0x81),
    }
}

/// Build a [`SpellDef`] from the pinned record (MP + target from [`SERU_MAGIC`]).
fn spell_def_for(s: &RetailSpell) -> SpellDef {
    spell_def_with(s, s.mp, s.target)
}

/// Map the parser's decoded `+2` target shape onto the engine [`SpellTarget`].
fn target_from_shape(shape: legaia_asset::spell_names::SpellTargetShape) -> SpellTarget {
    use legaia_asset::spell_names::SpellTargetShape as S;
    match shape {
        S::OneEnemy => SpellTarget::OneEnemy,
        S::AllEnemies => SpellTarget::AllEnemies,
        S::OneAlly => SpellTarget::OneAlly,
        S::AllAllies => SpellTarget::AllAllies,
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

/// Like [`retail_seru_magic_catalog`], but the player Seru-magic block's **MP
/// cost** (`+3`), **target shape** (`+2`, decoded via
/// [`legaia_asset::spell_names::SpellEntry::target_shape`]) and **display name**
/// (`name_ptr`) are read from the user's `SCUS_942.54` instead of the pinned
/// constants. On the retail disc this is byte-identical to
/// [`retail_seru_magic_catalog`] (the pinned values were decoded from the same
/// table); on a randomized / translated disc it honours the patched MP /
/// targeting and shows the disc's own (e.g. localised) names. Per-field fallback
/// to the pinned record keeps a malformed table from zeroing a spell. Returns
/// `None` only when the image isn't a parseable PSX-EXE.
pub fn seru_magic_catalog_from_scus(scus: &[u8]) -> Option<SpellCatalog> {
    let table = legaia_asset::spell_names::SpellNameTable::from_scus(scus)?;
    let mut c = SpellCatalog::vanilla();
    for s in SERU_MAGIC {
        let (mp, target) = match table.entry(s.id) {
            Some(e) => (e.mp, target_from_shape(e.target_shape())),
            None => (s.mp, s.target),
        };
        let mut def = spell_def_with(s, mp, target);
        // Prefer the disc's own name string (localised on a JP/EU disc); fall
        // back to the pinned English name for empty / missing slots.
        if let Some(name) = table.name(s.id) {
            def.name = name.to_string();
        }
        c.insert(def);
    }
    Some(c)
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
    fn target_from_shape_maps_all_four_shapes() {
        use legaia_asset::spell_names::SpellTargetShape as S;
        assert_eq!(target_from_shape(S::OneEnemy), SpellTarget::OneEnemy);
        assert_eq!(target_from_shape(S::AllEnemies), SpellTarget::AllEnemies);
        assert_eq!(target_from_shape(S::OneAlly), SpellTarget::OneAlly);
        assert_eq!(target_from_shape(S::AllAllies), SpellTarget::AllAllies);
    }

    #[test]
    fn pinned_seru_target_bytes_decode_to_pinned_targets() {
        // The +2 target byte each pinned Seru spell *would* carry (byte-exact
        // from SCUS), decoded via the parser, must reproduce its pinned target.
        // This locks the bit decode (0x02 ally / 0x20 all) to the catalog.
        use legaia_asset::spell_names::{SpellEntry, SpellTargetShape as S};
        let byte_for = |t: SpellTarget| -> u8 {
            match t {
                SpellTarget::OneEnemy => 0x44,
                SpellTarget::AllEnemies => 0x64,
                SpellTarget::OneAlly => 0x06,
                SpellTarget::AllAllies => 0x26,
                SpellTarget::SelfOnly => unreachable!("no Seru spell self-targets"),
            }
        };
        let shape_to_target = |s: S| target_from_shape(s);
        for spell in SERU_MAGIC {
            let e = SpellEntry {
                name: None,
                mp: spell.mp,
                target: byte_for(spell.target),
                desc: None,
            };
            assert_eq!(
                shape_to_target(e.target_shape()),
                spell.target,
                "{} (#{:#04x}) target round-trips through the byte decode",
                spell.name,
                spell.id
            );
        }
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
