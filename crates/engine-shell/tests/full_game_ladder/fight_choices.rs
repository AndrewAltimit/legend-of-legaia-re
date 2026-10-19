//! What the fighter picks on its turn: the item, MP item, fury or power
//! item, spell or arts string worth using, and the Miracle Art reads behind
//! them.

use super::*;

pub(super) fn party_hp_key(w: &legaia_engine_core::world::World) -> u32 {
    let n = w.party.party_count.clamp(1, 3) as usize;
    (0..n).map(|i| u32::from(w.actors[i].battle.hp)).sum()
}

/// What one heal item restores on `slot`, given the HP it would have.
pub(super) fn item_restore(
    w: &legaia_engine_core::world::World,
    id: u8,
    slot: usize,
    hp: u32,
) -> Option<u32> {
    use legaia_engine_menus::items::ItemEffect;
    // A story item the stretch's errand still owes is not a heal.
    if errand_item(w, id) {
        return None;
    }
    let max = u32::from(w.actors[slot].battle.max_hp);
    let e = w.tables.item_catalog.get(id)?;
    Some(match e.effect {
        ItemEffect::Revive { factor } if hp == 0 => (max * u32::from(factor) / 256).max(1),
        ItemEffect::HealAll if hp > 0 => max - hp.min(max),
        ItemEffect::Heal { amount } if hp > 0 => u32::from(amount).min(max - hp.min(max)),
        _ => return None,
    })
}

/// Each member's HP once the items and party heal spells the round's earlier
/// members already committed have landed - so two members never spend their
/// turns on the same wound. A party heal spell is counted at three eighths of
/// each standing member's maximum HP, short of what Orb (~700 on a 1900-HP
/// member) and Spoon (~1150) restore.
pub(super) fn projected_hp(w: &legaia_engine_core::world::World) -> Vec<u32> {
    use legaia_engine_core::battle_round::PendingPartyAction;
    let n = w.party.party_count.clamp(1, 3) as usize;
    let mut hp: Vec<u32> = (0..n).map(|i| u32::from(w.actors[i].battle.hp)).collect();
    for (actor, p) in w.battle.round_flow.pending.iter().enumerate().take(n) {
        if is_party_heal_spell(w, p) {
            for (i, h) in hp.iter_mut().enumerate() {
                let max = u32::from(w.actors[i].battle.max_hp);
                if *h > 0 {
                    *h = (*h + max * 3 / 8).min(max);
                }
            }
            continue;
        }
        let Some(PendingPartyAction::Item { item_id, .. }) = p else {
            continue;
        };
        let targets: Vec<usize> = if w.tables.item_catalog.is_all_party(*item_id) {
            (0..n).collect()
        } else {
            ITEM_TARGET
                .with(|t| t.borrow()[actor])
                .map(usize::from)
                .filter(|&t| t < n)
                .into_iter()
                .collect()
        };
        for t in targets {
            if let Some(r) = item_restore(w, *item_id, t, hp[t]) {
                hp[t] += r;
            }
        }
    }
    hp
}

