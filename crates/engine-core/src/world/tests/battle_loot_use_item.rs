use super::*;

#[test]
fn apply_battle_loot_never_drops_when_rate_zero() {
    use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};
    let mut cat = MonsterCatalog::new();
    let mut def = MonsterDef::new(7, "Slime", 10, 5);
    def.drop_item = Some(0x42);
    def.drop_rate_q8 = 0;
    cat.insert(def);
    let formation = FormationDef::new(1000, vec![FormationSlot::new(7)]);
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    let rewards = world.apply_battle_loot(&formation, &cat);
    assert!(rewards.drops.is_empty());
    assert!(!world.inventory.contains_key(&0x42));
}

#[test]
fn load_full_hydrates_level_up_tracker_from_record_levels() {
    // Build a 3-character save with levels 7, 12, 25.
    let mut party = legaia_save::Party::zeroed(3);
    party.members[0].set_level(7);
    party.members[1].set_level(12);
    party.members[2].set_level(25);
    let sf = legaia_save::SaveFile {
        party,
        ext: legaia_save::SaveExt::default(),
        ext_v2: legaia_save::SaveExtV2::default(),
    };
    let mut world = World::new();
    // Tracker defaults to 1 for every slot.
    assert_eq!(world.level_up_tracker.level[0], 1);
    world.load_full(sf);
    assert_eq!(world.level_up_tracker.level[0], 7);
    assert_eq!(world.level_up_tracker.level[1], 12);
    assert_eq!(world.level_up_tracker.level[2], 25);
}

#[test]
fn load_full_zero_level_record_clamps_to_one() {
    // Records that haven't had a level written (zero byte at +0x100)
    // shouldn't make the tracker think the slot is below L1.
    let party = legaia_save::Party::zeroed(2);
    let sf = legaia_save::SaveFile {
        party,
        ext: legaia_save::SaveExt::default(),
        ext_v2: legaia_save::SaveExtV2::default(),
    };
    let mut world = World::new();
    world.load_full(sf);
    assert_eq!(world.level_up_tracker.level[0], 1);
    assert_eq!(world.level_up_tracker.level[1], 1);
}

#[test]
fn apply_battle_xp_scales_three_quarters_and_ceils() {
    let mut world = World {
        party_count: 3,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.actors[1].battle.hp = 100;
    world.actors[2].battle.hp = 100;
    // FUN_8004E568: 101 summed -> *3/4 = 101 - (101>>2 = 25) = 76, then
    // ceil(76 / 3 alive) = 26 each (floor would give 25). Below the 50 L2
    // threshold, so it just accumulates.
    let _ = world.apply_battle_xp(101);
    assert_eq!(world.level_up_tracker.xp[0], 26);
    assert_eq!(world.level_up_tracker.xp[1], 26);
    assert_eq!(world.level_up_tracker.xp[2], 26);
}

#[test]
fn level_up_banner_countdown_clears() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.apply_battle_xp(68); // 3/4-scaled ceil to 51 >= the 50 L2 threshold
    assert!(world.current_level_up_banner.is_some());
    for _ in 0..=crate::levelup::LevelUpBanner::DEFAULT_FRAMES {
        world.tick();
    }
    assert!(
        world.current_level_up_banner.is_none(),
        "level-up banner should have cleared"
    );
}

#[test]
fn no_level_up_banner_when_xp_insufficient() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.apply_battle_xp(49); // retail table: 49 < 50 (L2 threshold)
    assert!(world.current_level_up_banner.is_none());
}

