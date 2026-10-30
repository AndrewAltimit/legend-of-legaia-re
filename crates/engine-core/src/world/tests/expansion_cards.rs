//! The trading-card economy on the `World` (`world::expansion_cards`), over
//! a synthetic pack: battle drops on the card stream (and the retail RNG left
//! alone), every grant path draining into the collection, the card screen's
//! buy / open / sell through `World::tick`, duel stakes both ways, and the
//! collection surviving the save block.

use super::*;
use legaia_engine_battle::monster_catalog::{
    FormationDef, FormationSlot, MonsterCatalog, MonsterDef,
};
use legaia_engine_expansion::cards::CardNotice;
use legaia_engine_expansion::modes::PendingCardReward;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::cards::screen::CardStage;
use legaia_engine_modes::session::Reward;
use legaia_engine_system::pad::PadButton;
use legaia_expansion_format::ExpansionPack;

const CARDS: &str = r#"
starter = ["a", "b", "c", "d", "e"]

[[cards]]
id = "a"
name = "Alpha"
sides = [1, 1, 1, 1]
art = { kind = "monster", id = 7 }

[[cards]]
id = "b"
name = "Beta"
sides = [2, 2, 2, 2]

[[cards]]
id = "c"
name = "Gamma"
sides = [3, 3, 3, 3]

[[cards]]
id = "d"
name = "Delta"
sides = [4, 4, 4, 4]

[[cards]]
id = "e"
name = "Epsilon"
rarity = "rare"
sides = [5, 5, 5, 5]

[[boosters]]
id = "pk"
name = "Pack"
cards_per_pack = 3
weights = { common = 3, rare = 1 }
price = 100

[[dealers]]
id = "shop"
name = "Test Dealer"
stock = ["pk"]
sell_percent = 50

[[rewards]]
source = { kind = "monster", id = 7 }
card = "a"

[[rewards]]
source = { kind = "monster", id = 8 }
booster = "pk"
chance_percent = 50
"#;

fn pack() -> std::sync::Arc<ExpansionPack> {
    let mut files = legaia_expansion_format::PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"t\"\nname = \"T\"\nversion = \"1.0.0\"\n".to_vec(),
    );
    files.insert("cards.toml".into(), CARDS.as_bytes().to_vec());
    std::sync::Arc::new(ExpansionPack::from_files(&files).expect("pack parses"))
}

fn fight_world() -> (World, FormationDef, MonsterCatalog) {
    let mut cat = MonsterCatalog::new();
    for id in [7, 8] {
        let mut def = MonsterDef::new(id, "M", 10, 5);
        def.drop_item = Some(0x42);
        def.drop_chance_pct = 50;
        cat.insert(def);
    }
    let formation = FormationDef::new(
        1000,
        vec![
            FormationSlot::new(7),
            FormationSlot::new(8),
            FormationSlot::new(8),
        ],
    );
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.rng_state = 0x1234_5678;
    (world, formation, cat)
}

#[test]
fn drops_roll_on_the_card_stream_and_leave_the_retail_rng_alone() {
    let (mut retail, formation, cat) = fight_world();
    let (mut packed, _, _) = fight_world();
    packed.activate_expansion(pack()).unwrap();
    let r1 = retail.apply_battle_loot(&formation, &cat);
    let r2 = packed.apply_battle_loot(&formation, &cat);
    assert_eq!(r1, r2, "the retail spoils are unchanged");
    assert_eq!(retail.rng_state, packed.rng_state, "and so is the RNG");
    assert_eq!(retail.party.inventory, packed.party.inventory);
    // Monster 7's rule is certain; 8's two seats each roll 50%.
    let cards = &packed.expansion.quests.progress.cards;
    assert_eq!(cards.collection.count("a"), 1);
    assert_ne!(cards.rng, 0, "the card stream was seeded");
    let notices = packed.take_card_notices();
    assert_eq!(notices[0].line(), "Got card: Alpha (new!)");
    assert!(
        notices[1..]
            .iter()
            .all(|n| matches!(n, CardNotice::Booster { .. }))
    );
    assert!(retail.take_card_notices().is_empty());
    assert_eq!(retail.expansion.quests.progress.cards, Default::default());
}

#[test]
fn drops_skip_a_no_reward_fight() {
    let (mut w, formation, cat) = fight_world();
    w.activate_expansion(pack()).unwrap();
    w.battle.special_word = 1;
    w.apply_battle_loot(&formation, &cat);
    assert_eq!(w.expansion.quests.progress.cards.collection.total(), 0);
}

#[test]
fn quest_and_mode_grants_drain_into_the_collection() {
    let mut w = World::default();
    // Without a pack the queues are kept.
    w.expansion.quests.card_inbox.push("a".into());
    w.collect_card_grants();
    assert_eq!(w.expansion.quests.card_inbox.len(), 1);
    w.activate_expansion(pack()).unwrap();
    w.expansion
        .pending_card_rewards
        .push(PendingCardReward::Booster {
            id: "pk".into(),
            from_mode: "m".into(),
        });
    w.collect_card_grants();
    let c = &w.expansion.quests.progress.cards;
    assert_eq!((c.collection.count("a"), c.booster_count("pk")), (1, 1));
    assert!(w.expansion.quests.card_inbox.is_empty());
    assert!(w.expansion.pending_card_rewards.is_empty());
}

