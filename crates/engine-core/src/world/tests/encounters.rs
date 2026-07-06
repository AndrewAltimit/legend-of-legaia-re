use super::*;

#[test]
fn install_encounter_for_scene_resolves_field_pattern() {
    use crate::encounter_registry::vanilla_encounter_registry;
    let mut world = World::new();
    let r = vanilla_encounter_registry();
    let installed = world.install_encounter_for_scene(&r, "map01");
    assert!(installed, "field pattern should match");
    assert!(world.encounter.is_some());
}

#[test]
fn install_encounter_for_scene_quiets_in_towns() {
    use crate::encounter_registry::vanilla_encounter_registry;
    let mut world = World::new();
    let r = vanilla_encounter_registry();
    let installed = world.install_encounter_for_scene(&r, "town01");
    assert!(!installed, "town pattern resolves but is quiet");
    assert!(
        world.encounter.is_some(),
        "session installed for nil checks"
    );
}

#[test]
fn install_encounter_for_scene_returns_false_with_no_default() {
    use crate::encounter_registry::EncounterRegistry;
    let mut world = World::new();
    let r = EncounterRegistry::new(); // empty, no default
    let installed = world.install_encounter_for_scene(&r, "anything");
    assert!(!installed);
    assert!(world.encounter.is_none());
}

#[test]
fn install_encounter_for_scene_replaces_active_session() {
    use crate::encounter_registry::vanilla_encounter_registry;
    let mut world = World::new();
    let r = vanilla_encounter_registry();
    // Install a field session, then a town session - the town call
    // should replace the field session even though it's quiet.
    world.install_encounter_for_scene(&r, "map01");
    assert!(world.encounter.is_some());
    let initial_table_label = world
        .encounter
        .as_ref()
        .unwrap()
        .tracker()
        .table()
        .scene_label
        .clone();
    world.install_encounter_for_scene(&r, "town01");
    let new_table_label = world
        .encounter
        .as_ref()
        .unwrap()
        .tracker()
        .table()
        .scene_label
        .clone();
    assert_ne!(initial_table_label, new_table_label);
}

#[test]
fn install_encounter_from_record_registers_and_arms() {
    use crate::encounter_record::EncounterRecord;
    let mut world = World::new();
    // mc2-shaped record: two monsters, both id 4.
    let record = EncounterRecord {
        count: 2,
        monster_ids: [0x04, 0x04, 0, 0],
    };
    let formation_id = world
        .install_encounter_from_record("map01", &record)
        .expect("non-empty record produces an id");
    // Formation registered.
    let formation = world
        .formation_table
        .formation(formation_id)
        .expect("formation registered");
    assert_eq!(formation.slots.len(), 2);
    assert_eq!(formation.slots[0].monster_id, 4);
    assert_eq!(formation.slots[1].monster_id, 4);
    // Session installed and rate forced high.
    let session = world.encounter.as_ref().expect("session installed");
    assert_eq!(session.tracker().table().trigger_rate_q8, 0xFF);
    assert_eq!(session.tracker().table().entries.len(), 1);
    assert_eq!(
        session.tracker().table().entries[0].formation_id,
        formation_id
    );
}

