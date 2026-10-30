//! A party night played alone on the `World`: the built-in night entered
//! like a mode instance, its four rounds played against the AI rivals by
//! scripted pad input through `World::tick`, the prize for the player's
//! final place granted, and the field restored when the podium closes.
//!
//! Synthetic world, no disc gate, no Sony bytes.

use legaia_engine_core::world::World;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::active::{ModeKind, ModeView};
use legaia_engine_modes::fixed::angle_delta;
use legaia_engine_modes::party::PartyPhase;
use legaia_engine_system::pad::PadButton;

fn field_world() -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.minigames.casino_coins = 1000;
    w
}

/// A press every six frames from a random walk, Cross most often; in a race,
/// the throttle held and steered at the next checkpoint.
fn pad(w: &World, f: u32) -> u16 {
    if let Some(m) = w.expansion.mode_seat.as_deref().and_then(|s| s.active())
        && m.kind() == ModeKind::Race
        && let ModeView::Race(v) = m.view()
        && let Some(track) = m.race_track()
    {
        let r = &v.racers[0];
        let d = angle_delta(r.heading, (track.points[r.next_cp] - r.pos).angle());
        let steer = if d > 30 {
            PadButton::Right.mask()
        } else if d < -30 {
            PadButton::Left.mask()
        } else {
            0
        };
        return PadButton::Cross.mask() | steer;
    }
    if !f.is_multiple_of(6) {
        return 0;
    }
    let walk = [
        PadButton::Cross,
        PadButton::Cross,
        PadButton::Cross,
        PadButton::Up,
        PadButton::Down,
        PadButton::Left,
        PadButton::Right,
    ];
    let h = (f / 6).wrapping_mul(2_654_435_761);
    walk[(h >> 7) as usize % walk.len()].mask()
}

#[test]
fn a_night_played_alone_runs_every_round_and_grants_the_prize() {
    let mut w = field_world();
    w.enter_pack_mode("builtin-party")
        .expect("the built-in night opens");
    assert_eq!(w.mode, SceneMode::Expansion);
    assert_eq!(w.local_party().unwrap().night.players.len(), 4);
    let mut kinds = Vec::new();
    let mut f = 0u32;
    while !w.local_party().is_some_and(|l| l.night.completed()) {
        assert!(
            f < 60 * 60 * 40,
            "the night ends: {:?} {:?} {:?}",
            w.local_party()
                .map(|l| (&l.night.phase, l.night.history.len(), &l.night.hill)),
            w.expansion
                .mode_seat
                .as_deref()
                .map(|s| (s.view(), &s.log, s.held_over)),
            kinds
        );
        if let Some(k) = w
            .expansion
            .mode_seat
            .as_deref()
            .and_then(|s| s.active())
            .map(|m| m.kind())
            && kinds.last() != Some(&k)
        {
            kinds.push(k);
        }
        let p = pad(&w, f);
        w.input.set_pad(p);
        w.tick();
        f += 1;
    }
    for k in [
        ModeKind::Golf,
        ModeKind::Poker,
        ModeKind::Race,
        ModeKind::CardDuel,
    ] {
        assert!(kinds.contains(&k), "{k:?}: {kinds:?}");
    }
    let lp = w.local_party().unwrap();
    assert_eq!(lp.night.history.len(), 4);
    assert!(lp.night.history.iter().all(|r| !r.voided));
    // The poker round's chips were points only: the coin bank is untouched.
    assert_eq!(w.minigames.casino_coins, 1000);
    let place = lp
        .night
        .standings()
        .iter()
        .find(|s| s.key == 0)
        .unwrap()
        .place;
    let gold = [300, 150, 50, 0][usize::from(place - 1).min(3)];
    assert_eq!(w.party.money, gold, "place {place}");
    assert_eq!(lp.granted.as_ref().map(|g| g.is_empty()), Some(gold == 0));
    // The podium closes on a press, back to the field.
    for _ in 0..3 {
        w.input.set_pad(0);
        w.tick();
    }
    while !w.local_party().is_some_and(|l| l.night.is_over()) {
        w.input.set_pad(0);
        w.tick();
    }
    w.input.set_pad(PadButton::Cross.mask());
    w.tick();
    assert!(w.local_party().is_none());
    assert_eq!(w.mode, SceneMode::Field);
}

#[test]
fn start_on_the_party_card_ends_the_night_early_with_no_prize() {
    let mut w = field_world();
    w.enter_pack_mode("builtin-party").unwrap();
    w.input.set_pad(0);
    w.tick();
    w.input.set_pad(PadButton::Start.mask());
    w.tick();
    let lp = w.local_party().unwrap();
    assert_eq!(lp.night.phase, PartyPhase::Over { early: true });
    assert_eq!(w.party.money, 0);
    w.input.set_pad(0);
    w.tick();
    w.input.set_pad(PadButton::Cross.mask());
    w.tick();
    assert_eq!(w.mode, SceneMode::Field);
    assert!(w.expansion.mode_seat.is_none());
}
