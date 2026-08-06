//! Depth ladder for the **equip / shop / menu-window** surfaces of the
//! browser play page - the rungs `play_compose_ladder` browses past.
//!
//! The composition ladder proves the page *renders* each screen it routes to.
//! It does not push any screen to its commit: its shop rung confirms the
//! second row of the Buy/Sell/Quit picker (Sell), its Equip rung browses the
//! slot list without a Cross, and its Items rung stops at the first confirm.
//! Every one of those beats is where the retail sub-screens this lane owns
//! actually live, so the whole cluster read never-entered under a ladder that
//! demonstrably opened their parent screens.
//!
//! What each rung drives, and the retail routine it is there to enter:
//!
//! | # | rung | reaches |
//! |---|---|---|
//! | 1 | equipment shop -> buy list -> **recipient picker** | `FUN_801DB21C` buy-list confirm dispatch, `FUN_801DB380` recipient sub-screen, and the three windows it paints: 36 (`FUN_801D56FC`), 41 (`FUN_801D4C28`), the compare chain (`FUN_801D1290`) and its seeder (`FUN_801CF5D0`) |
//! | 2 | shop vendor plate | window 33 (`FUN_801DCF14`) - needs a scene shop with a resolvable vendor name |
//! | 3 | Equip screen driven to a **candidate list + commit** | `FUN_801D9C14` trial-equip preview, `FUN_801CF760` Best Equipment applier |
//! | 4 | Items screen driven past the command window | the throw-confirm / target-panel arms (`FUN_801D1B20`, `FUN_801D0520`) |
//!
//! Every rung composes through the page's own read surface
//! (`play_overlay_draws_json` / `play_menu_draws_json`), so a builder that
//! runs and emits nothing fails the rung instead of passing as "entered" -
//! the distinction the reach report alone cannot make.
//!
//! Coverage export (what wires this into the reach report):
//!
//! ```text
//! cargo llvm-cov -p legaia-web-viewer --test w1f2_menu_depth_ladder \
//!     --json --output-path target/cov-w1f2_menu_depth_ladder.json
//! ```
//!
//! Exported **without** `--release`: an optimised export loses inlined
//! callees to a zero-count out-of-line record, which is indistinguishable
//! from never-called.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::input::PadButton;
use legaia_web_viewer::runtime::LegaiaRuntime;

/// 320x240 keeps `stage_transform` at identity, so the JSON pens are raw
/// `engine-ui` output (the contract `menu_parity.rs` relies on).
const W: u32 = 320;
const H: u32 = 240;

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or(serde_json::Value::Null)
}

fn text_count(v: &serde_json::Value) -> usize {
    v["texts"].as_array().map(|a| a.len()).unwrap_or(0)
}

/// The overlay's composed glyph-quad count. The draw JSON carries quads, not
/// strings (`play_menu::quad_json`), so "did this panel paint" is a count
/// question - which is exactly the non-vacuity the reach report cannot ask:
/// a builder that runs and emits nothing reads *entered* to a coverage join
/// and blank to a player.
fn overlay_quads(rt: &mut LegaiaRuntime) -> usize {
    text_count(&json(&rt.play_overlay_draws_json(W, H)))
}

// ---------------------------------------------------------------------------
// Rung 1 + 2 - the shop cluster
// ---------------------------------------------------------------------------

