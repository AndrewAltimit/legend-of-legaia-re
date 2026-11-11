use super::*;

// ---------------------------------------------------------------------------
// Shiny Seru (rare +35% capturable variant): battle-load stat boost, capture
// marking, persistence, and the +35% damage bonus on cast.
// ---------------------------------------------------------------------------

#[test]
fn shiny_roll_boosts_only_the_capturable_enemy() {
    use legaia_engine_battle::monster_catalog::{FormationDef, FormationSlot};
    let mut world = capture_world(1);
    world.set_shiny_chance_pct(100); // force a shiny this battle
    // Killer Bee (id 7) is capturable (Seru 1); Wolf (id 9) is not.
    let formation = FormationDef::new(1, vec![FormationSlot::new(7), FormationSlot::new(9)]);
    // Battle init may open with either species in front.
    let seated = world.enter_battle_from_formation(&formation);
    let bee = 1 + seated.slots.iter().position(|s| s.monster_id == 7).unwrap();
    let wolf = 3 - bee;

    // Killer Bee: hp 25 -> 33, attack 9 -> 12.
    assert!(
        world.seru.shiny_enemy_slots.contains(&(bee as u8)),
        "the capturable enemy is flagged shiny"
    );
    assert_eq!(world.actors[bee].battle.max_hp, 33, "shiny HP +35%");
    assert_eq!(world.battle.attack[bee], 12, "shiny ATK +35%");
    // Wolf (not capturable) is never chosen.
    assert!(!world.seru.shiny_enemy_slots.contains(&(wolf as u8)));
}

#[test]
fn shiny_disabled_when_chance_is_zero() {
    use legaia_engine_battle::monster_catalog::{FormationDef, FormationSlot};
    let mut world = capture_world(1);
    world.set_shiny_chance_pct(0);
    let formation = FormationDef::new(1, vec![FormationSlot::new(7)]);
    world.enter_battle_from_formation(&formation);
    assert!(
        world.seru.shiny_enemy_slots.is_empty(),
        "no shiny when disabled"
    );
    assert_eq!(world.actors[1].battle.max_hp, 25, "stats unmodified");
}

#[test]
fn non_shiny_capture_does_not_flag_shiny() {
    let mut world = capture_world(1);
    world.seru.battle_captures = vec![7]; // captured normally (not in shiny_captures)
    world.finish_battle();
    assert!(world.seru.log.has_learned(0, 1));
    assert!(
        !world.seru.log.is_shiny(0, 0x20),
        "a normal capture is never shiny"
    );
}

#[test]
fn shiny_spell_deals_35_percent_more_damage() {
    // Plain cast.
    let mut plain = summon_xp_world(4000, 4000);
    let def = gimard_spell_def();
    let before = plain.actors[1].battle.hp;
    plain.cast_spell_on_slots(0, &def, &[1]);
    let plain_dmg = (before - plain.actors[1].battle.hp) as u32;
    assert!(plain_dmg > 0);

    // Shiny cast: same world setup, spell 0x20-> here 0x81 flagged shiny.
    let mut shiny = summon_xp_world(4000, 4000);
    shiny.seru.log.mark_shiny(0, 0x81);
    let before_s = shiny.actors[1].battle.hp;
    shiny.cast_spell_on_slots(0, &def, &[1]);
    let shiny_dmg = (before_s - shiny.actors[1].battle.hp) as u32;

    let expected = (plain_dmg * 135 / 100).min(9999);
    assert_eq!(
        shiny_dmg, expected,
        "shiny cast deals +35% (plain {plain_dmg} -> shiny {shiny_dmg})"
    );
}

/// Battle init's species-order rebuild (`FUN_80055B6C`,
/// `0x80055C7C..0x80055D2C`): a formation of two species draws one `rand()`
/// and opens with either species in front; one species draws nothing.
#[test]
fn a_two_species_formation_opens_either_way_round_and_draws_once() {
    use legaia_engine_battle::monster_catalog::{FormationDef, FormationSlot};
    let ids = |f: &FormationDef| f.slots.iter().map(|s| s.monster_id).collect::<Vec<_>>();
    let mixed = FormationDef::new(
        1,
        vec![
            FormationSlot::new(7),
            FormationSlot::new(9),
            FormationSlot::new(7),
        ],
    );
    let (mut kept, mut swapped) = (0, 0);
    for seed in 0..64u32 {
        let mut world = capture_world(1);
        world.rng_state = seed.wrapping_mul(0x9E37_79B9) | 1;
        let mut twin = world.rng_state;
        twin = legaia_engine_system::rand::world_lcg_step(twin);
        let odd = legaia_engine_system::rand::bios_rand_shape(twin) & 1 != 0;
        let seated = world.formation_in_species_order(&mixed);
        assert_eq!(world.rng_state, twin, "one draw");
        if odd {
            assert_eq!(ids(&seated), vec![9, 7, 7]);
            swapped += 1;
        } else {
            assert_eq!(ids(&seated), vec![7, 9, 7]);
            kept += 1;
        }
    }
    assert!(kept > 8 && swapped > 8, "{kept} {swapped}");

    let mut world = capture_world(1);
    let before = world.rng_state;
    let solo = FormationDef::new(1, vec![FormationSlot::new(7), FormationSlot::new(7)]);
    assert_eq!(ids(&world.formation_in_species_order(&solo)), vec![7, 7]);
    assert_eq!(world.rng_state, before, "one species draws nothing");
}
