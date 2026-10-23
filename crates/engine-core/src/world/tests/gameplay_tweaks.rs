use super::*;
use legaia_engine_battle::monster_catalog::{FormationDef, FormationSlot};

// ---------------------------------------------------------------------------
// The port's gameplay tweaks that graduate patcher features into engine
// toggles (`WorldToggles::{shiny_seru, flee_exp, exp_rate_permille,
// fortress_encounter_fix, jewel_fix}`): each changes behaviour when on, and
// retail mode (`pin_retail`) is the unmodified game. The jewel fix's test
// sits with the bypass-wrapper tests (`casting/capture_bypass_tests.rs`).
// ---------------------------------------------------------------------------

/// Retail has no shiny roll. With the toggle off battle entry takes no
/// `rand()` for it, so the stream after entry is the one a zero-chance (no
/// roll) entry leaves; with it on the roll's draw moves the stream.
#[test]
fn shiny_toggle_off_takes_no_battle_entry_draw() {
    let formation = FormationDef::new(1, vec![FormationSlot::new(7), FormationSlot::new(9)]);
    let entry_rng = |shiny: bool, pct: u8| {
        let mut world = capture_world(1);
        world.toggles.shiny_seru = shiny;
        world.set_shiny_chance_pct(pct);
        world.rng_state = 0x1234_5678;
        world.enter_battle_from_formation(&formation);
        (world.rng_state, world.seru.shiny_enemy_slots.len())
    };
    let (no_roll, _) = entry_rng(true, 0);
    let (retail, retail_shiny) = entry_rng(false, 100);
    assert_eq!(retail, no_roll, "retail mode draws nothing for the roll");
    assert_eq!(retail_shiny, 0, "and no enemy spawns shiny");
    let (port, port_shiny) = entry_rng(true, 100);
    assert_ne!(port, no_roll, "the port's roll draws from the stream");
    assert_eq!(port_shiny, 1);
    let mut t = WorldToggles::new();
    t.pin_retail();
    assert!(!t.shiny_seru);
}

fn flee_world(flee: bool, rate: u16) -> World {
    let mut world = capture_world(2);
    world.toggles.flee_exp = flee;
    world.toggles.exp_rate_permille = rate;
    world.actors[0].battle.hp = 10;
    world.actors[1].battle.hp = 10;
    let mut cat = std::mem::take(&mut world.tables.monster_catalog);
    for (id, exp) in [(7u16, 400u16), (9, 600)] {
        if let Some(mut def) = cat.get(id).cloned() {
            def.exp = exp;
            cat.insert(def);
        }
    }
    world.tables.monster_catalog = cat;
    world.battle.active_formation = Some(FormationDef::new(
        1,
        vec![FormationSlot::new(7), FormationSlot::new(9)],
    ));
    world.battle.end = Some(legaia_engine_battle_vm::battle_action::BattleEndCause::Escaped);
    world
}

/// Run-away EXP: an escape banks 5% of the fled formation's EXP into every
/// member's cumulative XP, scaled by the EXP rate; off, an escape pays
/// nothing.
#[test]
fn flee_exp_banks_a_share_of_the_fled_formation() {
    let mut world = flee_world(true, EXP_RATE_RETAIL);
    world.begin_battle_end_sequence();
    // (400 + 600) * 5 / 100 = 50 per member.
    assert_eq!(world.party.level_up_tracker.xp[0], 50);
    assert_eq!(world.party.level_up_tracker.xp[1], 50);
    assert_eq!(world.party.roster.members[0].cumulative_xp(), 50);

    let mut doubled = flee_world(true, 2000);
    doubled.begin_battle_end_sequence();
    assert_eq!(doubled.party.level_up_tracker.xp[0], 100);

    let mut retail = flee_world(false, EXP_RATE_RETAIL);
    retail.begin_battle_end_sequence();
    assert!(retail.party.level_up_tracker.xp.iter().all(|&x| x == 0));
    assert_eq!(retail.party.roster.members[0].cumulative_xp(), 0);
    assert!(retail.battle.victory.is_some(), "the escape still ends");
}

