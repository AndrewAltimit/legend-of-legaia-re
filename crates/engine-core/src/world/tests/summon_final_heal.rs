use super::*;

#[test]
fn field_tile_crossing_refreshes_region_state() {
    // Wiring oracle for the per-tile region ports (FUN_80017FBC /
    // FUN_800180EC / FUN_801DBA20 in `crate::field_regions`): install a
    // synthetic `.MAP` region block + MAN zone table, cross a tile in a
    // live field tick, and assert the op-0x42 mask (`extra_flags`) and the
    // camera-zone record refresh.
    use crate::field_regions::ZONE_RECORD_STRIDE;

    // .MAP region block: one type-4 region covering tiles x [0,8), z [0,8),
    // one type-5 region covering x [8,16), z [0,8).
    let body_off = 0x20u16;
    let mut block = vec![0u8; 0x20 + 2 * 8];
    block[0xE..0x10].copy_from_slice(&body_off.to_le_bytes());
    block[0x10..0x12].copy_from_slice(&2u16.to_le_bytes());
    block[0x20..0x25].copy_from_slice(&[0, 0, 8, 8, 4]);
    block[0x28..0x2D].copy_from_slice(&[8, 0, 16, 8, 5]);
    // Zone table: a kind-5 record (matches while the type-5 region bit is
    // set) with a payload marker byte.
    let mut zone = vec![1u8];
    let mut rec = [0u8; ZONE_RECORD_STRIDE];
    rec[0] = 5;
    rec[5] = 0xAB;
    zone.extend_from_slice(&rec);

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.live_gameplay_loop = true;
    world.install_field_player(0);
    // Tile (5, 5): world = 0x40 + tile * 0x80 ((w - 0x40) >> 7 = tile).
    world.actors[0].move_state.world_x = 0x40 + 5 * 0x80;
    world.actors[0].move_state.world_z = 0x40 + 5 * 0x80;
    world.load_field_region_tables(&block, &zone);

    // Initial refresh: inside the type-4 region, no zone match.
    assert_eq!(world.extra_flags, 1 << 4);
    assert!(world.field_zone_record.is_none());

    // Prime the tile latch, then cross into the type-5 region.
    world.tick();
    world.actors[0].move_state.world_x = 0x40 + 9 * 0x80;
    world.tick();

    assert_eq!(world.extra_flags, 1 << 5, "mask rebuilt on tile crossing");
    let rec = world
        .field_zone_record
        .expect("kind-5 zone record selected");
    assert_eq!(rec[0], 5);
    assert_eq!(rec[5], 0xAB, "payload carried through");
}

#[test]
fn summon_cast_accrues_spell_xp_from_dealt_damage() {
    let mut world = summon_xp_world(4000, 4000);
    let def = gimard_spell_def();
    let before = world.actors[1].battle.hp;
    world.cast_spell_on_slots(0, &def, &[1]);
    let damage = (before - world.actors[1].battle.hp) as u32;
    assert!(damage > 0, "the placeholder cast deals damage");
    // Non-kill single-target accrual: damage * 12 / max_hp
    // (FUN_801ddb30 tail; kernel summon_spell_xp_gain).
    let expected = vm::battle_formulas::summon_spell_xp_gain(damage, 4000, 4000, false);
    let slot = crate::magic_xp::spell_slot(&world.roster.members[0], 0x81).unwrap();
    assert_eq!(
        crate::magic_xp::spell_xp(&world.roster.members[0], slot),
        expected
    );
    // No thresholds installed: XP accrues but the spell never levels.
    assert_eq!(world.roster.members[0].spell_list().levels[0], 1);
    assert!(world.drain_magic_level_ups().is_empty());
}

#[test]
fn summon_kill_accrues_flat_unit_and_levels_up_past_threshold() {
    let mut world = summon_xp_world(50, 4000);
    // Tiny live HP: the cast kills -> flat 12 XP (single-target).
    world.magic_xp_thresholds = Some([17, 50, 92, 144, 208, 288, 392, 536]);
    // Pre-bank XP just below the level-1 threshold: 6 + 12 = 18 > 17.
    let slot = crate::magic_xp::spell_slot(&world.roster.members[0], 0x81).unwrap();
    crate::magic_xp::add_spell_xp(&mut world.roster.members[0], slot, 6);

    let def = gimard_spell_def();
    world.cast_spell_on_slots(0, &def, &[1]);
    assert_eq!(world.actors[1].battle.hp, 0, "the cast kills the target");
    assert_eq!(
        crate::magic_xp::spell_xp(&world.roster.members[0], slot),
        18,
        "kill grants the flat 12-XP unit"
    );
    assert_eq!(
        world.roster.members[0].spell_list().levels[0],
        2,
        "18 XP > threshold 17 levels the spell (strict greater)"
    );
    assert_eq!(world.drain_magic_level_ups(), vec![(0, 0x81, 2)]);
    // The leveled byte is what the next cast's magic-power stage reads.
    assert_eq!(world.caster_magic_power_byte(0, 0x81), 2);
}