#[test]
fn art_strike_applier_pushes_apply_art_strike_event() {
    // Drive `BattleHostImpl::apply_art_strike` from a synthetic
    // ArtStrikeInfo and assert the world's pending_battle_events grows
    // by one ApplyArtStrike with the resolved damage.
    use legaia_art::Character;
    use legaia_art::power::PowerByte;
    use legaia_art::queue::ActionConstant;
    use legaia_art::record::EnemyEffect;
    use legaia_engine_vm::battle_action::{ArtStrikeInfo, BattleActionHost};

    let mut world = World::new();
    world.set_battle_attack(0, 64);
    world.set_battle_defense(3, 10);
    let info = ArtStrikeInfo {
        strike_index: 0,
        anim_byte: 0x10,
        actor_slot: 0,
        target_slot: 3,
        character: Character::Vahn,
        art: ActionConstant::Art1B,
        power: Some(PowerByte::from_byte(0x1A)), // UDF × 28
        dmg_timing: Some(0x10),
        enemy_effect: EnemyEffect::Toxic,
        hit_cue: None,
    };
    let mut host = BattleHostImpl { world: &mut world };
    host.apply_art_strike(info);

    assert_eq!(world.pending_battle_events.len(), 1);
    match &world.pending_battle_events[0] {
        BattleEvent::ApplyArtStrike {
            actor_slot,
            target_slot,
            strike_index,
            outcome,
        } => {
            assert_eq!(*actor_slot, 0);
            assert_eq!(*target_slot, 3);
            assert_eq!(*strike_index, 0);
            assert_eq!(outcome.damage, Some(102));
            assert_eq!(outcome.enemy_effect, EnemyEffect::Toxic);
        }
        other => panic!("unexpected event: {:?}", other.summary()),
    }
}

#[test]
fn art_strike_split_defense_picks_udf_or_ldf() {
    // With a (UDF=5, LDF=50) split on slot 3, a UDF-targeted strike
    // hits 5 def → high damage; LDF-targeted hits 50 def → low.
    use legaia_art::Character;
    use legaia_art::power::PowerByte;
    use legaia_art::queue::ActionConstant;
    use legaia_art::record::EnemyEffect;
    use legaia_engine_vm::battle_action::{ArtStrikeInfo, BattleActionHost};

    let mut world = World::new();
    world.set_battle_attack(0, 64);
    world.set_battle_defense_split(3, Some((5, 50)));

    let mk = |power: PowerByte| ArtStrikeInfo {
        strike_index: 0,
        anim_byte: 0x10,
        actor_slot: 0,
        target_slot: 3,
        character: Character::Vahn,
        art: ActionConstant::Art1B,
        power: Some(power),
        dmg_timing: Some(0x10),
        enemy_effect: EnemyEffect::None,
        hit_cue: None,
    };
    // 0x1A = UDF × 28 → (64 * 28)/16 - 5 = 112 - 5 = 107.
    let mut host = BattleHostImpl { world: &mut world };
    host.apply_art_strike(mk(PowerByte::from_byte(0x1A)));
    // 0x1F = LDF × 28 → (64 * 28)/16 - 50 = 112 - 50 = 62.
    host.apply_art_strike(mk(PowerByte::from_byte(0x1F)));
    let events = world.drain_battle_events();
    let mut udf_dmg = None;
    let mut ldf_dmg = None;
    for e in events {
        if let BattleEvent::ApplyArtStrike { outcome, .. } = e
            && let Some(t) = outcome.power_target
        {
            match t {
                legaia_art::power::PowerTarget::Udf => udf_dmg = outcome.damage,
                legaia_art::power::PowerTarget::Ldf => ldf_dmg = outcome.damage,
            }
        }
    }
    assert_eq!(udf_dmg, Some(107));
    assert_eq!(ldf_dmg, Some(62));
}