/// The EXP rate scales each record's reward before the victory sum; at
/// `1000` (retail) the payout is the retail one.
#[test]
fn exp_rate_scales_the_victory_payout() {
    use legaia_engine_battle::monster_catalog::{MonsterCatalog, MonsterDef};
    let payout = |rate: u16| {
        let mut cat = MonsterCatalog::new();
        let mut def = MonsterDef::new(7, "Slime", 10, 5);
        def.exp = 40;
        cat.insert(def);
        let formation = FormationDef::new(1000, vec![FormationSlot::new(7)]);
        let mut world = World {
            party: legaia_engine_menus::party_state::PartyState {
                party_count: 1,
                ..Default::default()
            },
            ..World::default()
        };
        world.toggles.exp_rate_permille = rate;
        world.actors[0].battle.hp = 100;
        world.apply_battle_loot(&formation, &cat).xp
    };
    assert_eq!(payout(EXP_RATE_RETAIL), 40);
    assert_eq!(payout(2000), 80);
    assert_eq!(payout(500), 20);
}

/// The Fortress fix: in `deroa` / `chitei2` the condition walk reads the
/// castle's stale flag `0x15` as clear, so it falls past the castle's copied
/// "no battles" group; off (retail), a save carrying the flag finds that
/// group. Every other scene - the castle itself included - reads the flag.
#[test]
fn fortress_fix_skips_the_stale_castle_group() {
    use legaia_engine_battle::region_encounter::{
        DEFAULT_GROUP_FLAG, RegionEncounterTable, RegionGroup,
    };
    let mut table = RegionEncounterTable::new("deroa");
    table.groups = vec![
        RegionGroup {
            flag_id: FORTRESS_STALE_FLAG,
            start: 0,
            len: 1,
        },
        RegionGroup {
            flag_id: DEFAULT_GROUP_FLAG,
            start: 1,
            len: 2,
        },
    ];
    let mut world = World::default();
    world.system_flag_set(FORTRESS_STALE_FLAG);

    world.active_scene_label = "deroa".into();
    world.toggles.fortress_encounter_fix = true;
    table.select_group(|f| world.encounter_condition_flag_test(f));
    assert_eq!(
        table.active_group().map(|g| g.flag_id),
        Some(DEFAULT_GROUP_FLAG)
    );

    world.toggles.fortress_encounter_fix = false;
    table.select_group(|f| world.encounter_condition_flag_test(f));
    assert_eq!(
        table.active_group().map(|g| g.flag_id),
        Some(FORTRESS_STALE_FLAG),
        "retail: the stale flag silences the Fortress"
    );

    world.toggles.fortress_encounter_fix = true;
    world.active_scene_label = "taiku".into();
    assert!(
        world.encounter_condition_flag_test(FORTRESS_STALE_FLAG),
        "the castle keeps its last-minute quiet window"
    );
    world.active_scene_label = "chitei2".into();
    assert!(!world.encounter_condition_flag_test(FORTRESS_STALE_FLAG));
}

/// The HP-readout guard: a party readout left off live HP with an empty
/// accumulator - the state that parks state `0x51` forever - is re-seeded
/// and counts to live HP. Off (retail mode), the absorbing state stays.
#[test]
fn hp_readout_guard_releases_the_absorbing_readout() {
    let readout_after = |guard: bool| {
        let mut world = World {
            party: legaia_engine_menus::party_state::PartyState {
                party_count: 1,
                ..Default::default()
            },
            ..World::default()
        };
        world.toggles.hp_readout_guard = guard;
        let a = &mut world.actors[0].battle;
        a.max_hp = 500;
        a.hp = 400;
        a.hp_display = Some(0);
        a.hp_bar_pending = 0;
        for _ in 0..128 {
            world.tick_battle_hp_bars();
        }
        let a = &world.actors[0].battle;
        (a.hp_display, a.hp_bar_pending)
    };
    assert_eq!(
        readout_after(false),
        (Some(0), 0),
        "retail: parked for good"
    );
    assert_eq!(readout_after(true), (Some(400), 0), "guarded: settles");
    let mut t = WorldToggles::new();
    assert!(t.hp_readout_guard);
    t.pin_retail();
    assert!(!t.hp_readout_guard);
}

