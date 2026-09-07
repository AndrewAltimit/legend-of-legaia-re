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
//! static multiplier array to capture. The cast-begin state (`0x28`) reads only
//! MP + capture flag + name; the per-summon effect (and its damage) is
//! dispatched by `(id - 0x81)` through `PTR_801f6734` in state `0x29`, i.e. it
//! lives in the battle effect scripts, not a scalar table.
//!
//! An earlier revision of this note said the player Seru block "all share
//! `cat = 0x32 / sub = 0`". It does not, and the exceptions are the two
//! **ally-side** spells: read off `SCUS_942.54`, `0x83` Vera is `+0 = 0x00 /
//! +1 = 0x03` and `0x89` Orb is `+0 = 0x01 / +1 = 0x04`, while the nine
//! enemy-side spells are `0x32 / 0x00`. Both bytes are pinned per record in
//! [`RetailSpell`] ([`RetailSpell::cast_class`] / [`RetailSpell::effect_class`])
//! rather than asserted once for the block, because `+1` is the key
//! `FUN_801F2160` dispatches a cast's module on
//! ([`legaia_asset::cast_effect_pool`]). The static atk/def kernel
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
    /// **Cast class**, table `+0`. `'c'` (`0x63`) marks a capture-class
    /// record; no player Seru spell is one.
    pub cast_class: u8,
    /// **Effect class**, table `+1` - the byte `FUN_801F2160` bounds with
    /// `sltiu ..., 0x20` and jumps through `0x801CF56C` on, selecting the cast
    /// module PROT `935 + class`
    /// ([`legaia_asset::cast_effect_pool::capture_module_prot`]). A player Seru
    /// cast does not route through that dispatcher - its module comes from the
    /// action id through `FUN_801F1ED4` - but the byte is table data and is
    /// pinned here because it is the *only* live key the capture band has.
    pub effect_class: u8,
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
        cast_class: 0x32,
        effect_class: 0x00,
    },
    RetailSpell {
        id: 0x82,
        name: "Theeder",
        element: SpellElement::Thunder,
        mp: 24,
        target: SpellTarget::OneEnemy,
        cast_class: 0x32,
        effect_class: 0x00,
    },
    RetailSpell {
        id: 0x83,
        name: "Vera",
        element: SpellElement::Light,
        mp: 6,
        target: SpellTarget::OneAlly,
        cast_class: 0x00,
        effect_class: 0x03,
    },
    RetailSpell {
        id: 0x84,
        name: "Gizam",
        element: SpellElement::Water,
        mp: 28,
        target: SpellTarget::AllEnemies,
        cast_class: 0x32,
        effect_class: 0x00,
    },
    RetailSpell {
        id: 0x85,
        name: "Nighto",
        element: SpellElement::Dark,
        mp: 13,
        target: SpellTarget::OneEnemy,
        cast_class: 0x32,
        effect_class: 0x00,
    },
    RetailSpell {
        id: 0x86,
        name: "Zenoir",
        element: SpellElement::Fire,
        mp: 36,
        target: SpellTarget::OneEnemy,
        cast_class: 0x32,
        effect_class: 0x00,
    },
    RetailSpell {
        id: 0x87,
        name: "Viguro",
        element: SpellElement::Thunder,
        mp: 64,
        target: SpellTarget::AllEnemies,
        cast_class: 0x32,
        effect_class: 0x00,
    },
    RetailSpell {
        id: 0x88,
        name: "Swordie",
        element: SpellElement::Wind,
        mp: 32,
        target: SpellTarget::OneEnemy,
        cast_class: 0x32,
        effect_class: 0x00,
    },
    RetailSpell {
        id: 0x89,
        name: "Orb",
        element: SpellElement::Light,
        mp: 18,
        target: SpellTarget::AllAllies,
        cast_class: 0x01,
        effect_class: 0x04,
    },
    RetailSpell {
        id: 0x8a,
        name: "Freed",
        element: SpellElement::Water,
        mp: 40,
        target: SpellTarget::AllEnemies,
        cast_class: 0x32,
        effect_class: 0x00,
    },
    RetailSpell {
        id: 0x8b,
        name: "Nova",
        element: SpellElement::Wind,
        mp: 48,
        target: SpellTarget::OneEnemy,
        cast_class: 0x32,
        effect_class: 0x00,
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
    spell_def_with_class(s, mp, target, s.effect_class)
}

/// [`spell_def_with`] with the record's `+0x01` **effect class** supplied
/// explicitly, so a disc-sourced catalog carries the disc's byte rather than
/// the pinned one (a patched / localised table may differ).
fn spell_def_with_class(
    s: &RetailSpell,
    mp: u8,
    target: SpellTarget,
    effect_class: u8,
) -> SpellDef {
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
        effect_class,
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
        let (mp, target, class) = match table.entry(s.id) {
            // `sub_class` is the record's `+0x01` byte - the cast-module key
            // `FUN_801F2160` dispatches on.
            Some(e) => (e.mp, target_from_shape(e.target_shape()), e.sub_class),
            None => (s.mp, s.target, s.effect_class),
        };
        let mut def = spell_def_with_class(s, mp, target, class);
        // Prefer the disc's own name string (localised on a JP/EU disc); fall
        // back to the pinned English name for empty / missing slots.
        if let Some(name) = table.name(s.id) {
            def.name = name.to_string();
        }
        c.insert(def);
    }
    insert_monster_specials(&mut c, &table);
    Some(c)
}

