use super::*;
use legaia_engine_battle::encounter::EncounterRoll;
use legaia_engine_battle::monster_catalog::{
    FormationDef, FormationSlot, MonsterCatalog, MonsterDef,
};
use legaia_engine_run_rules::rules::{
    CarrySet, Difficulty, Ironman, NgPlus, Randomizer, RunMode, RunRules,
};

// ---------------------------------------------------------------------------
// Run rules (`world::run_rules`): every reader is the identity under the
// retail rules, a preset moves exactly its knobs, New Game+ carries what it
// was told to, ironman marks the run on a wipe, the randomizer is a pure
// function of its seed, and the save block round-trips (and is absent under
// retail rules).
// ---------------------------------------------------------------------------

fn one_member_world() -> World {
    let mut w = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    w.actors[0].battle.hp = 100;
    w
}

fn slime_catalog() -> MonsterCatalog {
    let mut cat = MonsterCatalog::new();
    let mut def = MonsterDef::new(7, "Slime", 10, 5);
    def.exp = 40;
    def.gold = 100;
    cat.insert(def);
    cat
}

fn rules(d: Difficulty) -> RunRules {
    RunRules {
        difficulty: d,
        ..Default::default()
    }
}

#[test]
fn retail_rules_leave_every_reader_alone() {
    let mut w = one_member_world();
    w.toggles.exp_rate_permille = 1500;
    w.toggles.enemy_stat_permille = 2000;
    w.toggles.enemy_attack_permille = 3000;
    assert!(w.run_rules.rules.is_retail());
    assert_eq!(w.run_exp_permille(), 1500);
    assert_eq!(w.run_enemy_stat_permille(73), 2000);
    assert_eq!(w.run_enemy_attack_permille(), 3000);
    assert_eq!(w.run_scale_gold(123), 123);
    assert_eq!(w.run_scale_price(777), 777);
    assert_eq!(w.run_scale_heal(200), 200);
    assert_eq!(w.run_scale_ap(32), 32);
    assert!(!w.run_smart_monsters() && !w.run_no_escape() && w.run_battle_items_allowed());
    assert_eq!(w.run_encounter_rate_permille(), 1000);
    assert_eq!(
        w.run_rules_save_block(),
        None,
        "a retail run writes no block"
    );
    assert_eq!(w.run_rules_summary(), None);
    let roll = EncounterRoll {
        formation_id: 3,
        row_index: 3,
        roll_q8: 0,
    };
    assert_eq!(w.run_rules_substitute_roll(roll), roll);
    assert_eq!(w.run_rules_give_item(0x40), 0x40);
}

#[test]
fn difficulty_presets_apply_and_clear() {
    let payout = |d: Difficulty| {
        let mut w = one_member_world();
        w.run_rules.rules = rules(d);
        let f = FormationDef::new(1000, vec![FormationSlot::new(7)]);
        let r = w.apply_battle_loot(&f, &slime_catalog());
        (r.xp, r.gold)
    };
    let normal = payout(Difficulty::Normal);
    let easy = payout(Difficulty::Easy);
    let nightmare = payout(Difficulty::Nightmare);
    assert!(
        easy.0 > normal.0 && easy.1 > normal.1,
        "{easy:?} vs {normal:?}"
    );
    assert!(nightmare.0 < normal.0 && nightmare.1 < normal.1);

    let mut w = one_member_world();
    w.run_rules.rules = rules(Difficulty::Hard);
    assert_eq!(w.run_enemy_stat_permille(30), 1350);
    assert!(w.run_smart_monsters());
    assert_eq!(w.run_scale_price(1000), 1250);
    w.run_rules.rules = rules(Difficulty::Nightmare);
    assert!(w.run_no_escape());
    assert_eq!(w.run_scale_ap(32), 24);
    // A New Game with nothing staged clears the rules.
    w.begin_new_game();
    assert!(w.run_rules.rules.is_retail());
    assert_eq!(w.run_scale_price(1000), 1000);
}

#[test]
fn staged_rules_take_over_at_new_game_and_round_trip_the_save_block() {
    let mut w = one_member_world();
    let r = RunRules {
        mode: RunMode::Story,
        difficulty: Difficulty::Hard,
        ironman: Some(Ironman { hardcore: true }),
        randomizer: Some(Randomizer::all("legaia")),
        ng_plus: None,
    };
    w.stage_run(Some(crate::world::run_rules::StagedRun {
        rules: r.clone(),
        carry: None,
    }));
    w.begin_new_game_seeded(None, None);
    assert_eq!(w.run_rules.rules, r);
    assert_ne!(w.ironman_run_id(), 0, "an ironman run gets an id");
    assert!(
        !w.run_battle_items_allowed(),
        "hardcore refuses battle items"
    );
    let block = w
        .run_rules_save_block()
        .expect("a ruled run writes a block");
    let id = w.ironman_run_id();
    let mut back = one_member_world();
    back.run_rules_load_block(Some(&block));
    assert_eq!(back.run_rules.rules, r);
    assert_eq!(back.ironman_run_id(), id);
    back.run_rules_load_block(None);
    assert!(
        back.run_rules.rules.is_retail(),
        "a save without the block is retail"
    );
}

#[test]
fn ironman_marks_the_run_on_a_wipe_and_owes_autosaves() {
    let mut w = one_member_world();
    assert!(!w.run_rules_party_fell(), "no ironman, nothing to erase");
    assert_eq!(w.take_ironman_erase(), None);
    w.run_rules.rules.ironman = Some(Ironman::default());
    w.run_rules.run_id = 99;
    w.set_active_scene_label("town01");
    assert!(w.take_run_autosave());
    assert!(!w.take_run_autosave(), "taken once");
    assert!(w.run_rules_party_fell());
    assert_eq!(w.take_ironman_erase(), Some(99));
    assert_eq!(w.take_ironman_erase(), None);
}

