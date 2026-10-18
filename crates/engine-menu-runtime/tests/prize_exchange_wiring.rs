//! The casino prize exchange (field-VM op `0x49` sub-op 7), wired end to
//! end at the model layer: the arm recognises the counter op and stages a
//! session over the installed prize table, the menu runtime drives the
//! session against the live coin bank / inventory / system flags, and the
//! exit flips the op-`0x49` park Armed -> Done so the counter script
//! resumes.
//!
//! The regression this file pins: sub-op 7 used to fall through
//! `slot_for_op49_sub_op` to the generic close tick - the trigger fired,
//! the park unparked, and no window ever opened at the koin1 / balden
//! prize counters.

use legaia_engine_core::world::World;
use legaia_engine_menu_runtime::menu_runtime::MenuRuntime;
use legaia_engine_menus::menu::MenuInput;
use legaia_engine_minigames::prize_exchange::{
    PRIZE_EXCHANGE_VISITED_FLAG, PrizeRecord, parse_blocks,
};

fn prize_world() -> World {
    let mut w = World::new();
    w.shops.prize_blocks = vec![
        vec![
            PrizeRecord {
                item_id: 0x10,
                gate: 0,
                price: 100,
            },
            PrizeRecord {
                item_id: 0x20,
                gate: 0x36,
                price: 500,
            },
        ],
        vec![PrizeRecord {
            item_id: 0x30,
            gate: 0,
            price: 25,
        }],
    ];
    w
}

fn press(menu: &mut MenuRuntime, w: &mut World, set: impl Fn(&mut MenuInput)) {
    let mut input = MenuInput::default();
    set(&mut input);
    menu.tick(w, input);
}

#[test]
fn sub_op_7_arms_a_session_over_the_scripted_block() {
    let mut w = prize_world();
    // koin1's counter op: `49 07 00` (block 0).
    assert!(w.try_arm_prize_exchange(&[0x49, 0x07, 0x00]));
    assert!(w.shops.prize_exchange_armed, "the op-0x49 gate armed");
    assert!(w.shops.prize_exchange_open);
    assert!(
        w.system_flag_test(PRIZE_EXCHANGE_VISITED_FLAG),
        "retail raises system flag 8 on entry (FUN_8003CE08(8))"
    );
    let session = w.take_pending_prize_exchange().expect("session staged");
    assert_eq!(session.rows().count(), 2, "block 0's two live rows");

    // balden's `49 07 01` selects block 1.
    let mut w = prize_world();
    assert!(w.try_arm_prize_exchange(&[0x49, 0x07, 0x01]));
    let session = w.take_pending_prize_exchange().unwrap();
    assert_eq!(session.rows().count(), 1);
    assert_eq!(session.selected().unwrap().item_id, 0x30);
}

#[test]
fn without_a_prize_table_the_counter_refuses_rather_than_inventing_stock() {
    let mut w = World::new();
    assert!(!w.try_arm_prize_exchange(&[0x49, 0x07, 0x00]));
    assert!(!w.shops.prize_exchange_armed);
    // Out-of-range block index on an installed table.
    let mut w = prize_world();
    assert!(!w.try_arm_prize_exchange(&[0x49, 0x07, 0x07]));
}