#[test]
fn fold_battle_event_apply_art_strike_subtracts_hp_and_records_status() {
    use legaia_art::power::{PowerByte, PowerTarget};
    use legaia_art::queue::{ActionConstant, Character};
    use legaia_art::record::EnemyEffect;
    use legaia_engine_vm::battle_action::ArtStrikeInfo;

    let mut world = World::new();
    world.party_count = 4;
    for slot in 0..4 {
        world.actors[slot].active = true;
        world.actors[slot].battle.hp = 200;
        world.actors[slot].battle.max_hp = 200;
    }
    world.set_battle_attack(0, 64);
    world.set_battle_defense(3, 5);

    let info = ArtStrikeInfo {
        strike_index: 0,
        anim_byte: 0x10,
        actor_slot: 0,
        target_slot: 3,
        character: Character::Vahn,
        art: ActionConstant::Art1B,
        power: Some(PowerByte::from_byte(0x1A)), // UDF × 28
        dmg_timing: Some(0x10),
        enemy_effect: EnemyEffect::Toxic,
        hit_cue: None,
    };
    let mut host = BattleHostImpl { world: &mut world };
    host.apply_art_strike(info);
    let events = world.drain_battle_events();
    assert_eq!(events.len(), 1);
    for e in &events {
        let r = world.fold_battle_event(e);
        // 64 * 28 / 16 - 5 = 107 damage. Target slot 3 starts at 200,
        // ends at 93.
        assert_eq!(r, Some((3, 93)));
    }
    assert_eq!(world.actors[3].battle.hp, 93);
    // Toxic status was folded into pending_status.
    assert_eq!(
        world.actors[3].pending_status,
        Some(legaia_art::record::EnemyEffect::Toxic)
    );
    // PowerTarget enum is needed only to satisfy the import linter
    // when the assertions don't otherwise reference it.
    let _ = PowerTarget::Udf;
}

#[test]
fn fold_battle_event_surfaces_art_strike_sound_cues() {
    use crate::art_strike::{ArtStrikeOutcome, ScheduledCue};

    let mut world = World::new();
    world.party_count = 4;
    for slot in 0..4 {
        world.actors[slot].active = true;
        world.actors[slot].battle.hp = 200;
        world.actors[slot].battle.max_hp = 200;
    }

    // An art strike whose outcome carries a sound cue (0x1A, frame 16) and a
    // hit-effect-only visual cue (0x4C) - only the sound cue should surface.
    let outcome = ArtStrikeOutcome {
        damage: Some(40),
        enemy_effect: legaia_art::record::EnemyEffect::None,
        cues: vec![
            ScheduledCue {
                timing_frames: 16,
                kind: 0x1A,
            },
            ScheduledCue {
                timing_frames: 8,
                kind: 0x4C,
            },
        ],
        alt_range: false,
        power_target: Some(legaia_art::power::PowerTarget::Udf),
    };
    let event = BattleEvent::ApplyArtStrike {
        actor_slot: 0,
        target_slot: 3,
        strike_index: 0,
        outcome,
    };

    assert!(world.drain_battle_sfx_cues().is_empty(), "starts empty");
    world.fold_battle_event(&event);

    let cues = world.drain_battle_sfx_cues();
    assert_eq!(
        cues.len(),
        1,
        "only the sound cue (0x1A) surfaces, not 0x4C"
    );
    assert_eq!(cues[0].kind, 0x1A);
    assert_eq!(cues[0].timing_frames, 16);
    assert_eq!(cues[0].actor_slot, 0);
    assert_eq!(cues[0].target_slot, 3);
    // Drained once.
    assert!(world.drain_battle_sfx_cues().is_empty());
}

#[test]
fn fold_battle_event_other_variants_dont_modify_state() {
    let mut world = World::new();
    world.party_count = 1;
    world.actors[0].active = true;
    world.actors[0].battle.hp = 100;
    let r = world.fold_battle_event(&BattleEvent::CameraBounds);
    assert_eq!(r, None);
    assert_eq!(world.actors[0].battle.hp, 100);
}

#[test]
fn spell_anim_trigger_requests_summon_only_for_seru_ids() {
    let mut world = World::new();
    world.party_count = 1;
    world.actors[0].active = true;

    // A non-summon id (a monster attack) requests nothing.
    world.fold_battle_event(&BattleEvent::SpellAnimTrigger {
        party_slot: 0,
        spell_id: 0x27,
    });
    assert!(world.take_pending_summon_spawn().is_none());

    // Gimard Tail Fire (0x81) requests a summon spawn at the caster's pos.
    world.actors[0].move_state.world_x = 11;
    world.actors[0].move_state.world_y = 22;
    world.actors[0].move_state.world_z = 33;
    world.fold_battle_event(&BattleEvent::SpellAnimTrigger {
        party_slot: 0,
        spell_id: 0x81,
    });
    let req = world.take_pending_summon_spawn();
    assert_eq!(req, Some((0x81, [11, 22, 33])));
    // Taken once.
    assert!(world.take_pending_summon_spawn().is_none());
}

