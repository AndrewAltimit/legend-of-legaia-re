//! Disc-gated ladder: a **real bonus round**, played to its product payout, and
//! the marquee composed off that round's own live state.
//!
//! The sibling fixture `crates/asset/tests/w1l4_slot_bonus_marquee.rs` drives the
//! marquee kernels over hand-seeded [`MarqueeFrame`] tuples and the disc's
//! message bank; that half proves the composition. This half closes the loop the
//! other way round: it plays the machine - jackpot line, bonus round, three free
//! stops, product payout - and feeds the marquee **the machine's own globals**,
//! so the tally the strip prints and the coins the round pays cannot be two
//! numbers that happen to agree.
//!
//! ## Seeding, not grinding
//!
//! Retail's bonus round is opened by matching a line of a jackpot symbol, and
//! nothing in the machine steers a reel toward one: the normal-mode target is
//! `rand%6 + 2`, so `8` and `9` are never searched for. What decides the landing
//! is *where the reel was when you pressed*, and that is a schedule, not a roll.
//!
//! So [`aim_stop`] solves the schedule instead of rolling for it. The machine is
//! deterministic and [`SlotMachine`] is [`Clone`], so each candidate frame is
//! probed on a copy - the RNG state is identical on every probe, only the reel's
//! row differs - and the first frame whose landing carries the wanted value is
//! the one the real machine takes. No RNG is consumed searching, and the run is
//! reproducible from the pinned seed.
//!
//! That is also what makes the bonus half *exact* rather than approximate: a
//! bonus stop plan is `depth 0 / target -1` (the free stop), so the reel lands on
//! `from_row + 1` unconditionally and the three numbers a round multiplies are
//! pinned by the schedule.
//!
//! ## What stays dark, and why
//!
//! The browser minigames page owns a live machine and reads its reels, its tally
//! and its product - but it does **not** compose the dot matrix: `slot_marquee_json`
//! exports the message ids and dot columns, and `site/_content/minigames.html`
//! blits the bitmaps itself. So `SlotMachine::marquee_placements` has no host
//! caller, and the last rung below drives it directly rather than through the
//! page. See the report note; giving it a host means the page taking a
//! rasterised buffer from WASM instead of the raw bank.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / the extracted PROT entries are absent.

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};

use legaia_asset::minigame_art;
use legaia_asset::minigame_slot_scene as slot;
use legaia_asset::slot_payout;
use legaia_engine_core::slot_machine::{REEL_COUNT, SlotMachine, SlotPhase};
use legaia_web_viewer::minigames::LegaiaMinigames;

const OVERLAY_FILE: &str = "0975_other_game.BIN";
const ART_FILE: &str = "1200_other4.BIN";

/// Pinned session seed. Any seed works - the schedule is solved, not searched -
/// but pinning one keeps the run byte-reproducible.
const SEED: u32 = 0x51075_u32;
/// Coins the machine is racked with. A jackpot line takes a handful of 3-coin
/// spins to schedule, so the bank has to outlast them.
const BALANCE: i32 = 600;
/// Frames a reel is probed over before the aim is called impossible. One row is
/// at most `0x100 / 0x60` = 5 frames and a strip is 20 rows, so a full
/// revolution fits inside this with room to spare.
const AIM_FRAMES: usize = 240;

