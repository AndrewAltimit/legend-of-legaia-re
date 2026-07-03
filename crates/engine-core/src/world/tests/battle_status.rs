use super::*;

#[test]
fn enter_battle_populates_party_and_monsters() {
    let mut world = World::default();
    world.enter_battle(3, 5);
    assert_eq!(world.mode, SceneMode::Battle);
    assert_eq!(world.party_count, 3);
    // 3 party + 5 monsters = 8 active.
    let active_count = world.actors.iter().filter(|a| a.active).count();
    assert_eq!(active_count, 8);
    // Party slots sit at the retail 3-member seats (negative Z, facing
    // the monsters at positive Z).
    for i in 0..3 {
        let s = crate::battle_seats::party_seat(3, i);
        assert_eq!(world.actors[i].move_state.world_x, s.x);
        assert_eq!(world.actors[i].move_state.world_z, s.z);
        assert!(world.actors[i].move_state.world_z < 0);
        assert_eq!(world.actors[i].battle.liveness, 1);
    }
    // Monster slots at the retail seats on the positive-Z side.
    for i in 3..8 {
        assert!(world.actors[i].move_state.world_z > 0);
        assert_eq!(world.actors[i].battle.liveness, 1);
    }
    // SM seeded at Begin.
    assert_eq!(
        world.battle_ctx.action_state,
        vm::battle_action::ActionState::Begin.as_byte()
    );
}

#[test]
fn enter_battle_caps_party_at_three() {
    let mut world = World::default();
    // Even if asked for more party than the cap, we clamp to 3.
    world.enter_battle(8, 0);
    assert_eq!(world.party_count, 3);
}

#[test]
fn status_block_helpers_classify_by_kind() {
    use legaia_engine_vm::status_effects::StatusKind;
    let mut world = World::new();
    // Sleep blocks all actions but not magic specifically.
    world
        .status_effects
        .apply_with_duration(1, StatusKind::Sleep, 5);
    assert!(world.actor_blocked_from_acting(1));
    assert!(!world.actor_blocked_from_magic(1));
    // Numb is a full paralysis - blocks the whole turn (so magic is moot too).
    world
        .status_effects
        .apply_with_duration(4, StatusKind::Numb, 5);
    assert!(world.actor_blocked_from_acting(4));
    // Silence blocks magic only.
    world
        .status_effects
        .apply_with_duration(2, StatusKind::Curse, 5);
    assert!(!world.actor_blocked_from_acting(2));
    assert!(world.actor_blocked_from_magic(2));
    // Petrify blocks both.
    world
        .status_effects
        .apply_with_duration(3, StatusKind::Faint, 5);
    assert!(world.actor_blocked_from_acting(3));
    assert!(world.actor_blocked_from_magic(3));
    // A clean actor is blocked from nothing.
    assert!(!world.actor_blocked_from_acting(0));
    assert!(!world.actor_blocked_from_magic(0));
}