/// The heal worth using now among `ids`, and the ally it is for: a revive
/// for a member who is down, else a party heal when two or more are in
/// danger, else the smallest single heal that lifts the worst-off member out
/// of danger (the largest when none does). "In danger" is under
/// [`HEAL_BELOW_PCT`] of max HP, or unable to take another hit the size of
/// the biggest one seen this battle; heals the round's earlier members
/// committed count as landed ([`projected_hp`]).
pub(super) fn wanted_item(
    w: &legaia_engine_core::world::World,
    ids: impl Iterator<Item = u8>,
) -> Option<(u8, u8)> {
    let n = w.party.party_count.clamp(1, 3) as usize;
    let mut hp = projected_hp(w);
    // The backup heal. Turn order is drawn per round, so the member who
    // committed this round's heal may act after the foe. Against a
    // party-wide hitter that is the wipe: a member the next hit kills dies
    // before the heal lands (`jagaroom`'s Van Saryu, two Earthquakes of ~600
    // on consecutive rounds through a 1104-HP Vahn, the round's heal still
    // queued behind it). So while some member stands within one such hit of
    // death, one committed heal is not counted as landed and a second member
    // commits one too - whichever acts ahead of the foe saves the party, and
    // the other tops it up after the hit. A third member never does.
    let aoe = AOE_HIT.with(std::cell::Cell::get);
    let committed = w
        .battle
        .round_flow
        .pending
        .iter()
        .take(n)
        .filter(|p| {
            matches!(
                p,
                Some(legaia_engine_core::battle_round::PendingPartyAction::Item { .. })
            )
        })
        .count();
    let raw: Vec<u32> = (0..n).map(|i| u32::from(w.actors[i].battle.hp)).collect();
    // The backup never repeats the committed item when the bag holds only a
    // few: a second Healing Fruit behind the first spends one of two on what
    // the first already does whenever its owner acts ahead of the foe, and
    // the next Earthquake then finds the bag without one. An item the bag
    // holds by the dozen (a Healing Flower) is a fair backup. Nor does it
    // aim a second single heal or revive at the member the first one is for:
    // the backup is for someone else the hit would drop (Xain's single-target
    // swings drew two Flowers onto one member round after round, and the
    // fight ran out the pad budget on heals).
    let mut spent: Vec<u8> = Vec::new();
    let mut covered: Vec<usize> = Vec::new();
    if committed == 1 && aoe > 0 && raw.iter().any(|&h| h > 0 && h <= aoe + aoe / 8) {
        for (actor, p) in w.battle.round_flow.pending.iter().enumerate().take(n) {
            let Some(legaia_engine_core::battle_round::PendingPartyAction::Item {
                item_id, ..
            }) = p
            else {
                continue;
            };
            if w.party.inventory.get(item_id).copied().unwrap_or(0) < 10 {
                spent.push(*item_id);
            }
            if !w.tables.item_catalog.is_all_party(*item_id)
                && let Some(t) = ITEM_TARGET.with(|t| t.borrow()[actor])
            {
                covered.push(usize::from(t));
            }
        }
        hp = raw;
    }
    let threat = BIGGEST_HIT.with(std::cell::Cell::get);
    // The HP a member needs to hold to be out of danger.
    // A solo duel's member tops up below three fifths: its foe can act twice
    // across a round boundary, and two full rounds of Che's swings are most of
    // Gala's HP.
    let duel = solo_duel(w);
    let limit = |i: usize| {
        let max = u32::from(w.actors[i].battle.max_hp);
        let pct = max * heal_below_pct(w) / 100;
        // A party-wide hit's per-member share swings by a tenth or more
        // between casts (Van Saryu's Earthquake: 550, then 609 on a 690-HP
        // Gala), so the margin over it is a quarter.
        // A single hit bigger than the member's whole HP is one no heal
        // outlasts: holding the member near full against it only spends
        // every turn on items (Songi's 3..6-strike combo drops a full
        // 1956-HP Gala, and the hand healed round after round instead of
        // attacking).
        let single = threat + threat / 8;
        let single = if threat >= max { 0 } else { single };
        let hit = single.max(aoe + aoe / 4).min(max * 95 / 100);
        if duel {
            return pct.max(hit).max(max * 3 / 5);
        }
        pct.max(hit)
    };
    // A member a committed revive brings back is still down while the
    // window is open, and the target cursor refuses a heal on it (the
    // confirm bounces): it is not a heal target until it stands.
    let danger = |i: usize| {
        hp[i] > 0
            && w.actors[i].battle.hp > 0
            && !stoned(w, i)
            && hp[i] < limit(i)
            && !covered.contains(&i)
    };
    let dead: Vec<usize> = (0..n)
        .filter(|&i| hp[i] == 0 && !covered.contains(&i))
        .collect();
    let hurt: Vec<usize> = (0..n).filter(|&i| danger(i)).collect();
    if dead.is_empty() && hurt.is_empty() {
        return None;
    }
    let ids: Vec<u8> = ids
        .filter(|&id| {
            !spent.contains(&id)
                && w.tables
                    .item_catalog
                    .get(id)
                    .is_some_and(|e| e.usable_in_battle)
        })
        .collect();
    if let Some(&d) = dead.first() {
        let revive = ids
            .iter()
            .filter_map(|&id| Some((item_restore(w, id, d, 0)?, id)))
            .max();
        if let Some((_, id)) = revive {
            return Some((id, d as u8));
        }
    }
    if hurt.is_empty() {
        return None;
    }
    if hurt.len() >= 2 {
        let party = ids
            .iter()
            .filter(|&&id| w.tables.item_catalog.is_all_party(id))
            .filter_map(|&id| {
                let total: u32 = hurt
                    .iter()
                    .filter_map(|&i| item_restore(w, id, i, hp[i]))
                    .sum();
                (total > 0).then_some((total, id))
            })
            .max();
        // A party item only when it restores more than the best single
        // heal would on the worst-off member: three Healing Blooms' 200
        // apiece lose to one Healing Flower's 800, and spending the turn on
        // the smaller sum is how a party-wide hitter out-damages the heals.
        let worst = *hurt
            .iter()
            .min_by_key(|&&i| hp[i] * 1000 / u32::from(w.actors[i].battle.max_hp).max(1))?;
        let best_single = ids
            .iter()
            .filter(|&&id| !w.tables.item_catalog.is_all_party(id))
            .filter_map(|&id| item_restore(w, id, worst, hp[worst]))
            .max()
            .unwrap_or(0);
        if let Some((total, id)) = party
            && total > best_single
        {
            return Some((id, hurt[0] as u8));
        }
    }
    let worst = *hurt
        .iter()
        .min_by_key(|&&i| hp[i] * 1000 / u32::from(w.actors[i].battle.max_hp).max(1))?;
    // For a lone member a party heal is a heal like any other: Gala's
    // Che Delilas duel (`nilboa`, one member present) found five Healing
    // Fruit in the bag and held the Spirit stance until it fell, because
    // the Fruit's all-party flag kept it off this list.
    let singles: Vec<(u32, u8)> = ids
        .iter()
        .filter(|&&id| n == 1 || !w.tables.item_catalog.is_all_party(id))
        .filter_map(|&id| Some((item_restore(w, id, worst, hp[worst])?, id)))
        .filter(|&(r, _)| r > 0)
        .collect();
    // A lone member has nobody to swing while it heals, so a heal the foe's
    // next round undoes buys no turn: a Healing Leaf's 200 a round into Che
    // Delilas's 300..700 kept Gala alive and never attacking until the battle
    // cap. In a duel the heal must leave the member clear of the line after
    // another round the size of the foe's worst (capped at full HP), and
    // when none does it is the largest.
    let need = if duel {
        (limit(worst) + threat).min(u32::from(w.actors[worst].battle.max_hp))
    } else {
        limit(worst)
    };
    let enough = singles
        .iter()
        .filter(|&&(r, _)| hp[worst] + r >= need)
        .min();
    enough
        .or_else(|| singles.iter().max())
        .map(|&(_, id)| (id, worst as u8))
}