fn prot_entry(name: &str) -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for dir in ["extracted/PROT", "../../extracted/PROT"] {
        let p = Path::new(dir).join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

struct Disc {
    payouts: slot_payout::SlotPayoutTable,
    messages: Vec<slot::MarqueeMessage>,
}

fn disc() -> Option<Disc> {
    let overlay = std::fs::read(prot_entry(OVERLAY_FILE)?).ok()?;
    let art_raw = std::fs::read(prot_entry(ART_FILE)?).ok()?;
    let payouts = slot_payout::parse(&overlay).expect("the per-symbol payout table (PROT 0975)");
    let tims = minigame_art::parse_art_pack(&art_raw).expect("the slot art pack (PROT 1200)");
    let (page, w, _h) =
        minigame_art::slot_page_indices(&tims, slot::DOT_PAGE).expect("the dot-matrix art page");
    let scene = slot::parse_scene(&overlay, &page, w).expect("the slot scene");
    Some(Disc {
        payouts,
        messages: scene.messages,
    })
}

/// Run the spin-up out so stops are accepted.
fn to_stopping(m: &mut SlotMachine) {
    let mut guard = 0;
    while m.phase() != SlotPhase::Stopping {
        m.tick();
        guard += 1;
        assert!(guard < 4096, "the spin never reached the stopping state");
    }
}

/// Stop `reel` on the first frame whose landing carries a value `want` accepts.
///
/// The probe is a clone, so the machine's RNG is untouched by the search and the
/// stop the real machine takes is the one the winning probe measured.
fn aim_stop(m: &mut SlotMachine, reel: usize, want: impl Fn(u8) -> bool) -> Option<u8> {
    for _ in 0..AIM_FRAMES {
        let mut probe = m.clone();
        if probe.stop_reel(reel) {
            let landed = probe.strips()[reel][probe.payline_row(reel)];
            if want(landed) {
                assert!(m.stop_reel(reel), "the aimed stop must be accepted");
                return Some(landed);
            }
        }
        m.tick();
    }
    None
}

/// Play spins until a jackpot line opens the bonus round, aiming every reel at a
/// row carrying `symbol`. Returns the number of spins it took.
fn open_bonus_round(m: &mut SlotMachine, symbol: u8) -> usize {
    for spin in 1..=40usize {
        assert!(m.spin(), "the machine ran out of coins after {spin} spins");
        to_stopping(m);
        for reel in 0..REEL_COUNT {
            // A reel that cannot be steered onto the symbol this spin still has
            // to stop, or the spin never resolves.
            if aim_stop(m, reel, |v| v == symbol).is_none() {
                to_stopping(m);
                assert!(
                    m.stop_reel(reel) || m.phase() != SlotPhase::Stopping,
                    "reel {reel} would not stop"
                );
            }
        }
        let result = m.last_result().expect("a stopped spin evaluates");
        if result.bonus_triggered {
            return spin;
        }
        m.collect();
    }
    panic!("40 aimed spins did not open the bonus round");
}

/// The machine plays a real bonus round, and the strip across its top reads the
/// same three numbers the payout multiplies.
///
/// The non-vacuity that matters here is not "a round happened". It is that the
/// tally, the product and the credited payout are **one** state read three ways:
/// `claimed[] -> tally() -> tally_product() == last_result().payout == balance
/// delta`. A display copy could drift from the result; this cannot.
#[test]
fn a_played_bonus_round_pays_the_product_its_marquee_tallies() {
    let Some(d) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT missing (disc-gated)");
        return;
    };
    let mut m = SlotMachine::new(d.payouts.clone(), SEED, BALANCE);

    // --- rung 1: open the round on the red "punch" (3 rounds, not 1) --------
    let spins = open_bonus_round(&mut m, slot_payout::PUNCH_SYMBOL_ID);
    assert!(m.in_bonus_round(), "feature mode 6 after the jackpot line");
    assert_eq!(
        m.feature_mode(),
        slot::FEATURE_MODE_BONUS_ROUND,
        "the asset-side and engine-side bonus mode constants must agree"
    );
    assert_eq!(
        m.bonus_spins(),
        slot_payout::PUNCH_BONUS_ROUNDS as i32,
        "the red \"punch\" earns {} rounds",
        slot_payout::PUNCH_BONUS_ROUNDS
    );
    eprintln!("[w1l4] bonus round opened after {spins} aimed spin(s)");
    m.collect();

    // Between rounds the marquee is the round counter: three pips, one lit per
    // round still owed. This is the `4..=6` + state `1..=2` arm.
    let pips = m.marquee_placements();
    assert_eq!(
        pips.len(),
        slot::ROUND_PIP_COLS.len(),
        "three pips: {pips:?}"
    );
    assert_eq!(
        pips.iter()
            .filter(|p| p.msg == slot::MSG_ROUND_PIP_ON)
            .count(),
        slot_payout::PUNCH_BONUS_ROUNDS as usize,
        "one lit pip per round owed"
    );
    assert!(
        slot::render_marquee(&pips, &d.messages)
            .iter()
            .any(|&dot| dot != 0),
        "the pip strip rasterised empty"
    );

    // --- rung 2: play one bonus spin, three free stops ----------------------
    let balance_before = m.balance();
    let net_take_before = m.net_take();
    assert!(m.spin(), "a bonus spin still costs a coin");
    to_stopping(&mut m);

    let mut numbers = [0u32; REEL_COUNT];
    for reel in 0..REEL_COUNT {
        // Aim at the largest numeral the reel can be steered onto, so the
        // product is a distinctive multi-digit figure rather than a small one
        // the caption's leading-zero suppression would hide.
        let landed = (2..=10u32)
            .rev()
            .find_map(|n| {
                let value = slot_payout::BONUS_VALUE_BIAS + n as u8;
                aim_stop(&mut m, reel, |v| v == value)
            })
            .unwrap_or_else(|| panic!("reel {reel} never rotated a numeral onto its payline"));
        assert!(
            landed >= slot_payout::BONUS_VALUE_BASE,
            "reel {reel} landed {landed:#x}, not a bonus numeral - the display \
             strip had not finished rotating onto the numerals"
        );
        numbers[reel] = slot_payout::bonus_number_for_value(landed);

        // The tally strip, mid-round: one claimed column per stop taken.
        if m.phase() == SlotPhase::Stopping {
            let placed = m.marquee_placements();
            assert_eq!(
                placed.len(),
                slot::TALLY_NUMBER_COLS.len() + slot::TALLY_TIMES_COLS.len(),
                "the tally is 3 numerals and 2 signs after {} stop(s)",
                reel + 1
            );
            for (r, &col) in slot::TALLY_NUMBER_COLS.iter().enumerate() {
                let msg = placed
                    .iter()
                    .find(|p| p.col == col as i32)
                    .unwrap_or_else(|| panic!("nothing at tally column {col}"))
                    .msg;
                let want = if r <= reel {
                    slot::MSG_NUMBER_BASE + numbers[r] as usize
                } else {
                    slot::MSG_NUMBER_BASE
                };
                assert_eq!(msg, want, "tally column {r} after {} stop(s)", reel + 1);
            }
            assert!(
                slot::render_marquee(&placed, &d.messages)
                    .iter()
                    .any(|&dot| dot != 0),
                "the tally strip rasterised empty"
            );
        }
    }

    // --- rung 3: the product, three ways ------------------------------------
    let result = m.last_result().expect("the bonus spin evaluated");
    assert!(result.bonus_spin, "this spin was a bonus free spin");
    let product: u32 = numbers.iter().product();
    assert_eq!(
        m.tally(),
        numbers,
        "the marquee tally is the landed numbers"
    );
    assert!(m.tally_complete(), "all three columns claimed");
    assert_eq!(
        m.tally_product(),
        product,
        "the tally's product is the round's payout"
    );
    assert_eq!(
        result.payout as u32, product,
        "the credited payout is the product of the three numbers"
    );
    assert!(
        (slot_payout::BONUS_PAYOUT_MIN..=slot_payout::BONUS_PAYOUT_MAX).contains(&product),
        "a bonus round pays 1..=1000, got {product}"
    );
    assert_eq!(
        m.net_take(),
        net_take_before + legaia_engine_core::slot_machine::NET_TAKE_FEATURE_SPIN - result.payout,
        "a bonus payout is subtracted from the net-take counter"
    );
    let credited = m.collect();
    assert_eq!(credited as u32, product);
    assert_eq!(
        m.balance(),
        balance_before - legaia_engine_core::slot_machine::SPIN_COST_FEATURE + credited,
        "1 coin charged, the product credited"
    );
    eprintln!(
        "[w1l4] bonus spin paid {} x {} x {} = {product}",
        numbers[0], numbers[1], numbers[2]
    );
}

