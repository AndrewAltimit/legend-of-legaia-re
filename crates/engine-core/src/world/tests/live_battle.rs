use super::*;

fn live_battle_world_3v2() -> World {
    let mut world = World::new();
    world.party_count = 3;
    world.battle_player_driven = true;
    world.live_gameplay_loop = true;
    world.mode = SceneMode::Battle;
    for i in 0..5 {
        world.actors[i].active = true;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    world
}

#[test]
fn spirit_command_charges_ap_and_raises_the_guard_stance() {
    use crate::battle_input::{BattleCommandSession, CommandPhase};
    let mut world = live_battle_world_3v2();
    world.battle_command = Some(BattleCommandSession {
        actor: 0,
        party_slot: 0,
        no_escape: false,
        phase: CommandPhase::SpiritGuard,
    });
    world.tick_battle_command();
    assert!(world.ap_gauges[0].spirit_charged, "+5 AP spirit charge");
    assert!(world.battle_guarding[0], "guard stance raised");
    // Spirit consumes the turn, and the SpiritGuard arm claims the cycle in
    // the same tick (`World::cycle_battle_turn`): a parked EndOfAction would
    // be re-seeded with the actor's stale action bytes by the SM's 0x5A
    // self-advance next tick - the free-bonus-attack defect. So the
    // observable is the turn moving on (here: the next party member's own
    // command session, already open), not the intermediate park.
    assert_ne!(
        world.battle_ctx.active_actor, 0,
        "spirit consumes the turn and the cycle claims the next combatant"
    );
    if let Some(next) = world.battle_command.as_ref() {
        assert_ne!(next.actor, 0, "slot 0's session did not linger");
    }
}

#[test]
fn guard_stance_reduces_basic_attack_damage() {
    // Monster slot 3 strikes party slot 0. The melee kernel models the Spirit
    // stance as a **tripled guard roll** (`FUN_801EC3E4`, the defender's
    // `+0x1DE == 4` arm), not as the summon/arts finisher's damage halve - so
    // the assertion is a strict reduction, not an exact `>> 1`.
    let strike = |guarding: bool| -> u16 {
        let mut world = live_battle_world_3v2();
        world.rng_state = 0x1234_5678;
        world.battle_attack[3] = 200;
        world.battle_defense[0] = 40;
        world.actors[0].battle.max_hp = 9999;
        world.actors[0].battle.hp = 9999;
        world.actors[3].battle.active_target = 0;
        world.battle_ctx.active_actor = 3;
        world.battle_guarding[0] = guarding;
        world.apply_basic_attack();
        9999 - world.actors[0].battle.hp
    };
    let unguarded = strike(false);
    let guarded = strike(true);
    assert!(unguarded > 1, "the strike lands for real damage");
    assert!(
        guarded < unguarded,
        "the guard stance must reduce the hit: {guarded} !< {unguarded}"
    );
}

#[test]
fn enemy_agl_budget_drives_multi_strike_and_gates_on_agl() {
    use crate::monster_catalog::{MonsterCatalog, MonsterDef};

    // A monster whose per-round AGL gauge (60) affords three 20-cost swings.
    let mut cat = MonsterCatalog::new();
    let mut def = MonsterDef::new(42, "Multi", 500, 30);
    def.agl = 60;
    def.action_costs = vec![20];
    cat.insert(def);

    let mut world = live_battle_world_3v2();
    world.monster_catalog = cat;
    world.actors[3].battle_monster_id = Some(42);
    world.battle_attack[3] = 50;
    world.battle_defense[0] = 10;
    world.actors[3].battle.active_target = 0;
    world.actors[0].battle.max_hp = 9999;
    world.actors[0].battle.hp = 9999;

    // The picker's budget loop lands 3 swings (60 / 20).
    world.arm_monster_strike_budget(3);
    assert_eq!(
        world.monster_strike_budget, 3,
        "AGL gauge 60 / swing cost 20 = 3 swings"
    );
    world.battle_ctx.active_actor = 3;
    world.apply_basic_attack();
    // Each swing rolls its own damage (the melee roll pair), so count the
    // strikes rather than compare a sum against a closed-form single hit.
    assert_eq!(
        world.drain_battle_hit_fx().len(),
        3,
        "a monster with an AGL budget of 3 lands three swings in one turn"
    );
    assert!(9999 - world.actors[0].battle.hp > 0, "the swings landed");

    // A monster with no AGL / swing data falls back to a single swing (the
    // disc-free / synthetic catalog case) - one strike, RNG-free budget.
    let mut world = live_battle_world_3v2();
    world.actors[3].battle_monster_id = Some(1); // no catalog entry installed
    world.battle_attack[3] = 50;
    world.battle_defense[0] = 10;
    world.actors[3].battle.active_target = 0;
    world.arm_monster_strike_budget(3);
    assert_eq!(
        world.monster_strike_budget, 1,
        "no AGL data -> single swing"
    );
    world.battle_ctx.active_actor = 3;
    world.apply_basic_attack();
    assert_eq!(
        world.drain_battle_hit_fx().len(),
        1,
        "unbudgeted monster lands exactly one swing"
    );
}

#[test]
fn run_command_arms_the_run_band() {
    use crate::battle_input::{BattleCommandSession, CommandPhase};
    let mut world = live_battle_world_3v2();
    world.battle_command = Some(BattleCommandSession {
        actor: 0,
        party_slot: 0,
        no_escape: false,
        phase: CommandPhase::RunAway,
    });
    world.tick_battle_command();
    assert!(world.battle_command.is_none(), "session resolved");
    assert_eq!(world.actors[0].battle.action_category, 5, "Run category");
    assert_eq!(world.battle_ctx.queued_action, 5);
    assert_eq!(
        world.battle_ctx.action_state,
        legaia_engine_vm::battle_action::ActionState::Begin.as_byte()
    );
    assert!(world.battle_ctx.multi_cast_gate <= 1, "roll outcome staged");
}

#[test]
fn successful_run_escapes_the_battle_without_loot() {
    use legaia_engine_vm::battle_action::ActionState;
    let mut world = live_battle_world_3v2();
    // A downed member (slot 1) is floored at 1 HP by the successful escape.
    world.actors[1].battle.hp = 0;
    world.actors[1].battle.liveness = 0;
    // Arm the run band directly with a forced successful roll.
    world.actors[0].battle.action_category = 5;
    world.battle_ctx.active_actor = 0;
    world.battle_ctx.queued_action = 5;
    world.battle_ctx.multi_cast_gate = 1;
    world.battle_ctx.action_state = ActionState::Begin.as_byte();
    // Drive the live loop through Begin -> RunBegin -> RunWait (0x3C-frame
    // timer) -> RunEscape (battle_end Escaped -> finish_battle).
    let mut completed = false;
    for _ in 0..0x100 {
        if matches!(
            world.live_battle_tick(),
            Some(legaia_engine_vm::battle_action::StepOutcome::BattleComplete)
        ) {
            completed = true;
            break;
        }
    }
    assert!(completed, "the run band tears the battle down");
    assert!(
        world.actors[1].battle.liveness != 0,
        "escape floors a downed member's liveness at 1"
    );
    assert!(
        world.last_battle_rewards.is_none(),
        "an escape grants no loot"
    );
    assert!(!world.game_over, "an escape is not a wipe");
}

#[test]
fn round_boundary_state_is_not_a_spurious_victory() {
    use legaia_engine_vm::battle_action::{ActionState, StepOutcome};
    let mut world = live_battle_world_3v2();
    // The SM reaches state 0xFF through the 0x5A gate's non-wipe arm - e.g.
    // the tick after a folded monster cast leaves the SM parked at
    // EndOfAction, whose bump pushes the acted counter past alive_total.
    // Park the SM there directly: with both sides alive this is a ROUND
    // boundary (retail semantics), never a battle end. Pre-fix this tick
    // ran battle_end(MonsterWipe) -> finish_battle - a spurious victory
    // with loot granted after one round.
    world.battle_ctx.action_state = ActionState::RoundEnd.as_byte();
    for _ in 0..0x40 {
        let out = world.live_battle_tick();
        assert!(
            !matches!(out, Some(StepOutcome::BattleComplete)),
            "both sides alive: the round boundary must not complete the battle"
        );
        if world.battle_command.is_some() {
            break; // the loop armed the next turn - the battle continues
        }
    }
    assert!(world.battle_end.is_none(), "no battle-end cause staged");
    assert!(world.last_battle_rewards.is_none(), "no spurious loot");
    assert_eq!(world.mode, SceneMode::Battle, "still in battle");
}

#[test]
fn failed_run_consumes_the_turn_and_the_battle_continues() {
    use legaia_engine_vm::battle_action::ActionState;
    let mut world = live_battle_world_3v2();
    world.actors[0].battle.action_category = 5;
    world.battle_ctx.active_actor = 0;
    world.battle_ctx.queued_action = 5;
    world.battle_ctx.multi_cast_gate = 0; // roll failed
    world.battle_ctx.action_state = ActionState::Begin.as_byte();
    for _ in 0..0x100 {
        if matches!(
            world.live_battle_tick(),
            Some(legaia_engine_vm::battle_action::StepOutcome::BattleComplete)
        ) {
            panic!("a failed run must not end the battle");
        }
        if world.battle_command.is_some() {
            break; // the loop cycled to the next party turn - battle continues
        }
    }
    assert!(world.battle_end.is_none(), "no battle-end cause staged");
}

#[test]
fn shop_buy_fills_the_stack_at_99_and_refuses_past_it() {
    // Retail law min(gold/price, 99, 99-held): the buy-list row dims once
    // held stops being < 0x63 (sltiu at 0x80030f0c, FUN_80030628 shop
    // case) and the quantity max clamps to 99 - held (li a0,0x63 at
    // 0x801db8d0, FUN_801DB7F4) - so a stack tops off at exactly 99.
    let mut world = World::new();
    world.money = 1_000_000;
    let inv = crate::shop::ShopInventory::new(
        0,
        vec![crate::shop::ShopItem {
            item_id: 0x77,
            price: 10,
        }],
    );
    let mut session = crate::shop::ShopSession::new(inv);
    session.select_buy_item(0);

    // 95 held + 4 more = 99: allowed, exactly at the cap.
    world.inventory.insert(0x77, 95);
    session.set_quantity(3); // qty 4
    let (_, qty, _) = world.buy_from_shop(&session).expect("cap-exact buy lands");
    assert_eq!(qty, 4);
    assert_eq!(world.inventory.get(&0x77), Some(&99));

    // 99 held: one more refuses, inventory and gold untouched.
    let money = world.money;
    session.set_quantity(0); // qty 1
    assert!(world.buy_from_shop(&session).is_none());
    assert_eq!(world.inventory.get(&0x77), Some(&99));
    assert_eq!(world.money, money);

    // The retail quantity kernel agrees: at 95 held the picker maxes at 4.
    assert_eq!(crate::shop::buy_qty_max(1_000_000, 10, Some(95)), 4);
    assert_eq!(crate::shop::buy_qty_max(1_000_000, 10, Some(99)), 0);
}

#[test]
fn encounter_rate_modifiers_resolve_from_passives_and_flags() {
    // FUN_801D9E1C's four pre-roll tests: High/Low Encounter ability bits
    // (0x3B/0x3C) + system flags 0x1D/0x1E, statically pinned shifts.
    let mut world = World::new();
    assert!(world.encounter_rate_modifiers().is_neutral());

    // Ability bit 0x3B (High Encounter - Bad Luck Bell / Nemesis Gem).
    world.party_ability_mask[(0x3B >> 5) as usize] |= 1 << (0x3B & 0x1F);
    // System flag 0x1E (rate down).
    world.system_flag_set(0x1E);
    let m = world.encounter_rate_modifiers();
    assert!(m.high_encounter && !m.low_encounter && !m.flag_high && m.flag_low);

    // The shifts compose in retail order: (rate << 2) >> 1.
    assert_eq!(m.apply(8), 16);
}

#[test]
fn npc_walk_steps_track_heading_and_keep_it_after_arrival() {
    // Walkers record their travel heading (12-bit, 0 = Z+, the player's
    // render_26 convention) and keep facing that way once the leg ends.
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.field_npc_positions.insert(1, (1000, 1000));
    assert!(!world.field_npc_headings.contains_key(&1));

    // Walk X+ : heading = quarter turn (0x400). NPC glide steps only on the
    // retail-frame ticks (~60 of every 100), so budget extra sim ticks.
    assert!(world.start_field_npc_motion(1, 1200, 1000));
    for _ in 0..70 {
        let _ = world.tick();
    }
    assert_eq!(world.field_npc_positions.get(&1), Some(&(1200, 1000)));
    assert_eq!(world.field_npc_headings.get(&1), Some(&0x400));
    assert!(world.field_npc_motions.is_empty(), "leg ended");

    // Facing persists while standing.
    for _ in 0..5 {
        let _ = world.tick();
    }
    assert_eq!(world.field_npc_headings.get(&1), Some(&0x400));

    // Walk Z- : heading = half turn (0x800).
    assert!(world.start_field_npc_motion(1, 1200, 800));
    for _ in 0..70 {
        let _ = world.tick();
    }
    assert_eq!(world.field_npc_headings.get(&1), Some(&0x800));
}
