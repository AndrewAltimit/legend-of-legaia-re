//! The world side of a monster's death spoils ([`crate::battle_steal`]): the
//! steal attack a party member's killing blow can land, the thief's loot a
//! slain thief hands back, and the caption either raises.

use super::*;

impl World {
    /// Run monster seat `slot`'s death commit - the `FUN_8004AD80` arm at
    /// `0x8004B29C`, reached from [`Self::tick_battle_animations`] when the
    /// knockdown clip ends on a monster with no HP left. Idempotent per seat
    /// per battle ([`crate::battle_steal::StealBand::resolved`]).
    ///
    /// The killer is whoever holds the action (`battle_ctx.active_actor`,
    /// retail `ctx[+0x13]`), which is the same seat that is still playing its
    /// swing out when its victim's fall ends.
    ///
    /// Two retail inputs are narrower here than on the disc. The special
    /// battle word `_DAT_8007BAC0` is read as the dome session's word; the
    /// two battle-init raisers in `SCUS_942.54` (first enemy `0xAF`, or
    /// `0x3D..=0x3F` under mode `0xC` / `0x15`, see
    /// `docs/subsystems/minigame-muscle-dome.md`) are not modelled, so those
    /// fights roll a steal retail refuses. And the debug-mode bypass
    /// (`_DAT_8007B98C` with `_DAT_8007BA58`, `0x8004B58C..0x8004B5B0`) is
    /// never taken.
    ///
    /// REF: FUN_8004AD80 (the arm; kernel + `// PORT:` tag in
    /// `crate::battle_steal::resolve_death_spoils`)
    pub(in crate::world) fn resolve_monster_death_spoils(&mut self, slot: usize) {
        use crate::battle_steal::{self as bs, StealAttackInputs};
        let party_count = usize::from(self.party.party_count);
        let Some(m) = slot.checked_sub(party_count) else {
            return;
        };
        if m >= bs::BAND_SEATS || self.battle.steal.resolved[m] {
            return;
        }
        self.battle.steal.resolved[m] = true;
        let acting = usize::from(self.battle_ctx.active_actor);
        let word = |bits: [u8; legaia_save::ABILITY_BITS_LEN], w: usize| {
            u32::from_le_bytes([
                bits[w * 4],
                bits[w * 4 + 1],
                bits[w * 4 + 2],
                bits[w * 4 + 3],
            ])
        };
        let record_bits = |me: &World, member: usize| {
            me.party
                .roster
                .members
                .get(me.party_roster_slot(member))
                .map(|r| r.ability_bits())
        };
        let acting_party = acting < party_count;
        let killer_steals = acting_party
            && record_bits(self, acting).is_some_and(|b| word(b, 0) & bs::STEAL_ATTACK_BIT != 0);
        let items_up = (0..party_count).any(|i| {
            self.actors.get(i).is_some_and(|a| a.battle.hp != 0)
                && record_bits(self, i).is_some_and(|b| word(b, 1) & bs::ITEMS_UP_BIT != 0)
        });
        let entry = self.actors[slot]
            .battle_monster_id
            .and_then(|id| self.tables.steal_table.as_ref()?.entry(id));
        let inputs = StealAttackInputs {
            acting_party,
            special_battle: self
                .minigames
                .muscle_dome
                .as_ref()
                .is_some_and(|s| s.special_word() != 0),
            killer_attack: self
                .actors
                .get(acting)
                .is_some_and(|a| a.battle.action_category == bs::ATTACK_CATEGORY),
            killer_steals,
            items_up,
            entry,
            caption_up: self.battle_ctx.message_id == bs::STEAL_CAPTION_ELEMENT,
        };
        // `FUN_80042F4C` is only ever asked about the row's own item.
        let held = entry
            .and_then(|e| self.party.inventory.get(&e.item_id).copied())
            .unwrap_or(0);
        let mut band = self.battle.steal;
        let spoils = bs::resolve_death_spoils(&mut band, m, &inputs, || self.next_rng(), |_| held);
        self.battle.steal = band;
        self.raise_death_spoils(spoils);
    }

    /// Grant a death-spoils item and put its caption up: the bag add
    /// (`FUN_800421D4(item, 1)`), `ctx[+0x18] = 0x5B`, and the text composed
    /// from the templates on the user's executable.
    fn raise_death_spoils(&mut self, spoils: Option<crate::battle_steal::DeathSpoils>) {
        use crate::battle_steal as bs;
        let Some(spoils) = spoils else {
            return;
        };
        let _ = self.party.inventory.add(spoils.item(), 1);
        self.battle_ctx.message_id = bs::STEAL_CAPTION_ELEMENT;
        let text = bs::compose_caption(&self.battle.ui_strings, spoils).map(|bytes| {
            bs::caption_text(&bytes, |id| {
                self.menu
                    .text
                    .as_ref()
                    .and_then(|t| t.item_name(id))
                    .map(str::to_string)
                    .or_else(|| self.tables.item_catalog.get(id).map(|e| e.name.to_string()))
            })
        });
        log::debug!("battle death spoils: {spoils:?} caption {text:?}");
        if let Some(text) = text {
            self.battle.steal_caption = Some(bs::StealCaption {
                text,
                owner: self.battle_ctx.active_actor,
            });
        }
    }

    /// Record what PROT 0941's Steal took into the stolen band, so the
    /// thief's death hands it back. A party-seat caster has no cell in
    /// retail's band (`[seat - 3]` indexes below it) and records nothing.
    ///
    /// REF: FUN_801F730C (PROT 0941: the `sw` into `0x801C8FE0 + (seat-3)*4`
    /// at `0x801F7960` / `0x801F7A84` and the `[seat + 5]` bump at
    /// `0x801F7AF0`)
    pub(in crate::world) fn stash_cast_steal(
        &mut self,
        caster_slot: u8,
        outcome: vm::cast_arm_ticks::StealOutcome,
    ) {
        use vm::cast_arm_ticks::StealOutcome;
        let Some(m) = usize::from(caster_slot).checked_sub(usize::from(self.party.party_count))
        else {
            return;
        };
        if m >= crate::battle_steal::BAND_SEATS {
            return;
        }
        match outcome {
            StealOutcome::FromBag { item } => self.battle.steal.stolen[m] = item,
            StealOutcome::FromMonster {
                item, hit: true, ..
            } => {
                self.battle.steal.stolen[m] = item;
                self.battle.steal.took[m] = self.battle.steal.took[m].wrapping_add(1);
            }
            _ => {}
        }
    }

    /// Close the caption once a different seat holds the action. Retail
    /// closes element `0x5B` through the HUD element machinery rather than
    /// from this arm; holding it for the rest of the killer's action is the
    /// port's reading of the one capture that shows it
    /// (`player_steal_skeleton_banner`: the caption up beside the finished
    /// combo's `HIT` / `TOTAL`).
    pub(in crate::world) fn tick_steal_caption(&mut self) {
        if self
            .battle
            .steal_caption
            .as_ref()
            .is_some_and(|c| c.owner != self.battle_ctx.active_actor)
        {
            self.battle.steal_caption = None;
        }
    }
}