#[test]
fn ng_plus_carries_the_chosen_parts() {
    // A "cleared" save: Vahn at level 40 with gear and a bag, rich.
    let mut src = one_member_world();
    let mut rec = legaia_save::CharacterRecord::zeroed();
    let mut h = rec.hp_mp_sp();
    h.hp_max = 2000;
    h.hp_cur = 2000;
    rec.set_hp_mp_sp(h);
    rec.raw[0x130] = 40;
    rec.raw[0x198] = 0x05; // a weapon
    src.party.roster.members = vec![rec];
    src.party.money = 50_000;
    let _ = src.party.inventory.add(0x77, 9);
    let snap = src.ng_plus_snapshot();

    let start = |carry: CarrySet| {
        let mut w = one_member_world();
        let mut vahn = legaia_save::CharacterRecord::zeroed();
        let mut h = vahn.hp_mp_sp();
        h.hp_max = 180;
        h.hp_cur = 180;
        vahn.set_hp_mp_sp(h);
        vahn.raw[0x130] = 1;
        w.stage_run(Some(crate::world::run_rules::StagedRun {
            rules: RunRules {
                ng_plus: Some(NgPlus { cycle: 1, carry }),
                ..Default::default()
            },
            carry: Some(snap.clone()),
        }));
        w.begin_new_game();
        w.party.roster.members = vec![vahn];
        w.finish_run_new_game();
        w
    };
    let all = start(CarrySet::default());
    assert_eq!(all.party.roster.members[0].level(), 40);
    assert_eq!(all.party.roster.members[0].equipment().slots[2], 0x05);
    assert_eq!(all.party.money, 50_000);
    assert_eq!(all.party.inventory.get(&0x77).copied(), Some(9));
    assert_eq!(all.run_rules.rules.ng_plus_cycle(), 1);
    let none = start(CarrySet {
        levels: false,
        equipment: false,
        items: false,
        seru: false,
        arts: false,
        gold: false,
        cards: false,
    });
    assert_eq!(none.party.roster.members[0].level(), 1);
    assert_eq!(none.party.roster.members[0].equipment().slots[2], 0);
    assert_eq!(none.party.money, NEW_GAME_STARTING_GOLD);
    assert_eq!(none.party.inventory.get(&0x77).copied(), None);
    // New Game+ scales enemies up, bosses more.
    let mut w = start(CarrySet::default());
    w.party.level_up_tracker.level[0] = 40;
    assert!(w.run_enemy_stat_permille(73) > w.run_enemy_stat_permille(30));
    assert!(w.run_enemy_stat_permille(30) > 1000);
    assert_eq!(w.run_enemy_stat_permille(79), 1000, "Tetsu is pinned");
}

#[test]
fn the_randomizer_is_a_function_of_its_seed() {
    let world_with = |seed: &str| {
        let mut w = one_member_world();
        w.run_rules.rules.randomizer = Some(Randomizer::all(seed));
        w.active_scene_label = "map01".into();
        let mut cat = MonsterCatalog::new();
        for id in [30u16, 31, 32, 33, 10] {
            let mut d = MonsterDef::new(id, "M", 100, 10);
            d.drop_item = Some(0x77);
            cat.insert(d);
        }
        w.tables.monster_catalog = cat;
        w.tables.formation_table.insert(FormationDef::new(
            1,
            vec![FormationSlot::new(30), FormationSlot::new(30)],
        ));
        w.tables.formation_table.insert(FormationDef::new(
            2,
            vec![
                FormationSlot::new(31),
                FormationSlot::new(32),
                FormationSlot::new(33),
            ],
        ));
        // The Gimard fight: never redrawn, never a donor.
        w.tables
            .formation_table
            .insert(FormationDef::new(3, vec![FormationSlot::new(10)]));
        w
    };
    let draw = |seed: &str| {
        let mut w = world_with(seed);
        let mut seats = Vec::new();
        for f in [1u16, 2, 3] {
            let r = w.run_rules_substitute_roll(EncounterRoll {
                formation_id: f,
                row_index: f as usize,
                roll_q8: 0,
            });
            seats.push(
                w.tables
                    .formation_table
                    .formation(r.formation_id)
                    .unwrap()
                    .slots
                    .iter()
                    .map(|s| s.monster_id)
                    .collect::<Vec<_>>(),
            );
        }
        let f = FormationDef::new(1, vec![FormationSlot::new(30)]);
        w.run_rules_prepare_battle(&f);
        let hp = w.tables.monster_catalog.get(30).unwrap().hp;
        w.run_rules_end_battle();
        assert_eq!(
            w.tables.monster_catalog.get(30).unwrap().hp,
            100,
            "restored"
        );
        (seats, hp)
    };
    let a = draw("alpha");
    assert_eq!(a, draw("alpha"), "same seed, same run");
    assert_eq!(
        a.0[2],
        vec![10],
        "the protected boss formation is untouched"
    );
    for seats in &a.0[..2] {
        assert!(seats.iter().all(|id| *id != 10), "Gimard never donates");
    }
    assert!((80..=125).contains(&a.1), "stats stay in the band: {}", a.1);
    let others: Vec<_> = ["b", "c", "d", "e", "f"].iter().map(|s| draw(s)).collect();
    assert!(
        others.iter().any(|o| *o != a),
        "another seed is another run"
    );
}
