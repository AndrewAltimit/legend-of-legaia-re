use crate::world::World;
use legaia_game_tables::spell_names::{CAPTURE_CLASS, SpellEntry, SpellNameTable};

const GUILTY_CROSS: u8 = 0x37;

fn world_with_capture_special() -> World {
    let mut entries = vec![SpellEntry::default(); 0x100];
    entries[GUILTY_CROSS as usize] = SpellEntry {
        class: CAPTURE_CLASS,
        sub_class: 2,
        mp: 12,
        name: Some("Guilty Cross".into()),
        ..SpellEntry::default()
    };
    let mut world = World::default();
    world.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
        spell_names: Some(SpellNameTable::from_entries(entries)),
        ..Default::default()
    });
    world
}

/// A capture-class special is kept out of the spell catalog, so the
/// monster turn's cast record comes off the disc table - without it the
/// turn found no record and struck instead of casting.
#[test]
fn a_capture_special_has_a_cast_record_the_catalog_does_not_carry() {
    let world = world_with_capture_special();
    assert!(world.tables.spell_catalog.get(GUILTY_CROSS).is_none());
    let def = world.monster_cast_def(GUILTY_CROSS).expect("a cast record");
    assert_eq!(
        (def.id, def.mp_cost, def.effect_class),
        (GUILTY_CROSS, 12, 2)
    );
    assert!(
        World::default().monster_cast_def(GUILTY_CROSS).is_none(),
        "no disc table, no record"
    );
}

fn cort_under_shield(monster_id: u16, hp: u16) -> World {
    let mut world = World::default();
    world.party.party_count = 3;
    for _ in 0..4 {
        world.actors.push(crate::world::Actor::default());
    }
    let cort = &mut world.actors[3];
    cort.battle_monster_id = Some(monster_id);
    cort.battle.max_hp = 50_000;
    cort.battle.hp = hp;
    // The neutral colour word: the arm tests `+0x04` before it poses or
    // breaks the shield.
    cort.battle.render_color = 0x2008_0200;
    world.battle.monster_ai_state.flag_bd84 = 1;
    world
}

/// Cort's Mystic Shield holds until he is worn to half HP
/// (`FUN_8004CE2C`, `0x8004D5CC..0x8004D5D4`), then the pass restores
/// the reaction run arm 4 blanked and clears the shield word.
#[test]
fn the_mystic_shield_breaks_at_half_hp() {
    let mut world = cort_under_shield(0xB4, 25_001);
    world.tick_mystic_shield();
    assert!(world.mystic_shield_up(), "above half HP it holds");

    world.actors[3].battle.hp = 25_000;
    world.tick_mystic_shield();
    assert!(!world.mystic_shield_up(), "at half HP it breaks");
    let p = &world.actors[3].battle.params;
    assert_eq!(
        [
            p[0x1EF - 0x1DF],
            p[0x1F0 - 0x1DF],
            p[0x1F1 - 0x1DF],
            p[0x1F2 - 0x1DF]
        ],
        [3, 2, 4, 5]
    );

    let mut other = cort_under_shield(0xB5, 1);
    other.tick_mystic_shield();
    assert!(
        other.mystic_shield_up(),
        "the arm is keyed on formation 0xB4"
    );
}

/// A zero colour word on the first monster seat takes the hide branch
/// (`beq v0,zero,0x8004D670`): no pose, and no break test either, however
/// low the seat's HP has fallen.
#[test]
fn a_blank_colour_word_skips_the_shield_break() {
    let mut world = cort_under_shield(0xB4, 1);
    world.actors[3].battle.render_color = 0;
    world.tick_mystic_shield();
    assert!(world.mystic_shield_up());
    world.actors[3].battle.render_color = 0x2008_0200;
    world.tick_mystic_shield();
    assert!(!world.mystic_shield_up());
}

/// A synthetic `SCUS_942.54` image holding stand-ins for the three shield
/// records at their retail addresses: a long-waiting default-arm disc for
/// the sphere, a bare wait for the scroller, a short-lived burst.
fn scus_with_shield_records() -> Vec<u8> {
    use super::mystic_shield::{
        MYSTIC_SHIELD_BURST_RECORD, MYSTIC_SHIELD_SCROLLER_RECORD, MYSTIC_SHIELD_SPHERE_RECORD,
    };
    const BIAS: u32 = 0x8000_F800;
    let mut scus = vec![0u8; (0x8007_8780 - BIAS) as usize];
    let mut put = |va: u32, words: &[u16]| {
        let at = (va - BIAS) as usize;
        for (i, w) in words.iter().enumerate() {
            scus[at + 2 * i..at + 2 * i + 2].copy_from_slice(&w.to_le_bytes());
        }
    };
    put(
        MYSTIC_SHIELD_SPHERE_RECORD,
        &[
            0xFFFF, 0, 0x13, 0x100, 0x10, 0, 0xFF, 0xFF, 0xFF, 0x40, 0x40, 0x40, 0, 0xE0, 0, 0,
            0x1000, 0x1000, 0x09, 0x7FF,
        ],
    );
    put(MYSTIC_SHIELD_BURST_RECORD, &[0xFFFF, 0, 0x09, 0x10, 0x08]);
    put(MYSTIC_SHIELD_SCROLLER_RECORD, &[0xFFFF, 0, 0x09, 0x7FF]);
    scus
}

/// PROT 0940's arm 4 seats the sphere and its scroller on the first monster
/// seat's display trio; the actor pass then poses the sphere while the seat's
/// colour word is up, hides it while it is blank, and at half HP retires it
/// behind the burst (`FUN_8004CE2C`, `0x8004D534..0x8004D674`).
#[test]
fn the_mystic_shield_seats_poses_and_bursts() {
    let mut world = cort_under_shield(0xB4, 40_000);
    world.battle.monster_ai_state.flag_bd84 = 0;
    world.tables.scus = Some(std::sync::Arc::from(scus_with_shield_records()));
    world.actors[3].battle.seat = Some((12, 900));
    world.raise_mystic_shield();
    assert!(world.mystic_shield_up());
    let fx = world
        .casting
        .mystic_shield_fx
        .as_ref()
        .expect("shield seated");
    assert_eq!(fx.parts.len(), 2, "sphere and scroller");
    let s = &fx.parts[0].state;
    assert_eq!([s.world_x, s.world_z], [12, 900]);

    world.tick_mystic_shield();
    let sphere = |w: &World| w.casting.mystic_shield_fx.as_ref().unwrap().parts[0].clone();
    assert_eq!(
        sphere(&world).state.move_substate,
        4,
        "drawn while the colour word is up"
    );
    world.actors[3].battle.render_color = 0;
    world.tick_mystic_shield();
    assert_eq!(
        sphere(&world).state.move_substate,
        0,
        "hidden while it is blank"
    );

    world.actors[3].battle.render_color = 0x2008_0200;
    world.actors[3].battle.hp = 25_000;
    world.tick_mystic_shield();
    assert!(!world.mystic_shield_up());
    assert!(sphere(&world).finished, "the sphere is retired");
    assert_eq!(
        world.casting.active_action_fx.len(),
        1,
        "the burst is seated"
    );
}

/// Disc-free, the word still rises - its damage halve and the Evil Seru
/// Magic gate do not need the records - and nothing is seated.
#[test]
fn a_disc_free_shield_raises_the_word_only() {
    let mut world = cort_under_shield(0xB4, 40_000);
    world.battle.monster_ai_state.flag_bd84 = 0;
    world.raise_mystic_shield();
    assert!(world.mystic_shield_up());
    assert!(world.casting.mystic_shield_fx.is_none());
}
