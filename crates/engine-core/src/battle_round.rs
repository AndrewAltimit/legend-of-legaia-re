//! Per-round battle lifecycle - wires the pure-code subsystems
//! (`ap_gauge`, `battle_stats`, `legaia_engine_vm::status_effects`,
//! `items`) into the actor / battle-action chain.
//!
//! A "round" in retail Legaia is one full pass through the turn-order
//! list - each actor either acts or has their action skipped because of
//! a status effect. The retail engine threads three operations across
//! that round boundary:
//!
//!   1. Refresh per-character AP at round-start (`reset_for_turn`).
//!   2. Recompute per-actor [`BattleStats`] from equipment, status, and
//!      ability bits (the retail aggregator at `FUN_80042558`).
//!   3. Tick status effects at round-end (Toxic / Venom drain HP;
//!      Sleep / Stone / Faint might expire).
//!
//! This module ties those operations together against the existing
//! `World` API so engines call `BattleRound::begin(&mut world, ...)` at
//! turn start and `BattleRound::end(&mut world)` at turn end. The `World`
//! struct already carries the ApGauge / StatusEffectTracker /
//! ItemCatalog state - `BattleRound` is the orchestrator that drives
//! them in retail order.

use crate::battle_stats::{
    BattleStats, EquipmentTable, StatRecord, StatusModifiers, compute_battle_stats,
};
use crate::world::World;
use legaia_engine_vm::status_effects::StatusKind;

/// One round of battle. Constructed by [`BattleRound::begin`]; consumed
/// by [`BattleRound::end`].
///
/// Holds the per-slot resolved stats so engines can read them during
/// the round (e.g. a HUD that displays current ATK / DEF can pull
/// `round.stats[slot]`).
#[derive(Debug, Clone, Default)]
pub struct BattleRound {
    /// Per-slot resolved stats for the active round (slots 0..3 are
    /// party; 3..8 are monsters). Slots without a registered actor
    /// have the default zero-stats.
    pub stats: [BattleStats; 8],
    /// `true` for slots whose actor is asleep / stunned / petrified -
    /// used by the action validator to filter out command input.
    pub action_blocked: [bool; 8],
    /// `true` for slots whose actor is silenced or petrified.
    pub magic_blocked: [bool; 8],
}

/// Per-slot input the engine passes into [`BattleRound::begin`] -
/// the retail equivalent of reading the character record + equipment
/// table.
#[derive(Debug, Clone, Default)]
pub struct ActorRoundInput {
    /// Base stat record for this slot (None → skip; e.g. for empty
    /// monster slots).
    pub record: Option<StatRecord>,
}

impl BattleRound {
    /// Begin a new round.
    ///
    /// 1. Resets every party-member's AP gauge (`reset_for_turn`).
    /// 2. Computes per-slot [`BattleStats`] from each provided
    ///    [`StatRecord`] and the active status set.
    /// 3. Writes the resolved attack / defense values back into
    ///    `World::battle_attack` / `battle_defense` so the strike
    ///    resolver picks them up.
    pub fn begin(
        world: &mut World,
        per_slot: &[Option<StatRecord>; 8],
        equipment: &EquipmentTable,
        modifiers: &StatusModifiers,
    ) -> Self {
        // 1. AP refresh (matches retail per-turn AP reset).
        world.reset_party_ap();

        let mut round = BattleRound::default();
        // 2. Per-slot stat resolution.
        for (i, slot) in per_slot.iter().enumerate() {
            let Some(record) = slot else {
                continue;
            };
            let kinds: Vec<StatusKind> = world
                .status_effects
                .statuses(i as u8)
                .iter()
                .map(|s| s.kind)
                .collect();
            let stats = compute_battle_stats(record, equipment, &kinds, modifiers);
            round.stats[i] = stats;
            round.action_blocked[i] = stats.action_blocked;
            round.magic_blocked[i] = stats.magic_blocked;
            // 3. Push attack / defense into world.
            world.set_battle_attack(i as u8, stats.atk);
            world.set_battle_defense_split(i as u8, Some((stats.udf, stats.ldf)));
        }
        round
    }

