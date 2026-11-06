//! The battle path of the item applier `FUN_800402F4`: the arms a battle item
//! reaches, over the seated actors rather than the character records.
//!
//! `World::use_item` is the field path, and its catalog amounts match retail's
//! there. In battle the routine writes more than the pools: the readout word
//! `+0x10` on every HP arm, the MP readout `+0x178`, the whole status word on a
//! cure, and on a revive the shield byte and the live gauge. The arms are held
//! to the routine whole by the retail kernel differential
//! (`crates/retail-exec/tests/battle_item.rs`).

use super::*;
use legaia_engine_menus::items::ItemOutcome;

/// The Point Card. Its descriptor is an ordinary `(1, 0)`; state `0x3E` of the
/// action SM overwrites the committed class with `0x0E` by item id
/// (`0x801E4080..0x801E40C0`).
pub const POINT_CARD_ITEM: u8 = 0xFE;
/// The selector that discharge runs.
const CLASS_POINT_CARD: u8 = 0x0E;

/// The revive's HP before the arm's overrides: 40% of the maximum plus
/// `rand % (max >> 3)`. The divide has no guard in the routine, so a maximum
/// below 8 leaves the raw draw as the remainder, as the chip does.
///
/// PORT: FUN_800402F4 (class 4, `0x80040F38..0x80040F7C`)
fn revive_roll(max_hp: u16, rand: u32) -> u32 {
    let max = i64::from(max_hp);
    // `mult (max * 2), 0x66666667`, `mfhi`, `sra 1`.
    let forty_percent = (((max * 2) * 0x6666_6667) >> 33) as u32;
    let spread = u32::from(max_hp >> 3);
    forty_percent.wrapping_add(if spread == 0 { rand } else { rand % spread })
}

impl World {
    /// The `(class, tier)` selector an item's battle use runs, off the disc's
    /// effect table. `None` without a table.
    fn battle_item_selector(&self, item_id: u8) -> Option<(u8, u8)> {
        let eff = self.tables.item_effects.as_ref()?.effect(item_id)?;
        Some(if item_id == POINT_CARD_ITEM {
            (CLASS_POINT_CARD, eff.tier)
        } else {
            (eff.class, eff.tier)
        })
    }

    /// An item whose catalog effect is the port's own (a synthetic Bomb,
    /// capture or escape item): no arm of the routine serves it.
    pub(in crate::world) fn item_folds_at_dispatch(&self, item_id: u8) -> bool {
        use legaia_engine_menus::items::ItemEffect;
        let retail_id = self.pack_alias_as_item(item_id).unwrap_or(item_id);
        self.tables.item_catalog.get(retail_id).is_some_and(|e| {
            matches!(
                e.effect,
                ItemEffect::Damage { .. } | ItemEffect::Capture { .. } | ItemEffect::Escape
            )
        })
    }

    /// The applier call of the action SM's Item band: state `0x3F`'s timer has
    /// run out, the acting member's committed item lands on the byte `+0x1DD`
    /// holds now, which the dead-target redirect may have moved since the
    /// commit. Class 1 walks every seat on one side whatever the target
    /// (`0x80040908..0x80040928`). Nothing but a committed Item comes through;
    /// the cast bands fold at their own seams.
    ///
    /// PORT: FUN_801E295C (`0x801E4108..0x801E4138`, the applier call)
    pub(in crate::world) fn fold_battle_item_action(&mut self, target: u8) {
        use legaia_engine_battle_vm::battle_action::ActionCategory;
        let actor = self.battle_ctx.active_actor;
        let Some(a) = self.actors.get(usize::from(actor)) else {
            return;
        };
        if a.battle.action_category != ActionCategory::Item.as_byte() {
            return;
        }
        let item_id = a.battle.params[0];
        if self.item_folds_at_dispatch(item_id) {
            return;
        }
        let retail_id = self.pack_alias_as_item(item_id).unwrap_or(item_id);
        // The selector, not the descriptor: the Point Card's row reads class 1
        // and state `0x3E` overwrites it by id, so the strike takes its one target.
        let whole_party = self
            .battle_item_selector(retail_id)
            .is_some_and(|(class, _)| class == 1)
            || target == legaia_engine_battle_vm::battle_cue_group::TARGET_PARTY_WIDE;
        if whole_party {
            for slot in 0..self.party.party_count {
                let outcome = self.apply_battle_item(item_id, slot);
                self.push_item_use_fx(slot, outcome);
            }
        } else {
            let outcome = self.apply_battle_item(item_id, target);
            self.push_item_use_fx(target, outcome);
        }
    }

