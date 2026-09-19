//! The **auto command string** across a round boundary.
//!
//! Retail parks a party member's confirmed 16-byte action queue in its own
//! save record (`FUN_801DA59C`) and loads it back the next time that member
//! attacks (`FUN_801DA34C`), picking between two record bands on the actor's
//! action gauge. These tests drive the two engine call sites - the arts commit
//! and the Attack dispatch - and check the string survives the turn between
//! them, which is the property the retail pair exists for.

use super::*;

/// A one-member party in battle with its character record installed.
fn auto_world() -> World {
    let mut w = World::new();
    while w.actors.len() < 3 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 1;
    w.load_party(legaia_save::Party::zeroed(1));
    w.mode = SceneMode::Battle;
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 500;
        w.actors[i].battle.max_hp = 500;
        w.actors[i].battle.liveness = 1;
    }
    w
}

/// A recognisable arts queue - the bytes are only ever copied, never decoded,
/// by the pair under test.
fn queue_bytes() -> Vec<u8> {
    vec![0x22, 0x26, 0x25, 0x22, 0x21]
}

/// Commit `queue` as member 0's arts action, which is where the write-back
/// sits.
fn commit_arts(w: &mut World, queue: &[u8]) {
    w.arm_battle_art_action(
        0,
        queue,
        &[],
        [0u32; vm::battle_action::ACTION_QUEUE_CAP],
        crate::target_picker::CursorRow::Enemy,
        0,
    );
}

#[test]
fn an_arts_commit_parks_the_queue_and_the_next_attack_replays_it() {
    use legaia_save::AutoCommandBand;
    let mut w = auto_world();
    let queue = queue_bytes();

    // Round 1: the player confirms an arts string.
    commit_arts(&mut w, &queue);
    let band = w.auto_command_band(0);
    let parked = w.party.roster.members[0].auto_command_string(band);
    assert_eq!(
        &parked[..queue.len()],
        &queue[..],
        "the arts commit must park the confirmed queue in the gauge-selected band"
    );
    assert_eq!(parked[queue.len()], 0, "the queue's terminator travels too");
    // Exactly one band is written - retail's write-back has no fallback leg.
    let other = match band {
        AutoCommandBand::Primary => AutoCommandBand::Secondary,
        AutoCommandBand::Secondary => AutoCommandBand::Primary,
    };
    assert_eq!(
        w.party.roster.members[0].auto_command_string(other),
        [0u8; legaia_save::AUTO_COMMAND_STRING_LEN],
        "the other band must be untouched"
    );

    // The round boundary: the actor's live stream is wiped the way a fresh
    // action arms it, so nothing but the record can carry the queue over.
    w.actors[0].battle.params = [0u8; vm::battle_action::ACTION_PARAM_BYTES];
    w.actors[0].battle.strike_index = 0;

    // Round 2: a plain Attack. The preseed wins over the no-input swing roll.
    w.dispatch_pending_party_action(
        0,
        crate::battle_round::PendingPartyAction::Attack { target: 1 },
    );
    assert_eq!(
        &w.actors[0].battle.params[..queue.len()],
        &queue[..],
        "the next Attack must replay the parked queue"
    );
}

#[test]
fn a_member_that_never_confirmed_a_string_takes_the_swing_roll() {
    let mut w = auto_world();
    w.dispatch_pending_party_action(
        0,
        crate::battle_round::PendingPartyAction::Attack { target: 1 },
    );
    // The no-input attack arm writes rolled swing commands, which the parked
    // string never contains (its bytes are art constants).
    let first = w.actors[0].battle.params[0];
    assert!(
        vm::battle_action::is_swing_command(first),
        "an empty record must fall through to the swing roll, got {first:#04x}"
    );
}

#[test]
fn the_gauge_picks_the_band_and_a_topped_up_gauge_reads_its_own() {
    use legaia_save::AutoCommandBand;
    let mut w = auto_world();

    // Base strictly below live: the primary band.
    w.actors[0].battle.agl = 120;
    w.actors[0].battle.agl_base = 100;
    assert_eq!(w.auto_command_band(0), AutoCommandBand::Primary);
    let topped = vec![0x31, 0x32];
    commit_arts(&mut w, &topped);
    assert_eq!(
        w.party.roster.members[0].auto_command_string(AutoCommandBand::Primary)[..2],
        topped[..]
    );

    // Gauge spent back to base: the secondary band, and it is still empty -
    // the two bands do not shadow each other.
    w.actors[0].battle.agl = 100;
    assert_eq!(w.auto_command_band(0), AutoCommandBand::Secondary);
    assert_eq!(
        w.party.roster.members[0].auto_command_string(AutoCommandBand::Secondary),
        [0u8; legaia_save::AUTO_COMMAND_STRING_LEN]
    );
    // ...so the secondary leg zero-fills rather than borrowing the primary
    // one, which is the asymmetry the retail reader has.
    w.actors[0].battle.params = [0xEE; vm::battle_action::ACTION_PARAM_BYTES];
    assert_eq!(w.preseed_auto_command_string(0), 0);
    assert_eq!(w.actors[0].battle.params[0], 0);

    // The primary leg, by contrast, does fall back to the secondary band.
    w.actors[0].battle.agl = 120;
    let spent = vec![0x41, 0x42, 0x43];
    w.party.roster.members[0].set_auto_command_string(AutoCommandBand::Primary, [0u8; 16]);
    let mut second = [0u8; 16];
    second[..spent.len()].copy_from_slice(&spent);
    w.party.roster.members[0].set_auto_command_string(AutoCommandBand::Secondary, second);
    assert_eq!(w.preseed_auto_command_string(0), spent.len());
    assert_eq!(&w.actors[0].battle.params[..spent.len()], &spent[..]);
}

#[test]
fn a_downed_member_writes_nothing_back() {
    let mut w = auto_world();
    let queue = queue_bytes();
    commit_arts(&mut w, &queue);
    let band = w.auto_command_band(0);
    w.party.roster.members[0].set_auto_command_string(band, [0u8; 16]);

    w.actors[0].battle.liveness = 0;
    assert!(
        !w.save_auto_command_string(0),
        "retail's `+0x14C == 0` guard refuses the write-back"
    );
    assert_eq!(
        w.party.roster.members[0].auto_command_string(band),
        [0u8; legaia_save::AUTO_COMMAND_STRING_LEN]
    );

    // ...and so does a member whose action category is not the attack/arts
    // band (retail's `+0x1DE != 3`).
    w.actors[0].battle.liveness = 1;
    w.actors[0].battle.action_category = 4;
    assert!(!w.save_auto_command_string(0));
}