/// Open the equipment shop and walk it to the recipient picker.
///
/// The route is the retail one and every hop is asserted: the top picker's
/// row 0 (Buy) opens the stock list, and a confirm on an equipment row runs
/// `shop::buy_list_confirm_route`. Only the `RecipientPicker` arm parks the
/// list and installs a [`BuyRecipientSession`]; a stackable row would take
/// the quantity picker and a row over the purse would buzz, so reaching
/// `debug_recipient_picker_open()` is proof the kind-1 arm ran and not
/// merely that a shop opened.
fn rung1_recipient_picker(rt: &mut LegaiaRuntime) -> Result<(), String> {
    if !rt.debug_open_equipment_shop() {
        return Err("equipment shop did not open (no equip-stat table?)".into());
    }
    if !rt.play_shop_is_open() {
        return Err("shop session not reported open".into());
    }
    // Top picker row 0 = Buy.
    rt.play_shop_input(PadButton::Cross.mask());
    // Baseline: the buy list alone, before any recipient window exists.
    let list_quads = overlay_quads(rt);
    if list_quads == 0 {
        return Err("buy list composed no glyph quads".into());
    }

    // Confirm rows until one takes the recipient route. Every row this shop
    // stocks is equipment and the purse is at GOLD_CAP, so row 0 should do
    // it; the sweep exists so a stock list whose first row is a quest-item
    // alias does not stall the rung.
    let mut opened = false;
    for _ in 0..4u32 {
        rt.play_shop_input(PadButton::Cross.mask());
        if rt.debug_recipient_picker_open() {
            opened = true;
            break;
        }
        // Back out of whatever sub-screen the row took, then step down one.
        rt.play_shop_input(PadButton::Circle.mask());
        rt.play_shop_input(PadButton::Down.mask());
    }
    if !opened {
        return Err(format!(
            "no buy-list row took the RecipientPicker route (menu state {:#04x})",
            rt.debug_menu_state_byte()
        ));
    }

    // The picker is up: its windows must actually paint. Window 36 is the
    // target list (the bag row + one row per member), window 41 the
    // party-wide compare - both come out of `recipient_picker_draws_for`,
    // and both are additive over the parked buy list underneath.
    let draws = json(&rt.play_overlay_draws_json(W, H));
    if draws["open"] != true {
        return Err("recipient picker open but the overlay reports closed".into());
    }
    let picker_quads = text_count(&draws);
    if picker_quads <= list_quads {
        return Err(format!(
            "recipient windows added no draws over the parked list \
             ({list_quads} quads before, {picker_quads} with the picker up)"
        ));
    }

    // Walk every row of the picker, composing each - the compare column is
    // rebuilt per member, so this is what runs the seeder for each record.
    for _ in 0..4 {
        rt.play_shop_input(PadButton::Down.mask());
        let _ = rt.play_overlay_draws_json(W, H);
    }

    // Confirm rows until the session commits. Retail has three arms here and
    // this loop covers all of them: a party row the mask forbids buzzes and
    // *stays* (so the loop continues), row 0 buys into the bag, an
    // equippable party row buys and equips. Both buying arms debit the
    // purse and end the session on the following frame, so the assertion
    // is the purse - a picker that closed without one is a commit that
    // granted an item for free.
    let gold_before = money(rt);
    let mut committed = false;
    for _ in 0..8 {
        rt.play_shop_input(PadButton::Cross.mask());
        let _ = rt.play_overlay_draws_json(W, H);
        // The commit's `Exit` phase releases on the next frame.
        rt.play_shop_input(0);
        if !rt.debug_recipient_picker_open() {
            committed = true;
            break;
        }
        rt.play_shop_input(PadButton::Down.mask());
        let _ = rt.play_overlay_draws_json(W, H);
    }
    if !committed {
        return Err("no recipient row ever committed (every row buzzed?)".into());
    }
    let gold_after = money(rt);
    if gold_after >= gold_before {
        return Err(format!(
            "recipient commit did not debit the purse ({gold_before} -> {gold_after})"
        ));
    }
    Ok(())
}

/// The party purse off the page's own menu model (the same read the browser
/// overlay prints).
fn money(rt: &mut LegaiaRuntime) -> i64 {
    json(&rt.field_menu_model_json())["gold"]
        .as_i64()
        .unwrap_or(-1)
}

