//! The Rebirth pack's `cards.toml` and `modes.toml` parse and every
//! instance converts into a session config.

use std::collections::BTreeSet;

use legaia_engine_modes::cards::duel::DuelConfig;
use legaia_engine_modes::cards::{CardBook, Collection};
use legaia_engine_modes::golf::GolfConfig;
use legaia_engine_modes::poker::PokerConfig;
use legaia_engine_modes::prop_hunt::PropHuntConfig;
use legaia_engine_modes::racing::RaceConfig;
use legaia_engine_modes::session::ModeRng;
use legaia_expansion_format::refs::DiscRef;

use crate::common::{rebirth_cards, rebirth_modes};

#[test]
fn rebirth_cards_are_well_formed() {
    let s = rebirth_cards();
    assert!(s.cards.len() >= 20, "{} cards", s.cards.len());
    let ids: BTreeSet<&str> = s.cards.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), s.cards.len(), "duplicate card id");
    for c in &s.cards {
        assert!(c.sides.iter().all(|&v| (1..=10).contains(&v)), "{}", c.id);
        assert!(matches!(c.art, Some(DiscRef::Monster { .. })), "{}", c.id);
        assert!(!c.name.is_empty() && !c.flavor.is_empty());
    }
    let book = CardBook::new(s.clone());
    let mut rng = ModeRng::new(1);
    let mut col = Collection::new();
    for b in &s.boosters {
        for id in &b.pool {
            assert!(ids.contains(id.as_str()), "booster {} names {id}", b.id);
        }
        for id in book.open_booster(&b.id, &mut rng).unwrap() {
            col.add(&id, 1);
        }
    }
    for r in &s.rewards {
        if let Some(b) = &r.booster {
            assert!(book.booster(b).is_some(), "reward names booster {b}");
        }
    }
    assert!(col.total() > 0);
}

#[test]
fn rebirth_modes_convert_to_sessions() {
    let m = rebirth_modes();
    let book = CardBook::new(rebirth_cards());
    assert!(!m.poker.is_empty());
    let p = PokerConfig::from_table(&m.poker[0], 0);
    assert_eq!(p.seats.len(), 4);
    for t in &m.poker {
        let p = PokerConfig::from_table(t, 0);
        assert_eq!(p.seats.len(), usize::from(t.seats), "{}", t.id);
    }
    let deck: Vec<String> = book.section().cards[..5]
        .iter()
        .map(|c| c.id.clone())
        .collect();
    for d in &m.card_duels {
        DuelConfig::from_duel(d, &book, &deck, 0).unwrap_or_else(|e| panic!("{}: {e:?}", d.id));
    }
    let g = GolfConfig::from_course(&m.golf[0], &[], 0).unwrap();
    assert_eq!(g.holes.len(), 3);
    // A course or track that names a pack map builds from the map
    // (`rebirth_map_courses_build_from_their_maps`).
    for c in m.golf.iter().filter(|c| c.map.is_none()) {
        let g = GolfConfig::from_course(c, &[], 0).unwrap_or_else(|e| panic!("{}: {e:?}", c.id));
        assert_eq!(g.holes.len(), c.holes.len(), "{}", c.id);
    }
    for r in m.races.iter().filter(|r| r.map.is_none()) {
        RaceConfig::from_def(r, 0).unwrap_or_else(|e| panic!("{}: {e:?}", r.id));
    }
    for a in &m.prop_hunt {
        let ph = PropHuntConfig::from_arena(a, 0).unwrap_or_else(|e| panic!("{}: {e:?}", a.id));
        for d in &ph.decoys {
            assert!(ph.arena.open(d.pos), "{}: decoy inside a wall", a.id);
        }
    }
}

/// Every Rebirth instance that names a tower map builds against the pack's
/// maps, as the mode seat builds it.
#[test]
fn rebirth_map_courses_build_from_their_maps() {
    use legaia_engine_modes::active::{ActiveMode, BuildOptions};
    let pack = legaia_expansion_format::ExpansionPack::from_dir(&crate::common::pack_dir())
        .expect("the Rebirth pack parses");
    let ids: Vec<&str> = (pack
        .modes
        .golf
        .iter()
        .filter(|c| c.map.is_some())
        .map(|c| c.id.as_str()))
    .chain(
        pack.modes
            .races
            .iter()
            .filter(|r| r.map.is_some())
            .map(|r| r.id.as_str()),
    )
    .collect();
    assert!(!ids.is_empty(), "Rebirth lays a course on its tower");
    for id in ids {
        ActiveMode::build_with_maps(
            &pack.modes,
            &pack.cards,
            &pack.maps,
            id,
            3,
            &BuildOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{id}: {e}"));
    }
}

/// Tower Game Night: every round resolves against Rebirth's instances and
/// builds (its map rounds on the tower's maps), for a night alone and for a
/// full room.
#[test]
fn rebirth_tower_game_night_builds_every_round() {
    use legaia_engine_modes::active::{ActiveMode, BuildOptions, ModeKind};
    use legaia_engine_modes::party::{PartyNight, round_section, solo_players};
    let pack = legaia_expansion_format::ExpansionPack::from_dir(&crate::common::pack_dir())
        .expect("the Rebirth pack parses");
    let def = pack
        .modes
        .party_night("tower-game-night")
        .expect("Rebirth hosts a party night");
    let players = solo_players(def, "You");
    assert_eq!(players.len(), 4);
    assert_eq!(players[1].1, "Keeper Odalric");
    let night = PartyNight::new(def, &pack.modes, &players, 5).unwrap();
    assert_eq!(night.total_rounds(), 6);
    let kinds: BTreeSet<ModeKind> = night.infos.iter().map(|i| i.kind).collect();
    assert_eq!(kinds.len(), 5, "every kind of game: {kinds:?}");
    for seats in [2, 4] {
        for r in &night.rounds {
            let section = round_section(&pack.modes, r, seats).unwrap();
            ActiveMode::build_with_maps(
                &section,
                &pack.cards,
                &pack.maps,
                &r.mode,
                1,
                &BuildOptions::default(),
            )
            .unwrap_or_else(|e| panic!("{} for {seats}: {e}", r.mode));
        }
    }
}