/// The payout caption the round ends on, composed off the machine's own latched
/// figure and rasterised against the disc's message bank.
///
/// This is where `compose_marquee_frame` (`FUN_801CFFF0`), `place_message`
/// (`FUN_801D3230`) and `clear_dots` (`FUN_801D069C`) run over live machine
/// state rather than a hand-written tuple, which is what makes the caption's
/// digits a claim about the round instead of about the composer.
#[test]
fn the_rounds_payout_caption_spells_the_product_on_the_disc_bank() {
    let Some(d) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT missing (disc-gated)");
        return;
    };
    let mut m = SlotMachine::new(d.payouts.clone(), SEED, BALANCE);
    open_bonus_round(&mut m, slot_payout::PUNCH_SYMBOL_ID);
    m.collect();
    assert!(m.spin());
    to_stopping(&mut m);
    let mut numbers = [0u32; REEL_COUNT];
    for (reel, n) in numbers.iter_mut().enumerate() {
        let landed = (2..=10u32)
            .rev()
            .find_map(|k| {
                let value = slot_payout::BONUS_VALUE_BIAS + k as u8;
                aim_stop(&mut m, reel, |v| v == value)
            })
            .expect("a numeral on the payline");
        *n = slot_payout::bonus_number_for_value(landed);
    }
    let product: u32 = numbers.iter().product();
    assert_eq!(m.phase(), SlotPhase::Payout);

    // Frame 1 of the caption: it starts 12 rows above the matrix, and only the
    // unsigned destination clip keeps those rows off the strip.
    let first = m.marquee_placements();
    assert!(!first.is_empty(), "a paying round captions");
    assert_eq!(first[0].row, 1 - slot::PAYOUT_SLIDE_ROWS);
    let first_dots = slot::render_marquee(&first, &d.messages)
        .iter()
        .filter(|&&x| x != 0)
        .count();

    // It descends one row per machine tick and then holds.
    for _ in 0..(slot::PAYOUT_SLIDE_ROWS + 4) {
        m.tick();
    }
    let settled = m.marquee_placements();
    assert_eq!(settled[0].row, 0, "the caption lands at row 0 and holds");
    let settled_dots = slot::render_marquee(&settled, &d.messages)
        .iter()
        .filter(|&&x| x != 0)
        .count();
    assert!(
        settled_dots > first_dots,
        "the caption must arrive gradually ({first_dots} -> {settled_dots} dots)"
    );

    // The digits are the product's, at the retail columns, with the leading
    // places suppressed on the whole figure.
    let digit_at = |col: usize| {
        settled
            .iter()
            .find(|p| p.col == col as i32)
            .map(|p| p.msg - slot::MSG_NUMBER_BASE)
    };
    let places = [
        (product / 1000, 1000),
        ((product % 1000) / 100, 100),
        ((product % 100) / 10, 10),
        (product % 10, 0),
    ];
    for (i, (digit, threshold)) in places.iter().enumerate() {
        let got = digit_at(slot::PAYOUT_DIGIT_COLS[i]);
        if product >= *threshold {
            assert_eq!(
                got,
                Some(*digit as usize),
                "place {i} of {product} at column {}",
                slot::PAYOUT_DIGIT_COLS[i]
            );
        } else {
            assert_eq!(got, None, "place {i} is above {product} and must not draw");
        }
    }
    assert!(
        settled
            .iter()
            .any(|p| p.col == slot::PAYOUT_COINS_COL as i32 && p.msg == slot::MSG_COINS),
        "the word \"coin\" follows the figure"
    );

    // Collecting takes the caption down with the tally it captioned.
    m.collect();
    assert!(
        m.marquee_placements()
            .iter()
            .all(|p| p.msg != slot::MSG_COINS),
        "the caption must come down with the payout it captioned"
    );
}

