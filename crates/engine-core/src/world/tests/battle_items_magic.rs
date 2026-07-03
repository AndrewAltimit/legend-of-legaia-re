use super::*;

#[test]
fn battle_item_bomb_damages_enemy_and_cursor_lands_on_the_monster() {
    use crate::input::PadButton;
    use legaia_engine_vm::battle_action::ActionState;

    let mut world = offensive_item_world(500, 7);
    // Bomb (0x13) deals 200 HP to an enemy.
    world.inventory.insert(0x13, 1);
    world.battle_item_menu = Some(world.build_battle_item_session());
    {
        let m = world.battle_item_menu.as_ref().unwrap();
        assert_eq!(m.targets.len(), 2, "one ally + one enemy target");
        assert!(!m.targets[0].is_enemy, "ally row first");
        assert!(m.targets[1].is_enemy, "enemy row second");
    }

    // Frame 1: Cross confirms the Bomb -> target select. The cursor must
    // skip the ally and land on the enemy row (offensive item).
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu();
    {
        let m = world.battle_item_menu.as_ref().unwrap();
        match m.state {
            crate::inventory_use::InventoryUseState::TargetSelect { cursor, .. } => {
                assert_eq!(cursor, 1, "cursor positioned on the enemy row");
            }
            other => panic!("expected TargetSelect, got {other:?}"),
        }
    }

    // Frame 2: Cross confirms the enemy -> 200 damage.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu();

    assert_eq!(world.actors[1].battle.hp, 300, "500 -> 300 after Bomb");
    assert_eq!(world.inventory.get(&0x13).copied(), None, "Bomb consumed");
    assert!(world.battle_item_menu.is_none(), "menu closed after use");
    assert_eq!(
        world.battle_ctx.action_state,
        ActionState::EndOfAction.as_byte(),
        "turn parked at EndOfAction"
    );
    let fx = world.drain_battle_hit_fx();
    assert_eq!(fx.len(), 1);
    assert!(!fx[0].is_heal, "damage-coloured popup");
    assert_eq!(fx[0].amount, 200);
    assert_eq!(fx[0].target_slot, 1);
}

#[test]
fn battle_item_bomb_downs_a_low_hp_enemy() {
    use crate::input::PadButton;

    let mut world = offensive_item_world(120, 7);
    world.inventory.insert(0x13, 1); // Bomb, 200 dmg vs 120 HP.
    world.battle_item_menu = Some(world.build_battle_item_session());

    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu(); // confirm item -> target
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu(); // confirm enemy

    assert_eq!(world.actors[1].battle.hp, 0, "HP floored at zero");
    assert_eq!(world.actors[1].battle.liveness, 0, "monster downed");
}

#[test]
fn battle_item_capture_downs_a_weakened_enemy_and_logs_the_id() {
    use crate::input::PadButton;

    // Weakened monster (10/500 HP) so the missing-HP capture roll is
    // near-certain; pin the RNG so the roll (23) lands.
    let mut world = offensive_item_world(500, 42);
    world.actors[1].battle.hp = 10;
    world.rng_state = 0;
    world.inventory.insert(0x11, 1); // Genocide Crystal (capture).
    world.battle_item_menu = Some(world.build_battle_item_session());

    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu(); // item -> target (lands on enemy)
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu(); // confirm enemy

    assert_eq!(
        world.actors[1].battle.liveness, 0,
        "captured monster downed"
    );
    assert_eq!(
        world.drain_battle_captures(),
        vec![42],
        "monster id logged for post-battle Seru learning"
    );
}

#[test]
fn battle_item_escape_returns_to_field() {
    use crate::input::PadButton;

    let mut world = offensive_item_world(500, 7);
    world.inventory.insert(0x12, 1); // Goblin Foot (escape).
    world.battle_item_menu = Some(world.build_battle_item_session());

    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu(); // item -> target
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu(); // confirm

    assert_eq!(world.mode, SceneMode::Field, "escaped back to the field");
    assert!(!world.battle_escaped, "escape flag reset by finish_battle");
    assert!(world.battle_item_menu.is_none(), "battle menus cleared");
    assert_eq!(world.inventory.get(&0x12).copied(), None, "item consumed");
}

