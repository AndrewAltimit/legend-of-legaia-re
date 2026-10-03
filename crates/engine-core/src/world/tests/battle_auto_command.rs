//! The **auto command string** across a round boundary.
//!
//! Retail parks a party member's confirmed 16-byte action queue in its own
//! save record (`FUN_801DA59C`) and loads it back the next time that member
//! attacks or opens the arts entry (`FUN_801DA34C`), picking between two
//! record bands on the actor's action gauge. These tests drive the engine call
//! sites - the arts commit, the Attack dispatch and the entry open - and check
//! the string survives the turn between them, which is the property the retail
//! pair exists for.
//!
//! The string is the entered **arrows** as the gauge writes them (swing bytes
//! `0x0C..=0x0F`): the write-back runs at the input confirm, before the queue
//! builder tokenizes anything, so every replay goes back through the builder.

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

/// A recognisable window - the bytes are only ever copied, never decoded, by
/// the write-back leaf.
fn queue_bytes() -> Vec<u8> {
    vec![0x22, 0x26, 0x25, 0x22, 0x21]
}

/// Run retail's write-back leaf over `queue` as member 0's window, with the
/// arts category stamped - the state the input confirm leaves it in.
fn commit_arts(w: &mut World, queue: &[u8]) {
    w.actors[0].battle.params = [0u8; vm::battle_action::ACTION_PARAM_BYTES];
    w.actors[0].battle.params[..queue.len()].copy_from_slice(queue);
    w.actors[0].battle.action_category = 3;
    w.save_auto_command_string(0);
}

/// Entered arrows Up, Left, Right, Down as `Command::as_byte()` values, and
/// the swing bytes the gauge writes for them.
const ARROWS: [u8; 4] = [4, 1, 2, 3];
const SWINGS: [u8; 4] = [0x0F, 0x0C, 0x0D, 0x0E];

#[test]
fn an_arts_commit_parks_the_arrows_and_the_next_attack_replays_them() {
    let mut w = auto_world();
    w.run_battle_art(0, &ARROWS, crate::target_picker::CursorRow::Enemy, 0);
    let band = w.auto_command_band(0);
    let parked = w.party.roster.members[0].auto_command_string(band);
    assert_eq!(&parked[..4], &SWINGS, "the record holds the raw arrows");
    assert_eq!(parked[4], 0);

    w.actors[0].battle.params = [0u8; vm::battle_action::ACTION_PARAM_BYTES];
    w.dispatch_pending_party_action(
        0,
        crate::battle_round::PendingPartyAction::Attack { target: 1 },
    );
    // No art catalog: the builder turns the arrows into plain swings.
    assert_eq!(&w.actors[0].battle.params[..4], &SWINGS);
    assert_eq!(w.actors[0].battle.params[4], 0);
}

/// The entry opens on the parked string, and a bare confirm commits it
/// without charging a press.
#[test]
fn the_arts_entry_opens_on_the_parked_string_and_replays_it() {
    use crate::input::PadButton;
    let mut w = auto_world();
    w.run_battle_art(0, &ARROWS, crate::target_picker::CursorRow::Enemy, 0);
    w.actors[0].battle.params = [0u8; vm::battle_action::ACTION_PARAM_BYTES];

    w.open_arts_command_input(0);
    // The zeroed record has no AGL, so the entry seeds the default 100-AP
    // pool: three 30-AP arrows fit, and the gauge build (`FUN_801D388C`
    // case `0x2C`) cuts the window at the fourth, zeroing that byte.
    assert_eq!(
        &w.actors[0].battle.params[..3],
        &SWINGS[..3],
        "the window is preseeded as the entry opens (0x801D1734)"
    );
    assert_eq!(w.actors[0].battle.params[3], 0, "cut at 0x801D4DC4");
    let s = w.battle.arts_input.as_ref().expect("entry open");
    assert_eq!(s.preseed, ARROWS[..3]);
    let pool = s.pool;

    w.input.set_pad(0);
    w.input.set_pad(PadButton::Cross.mask());
    w.tick_battle_arts_input();
    let s = w.battle.arts_input.as_ref().expect("review up");
    assert!(s.replay);
    assert_eq!(s.pool, pool, "the replay charges no press");
    assert_eq!(s.committed_string(), &ARROWS[..3]);
}

#[test]
fn an_arts_window_parks_in_the_gauge_band_and_the_next_attack_reads_it() {
    use legaia_save::AutoCommandBand;
    let mut w = auto_world();
    let queue = queue_bytes();

    // Round 1: the write-back parks the window.
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

    // Round 2: the reader loads the window back verbatim.
    w.preseed_auto_command_string(0);
    assert_eq!(
        &w.actors[0].battle.params[..queue.len()],
        &queue[..],
        "the next read must load the parked window"
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