/// The full player path: redeem a prize through the Yes/No prompt, then
/// leave - coins debited, item granted, one-shot gate raised, park flipped
/// to Done.
#[test]
fn redeem_and_exit_through_the_menu_runtime() {
    let mut w = prize_world();
    w.minigames.casino_coins = 1_000;
    assert!(w.try_arm_prize_exchange(&[0x49, 0x07, 0x00]));
    let session = w.take_pending_prize_exchange().unwrap();

    let mut menu = MenuRuntime::new(std::env::temp_dir());
    menu.open_prize_exchange(session);
    assert!(menu.is_open(), "the prize session holds the menu open");

    // Move to the gated 500-coin prize and confirm.
    press(&mut menu, &mut w, |i| i.down = true);
    press(&mut menu, &mut w, |i| i.cross = true); // opens Yes/No (No default)
    press(&mut menu, &mut w, |i| i.up = true); // onto Yes
    press(&mut menu, &mut w, |i| i.cross = true); // commit

    assert_eq!(
        w.minigames.casino_coins, 500,
        "price debited from the coin bank"
    );
    assert_eq!(w.party.inventory.get(&0x20), Some(&1), "prize granted");
    assert!(
        w.system_flag_test(0x36),
        "the one-shot gate flag raised on commit"
    );
    // The one-shot prize is gone from the rebuilt list.
    assert_eq!(
        menu.prize_session.as_ref().unwrap().rows().count(),
        1,
        "the redeemed one-shot prize left the list"
    );

    // Cancel out: the session drops and the park flips Armed -> Done.
    assert!(w.shops.prize_exchange_open);
    press(&mut menu, &mut w, |i| i.circle = true);
    assert!(
        menu.prize_session.is_none(),
        "browse cancel ends the session"
    );
    assert!(!menu.is_open());
    assert!(
        !w.shops.prize_exchange_open,
        "finish_prize_exchange ran - the counter script resumes"
    );
    assert!(
        w.shops.prize_exchange_armed,
        "the arm clears on the VM's resume"
    );
}

/// The counter's cues at the surface both hosts key from:
/// `MenuRuntime::step_field_session` returns the frame's cue, and each host
/// hands it to its blip path. A visit - step, a redeem the coin bank cannot
/// cover, the prompt, No, the prompt, Yes, leave - reads as retail's ring:
/// `0x21`, the buzz `0x23`, the list confirm `0x36`, silence, `0x36`, the
/// step onto Yes, the coin jingle `0x25`, the cancel `0x37`.
#[test]
fn the_counter_hands_the_hosts_its_cues() {
    use legaia_engine_system::input::PadButton as B;
    let mut w = prize_world();
    w.minigames.casino_coins = 0;
    assert!(w.try_arm_prize_exchange(&[0x49, 0x07, 0x00]));
    let session = w.take_pending_prize_exchange().unwrap();
    let mut menu = MenuRuntime::new(std::env::temp_dir());
    menu.open_prize_exchange(session);

    let mut cues = Vec::new();
    let mut edge = |menu: &mut MenuRuntime, w: &mut World, b: B| {
        cues.push(menu.step_field_session(w, b as u16));
        // An edge-free tick between presses raises nothing.
        assert_eq!(menu.step_field_session(w, 0), None);
    };
    edge(&mut menu, &mut w, B::Down);
    edge(&mut menu, &mut w, B::Cross); // 500-coin prize, empty bank: buzz
    w.minigames.casino_coins = 1_000;
    edge(&mut menu, &mut w, B::Cross); // prompt
    edge(&mut menu, &mut w, B::Cross); // No (the default row)
    edge(&mut menu, &mut w, B::Cross); // prompt
    edge(&mut menu, &mut w, B::Up); // onto Yes
    edge(&mut menu, &mut w, B::Cross); // commit
    assert_eq!(w.minigames.casino_coins, 500);
    cues.push(menu.step_field_session(&mut w, B::Circle as u16)); // leave
    assert!(!menu.is_open());
    assert_eq!(
        cues,
        vec![
            Some(0x21),
            Some(0x23),
            Some(0x36),
            None,
            Some(0x36),
            Some(0x21),
            Some(0x25),
            Some(0x37),
        ]
    );
}

/// The field-overlay poll both play hosts run once per sim tick
/// (`MenuRuntime::poll_field_overlay`): nothing pending answers `None` and
/// opens nothing; a counter the script armed opens on the poll and takes its
/// first step there, on no edge, so the press that ran the script into op
/// `0x49` cannot also act on the screen. The request is consumed by the poll.
#[test]
fn the_overlay_poll_opens_and_steps_on_no_edge() {
    let mut w = prize_world();
    let mut menu = MenuRuntime::new(std::env::temp_dir());
    assert_eq!(menu.poll_field_overlay(&mut w), None);
    assert!(!menu.is_open());

    assert!(w.try_arm_prize_exchange(&[0x49, 0x07, 0x00]));
    assert_eq!(
        menu.poll_field_overlay(&mut w),
        Some(None),
        "opened, and the no-edge first step raised no cue"
    );
    assert!(menu.is_open());
    assert!(
        w.shops.prize_exchange_open,
        "the first step did not unpark the counter under an open screen"
    );
    assert_eq!(
        menu.prize_session.as_ref().map(|s| s.cursor()),
        Some(0),
        "no edge reached the list"
    );
    // The request was taken: a second poll opens nothing.
    assert_eq!(menu.poll_field_overlay(&mut w), None);
    assert!(menu.is_open());
}

