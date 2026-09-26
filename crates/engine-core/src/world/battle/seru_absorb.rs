//! The **killing-blow Seru absorb**: how a party member learns Seru magic by
//! finishing off a Seru-carrying monster with an attack.
//!
//! Retail decides it inside the arts execution resolver `FUN_801EC3E4`
//! (PROT 0898, `0x801EE1C0..0x801EE2E8`), on the per-hit kill check, and
//! grants it in the action SM's Done band (`FUN_801E295C`
//! `0x801E6224..0x801E6240`):
//!
//! 1. The hit kills: the target's accumulated damage `+0x0` is not below its
//!    HP `+0x14C` (`sltu v0,a0,a2` at `0x801EE1CC`).
//! 2. The target is a monster seat and the attacker a party seat
//!    (`sltiu v0,a1,0x3` at `0x801EE1D8`, `sltiu v0,v1,0x3` at `0x801EE1E8`).
//! 3. The monster record's `+0x3E` names a Seru (`beq v0,zero` at
//!    `0x801EE260`); its chance is record `+0x3F`, plus a flat `30` when the
//!    attacker's ability word `+0xF8` carries `0x4000` - the Ivory Book's
//!    Magic Boost passive (`0x801EE22C..0x801EE238`).
//! 4. One `rand()` (`jal 0x80056798` at `0x801EE26C`), `% 100` through the
//!    `0x51EB851F` reciprocal, compared `<` the chance. The draw happens on
//!    every Seru kill, whether or not the rest can land.
//! 5. On a hit, `FUN_801E91E8` asks whether the Seru is **already learned**
//!    ([`vm::battle_action::learned_seru_position`] - which scans the acting
//!    character's spell list, not a Miracle string). Only a `0` answer stages
//!    the Seru into `ctx[+0x269]` (`sb v0,0x269(a0)` at `0x801EE2E8`). The
//!    lookup answers `1` - "known" - for a non-party acting slot, for a slot
//!    without its Ra-Seru (`ctx[+0x25F + slot]`, see
//!    [`vm::battle_action::miracle_marker_armed`]) and in a no-reward battle
//!    (`_DAT_8007BAC0`), so none of those can absorb.
//! 6. The Done band sees the non-zero byte: `FUN_801E92DC` prepends spell
//!    `seru + 0x80` to the acting character's list (engine
//!    [`crate::magic_xp::learn_spell_prepend`]) and raises the learn banner
//!    `0x59`, and the band holds `0xB4` frames on its `0x52` arm so the
//!    banner can be read ([`vm::battle_action`]'s Done band, which reaches
//!    this module through `BattleActionHost::learn_absorbed_seru`).
//!
//! This is the retail path, driven off the disc record's two bytes
//! ([`crate::monster_catalog::MonsterDef::absorb_seru`] /
//! [`crate::monster_catalog::MonsterDef::absorb_chance_pct`]). The engine's
//! capture-spell path (`World::resolve_capture` plus the Seru registry) is a
//! separate mechanism and is untouched.
//!
//! One gate of the resolver's head is not modelled, and the engine bounds
//! what it would have bounded: retail reaches the kill check only for hits
//! that pass the per-hit gates at `0x801EE134..0x801EE1A4` (the context's
//! `+0x15` strike cursor and the committed record's per-hit byte at
//! `+0x11 + hit`), so it does not re-check on every hit of a combo that has
//! already killed. The port rolls on the hit whose damage **first** reaches
//! the target's HP, once per target per combo.

use super::*;

/// Ability-word `+0xF8` bit of the Magic Boost passive (the Ivory Book).
pub(in crate::world) const MAGIC_BOOST_BIT: u32 = 0x4000;

/// The flat chance the Magic Boost passive adds (`li a2,0x1e` at
/// `0x801EE238`).
pub(in crate::world) const MAGIC_BOOST_BONUS_PCT: u8 = 30;

impl World {
    /// The killing-blow absorb roll for one hit (module doc, steps 2..5).
    /// `kills` is step 1, decided by the caller's accumulate.
    ///
    /// REF: FUN_801EC3E4 (`0x801EE1C0..0x801EE2E8`, the absorb block)
    pub(in crate::world) fn roll_seru_absorb(&mut self, attacker: u8, target: u8) {
        let attacker_i = usize::from(attacker);
        let target_i = usize::from(target);
        // Party-ness is identity, not seat number, in the port's layout.
        let (Some(att), Some(tgt)) = (self.actors.get(attacker_i), self.actors.get(target_i))
        else {
            return;
        };
        if att.battle_monster_id.is_some() {
            return;
        }
        let Some(monster_id) = tgt.battle_monster_id else {
            return;
        };
        let Some(def) = self.tables.monster_catalog.get(monster_id) else {
            return;
        };
        let (seru, base_pct) = (def.absorb_seru, def.absorb_chance_pct);
        if seru == 0 {
            return;
        }
        let roster = self.party_roster_slot(attacker_i);
        let boosted = self.party.roster.members.get(roster).is_some_and(|rec| {
            let b = rec.ability_bits();
            u32::from_le_bytes([b[4], b[5], b[6], b[7]]) & MAGIC_BOOST_BIT != 0
        });
        let chance = u32::from(base_pct)
            + if boosted {
                u32::from(MAGIC_BOOST_BONUS_PCT)
            } else {
                0
            };
        if self.next_rand() % 100 >= chance {
            return;
        }
        // `FUN_801E91E8` keys its gates and its list off the ACTING slot
        // (`ctx[+0x13]`), which on the arts path is the attacker.
        let acting = self.battle_ctx.active_actor;
        let armed = acting < self.party.party_count
            && self.miracle_marker_armed_for(self.party_roster_slot(usize::from(acting)) as u8);
        let no_reward = self
            .minigames
            .muscle_dome
            .as_ref()
            .is_some_and(|s| s.special_word() != 0);
        let known = self
            .party
            .roster
            .members
            .get(self.party_roster_slot(usize::from(acting)))
            .map(|rec| {
                let list = rec.spell_list();
                let n = usize::from(list.count).min(list.ids.len());
                vm::battle_action::learned_seru_position(seru, armed && !no_reward, &list.ids[..n])
            })
            .unwrap_or(1);
        if known == 0 {
            self.battle_ctx.multi_cast_gate = seru;
        }
    }

    /// The Done band's grant (module doc, step 6): prepend spell
    /// `seru + 0x80` to the acting slot's character record, the list every
    /// menu and the battle spell session read.
    ///
    /// REF: FUN_801E92DC (the grant `FUN_801E295C` calls at `0x801E6234`)
    pub(in crate::world) fn learn_absorbed_seru(&mut self, slot: u8, seru: u8) {
        if seru == 0 || slot >= self.party.party_count {
            return;
        }
        let roster = self.party_roster_slot(usize::from(slot));
        if let Some(rec) = self.party.roster.members.get_mut(roster) {
            crate::magic_xp::learn_spell_prepend(rec, seru.wrapping_add(0x80));
        }
    }
}