#[test]
fn use_item_heals_hp_clamped_to_max() {
    let mut world = World::new();
    world.party_count = 1;
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 50;
    world.set_item_catalog(full_test_catalog());
    // Item id 1 is the small heal in the vanilla catalog.
    let outcome = world.use_item(1, 0);
    assert!(matches!(
        outcome,
        crate::items::ItemOutcome::HealedHp { .. }
    ));
    // HP raised but clamped at max.
    assert!(world.actors[0].battle.hp > 50);
    assert!(world.actors[0].battle.hp <= 200);
}

#[test]
fn use_item_heal_all_fills_to_max() {
    let mut world = World::new();
    world.party_count = 1;
    world.actors[0].battle.max_hp = 300;
    world.actors[0].battle.hp = 100;
    world.set_item_catalog(full_test_catalog());
    // Find the HealAll entry (id 4 in the vanilla catalog - Healing Globe).
    let outcome = world.use_item(4, 0);
    assert!(matches!(
        outcome,
        crate::items::ItemOutcome::HealedHp { .. }
    ));
    assert_eq!(world.actors[0].battle.hp, 300);
}

#[test]
fn use_item_unknown_id_returns_no_effect() {
    let mut world = World::new();
    world.party_count = 1;
    world.set_item_catalog(full_test_catalog());
    let outcome = world.use_item(99, 0);
    assert!(matches!(outcome, crate::items::ItemOutcome::NoEffect));
}

#[test]
fn use_item_revive_writes_hp_after() {
    let mut world = World::new();
    world.party_count = 1;
    world.actors[0].battle.max_hp = 400;
    world.actors[0].battle.hp = 0; // dead
    world.set_item_catalog(full_test_catalog());
    // Resurrection Leaf is id 0x0C (50% revive).
    let outcome = world.use_item(0x0C, 0);
    assert!(matches!(outcome, crate::items::ItemOutcome::Revived { .. }));
    // 50% of 400 = 200.
    assert_eq!(world.actors[0].battle.hp, 200);
}

#[test]
fn use_item_hp_max_boost_raises_record_and_live_actor() {
    let mut party = legaia_save::Party::zeroed(1);
    let mut hms = party.members[0].hp_mp_sp();
    hms.hp_cur = 50;
    hms.hp_max = 100;
    party.members[0].set_hp_mp_sp(hms);
    let mut world = World::new();
    world.load_party(party);
    world.set_item_catalog(full_test_catalog());
    // Vital Tonic (0x0F): HpMax +10 - the outcome the old kernel dropped.
    let outcome = world.use_item(0x0F, 0);
    assert!(matches!(
        outcome,
        crate::items::ItemOutcome::StatRaised { .. }
    ));
    // Persistent record raised, current HP refilled by the gained amount.
    let rec_hms = world.roster.members[0].hp_mp_sp();
    assert_eq!(rec_hms.hp_max, 110);
    assert_eq!(rec_hms.hp_cur, 60);
    // Live battle actor raised too.
    assert_eq!(world.actors[0].battle.max_hp, 110);
    assert_eq!(world.actors[0].battle.hp, 60);
}

#[test]
fn use_item_attack_boost_raises_persistent_record_and_live_stat() {
    let mut party = legaia_save::Party::zeroed(1);
    let mut ls = party.members[0].live_stats();
    ls.atk = 20;
    party.members[0].set_live_stats(ls);
    let mut world = World::new();
    world.load_party(party);
    world.set_battle_attack(0, 20);
    world.set_item_catalog(full_test_catalog());
    // Power Tonic (0x0E): Attack +1.
    let outcome = world.use_item(0x0E, 0);
    assert!(matches!(
        outcome,
        crate::items::ItemOutcome::StatRaised { .. }
    ));
    assert_eq!(
        world.roster.members[0].live_stats().atk,
        21,
        "persistent attack raised"
    );
    // Re-derived live battle stat reflects it.
    assert_eq!(world.battle_attack[0], 21);
}