#[test]
fn duel_stakes_move_cards_both_ways() {
    let mut w = World::default();
    w.activate_expansion(pack()).unwrap();
    w.expansion.quests.progress.cards.collection.add("e", 1);
    // The player took the opponent's card; then lost their own.
    w.grant_pack_mode_rewards(
        "duel",
        &[Reward::Card {
            seat: 0,
            card: "d".into(),
        }],
    );
    assert_eq!(w.expansion.quests.progress.cards.collection.count("d"), 1);
    w.grant_pack_mode_rewards(
        "duel",
        &[Reward::Card {
            seat: 1,
            card: "e".into(),
        }],
    );
    assert_eq!(w.expansion.quests.progress.cards.collection.count("e"), 0);
    let lines: Vec<String> = w.take_card_notices().iter().map(CardNotice::line).collect();
    assert_eq!(lines, ["Got card: Delta (new!)", "Lost card: Epsilon"]);
}

fn press(w: &mut World, b: PadButton) {
    w.input.set_pad(b.mask());
    w.tick();
    w.input.set_pad(0);
    w.tick();
}

#[test]
fn the_dealer_sells_boosters_that_open_into_the_collection() {
    let mut w = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    w.party.money = 250;
    assert!(!w.open_card_dealer("shop"), "no pack, no dealer");
    w.activate_expansion(pack()).unwrap();
    assert!(!w.open_card_dealer("nope"));
    assert!(w.open_card_dealer("shop"));
    assert!(w.card_screen_open());
    // Opening the cards hands over the starter deck.
    assert_eq!(w.expansion.quests.progress.cards.collection.total(), 5);
    w.tick();
    press(&mut w, PadButton::Cross);
    press(&mut w, PadButton::Cross);
    assert_eq!(w.party.money, 50);
    assert_eq!(w.expansion.quests.progress.cards.booster_count("pk"), 2);
    // Rows: Buy, Sell, Open boosters, Binder.
    press(&mut w, PadButton::Down);
    press(&mut w, PadButton::Down);
    press(&mut w, PadButton::Cross);
    press(&mut w, PadButton::Cross);
    let seat = w.expansion.cards.seat.as_ref().unwrap();
    assert!(matches!(seat.screen.stage, CardStage::Reveal { .. }));
    assert_eq!(w.expansion.quests.progress.cards.collection.total(), 8);
    // Back to the counter and sell one card at half value (common: 5 G).
    press(&mut w, PadButton::Triangle);
    press(&mut w, PadButton::Cross);
    press(&mut w, PadButton::Circle);
    press(&mut w, PadButton::Up);
    press(&mut w, PadButton::Cross);
    press(&mut w, PadButton::Cross);
    assert_eq!(w.expansion.quests.progress.cards.collection.total(), 7);
    assert!(w.party.money > 50);
    press(&mut w, PadButton::Start);
    assert_eq!(w.mode, SceneMode::Field);
    assert!(!w.card_screen_open());
}

#[test]
fn a_pack_duel_deals_the_collection_deck() {
    let mut w = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    let mut files = legaia_expansion_format::PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"t\"\nname = \"T\"\nversion = \"1.0.0\"\n".to_vec(),
    );
    files.insert("cards.toml".into(), CARDS.as_bytes().to_vec());
    files.insert(
        "modes.toml".into(),
        br#"
[[card_duels]]
id = "duel"
deck = ["a", "a", "a", "a", "a"]
stakes = true
"#
        .to_vec(),
    );
    w.activate_expansion(std::sync::Arc::new(
        ExpansionPack::from_files(&files).unwrap(),
    ))
    .unwrap();
    w.expansion.quests.progress.cards.collection.add("e", 5);
    w.enter_pack_mode("duel").unwrap();
    let seat = w.expansion.mode_seat.as_ref().unwrap();
    let legaia_engine_modes::active::ModeView::Duel(v) = seat.view().unwrap() else {
        panic!("a duel")
    };
    assert!(
        v.hands[0].iter().all(|c| c.id == "e"),
        "the strongest five owned"
    );
}

#[test]
fn the_collection_rides_the_save_block() {
    let mut w = World::default();
    assert_eq!(w.expansion_save_block(), None, "no pack, no block");
    w.activate_expansion(pack()).unwrap();
    let plain = w.expansion_save_block().unwrap();
    let c = &mut w.expansion.quests.progress.cards;
    c.collection.add("c", 2);
    c.boosters.insert("pk".into(), 1);
    c.deck = vec!["c".into(); 5];
    let block = w.expansion_save_block().unwrap();
    assert!(block.len() > plain.len());
    let mut back = World::default();
    back.activate_expansion(pack()).unwrap();
    back.expansion_load_block(Some(&block));
    assert_eq!(
        back.expansion.quests.progress.cards,
        w.expansion.quests.progress.cards
    );
}