/// The MP the member's strongest damaging spell costs, if it knows one.
pub(super) fn best_spell_cost(w: &legaia_engine_core::world::World, actor: u8) -> Option<u32> {
    use legaia_engine_battle::spells::SpellEffect;
    let m = w
        .party
        .roster
        .members
        .get(w.party_roster_slot(usize::from(actor)))?;
    let list = m.spell_list();
    list.ids[..(list.count as usize).min(list.ids.len())]
        .iter()
        .filter_map(|&id| {
            let d = w.tables.spell_catalog.get(id)?;
            match d.effect {
                SpellEffect::Damage { base_power, .. } => Some((base_power, u32::from(d.mp_cost))),
                _ => None,
            }
        })
        .max()
        .map(|(_, cost)| cost)
}

/// The MP restorative worth drinking now, and who for: in a fight that
/// forbids running, a member who can no longer afford its strongest damage
/// spell drinks the largest MP item the bag offers. Against a boss whose
/// HP dwarfs the party's swings (Songi, 48000), a drink and a Gilium every
/// other turn outdamage two Arts strings.
pub(super) fn wanted_mp_item(
    w: &legaia_engine_core::world::World,
    ids: impl Iterator<Item = u8>,
    actor: u8,
) -> Option<(u8, u8)> {
    use legaia_engine_menus::items::ItemEffect;
    if !w.battle.no_escape {
        return None;
    }
    // A duellist with no restorative left and too little MP for its heal
    // spell ([`spell_heal`]) drinks the largest MP item: a Magic Fruit
    // is four Spoons.
    if solo_duel(w) {
        let a = &w.actors.get(usize::from(actor))?.battle;
        if bag_can_heal(w)
            || a.hp == 0
            || u32::from(a.hp) * 5 >= u32::from(a.max_hp) * 3
            || spell_heal(w, actor).is_some()
            || !knows_heal_spell(w, actor)
        {
            return None;
        }
        return ids
            .filter_map(|id| {
                let e = w.tables.item_catalog.get(id)?;
                match e.effect {
                    ItemEffect::HealMp { amount } if e.usable_in_battle => Some((amount, id)),
                    _ => None,
                }
            })
            .max()
            .map(|(_, id)| (id, actor));
    }
    // Only against a foe of twenty times the party's largest HP or more
    // (Songi, 48000 to ~1950): in a shorter fight the drinks cost turns the
    // Arts and heals win it with (Van Saryu at 11x lost two seeds in ten).
    let n = w.party.party_count.clamp(1, 3) as usize;
    let ours = (0..n)
        .map(|i| u32::from(w.actors[i].battle.max_hp))
        .max()
        .unwrap_or(1)
        .max(1);
    if !(n..w.actors.len()).any(|i| {
        let a = &w.actors[i].battle;
        a.hp > 0 && u32::from(a.max_hp) >= ours * 20
    }) {
        return None;
    }
    let cost = best_spell_cost(w, actor)?;
    let mp = u32::from(w.actors.get(usize::from(actor))?.battle.mp);
    if mp >= cost {
        return None;
    }
    ids.filter_map(|id| {
        let e = w.tables.item_catalog.get(id)?;
        match e.effect {
            ItemEffect::HealMp { amount } if e.usable_in_battle => Some((amount, id)),
            _ => None,
        }
    })
    // The smallest drink that pays for the spell, else the largest.
    .filter(|&(a, _)| mp + u32::from(a) >= cost)
    .min()
    .map(|(_, id)| (id, actor))
}