#[test]
fn install_scripted_encounter_parses_window_and_arms_battle() {
    let mut world = World::new();
    world.set_formation_table(
        crate::monster_catalog::vanilla_formation_table(),
        crate::monster_catalog::vanilla_monster_catalog(),
    );
    world.set_active_scene_label("town01");
    world.mode = SceneMode::Field;
    world.arm_scripted_encounter(true);
    // Record window overlaying the arm opcode: [op][op1][op2][count=2][ids..].
    let window = [0x37u8, 0x00, 0x00, 0x02, 0x4F, 0x50, 0x00, 0x00];
    let formation_id = world
        .install_scripted_encounter(&window)
        .expect("non-empty record installs a formation");
    // Fire-once: a successful install disarms the carrier flag.
    assert!(!world.scripted_encounter_armed);
    // Formation registered with the window's two ids.
    let formation = world
        .formation_table
        .formation(formation_id)
        .expect("formation registered");
    assert_eq!(formation.slots.len(), 2);
    assert_eq!(formation.slots[0].monster_id, 0x4F);
    assert_eq!(formation.slots[1].monster_id, 0x50);
    // Session installed at the forced-high rate.
    assert_eq!(
        world
            .encounter
            .as_ref()
            .unwrap()
            .tracker()
            .table()
            .trigger_rate_q8,
        0xFF
    );
    // Event surfaced for engine visibility.
    assert!(world.pending_field_events.iter().any(|e| matches!(
        e,
        FieldEvent::ScriptedEncounter { record } if record == &window
    )));
    // The very next field step flips Field -> a triggered encounter.
    assert!(
        world.on_field_step(),
        "forced-rate roll triggers the battle"
    );
}

#[test]
fn install_scripted_encounter_empty_or_short_window_returns_none() {
    let mut world = World::new();
    world.set_active_scene_label("town01");
    // count = 0 -> empty record -> no install.
    assert_eq!(world.install_scripted_encounter(&[0, 0, 0, 0]), None);
    assert!(world.encounter.is_none());
    // Too short to even hold the count byte -> parse fails.
    assert_eq!(world.install_scripted_encounter(&[0, 0]), None);
}

#[test]
fn seed_party_battle_stats_folds_live_stats_and_equipment() {
    use crate::battle_stats::{EquipmentTable, ItemModifier};
    use legaia_save::EquipmentSlots;
    use legaia_save::character::LiveStats;

    let mut world = World::new();
    let mut party = legaia_save::Party::zeroed(1);
    party.members[0].set_live_stats(LiveStats {
        agl: 12,
        atk: 30,
        udf: 10,
        ldf: 8,
        spd: 5,
        int: 4,
    });
    let mut slots = [0u8; 8];
    slots[0] = 5; // a weapon in the first slot
    party.members[0].set_equipment(EquipmentSlots { slots });
    world.load_party(party);

    // Item 5 grants +7 attack, +3 UDF, +2 LDF.
    let mut table = EquipmentTable::new();
    table.set(
        5,
        ItemModifier {
            atk: 7,
            udf: 3,
            ldf: 2,
            spd: 0,
            int: 0,
            ability_bits: [0; 32],
        },
    );
    world.set_equipment_table(table);

    world.seed_party_battle_stats();
    assert_eq!(world.battle_attack[0], 37, "30 base + 7 weapon");
    assert_eq!(
        world.battle_defense_split[0],
        Some((13, 10)),
        "(10+3) UDF, (8+2) LDF"
    );
}

#[test]
fn seed_party_battle_stats_skips_zeroed_roster() {
    // A synthetic battle sets battle_attack directly then loads a zeroed
    // roster; seeding must not clobber the manual value.
    let mut world = World::new();
    world.set_battle_attack(0, 60);
    world.load_party(legaia_save::Party::zeroed(3));
    world.seed_party_battle_stats();
    assert_eq!(world.battle_attack[0], 60, "zeroed roster leaves it intact");
    assert_eq!(world.battle_defense_split[0], None);
}

#[test]
fn seed_party_battle_stats_scales_ap_base_with_level() {
    use legaia_save::character::LiveStats;

    let mut world = World::new();
    let mut party = legaia_save::Party::zeroed(3);
    // Slot 0 at level 1 (base 4), slot 1 at level 23 (base 6), slot 2 at
    // level 99 (capped 10). A non-zero atk so the seed doesn't skip them.
    for (slot, level) in [(0usize, 1u8), (1, 23), (2, 99)] {
        party.members[slot].set_live_stats(LiveStats {
            agl: 10,
            atk: 20,
            udf: 8,
            ldf: 8,
            spd: 5,
            int: 4,
        });
        party.members[slot].set_level(level);
    }
    world.load_party(party);

    world.seed_party_battle_stats();
    assert_eq!(world.ap_gauges[0].base_ap, 4, "level 1 -> base 4");
    assert_eq!(world.ap_gauges[1].base_ap, 6, "level 23 -> 4 + 23/10 = 6");
    assert_eq!(world.ap_gauges[2].base_ap, 10, "level 99 -> capped at 10");

    // The round-start reset picks up the seeded base as the per-turn budget.
    world.reset_party_ap();
    assert_eq!(world.ap_gauges[1].current_ap, 6);
    assert_eq!(world.ap_gauges[2].current_ap, 10);
}

