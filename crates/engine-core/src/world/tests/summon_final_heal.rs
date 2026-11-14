use super::*;
use legaia_engine_mode::mode::SceneMode;

#[test]
fn field_tile_crossing_keeps_the_latched_region_mask() {
    // The region-type mask (`extra_flags`, `_DAT_8007B8F4`) is a latch.
    // Retail rewrites it at the re-centres, the zone queries and `[4C 3D]`,
    // and per frame only behind scratchpad bit 22 (`FUN_801D1344`,
    // `0x801D17DC..0x801D1830`); a walk's tile crossing never does. Install
    // a synthetic `.MAP` region block + MAN zone table, cross a tile in a
    // live field tick, and assert the mask holds - then raise bit 22 and
    // assert the per-frame path rebuilds it at `(coord + 0x40) >> 7`.
    use legaia_engine_field::field_regions::ZONE_RECORD_STRIDE;

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
    world.toggles.live_gameplay_loop = true;
    world.install_field_player(0);
    // Tile (5, 5): world = 0x40 + tile * 0x80 ((w - 0x40) >> 7 = tile).
    world.actors[0].move_state.world_x = 0x40 + 5 * 0x80;
    world.actors[0].move_state.world_z = 0x40 + 5 * 0x80;
    world.load_field_region_tables(&block, &zone);

    // The install latches at the seat: inside the type-4 region.
    assert_eq!(world.flags.extra_flags, 1 << 4);
    assert!(world.terrain.zone_record.is_none());

    // Walk into the type-5 region: the latch holds.
    world.tick();
    world.actors[0].move_state.world_x = 0x40 + 9 * 0x80;
    world.tick();
    assert_eq!(
        world.flags.extra_flags,
        1 << 4,
        "a tile crossing must not rebuild the mask"
    );

    // Bit 22 raised: the player tick rebuilds it every frame.
    world.flags.story_flags |= crate::world::ZONE_REQUERY_FLAG;
    world.tick();
    assert_eq!(
        world.flags.extra_flags,
        1 << 5,
        "bit 22 re-queries per frame"
    );

    // The `[4C C4 x z]` query's own mask write, at an explicit tile.
    world.flags.story_flags &= !crate::world::ZONE_REQUERY_FLAG;
    world.zone_query_region_mask(3, 3);
    assert_eq!(world.flags.extra_flags, 1 << 4);
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
    let expected =
        legaia_engine_battle_vm::battle_formulas::summon_spell_xp_gain(damage, 4000, 4000, false);
    let slot =
        legaia_engine_battle::magic_xp::spell_slot(&world.party.roster.members[0], 0x81).unwrap();
    assert_eq!(
        legaia_engine_battle::magic_xp::spell_xp(&world.party.roster.members[0], slot),
        expected
    );
    // No thresholds installed: XP accrues but the spell never levels.
    assert_eq!(world.party.roster.members[0].spell_list().levels[0], 1);
    assert!(world.drain_magic_level_ups().is_empty());
}

#[test]
fn summon_kill_accrues_flat_unit_and_levels_up_past_threshold() {
    let mut world = summon_xp_world(50, 4000);
    // Tiny live HP: the cast kills -> flat 12 XP (single-target).
    world.tables.magic_xp_thresholds = Some([17, 50, 92, 144, 208, 288, 392, 536]);
    // Pre-bank XP just below the level-1 threshold: 6 + 12 = 18 > 17.
    let slot =
        legaia_engine_battle::magic_xp::spell_slot(&world.party.roster.members[0], 0x81).unwrap();
    legaia_engine_battle::magic_xp::add_spell_xp(&mut world.party.roster.members[0], slot, 6);

    let def = gimard_spell_def();
    world.cast_spell_on_slots(0, &def, &[1]);
    assert_eq!(world.actors[1].battle.hp, 0, "the cast kills the target");
    assert_eq!(
        legaia_engine_battle::magic_xp::spell_xp(&world.party.roster.members[0], slot),
        18,
        "kill grants the flat 12-XP unit"
    );
    assert_eq!(
        world.party.roster.members[0].spell_list().levels[0],
        2,
        "18 XP > threshold 17 levels the spell (strict greater)"
    );
    assert_eq!(world.drain_magic_level_ups(), vec![(0, 0x81, 2)]);
    // The leveled byte is what the next cast's magic-power stage reads.
    assert_eq!(world.caster_magic_power_byte(0, 0x81), 2);
    // `sb v0,0x26(v1)` at `0x801E723C`: the same arm records the banner
    // element on the battle context. Its reader is the action machine's Done
    // band, which seeds `0x96` frames instead of `0x3C` while it is set (see
    // `battle_action::done`'s own tests). Asserted here at the fold rather
    // than after a tick, because the next action's `ActionSeed` clears it
    // (`0x801E2CFC`) and a whole tick can cross that boundary.
    assert_eq!(world.battle_ctx.levelup_banner_element, 0x65);
}

