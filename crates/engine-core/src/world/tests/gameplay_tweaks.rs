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