/// The Fury Boost worth using now, and on whom (the acting member itself):
/// in a fight that forbids running against a foe of ten times the party's
/// largest HP or more, a member whose Miracle Art string costs more than its
/// command pool, but not more than the pool the boost buys. The item raises
/// the actor's `+0x1F9` charge byte, and every round boundary after it
/// restores the pool `+0x154` to `base * 7 / 5 + 8` (cap `0x120`) instead of
/// the base (`FUN_801D88CC` loop A, `World::apply_fury_boost_item`), so one
/// turn spent buys a Miracle Art every round after. Koru's timed fight
/// (`0xB6`: 20000 HP, the all-party finisher on round counter `4`) is lost on
/// spells and short Arts strings alone.
pub(super) fn wanted_fury_item(
    w: &legaia_engine_core::world::World,
    ids: impl Iterator<Item = u8>,
    actor: u8,
) -> Option<(u8, u8)> {
    use legaia_engine_menus::items::ItemEffect;
    if !w.battle.no_escape {
        return None;
    }
    let n = w.party.party_count.clamp(1, 3) as usize;
    let i = usize::from(actor);
    if i >= n || w.battle.fury_boost.get(i).copied().flatten().is_some() {
        return None;
    }
    let ours = (0..n)
        .map(|k| u32::from(w.actors[k].battle.max_hp))
        .max()
        .unwrap_or(1)
        .max(1);
    // A solo duel ([`solo_duel`]) is such a fight too: its member's Arts are
    // all the damage the fight gets.
    if !solo_duel(w)
        && !(n..w.actors.len()).any(|k| {
            let a = &w.actors[k].battle;
            a.hp > 0 && u32::from(a.max_hp) >= ours * 10
        })
    {
        return None;
    }
    if !miracle_armed(w, actor) {
        return None;
    }
    let roster = w.party_roster_slot(i);
    let character = legaia_engine_battle::battle_arts::character_for_slot(roster as u8);
    let miracle = legaia_art::MIRACLE_ARTS
        .iter()
        .find(|m| m.character == character)?;
    // Every direction costs the same outside the weapon arm (`+0x74`), and the
    // Miracle string is typed with whatever the arm costs: price it at the
    // dearest press, as the entry will.
    let press = w
        .battle
        .swing_costs
        .get(roster)
        .copied()
        .unwrap_or([legaia_engine_battle::arts_command_input::FAVORED_COST; 4])
        .into_iter()
        .max()
        .unwrap_or(u16::MAX);
    let cost = u32::from(press) * miracle.commands.len() as u32;
    let base = u32::from(w.actors[i].battle.agl_base);
    let boosted = (base * 7 / 5 + 8).min(0x120);
    if cost <= base || cost > boosted {
        return None;
    }
    let mut ids = ids;
    ids.find(|&id| {
        w.tables
            .item_catalog
            .get(id)
            .is_some_and(|e| e.usable_in_battle && matches!(e.effect, ItemEffect::ActionGauge))
    })
    .map(|id| (id, actor))
}

/// The attack Elixir worth handing out now, and to whom: in the same fights
/// as [`wanted_fury_item`], a member that will not type a Miracle Art itself
/// gives one to a member that does (Fury-boosted or already paid for) and
/// carries no Attack buff yet. The class-7 Elixir ramps the target's attack
/// scalar `x6/5` for the rest of the battle, and a Miracle Art is one strike
/// against Koru's defence, so the ramp buys more than the summon the giver
/// gives up.
pub(super) fn wanted_power_item(
    w: &legaia_engine_core::world::World,
    ids: impl Iterator<Item = u8>,
    actor: u8,
) -> Option<(u8, u8)> {
    use legaia_engine_battle::spells::BuffStat;
    use legaia_game_tables::item_effect::{StatItemEffect, StatTarget};
    if !w.battle.no_escape {
        return None;
    }
    let n = w.party.party_count.clamp(1, 3) as usize;
    let table = w.tables.item_effects.as_ref()?;
    let attack_elixir = |ids: &mut dyn Iterator<Item = u8>| {
        ids.filter(|&id| {
            w.tables
                .item_catalog
                .get(id)
                .is_some_and(|e| e.usable_in_battle)
                && matches!(table.stat_effect(id),
                    Some(StatItemEffect::BuffOneBattle(stats)) if stats.contains(&StatTarget::Attack))
        })
        // The single-stat Elixir before the four-stat one.
        .min_by_key(|&id| match table.stat_effect(id) {
            Some(StatItemEffect::BuffOneBattle(stats)) => stats.len(),
            _ => usize::MAX,
        })
    };
    // A solo duel's member drinks one itself, once, before its first Arts
    // round: nobody else swings, and the ramp lasts the whole fight. Gala's
    // Che Delilas duel (`nilboa`) is about thirty Arts rounds against a
    // 12000-HP foe out of one bag that has to last two more duels.
    if solo_duel(w) {
        let mine = w
            .battle
            .buffs
            .iter()
            .any(|b| b.slot == actor && b.stat == BuffStat::Attack);
        if mine || usize::from(actor) >= n || miracle_armed_fury(w, actor) {
            return None;
        }
        let mut ids = ids;
        return attack_elixir(&mut ids).map(|id| (id, actor));
    }
    if usize::from(actor) >= n || miracle_ready(w, actor) || miracle_armed_fury(w, actor) {
        return None;
    }
    let ours = (0..n)
        .map(|k| u32::from(w.actors[k].battle.max_hp))
        .max()
        .unwrap_or(1)
        .max(1);
    if !(n..w.actors.len()).any(|k| {
        let a = &w.actors[k].battle;
        a.hp > 0 && u32::from(a.max_hp) >= ours * 10
    }) {
        return None;
    }
    let target = (0..n as u8).find(|&m| {
        m != actor
            && w.actors[usize::from(m)].battle.hp > 0
            && (w
                .battle
                .fury_boost
                .get(usize::from(m))
                .copied()
                .flatten()
                .is_some()
                || miracle_ready(w, m))
            && !w
                .battle
                .buffs
                .iter()
                .any(|b| b.slot == m && b.stat == BuffStat::Attack)
    })?;
    let mut ids = ids;
    attack_elixir(&mut ids).map(|id| (id, target))
}