/// The contrapositive: a cast that accrues XP without crossing the threshold
/// raises no banner, so `ctx[+0x26]` stays clear and the Done band keeps its
/// ordinary tail.
#[test]
fn a_cast_that_does_not_level_leaves_the_banner_byte_clear() {
    let mut world = summon_xp_world(4000, 4000);
    world.tables.magic_xp_thresholds = Some([17, 50, 92, 144, 208, 288, 392, 536]);
    let def = gimard_spell_def();
    world.cast_spell_on_slots(0, &def, &[1]);
    assert!(world.drain_magic_level_ups().is_empty());
    assert_eq!(world.battle_ctx.levelup_banner_element, 0);
}

#[test]
fn summon_xp_threshold_compare_is_strict() {
    let mut world = summon_xp_world(50, 4000);
    world.tables.magic_xp_thresholds = Some([17, 50, 92, 144, 208, 288, 392, 536]);
    // 5 + 12 = 17 == threshold: strict compare -> no level.
    let slot =
        legaia_engine_battle::magic_xp::spell_slot(&world.party.roster.members[0], 0x81).unwrap();
    legaia_engine_battle::magic_xp::add_spell_xp(&mut world.party.roster.members[0], slot, 5);
    let def = gimard_spell_def();
    world.cast_spell_on_slots(0, &def, &[1]);
    assert_eq!(
        legaia_engine_battle::magic_xp::spell_xp(&world.party.roster.members[0], slot),
        17
    );
    assert_eq!(world.party.roster.members[0].spell_list().levels[0], 1);
    assert!(world.drain_magic_level_ups().is_empty());
}

#[test]
fn non_summon_spell_accrues_no_spell_xp() {
    let mut world = summon_xp_world(4000, 4000);
    // Same shape but a non-Seru-magic id (outside 0x81..=0x8B).
    let mut def = gimard_spell_def();
    def.id = 0x27;
    let mut list = world.party.roster.members[0].spell_list();
    list.ids[0] = 0x27;
    world.party.roster.members[0].set_spell_list(list);
    world.cast_spell_on_slots(0, &def, &[1]);
    let slot =
        legaia_engine_battle::magic_xp::spell_slot(&world.party.roster.members[0], 0x27).unwrap();
    assert_eq!(
        legaia_engine_battle::magic_xp::spell_xp(&world.party.roster.members[0], slot),
        0
    );
}

/// Re-key the `summon_xp_world` record + def onto another spell id.
fn summon_xp_world_for(
    id: u8,
    enemy_hp: u16,
    enemy_max_hp: u16,
) -> (World, legaia_engine_battle::spells::SpellDef) {
    let mut world = summon_xp_world(enemy_hp, enemy_max_hp);
    let mut list = world.party.roster.members[0].spell_list();
    list.ids[0] = id;
    world.party.roster.members[0].set_spell_list(list);
    let mut def = gimard_spell_def();
    def.id = id;
    (world, def)
}

fn xp_of(world: &World, id: u8) -> u32 {
    let slot =
        legaia_engine_battle::magic_xp::spell_slot(&world.party.roster.members[0], id).unwrap();
    legaia_engine_battle::magic_xp::spell_xp(&world.party.roster.members[0], slot)
}

/// The high block strikes through the same slot-7 body (`FUN_801DD0AC(0x12,
/// 7, seat)` in every damaging module, e.g. Meta at `0x801F7BA0`), so the
/// finisher's tail credits it per hit like a base cast.
#[test]
fn high_block_damage_cast_accrues_spell_xp() {
    for id in [0x99u8, 0x9A, 0x9B, 0x9E, 0x9F, 0xA0] {
        let (mut world, def) = summon_xp_world_for(id, 4000, 4000);
        let before = world.actors[1].battle.hp;
        world.cast_spell_on_slots(0, &def, &[1]);
        let damage = (before - world.actors[1].battle.hp) as u32;
        let expected = legaia_engine_battle_vm::battle_formulas::summon_spell_xp_gain(
            damage, 4000, 4000, false,
        );
        assert!(expected > 0, "id {id:#x}");
        assert_eq!(xp_of(&world, id), expected, "id {id:#x}");
    }
}

/// Horn credits per party seat in its own arm: `+3` missing HP, `+1` status.
#[test]
fn horn_credits_per_party_seat() {
    let (mut world, mut def) = summon_xp_world_for(0x9C, 4000, 4000);
    def.target = legaia_engine_battle::spells::SpellTarget::AllAllies;
    def.effect = legaia_engine_battle::spells::SpellEffect::HealAll { amount: 10 };
    world.actors[0].battle.hp = 300;
    world.cast_spell_on_slots(0, &def, &[0]);
    assert_eq!(xp_of(&world, 0x9C), 3);
}