#[test]
fn battle_magic_cast_damages_monster_spends_mp_and_cycles_turn() {
    use crate::input::PadButton;
    use crate::spells::SpellCatalog;
    use legaia_engine_vm::battle_action::ActionState;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.battle_player_driven = true;
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    // Caster with a magic stat + MP; one monster.
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.mp = 50;
    world.actors[0].battle.liveness = 1;
    world.set_battle_magic(0, 100);
    world.actors[1].battle.max_hp = 300;
    world.actors[1].battle.hp = 300;
    world.actors[1].battle.liveness = 1;
    // Give the caster a learned offensive spell: Flame (0x20, 5 MP).
    let mut party = legaia_save::Party::zeroed(1);
    let mut list = party.members[0].spell_list();
    list.count = 1;
    list.ids[0] = 0x20;
    party.members[0].set_spell_list(list);
    world.roster = party;

    // Open the spell submenu for the caster.
    world.battle_ctx.active_actor = 0;
    world.battle_spell_menu = world.build_battle_spell_session(0);
    {
        let m = world.battle_spell_menu.as_ref().expect("spell menu built");
        assert_eq!(m.spells.len(), 1, "one learned spell");
        assert!(m.spells[0].affordable, "50 MP covers a 5 MP spell");
    }

    // Frame 1: Cross opens the target cursor on the lone monster.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_spell_menu();
    assert!(world.battle_spell_menu.is_some(), "still picking a target");

    // Frame 2: Cross confirms the monster; the cast resolves.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_spell_menu();

    assert!(world.battle_spell_menu.is_none(), "spell menu closed");
    assert_eq!(world.actors[0].battle.mp, 45, "5 MP spent on Flame");
    assert!(
        world.actors[1].battle.hp < 300,
        "Flame should have damaged the monster"
    );
    assert_eq!(
        world.battle_ctx.action_state,
        ActionState::EndOfAction.as_byte(),
        "turn parked at EndOfAction so the loop cycles"
    );
    let fx = world.drain_battle_hit_fx();
    assert_eq!(fx.len(), 1);
    assert!(!fx[0].is_heal, "offensive spell is damage, not heal");
    assert_eq!(fx[0].target_slot, 1);
}

#[test]
fn silenced_caster_cannot_open_the_magic_submenu() {
    use crate::spells::SpellCatalog;
    use legaia_engine_vm::status_effects::StatusKind;

    let mut world = World::new();
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    world.actors[0].battle.mp = 50;
    world.actors[0].battle.liveness = 1;
    world.set_battle_magic(0, 100);
    // A learned offensive spell, so the submenu would build absent any status.
    let mut party = legaia_save::Party::zeroed(1);
    let mut list = party.members[0].spell_list();
    list.count = 1;
    list.ids[0] = 0x20;
    party.members[0].set_spell_list(list);
    world.roster = party;

    // No status: the Magic submenu builds.
    assert!(
        world.build_battle_spell_session(0).is_some(),
        "control: an unafflicted caster can open Magic"
    );

    // Curse: the submenu refuses to open, so the caller bounces the player
    // back to the command menu (the party-side mirror of the monster path).
    world
        .status_effects
        .apply_with_duration(0, StatusKind::Curse, 4);
    assert!(
        world.build_battle_spell_session(0).is_none(),
        "a silenced caster must not open the Magic submenu"
    );
}

#[test]
fn battle_magic_cast_applies_mp_half_ability_bit() {
    use crate::input::PadButton;
    use crate::spells::SpellCatalog;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.battle_player_driven = true;
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.mp = 50;
    world.actors[0].battle.liveness = 1;
    world.set_battle_magic(0, 100);
    world.actors[1].battle.max_hp = 300;
    world.actors[1].battle.hp = 300;
    world.actors[1].battle.liveness = 1;
    // MP-half accessory bit (0x20) on the caster's character record.
    world.character_ability_bits[0] = 0x20;

    let mut party = legaia_save::Party::zeroed(1);
    let mut list = party.members[0].spell_list();
    list.count = 1;
    list.ids[0] = 0x20; // Flame, 5 MP
    party.members[0].set_spell_list(list);
    world.roster = party;

    world.battle_ctx.active_actor = 0;
    world.battle_spell_menu = world.build_battle_spell_session(0);

    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_spell_menu();
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_spell_menu();

    // Flame is 5 MP; the MP-half bit charges `5 - (5>>1) = 3` (retail rounds
    // up on odd costs, not floor 5/2 = 2), so 50 -> 47 (vs 45 flat).
    assert_eq!(
        world.actors[0].battle.mp, 47,
        "MP-half ability bit should reduce the live-cast cost by half (round up)"
    );
}

