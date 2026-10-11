use super::*;

fn region(x0: u8, z0: u8, x1: u8, z1: u8, rate: u8, base: u8, count: u8) -> EncounterRegion {
    EncounterRegion {
        tile_x_min: x0,
        tile_z_min: z0,
        tile_x_max: x1,
        tile_z_max: z1,
        rate_increment: rate,
        formation_base: base,
        formation_count: count,
        setup: RegionSetupBytes::default(),
    }
}

#[test]
fn the_setup_half_on_the_short_layout_only_stamps_the_variant_and_blocks() {
    let s = region_battle_setup(&RegionSetupBytes {
        byte5: 0,
        byte8: 0xE3,
        ext: None,
    });
    assert_eq!(s.stage_variant, 3);
    assert_eq!(s.keep_backdrop_object_1, None);
    assert!(s.door_of_light_blocked && s.door_of_wind_blocked);
    assert_eq!(s.world_map_return, None);
}

#[test]
fn the_setup_half_on_the_long_layout() {
    let s = region_battle_setup(&RegionSetupBytes {
        byte5: 0x81,
        byte8: 0xA1,
        ext: Some([0x55, 0x12, 0x34]),
    });
    assert_eq!(s.stage_variant, 1);
    assert_eq!(s.keep_backdrop_object_1, Some(true));
    assert!(!s.door_of_light_blocked, "bit 7 opens the Door of Light");
    assert!(
        s.door_of_wind_blocked,
        "bit 6 clear keeps Door of Wind shut"
    );
    assert_eq!(
        s.world_map_return,
        Some(WorldMapReturn {
            map_word: 0x0155,
            tile_x: 0x12,
            tile_z: 0x34,
        })
    );
}

#[test]
fn world_to_tile_is_arithmetic_shift() {
    assert_eq!(RegionEncounterTable::world_to_tile(0), 0);
    assert_eq!(RegionEncounterTable::world_to_tile(127), 0);
    assert_eq!(RegionEncounterTable::world_to_tile(128), 1);
    assert_eq!(RegionEncounterTable::world_to_tile(256), 2);
    // Negative floors toward -inf (arithmetic shift), matching `sra`.
    assert_eq!(RegionEncounterTable::world_to_tile(-1), -1);
    assert_eq!(RegionEncounterTable::world_to_tile(-128), -1);
    assert_eq!(RegionEncounterTable::world_to_tile(-129), -2);
}

#[test]
fn region_at_tile_first_match_wins() {
    let mut t = RegionEncounterTable::new("s");
    t.regions.push(region(0, 0, 4, 4, 8, 0, 2));
    t.regions.push(region(2, 2, 6, 6, 16, 2, 3)); // overlaps the first
    // (3,3) is in both; the walk takes the first.
    let r = t.region_at_tile(3, 3).unwrap();
    assert_eq!(r.rate_increment, 8);
    // (5,5) is only in the second.
    assert_eq!(t.region_at_tile(5, 5).unwrap().rate_increment, 16);
    // (9,9) is in neither.
    assert!(t.region_at_tile(9, 9).is_none());
}

/// A MAN whose section 0 carries `conditions` over `regions`, in the
/// retail layout. `regions` are `(rate, x1, z1)` whole-corner boxes from
/// the origin so the caller can tell them apart by rate alone.
fn man_with_groups(conditions: &[(u16, u16)], regions: &[(u8, u8, u8)]) -> Vec<u8> {
    let mut body = vec![8u8, 4, 12, 1];
    body.extend_from_slice(&[0, 0, 0, 1, 4, 0, 0, 0]); // formation 0
    body.push(conditions.len() as u8);
    for (flag, n) in conditions {
        body.extend_from_slice(&flag.to_le_bytes());
        body.extend_from_slice(&n.to_le_bytes());
    }
    body.push(regions.len() as u8);
    for &(rate, x1, z1) in regions {
        body.extend_from_slice(&[0, 0, x1, z1, rate, 0, 0, 1, 0, 0, 0, 0]);
    }

    let mut buf = vec![0u8; 0x2B];
    let ln = body.len() as u32;
    buf.extend_from_slice(&[(ln & 0xFF) as u8, (ln >> 8) as u8, (ln >> 16) as u8]);
    buf.extend_from_slice(&body);
    for _ in 0..5 {
        buf.extend_from_slice(&[0, 0, 0]);
    }
    buf
}