#[test]
fn drain_pending_scripted_encounter_only_when_queued() {
    let mut world = World::new();
    world.set_formation_table(
        crate::monster_catalog::vanilla_formation_table(),
        crate::monster_catalog::vanilla_monster_catalog(),
    );
    world.set_active_scene_label("town01");
    // Nothing queued -> no-op.
    world.drain_pending_scripted_encounter();
    assert!(world.encounter.is_none());
    // Queue a window (as the armed forwarded-PC hook would) and drain.
    world.pending_scripted_encounter = Some(vec![0, 0, 0, 1, 0x12, 0, 0, 0]);
    world.drain_pending_scripted_encounter();
    assert!(world.pending_scripted_encounter.is_none());
    assert!(world.encounter.is_some());
}

#[test]
fn install_encounter_from_record_empty_returns_none() {
    use crate::encounter_record::EncounterRecord;
    let mut world = World::new();
    let id = world.install_encounter_from_record("map01", &EncounterRecord::EMPTY);
    assert!(id.is_none());
    // No session installed.
    assert!(world.encounter.is_none());
}

#[test]
fn install_man_formation_forces_registered_row() {
    use crate::monster_catalog::{FormationDef, FormationSlot};
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.set_active_scene_label("town01");
    // Register a lone-monster formation at id 4 (town01's Tetsu row shape).
    world
        .formation_table
        .insert(FormationDef::new(4, vec![FormationSlot::new(0x4F)]));

    // Unknown id -> None, no session.
    assert!(world.install_man_formation(9).is_none());
    assert!(world.encounter.is_none());

    // Registered id installs a forced-rate session that triggers next step.
    assert_eq!(world.install_man_formation(4), Some(4));
    assert!(world.encounter.is_some());
    assert!(
        world.on_field_step(),
        "forced-rate session triggers on the next step"
    );
}

#[test]
fn field_carrier_engage_launches_battle_and_returns_to_field() {
    use crate::encounter_record::RIM_ELM_TRAINING_FORMATION_ID;
    use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Field;
    world.live_gameplay_loop = true; // auto-resolve the battle leg
    world.set_active_scene_label("town01");
    // A capable lone party member so the battle can resolve.
    world.actors[0].active = true;
    world.actors[0].battle.hp = 400;
    world.actors[0].battle.max_hp = 400;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 80);
    // town01's Tetsu row: formation index 4 = lone monster id 0x4F.
    world.formation_table.insert(FormationDef::new(
        RIM_ELM_TRAINING_FORMATION_ID,
        vec![FormationSlot::new(0x4F)],
    ));
    let mut cat = MonsterCatalog::new();
    cat.insert(MonsterDef::new(0x4F, "Tetsu", 999, 40));
    world.set_monster_catalog(cat);

    // Place one scripted-encounter carrier (the Tetsu NPC) - the field-mode
    // use of the FUN_801DA51C SM.
    world.install_field_carriers(vec![FieldCarrierConfig::ScriptedEncounter {
        formation_id: RIM_ELM_TRAINING_FORMATION_ID,
    }]);

    // Idle: ticking does NOT launch a battle (towns are 0% random; the
    // carrier waits for the dialogue-accept).
    world.tick();
    assert_eq!(
        world.mode,
        SceneMode::Field,
        "an idle scripted carrier never self-fires"
    );
    assert_eq!(world.field_carriers[0].state, 0, "carrier still Idle");

    // The dialogue-accept advances the carrier to Activating; the next tick
    // runs the state-1 body (formation copy) and the case 2/3 fall-through
    // (battle handoff), flipping Field -> Battle, tagged to return to field.
    world.engage_field_carrier(0);
    world.tick();
    assert_eq!(world.mode, SceneMode::Battle);
    assert_eq!(world.battle_return_mode, SceneMode::Field);
    assert!(world.field_return.is_some());
    let formation = world.active_formation.as_ref().expect("active formation");
    assert_eq!(
        formation.slots[0].monster_id, 0x4F,
        "Tetsu in the enemy slot"
    );
    assert_eq!(
        world.field_carriers[0].state,
        vm::world_map::EntityState::Terminal as u16,
        "carrier retired to Terminal after the transition"
    );

    // Drive the fight to completion; it must return to the field.
    let mut returned = false;
    for _ in 0..8000 {
        world.tick();
        if world.mode != SceneMode::Battle {
            returned = true;
            break;
        }
    }
    assert!(returned, "battle resolves");
    assert_eq!(world.mode, SceneMode::Field, "returns to the field");
    // The carrier stays Terminal - the scripted fight fires exactly once.
    assert_eq!(
        world.field_carriers[0].state,
        vm::world_map::EntityState::Terminal as u16
    );
}

