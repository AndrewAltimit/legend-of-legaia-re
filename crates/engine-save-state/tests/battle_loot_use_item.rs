use legaia_engine_core::world::*;
use legaia_engine_save_state::WorldSaveState;

#[test]
fn load_full_hydrates_level_up_tracker_from_record_levels() {
    // Build a 3-character save with levels 7, 12, 25.
    let mut party = legaia_save::Party::zeroed(3);
    party.members[0].set_level(7);
    party.members[1].set_level(12);
    party.members[2].set_level(25);
    let sf = legaia_save::SaveFile {
        party,
        ext: legaia_save::SaveExt::default(),
        ext_v2: legaia_save::SaveExtV2::default(),
    };
    let mut world = World::new();
    // Tracker defaults to 1 for every slot.
    assert_eq!(world.party.level_up_tracker.level[0], 1);
    world.load_full(sf);
    assert_eq!(world.party.level_up_tracker.level[0], 7);
    assert_eq!(world.party.level_up_tracker.level[1], 12);
    assert_eq!(world.party.level_up_tracker.level[2], 25);
}

#[test]
fn load_full_zero_level_record_clamps_to_one() {
    // Records that haven't had a level written (zero byte at +0x100)
    // shouldn't make the tracker think the slot is below L1.
    let party = legaia_save::Party::zeroed(2);
    let sf = legaia_save::SaveFile {
        party,
        ext: legaia_save::SaveExt::default(),
        ext_v2: legaia_save::SaveExtV2::default(),
    };
    let mut world = World::new();
    world.load_full(sf);
    assert_eq!(world.party.level_up_tracker.level[0], 1);
    assert_eq!(world.party.level_up_tracker.level[1], 1);
}
