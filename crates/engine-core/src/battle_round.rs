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
use legaia_engine_vm as vm;
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

    /// The round-boundary actor sweep - the port of `FUN_801D88CC`, the first
    /// of the two passes retail's battle-flow SM runs when a round ends.
    ///
    /// The call site is `FUN_801D0748` at `801d0ec4`, and the order there is
    /// what pins this routine to the boundary:
    ///
    /// ```text
    /// 801d0ecc  sw   v0,0x880(v1)     ; ctx+0x880 = 0x8000
    /// 801d0ed0  jal  0x801d88cc       ; this pass
    /// 801d0ed8  jal  0x801da780       ; then reseed the initiative keys
    /// 801d0ee4  jal  0x801d388c
    /// 801d0f04  jal  0x801e752c       ; then the DoT / status tick
    /// ```
    ///
    /// Note the order: the actor sweep runs **before** the initiative seeder,
    /// not after it.
    ///
    /// `FUN_801D88CC` is two loops over the actor pointer table at
    /// `DAT_801C9370`, and they cover different bands:
    ///
    /// - **Loop A** (`801d892c..801d89f4`) walks all 7 slots. Per actor it
    ///   restores the action gauge `+0x154` through the three arms of
    ///   [`vm::battle_formulas::round_reset_agility`], then zeroes the 16-byte
    ///   action-parameter stream at `+0x1DF`
    ///   ([`vm::battle_formulas::ACTION_STREAM_RANGE`]) - the clear that makes
    ///   last round's action id unreadable.
    /// - **Loop B** (`801d8a00..801d8a70`) walks the **party band only** (the
    ///   loop bound is `s1+0xc`, three pointers). It re-picks a stale target
    ///   ([`vm::battle_formulas::needs_retarget`]) and clears the action
    ///   category byte `+0x1DE`.
    ///
    /// The re-pick itself is `FUN_801DB8B4`, which is a plain first-living-
    /// monster scan (`801db8b4`: `v1 = 3`, walk `DAT_801C937C` while
    /// `v1 < 7`, return on the first slot whose `+0x14C` is non-zero, else
    /// return `7`). It draws no RNG.
    ///
    /// The engine **compacts** its battle seating - the first monster sits at
    /// `party_count`, not at a fixed slot 3 - so both bands are taken from
    /// `party_count` here rather than from retail's literal `3`. That is the
    /// same seating adapter `World::reseed_initiative` applies.
    ///
    /// The routine's ctx header writes (`+0x13 = 0xFF`, `+0x1B = 1`,
    /// `+0x1C = 0x10`, `+0x1F = 0xFF`, and the `FUN_801D32BC(0)` call) stay
    /// with the host: `+0x1B` / `+0x1C` / `+0x1F` are battle-flow fields the
    /// engine does not model, and `+0x13` is the active-actor cursor, which the
    /// engine's boundary hook sits *before* rather than after.
    ///
    /// PORT: FUN_801d88cc
    /// REF: FUN_801db8b4 (loop B's re-pick), FUN_801d0748 (the call site)
    pub fn boundary(world: &mut World) {
        let party_count = (world.party_count as usize).max(1);
        let slots = world.actors.len().min(8);

        // Loop A: every slot - gauge restore + action-stream clear.
        for slot in 0..slots {
            let a = &mut world.actors[slot].battle;
            // `+0x1DE == 4` or the `+0x1F9` charge byte non-zero.
            let spirit_charged = a.action_category == 4 || a.spirit_shield != 0;
            // `+0x1DE == 3`, or any monster slot.
            let plain_reset = a.action_category == 3 || slot >= party_count;
            a.agl = vm::battle_formulas::round_reset_agility(
                a.agl,
                a.agl_base,
                spirit_charged,
                plain_reset,
            );
            // `+0x1DF..+0x1EE`. `BattleActor::params` is based at `+0x1DF`, so
            // the retail range indexes it from zero.
            let len = vm::battle_formulas::ACTION_STREAM_RANGE.len();
            for b in a.params.iter_mut().take(len) {
                *b = 0;
            }
        }

        // Loop B: party band only - stale-target re-pick + category clear.
        for slot in 0..party_count.min(slots) {
            let target = world.actors[slot].battle.active_target;
            let target_hp = world
                .actors
                .get(target as usize)
                .map(|a| a.battle.hp)
                .unwrap_or(0);
            if vm::battle_formulas::needs_retarget(target, target_hp) {
                world.actors[slot].battle.active_target = Self::first_living_monster(world);
            }
            world.actors[slot].battle.action_category = 0;
        }
    }

    /// `FUN_801DB8B4`: the first living monster slot, or the one-past-the-end
    /// sentinel when the monster band is wiped (retail returns `7`, the slot
    /// count; the engine returns its own band end for the same reason).
    ///
    /// PORT: FUN_801db8b4
    fn first_living_monster(world: &World) -> u8 {
        let party_count = (world.party_count as usize).max(1);
        let end = world.actors.len().min(8);
        (party_count..end)
            .find(|&i| world.actors[i].battle.liveness != 0)
            .unwrap_or(end) as u8
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
        // Mark slot 1 as Sleep before begin. No on-disc byte maps to Sleep
        // since the 4/5 remap (4 = Toxic, 5 = Rot per the pinned appliers),
        // so apply the host-driven kind directly.
        world
            .status_effects
            .apply(1, legaia_engine_vm::status_effects::StatusKind::Sleep);

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

    /// Build the 1-vs-1 live-loop battle the round-boundary tests drive: both
    /// sides carry SPD (the boundary is gated on the initiative path), the
    /// monster is asleep so it never acts, and both sides carry enough HP to
    /// survive the tick budget.
    fn live_round_battle() -> World {
        use legaia_engine_vm::status_effects::StatusKind;
        let mut world = World::new();
        world.enter_battle(1, 1); // slot 0 = party, slot 1 = monster
        world.live_gameplay_loop = true;
        world.battle_player_driven = false;
        world.battle_speed[0] = 10;
        world.battle_speed[1] = 10;
        world
            .status_effects
            .apply_with_duration(1, StatusKind::Sleep, 255);
        world.actors[0].battle.max_hp = 800;
        world.actors[0].battle.hp = 800;
        world.actors[1].battle.max_hp = 9999;
        world.actors[1].battle.hp = 9999;
        world
    }

    /// `FUN_801D88CC` loop A zeroes each actor's 16-byte action-parameter
    /// stream (`+0x1DF..+0x1EE`) every round. `+0x1DF` is the action id the
    /// move-power table is indexed by, so a stale byte surviving the round is
    /// a stale action staying readable.
    ///
    /// This drives the **live loop**, not the pass directly: nothing else in
    /// the engine clears `params`, so the byte only goes to zero if
    /// `live_battle_tick` actually reaches `BattleRound::boundary` at its round
    /// boundary. Removing that call leaves the byte at `0x42` for the whole
    /// tick budget and this fails.
    #[test]
    fn live_loop_clears_the_action_stream_at_the_round_boundary() {
        use crate::world::SceneMode;
        let mut world = live_round_battle();
        world.actors[0].battle.params[0] = 0x42;

        let mut cleared = false;
        for _ in 0..600 {
            world.tick();
            if world.mode != SceneMode::Battle {
                break;
            }
            if world.actors[0].battle.params[0] == 0 {
                cleared = true;
                break;
            }
        }
        assert!(
            cleared,
            "the round boundary must zero the action-parameter stream"
        );
    }

    /// Loop A's gauge restore, end to end. A monster slot always takes the
    /// plain-reset arm, so a gauge drained during the round is back at its base
    /// after the boundary. Fails without the `live_battle_tick` wiring - the
    /// engine has no other writer that raises `agl`.
    #[test]
    fn live_loop_restores_the_action_gauge_at_the_round_boundary() {
        use crate::world::SceneMode;
        let mut world = live_round_battle();
        world.actors[1].battle.agl_base = 40;
        world.actors[1].battle.agl = 0;

        let mut restored = false;
        for _ in 0..600 {
            world.tick();
            if world.mode != SceneMode::Battle {
                break;
            }
            if world.actors[1].battle.agl == 40 {
                restored = true;
                break;
            }
        }
        assert!(
            restored,
            "the round boundary must restore a monster's AGL to its base"
        );
    }

    /// Loop B: a party actor whose stored target (`+0x1DD`) names a dead actor
    /// re-points at the first living monster slot, and its action category
    /// (`+0x1DE`) is cleared. `FUN_801DB8B4` picks the **lowest** living
    /// monster slot deterministically - it draws no RNG - so slot 1 is the
    /// only correct answer here.
    #[test]
    fn boundary_retargets_a_party_actor_off_a_dead_target() {
        let mut world = World::new();
        world.enter_battle(1, 3); // slot 0 party, slots 1..=3 monsters
        world.actors[0].battle.active_target = 3;
        world.actors[0].battle.action_category = 3;
        // Slot 3 is dead; slot 1 is the lowest living monster.
        world.actors[3].battle.hp = 0;
        world.actors[3].battle.liveness = 0;
        for slot in 1..=2 {
            world.actors[slot].battle.hp = 50;
            world.actors[slot].battle.liveness = 1;
        }

        BattleRound::boundary(&mut world);

        assert_eq!(world.actors[0].battle.active_target, 1);
        assert_eq!(world.actors[0].battle.action_category, 0);
    }

    /// A live target is left alone - the predicate is "still usable", not
    /// "re-pick every round". Guards the retarget test above from passing for
    /// the trivial reason that the boundary rewrites every target.
    #[test]
    fn boundary_leaves_a_living_target_in_place() {
        let mut world = World::new();
        world.enter_battle(1, 3);
        for slot in 1..=3 {
            world.actors[slot].battle.hp = 50;
            world.actors[slot].battle.liveness = 1;
        }
        world.actors[0].battle.active_target = 3;

        BattleRound::boundary(&mut world);

        assert_eq!(
            world.actors[0].battle.active_target, 3,
            "a living target must survive the boundary"
        );
    }

    /// Loop A's third arm: a **party** slot in an action state below `3` that
    /// is not spirit-charged gets *no* reset at all and carries its remaining
    /// gauge into the next round. A monster slot in the same state does reset,
    /// because the slot-band test is an `||` with the state test.
    #[test]
    fn boundary_gauge_arms_split_party_from_monster() {
        let mut world = World::new();
        world.enter_battle(1, 1);
        for slot in 0..2 {
            world.actors[slot].battle.agl_base = 60;
            world.actors[slot].battle.agl = 5;
            world.actors[slot].battle.action_category = 1;
        }

        BattleRound::boundary(&mut world);

        assert_eq!(
            world.actors[0].battle.agl, 5,
            "a party actor mid-combo carries its gauge"
        );
        assert_eq!(
            world.actors[1].battle.agl, 60,
            "a monster slot always resets to base"
        );
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
