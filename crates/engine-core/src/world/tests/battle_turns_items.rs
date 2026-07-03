use super::*;

#[test]
fn monsters_take_turns_and_can_wipe_the_party() {
    use legaia_engine_vm::battle_action::ActionState;
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.live_gameplay_loop = true;
    world.mode = SceneMode::Battle;
    // Lone party member: low HP, weak attack so the fight lasts several
    // rounds and the monster gets turns.
    world.actors[0].active = true;
    world.actors[0].battle.hp = 40;
    world.actors[0].battle.max_hp = 40;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 4);
    // Lone monster: tanky + hits hard enough to kill the party member.
    world.actors[1].battle.hp = 500;
    world.actors[1].battle.max_hp = 500;
    world.actors[1].battle.liveness = 1;
    world.set_battle_attack(1, 25);
    // Arm the first turn (party member swings at the monster).
    world.battle_ctx.active_actor = 0;
    world.battle_ctx.queued_action = 3;
    world.battle_ctx.action_state = ActionState::Begin.as_byte();
    world.actors[0].battle.active_target = 1;
    world.actors[0].battle.action_category = 3;

    let start_party_hp = world.actors[0].battle.hp;
    let mut party_took_damage = false;
    let mut ended = false;
    for _ in 0..4000 {
        world.tick();
        if world.actors[0].battle.hp < start_party_hp {
            party_took_damage = true;
        }
        // finish_battle flips back to Field (and raises game_over on a
        // party wipe).
        if world.mode == SceneMode::Field {
            ended = true;
            break;
        }
    }
    assert!(
        party_took_damage,
        "the monster must take turns and damage the party"
    );
    assert!(ended, "the battle must resolve (party wiped)");
    assert!(world.game_over, "a party wipe raises game_over");
}

#[test]
fn multi_monster_battle_all_monsters_act_and_party_can_win() {
    use legaia_engine_vm::battle_action::ActionState;
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.live_gameplay_loop = true;
    world.mode = SceneMode::Battle;
    // Lone party member: enough HP to survive three weak monsters, enough
    // attack to chip each down over a few rounds.
    world.actors[0].active = true;
    world.actors[0].battle.hp = 400;
    world.actors[0].battle.max_hp = 400;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 30);
    // Three monsters in slots 1..=3, each with modest HP + a light hit.
    for s in 1..=3 {
        world.actors[s].battle.hp = 40;
        world.actors[s].battle.max_hp = 40;
        world.actors[s].battle.liveness = 1;
        world.set_battle_attack(s as u8, 3);
    }
    // Arm the party member's first swing.
    world.battle_ctx.active_actor = 0;
    world.battle_ctx.queued_action = 3;
    world.battle_ctx.action_state = ActionState::Begin.as_byte();
    world.actors[0].battle.active_target = 1;
    world.actors[0].battle.action_category = 3;

    let start_hp = world.actors[0].battle.hp;
    let mut ended = false;
    for _ in 0..8000 {
        world.tick();
        if world.mode == SceneMode::Field {
            ended = true;
            break;
        }
    }
    assert!(ended, "the multi-monster battle must resolve");
    // Party wiped all three monsters (victory, not a party wipe).
    assert!(!world.game_over, "party should survive and win");
    for s in 1..=3 {
        assert_eq!(
            world.actors[s].battle.liveness, 0,
            "monster slot {s} should be defeated"
        );
    }
    // The monsters got turns: the party took at least some damage from
    // three light attackers over the fight.
    assert!(
        world.actors[0].battle.hp < start_hp,
        "monsters should have damaged the party over the multi-round fight"
    );
}

#[test]
fn battle_item_use_heals_ally_consumes_item_and_cycles_turn() {
    use crate::input::PadButton;
    use legaia_engine_vm::battle_action::ActionState;

    let mut world = World {
        party_count: 2,
        ..World::default()
    };
    world.battle_player_driven = true;
    world.mode = SceneMode::Battle;
    world.set_item_catalog(full_test_catalog());
    // Two party members (slot 0 wounded), one monster.
    for i in 0..2usize {
        world.actors[i].battle.max_hp = 200;
        world.actors[i].battle.hp = 200;
        world.actors[i].battle.liveness = 1;
        world.set_character_max_mp(i as u8, 30);
    }
    world.actors[0].battle.hp = 50;
    world.actors[2].battle.max_hp = 80;
    world.actors[2].battle.hp = 80;
    world.actors[2].battle.liveness = 1;
    // Healing Leaf (id 0x01) heals 100 HP; hold two.
    world.inventory.insert(0x01, 2);

    // Open the item submenu for the active party member.
    world.battle_ctx.active_actor = 0;
    world.battle_item_menu = Some(world.build_battle_item_session());
    {
        let m = world.battle_item_menu.as_ref().unwrap();
        assert_eq!(m.filtered_items.len(), 1, "one battle-usable item");
        assert_eq!(m.targets.len(), 2, "two party targets");
    }

    // Frame 1: Cross confirms the item -> target select.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu();
    assert!(world.battle_item_menu.is_some(), "still picking a target");

    // Frame 2: Cross confirms the first target (the wounded slot 0).
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu();

    assert_eq!(world.actors[0].battle.hp, 150, "healed 50 -> 150");
    assert_eq!(
        world.inventory.get(&0x01).copied(),
        Some(1),
        "one Healing Leaf consumed"
    );
    assert!(world.battle_item_menu.is_none(), "menu closed after use");
    assert_eq!(
        world.battle_ctx.action_state,
        ActionState::EndOfAction.as_byte(),
        "turn parked at EndOfAction so the loop cycles"
    );
    let fx = world.drain_battle_hit_fx();
    assert_eq!(fx.len(), 1);
    assert!(fx[0].is_heal);
    assert_eq!(fx[0].amount, 100);
    assert_eq!(fx[0].target_slot, 0);
}

#[test]
fn battle_item_menu_cancel_reopens_command_menu() {
    use crate::input::PadButton;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.battle_player_driven = true;
    world.mode = SceneMode::Battle;
    world.set_item_catalog(full_test_catalog());
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.liveness = 1;
    world.inventory.insert(0x01, 1);

    world.battle_ctx.active_actor = 0;
    world.battle_item_menu = Some(world.build_battle_item_session());

    // Circle from the item list backs all the way out.
    world.set_pad(0);
    world.set_pad(PadButton::Circle.mask());
    world.tick_battle_item_menu();

    assert!(world.battle_item_menu.is_none(), "item menu closed");
    assert!(
        world.battle_command.is_some(),
        "command menu reopened for the same actor"
    );
    assert_eq!(world.battle_command.as_ref().unwrap().actor, 0);
    // No item was consumed on a cancel.
    assert_eq!(world.inventory.get(&0x01).copied(), Some(1));
}