/// Whether `actor` is (or is about to be) a Miracle typist through a Fury
/// Boost: boosted already, or [`wanted_fury_item`] would buy one.
pub(super) fn miracle_armed_fury(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    w.battle
        .fury_boost
        .get(usize::from(actor))
        .copied()
        .flatten()
        .is_some()
        && miracle_armed(w, actor)
        || wanted_fury_item(
            w,
            w.party
                .inventory
                .iter()
                .filter(|(_, c)| **c > 0)
                .map(|(id, _)| *id),
            actor,
        )
        .is_some()
}

/// Whether `actor` can type its Miracle Art this turn: the marker is armed
/// and the live command pool (`+0x154`, what the entry seeds from) pays for
/// the whole string at the dearest press.
pub(super) fn miracle_ready(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    let i = usize::from(actor);
    if i >= w.party.party_count.clamp(1, 3) as usize || !miracle_armed(w, actor) {
        return false;
    }
    let roster = w.party_roster_slot(i);
    let character = legaia_engine_battle::battle_arts::character_for_slot(roster as u8);
    let Some(miracle) = legaia_art::MIRACLE_ARTS
        .iter()
        .find(|m| m.character == character)
    else {
        return false;
    };
    let press = w
        .battle
        .swing_costs
        .get(roster)
        .copied()
        .unwrap_or([legaia_engine_battle::arts_command_input::FAVORED_COST; 4])
        .into_iter()
        .max()
        .unwrap_or(u16::MAX);
    u32::from(press) * miracle.commands.len() as u32 <= u32::from(w.actors[i].battle.agl)
}

/// Whether `actor`'s Miracle marker is armed (a Ra-Seru equipped): the queue
/// builder replaces a typed Miracle string with the Miracle row only then.
pub(super) fn miracle_armed(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    let roster = w.party_roster_slot(usize::from(actor));
    w.party.roster.members.get(roster).is_some_and(|m| {
        legaia_engine_battle_vm::battle_action::miracle_marker_armed(
            (roster as u8).wrapping_add(1),
            &m.equipment().slots,
        )
    })
}