#[test]
fn refresh_party_ability_bits_derives_and_propagates_party_wide() {
    use crate::accessory_passives::AccessoryPassives;

    let mut world = World {
        party_count: 2,
        ..World::default()
    };
    // Synthetic catalog: item 0x50 grants wearer-only passive 0x05 (the
    // MP-half bit 0x20); item 0x51 grants party-wide passive 0x0E.
    world.set_accessory_passives(AccessoryPassives::from_entries(
        [(0x50, 0x05), (0x51, 0x0E)],
        [0x0E],
    ));
    let mut party = legaia_save::Party::zeroed(2);
    let mut eq = party.members[0].equipment();
    eq.slots[7] = 0x50;
    party.members[0].set_equipment(eq);
    let mut eq = party.members[1].equipment();
    eq.slots[5] = 0x51;
    party.members[1].set_equipment(eq);
    world.roster = party;

    world.refresh_party_ability_bits();

    // Wearer-only bit lands on member 0 only.
    assert_eq!(world.character_ability_bits[0] & 0x20, 0x20);
    assert_eq!(world.character_ability_bits[1] & 0x20, 0);
    // Party-wide bit (index 0x0E) propagates into every member's effective
    // mask, and into the global mask (the FUN_800431D0 port).
    assert_eq!(world.character_ability_bits[0] & (1 << 0x0E), 1 << 0x0E);
    assert_eq!(world.character_ability_bits[1] & (1 << 0x0E), 1 << 0x0E);
    assert!(world.party_has_ability(0x0E));
    assert!(!world.party_has_ability(0x06));
    // The record-side bitfield is rebuilt with each wearer's OWN bits.
    assert_eq!(world.roster.members[0].ability_bits()[0], 0x20);
    assert_eq!(world.roster.members[1].ability_bits()[1], 0x40); // bit 14
}

#[test]
fn refresh_party_ability_bits_noops_without_a_catalog() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.roster = legaia_save::Party::zeroed(1);
    // Synthetic setups write the bits directly; an empty catalog must not
    // clobber them.
    world.character_ability_bits[0] = 0x20;
    world.refresh_party_ability_bits();
    assert_eq!(world.character_ability_bits[0], 0x20);
}

#[test]
fn seed_party_battle_stats_applies_accessory_stat_and_hp_boosts() {
    use crate::accessory_passives::AccessoryPassives;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    // Item 0x52 grants passive 0x06 (ATK +20%); item 0x53 grants passive
    // 0x00 (max HP +10%).
    world.set_accessory_passives(AccessoryPassives::from_entries(
        [(0x52, 0x06), (0x53, 0x00)],
        [],
    ));
    let mut party = legaia_save::Party::zeroed(1);
    let rec = &mut party.members[0];
    rec.set_record_stats(legaia_save::character::RecordStats {
        hp_max: 100,
        mp_max: 30,
        cap_constant: 100,
        agl: 40,
        atk: 100,
        udf: 50,
        ldf: 60,
        spd: 35,
        int: 20,
    });
    rec.set_live_stats(legaia_save::character::LiveStats {
        agl: 40,
        atk: 100,
        udf: 50,
        ldf: 60,
        spd: 35,
        int: 20,
    });
    let mut eq = rec.equipment();
    eq.slots[6] = 0x52;
    eq.slots[7] = 0x53;
    rec.set_equipment(eq);
    let mut hms = rec.hp_mp_sp();
    hms.hp_cur = 100;
    hms.hp_max = 100;
    rec.set_hp_mp_sp(hms);
    world.load_party(party);

    world.seed_party_battle_stats();

    // ATK +20% of the base: 100 + 100/5 = 120.
    assert_eq!(world.battle_attack[0], 120);
    // Max HP +10% of the base, applied to the live battle actor.
    assert_eq!(world.actors[0].battle.max_hp, 110);
    // The ability bits are populated for the MP-cost consumers.
    assert_eq!(world.character_ability_bits[0] & 0x41, 0x41); // bits 0 + 6
}

#[test]
fn battle_magic_buff_raises_scalar_refreshes_and_expires() {
    use crate::spells::{BuffStat, SpellOutcome};

    let mut world = World::default();
    world.set_battle_attack(0, 50);

    // Power Up: retail stat-up is the x6/5 ramp (50 -> 60), not a flat +20.
    world.fold_spell_outcome(SpellOutcome::Buff {
        target: 0,
        stat: BuffStat::Attack,
        magnitude: 20,
        turns: 2,
    });
    assert_eq!(
        world.battle_attack[0], 60,
        "stat-up ramps the scalar by x6/5"
    );
    assert_eq!(world.battle_buffs.len(), 1);

    // Re-casting refreshes (reverts the old delta first, so the ramp re-applies
    // from the base 50 -> 60, no compounding).
    world.fold_spell_outcome(SpellOutcome::Buff {
        target: 0,
        stat: BuffStat::Attack,
        magnitude: 20,
        turns: 2,
    });
    assert_eq!(
        world.battle_attack[0], 60,
        "refresh does not compound the ramp"
    );
    assert_eq!(world.battle_buffs.len(), 1);

    // Ages one turn per the buffed actor's turn; expires on the 2nd.
    world.tick_battle_buffs_on_turn(0);
    assert_eq!(world.battle_attack[0], 60);
    world.tick_battle_buffs_on_turn(0);
    assert_eq!(
        world.battle_attack[0], 50,
        "expiry reverts the ramp delta exactly"
    );
    assert!(world.battle_buffs.is_empty());
}