/// Every named, non-capture id below the player block (`0x01..=0x80`) as the
/// disc names it: the **monster specials** a record's `+0x21..=+0x23` magic
/// slots and the picker's scripted arms emit (Gimard's `+0x21` is `0x27` =
/// Tail Fire). The disc is the single source for this block, so a vanilla
/// placeholder that happens to sit on a real id under another name (`0x26`
/// "Crash" on the disc's Thunderbolt) is replaced, while a vanilla record on
/// its real id under the same name (the clean-room monster block - Divide /
/// Steal / Power Up / Curse All / ...) keeps its effect class and target and
/// takes the disc's cost.
///
/// MP, target shape and name are the table's. The effect class of a fresh
/// record follows the shape's side - enemy side = damage, ally side = heal -
/// because the table carries no effect byte; the magnitude and the impact
/// status of a live cast are the move-power record's once the 0898 catalog
/// is installed at scene entry (`World::enemy_move_power` /
/// `apply_enemy_move_status`), and the vanilla block's MP-scaled placeholder
/// otherwise. Capture-class records are not inserted: their fold is the
/// streamed module's and the SM's capture branch keys on the class byte, not
/// on a catalog record. Id `0x00` is the table's template row ("Magic", the
/// MES-substituted elemental tiers' head) and stays out - `params[0] == 0`
/// is "no spell" to the action stream.
///
/// Without this block every monster whose magic slot names such an id
/// degrades to a physical strike at `take_monster_turn`'s catalog lookup, in
/// every disc-booted fight - the roll picks the cast, the lookup discards it.
fn insert_monster_specials(
    c: &mut SpellCatalog,
    table: &legaia_asset::spell_names::SpellNameTable,
) {
    for id in 0x01..=0x80u8 {
        let Some(e) = table.entry(id) else {
            continue;
        };
        if e.is_capture_class() {
            continue;
        }
        let Some(name) = e.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) else {
            continue;
        };
        if let Some(mut v) = c.get(id).cloned()
            && v.name.eq_ignore_ascii_case(name)
        {
            v.mp_cost = e.mp;
            v.effect_class = e.sub_class;
            c.insert(v);
            continue;
        }
        let target = target_from_shape(e.target_shape());
        let mp = u16::from(e.mp);
        let effect = match target {
            SpellTarget::OneAlly => SpellEffect::Heal { amount: mp * 8 },
            SpellTarget::AllAllies => SpellEffect::HealAll { amount: mp * 6 },
            _ => SpellEffect::Damage {
                base_power: mp * 2,
                element: SpellElement::Neutral,
            },
        };
        c.insert(SpellDef {
            id,
            name: name.to_string(),
            mp_cost: e.mp,
            element: SpellElement::Neutral,
            target,
            effect,
            effect_class: e.sub_class,
            ..Default::default()
        });
    }
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

    /// The `+0` / `+1` byte pair of every pinned player Seru record, and the
    /// module each `+1` would name if the record were capture-class.
    ///
    /// Two things this locks. First, the block is **not** uniform: an earlier
    /// note here claimed `cat = 0x32 / sub = 0` for all eleven, and the two
    /// ally-side spells break it (`0x83` Vera = `0x00 / 0x03`, `0x89` Orb =
    /// `0x01 / 0x04`). Second, no player Seru record is capture-class, so a
    /// player cast's module comes from `FUN_801F1ED4`'s action-id row, not
    /// from `FUN_801F2160`'s `+1` row - `seru_module_prot` is the right
    /// arithmetic for them and `capture_module_prot` is not.
    #[test]
    fn pinned_effect_class_bytes_and_their_band_rows() {
        use legaia_asset::cast_effect_pool::{capture_module_prot, seru_module_prot};
        use legaia_asset::spell_names::CAPTURE_CLASS;
        let expect: &[(u8, u8, u8)] = &[
            (0x81, 0x32, 0x00),
            (0x82, 0x32, 0x00),
            (0x83, 0x00, 0x03),
            (0x84, 0x32, 0x00),
            (0x85, 0x32, 0x00),
            (0x86, 0x32, 0x00),
            (0x87, 0x32, 0x00),
            (0x88, 0x32, 0x00),
            (0x89, 0x01, 0x04),
            (0x8a, 0x32, 0x00),
            (0x8b, 0x32, 0x00),
        ];
        for &(id, cast, class) in expect {
            let s = get(id).unwrap_or_else(|| panic!("missing spell {id:#04x}"));
            assert_eq!(s.cast_class, cast, "+0 for {}", s.name);
            assert_eq!(s.effect_class, class, "+1 for {}", s.name);
            assert_ne!(
                s.cast_class, CAPTURE_CLASS,
                "{} is not capture-class",
                s.name
            );
            // Every `+1` in the block is inside the capture dispatcher's
            // `sltiu ..., 0x20` bound even though no player cast uses it.
            assert!(capture_module_prot(s.effect_class).is_some());
            // The row a player cast actually takes.
            assert_eq!(seru_module_prot(id), Some(903 + u32::from(id - 0x81)));
        }
        // The block is not uniform - the claim this test replaced.
        assert!(
            SERU_MAGIC.iter().any(|s| s.effect_class != 0),
            "at least one player Seru record carries a non-zero +1"
        );
    }

    #[test]
    fn catalog_carries_the_effect_class() {
        let c = retail_seru_magic_catalog();
        assert_eq!(c.get(0x81).map(|d| d.effect_class), Some(0x00));
        assert_eq!(c.get(0x83).map(|d| d.effect_class), Some(0x03));
        assert_eq!(c.get(0x89).map(|d| d.effect_class), Some(0x04));
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
                mp: spell.mp,
                target: byte_for(spell.target),
                ..Default::default()
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
