//! Element lookups for casts: seat and slot elements, the summon creature's
//! defence and element, and the affinity percentages.

use super::*;

impl World {
    /// Element-affinity multiplier (percent) for a monster (`attacker`) special
    /// attack landing on a party member (`target`): `matrix[enemy_element]
    /// [party_member_element]` from the parsed affinity tables
    /// ([`legaia_game_tables::element_affinity`], `FUN_801dd864`). Returns `100`
    /// (neutral, no change) when the affinity tables aren't installed or either
    /// element doesn't resolve, so disc-free / synthetic battles are unaffected.
    ///
    /// The attacker (enemy) element is its monster record's `+0x1D` byte
    /// (carried on [`legaia_engine_battle::monster_catalog::MonsterDef::element`]); the defender
    /// (party member) element is the per-character table entry for the seat's
    /// roster id `DAT_8007BD10[slot]` ([`Self::battle_party_char_id`]), not
    /// the seat plus one: a party without Noa seats Gala on seat 1. Reading the enemy element off
    /// `MonsterDef::element` is faithful to the retail read: `FUN_801dd864`
    /// fetches it record-direct (`0x801C9348[slot-3]` → `+0x1d`), not from a
    /// copied live-actor field.
    ///
    /// PORT: FUN_801dd864 (element resolution + affinity-matrix lookup, the
    /// enemy→party direction; the status-weaken / guard-double / slot-7 summon
    /// stages of the full retail function are not part of this scalar)
    pub(in crate::world::battle) fn enemy_affinity_pct(&self, attacker: u8, target: u8) -> u8 {
        let Some(aff) = self.tables.element_affinity.as_ref() else {
            return 100;
        };
        let Some(enemy_elem) = self.monster_seat_element(attacker as usize) else {
            return 100;
        };
        let Some(party_elem) = aff.character_element(self.battle_party_char_id(target)) else {
            return 100;
        };
        aff.affinity_pct(enemy_elem, party_elem).unwrap_or(100)
    }

    /// Element id (`0..=7`) of a battle slot, resolved by slot the way the retail
    /// affinity stage [`FUN_801dd864`] does: a party member (slot `< party_count`)
    /// takes its per-character table element; any other slot - an enemy, or the
    /// slot-7 summon body - takes its monster record's `+0x1D` element
    /// ([`legaia_engine_battle::monster_catalog::MonsterDef::element`]). Returns `None` when the
    /// affinity tables aren't installed or no element resolves, so callers fall
    /// back to neutral.
    pub(in crate::world) fn battle_slot_element(&self, slot: u8) -> Option<u8> {
        let aff = self.tables.element_affinity.as_ref()?;
        if (slot as usize) < self.party.party_count as usize {
            aff.character_element(self.battle_party_char_id(slot))
        } else {
            self.monster_seat_element(slot as usize)
        }
    }

    /// The 1-based roster id seated on party seat `slot` - retail's
    /// `DAT_8007BD10[slot]`, which every per-character table (element,
    /// summon power, camera height) indexes. Only meaningful for a party seat.
    pub(in crate::world) fn battle_party_char_id(&self, slot: u8) -> u8 {
        (self.party_roster_slot(usize::from(slot)) as u8).wrapping_add(1)
    }

    /// A monster seat's element this fight: the seat's own copy
    /// ([`crate::world::Actor::battle_element`], which PROT 0964's Element
    /// Change rewrites) ahead of its catalog record's `+0x1D`. `None` for a
    /// seat with no monster id or no catalog record.
    pub(in crate::world) fn monster_seat_element(&self, slot: usize) -> Option<u8> {
        let actor = self.actors.get(slot)?;
        let id = actor.battle_monster_id?;
        actor
            .battle_element
            .or_else(|| Some(self.tables.monster_catalog.get(id)?.element))
    }

    /// Element id of the *summon creature* a player Seru-magic `spell_id` attacks
    /// as. Retail resolves a player magic cast's attacker element by slot, and a
    /// summon cast runs through the slot-7 summon body - its namesake
    /// `battle_data` creature ([`legaia_engine_effects::summon::summon_creature_id`]) - so the
    /// attacker element is that creature's record `+0x1D`, *not* the casting
    /// character's. Resolved through [`Self::summon_creature_def`]. `None` for a
    /// non-summon id or when the creature doesn't resolve.
    /// The `battle_data` creature def a player Seru-magic `spell_id` summons -
    /// the namesake creature (Gimard spell → Gimard creature; see
    /// [`legaia_engine_effects::summon::summon_creature_id`]). Read from
    /// [`crate::world::DiscTables::summon_creatures`], the archive record the
    /// scene entry resolved for every summon; failing that, the spell's
    /// display name is matched against the loaded monster catalog, so the
    /// `"$2"`/`"$3"` higher-level enemy variants are excluded. `None` when the
    /// id isn't a summon or neither source carries the creature (disc-free /
    /// synthetic battles).
    pub(in crate::world) fn summon_creature_def(
        &self,
        spell_id: u8,
    ) -> Option<&legaia_engine_battle::monster_catalog::MonsterDef> {
        // The evolved block (`0x8C..=0x95`) summons a `battle_data` creature
        // exactly as the base block does; gating on the base block alone sent
        // Aluru / Gilium & co. to the MP-scaled placeholder, which a boss's
        // magic defence floors at 1.
        // The summon body installed at scene entry for every player cast, the
        // flutes and big summons included: the cast's own `summon.dat`
        // record, which is what the seat arm copies onto slot 7. The scene
        // catalog below holds only the scene's own monsters, so on its own it
        // resolves a summon only where the creature also fights as an enemy.
        if let Some(def) = self.tables.summon_creatures.get(&spell_id) {
            return Some(def);
        }
        if !legaia_engine_effects::summon::has_summon_body(spell_id) {
            return None;
        }
        let name = legaia_engine_battle::retail_magic::get(spell_id)?.name;
        self.tables
            .monster_catalog
            .by_id
            .values()
            .filter(|d| d.name == name)
            .min_by_key(|d| d.id)
    }

    pub(in crate::world) fn summon_attacker_element(&self, spell_id: u8) -> Option<u8> {
        self.summon_creature_def(spell_id).map(|d| d.element)
    }

    /// Element-affinity multiplier (percent) for a player Seru-magic cast
    /// (`spell_id`) landing on `target`: `matrix[summon-creature element][target
    /// element]` ([`legaia_game_tables::element_affinity`], `FUN_801dd864`). The
    /// attacker element is the summon creature's ([`Self::summon_attacker_element`]),
    /// the defender element is resolved by slot ([`Self::battle_slot_element`]).
    /// Returns `100` (neutral, no change) when the tables aren't installed, the id
    /// isn't a summon, or either element fails to resolve - so disc-free /
    /// synthetic battles and non-summon casts are unaffected. Applied post-roll,
    /// so it never touches the RNG stream.
    pub(in crate::world) fn cast_affinity_pct(&self, spell_id: u8, target: u8) -> u8 {
        let Some(aff) = self.tables.element_affinity.as_ref() else {
            return 100;
        };
        let Some(atk_elem) = self.summon_attacker_element(spell_id) else {
            return 100;
        };
        let Some(def_elem) = self.battle_slot_element(target) else {
            return 100;
        };
        aff.affinity_pct(atk_elem, def_elem).unwrap_or(100)
    }
}