/// The Magic row worth casting: the strongest affordable damage spell.
pub(super) fn wanted_spell(
    w: &legaia_engine_core::world::World,
    m: &legaia_engine_battle::battle_magic::BattleSpellSession,
) -> Option<usize> {
    use legaia_engine_battle::spells::SpellEffect;
    // The finishing blow ([`going_for_the_kill`]): the strongest damage
    // spell the member can pay for, no heal and no MP kept back.
    if going_for_the_kill(w, m.actor) {
        return m
            .spells
            .iter()
            .enumerate()
            .filter(|(_, r)| r.affordable)
            .filter_map(|(i, r)| match w.tables.spell_catalog.get(r.id)?.effect {
                SpellEffect::Damage { base_power, .. } => {
                    Some((i, u32::from(base_power) * spell_affinity_pct(w, r.id)))
                }
                _ => None,
            })
            .max_by_key(|&(_, p)| p)
            .map(|(i, _)| i);
    }
    // A heal spell worth casting ([`spell_heal`]) comes before any damage.
    if let Some(heal) = spell_heal(w, m.actor) {
        return m.spells.iter().position(|r| r.id == heal && r.affordable);
    }
    // The continuous hand keeps half its MP for the fights it cannot run
    // from: a random encounter is cast at only out of the upper half. So is
    // a staged fight short of a boss on the way to one: `taiku`'s P1[2]
    // pair (1600 HP each) took Vahn from 197 MP to 24 two rooms before Zora,
    // with no bed between and no MP item in the bag.
    let saving = continuous::SAVING_FOR_BOSS.with(std::cell::Cell::get) && !boss_fight(w);
    let floor =
        if continuous::PLAYS_ON.with(std::cell::Cell::get) && (!w.battle.no_escape || saving) {
            w.party
                .roster
                .members
                .get(w.party_roster_slot(usize::from(m.actor)))
                .map_or(0, |r| u32::from(r.hp_mp_sp().mp_max) / 2)
        } else {
            0
        };
    // Against a boss, a member who knows a party heal keeps the MP for two
    // casts of it: one Spoon is ~1150 on each member, worth more than any
    // summon it pays for, and a Vahn who spent his pool on two Viguros met
    // Zora's (`taiku` P2[19]) third party-wide hit with three MP and a dry bag.
    // What a cast has measurably taken off the foes this battle ([`DEALT`]),
    // per spell: the same spell is not the same hit from every caster. The
    // attacker roll scales by the caster's own magic level before the
    // defender roll is subtracted (`FUN_801DD864`), so Gala's level-7
    // Slippery took 3171 off Songi where Vahn's level-3 one took 862. A
    // spell not cast yet is estimated from the catalog power, scaled by how
    // this caster's measured casts compare with their own estimates.
    let est = |id: u8| -> Option<u32> {
        match w.tables.spell_catalog.get(id)?.effect {
            SpellEffect::Damage { base_power, .. } => {
                Some(u32::from(base_power) * spell_affinity_pct(w, id))
            }
            _ => None,
        }
    };
    let seen: Vec<(u8, u32)> = DEALT.with(|d| {
        d.borrow()
            .iter()
            .filter(|((a, k), _)| *a == m.actor && *k != ART_KIND)
            .map(|((_, k), &dmg)| (*k as u8, dmg))
            .collect()
    });
    let ratio: Option<f64> = {
        let r: Vec<f64> = seen
            .iter()
            .filter_map(|&(id, dmg)| {
                est(id)
                    .filter(|&e| e > 0)
                    .map(|e| f64::from(dmg) / f64::from(e))
            })
            .collect();
        (!r.is_empty()).then(|| r.iter().sum::<f64>() / r.len() as f64)
    };
    let score = |id: u8| -> Option<u64> {
        let e = est(id)?;
        Some(match (seen.iter().find(|&&(s, _)| s == id), ratio) {
            (Some(&(_, dmg)), _) => u64::from(dmg),
            (None, Some(r)) => (f64::from(e) * r) as u64,
            (None, None) => u64::from(e),
        })
    };
    // A caster whose measured spell hits at least twice what its Arts do
    // keeps back one party heal, not two: Gala's second Slippery was worth
    // more than the Spoon her reserve held it for.
    let art = DEALT.with(|d| d.borrow().get(&(m.actor, ART_KIND)).copied());
    let best_seen = seen.iter().map(|&(_, d)| d).max().unwrap_or(0);
    let reserve = heal_spell_reserve(w, m.actor);
    let reserve = if art.is_some_and(|a| best_seen >= a.saturating_mul(2)) {
        reserve / 2
    } else {
        reserve
    };
    let floor = floor.max(reserve);
    let mp = w
        .actors
        .get(usize::from(m.actor))
        .map_or(0, |a| u32::from(a.battle.mp));
    m.spells
        .iter()
        .enumerate()
        .filter(|(_, r)| r.affordable)
        .filter(|(_, r)| {
            floor == 0
                || w.tables
                    .spell_catalog
                    .get(r.id)
                    .is_some_and(|d| mp >= floor + u32::from(d.mp_cost))
        })
        .filter_map(|(i, r)| score(r.id).map(|s| (i, s)))
        .max_by_key(|&(_, p)| p)
        .map(|(i, _)| i)
}

/// The MP a member keeps back from damage spells in a boss fight
/// ([`boss_fight`]) with allies beside it: two casts of the largest party
/// heal it knows, `0` when it knows none.
pub(super) fn heal_spell_reserve(w: &legaia_engine_core::world::World, actor: u8) -> u32 {
    use legaia_engine_battle::spells::SpellEffect;
    if w.party.party_count.clamp(1, 3) < 2 || !boss_fight(w) {
        return 0;
    }
    let Some(m) = w
        .party
        .roster
        .members
        .get(w.party_roster_slot(usize::from(actor)))
    else {
        return 0;
    };
    let list = m.spell_list();
    list.ids[..(list.count as usize).min(list.ids.len())]
        .iter()
        .filter_map(|&id| {
            let d = w.tables.spell_catalog.get(id)?;
            match d.effect {
                SpellEffect::HealAll { amount } => Some((amount, u32::from(d.mp_cost))),
                _ => None,
            }
        })
        .max()
        .map_or(0, |(_, cost)| cost * 2)
}