    /// Run the battle arm of `item_id` on `target`, for the arms this module
    /// carries. `None` sends the use to the catalog path (the Elixirs, or a
    /// world with no disc table).
    pub(in crate::world) fn battle_applier_arm(
        &mut self,
        item_id: u8,
        target: u8,
    ) -> Option<ItemOutcome> {
        let (class, tier) = self.battle_item_selector(item_id)?;
        Some(match class {
            0 | 1 => self.battle_restore_hp(tier, target),
            2 => self.battle_restore_mp(tier, target),
            3 => self.battle_cure_all(target),
            4 => self.battle_revive(tier, target),
            5 => self.battle_gauge_boost(target),
            8 => self.battle_status_clear(target),
            CLASS_POINT_CARD => self.apply_point_card_strike(target),
            _ => return None,
        })
    }

    /// Classes 0 and 1, per seat. A fallen seat takes nothing, but the readout
    /// word is still assigned on every seat the arm visits (`sw v0,0x10(v1)` at
    /// `0x800408FC` / `0x80040D28`, zero for a seat that gained nothing).
    ///
    /// PORT: FUN_800402F4 (class 0 `0x80040470..0x800408FC`, class 1 `0x80040908..0x80040D28`)
    fn battle_restore_hp(&mut self, tier: u8, target: u8) -> ItemOutcome {
        let cap = self
            .tables
            .item_effects
            .as_ref()
            .map(|t| t.heal_amounts().hp[usize::from(tier.min(2))])
            .unwrap_or(0);
        let Some(a) = self.actors.get(usize::from(target)) else {
            return ItemOutcome::NoEffect;
        };
        let (hp, max) = (a.battle.hp, a.battle.max_hp);
        let mut delta = 0u16;
        if hp != 0 {
            delta = max.wrapping_sub(hp);
            // The compare is signed: a seat over its maximum loses the excess.
            if delta as i16 > cap as i16 {
                delta = cap;
            }
        }
        let outcome = self.run_scale_item_outcome(
            ItemOutcome::HealedHp { amount: delta },
            &legaia_engine_menus::items::TargetSnapshot {
                hp,
                hp_max: max,
                mp: 0,
                mp_max: 0,
                is_dead: hp == 0,
                status_mask: 0,
            },
        );
        let delta = match outcome {
            ItemOutcome::HealedHp { amount } => amount,
            _ => delta,
        };
        let a = &mut self.actors[usize::from(target)].battle;
        a.hp = hp.wrapping_add(delta);
        a.assign_hp_bar(delta as i16);
        if hp == 0 {
            ItemOutcome::NoEffect
        } else {
            ItemOutcome::HealedHp { amount: delta }
        }
    }

    /// Class 2. The arm gates on HP (`beq v0,zero` at `0x80040DB0`), so a fallen
    /// member takes no MP, and it writes `-delta` to the MP readout `+0x178`
    /// whether or not anything moved.
    ///
    /// PORT: FUN_800402F4 (class 2, `0x80040D94..0x80040E58`)
    fn battle_restore_mp(&mut self, tier: u8, target: u8) -> ItemOutcome {
        let cap = self
            .tables
            .item_effects
            .as_ref()
            .map(|t| t.heal_amounts().mp[usize::from(tier.min(2))])
            .unwrap_or(0);
        let i = usize::from(target);
        let Some(a) = self.actors.get(i) else {
            return ItemOutcome::NoEffect;
        };
        let (hp, mp) = (a.battle.hp, a.battle.mp);
        let max = a.battle.mp_max;
        let mut delta = 0u16;
        if hp != 0 {
            delta = max.wrapping_sub(mp);
            if delta as i16 > cap as i16 {
                delta = cap;
            }
            let outcome = self.run_scale_item_outcome(
                ItemOutcome::HealedMp { amount: delta },
                &legaia_engine_menus::items::TargetSnapshot {
                    hp,
                    hp_max: 0,
                    mp,
                    mp_max: max,
                    is_dead: false,
                    status_mask: 0,
                },
            );
            if let ItemOutcome::HealedMp { amount } = outcome {
                delta = amount;
            }
        }
        let a = &mut self.actors[i].battle;
        a.mp = mp.wrapping_add(delta);
        a.last_mp_cost = 0u16.wrapping_sub(delta);
        if hp == 0 {
            ItemOutcome::NoEffect
        } else {
            ItemOutcome::HealedMp { amount: delta }
        }
    }

    /// Clear the whole status word: the raw bits a cast or a hit wrote and the
    /// tracked kinds, which are one halfword in retail.
    fn clear_status_word(&mut self, target: u8) {
        self.battle.status_effects.cure_all(target);
        if let Some(a) = self.actors.get_mut(usize::from(target)) {
            a.battle.field_flags = 0;
        }
    }