#[test]
fn summon_xp_threshold_compare_is_strict() {
    let mut world = summon_xp_world(50, 4000);
    world.magic_xp_thresholds = Some([17, 50, 92, 144, 208, 288, 392, 536]);
    // 5 + 12 = 17 == threshold: strict compare -> no level.
    let slot = crate::magic_xp::spell_slot(&world.roster.members[0], 0x81).unwrap();
    crate::magic_xp::add_spell_xp(&mut world.roster.members[0], slot, 5);
    let def = gimard_spell_def();
    world.cast_spell_on_slots(0, &def, &[1]);
    assert_eq!(
        crate::magic_xp::spell_xp(&world.roster.members[0], slot),
        17
    );
    assert_eq!(world.roster.members[0].spell_list().levels[0], 1);
    assert!(world.drain_magic_level_ups().is_empty());
}

#[test]
fn non_summon_spell_accrues_no_spell_xp() {
    let mut world = summon_xp_world(4000, 4000);
    // Same shape but a non-Seru-magic id (outside 0x81..=0x8B).
    let mut def = gimard_spell_def();
    def.id = 0x27;
    let mut list = world.roster.members[0].spell_list();
    list.ids[0] = 0x27;
    world.roster.members[0].set_spell_list(list);
    world.cast_spell_on_slots(0, &def, &[1]);
    let slot = crate::magic_xp::spell_slot(&world.roster.members[0], 0x27).unwrap();
    assert_eq!(crate::magic_xp::spell_xp(&world.roster.members[0], slot), 0);
}

#[test]
fn final_heal_revives_and_consumes_one_lost_grail() {
    use legaia_save::Party;
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.roster = Party::zeroed(1);
    // Down at 0 HP with the Final Heal bit (word 1 bit 7 = ability 0x27) and
    // one equipped Lost Grail (0xE7) in the first accessory slot (+0x19B).
    world.actors[0].battle.max_hp = 250;
    world.actors[0].battle.hp = 0;
    world.actors[0].battle.liveness = 0;
    let rec = &mut world.roster.members[0];
    let mut bits = rec.ability_bits();
    bits[4] = 0x80;
    rec.set_ability_bits(bits);
    let mut eq = rec.equipment();
    eq.slots[5] = 0xE7;
    rec.set_equipment(eq);

    world.apply_final_heal_revives();

    assert_eq!(
        world.actors[0].battle.hp, 250,
        "full max-HP revive (tier 1)"
    );
    assert_eq!(world.actors[0].battle.liveness, 1);
    let rec = &world.roster.members[0];
    assert_eq!(rec.equipment().slots[5], 0, "the Lost Grail is consumed");
    assert_eq!(
        rec.ability_bits()[4] & 0x80,
        0,
        "the Final Heal bit clears with no second Grail equipped"
    );
    assert!(
        world
            .battle_hit_fx
            .iter()
            .any(|fx| fx.target_slot == 0 && fx.is_heal && fx.amount == 250),
        "heal popup recorded"
    );
}

#[test]
fn final_heal_keeps_bit_when_second_grail_is_equipped() {
    use legaia_save::Party;
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.roster = Party::zeroed(1);
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.hp = 0;
    let rec = &mut world.roster.members[0];
    let mut bits = rec.ability_bits();
    bits[4] = 0x80;
    rec.set_ability_bits(bits);
    let mut eq = rec.equipment();
    eq.slots[5] = 0xE7;
    eq.slots[7] = 0xE7;
    rec.set_equipment(eq);

    world.apply_final_heal_revives();

    let rec = &world.roster.members[0];
    assert_eq!(rec.equipment().slots[5], 0, "first Grail consumed");
    assert_eq!(rec.equipment().slots[7], 0xE7, "second Grail kept");
    assert_eq!(
        rec.ability_bits()[4] & 0x80,
        0x80,
        "bit re-set while another Grail is equipped (the second slot scan)"
    );
}

#[test]
fn final_heal_ignores_members_without_the_bit() {
    use legaia_save::Party;
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.roster = Party::zeroed(1);
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.hp = 0;
    world.actors[0].battle.liveness = 0;

    world.apply_final_heal_revives();

    assert_eq!(world.actors[0].battle.hp, 0, "stays down without the bit");
    assert_eq!(world.actors[0].battle.liveness, 0);
}