/// Jedo outside a scripted fight strikes nothing and credits `+3` per living
/// monster; inside one it strikes, and the tail credits per hit.
#[test]
fn jedo_credits_per_living_monster_outside_a_scripted_fight() {
    let (mut world, def) = summon_xp_world_for(0x9D, 4000, 4000);
    world.actors[2].battle.max_hp = 100;
    world.actors[2].battle.hp = 0;
    world.cast_spell_on_slots(0, &def, &[1]);
    assert_eq!(xp_of(&world, 0x9D), 3, "one living monster, one dead");

    let (mut world, def) = summon_xp_world_for(0x9D, 4000, 4000);
    world.battle.scripted_fight = true;
    let before = world.actors[1].battle.hp;
    world.cast_spell_on_slots(0, &def, &[1]);
    let damage = (before - world.actors[1].battle.hp) as u32;
    let expected =
        legaia_engine_battle_vm::battle_formulas::summon_spell_xp_gain(damage, 4000, 4000, false);
    assert_eq!(xp_of(&world, 0x9D), expected);
}

#[test]
fn final_heal_revives_and_consumes_one_lost_grail() {
    use legaia_save::Party;
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.party.roster = Party::zeroed(1);
    // Down at 0 HP with the Final Heal bit (word 1 bit 7 = ability 0x27) and
    // one equipped Lost Grail (0xE7) in the first accessory slot (+0x19B).
    world.actors[0].battle.max_hp = 250;
    world.actors[0].battle.hp = 0;
    world.actors[0].battle.liveness = 0;
    let rec = &mut world.party.roster.members[0];
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
    let rec = &world.party.roster.members[0];
    assert_eq!(rec.equipment().slots[5], 0, "the Lost Grail is consumed");
    assert_eq!(
        rec.ability_bits()[4] & 0x80,
        0,
        "the Final Heal bit clears with no second Grail equipped"
    );
    // Retail throws no numeral (`FUN_801E6968` pushes nothing); the
    // heal-numeral enhancement, on by default, throws the revive's HP.
    assert!(
        world
            .battle
            .hit_fx
            .iter()
            .any(|fx| fx.target_slot == 0 && fx.is_heal && fx.amount == 250),
        "the enhancement's heal numeral"
    );
}

/// A member downed and Final-Healed by one hit: the killing hit's readout
/// ramp is still in flight on the sweep's tick, and the revive's assigning
/// seed must not stack on top of it - the readout has to land on max HP, not
/// above it (an absorbing pair the `0x51` gate parks on).
#[test]
fn final_heal_mid_drain_settles_on_max_hp() {
    use legaia_save::Party;
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.party.roster = Party::zeroed(1);
    world.actors[0].battle.max_hp = 2368;
    world.actors[0].battle.hp = 744;
    let rec = &mut world.party.roster.members[0];
    let mut bits = rec.ability_bits();
    bits[4] = 0x80;
    rec.set_ability_bits(bits);
    let mut eq = rec.equipment();
    eq.slots[5] = 0xE7;
    rec.set_equipment(eq);

    // The killing hit lands: live HP to zero, the readout still at 744 with
    // the whole drain owed.
    world.apply_battle_hp_delta(0, 744);
    assert_eq!(world.actors[0].battle.hp, 0);
    world.apply_final_heal_revives();
    for _ in 0..200 {
        world.actors[0]
            .battle
            .tick_hp_bar(legaia_engine_battle_vm::battle_slot::RetailSlot::new(0));
    }
    let a = &world.actors[0].battle;
    assert_eq!(a.hp, 2368);
    assert_eq!(a.hp_display, Some(2368), "the readout settles on live HP");
    assert_eq!(a.hp_bar_pending, 0);
}

#[test]
fn final_heal_keeps_bit_when_second_grail_is_equipped() {
    use legaia_save::Party;
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.party.roster = Party::zeroed(1);
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.hp = 0;
    let rec = &mut world.party.roster.members[0];
    let mut bits = rec.ability_bits();
    bits[4] = 0x80;
    rec.set_ability_bits(bits);
    let mut eq = rec.equipment();
    eq.slots[5] = 0xE7;
    eq.slots[7] = 0xE7;
    rec.set_equipment(eq);

    world.apply_final_heal_revives();

    let rec = &world.party.roster.members[0];
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
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.party.roster = Party::zeroed(1);
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.hp = 0;
    world.actors[0].battle.liveness = 0;

    world.apply_final_heal_revives();

    assert_eq!(world.actors[0].battle.hp, 0, "stays down without the bit");
    assert_eq!(world.actors[0].battle.liveness, 0);
}