#[test]
fn man_table_carries_the_condition_partition() {
    let man = man_with_groups(
        &[(0x0141, 2), (0xFFFF, 1)],
        &[(0, 128, 128), (24, 40, 40), (18, 60, 60)],
    );
    let t = region_encounter_table_from_man("s", &man).expect("table");
    assert_eq!(t.regions.len(), 3, "every variant is preserved");
    assert_eq!(
        t.groups,
        vec![
            RegionGroup {
                flag_id: 0x0141,
                start: 0,
                len: 2
            },
            RegionGroup {
                flag_id: DEFAULT_GROUP_FLAG,
                start: 2,
                len: 1
            },
        ]
    );
    // Built resolved for a cleared flag bank: the default tail, not
    // group 0 - so the leading whole-map rate-0 row is invisible.
    assert_eq!(t.active_regions().len(), 1);
    assert_eq!(t.active_regions()[0].rate_increment, 18);
    assert!(t.any_rollable());
}

#[test]
fn selecting_a_gated_group_swaps_the_live_region_set() {
    let man = man_with_groups(
        &[(0x0141, 2), (0xFFFF, 1)],
        &[(0, 128, 128), (24, 40, 40), (18, 60, 60)],
    );
    let mut t = region_encounter_table_from_man("s", &man).expect("table");
    // Default state: tile (10,10) rolls the trailing group's rate-18 row.
    assert_eq!(t.region_at_tile(10, 10).unwrap().rate_increment, 18);
    // Flag 0x0141 set: the gated group wins, and *its* first row is the
    // whole-map rate-0 one, which shadows the rate-24 row behind it.
    t.select_group(|f| f == 0x0141);
    assert_eq!(t.region_at_tile(10, 10).unwrap().rate_increment, 0);
    assert!(!t.any_rollable(), "the group's rate-0 row shadows the rest");
    assert_eq!(t.active_group().unwrap().flag_id, 0x0141);
}

#[test]
fn a_walk_with_no_match_leaves_no_active_regions() {
    // No default sentinel: retail returns without rolling rather than
    // falling back to region 0.
    let man = man_with_groups(&[(0x0141, 1)], &[(24, 40, 40)]);
    let mut t = region_encounter_table_from_man("s", &man).expect("table");
    assert!(t.active_regions().is_empty());
    assert!(t.region_at_tile(10, 10).is_none());
    assert!(!t.any_rollable());
    t.select_group(|f| f == 0x0141);
    assert_eq!(t.active_regions().len(), 1);
    assert!(t.any_rollable());
}

#[test]
fn a_hand_built_table_keeps_every_region_active() {
    // Synthetic tables (tests, synthetic worlds) never ran a condition
    // walk; they must not read as "all groups dead".
    let mut t = RegionEncounterTable::new("s");
    t.regions.push(region(0, 0, 8, 8, 16, 0, 2));
    assert_eq!(t.active_regions().len(), 1);
    assert!(t.region_at_tile(1, 1).is_some());
    t.select_group(|_| false);
    assert_eq!(t.active_regions().len(), 1);
}

#[test]
fn rate_setting_scale_matches_disasm() {
    assert_eq!(EncounterRateSetting::Off.scale(10), 0);
    assert_eq!(EncounterRateSetting::Normal.scale(10), 10); // as-is
    assert_eq!(EncounterRateSetting::High.scale(10), 40); // << 2
    assert_eq!(EncounterRateSetting::Low.scale(10), 2); // >> 2
    assert_eq!(
        EncounterRateSetting::from_u8(1),
        EncounterRateSetting::Normal
    );
    assert_eq!(EncounterRateSetting::from_u8(2), EncounterRateSetting::High);
    assert_eq!(
        EncounterRateSetting::from_u8(99),
        EncounterRateSetting::Normal
    );
    // Three independent retail save states carry `1`, so a fresh tracker
    // must run at the pass-through rate and not the `<< 2` one.
    assert_eq!(EncounterRateSetting::default().as_u8(), 1);
}

#[test]
fn no_region_no_trigger_and_no_rng() {
    let mut t = RegionEncounterTable::new("s");
    t.regions.push(region(0, 0, 1, 1, 255, 0, 2));
    let mut tracker = RegionEncounterTracker::new(t);
    let mut draws = 0u32;
    // World (1000, 1000) -> tile (7, 7), outside the region.
    for _ in 0..100 {
        let r = tracker.on_step(1000, 1000, || {
            draws += 1;
            0
        });
        assert!(r.is_none());
    }
    assert_eq!(draws, 0, "no RNG consumed off-region");
}

