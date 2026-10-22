use super::*;
use legaia_engine_mode::mode::SceneMode;

// ---------------------------------------------------------------------------
// Present-party composition (`World::party.active_party`)
// ---------------------------------------------------------------------------

/// Four-record roster with distinct, recognisable stats per character.
fn composition_roster() -> legaia_save::Party {
    let mut party = legaia_save::Party::zeroed(4);
    for (slot, rec) in party.members.iter_mut().enumerate() {
        let mut hms = rec.hp_mp_sp();
        hms.hp_max = 100 + slot as u16 * 100; // 100/200/300/400
        hms.hp_cur = hms.hp_max;
        hms.mp_max = 10 + slot as u16 * 10;
        hms.mp_cur = hms.mp_max;
        rec.set_hp_mp_sp(hms);
        let mut ls = rec.live_stats();
        ls.atk = 11 + slot as u16 * 11; // 11/22/33/44
        ls.spd = 5 + slot as u16 * 5;
        rec.set_live_stats(ls);
    }
    party
}

#[test]
fn active_party_maps_battle_ordinals_to_characters() {
    let mut world = World::new();
    world.load_party(composition_roster());
    // Noa + Terra present: battle ordinal 0 = roster slot 1, ordinal 1 =
    // roster slot 3 (the live-verified retail Terra-party shape).
    world.set_active_party(vec![1, 3]);
    assert_eq!(world.party.party_count, 2);
    assert_eq!(world.party_roster_slot(0), 1);
    assert_eq!(world.party_roster_slot(1), 3);
    // Actor mirrors reseeded per the mapping.
    assert_eq!(world.actors[0].battle.max_hp, 200, "ordinal 0 = Noa's HP");
    assert_eq!(world.actors[1].battle.max_hp, 400, "ordinal 1 = Terra's HP");
    assert_eq!(world.battle.speed[0], 10);
    assert_eq!(world.battle.speed[1], 20);
    // Stat seeding folds the OCCUPYING character's record onto the ordinal.
    world.seed_party_battle_stats();
    assert_eq!(
        world.battle.attack[0], 22,
        "ordinal 0 attacks with Noa's ATK"
    );
    assert_eq!(
        world.battle.attack[1], 44,
        "ordinal 1 attacks with Terra's ATK"
    );
}

#[test]
fn battle_spell_session_reads_composed_character() {
    let mut world = World::new();
    let mut party = composition_roster();
    // Terra (slot 3) knows Flame; nobody else knows anything.
    let mut list = party.members[3].spell_list();
    list.count = 1;
    list.ids[0] = 0x20;
    party.members[3].set_spell_list(list);
    world.load_party(party);
    world.set_spell_catalog(legaia_engine_battle::spells::SpellCatalog::vanilla());
    world.set_active_party(vec![3, 0]);
    world.mode = SceneMode::Battle;
    // Ordinal 0 (Terra) offers her spell; ordinal 1 (Vahn) has none.
    let menu = world
        .build_battle_spell_session(0)
        .expect("composed caster builds a session");
    assert_eq!(menu.spells.len(), 1, "Terra's learned spell shows");
    let vahn_menu = world.build_battle_spell_session(1);
    assert!(
        vahn_menu.is_none_or(|m| m.spells.is_empty()),
        "ordinal 1 (Vahn) has no learned spells"
    );
}

#[test]
fn battle_xp_routes_to_composed_characters() {
    let mut world = World::new();
    world.load_party(composition_roster());
    world.set_active_party(vec![2, 3]);
    world.enter_battle(2, 1);
    world.apply_battle_xp(100);
    // The 3/4-scaled split lands on the OCCUPYING characters' XP wells
    // (roster slots 2 + 3), not on slots 0/1.
    assert_eq!(
        world.party.level_up_tracker.xp[0], 0,
        "Vahn (absent) gets none"
    );
    assert_eq!(
        world.party.level_up_tracker.xp[1], 0,
        "Noa (absent) gets none"
    );
    assert!(
        world.party.level_up_tracker.xp[2] > 0,
        "Gala (ordinal 0) earns XP"
    );
    assert!(
        world.party.level_up_tracker.xp[3] > 0,
        "Terra (ordinal 1) earns XP"
    );
}