    /// End-of-round bookkeeping. Ticks every actor's status effects,
    /// folds Toxic / Venom tick damage into `BattleActor::hp`.
    /// Returns the number of actors that died from tick damage this
    /// round (battle UI can use this to surface the death cue).
    pub fn end(world: &mut World) -> u32 {
        let actor_count = world.actors.len();
        let pre: Vec<u16> = world.actors.iter().map(|a| a.battle.hp).collect();
        world.tick_status_effects();
        let mut deaths = 0u32;
        for (i, prev_hp) in pre.iter().enumerate().take(actor_count) {
            if let Some(actor) = world.actors.get(i)
                && *prev_hp > 0
                && actor.battle.hp == 0
            {
                deaths += 1;
            }
        }
        deaths
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ap_gauge::ApGauge;
    use crate::battle_stats::{ItemModifier, StatRecord};
    use crate::world::World;
    use legaia_art::record::EnemyEffect;

    /// Build a minimal World with three party members for round tests.
    fn world_with_party() -> World {
        let mut w = World::default();
        // Need 8 actor slots so the BattleRound stat array maps cleanly.
        while w.actors.len() < 8 {
            w.actors.push(crate::world::Actor::default());
        }
        w
    }

    #[test]
    fn begin_round_resets_ap_for_every_party_member() {
        let mut world = world_with_party();
        // Burn AP on every member.
        for g in world.ap_gauges.iter_mut() {
            *g = ApGauge::with_base(8);
            assert!(g.try_spend(2));
        }
        let blanks: [Option<StatRecord>; 8] = Default::default();
        let _round = BattleRound::begin(
            &mut world,
            &blanks,
            &EquipmentTable::new(),
            &StatusModifiers::default(),
        );
        for g in world.ap_gauges.iter() {
            assert_eq!(g.current_ap, g.base_ap);
        }
    }

    #[test]
    fn begin_round_computes_stats_from_record_plus_equipment() {
        let mut world = world_with_party();
        let mut equipment = EquipmentTable::new();
        equipment.set(
            10,
            ItemModifier {
                atk: 7,
                ..Default::default()
            },
        );
        let record = StatRecord {
            base_attack: 100,
            base_udf: 50,
            base_ldf: 40,
            base_accuracy: 80,
            base_evasion: 30,
            base_spd: 45,
            base_int: 20,
            equip: [10, 0, 0, 0, 0, 0, 0, 0],
        };
        let mut per_slot: [Option<StatRecord>; 8] = Default::default();
        per_slot[0] = Some(record);

        let round = BattleRound::begin(
            &mut world,
            &per_slot,
            &equipment,
            &StatusModifiers::default(),
        );

        assert_eq!(round.stats[0].atk, 107);
        // World snapshot the values for the strike resolver.
        assert_eq!(world.battle_attack[0], 107);
        // UDF / LDF split written.
        let (udf, ldf) = world.battle_defense_split[0].expect("split written by begin");
        assert_eq!(udf, 50);
        assert_eq!(ldf, 40);
    }

    #[test]
    fn begin_round_marks_action_blocked_for_asleep_actor() {
        let mut world = world_with_party();
        // Mark slot 1 as Sleep before begin (Other(4) = Sleep).
        world
            .status_effects
            .apply_from_enemy_effect(1, EnemyEffect::Other(4));

        let mut per_slot: [Option<StatRecord>; 8] = Default::default();
        per_slot[1] = Some(StatRecord {
            base_attack: 50,
            base_evasion: 25,
            ..Default::default()
        });

        let round = BattleRound::begin(
            &mut world,
            &per_slot,
            &EquipmentTable::new(),
            &StatusModifiers::default(),
        );

        assert!(round.action_blocked[1]);
        // Evasion zeroed by the immobilising status.
        assert_eq!(round.stats[1].eva, 0);
    }

    #[test]
    fn begin_round_marks_magic_blocked_for_silenced_actor() {
        let mut world = world_with_party();
        world
            .status_effects
            .apply_from_enemy_effect(2, EnemyEffect::Other(6));

        let mut per_slot: [Option<StatRecord>; 8] = Default::default();
        per_slot[2] = Some(StatRecord::default());

        let round = BattleRound::begin(
            &mut world,
            &per_slot,
            &EquipmentTable::new(),
            &StatusModifiers::default(),
        );
        assert!(round.magic_blocked[2]);
    }

    #[test]
    fn end_round_ticks_status_and_returns_zero_when_no_deaths() {
        let mut world = world_with_party();
        // Slot 0 with full HP - nothing dies this round.
        if let Some(a) = world.actors.get_mut(0) {
            a.battle.hp = 100;
            a.battle.max_hp = 100;
        }
        let deaths = BattleRound::end(&mut world);
        assert_eq!(deaths, 0);
    }

    #[test]
    fn end_round_tick_damage_never_kills() {
        // The retail DoT ticker (FUN_801E752C) clamps each tick to
        // `current_hp - 1`, so Toxic / Venom drain to 1 HP but never kill -
        // a low-HP poisoned actor survives the round at 1 HP and `end`
        // reports no deaths.
        let mut world = world_with_party();
        if let Some(a) = world.actors.get_mut(0) {
            a.battle.hp = 5;
            a.battle.max_hp = 100;
        }
        // Toxic raw tick = max_hp/16 = 6 >= the 5 HP left → clamped to 4.
        world
            .status_effects
            .apply_from_enemy_effect(0, EnemyEffect::Toxic);
        let deaths = BattleRound::end(&mut world);
        assert_eq!(deaths, 0);
        assert_eq!(world.actors[0].battle.hp, 1);
    }

    #[test]
    fn round_skips_slots_with_no_record() {
        let mut world = world_with_party();
        let blanks: [Option<StatRecord>; 8] = Default::default();
        let round = BattleRound::begin(
            &mut world,
            &blanks,
            &EquipmentTable::new(),
            &StatusModifiers::default(),
        );
        // Slots without records have zero-default stats (no ATK).
        for s in round.stats.iter() {
            assert_eq!(s.atk, 0);
        }
    }

    #[test]
    fn round_blocked_arrays_default_false_when_no_status() {
        let mut world = world_with_party();
        let mut per_slot: [Option<StatRecord>; 8] = Default::default();
        per_slot[3] = Some(StatRecord {
            base_attack: 30,
            ..Default::default()
        });
        let round = BattleRound::begin(
            &mut world,
            &per_slot,
            &EquipmentTable::new(),
            &StatusModifiers::default(),
        );
        assert!(!round.action_blocked[3]);
        assert!(!round.magic_blocked[3]);
    }
}