/// The element-affinity percent a Seru-magic cast lands with:
/// `matrix[summon creature element][foe element]` (`FUN_801dd864`), the
/// attacker element being the summoned creature's record `+0x1D`, not the
/// caster's. The segmented hands read the first standing foe, the one the
/// target cursor opens on; the continuous hand aims ([`aimed_foe`]), so it
/// reads the best standing foe. `100` when either element is unknown.
pub(super) fn spell_affinity_pct(w: &legaia_engine_core::world::World, spell_id: u8) -> u32 {
    let n = w.party.party_count.clamp(1, 3) as usize;
    let mut foes = (n..w.actors.len()).filter(|&i| w.actors[i].battle.hp > 0);
    if continuous::PLAYS_ON.with(std::cell::Cell::get) {
        foes.map(|i| spell_affinity_on(w, spell_id, i))
            .max()
            .unwrap_or(100)
    } else {
        foes.next()
            .map_or(100, |i| spell_affinity_on(w, spell_id, i))
    }
}

/// [`spell_affinity_pct`] against the foe at actor index `foe`.
pub(super) fn spell_affinity_on(
    w: &legaia_engine_core::world::World,
    spell_id: u8,
    foe: usize,
) -> u32 {
    let Some(aff) = w.tables.element_affinity.as_ref() else {
        return 100;
    };
    let Some(atk) = w.tables.summon_creatures.get(&spell_id).map(|d| d.element) else {
        return 100;
    };
    let Some(def) = w.actors.get(foe).and_then(|a| {
        a.battle_element.or_else(|| {
            w.tables
                .monster_catalog
                .get(a.battle_monster_id?)
                .map(|d| d.element)
        })
    }) else {
        return 100;
    };
    u32::from(aff.affinity_pct(atk, def).unwrap_or(100))
}

/// The monster row (`0..5`, actor `party_count + row`) the continuous hand
/// aims a single-foe strike or cast at, or `None` to take the cursor where
/// it opens (the segmented hands always do).
///
/// A cast goes to the standing foe it lands hardest on (the element
/// matrix, [`spell_affinity_on`]); a strike, and a tie, to the standing foe
/// with the least HP left - a foe killed a round sooner takes no more turns.
/// Stepping the cursor there drives the picker's Left / Right arm, retail's
/// angular attack-target ring (`FUN_801D8A88` -> `FUN_801D8D00`), which a
/// hand that only confirms the opening slot never reaches.
pub(super) fn aimed_foe(w: &legaia_engine_core::world::World, spell: Option<u8>) -> Option<u8> {
    if !continuous::PLAYS_ON.with(std::cell::Cell::get) {
        return None;
    }
    let n = w.party.party_count.clamp(1, 3) as usize;
    (0..5u8)
        .filter(|&r| {
            w.actors
                .get(n + usize::from(r))
                .is_some_and(|a| a.battle.hp > 0 && a.battle.max_hp > 0)
        })
        .max_by_key(|&r| {
            let i = n + usize::from(r);
            let pct = spell.map_or(100, |id| spell_affinity_on(w, id, i));
            (
                pct,
                std::cmp::Reverse(w.actors[i].battle.hp),
                std::cmp::Reverse(r),
            )
        })
}

thread_local! {
    /// Each character's Super Art physical inputs ([`arts_plan`]), derived
    /// once per process.
    pub(super) static SUPER_INPUTS: std::cell::RefCell<
        HashMap<legaia_art::Character, Vec<Vec<legaia_art::Command>>>,
    > = Default::default();
}

