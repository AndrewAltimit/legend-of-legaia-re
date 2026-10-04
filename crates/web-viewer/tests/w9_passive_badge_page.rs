//! Page ladder for the field overlay's **passive-ability badge column**
//! (`FUN_801D095C`): the icons floated over the player's head while an
//! equipped accessory grants one of six passive bits, anchored by
//! `field_passive_hud::hud_anchor_offsets`.
//!
//! The gate is the party's passive mask, which retail rebuilds from
//! equipment (`FUN_800431D0`), and a cold-start party wears none of the six -
//! so the column never anchors on any ladder that keeps the starting gear.
//! This one equips the accessory the way a player does, through the pause
//! menu's Equip screen by pad, after seeding one copy into the bag (a shop
//! purchase's worth). Which item grants the bit is read off the visitor's own
//! executable (`legaia_asset::accessory_passive`), not written down here.
//!
//! The assertion is the badge itself, by contrast: the same menu walk with
//! nothing seeded is the control, so whatever the field HUD draws after a
//! menu closes, it draws in both runs, and only the badge separates them.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::input::PadButton;
use legaia_engine_vm::field_passive_hud::ability_bit;
use legaia_web_viewer::runtime::LegaiaRuntime;

const W: u32 = 960;
const H: u32 = 720;

fn tick(rt: &mut LegaiaRuntime, n: usize) {
    for _ in 0..n {
        rt.tick_frame().expect("tick_frame");
    }
}

fn field_texts(rt: &mut LegaiaRuntime) -> usize {
    let v: serde_json::Value =
        serde_json::from_str(&rt.play_overlay_draws_json(W, H)).unwrap_or_default();
    v["texts"].as_array().map_or(0, |a| a.len())
}

fn menu(rt: &mut LegaiaRuntime, edge: PadButton) {
    rt.play_menu_input(edge.mask());
    let _ = rt.play_menu_draws_json(320, 240);
}

/// Boot `town01`, optionally seed item `give`, run the Equip screen's pad
/// sequence onto the first Goods row, close the menu and settle. Returns the
/// field overlay's text-quad count afterwards.
fn equip_run(bytes: &[u8], give: Option<u8>, row: usize) -> (usize, bool) {
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes.to_vec(), String::new())
        .expect("load disc");
    rt.enter_field("town01").expect("enter town01");
    tick(&mut rt, 30);
    if let Some(id) = give {
        let _ = rt.cheat_give_item(&format!("0x{id:02x}"), 1);
    }
    assert!(rt.play_menu_open_row("Equip"), "Equip opens");
    // The character picker, then the slot browse's first Goods row (row 5,
    // engine slot 5), its candidate list, the hovered row and the Yes.
    menu(&mut rt, PadButton::Cross);
    for _ in 0..5 {
        menu(&mut rt, PadButton::Down);
    }
    // Open the candidate list, walk to `row`, pick it and answer Yes.
    menu(&mut rt, PadButton::Cross);
    for _ in 0..row {
        menu(&mut rt, PadButton::Down);
    }
    menu(&mut rt, PadButton::Cross);
    menu(&mut rt, PadButton::Cross);
    for _ in 0..4 {
        if !rt.play_menu_is_open() {
            break;
        }
        menu(&mut rt, PadButton::Circle);
    }
    rt.play_menu_close();
    // The equip commit takes the copy out of the bag.
    let model: serde_json::Value =
        serde_json::from_str(&rt.field_menu_model_json()).unwrap_or_default();
    let in_bag = |id: u8| {
        model["items"]
            .as_array()
            .is_some_and(|a| a.iter().any(|r| r["id"].as_u64() == Some(u64::from(id))))
    };
    let equipped = give.is_some_and(|id| !in_bag(id));
    tick(&mut rt, 30);
    (field_texts(&mut rt), equipped)
}

#[test]
fn an_equipped_low_encounter_accessory_raises_the_badge_column() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let bytes = std::fs::read(&disc).expect("read disc");
    let scus = legaia_iso::iso9660::read_file_in_image(&bytes, "SCUS_942.54").expect("SCUS");
    let table =
        legaia_asset::accessory_passive::AccessoryPassiveTable::from_scus(&scus).expect("table");
    // Every item that grants one of the six bits, in id order; the first one
    // the lead can wear in a Goods slot is the one the run equips (some of
    // them are not Goods at all - a used item's passive, a quest twin).
    let six = [
        ability_bit::STACK_A,
        ability_bit::STACK_B,
        ability_bit::STACK_C,
        ability_bit::ENCOUNTER_HIGH,
        ability_bit::ENCOUNTER_LOW,
        ability_bit::BADGE_LEFT,
    ];
    let (control, _) = equip_run(&bytes, None, 0);
    let (id, equipped) = (1..=255u8)
        .filter(|&id| table.passive_index(id).is_some_and(|p| six.contains(&p)))
        .find_map(|id| {
            // The Goods list walks the whole bag, so the seeded copy's row is
            // the bag's order, not the top.
            (0..4).find_map(|row| {
                let (texts, worn) = equip_run(&bytes, Some(id), row);
                worn.then_some((id, texts))
            })
        })
        .expect("one of the badge-bit items equips into the lead's Goods slot");
    assert!(
        equipped > control,
        "equipping item {id:#04x} drew no badge over the player \
         ({control} text quads without it, {equipped} with)"
    );
    eprintln!("[ok] passive badge: item {id:#04x}, field text quads {control} -> {equipped}");
}
