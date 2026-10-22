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
fn step_name_entry_frame_owns_the_frame_only_while_the_prompt_is_open() {
    let mut world = World::new();
    let f0 = world.frame;
    assert!(!world.step_name_entry_frame(0));
    assert_eq!(world.frame, f0, "no prompt: nothing advances");
    world.open_name_entry(0);
    assert!(world.step_name_entry_frame(0));
    assert_eq!(world.frame, f0 + 1, "the caret clock advances");
    assert!(world.name_entry_active());
}