#[test]
fn use_item_stat_boost_caps_at_cap_constant() {
    let mut party = legaia_save::Party::zeroed(1);
    let mut rs = party.members[0].record_stats();
    rs.cap_constant = 100;
    party.members[0].set_record_stats(rs);
    let mut ls = party.members[0].live_stats();
    ls.atk = 99;
    party.members[0].set_live_stats(ls);
    let mut world = World::new();
    world.load_party(party);
    world.set_battle_attack(0, 99);
    // A custom big-boost item to exercise the cap.
    let mut cat = crate::items::ItemCatalog::new();
    cat.insert(crate::items::ItemEntry {
        id: 0x50,
        name: "Mega Tonic",
        effect: crate::items::ItemEffect::StatBoost {
            target: crate::items::StatBoostTarget::Attack,
            delta: 50,
        },
        usable_in_battle: false,
        usable_in_field: true,
    });
    world.set_item_catalog(cat);
    world.use_item(0x50, 0);
    assert_eq!(
        world.roster.members[0].live_stats().atk,
        100,
        "capped at the per-stat cap constant"
    );
}

#[test]
fn use_item_fury_boost_extends_ap_gauge_and_reverts_at_battle_end() {
    let mut world = World::new();
    // Seed a Fury Boost catalog entry directly (the disc seeder installs the
    // same `ActionGauge` marker; this exercises the apply path without a disc).
    world.item_catalog.insert(crate::items::ItemEntry {
        id: 0x81,
        name: "Fury Boost",
        effect: crate::items::ItemEffect::ActionGauge,
        usable_in_battle: true,
        usable_in_field: false,
    });
    world.ap_gauges[0] = crate::ap_gauge::ApGauge::with_base(10);
    world.ap_gauges[0].current_ap = 6; // mid-turn, some AP already spent

    // Fury Boost extends the gauge by the retail ×7/5 ratio: base 10 -> 14, and
    // the live gauge gains the +4 delta immediately.
    let out = world.use_item(0x81, 0);
    assert_eq!(out, crate::items::ItemOutcome::ActionGaugeExtended);
    assert_eq!(world.ap_gauges[0].base_ap, 14);
    assert_eq!(world.ap_gauges[0].current_ap, 10);
    assert_eq!(world.fury_boost[0], Some(4));

    // The boost survives a turn reset (it's "for one battle").
    world.ap_gauges[0].reset_for_turn();
    assert_eq!(world.ap_gauges[0].base_ap, 14);
    assert_eq!(world.ap_gauges[0].current_ap, 14);

    // Idempotent within the battle: a second Fury Boost does not compound.
    assert_eq!(
        world.use_item(0x81, 0),
        crate::items::ItemOutcome::ActionGaugeExtended
    );
    assert_eq!(world.ap_gauges[0].base_ap, 14);
    assert_eq!(world.fury_boost[0], Some(4));

    // Battle end reverts the extension and clears the flag.
    world.finish_battle();
    assert_eq!(world.ap_gauges[0].base_ap, 10);
    assert_eq!(world.fury_boost[0], None);
}

#[test]
fn use_item_fury_boost_on_non_party_slot_is_noop() {
    let mut world = World::new();
    world.item_catalog.insert(crate::items::ItemEntry {
        id: 0x81,
        name: "Fury Boost",
        effect: crate::items::ItemEffect::ActionGauge,
        usable_in_battle: true,
        usable_in_field: false,
    });
    // Slot 3+ is not a party AP-gauge slot (gauges are 0..=2).
    assert_eq!(world.use_item(0x81, 5), crate::items::ItemOutcome::NoEffect);
}

