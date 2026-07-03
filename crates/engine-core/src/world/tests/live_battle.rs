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
        phase: CommandPhase::SpiritGuard,
    });
    world.tick_battle_command();
    assert!(world.battle_command.is_none(), "session resolved");
    assert!(world.ap_gauges[0].spirit_charged, "+5 AP spirit charge");
    assert!(world.battle_guarding[0], "guard stance raised");
    assert_eq!(
        world.battle_ctx.action_state,
        legaia_engine_vm::battle_action::ActionState::EndOfAction.as_byte(),
        "spirit consumes the turn"
    );
}

#[test]
fn guard_stance_halves_basic_attack_damage() {
    let mut world = live_battle_world_3v2();
    // Monster slot 3 strikes party slot 0 (attack 50 vs defense 10).
    world.battle_attack[3] = 50;
    world.battle_defense[0] = 10;
    world.actors[3].battle.active_target = 0;
    world.battle_ctx.active_actor = 3;
    world.apply_basic_attack();
    let unguarded_dmg = 100 - world.actors[0].battle.hp;
    assert!(unguarded_dmg > 1, "the strike lands for real damage");

    // Same strike against a guarding defender: the guard-halve stage applies.
    let mut world = live_battle_world_3v2();
    world.battle_attack[3] = 50;
    world.battle_defense[0] = 10;
    world.actors[3].battle.active_target = 0;
    world.battle_ctx.active_actor = 3;
    world.battle_guarding[0] = true;
    world.apply_basic_attack();
    let guarded_dmg = 100 - world.actors[0].battle.hp;
    assert_eq!(
        guarded_dmg,
        unguarded_dmg >> 1,
        "guard halves the strike (finisher stage 3)"
    );
}

#[test]
fn run_command_arms_the_run_band() {
    use crate::battle_input::{BattleCommandSession, CommandPhase};
    let mut world = live_battle_world_3v2();
    world.battle_command = Some(BattleCommandSession {
        actor: 0,
        party_slot: 0,
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
fn shop_buy_refuses_past_the_98_held_cap() {
    // Retail dims buy attempts past 98 held of one item id (SHOP_HELD_CAP).
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

    // 94 held + 4 more = 98: allowed, exactly at the cap.
    world.inventory.insert(0x77, 94);
    session.set_quantity(3); // qty 4
    let (_, qty, _) = world.buy_from_shop(&session).expect("cap-exact buy lands");
    assert_eq!(qty, 4);
    assert_eq!(world.inventory.get(&0x77), Some(&98));

    // 98 held: one more refuses, inventory and gold untouched.
    let money = world.money;
    session.set_quantity(0); // qty 1
    assert!(world.buy_from_shop(&session).is_none());
    assert_eq!(world.inventory.get(&0x77), Some(&98));
    assert_eq!(world.money, money);
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

    // Walk X+ : heading = quarter turn (0x400).
    assert!(world.start_field_npc_motion(1, 1200, 1000));
    for _ in 0..40 {
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
    for _ in 0..40 {
        let _ = world.tick();
    }
    assert_eq!(world.field_npc_headings.get(&1), Some(&0x800));
}
