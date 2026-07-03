use super::*;

// ---------------------------------------------------------------------------
// Shiny Seru (rare +35% capturable variant): battle-load stat boost, capture
// marking, persistence, and the +35% damage bonus on cast.
// ---------------------------------------------------------------------------

#[test]
fn shiny_roll_boosts_only_the_capturable_enemy() {
    use crate::monster_catalog::{FormationDef, FormationSlot};
    let mut world = capture_world(1);
    world.set_shiny_chance_pct(100); // force a shiny this battle
    // Killer Bee (id 7) is capturable (Seru 1); Wolf (id 9) is not.
    let formation = FormationDef::new(1, vec![FormationSlot::new(7), FormationSlot::new(9)]);
    world.enter_battle_from_formation(&formation);

    // Slot 1 = first monster (Killer Bee). hp 25 -> 33, attack 9 -> 12.
    assert!(
        world.shiny_enemy_slots.contains(&1),
        "the capturable enemy is flagged shiny"
    );
    assert_eq!(world.actors[1].battle.max_hp, 33, "shiny HP +35%");
    assert_eq!(world.battle_attack[1], 12, "shiny ATK +35%");
    // Slot 2 = Wolf (not capturable) is never chosen.
    assert!(!world.shiny_enemy_slots.contains(&2));
}

#[test]
fn shiny_disabled_when_chance_is_zero() {
    use crate::monster_catalog::{FormationDef, FormationSlot};
    let mut world = capture_world(1);
    world.set_shiny_chance_pct(0);
    let formation = FormationDef::new(1, vec![FormationSlot::new(7)]);
    world.enter_battle_from_formation(&formation);
    assert!(world.shiny_enemy_slots.is_empty(), "no shiny when disabled");
    assert_eq!(world.actors[1].battle.max_hp, 25, "stats unmodified");
}

#[test]
fn shiny_capture_marks_spell_shiny_and_persists_through_save() {
    let mut world = capture_world(1);
    world.set_spell_catalog(crate::spells::SpellCatalog::vanilla());
    // Killer Bee captured as shiny (Seru 1 -> spell 0x20).
    world.battle_captures = vec![7];
    world.shiny_captures = vec![7];
    world.finish_battle();

    assert!(world.seru_log.has_learned(0, 1), "spell learned");
    assert!(
        world.seru_log.is_shiny(0, 0x20),
        "shiny capture flags the learned spell shiny"
    );
    // shiny_captures drained.
    assert!(world.shiny_captures.is_empty());

    // Round-trips through the LGSF v4 save.
    let sf = world.save_full();
    assert!(
        sf.ext_v2
            .per_char
            .iter()
            .any(|(slot, ce)| *slot == 0 && ce.shiny_spells.contains(&0x20)),
        "shiny spell serialised into the save"
    );
    let mut reloaded = capture_world(1);
    reloaded.load_full(sf);
    assert!(
        reloaded.seru_log.is_shiny(0, 0x20),
        "shiny survives save/load"
    );
}

#[test]
fn non_shiny_capture_does_not_flag_shiny() {
    let mut world = capture_world(1);
    world.battle_captures = vec![7]; // captured normally (not in shiny_captures)
    world.finish_battle();
    assert!(world.seru_log.has_learned(0, 1));
    assert!(
        !world.seru_log.is_shiny(0, 0x20),
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
    shiny.seru_log.mark_shiny(0, 0x81);
    let before_s = shiny.actors[1].battle.hp;
    shiny.cast_spell_on_slots(0, &def, &[1]);
    let shiny_dmg = (before_s - shiny.actors[1].battle.hp) as u32;

    let expected = (plain_dmg * 135 / 100).min(9999);
    assert_eq!(
        shiny_dmg, expected,
        "shiny cast deals +35% (plain {plain_dmg} -> shiny {shiny_dmg})"
    );
}