/// Enemy-damage AP (`WorldToggles::damage_ap`): a hit taking a quarter of
/// max HP charges 25 AP at retail's x100, 50 at x200, and drains at a
/// negative scale.
#[test]
fn damage_ap_scales_the_hit_charge() {
    let after = |scale: i16, start: u16| {
        let mut world = World {
            party: legaia_engine_menus::party_state::PartyState {
                party_count: 1,
                ..Default::default()
            },
            ..World::default()
        };
        world.toggles.damage_ap = scale;
        world.actors[0].battle.max_hp = 400;
        world.actors[0].battle.hp = 400;
        world.actors[0].battle.spirit_gauge = start;
        world.accrue_spirit_gauge(0, 100);
        world.actors[0].battle.spirit_gauge
    };
    assert_eq!(after(DAMAGE_AP_RETAIL, 0), 25);
    assert_eq!(after(200, 0), 50);
    assert_eq!(after(0, 10), 10);
    assert_eq!(after(-100, 30), 5);
}

/// Enemy attack count (`WorldToggles::enemy_attack_permille`): at 2x every
/// affordable price halves, so the same AGL buys about twice the strikes;
/// retail's 1x leaves the stream as it was. Tetsu is pinned.
#[test]
fn enemy_attack_count_scales_the_strike_budget() {
    use legaia_engine_battle::monster_catalog::{MonsterCatalog, MonsterDef};
    let budget = |permille: u16, id: u16| {
        let mut world = World::default();
        let mut def = MonsterDef::new(id, "Brute", 100, 10);
        def.agl = 88;
        def.action_costs = vec![44];
        let mut cat = MonsterCatalog::new();
        cat.insert(def);
        world.tables.monster_catalog = cat;
        world.toggles.enemy_attack_permille = permille;
        world.actors[3].battle_monster_id = Some(id);
        world.rng_state = 0x1234_5678;
        world.arm_monster_strike_budget(3);
        world.battle.monster_strike_budget
    };
    assert_eq!(budget(SCALE_RETAIL, 7), 2, "88 AGL / 44 = two strikes");
    assert_eq!(budget(2000, 7), 4, "the price halves: four strikes");
    assert_eq!(budget(500, 7), 1, "0.5x: one strike minimum");
    assert_eq!(budget(2000, SCALE_PINNED_MONSTER_ID), 2, "Tetsu is pinned");
}

/// Enemy stat scale (`WorldToggles::enemy_stat_permille`): a formation's
/// monster enters with its HP / ATK scaled; retail's 1x is the record.
#[test]
fn enemy_stat_scale_scales_the_seated_monster() {
    let seated = |permille: u16| {
        let mut world = capture_world(1);
        world.set_shiny_chance_pct(0);
        world.toggles.enemy_stat_permille = permille;
        let formation = FormationDef::new(1, vec![FormationSlot::new(9)]);
        world.enter_battle_from_formation(&formation);
        (world.actors[1].battle.max_hp, world.battle.attack[1])
    };
    // Wolf: 35 HP, 12 ATK.
    assert_eq!(seated(SCALE_RETAIL), (35, 12));
    assert_eq!(seated(2000), (70, 24));
}

/// The bonus equipment drop (`WorldToggles::bonus_drop`): with the toggle
/// on, some battle among many grants one id from the pool on top of the
/// normal drop; off, nothing is drawn and the stream is retail's.
#[test]
fn bonus_drop_grants_equipment_and_retail_draws_nothing() {
    use legaia_engine_battle::monster_catalog::{MonsterCatalog, MonsterDef};
    let run = |on: bool| {
        let mut cat = MonsterCatalog::new();
        cat.insert(MonsterDef::new(7, "Slime", 10, 5));
        let formation = FormationDef::new(1, vec![FormationSlot::new(7)]);
        let mut world = World {
            party: legaia_engine_menus::party_state::PartyState {
                party_count: 1,
                ..Default::default()
            },
            ..World::default()
        };
        world.tables.bonus_drop_pool = Some(vec![0x30, 0x31]);
        world.toggles.bonus_drop = on;
        world.actors[0].battle.hp = 100;
        world.rng_state = 0xC0FF_EE01;
        for _ in 0..400 {
            world.apply_battle_loot(&formation, &cat);
        }
        let granted: u32 = [0x30u8, 0x31]
            .iter()
            .map(|id| u32::from(world.party.inventory.get(id).copied().unwrap_or(0)))
            .sum();
        (granted, world.rng_state)
    };
    let (on_granted, on_rng) = run(true);
    assert!(on_granted > 0, "400 battles at 5% grant some equipment");
    let (off_granted, off_rng) = run(false);
    assert_eq!(off_granted, 0);
    assert_ne!(on_rng, off_rng, "the roll draws only when on");
    // No executable installed: an empty pool, and nothing to roll.
    assert!(World::default().bonus_drop_pool().is_empty());
}