#[test]
fn use_item_cure_clears_status() {
    use legaia_art::record::EnemyEffect;
    let mut world = World::new();
    world.party_count = 1;
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.hp = 50;
    // Apply a Toxic status, then cure it via CureAll.
    world
        .status_effects
        .apply_from_enemy_effect(0, EnemyEffect::Toxic);
    assert!(world.status_effects.is_afflicted(0));
    world.set_item_catalog(full_test_catalog());
    // Antidote Flower is id 0x09 (CureAll).
    let outcome = world.use_item(0x09, 0);
    assert!(matches!(outcome, crate::items::ItemOutcome::CuredAll));
    assert!(!world.status_effects.is_afflicted(0));
}

#[test]
fn fold_battle_event_clamps_to_zero_hp() {
    use legaia_art::power::PowerByte;
    use legaia_art::queue::{ActionConstant, Character};
    use legaia_art::record::EnemyEffect;
    use legaia_engine_vm::battle_action::ArtStrikeInfo;

    let mut world = World::new();
    world.party_count = 4;
    world.actors[3].active = true;
    world.actors[3].battle.hp = 30;
    world.actors[3].battle.max_hp = 30;
    world.set_battle_attack(0, 64);
    world.set_battle_defense(3, 0);

    let info = ArtStrikeInfo {
        strike_index: 0,
        anim_byte: 0x10,
        actor_slot: 0,
        target_slot: 3,
        character: Character::Vahn,
        art: ActionConstant::Art1B,
        power: Some(PowerByte::from_byte(0x1A)), // huge damage vs 30 HP
        dmg_timing: None,
        enemy_effect: EnemyEffect::None,
        hit_cue: None,
    };
    let mut host = BattleHostImpl { world: &mut world };
    host.apply_art_strike(info);
    let events = world.drain_battle_events();
    for e in &events {
        world.fold_battle_event(e);
    }
    // saturating_sub clamps to 0 instead of wrapping.
    assert_eq!(world.actors[3].battle.hp, 0);
}

#[test]
fn fold_battle_event_pushes_status_into_tracker() {
    use legaia_art::power::PowerByte;
    use legaia_art::queue::{ActionConstant, Character};
    use legaia_art::record::EnemyEffect;
    use legaia_engine_vm::battle_action::ArtStrikeInfo;
    use legaia_engine_vm::status_effects::StatusKind;

    let mut world = World::new();
    world.party_count = 4;
    world.actors[3].active = true;
    world.actors[3].battle.hp = 100;
    world.actors[3].battle.max_hp = 100;
    world.set_battle_attack(0, 64);
    world.set_battle_defense(3, 10);
    let info = ArtStrikeInfo {
        strike_index: 0,
        anim_byte: 0x10,
        actor_slot: 0,
        target_slot: 3,
        character: Character::Vahn,
        art: ActionConstant::Art1B,
        power: Some(PowerByte::from_byte(0x1A)),
        dmg_timing: None,
        enemy_effect: EnemyEffect::Toxic,
        hit_cue: None,
    };
    let mut host = BattleHostImpl { world: &mut world };
    host.apply_art_strike(info);
    let events = world.drain_battle_events();
    for e in &events {
        world.fold_battle_event(e);
    }
    assert!(world.status_effects.has(3, StatusKind::Toxic));
}

#[test]
fn tick_status_effects_drains_hp() {
    use legaia_engine_vm::status_effects::StatusKind;
    let mut world = World::new();
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.max_hp = 160;
    world.status_effects.apply(0, StatusKind::Toxic);
    world.tick_status_effects();
    // Toxic drains max_hp / 16 = 160 / 16 = 10 (FUN_801E752C).
    assert_eq!(world.actors[0].battle.hp, 90);
}

#[test]
fn reset_party_ap_refills_all_three_gauges() {
    let mut world = World::new();
    for g in world.ap_gauges.iter_mut() {
        g.try_spend(3);
    }
    world.reset_party_ap();
    for g in world.ap_gauges.iter() {
        assert_eq!(g.current_ap, g.base_ap);
        assert!(!g.spirit_charged);
    }
}

#[test]
fn item_catalog_setter_replaces() {
    let mut world = World::new();
    assert!(world.item_catalog.is_empty());
    world.set_item_catalog(full_test_catalog());
    assert!(!world.item_catalog.is_empty());
}