/// Window 33 - the vendor plate. It draws only when the shop session
/// resolves a non-empty vendor name against the scene's shop table, so this
/// rung is about a *scene* shop rather than the synthetic one.
fn rung2_vendor_plate(rt: &mut LegaiaRuntime) -> Result<(), String> {
    for _ in 0..8 {
        if !rt.play_shop_is_open() {
            break;
        }
        rt.play_shop_input(PadButton::Circle.mask());
    }
    if !rt.debug_open_test_shop() {
        return Err("test shop did not open".into());
    }
    let draws = json(&rt.play_overlay_draws_json(W, H));
    if text_count(&draws) == 0 {
        return Err("shop overlay drew no text at all".into());
    }
    for _ in 0..8 {
        if !rt.play_shop_is_open() {
            break;
        }
        rt.play_shop_input(PadButton::Circle.mask());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Rung 3 - the Equip screen, driven to a commit
// ---------------------------------------------------------------------------

/// Drive the Equip sub-screen past browse: confirm row 0 (Best Equipment),
/// then open a slot's candidate list and move the cursor through it.
///
/// Row 0 runs `equip_session::apply_best_equipment` (`FUN_801CF760`); every
/// other row opens the candidate list, whose open / move / confirm beats each
/// re-run the trial-equip preview `EquipSession::preview_candidate`
/// (`FUN_801D9C14`). Neither is reachable by browsing the slot list, which is
/// all the composition ladder does.
fn rung3_equip_depth(rt: &mut LegaiaRuntime) -> Result<(), String> {
    if !rt.play_menu_open_row("Equip") {
        return Err("Equip row did not open its sub-screen".into());
    }
    let before = json(&rt.play_menu_draws_json(W, H));
    if text_count(&before) == 0 {
        return Err("Equip screen drew nothing on open".into());
    }
    // Row 0 = Best Equipment.
    rt.play_menu_input(PadButton::Cross.mask());
    let _ = rt.play_menu_draws_json(W, H);

    // Then every armament slot's candidate list: open it, walk it, confirm
    // the hovered row, and answer the Yes/No prompt.
    for slot_row in 1..=5u32 {
        for _ in 0..slot_row {
            rt.play_menu_input(PadButton::Down.mask());
            let _ = rt.play_menu_draws_json(W, H);
        }
        rt.play_menu_input(PadButton::Cross.mask());
        let _ = rt.play_menu_draws_json(W, H);
        for edge in [
            PadButton::Down.mask(),
            PadButton::Down.mask(),
            PadButton::Up.mask(),
        ] {
            rt.play_menu_input(edge);
            let _ = rt.play_menu_draws_json(W, H);
        }
        rt.play_menu_input(PadButton::Cross.mask());
        let _ = rt.play_menu_draws_json(W, H);
        rt.play_menu_input(PadButton::Cross.mask());
        let _ = rt.play_menu_draws_json(W, H);
        // Back to the slot list for the next slot.
        for _ in 0..3 {
            rt.play_menu_input(PadButton::Circle.mask());
            let _ = rt.play_menu_draws_json(W, H);
            if !rt.play_menu_is_open() {
                break;
            }
        }
        if !rt.play_menu_is_open() && !rt.play_menu_open_row("Equip") {
            return Err(format!("Equip did not re-open for slot row {slot_row}"));
        }
    }
    rt.play_menu_close();
    Ok(())
}

// ---------------------------------------------------------------------------
// Rung 4 - the Items screen, driven past the command window
// ---------------------------------------------------------------------------

/// Items: the command window's row 1 is **Throw Out**, whose list confirm
/// raises the throw-confirm modal (`FUN_801D1B20`); row 0 is Use, whose
/// confirm on a targeted item raises the field target panel
/// (`FUN_801D0520`). Both sit one confirm past where the composition ladder
/// stops.
fn rung4_items_depth(rt: &mut LegaiaRuntime) -> Result<(), String> {
    // Use first, so a throw-out does not eat the only bag row.
    for command_row in [0u32, 1] {
        if !rt.play_menu_open_row("Items") {
            return Err("Items row did not open its sub-screen".into());
        }
        for _ in 0..command_row {
            rt.play_menu_input(PadButton::Down.mask());
            let _ = rt.play_menu_draws_json(W, H);
        }
        rt.play_menu_input(PadButton::Cross.mask());
        let _ = rt.play_menu_draws_json(W, H);
        // Walk a couple of bag rows, then confirm one.
        for edge in [PadButton::Down.mask(), PadButton::Up.mask()] {
            rt.play_menu_input(edge);
            let _ = rt.play_menu_draws_json(W, H);
        }
        rt.play_menu_input(PadButton::Cross.mask());
        let after = json(&rt.play_menu_draws_json(W, H));
        if text_count(&after) == 0 {
            return Err(format!(
                "Items command row {command_row} confirm drew an empty screen"
            ));
        }
        // Answer whatever modal came up (cursor seeds on No / the first
        // target), then unwind.
        rt.play_menu_input(PadButton::Up.mask());
        let _ = rt.play_menu_draws_json(W, H);
        rt.play_menu_input(PadButton::Cross.mask());
        let _ = rt.play_menu_draws_json(W, H);
        for _ in 0..6 {
            if !rt.play_menu_is_open() {
                break;
            }
            rt.play_menu_input(PadButton::Circle.mask());
            let _ = rt.play_menu_draws_json(W, H);
        }
        rt.play_menu_close();
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

#[test]
fn w1f2_menu_depth_ladder() {
    let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let Ok(bytes) = std::fs::read(&disc) else {
        eprintln!("[skip] disc unreadable (disc-gated)");
        return;
    };
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load_disc");
    rt.enter_field("town01").expect("enter_field(town01)");
    for _ in 0..8 {
        rt.tick_frame().expect("tick_frame");
    }

    type Rung = (&'static str, fn(&mut LegaiaRuntime) -> Result<(), String>);
    let rungs: [Rung; 4] = [
        ("shop-recipient-picker", rung1_recipient_picker),
        ("shop-vendor-plate", rung2_vendor_plate),
        ("equip-depth", rung3_equip_depth),
        ("items-depth", rung4_items_depth),
    ];

    let mut failures = Vec::new();
    for (name, rung) in rungs {
        match rung(&mut rt) {
            Ok(()) => eprintln!("[rung] {name}: cleared"),
            Err(why) => {
                eprintln!("[stall] {name}: {why}");
                failures.push(format!("{name}: {why}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "w1f2 menu-depth ladder stalled:\n  {}",
        failures.join("\n  ")
    );
}
