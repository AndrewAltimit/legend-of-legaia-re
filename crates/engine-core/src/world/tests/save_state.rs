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
    assert_eq!(world.party_count, 3);
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
    assert_eq!(world.party_count, MAX_ACTORS as u8);
}

#[test]
fn save_full_round_trips_globals() {
    let mut world = World::new();
    world.load_party(legaia_save::Party::zeroed(2));
    world.story_flags = 0xCAFE_F00D;
    world.money = 54321;
    world.inventory.insert(3, 9);
    world.inventory.insert(77, 1);

    let sf = world.save_full();
    assert_eq!(sf.ext.story_flags, 0xCAFE_F00D);
    assert_eq!(sf.ext.money, 54321);
    // inventory is sorted by item_id
    assert_eq!(sf.ext.inventory, vec![(3, 9), (77, 1)]);

    let bytes = sf.write();
    let parsed = legaia_save::SaveFile::parse(&bytes).unwrap();

    let mut world2 = World::new();
    world2.load_full(parsed);
    assert_eq!(world2.story_flags, 0xCAFE_F00D);
    assert_eq!(world2.money, 54321);
    assert_eq!(world2.inventory.get(&3), Some(&9));
    assert_eq!(world2.inventory.get(&77), Some(&1));
    assert_eq!(world2.party_count, 2);
}

#[test]
fn load_full_clears_old_inventory() {
    let mut world = World::new();
    world.inventory.insert(1, 10);
    world.inventory.insert(2, 20);

    let sf = legaia_save::SaveFile {
        party: legaia_save::Party::zeroed(1),
        ext: legaia_save::SaveExt {
            story_flags: 1,
            story_flag_bits: Vec::new(),
            money: 0,
            inventory: vec![(5, 3)],
        },
        ext_v2: legaia_save::SaveExtV2::default(),
    };
    world.load_full(sf);
    assert!(!world.inventory.contains_key(&1));
    assert!(!world.inventory.contains_key(&2));
    assert_eq!(world.inventory.get(&5), Some(&3));
}
