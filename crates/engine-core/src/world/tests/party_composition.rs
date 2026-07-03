use super::*;

// ---------------------------------------------------------------------------
// Present-party composition (`World::active_party`)
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
    assert_eq!(world.party_count, 2);
    assert_eq!(world.party_roster_slot(0), 1);
    assert_eq!(world.party_roster_slot(1), 3);
    // Actor mirrors reseeded per the mapping.
    assert_eq!(world.actors[0].battle.max_hp, 200, "ordinal 0 = Noa's HP");
    assert_eq!(world.actors[1].battle.max_hp, 400, "ordinal 1 = Terra's HP");
    assert_eq!(world.battle_speed[0], 10);
    assert_eq!(world.battle_speed[1], 20);
    // Stat seeding folds the OCCUPYING character's record onto the ordinal.
    world.seed_party_battle_stats();
    assert_eq!(
        world.battle_attack[0], 22,
        "ordinal 0 attacks with Noa's ATK"
    );
    assert_eq!(
        world.battle_attack[1], 44,
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
    world.set_spell_catalog(crate::spells::SpellCatalog::vanilla());
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
    assert_eq!(world.level_up_tracker.xp[0], 0, "Vahn (absent) gets none");
    assert_eq!(world.level_up_tracker.xp[1], 0, "Noa (absent) gets none");
    assert!(
        world.level_up_tracker.xp[2] > 0,
        "Gala (ordinal 0) earns XP"
    );
    assert!(
        world.level_up_tracker.xp[3] > 0,
        "Terra (ordinal 1) earns XP"
    );
}

#[test]
fn active_party_survives_save_roundtrip_and_maps_hp_writeback() {
    let mut world = World::new();
    world.load_party(composition_roster());
    world.set_active_party(vec![1, 3]);
    // Battle damage on ordinal 0 (= Noa).
    world.actors[0].battle.hp = 150;
    let sf = world.save_full();
    assert_eq!(sf.ext_v2.active_party, vec![1, 3]);
    let noa = sf.party.members[1].hp_mp_sp();
    assert_eq!(noa.hp_cur, 150, "ordinal-0 damage lands on Noa's record");
    let vahn = sf.party.members[0].hp_mp_sp();
    assert_eq!(vahn.hp_cur, 100, "absent Vahn's record is untouched");

    let mut fresh = World::new();
    fresh.load_full(sf);
    assert_eq!(fresh.active_party, vec![1, 3]);
    assert_eq!(fresh.party_count, 2);
    assert_eq!(fresh.actors[0].battle.hp, 150, "Noa's HP back on ordinal 0");
}

#[test]
fn identity_save_keeps_legacy_party_semantics() {
    let mut world = World::new();
    world.load_party(composition_roster());
    let sf = world.save_full();
    // No composition installed: the historical full-roster identity order.
    assert_eq!(sf.ext_v2.active_party, vec![0, 1, 2, 3]);
    let mut fresh = World::new();
    fresh.load_full(sf);
    assert!(
        fresh.active_party.is_empty(),
        "identity order restores as the identity default"
    );
    assert_eq!(fresh.party_count, 4, "legacy party_count preserved");
}