#[test]
fn battle_magic_buff_ramp_is_multiplicative_not_additive() {
    use crate::spells::{BuffStat, SpellOutcome};

    let mut world = World::default();
    // At a 200 scalar the retail x6/5 ramp (->240) diverges from a flat +20
    // (->220): proves the live buff is multiplicative, not additive.
    world.set_battle_magic(1, 200);
    world.fold_spell_outcome(SpellOutcome::Buff {
        target: 1,
        stat: BuffStat::MagicAttack,
        magnitude: 20,
        turns: 1,
    });
    assert_eq!(world.battle_magic[1], 240, "x6/5 ramp, not flat +20");
    world.tick_battle_buffs_on_turn(1);
    assert_eq!(world.battle_magic[1], 200, "ramp delta reverts exactly");

    // The ramp clamps at 0xFFFF (buff_ramp ceiling) without overflow.
    world.set_battle_attack(2, 60_000);
    world.fold_spell_outcome(SpellOutcome::Buff {
        target: 2,
        stat: BuffStat::Attack,
        magnitude: 20,
        turns: 1,
    });
    assert_eq!(world.battle_attack[2], 0xFFFF, "ramp clamps at u16 max");
    world.tick_battle_buffs_on_turn(2);
    assert_eq!(
        world.battle_attack[2], 60_000,
        "clamped delta still reverts"
    );
}

#[test]
fn battle_magic_debuff_saturates_at_zero_and_reverts_exactly() {
    use crate::spells::{BuffStat, SpellOutcome};

    let mut world = World::default();
    // Power Down on an enemy with a small attack: -25 saturates the u16
    // scalar at 0, and the recorded delta is the actual change (-10).
    world.set_battle_attack(3, 10);
    world.fold_spell_outcome(SpellOutcome::Buff {
        target: 3,
        stat: BuffStat::Attack,
        magnitude: -25,
        turns: 1,
    });
    assert_eq!(world.battle_attack[3], 0, "debuff saturates at zero");

    // One tick expires it; the exact -10 delta is reverted back to 10.
    world.tick_battle_buffs_on_turn(3);
    assert_eq!(world.battle_attack[3], 10);
    assert!(world.battle_buffs.is_empty());
}

#[test]
fn battle_magic_capture_downs_a_weakened_monster_and_logs_the_id() {
    use crate::spells::SpellOutcome;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    // rng_state 0 -> first next_rng() % 100 == 23 (deterministic).
    world.rng_state = 0;
    world.actors[1].battle.max_hp = 100;
    world.actors[1].battle.hp = 10; // missing 90
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(42);

    // hit_pct 60, missing 90/100 -> effective 54; roll 23 < 54 -> captured.
    world.fold_spell_outcome(SpellOutcome::CaptureRoll {
        target: 1,
        hit_pct: 60,
    });
    assert_eq!(
        world.actors[1].battle.liveness, 0,
        "captured monster is downed"
    );
    assert_eq!(world.actors[1].battle.hp, 0);
    assert_eq!(world.drain_battle_captures(), vec![42]);

    // A near-full-HP monster has a tiny effective chance -> the same roll
    // misses and the monster is untouched.
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.rng_state = 0; // roll 23
    world.actors[1].battle.max_hp = 100;
    world.actors[1].battle.hp = 95; // missing 5 -> effective 3
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(7);
    world.fold_spell_outcome(SpellOutcome::CaptureRoll {
        target: 1,
        hit_pct: 60,
    });
    assert_eq!(
        world.actors[1].battle.liveness, 1,
        "healthy monster resists"
    );
    assert!(world.battle_captures.is_empty());
}

#[test]
fn battle_magic_escape_returns_to_field() {
    use crate::input::PadButton;
    use crate::spells::SpellCatalog;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.battle_player_driven = true;
    world.mode = SceneMode::Battle;
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.mp = 20;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.max_hp = 200;
    world.actors[1].battle.hp = 200;
    world.actors[1].battle.liveness = 1;
    world.spell_catalog = SpellCatalog::vanilla();

    // Open the spell submenu with Warp (0x41, SelfOnly escape) learned.
    world.battle_ctx.active_actor = 0;
    world.battle_spell_menu = Some(crate::battle_magic::BattleSpellSession::new(
        0,
        0,
        &[0x41],
        &world.spell_catalog,
        20,
        0,
    ));

    // SelfOnly target resolves immediately, so one Cross casts Warp.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_spell_menu();

    assert_eq!(world.mode, SceneMode::Field, "escape returns to the field");
    assert!(
        world.battle_spell_menu.is_none(),
        "submenu dropped on escape"
    );
    assert!(
        !world.battle_escaped,
        "escape flag cleared by finish_battle"
    );
    assert!(world.last_battle_rewards.is_none(), "escape grants no loot");
}