/// The level-up event list is a per-battle record: no host drains it (the
/// player-facing half is the `0x65` message banner), so battle entry bounds it
/// instead of letting it grow for the whole session.
#[test]
fn battle_entry_bounds_the_magic_level_up_record() {
    let mut world = summon_xp_world(50, 4000);
    world.tables.magic_xp_thresholds = Some([17, 50, 92, 144, 208, 288, 392, 536]);
    let slot =
        legaia_engine_battle::magic_xp::spell_slot(&world.party.roster.members[0], 0x81).unwrap();
    legaia_engine_battle::magic_xp::add_spell_xp(&mut world.party.roster.members[0], slot, 6);
    world.cast_spell_on_slots(0, &gimard_spell_def(), &[1]);
    assert_eq!(
        world.seru.magic_level_ups.len(),
        1,
        "the kill levels the spell"
    );
    world.enter_battle(1, 1);
    assert!(
        world.seru.magic_level_ups.is_empty(),
        "a new battle starts with an empty level-up record"
    );
}

/// A module's own wrapper rolls train the spell as the fold's does, and a
/// module with no fold (PROT 0910, 0915) still banks what they earned.
#[test]
fn a_waived_fold_banks_the_xp_its_modules_rolls_earned() {
    let mut world = summon_xp_world(4000, 4000);
    world.party.party_count = 1;
    let expected = 7;
    world.casting.module_hit_xp = expected;
    world.casting.module_skips_fold = true;
    world.casting.pending_cast = Some(legaia_engine_battle_loop::cast_band::PendingCast {
        caster: 0,
        spell_id: 0x81,
        targets: vec![1],
        on_flight: false,
    });
    world.fold_pending_cast();
    assert_eq!(world.actors[1].battle.hp, 4000, "the fold rolled nothing");
    let slot =
        legaia_engine_battle::magic_xp::spell_slot(&world.party.roster.members[0], 0x81).unwrap();
    assert_eq!(
        legaia_engine_battle::magic_xp::spell_xp(&world.party.roster.members[0], slot),
        expected
    );
    assert_eq!(world.casting.module_hit_xp, 0, "banked once");
}

/// A fold that is not waived adds to what the module's own rolls earned: Meta's,
/// Terra's and Palma's pre-hits and Ozma's ladder each ran the finisher's XP
/// tail (`FUN_801DD0AC` calls `FUN_801DDB30` on every roll, `0x801DD478`), so
/// the fold's own hits bank on top of them, not in their place.
#[test]
fn a_folded_cast_banks_its_modules_rolls_beside_its_own() {
    for id in [0x81u8, 0x9E, 0xA0] {
        let (mut plain, def) = summon_xp_world_for(id, 4000, 4000);
        let (mut pre, _) = summon_xp_world_for(id, 4000, 4000);
        plain.cast_spell_on_slots(0, &def, &[1]);
        pre.casting.module_hit_xp = 7;
        pre.cast_spell_on_slots(0, &def, &[1]);
        assert_eq!(xp_of(&pre, id), xp_of(&plain, id) + 7, "spell {id:#04X}");
        assert_eq!(pre.casting.module_hit_xp, 0, "banked once");
    }
}

/// PROT 0918's hit stages no clip: the fold lands its HP and leaves the
/// victim's reaction channel alone.
#[test]
fn a_quiet_fold_lands_its_hp_without_a_reaction() {
    let mut world = summon_xp_world(4000, 4000);
    world.casting.module_fold_quiet = true;
    let def = gimard_spell_def();
    let before = world.actors[1].battle.hp;
    world.cast_spell_on_slots(0, &def, &[1]);
    assert!(world.actors[1].battle.hp < before);
    assert!(world.actors[1].battle_reaction_entry.is_none());
}

/// Inside the summon band the hit banks its XP but the level check waits for
/// state `0x36` (`jal 0x801E70BC` on the band's way out), so the banner rises
/// after the module's last arm, not on the hit's pass.
#[test]
fn the_band_defers_the_level_check_to_its_return() {
    let mut world = summon_xp_world(50, 4000);
    world.tables.magic_xp_thresholds = Some([17, 50, 92, 144, 208, 288, 392, 536]);
    let slot =
        legaia_engine_battle::magic_xp::spell_slot(&world.party.roster.members[0], 0x81).unwrap();
    legaia_engine_battle::magic_xp::add_spell_xp(&mut world.party.roster.members[0], slot, 6);
    world.battle_ctx.action_state = 0x35;
    world.cast_spell_on_slots(0, &gimard_spell_def(), &[1]);
    assert!(world.drain_magic_level_ups().is_empty());
    assert_eq!(world.battle_ctx.levelup_banner_element, 0);
    world.run_summon_magic_level_check();
    assert_eq!(world.drain_magic_level_ups(), vec![(0, 0x81, 2)]);
    assert_eq!(world.battle_ctx.levelup_banner_element, 0x65);
}