#[test]
fn confuse_retargets_a_monster_strike_to_its_own_band() {
    use legaia_engine_vm::status_effects::StatusKind;
    let mut world = World::new();
    world.party_count = 3;
    // Party slots 0..2 + monster slots 3,4 alive; everything else stays dead.
    for i in 0..5 {
        world.actors[i].active = true;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    // Monster slot 3 picked a party member (slot 1) as its target.
    world.actors[3].battle.active_target = 1;

    // Not confused: the picked target stands.
    world.maybe_confuse_retarget(3);
    assert_eq!(world.actors[3].battle.active_target, 1);

    // Confused: the strike flips to a living member of its own (monster) band.
    world
        .status_effects
        .apply_with_duration(3, StatusKind::Confuse, 3);
    world.maybe_confuse_retarget(3);
    let t = world.actors[3].battle.active_target;
    assert!(
        t >= 3,
        "confused monster targets its own band, got slot {t}"
    );
    assert!(
        world.actors[t as usize].battle.liveness != 0,
        "the retarget lands on a living actor"
    );
}

#[test]
fn confuse_retargets_a_party_strike_to_a_living_ally() {
    use legaia_engine_vm::status_effects::StatusKind;
    let mut world = World::new();
    world.party_count = 3;
    for i in 0..5 {
        world.actors[i].active = true;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    // A confused party member (slot 1) whose action targeted a monster (slot 3)
    // flips to a random living member of its own (party) side.
    world.actors[1].battle.active_target = 3;
    world
        .status_effects
        .apply_with_duration(1, StatusKind::Confuse, 3);
    world.maybe_confuse_retarget(1);
    let t = world.actors[1].battle.active_target;
    assert!(t < 3, "confused party member targets an ally, got slot {t}");
    assert!(world.actors[t as usize].battle.liveness != 0);
}

#[test]
fn confused_party_member_auto_acts_instead_of_opening_the_command_menu() {
    use legaia_engine_vm::status_effects::StatusKind;
    let mut world = World::new();
    world.party_count = 3;
    world.battle_player_driven = true;
    for i in 0..5 {
        world.actors[i].active = true;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    world
        .status_effects
        .apply_with_duration(0, StatusKind::Confuse, 3);
    // The confused party member auto-arms a physical strike (no command session)
    // aimed at a living ally.
    world.arm_party_physical(0);
    assert!(
        world.battle_command.is_none(),
        "a confused party member never opens the command menu"
    );
    assert_eq!(world.actors[0].battle.action_category, 3, "physical armed");
    let t = world.actors[0].battle.active_target;
    assert!(t < 3, "retargeted onto an ally, got slot {t}");
}

#[test]
fn confuse_retargets_a_monster_cast_to_the_opposite_side() {
    use legaia_engine_vm::status_effects::StatusKind;
    let mut world = World::new();
    world.party_count = 3;
    for i in 0..5 {
        world.actors[i].active = true;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    world
        .status_effects
        .apply_with_duration(3, StatusKind::Confuse, 3);

    // Non-confused caster (slot 4): targets untouched.
    let mut t = vec![0u8, 1, 2];
    world.confuse_retarget_cast(4, &mut t);
    assert_eq!(t, vec![0, 1, 2]);

    // Confused single-target cast at a party member flips to one living monster.
    let mut t1 = vec![1u8];
    world.confuse_retarget_cast(3, &mut t1);
    assert_eq!(t1.len(), 1);
    assert!(t1[0] >= 3, "single cast flips to a monster, got {}", t1[0]);

    // Confused area cast at the whole party flips to every living monster.
    let mut t2 = vec![0u8, 1, 2];
    world.confuse_retarget_cast(3, &mut t2);
    assert_eq!(t2, vec![3, 4]);

    // A self-only cast is left alone.
    let mut t3 = vec![3u8];
    world.confuse_retarget_cast(3, &mut t3);
    assert_eq!(t3, vec![3]);
}

#[test]
fn stone_counts_as_defeated_and_is_petrified() {
    use legaia_engine_vm::status_effects::StatusKind;
    let mut world = World::new();
    // A petrified actor stays "alive" (liveness != 0) but counts as defeated
    // for wipe detection, and reads as petrified.
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle.hp = 100;
    world
        .status_effects
        .apply_with_duration(1, StatusKind::Stone, 255);
    assert!(world.actor_is_petrified(1));
    assert!(world.actor_blocked_from_acting(1), "Stone blocks the turn");
    assert!(
        world.actor_effectively_defeated(1),
        "Stone counts as defeated for wipe detection"
    );
    // A clean living actor is neither.
    world.actors[0].battle.liveness = 1;
    assert!(!world.actor_is_petrified(0));
    assert!(!world.actor_effectively_defeated(0));
}

#[test]
fn petrified_target_absorbs_art_strike_damage() {
    use crate::art_strike::ArtStrikeOutcome;
    use legaia_engine_vm::status_effects::StatusKind;
    let mut world = World::new();
    world.party_count = 4;
    for slot in 0..4 {
        world.actors[slot].active = true;
        world.actors[slot].battle.hp = 200;
        world.actors[slot].battle.max_hp = 200;
        world.actors[slot].battle.liveness = 1;
    }
    world
        .status_effects
        .apply_with_duration(3, StatusKind::Stone, 255);
    let event = BattleEvent::ApplyArtStrike {
        actor_slot: 0,
        target_slot: 3,
        strike_index: 0,
        outcome: ArtStrikeOutcome {
            damage: Some(150),
            enemy_effect: legaia_art::record::EnemyEffect::None,
            cues: vec![],
            alt_range: false,
            power_target: None,
        },
    };
    let r = world.fold_battle_event(&event);
    assert_eq!(world.actors[3].battle.hp, 200, "Stone absorbs the strike");
    assert_eq!(r, Some((3, 200)));
}

/// Stone is invulnerable at the spell damage path too, not just the basic-attack
/// / SM strike. A petrified party member is still targetable (target resolvers
/// key on `liveness`, which Stone leaves non-zero), so an enemy damage spell can
/// land on it - and it must absorb. The defender's spirit gauge still charges
/// from the pre-nullify amount (matching the basic-attack / finisher order).
#[test]
fn stone_absorbs_a_damage_spell() {
    use crate::spells::{SpellElement, SpellOutcome};
    use legaia_engine_vm::status_effects::StatusKind;

    let mut world = World::new();
    world.enter_battle(3, 1);
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.liveness = 1;
    world
        .status_effects
        .apply_with_duration(0, StatusKind::Stone, 255);

    world.fold_spell_outcome(SpellOutcome::Damage {
        target: 0,
        amount: 150,
        element: SpellElement::Neutral,
        weakness: false,
    });

    assert_eq!(
        world.actors[0].battle.hp, 200,
        "a petrified target absorbs the damage spell"
    );
    assert_ne!(
        world.actors[0].battle.liveness, 0,
        "absorbing the cast must not down the petrified actor"
    );
    assert!(
        world.spirit_gauge(0) > 0,
        "the pre-nullify hit still charges the defender's spirit gauge"
    );
}

#[test]
fn asleep_monster_loses_its_turn_and_never_attacks() {
    use legaia_engine_vm::status_effects::StatusKind;

    // Drive a 1-vs-1 auto-resolving battle for many ticks and report whether
    // the party member took any damage. With unseeded battle stats the monster
    // auto-hits for >= 1 each turn it acts, so the only way the party stays at
    // full HP is if the monster never gets to act.
    fn party_took_damage(asleep: bool) -> bool {
        let mut world = World::new();
        world.enter_battle(1, 1); // slot 0 = party, slot 1 = monster
        world.live_gameplay_loop = true; // route tick() through live_battle_tick
        world.battle_player_driven = false; // both sides auto-act
        // Big monster HP so it survives long enough to take many turns; the
        // party HP is what we watch.
        world.actors[1].battle.hp = 9999;
        world.actors[1].battle.max_hp = 9999;
        world.actors[0].battle.hp = 500;
        world.actors[0].battle.max_hp = 500;
        if asleep {
            world
                .status_effects
                .apply_with_duration(1, StatusKind::Sleep, 255);
        }
        let start = world.actors[0].battle.hp;
        for _ in 0..600 {
            world.tick();
            if world.mode != SceneMode::Battle {
                break;
            }
        }
        world.actors[0].battle.hp < start
    }

    // Non-vacuous control: an awake monster auto-hits the party.
    assert!(
        party_took_damage(false),
        "control: an awake monster must damage the party"
    );
    // The fix: an asleep monster loses its turn, so the party is untouched.
    assert!(
        !party_took_damage(true),
        "an asleep monster must skip its turn and never attack"
    );
}

/// The retail DoT ticker (FUN_801E752C) never kills: each tick is clamped to
/// `current_hp - 1`, so a poisoned actor bottoms out at 1 HP and stays alive
/// (`liveness` untouched). The `hp == 0 → liveness = 0` pairing in
/// `tick_status_effects` remains as a safety net for other damage entry
/// points - this pins the never-kill clamp end to end.
#[test]
fn dot_never_kills_actor_bottoms_out_at_one_hp() {
    use legaia_engine_vm::status_effects::StatusKind;

    let mut world = World::new();
    world.enter_battle(1, 1);
    // Toxic raw tick = max_hp/16 = 5, more than the monster's remaining 4 HP
    // → clamped to current_hp - 1 = 3.
    world.actors[1].battle.max_hp = 80;
    world.actors[1].battle.hp = 4;
    world.actors[1].battle.liveness = 1;
    world.status_effects.apply(1, StatusKind::Toxic);

    world.tick_status_effects();

    assert_eq!(
        world.actors[1].battle.hp, 1,
        "Toxic DoT clamps to current_hp - 1 (never lethal)"
    );
    assert_eq!(
        world.actors[1].battle.liveness, 1,
        "a DoT tick never downs the actor"
    );
}

/// In the live battle loop a poison/toxic affliction must actually drain HP and
/// expire: `tick_status_effects` is called once per round at the initiative
/// boundary. A poisoned party member loses HP across rounds even when the enemy
/// can never strike (asleep), so the only HP source is the DoT.
#[test]
fn live_loop_ticks_dot_at_the_round_boundary() {
    use legaia_engine_vm::status_effects::StatusKind;

    fn party_lost_hp(poisoned: bool) -> bool {
        let mut world = World::new();
        world.enter_battle(1, 1); // slot 0 = party, slot 1 = monster
        world.live_gameplay_loop = true;
        world.battle_player_driven = false;
        // Both sides carry SPD so the initiative round boundary engages (the DoT
        // tick is gated on it); seed up front so battle start isn't mistaken for
        // a round boundary.
        world.battle_speed[0] = 10;
        world.battle_speed[1] = 10;
        world.seed_battle_initiative();
        // The monster is asleep, so it never attacks - the party's only HP loss
        // can come from the DoT.
        world
            .status_effects
            .apply_with_duration(1, StatusKind::Sleep, 255);
        world.actors[0].battle.max_hp = 800;
        world.actors[0].battle.hp = 800;
        world.actors[1].battle.max_hp = 9999;
        world.actors[1].battle.hp = 9999;
        if poisoned {
            world
                .status_effects
                .apply_with_duration(0, StatusKind::Toxic, 255);
        }
        let start = world.actors[0].battle.hp;
        for _ in 0..600 {
            world.tick();
            if world.mode != SceneMode::Battle {
                break;
            }
        }
        world.actors[0].battle.hp < start
    }

    // Control: with no poison and an asleep enemy the party is never touched.
    assert!(
        !party_lost_hp(false),
        "control: no DoT + asleep enemy must leave the party at full HP"
    );
    // The fix: the live loop ticks the DoT each round, so the party bleeds.
    assert!(
        party_lost_hp(true),
        "a poisoned party member must lose HP to the DoT in the live loop"
    );
}

#[test]
fn all_party_item_heals_every_living_party_actor_in_battle() {
    use crate::inventory_use::{
        InventoryContext, InventoryUseInput, InventoryUseSession, TargetRow,
    };

    let mut world = World::default();
    world.set_item_catalog(crate::items::ItemCatalog::vanilla());
    world.enter_battle(3, 1);
    // Wound the whole party; down the third member.
    for i in 0..3 {
        world.actors[i].battle.max_hp = 500;
        world.actors[i].battle.hp = 100;
    }
    world.actors[2].battle.hp = 0; // dead - excluded from a party heal

    // Healing Bloom (0x7A): all-party HP heal of 200.
    let targets: Vec<TargetRow> = (0..3)
        .map(|i| {
            let a = &world.actors[i];
            let mut r = TargetRow::new(i as u8, "P").with_stats(a.battle.hp, a.battle.max_hp, 0, 0);
            r.alive = a.battle.liveness != 0 && a.battle.hp > 0;
            r
        })
        .collect();
    let mut s = InventoryUseSession::new(
        world.item_catalog.clone(),
        vec![0x7A],
        targets,
        InventoryContext::Battle,
    );
    // One Confirm fans the item out across the living party (no target select).
    s.input(InventoryUseInput::Confirm);
    assert!(matches!(
        s.state,
        crate::inventory_use::InventoryUseState::Done(_)
    ));
    assert_eq!(s.used_item, Some(0x7A));
    assert_eq!(s.used_slots, vec![0, 1], "only the two living allies");

    // Apply exactly as the field / battle consumers do: one use_item per slot.
    for &slot in &s.used_slots {
        world.use_item(0x7A, slot);
    }
    assert_eq!(world.actors[0].battle.hp, 300, "Vahn +200");
    assert_eq!(world.actors[1].battle.hp, 300, "Noa +200");
    assert_eq!(
        world.actors[2].battle.hp, 0,
        "dead ally untouched by a heal"
    );
}