/// The **page's** own machine reaches the bonus round through the surface the
/// site drives - `slot_press` / `slot_tick` - and reports it through
/// `slot_bonus_json`.
///
/// The page has one key, so the aim here is the same schedule solve done from
/// outside: tick until the reel's payline row is one short of a jackpot row,
/// then press. Without the clone lookahead a spin can still be spoiled by the
/// normal-mode search grabbing its own target first, so the rung is budgeted
/// over several spins rather than assumed to land on the first.
#[test]
fn the_minigames_page_drives_its_machine_into_a_bonus_round() {
    let Some(disc_path) = std::env::var("LEGAIA_DISC_BIN").ok() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let Ok(bytes) = std::fs::read(&disc_path) else {
        eprintln!("[skip] disc image unreadable (disc-gated)");
        return;
    };
    let mut mg = LegaiaMinigames::new();
    mg.load_disc(bytes).expect("load_disc");
    assert!(mg.slot_start(SEED, BALANCE), "the machine racks");

    let row_of = |mg: &LegaiaMinigames, reel: usize| {
        (mg.slot_reel_pos()[reel] >> 8).rem_euclid(slot::STRIP_LEN) as usize
    };
    let jackpot_rows = |mg: &LegaiaMinigames, reel: usize| -> Vec<usize> {
        mg.slot_strip(reel)
            .iter()
            .enumerate()
            .filter(|&(_, &v)| v == slot_payout::PUNCH_SYMBOL_ID)
            .map(|(i, _)| i)
            .collect()
    };

    let mut opened = None;
    'spins: for spin in 1..=60usize {
        if mg.slot_press() != "spin" {
            break;
        }
        while mg.slot_press() == "spinup" {
            mg.slot_tick();
        }
        // The press above already took reel 0's stop once spin-up cleared, so
        // aim the remaining reels and let reel 0 be wherever it landed.
        for reel in 1..REEL_COUNT {
            let targets = jackpot_rows(&mg, reel);
            let mut aimed = false;
            for _ in 0..AIM_FRAMES {
                let row = row_of(&mg, reel);
                if targets
                    .iter()
                    .any(|&t| (row + 1) % slot::STRIP_LEN as usize == t)
                {
                    aimed = true;
                    break;
                }
                mg.slot_tick();
            }
            let _ = aimed;
            if mg.slot_press() == "none" {
                continue 'spins;
            }
        }
        mg.slot_tick();
        let bonus: serde_json::Value = serde_json::from_str(&mg.slot_bonus_json()).unwrap();
        if bonus["active"] == true {
            opened = Some(spin);
            break;
        }
        let st: serde_json::Value = serde_json::from_str(&mg.slot_state_json()).unwrap();
        if st["balance"].as_i64().unwrap_or(0) < 3 {
            break;
        }
    }

    // Whether or not the page's blind aim opened a round, the page must at
    // least keep reporting a coherent machine - the rung is a reach test for
    // the page's own surface, and a silently-empty payload is the failure it
    // is looking for.
    let bonus: serde_json::Value = serde_json::from_str(&mg.slot_bonus_json()).unwrap();
    assert_eq!(bonus["kick_symbol"], slot_payout::KICK_SYMBOL_ID as u64);
    assert_eq!(bonus["punch_symbol"], slot_payout::PUNCH_SYMBOL_ID as u64);
    assert_eq!(bonus["max"], slot_payout::BONUS_PAYOUT_MAX as u64);
    assert_eq!(bonus["numbers"].as_array().unwrap().len(), REEL_COUNT);

    let opened = opened.unwrap_or_else(|| {
        panic!(
            "60 aimed spins on the page never opened a bonus round (last state: {})",
            mg.slot_state_json()
        )
    });
    eprintln!("[w1l4] page opened a bonus round on spin {opened}");

    // Inside a round the page's reels must be carrying the numerals, and its
    // tally / product must move with them.
    let mut sawnumeral = false;
    for _ in 0..AIM_FRAMES {
        let bonus: serde_json::Value = serde_json::from_str(&mg.slot_bonus_json()).unwrap();
        if bonus["active"] != true {
            break;
        }
        if mg
            .slot_strip(0)
            .iter()
            .any(|&v| v >= slot_payout::BONUS_VALUE_BASE)
        {
            sawnumeral = true;
        }
        match mg.slot_press().as_str() {
            "spinup" | "none" => {
                mg.slot_tick();
            }
            _ => {}
        }
    }
    assert!(
        sawnumeral,
        "the page's reels never rotated a bonus numeral onto their strip"
    );

    // The page's marquee surface is constants only - it exports the ids and dot
    // columns and blits in JavaScript. Pin the hand-off so the page and the
    // composer cannot drift apart silently.
    let mq: serde_json::Value = serde_json::from_str(&mg.slot_marquee_json()).unwrap();
    assert_eq!(mq["number_base"], slot::MSG_NUMBER_BASE as u64);
    assert_eq!(mq["number_max"], slot::MSG_NUMBER_MAX as u64);
    assert_eq!(mq["times"], slot::MSG_TIMES as u64);
    assert_eq!(mq["coins"], slot::MSG_COINS as u64);
    assert_eq!(mq["payout_slide_rows"], slot::PAYOUT_SLIDE_ROWS as u64);
    let cols: Vec<u64> = mq["tally_cols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert_eq!(
        cols,
        slot::TALLY_NUMBER_COLS
            .iter()
            .map(|&c| c as u64)
            .collect::<Vec<_>>()
    );
}
