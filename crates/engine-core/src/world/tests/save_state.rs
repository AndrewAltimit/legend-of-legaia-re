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

// --- Retail card-load fidelity ---------------------------------------

/// A retail-shaped SC block: the New Game template's four populated
/// records, a one-member present party, flags either side of the old
/// `0x540` cut, a field position.
fn vahn_alone_sc_block() -> Vec<u8> {
    use legaia_save::card::*;
    let mut block = vec![0u8; legaia_save::BLOCK_SIZE];
    block[..legaia_save::SAVE_BLOCK_HEADER.len()].copy_from_slice(&legaia_save::SAVE_BLOCK_HEADER);
    let records: Vec<Vec<u8>> = (0..4u8)
        .map(|i| {
            let mut r = legaia_save::CharacterRecord::zeroed();
            r.raw[0] = i + 1;
            r.raw.to_vec()
        })
        .collect();
    write_retail_char_records(&mut block, &records).unwrap();
    for f in [0x0010u16, 0x053F, 0x0540, 0x05B3, 0x06C4, 0x0FFF] {
        block[0x1618 + usize::from(f >> 3)] |= 0x80 >> (f & 7);
    }
    write_retail_present_party(&mut block, &[0]).unwrap();
    write_retail_field_position(&mut block, (0x0E40, 0x2DC0)).unwrap();
    block
}

#[test]
fn a_card_load_seeds_the_whole_system_flag_bank() {
    let sf = legaia_save::SaveFile::from_retail_sc_block(
        &vahn_alone_sc_block(),
        legaia_save::RETAIL_SC_PARTY_RECORDS,
    )
    .unwrap();
    let mut world = World::new();
    world.load_full(sf);
    for f in [0x0010u16, 0x053F, 0x0540, 0x05B3, 0x06C4, 0x0FFF] {
        assert!(world.system_flag_test(f), "flag {f:#05X} lost on load");
    }
    assert!(!world.system_flag_test(0x0541));
}

#[test]
fn a_card_load_seats_the_saved_present_party_not_every_record() {
    let sf = legaia_save::SaveFile::from_retail_sc_block(
        &vahn_alone_sc_block(),
        legaia_save::RETAIL_SC_PARTY_RECORDS,
    )
    .unwrap();
    let mut world = World::new();
    world.load_full(sf);
    assert_eq!(world.party.roster.members.len(), 4);
    assert_eq!(world.party.party_count, 1, "Vahn alone");
    assert_eq!(world.party.active_party, vec![0]);
    assert_eq!(world.party.party_actor_slots, vec![Some(0)]);
    assert_eq!(world.party.party_leader_slot, Some(0));
}

#[test]
fn save_full_keeps_the_party_count_and_a_cleared_flag_stays_cleared() {
    let sf = legaia_save::SaveFile::from_retail_sc_block(
        &vahn_alone_sc_block(),
        legaia_save::RETAIL_SC_PARTY_RECORDS,
    )
    .unwrap();
    let mut world = World::new();
    world.load_full(sf);
    // The game clears a loaded flag; the next save must not resurrect it
    // from the bytes the load came from.
    world.system_flag_clear(0x06C4);
    world.system_flag_set(0x0700);
    let saved = world.save_full();
    let mut again = World::new();
    again.load_full(saved);
    assert!(!again.system_flag_test(0x06C4), "cleared flag resurrected");
    assert!(again.system_flag_test(0x0700));
    assert!(again.system_flag_test(0x0FFF));
    assert_eq!(again.party.party_count, 1);
}

#[test]
fn an_identity_party_below_the_roster_saves_as_a_prefix() {
    let mut world = World::new();
    world.load_party(legaia_save::Party::zeroed(4));
    world.party.party_count = 2;
    let saved = world.save_full();
    assert_eq!(saved.ext_v2.active_party, vec![0, 1]);
    let mut again = World::new();
    again.load_full(saved);
    assert_eq!(again.party.party_count, 2);
    // Every record in the party is still the historical identity encoding.
    world.party.party_count = 4;
    assert_eq!(world.save_full().ext_v2.active_party, vec![0, 1, 2, 3]);
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

/// The audio-level pair a retail block carries (`0x8008457C` configured
/// level, `0x80084580` voice volume) reaches the world on import, drives the
/// consumers that key off it, and goes back out on a save - into an LGSF
/// file and into a freshly composed retail block alike.
#[test]
fn a_retail_saves_audio_levels_are_honoured_on_import() {
    use legaia_save::card::{self, RetailAudioLevels};
    let cold = RetailAudioLevels::COLD_RESET;
    assert_eq!(
        (cold.configured_level, cold.voice_volume),
        (
            crate::new_game::GAME_STATE_COLD_RESET.brightness_ref,
            crate::new_game::GAME_STATE_COLD_RESET.voice_volume
        ),
        "the save crate's cold-reset pair is FUN_8001FFA4's"
    );
    let mut world = World::new();
    assert_eq!(world.audio.levels, cold, "a cold boot holds the reset pair");

    // A block written by retail with a player-lowered pair.
    let mut block = vec![0u8; card::BLOCK_SIZE];
    World::new()
        .save_full()
        .write_into_retail_sc_block(&mut block)
        .unwrap();
    let set = RetailAudioLevels {
        configured_level: 0x6B,
        voice_volume: 90,
    };
    card::write_retail_audio_levels(&mut block, set).unwrap();
    let sf = legaia_save::SaveFile::from_retail_sc_block(&block, 4).unwrap();
    assert_eq!(sf.ext_v2.audio_levels, Some(set));
    world.load_full(sf);
    assert_eq!(world.audio.levels, set, "the import installs the pair");

    // Consumers: the sound-release arm latches the level the MAN loader
    // rests the live cell on.
    world.arm_sound_release(30);
    let arm = world.audio.sound_arm.expect("armed");
    assert_eq!(
        arm,
        crate::scus_leaf_kernels::TimedSoundArm::arm(0, 30, 0x6B)
    );

    // And it goes back out: LGSF and a fresh retail block.
    let out = world.save_full();
    assert_eq!(out.ext_v2.audio_levels, Some(set));
    let lgsf = legaia_save::SaveFile::parse(&out.write()).unwrap();
    assert_eq!(lgsf.ext_v2.audio_levels, Some(set));
    let mut fresh = vec![0u8; card::BLOCK_SIZE];
    out.write_into_retail_sc_block(&mut fresh).unwrap();
    assert_eq!(card::read_retail_audio_levels(&fresh), Some(set));

    // A save naming no pair keeps the live one.
    let mut none = out.clone();
    none.ext_v2.audio_levels = None;
    world.audio.levels = cold;
    world.load_full(none);
    assert_eq!(world.audio.levels, cold);
}
