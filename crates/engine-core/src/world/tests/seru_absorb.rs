//! The killing-blow Seru absorb (`FUN_801EC3E4` `0x801EE1C0..0x801EE2E8`)
//! staging its Seru, and the Done band's grant teaching it.

use super::*;

const GIMARD: u16 = 0x40;

/// One party member (Vahn, roster slot 0) facing one Seru monster in slot 1
/// whose record carries Seru `1` (spell `0x81`) at `chance` percent.
fn absorb_world(chance: u8, ra_seru: bool) -> World {
    let mut world = World::new();
    world.load_party(legaia_save::Party::zeroed(1));
    world.party.party_count = 1;
    let rec = &mut world.party.roster.members[0];
    let mut eq = rec.equipment();
    // Vahn's Ra-Seru marker reads equipment slot 3 (`FUN_80053CB8`).
    eq.slots[3] = if ra_seru { 0x30 } else { 0 };
    rec.set_equipment(eq);
    let mut def = crate::monster_catalog::MonsterDef::new(GIMARD, "Seru", 50, 10);
    def.absorb_seru = 1;
    def.absorb_chance_pct = chance;
    world.tables.monster_catalog.insert(def);
    world.actors[0].battle.hp = 100;
    world.actors[1].battle_monster_id = Some(GIMARD);
    world.actors[1].battle.hp = 50;
    world.battle_ctx.active_actor = 0;
    world
}

fn learned(world: &World) -> Vec<u8> {
    let list = world.party.roster.members[0].spell_list();
    list.ids[..usize::from(list.count)].to_vec()
}

/// A certain roll stages Seru `1` into `ctx[+0x269]`, and the Done band's
/// grant prepends spell `0x81` to the character record.
#[test]
fn a_killing_blow_on_a_seru_monster_stages_and_grants_its_spell() {
    let mut world = absorb_world(100, true);
    world.roll_seru_absorb(0, 1);
    assert_eq!(world.battle_ctx.multi_cast_gate, 1, "Seru 1 staged");
    world.learn_absorbed_seru(0, world.battle_ctx.multi_cast_gate);
    assert_eq!(learned(&world), vec![0x81]);
}

/// The lookup's "not applicable" answer is "already known": no Ra-Seru, no
/// absorb - but the roll still consumed its draw.
#[test]
fn no_ra_seru_means_no_absorb() {
    let mut world = absorb_world(100, false);
    world.roll_seru_absorb(0, 1);
    assert_eq!(world.battle_ctx.multi_cast_gate, 0);
}

/// A Seru the character already knows is not staged again.
#[test]
fn a_known_seru_is_not_absorbed_twice() {
    let mut world = absorb_world(100, true);
    world.learn_absorbed_seru(0, 1);
    world.roll_seru_absorb(0, 1);
    assert_eq!(world.battle_ctx.multi_cast_gate, 0);
    assert_eq!(learned(&world), vec![0x81]);
}

/// A monster attacker, a non-Seru target, and a zero chance all stage
/// nothing.
#[test]
fn only_a_party_blow_on_a_seru_can_absorb() {
    let mut world = absorb_world(100, true);
    world.roll_seru_absorb(1, 0);
    assert_eq!(world.battle_ctx.multi_cast_gate, 0, "monster attacker");
    let mut world = absorb_world(0, true);
    world.roll_seru_absorb(0, 1);
    assert_eq!(world.battle_ctx.multi_cast_gate, 0, "0% chance");
    let mut world = absorb_world(100, true);
    world.actors[1].battle_monster_id = None;
    world.roll_seru_absorb(0, 1);
    assert_eq!(
        world.battle_ctx.multi_cast_gate, 0,
        "no record behind the target"
    );
}

/// The Ivory Book's Magic Boost adds a flat 30: a 0% Seru becomes 30%, so
/// across many seeds some rolls land that never could without it.
#[test]
fn magic_boost_adds_thirty_points() {
    let mut landed = 0;
    for seed in 0..200u32 {
        let mut world = absorb_world(0, true);
        world.rng_state = seed.wrapping_mul(0x9E37_79B9);
        let rec = &mut world.party.roster.members[0];
        let mut bits = rec.ability_bits();
        bits[5] |= 0x40; // word +0xF8, bit 0x4000
        rec.set_ability_bits(bits);
        world.roll_seru_absorb(0, 1);
        landed += usize::from(world.battle_ctx.multi_cast_gate != 0);
    }
    assert!((20..=110).contains(&landed), "~30% of 200, got {landed}");
}