#[test]
fn field_carrier_unengaged_never_fires() {
    use crate::encounter_record::RIM_ELM_TRAINING_FORMATION_ID;
    use crate::monster_catalog::{FormationDef, FormationSlot};

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Field;
    world.set_active_scene_label("town01");
    world.formation_table.insert(FormationDef::new(
        RIM_ELM_TRAINING_FORMATION_ID,
        vec![FormationSlot::new(0x4F)],
    ));
    world.install_field_carriers(vec![FieldCarrierConfig::ScriptedEncounter {
        formation_id: RIM_ELM_TRAINING_FORMATION_ID,
    }]);

    // Many idle ticks must never flip into battle (no random rate).
    for _ in 0..256 {
        world.tick();
        assert_eq!(world.mode, SceneMode::Field);
    }
    assert!(world.field_return.is_none());
    assert!(world.pending_field_carrier_battle.is_none());
}

#[test]
fn begin_new_game_clears_state_and_enters_field() {
    let mut world = World::new();
    // Dirty the world as if a prior session had been played.
    world.mode = SceneMode::Battle;
    world.story_flags = 0xDEAD_BEEF;
    world.story_flag_bits = vec![1, 2, 3];
    world.money = 4242;
    world.inventory.insert(0x10, 5);
    world.scripted_encounter_armed = true;
    world.game_over = true;
    world.play_time_seconds = 9999;

    world.begin_new_game();

    // The retail field-launch (master mode 3) clean slate.
    assert_eq!(world.mode, SceneMode::Field);
    assert_eq!(world.story_flags, 0);
    assert!(world.story_flag_bits.is_empty());
    // New-game gold is the retail constant (FUN_80034A6C), not zero.
    assert_eq!(world.money, NEW_GAME_STARTING_GOLD);
    assert!(world.inventory.is_empty());
    assert!(!world.scripted_encounter_armed);
    assert!(world.encounter.is_none());
    assert!(!world.game_over);
    assert_eq!(world.play_time_seconds, 0);
}

#[test]
fn prologue_handoff_fires_once_on_confirm_in_the_opening_chain() {
    let mut world = World::new();
    world.set_active_scene_label(legaia_asset::new_game::OPENING_CUTSCENE_SCENE);
    world.opening_chain_active = true;

    // Not armed yet: confirm does nothing.
    assert_eq!(world.take_prologue_handoff(true), None);

    world.arm_prologue_handoff();
    assert_ne!(world.story_flags & PROLOGUE_HANDOFF_FLAG, 0);

    // Armed but no confirm: stays in the cutscene.
    assert_eq!(world.take_prologue_handoff(false), None);
    assert_ne!(world.story_flags & PROLOGUE_HANDOFF_FLAG, 0);

    // Armed + confirm: skips to town01 and clears the bit (fire-once). This
    // is the retail intro-skip - it fires mid-narration too.
    world.open_cutscene_narration(vec!["Page".into()]);
    assert_eq!(
        world.take_prologue_handoff(true),
        Some(legaia_asset::new_game::OPENING_SCENE)
    );
    assert_eq!(world.story_flags & PROLOGUE_HANDOFF_FLAG, 0);
    assert!(
        world.cutscene_narration.is_none(),
        "the skip tears down the playing narration"
    );
    assert!(world.entering_town01_opening);
    assert!(!world.opening_chain_active);

    // A second confirm does not re-fire.
    assert_eq!(world.take_prologue_handoff(true), None);
}

