use super::*;

// --- Save / load round-trip ----------------------------------------

#[test]
fn load_party_populates_battle_actor_hp_mp() {
    let mut party = legaia_save::Party::zeroed(3);
    let mut hms = party.members[0].hp_mp_sp();
    hms.hp_cur = 137;
    hms.hp_max = 150;
    hms.mp_cur = 42;
    party.members[0].set_hp_mp_sp(hms);
    let mut hms1 = party.members[1].hp_mp_sp();
    hms1.hp_cur = 0; // dead member
    hms1.hp_max = 100;
    party.members[1].set_hp_mp_sp(hms1);

    let mut world = World::new();
    world.load_party(party);

    assert!(world.actors[0].active);
    assert_eq!(world.actors[0].battle.hp, 137);
    assert_eq!(world.actors[0].battle.max_hp, 150);
    assert_eq!(world.actors[0].battle.mp, 42);
    assert_eq!(world.actors[0].battle.liveness, 1);
    // Dead member: liveness flipped to 0.
    assert_eq!(world.actors[1].battle.liveness, 0);
    assert_eq!(world.party.party_count, 3);
}

#[test]
fn save_party_round_trips_after_load() {
    let mut party = legaia_save::Party::zeroed(3);
    let mut hms = party.members[0].hp_mp_sp();
    hms.hp_cur = 200;
    hms.hp_max = 250;
    hms.mp_cur = 100;
    party.members[0].set_hp_mp_sp(hms);

    let original_bytes = party.write();

    let mut world = World::new();
    world.load_party(party);
    let saved = world.save_party();

    assert_eq!(saved.write(), original_bytes);
}

#[test]
fn save_party_picks_up_in_battle_hp_changes() {
    let mut party = legaia_save::Party::zeroed(2);
    let mut hms = party.members[0].hp_mp_sp();
    hms.hp_cur = 100;
    hms.hp_max = 100;
    party.members[0].set_hp_mp_sp(hms);

    let mut world = World::new();
    world.load_party(party);
    // Simulate damage during battle.
    world.actors[0].battle.hp = 25;

    let saved = world.save_party();
    assert_eq!(saved.members[0].hp_mp_sp().hp_cur, 25);
    // Max HP unchanged.
    assert_eq!(saved.members[0].hp_mp_sp().hp_max, 100);
}

#[test]
fn load_party_caps_at_max_actors() {
    let many = legaia_save::Party::zeroed(MAX_ACTORS + 10);
    let mut world = World::new();
    world.load_party(many);
    assert_eq!(world.party.party_count, MAX_ACTORS as u8);
}

#[test]
fn save_full_round_trips_globals() {
    let mut world = World::new();
    world.load_party(legaia_save::Party::zeroed(2));
    world.flags.story_flags = 0xCAFE_F00D;
    world.party.money = 54321;
    world.party.inventory.insert(3, 9);
    world.party.inventory.insert(77, 1);

    let sf = world.save_full();
    assert_eq!(sf.ext.story_flags, 0xCAFE_F00D);
    assert_eq!(sf.ext.money, 54321);
    // inventory is sorted by item_id
    assert_eq!(sf.ext.inventory, vec![(3, 9), (77, 1)]);

    let bytes = sf.write();
    let parsed = legaia_save::SaveFile::parse(&bytes).unwrap();

    let mut world2 = World::new();
    world2.load_full(parsed);
    assert_eq!(world2.flags.story_flags, 0xCAFE_F00D);
    assert_eq!(world2.party.money, 54321);
    assert_eq!(world2.party.inventory.get(&3), Some(&9));
    assert_eq!(world2.party.inventory.get(&77), Some(&1));
    assert_eq!(world2.party.party_count, 2);
}

/// Casino coins, the Point Card bank and the fishing point record survive a
/// save / load - through the LGSF `LGX7` block and through a retail SC block,
/// where retail keeps all nine words in its live-state window.
#[test]
fn save_full_round_trips_the_minigame_purses() {
    let mut world = World::new();
    world.load_party(legaia_save::Party::zeroed(1));
    world.minigames.casino_coins = 4321;
    world.minigames.point_card = 765;
    world.minigames.fishing_points = 12_000;
    world.minigames.fishing_lure = 2;
    world.minigames.fishing_rod = 1;
    world.minigames.fishing_best_points = 480;
    world.minigames.fishing_best_fish = 9;
    world.minigames.fishing_casts = 55;
    world.minigames.fishing_prizes_purchased = 0x81;
    let check = |w: &World| {
        assert_eq!(w.minigames.casino_coins, 4321);
        assert_eq!(w.minigames.point_card, 765);
        assert_eq!(w.minigames.fishing_points, 12_000);
        assert_eq!(w.minigames.fishing_lure, 2);
        assert_eq!(w.minigames.fishing_rod, 1);
        assert_eq!(w.minigames.fishing_best_points, 480);
        assert_eq!(w.minigames.fishing_best_fish, 9);
        assert_eq!(w.minigames.fishing_casts, 55);
        assert_eq!(w.minigames.fishing_prizes_purchased, 0x81);
    };
    let sf = world.save_full();

    let mut from_file = World::new();
    from_file.load_full(legaia_save::SaveFile::parse(&sf.write()).unwrap());
    check(&from_file);

    let mut block = vec![0u8; legaia_save::card::BLOCK_SIZE];
    sf.write_into_retail_sc_block(&mut block).unwrap();
    assert_eq!(legaia_save::read_retail_coins(&block), Some(4321));
    let mut from_block = World::new();
    from_block.load_full(legaia_save::SaveFile::from_retail_sc_block(&block, 1).unwrap());
    check(&from_block);
}

#[test]
fn load_full_clears_old_inventory() {
    let mut world = World::new();
    world.party.inventory.insert(1, 10);
    world.party.inventory.insert(2, 20);

    let sf = legaia_save::SaveFile {
        party: legaia_save::Party::zeroed(1),
        ext: legaia_save::SaveExt {
            story_flags: 1,
            story_flag_bits: Vec::new(),
            money: 0,
            inventory: vec![(5, 3)],
            ..Default::default()
        },
        ext_v2: legaia_save::SaveExtV2::default(),
    };
    world.load_full(sf);
    assert!(!world.party.inventory.contains_key(&1));
    assert!(!world.party.inventory.contains_key(&2));
    assert_eq!(world.party.inventory.get(&5), Some(&3));
}