#[test]
fn counter_depletes_then_triggers() {
    let mut t = RegionEncounterTable::new("s");
    // One region covering tile (0,0), big rate so it depletes fast.
    t.regions.push(region(0, 0, 1, 1, 255, 5, 3));
    let mut tracker = RegionEncounterTracker::new(t);
    tracker.set_setting(EncounterRateSetting::High); // 255<<2 = 1020/step
    // Counter starts at 0x3ce (974); 974 - 1020 <= 0 -> first step triggers.
    let mut seq = [7u32, 100, 50].into_iter().cycle();
    let roll = tracker.on_step(0, 0, || seq.next().unwrap());
    let roll = roll.expect("triggers on the first step");
    // formation_id = base(5) + 7 % 3 = 5 + 1 = 6.
    assert_eq!(roll.formation_id, 6);
    // Counter reset to 0x3ce + 100%487 - 50%487 = 974 + 100 - 50 = 1024.
    assert_eq!(tracker.counter(), 974 + 100 - 50);
}

#[test]
fn off_setting_never_triggers() {
    let mut t = RegionEncounterTable::new("s");
    t.regions.push(region(0, 0, 8, 8, 255, 0, 4));
    let mut tracker = RegionEncounterTracker::new(t);
    tracker.set_setting(EncounterRateSetting::Off);
    for _ in 0..10_000 {
        assert!(tracker.on_step(64, 64, || 0).is_none());
    }
}

#[test]
fn only_a_one_tile_move_is_a_step() {
    // Same tile: no step. One tile on either or both axes: a step.
    assert!(!is_region_step((10, 10), (10, 10)));
    assert!(is_region_step((10, 10), (11, 10)));
    assert!(is_region_step((10, 10), (9, 11)));
    // Two or more on either axis (a script seat, a warp landing): the
    // reader's `slti 0x2` exits without reading a region.
    assert!(!is_region_step((10, 10), (12, 10)));
    assert!(!is_region_step((32, 93), (25, 31)));
}

#[test]
fn suppression_blocks_trigger() {
    let mut t = RegionEncounterTable::new("s");
    t.regions.push(region(0, 0, 8, 8, 255, 0, 4));
    let mut tracker = RegionEncounterTracker::new(t);
    tracker.set_setting(EncounterRateSetting::High);
    tracker.suppress();
    for _ in 0..10_000 {
        assert!(tracker.on_step(64, 64, || 0).is_none());
    }
    tracker.clear_suppression();
    // Now a step in-region eventually fires.
    let mut fired = false;
    for _ in 0..10_000 {
        if tracker.on_step(64, 64, || 7).is_some() {
            fired = true;
            break;
        }
    }
    assert!(fired);
}

#[test]
fn anti_repeat_advances_on_duplicate_pick() {
    let mut t = RegionEncounterTable::new("s");
    // base 10, count 4 -> ids 10..14. Big rate so every step triggers.
    t.regions.push(region(0, 0, 1, 1, 255, 10, 4));
    let mut tracker = RegionEncounterTracker::new(t);
    tracker.set_setting(EncounterRateSetting::High);
    // Force pick == 0 every time -> base 10. The reset draws don't matter
    // for the pick; keep them tiny so the counter stays <= 0 next step.
    // First trigger: 10. Second: pick 0 -> 10 == last -> bump to 11.
    let mut draws = [0u32, 0, 1000].into_iter().cycle();
    let first = tracker.on_step(0, 0, || draws.next().unwrap()).unwrap();
    assert_eq!(first.formation_id, 10);
    // Counter is now 974 + 0 - (1000%487=26) ... keep it triggering: set
    // counter low directly.
    tracker.counter = -1;
    let mut draws2 = [0u32, 0, 0].into_iter().cycle();
    let second = tracker.on_step(0, 0, || draws2.next().unwrap()).unwrap();
    assert_eq!(second.formation_id, 11, "duplicate pick advanced by one");
}

#[test]
fn counter_reroll_is_first_draw_minus_second_plus_974() {
    // Retail order: the first `rand()` is the added term.
    let mut draws = [1000u32, 3].into_iter();
    // 1000 % 487 = 26; 974 + 26 - 3 = 997.
    assert_eq!(encounter_counter_reroll(|| draws.next().unwrap()), 997);
    // Extremes of the triangular range.
    let mut hi = [486u32, 0].into_iter();
    assert_eq!(encounter_counter_reroll(|| hi.next().unwrap()), 1460);
    let mut lo = [0u32, 486].into_iter();
    assert_eq!(encounter_counter_reroll(|| lo.next().unwrap()), 488);
}

#[test]
fn scene_entry_top_up_only_below_487() {
    // At or above 487 the counter is carried unchanged and no RNG is drawn.
    let mut drew = false;
    assert_eq!(
        encounter_counter_scene_entry_top_up(487, || {
            drew = true;
            0
        }),
        487
    );
    assert!(!drew);
    // Below: add half a reroll (974 + 100 - 0 = 1074 -> +537).
    let mut d = [100u32, 0].into_iter();
    assert_eq!(
        encounter_counter_scene_entry_top_up(-5, || d.next().unwrap()),
        -5 + 537
    );
}