#[test]
fn prologue_handoff_only_fires_while_the_opening_chain_plays() {
    let mut world = World::new();
    // Armed, confirm pressed, but no opening chain is playing (e.g. a normal
    // field visit) - the skip gate stays closed.
    world.set_active_scene_label(legaia_asset::new_game::OPENING_SCENE);
    world.arm_prologue_handoff();
    assert_eq!(world.take_prologue_handoff(true), None);
    // Bit is left intact for the gate to fire only during the opening.
    assert_ne!(world.story_flags & PROLOGUE_HANDOFF_FLAG, 0);
}

// ---- scripted single-boss encounter (battle-id path, FUN_8005567c) ----

#[test]
fn install_boss_encounter_arms_a_lone_monster_formation() {
    let mut world = World::new();
    world.set_active_scene_label("rikuroa");
    let fid = world
        .install_boss_encounter(75, Some(0x1BE))
        .expect("boss install returns a formation id");
    // Synthetic boss-namespace id, disjoint from MAN row ids.
    assert_eq!(fid, crate::world::BOSS_FORMATION_ID_BASE | 75);
    let def = world
        .formation_table
        .formation(fid)
        .expect("boss formation registered");
    assert_eq!(def.slots.len(), 1, "boss is a lone monster");
    assert_eq!(def.slots[0].monster_id, 75);
    // Armed to fire on the next field step, with the victory latch pending.
    assert!(world.scripted_formation_pending);
    assert_eq!(world.boss_formation_id, Some(fid));
    assert_eq!(world.pending_boss_victory_flag, Some(0x1BE));
    // The gate flag is NOT set yet - only a win latches it.
    assert!(!world.system_flag_test(0x1BE));
}

#[test]
fn boss_victory_latches_the_gate_flag_via_apply_battle_loot() {
    use crate::monster_catalog::MonsterCatalog;
    let mut world = World::new();
    world.set_active_scene_label("rikuroa");
    let fid = world.install_boss_encounter(75, Some(0x1BE)).unwrap();
    let boss_formation = world.formation_table.formation(fid).cloned().unwrap();
    // Resolving loot for the boss formation (the win path) latches the gate.
    let cat = MonsterCatalog::new();
    let _ = world.apply_battle_loot(&boss_formation, &cat);
    assert!(
        world.system_flag_test(0x1BE),
        "beating the boss sets its first-visit one-shot gate flag"
    );
    // Pending state cleared so a re-fought formation doesn't re-latch.
    assert_eq!(world.pending_boss_victory_flag, None);
    assert_eq!(world.boss_formation_id, None);
}

#[test]
fn non_boss_victory_does_not_latch_a_gate_flag() {
    use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog};
    let mut world = World::new();
    world.set_active_scene_label("rikuroa");
    world.install_boss_encounter(75, Some(0x1BE)).unwrap();
    // Win a DIFFERENT (random) formation while the boss is armed: the latch
    // is keyed on the boss formation id, so the gate flag stays clear.
    let other = FormationDef::new(3, vec![FormationSlot::new(10)]);
    let cat = MonsterCatalog::new();
    let _ = world.apply_battle_loot(&other, &cat);
    assert!(!world.system_flag_test(0x1BE));
    assert_eq!(world.pending_boss_victory_flag, Some(0x1BE));
}