/// The arts string worth entering, the way the queue builder will read it
/// (`FUN_801EED1C`, `World::build_arts_action_queue`):
///
/// - the character's **Miracle Art** string, when the slot's Miracle marker
///   is armed (a Ra-Seru equipped) and the command pool pays for all of it -
///   the builder replaces the whole queue with the Miracle row;
/// - else a **Super Art**'s physical input ([`legaia_art::derive_super_input`]
///   over the trigger table's `find` pattern), the longest the pool pays for;
/// - else the longest **normal** arts (constants `0x1F` up, two arrows or
///   more) the pool pays for, repeated while they fit, then the cheapest
///   plain direction until nothing more is affordable.
///
/// The Miracle and the three Hyper arts (constants `0x1B..=0x1E`) never
/// tokenize as arts on their own strings - the builder only reaches them
/// through the Miracle copy and the Super tail-replace - so typing a Hyper
/// art's string swings plain arrows. The fighter did exactly that for Vahn
/// (Fiery Miyawaki, `2 3 1 3 1`) and Noa (Illusion Kick), whose "arts" never
/// spent the gauge.
pub(super) fn arts_plan(
    w: &legaia_engine_core::world::World,
    s: &legaia_engine_battle::arts_command_input::ArtsCommandInputSession,
) -> Vec<u8> {
    use legaia_art::Command;
    // Nine: each character's Miracle Art is a nine-command string
    // (`art-data.md`), and a cap below it kept the hand from ever entering
    // one. The pool still decides what fits; this only stops the plan.
    const MAX_ENTRY: usize = 9;
    const NORMAL_ART_MIN_CONSTANT: u8 = 0x1F;
    // The occupying character's table: the actor's `character` key is only
    // written by the first arts commit, so before it every member reads
    // Vahn's.
    let roster = w.party_roster_slot(usize::from(s.actor));
    let character = legaia_engine_battle::battle_arts::character_for_slot(roster as u8);
    let cost = |cmds: &[Command]| -> u16 { cmds.iter().map(|&c| s.cost_of(c)).sum() };
    // A rotted limb refuses its directions at entry (`rot_blocks`, the
    // `0x801D1E60..` gate): a plan through one waits on a press that never
    // lands, and the hand held Left against a rotted arm until the battle
    // ran out (`jouinc`, after a party-wide Rot).
    let status = rot_status(w, s.actor);
    let open = |cmds: &[Command]| {
        !cmds
            .iter()
            .any(|&c| legaia_engine_battle::arts_command_input::rot_blocks(status, c))
    };
    let mut normal: Vec<(legaia_art::ActionConstant, &[Command])> = w
        .tables
        .art_records
        .iter()
        .filter(|((c, a), r)| {
            *c == character && r.commands.len() >= 2 && a.as_byte() >= NORMAL_ART_MIN_CONSTANT
        })
        .map(|((_, a), r)| (*a, r.commands.as_slice()))
        .collect();
    normal.sort_by_key(|(a, _)| a.as_byte());
    let bytes = |cmds: &[Command]| cmds.iter().map(|c| c.as_byte()).collect::<Vec<u8>>();
    if miracle_armed(w, s.actor)
        && let Some(m) = legaia_art::MIRACLE_ARTS
            .iter()
            .find(|m| m.character == character)
        && cost(m.commands) <= s.pool_max
        && m.commands.len() <= MAX_ENTRY
        && open(m.commands)
    {
        return bytes(m.commands);
    }
    // The derivation searches up to `4^12` strings, and the hand re-plans on
    // every pad poll of the entry: derive each character's Super inputs once.
    let supers = SUPER_INPUTS.with(|c| {
        c.borrow_mut()
            .entry(character)
            .or_insert_with(|| {
                legaia_art::SUPER_ARTS
                    .iter()
                    .filter(|sa| sa.character == character)
                    .filter_map(|sa| legaia_art::derive_super_input(&normal, sa.find))
                    .collect()
            })
            .clone()
    });
    if let Some(input) = supers
        .into_iter()
        .filter(|inp| inp.len() <= MAX_ENTRY && cost(inp) <= s.pool_max && open(inp))
        .max_by_key(|inp| inp.len())
    {
        return bytes(&input);
    }
    let arts: Vec<&[Command]> = normal.iter().map(|(_, c)| *c).filter(|c| open(c)).collect();
    let mut plan: Vec<Command> = Vec::new();
    let mut pool = s.pool_max;
    loop {
        let art = arts
            .iter()
            .filter(|c| cost(c) <= pool && plan.len() + c.len() <= MAX_ENTRY)
            .max_by_key(|c| {
                (
                    c.len(),
                    std::cmp::Reverse(c.iter().map(|x| x.as_byte()).collect::<Vec<u8>>()),
                )
            });
        if let Some(c) = art {
            pool -= cost(c);
            plan.extend_from_slice(c);
            continue;
        }
        let Some(&d) = [Command::Left, Command::Right, Command::Down, Command::Up]
            .iter()
            .filter(|&&d| s.cost_of(d) <= pool && open(&[d]))
            .min_by_key(|&&d| s.cost_of(d))
        else {
            break;
        };
        if plan.len() >= MAX_ENTRY {
            break;
        }
        pool -= s.cost_of(d);
        plan.push(d);
    }
    plan.into_iter().map(|c| c.as_byte()).collect()
}

/// The ring press for one arts-entry direction byte.
pub(super) fn direction_mask(b: u8) -> u16 {
    use legaia_art::queue::Command;
    match Command::from_byte(b) {
        Some(Command::Left) => PadButton::Left.mask(),
        Some(Command::Right) => PadButton::Right.mask(),
        Some(Command::Down) => PadButton::Down.mask(),
        _ => PadButton::Up.mask(),
    }
}

/// Does the command ring refuse this member's Attack arm? All three Rot
/// limbs (`+0x16E & 0x38 == 0x38`) leave the arts entry no direction, and
/// the ring answers the Left press with the refusal cue and stays up
/// (`FUN_801D0748`, `0x801D1560..0x801D156C`; `ring_arm_refused` in the
/// engine). Sol's monster 120 rots limbs.
pub(super) fn attack_refused(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    rot_status(w, actor) & 0x38 == 0x38
}

/// The `+0x16E` status word the arts entry's Rot gate reads for `actor`.
fn rot_status(w: &legaia_engine_core::world::World, actor: u8) -> u16 {
    w.actors
        .get(usize::from(actor))
        .map_or(0, |a| a.battle.field_flags)
        | legaia_engine_battle_vm::status_effects::pack_display_flags(
            w.battle.status_effects.statuses(actor),
        )
}