/// A refused redeem (short coins) leaves everything untouched and the
/// session browsing.
#[test]
fn a_short_coin_bank_refuses_without_mutation() {
    let mut w = prize_world();
    w.minigames.casino_coins = 10;
    assert!(w.try_arm_prize_exchange(&[0x49, 0x07, 0x00]));
    let session = w.take_pending_prize_exchange().unwrap();
    let mut menu = MenuRuntime::new(std::env::temp_dir());
    menu.open_prize_exchange(session);
    press(&mut menu, &mut w, |i| i.cross = true);
    assert_eq!(w.minigames.casino_coins, 10);
    assert!(w.party.inventory.is_empty());
    assert!(
        menu.prize_session.is_some(),
        "still browsing after the buzz"
    );
}

/// The block parser reads the retail table shape: 4 x 0x60-byte blocks of
/// 8-byte `[u16 id][u16 gate][u32 price]` records at file `0x15D00`.
#[test]
fn parse_blocks_reads_the_table_at_its_file_offset() {
    let mut overlay = vec![0u8; 0x15D00 + 4 * 0x60];
    // Block 0 record 0: id 0xD0, gate 0x36, price 5000.
    let base = 0x15D00;
    overlay[base..base + 2].copy_from_slice(&0xD0u16.to_le_bytes());
    overlay[base + 2..base + 4].copy_from_slice(&0x36u16.to_le_bytes());
    overlay[base + 4..base + 8].copy_from_slice(&5000u32.to_le_bytes());
    // Block 1 record 0: id 0x11, no gate, price 3.
    let b1 = base + 0x60;
    overlay[b1..b1 + 2].copy_from_slice(&0x11u16.to_le_bytes());
    overlay[b1 + 4..b1 + 8].copy_from_slice(&3u32.to_le_bytes());

    let blocks = parse_blocks(&overlay).expect("in range");
    assert_eq!(blocks.len(), 4);
    assert_eq!(blocks[0].len(), 12, "0x60 / 8 records per block");
    assert_eq!(
        blocks[0][0],
        PrizeRecord {
            item_id: 0xD0,
            gate: 0x36,
            price: 5000
        }
    );
    assert_eq!(blocks[1][0].item_id, 0x11);
    // A short (non-extended) read yields no table at all.
    assert!(parse_blocks(&overlay[..0x15D00]).is_none());
}

/// The engine bag's `RedeemBag` impl: `apply_redeem` against a real
/// `ItemBag` grants one copy on top of what is held, and the `< 0x63`
/// held-count gate reads the bag's count.
#[test]
fn apply_redeem_grants_into_the_engine_bag() {
    use legaia_engine_menus::item_bag::ItemBag;
    use legaia_engine_minigames::prize_exchange::apply_redeem;

    let mut bag = ItemBag::new();
    bag.insert(0x20, 2);
    let mut coins = 900u32;
    let mut set = Vec::new();
    assert!(apply_redeem(
        &mut coins,
        &mut bag,
        &mut |g| set.push(g),
        0x20,
        500,
        0x36
    ));
    assert_eq!(coins, 400);
    assert_eq!(bag.get(&0x20), Some(&3));
    assert_eq!(set, vec![0x36]);

    // At the cap the gate refuses and nothing moves.
    bag.insert(0x30, 0x63);
    let mut coins = 900u32;
    assert!(!apply_redeem(&mut coins, &mut bag, &mut |_| {}, 0x30, 1, 0));
    assert_eq!(coins, 900);
    assert_eq!(bag.get(&0x30), Some(&0x63));
}
