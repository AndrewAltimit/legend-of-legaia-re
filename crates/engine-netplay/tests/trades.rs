//! Card trades in a room: offers settled by the host move the cards on
//! both worlds' collections, or on neither.

use legaia_engine_modes::cards::TradeOffer;

use crate::harness::{Room, card_count, give_cards, lan};

fn one_for_one(give: &str, take: &str) -> TradeOffer {
    TradeOffer {
        give: vec![(give.into(), 1)],
        take: vec![(take.into(), 1)],
    }
}

/// A room of `n` whose players hold `cards[i]`, with every holdings report
/// relayed.
fn card_room(cards: &[&[(&str, u32)]]) -> Room {
    let mut room = Room::open(cards.len(), lan(), 21);
    for (p, c) in room.peers.iter_mut().zip(cards) {
        give_cards(&mut p.world, c);
        p.net.screen_open = false;
    }
    let n = cards.len();
    assert!(
        room.run_until(200, |r| r
            .peers
            .iter()
            .all(|p| p.net.trades.holdings.len() == n)),
        "every holdings report reaches every player"
    );
    room
}

#[test]
fn a_guest_and_the_host_swap_cards() {
    let mut room = card_room(&[&[("stone-imp", 2)], &[("kite-owl", 1)]]);
    let host = room.peers[0].net.me.unwrap();
    let guest = &mut room.peers[1];
    guest
        .net
        .offer_trade(&mut guest.world, host, one_for_one("kite-owl", "stone-imp"));
    assert!(room.run_until(60, |r| r.peers[0].net.trades.open.is_some()));
    let h = &mut room.peers[0];
    h.net.confirm_trade(&mut h.world);
    let ok = room.run_until(60, |r| card_count(&r.peers[1].world, "stone-imp") == 1);
    assert!(
        ok,
        "{:?} {:?} {:?}",
        room.peers[0].net.chat, room.peers[1].net.chat, room.peers[0].net.trades.holdings
    );
    assert_eq!(card_count(&room.peers[1].world, "kite-owl"), 0);
    assert_eq!(card_count(&room.peers[0].world, "stone-imp"), 1);
    assert_eq!(card_count(&room.peers[0].world, "kite-owl"), 1);
    for p in &room.peers {
        assert!(p.net.trades.open.is_none());
        assert!(
            p.net.chat.iter().any(|l| l.starts_with("Trade done")),
            "{:?}",
            p.net.chat
        );
    }
}

#[test]
fn two_guests_trade_through_the_host() {
    let mut room = card_room(&[&[], &[("ash-sprite", 1)], &[("lamp-wraith", 3)]]);
    let b = room.peers[2].net.me.unwrap();
    let a = &mut room.peers[1];
    a.net
        .offer_trade(&mut a.world, b, one_for_one("ash-sprite", "lamp-wraith"));
    assert!(room.run_until(60, |r| r.peers[2].net.trades.open.is_some()));
    let p = &mut room.peers[2];
    p.net.confirm_trade(&mut p.world);
    assert!(room.run_until(60, |r| card_count(&r.peers[1].world, "lamp-wraith") == 1));
    assert_eq!(card_count(&room.peers[1].world, "ash-sprite"), 0);
    assert_eq!(card_count(&room.peers[2].world, "ash-sprite"), 1);
    assert_eq!(card_count(&room.peers[2].world, "lamp-wraith"), 2);
    // The host held no part of it.
    assert_eq!(card_count(&room.peers[0].world, "ash-sprite"), 0);
}

#[test]
fn a_declined_or_impossible_offer_moves_nothing() {
    let mut room = card_room(&[&[("stone-imp", 1)], &[("kite-owl", 1)]]);
    let host = room.peers[0].net.me.unwrap();
    // Declined.
    let g = &mut room.peers[1];
    g.net
        .offer_trade(&mut g.world, host, one_for_one("kite-owl", "stone-imp"));
    assert!(room.run_until(60, |r| r.peers[0].net.trades.open.is_some()));
    let h = &mut room.peers[0];
    h.net.cancel_trade(&mut h.world);
    assert!(room.run_until(60, |r| r.peers.iter().all(|p| p.net.trades.open.is_none())));
    // Asking for a card the partner does not hold is refused at settle.
    let g = &mut room.peers[1];
    g.net
        .offer_trade(&mut g.world, host, one_for_one("kite-owl", "dune-beetle"));
    assert!(room.run_until(60, |r| r.peers[0].net.trades.open.is_some()));
    let h = &mut room.peers[0];
    h.net.confirm_trade(&mut h.world);
    assert!(room.run_until(60, |r| {
        r.peers[1]
            .net
            .chat
            .iter()
            .any(|l| l.starts_with("Trade refused"))
    }));
    for (i, (id, n)) in [("stone-imp", 1), ("kite-owl", 1)].into_iter().enumerate() {
        assert_eq!(card_count(&room.peers[i].world, id), n);
    }
}
