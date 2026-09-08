//! The pause menu's ailment ink has a live input.
//!
//! Retail's menu HP colour fn `FUN_800349EC` re-inks the number gold when the
//! character record's `+0x12E` battle-status halfword is non-zero (read as
//! `lh v0,0x6f6(v1)` off `0x80084140 + slot*0x414`; see
//! `docs/formats/save-record.md`). The port's equivalent latch is
//! `World::status_effects`, whose `display_flags` packs the same bit word that
//! retail's `FUN_80047430` mirrors into the record every frame a party actor
//! ticks - and it is never cleared, so an ailment walks out of the battle the
//! way retail's does.
//!
//! This pins the carrier: `field_menu_dispatch::status_snapshots` must publish
//! that word on every snapshot, because both hosts fill their view structs
//! from it (`engine-shell`'s `menu_draws` and `web-viewer`'s `play_menu`), and
//! `engine-ui::menu_hp_ink_with_status` is what consumes it.
//!
//! Disc-free: the roster is seeded synthetically.

use legaia_engine_core::world::World;
use legaia_engine_vm::status_effects::StatusKind;

/// A world with one roster member carrying live HP, so `status_snapshots`
/// emits a row for it (rows with `hp_max == 0` are filtered out).
fn world_with_one_member() -> World {
    let mut w = World::default();
    w.party_count = 1;
    // `World::default()` starts with an empty roster; seed one claimed slot.
    w.roster
        .members
        .push(legaia_save::CharacterRecord::zeroed());
    let m = &mut w.roster.members[0];
    m.set_hp_mp_sp(legaia_save::HpMpSp {
        hp_cur: 120,
        hp_max: 120,
        mp_cur: 20,
        mp_max: 20,
        sp_cur: 0,
        sp_max: 0,
    });
    w
}

#[test]
fn snapshots_publish_a_clean_party_as_status_zero() {
    let w = world_with_one_member();
    let snaps = legaia_engine_core::field_menu_dispatch::status_snapshots(&w);
    assert!(
        !snaps.is_empty(),
        "expected one snapshot for the seeded member"
    );
    assert_eq!(
        snaps[0].status_flags, 0,
        "a party with no tracked effect must publish a zero +0x12E word"
    );
}

#[test]
fn an_applied_ailment_reaches_the_snapshot_status_word() {
    let mut w = world_with_one_member();
    w.status_effects.apply(0, StatusKind::Toxic);

    let snaps = legaia_engine_core::field_menu_dispatch::status_snapshots(&w);
    assert!(
        !snaps.is_empty(),
        "expected one snapshot for the seeded member"
    );
    let flags = snaps[0].status_flags;
    assert_ne!(
        flags, 0,
        "an applied ailment must publish a non-zero +0x12E word - this is the \
         input `menu_hp_ink_with_status`'s ailment arm reads"
    );
    assert_eq!(
        flags,
        w.status_effects.display_flags(0),
        "the snapshot must carry the tracker's own packed word verbatim"
    );
}
