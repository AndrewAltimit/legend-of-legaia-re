use legaia_engine_core::world::*;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_save_state::WorldSaveState;
use legaia_engine_world_views::passive_hud::WorldPassiveHud;

/// A field save reads the records, not the scene's actor slots. `opurud`'s
/// scripts re-stamp slots 1 and 2 from zeroed nodes; folding those mirrors
/// into the second and third records zeroed their pools, and the next load
/// took the zeroed records for unjoined members and re-seeded them from the
/// New Game template.
#[test]
fn a_field_save_keeps_the_records_over_the_scenes_actor_slots() {
    let mut party = legaia_save::Party::zeroed(3);
    for (i, rec) in party.members.iter_mut().enumerate() {
        let mut hms = rec.hp_mp_sp();
        hms.hp_max = 300 + i as u16;
        hms.hp_cur = 200 + i as u16;
        hms.mp_max = 50;
        hms.mp_cur = 40;
        rec.set_hp_mp_sp(hms);
    }
    let original = party.write();
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.load_party(party);
    // A scene placement takes slots 1 and 2 over from a zeroed node.
    for slot in 1..3 {
        world.actors[slot] = Default::default();
        world.actors[slot].active = true;
    }
    assert_eq!(world.save_party().write(), original);
    assert_eq!(world.save_full().party.write(), original);

    // In a battle the same slots are the party's seats and do fold back.
    world.mode = SceneMode::Battle;
    world.actors[1].battle.hp = 7;
    world.actors[1].battle.max_hp = 301;
    assert_eq!(world.save_party().members[1].hp_mp_sp().hp_cur, 7);
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
            legaia_engine_menus::new_game::GAME_STATE_COLD_RESET.brightness_ref,
            legaia_engine_menus::new_game::GAME_STATE_COLD_RESET.voice_volume
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
        legaia_engine_system::scus_leaf_kernels::TimedSoundArm::arm(0, 30, 0x6B)
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

/// The options screen's words a retail block carries (Field Move at
/// `0x800846CC`, SC `+0x58C`, and the rest of the window's rows) reach the
/// world's mirrors on a load, are offered to the host once, and go back out
/// on a save - into an LGSF file and into a freshly composed retail block.
#[test]
fn a_retail_saves_options_are_honoured_on_import() {
    use legaia_save::card::{self, RetailOptions};
    let mut block = vec![0u8; card::BLOCK_SIZE];
    World::new()
        .save_full()
        .write_into_retail_sc_block(&mut block)
        .unwrap();
    let set = RetailOptions {
        battle_camera: 2,
        select_attack: 1,
        field_move: 1,
        hp_display: 2,
        sound: 1,
        vibration_battles: 1,
        ..Default::default()
    };
    card::write_retail_options(&mut block, set).unwrap();
    let sf = legaia_save::SaveFile::from_retail_sc_block(&block, 4).unwrap();
    assert_eq!(sf.ext_v2.options, Some(set));

    let mut world = World::new();
    assert!(!world.locomotion.run_default);
    world.load_full(sf);
    assert!(
        world.locomotion.run_default,
        "a save made running resumes running"
    );
    assert_eq!(
        world.toggles.battle_camera,
        legaia_engine_system::option_values::BattleCameraOpt::Far
    );
    assert_eq!(
        world.toggles.field_hp_display,
        legaia_engine_system::option_values::HpDisplayOpt::DisplayOff
    );
    assert_eq!(
        world.take_loaded_options(),
        Some(set),
        "offered to the host"
    );
    assert_eq!(world.take_loaded_options(), None, "once");

    let out = world.save_full();
    assert_eq!(out.ext_v2.options, Some(set));
    let lgsf = legaia_save::SaveFile::parse(&out.write()).unwrap();
    assert_eq!(lgsf.ext_v2.options, Some(set));
    let mut fresh = vec![0u8; card::BLOCK_SIZE];
    out.write_into_retail_sc_block(&mut fresh).unwrap();
    assert_eq!(card::read_retail_options(&fresh), Some(set));

    // A live mirror is what a save writes: walking again saves Walk.
    world.locomotion.run_default = false;
    assert_eq!(world.save_full().ext_v2.options.unwrap().field_move, 0);
}

/// A loaded save's accessory passives are live in the field at once: the
/// derived ability mask is rebuilt from the loaded equipment rather than
/// left empty until the first battle entry re-derives it.
#[test]
fn a_loaded_save_rebuilds_the_accessory_passive_mask() {
    use legaia_engine_battle::accessory_passives::AccessoryPassives;
    use legaia_engine_field::field_passive_hud::ability_bit;
    let mut src = World::new();
    let mut party = legaia_save::Party::zeroed(1);
    let mut eq = party.members[0].equipment();
    eq.slots[7] = 0x50;
    party.members[0].set_equipment(eq);
    src.load_party(party);
    let sf = src.save_full();

    let mut world = World::new();
    world.set_accessory_passives(AccessoryPassives::from_entries(
        [(0x50, ability_bit::ENCOUNTER_LOW)],
        [],
    ));
    assert!(!world.party_has_ability(ability_bit::ENCOUNTER_LOW));
    world.load_full(sf);
    assert!(
        world.party_has_ability(ability_bit::ENCOUNTER_LOW),
        "the equipped accessory's passive is live after the load"
    );
    assert!(world.encounter_rate_modifiers().low_encounter);
    assert!(world.passive_hud_active(), "the badge column anchors");
}