    /// Class 3 stores `+0x16E = 0` with no gate at all: Medicine works on the
    /// fallen too. (`FUN_8005133C` ahead of it restores a petrified palette;
    /// the HUD's status sync does that here, on the word losing its marks:
    /// `StatusClutState::sync_word`.)
    ///
    /// PORT: FUN_800402F4 (class 3, `0x80040E64..0x80040EB8`)
    fn battle_cure_all(&mut self, target: u8) -> ItemOutcome {
        let had = self.raw_status_word(target) != 0;
        self.clear_status_word(target);
        if had {
            ItemOutcome::CuredAll
        } else {
            ItemOutcome::NoEffect
        }
    }

    /// Class 4. On a fallen seat: `40% + rand % (max / 8)` of the maximum, or
    /// all of it at a non-zero tier or when a monster is acting (the present
    /// list reads `4` for one, `0x80041018`). It clears the status word, the
    /// `+0x1F9` gauge boost and restores the live gauge from its base. A living
    /// seat draws nothing and takes nothing, but its readout word is assigned
    /// zero all the same.
    ///
    /// The arm's `+0x21C = 2` is the fade that puts the body back on its feet;
    /// `stand_revived_party_member` is that step here.
    ///
    /// PORT: FUN_800402F4 (class 4, `0x80040F14..0x800410D0`)
    fn battle_revive(&mut self, tier: u8, target: u8) -> ItemOutcome {
        let i = usize::from(target);
        let Some(a) = self.actors.get(i) else {
            return ItemOutcome::NoEffect;
        };
        let (hp, max) = (a.battle.hp, a.battle.max_hp);
        if hp != 0 {
            self.actors[i].battle.assign_hp_bar(0);
            return ItemOutcome::NoEffect;
        }
        let rand = self.next_rand();
        let monster_acting = self.battle_ctx.active_actor >= self.party.party_count;
        let restored = if tier != 0 || monster_acting {
            max
        } else {
            revive_roll(max, rand) as u16
        };
        self.clear_status_word(target);
        let a = &mut self.actors[i].battle;
        a.spirit_shield = 0;
        a.agl = a.agl_base;
        a.hp = restored;
        a.assign_hp_bar(restored as i16);
        if restored != 0 {
            a.liveness = 1;
            self.stand_revived_party_member(i);
        }
        ItemOutcome::Revived { hp_after: restored }
    }

    /// Class 5: the gauge boost lands on a living seat only (`beq v0,zero` at
    /// `0x800410F0`).
    ///
    /// PORT: FUN_800402F4 (class 5, `0x800410D4..0x80041108`)
    fn battle_gauge_boost(&mut self, target: u8) -> ItemOutcome {
        if self
            .actors
            .get(usize::from(target))
            .is_none_or(|a| a.battle.hp == 0)
        {
            return ItemOutcome::NoEffect;
        }
        self.apply_fury_boost_item(target)
    }

    /// Class 8: `+0x16E &= 0xFFFC` on a living seat, the raw word and the
    /// tracked poisons alike.
    ///
    /// PORT: FUN_800402F4 (class 8, `0x80041BB0..0x80041C0C`)
    fn battle_status_clear(&mut self, target: u8) -> ItemOutcome {
        use legaia_engine_battle_vm::status_effects::StatusKind;
        let Some(a) = self.actors.get_mut(usize::from(target)) else {
            return ItemOutcome::NoEffect;
        };
        if a.battle.hp == 0 {
            return ItemOutcome::NoEffect;
        }
        let had_raw = a.battle.field_flags & 3 != 0;
        a.battle.field_flags &= 0xFFFC;
        let mut first = None;
        for kind in [StatusKind::Venom, StatusKind::Toxic] {
            if self.battle.status_effects.cure(target, kind) {
                first.get_or_insert(kind);
            }
        }
        match first {
            Some(kind) => ItemOutcome::Cured { kind },
            None if had_raw => ItemOutcome::Cured {
                kind: StatusKind::Venom,
            },
            None => ItemOutcome::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_revive_roll_is_forty_percent_plus_an_eighth_spread() {
        assert_eq!(revive_roll(1000, 0), 400);
        assert_eq!(revive_roll(1000, 124), 524);
        assert_eq!(revive_roll(1000, 125), 400);
        // A maximum under 8 divides by zero; the chip leaves the raw draw.
        assert_eq!(revive_roll(5, 3), 2 + 3);
    }
}
