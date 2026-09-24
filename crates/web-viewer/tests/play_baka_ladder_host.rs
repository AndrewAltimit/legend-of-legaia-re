//! Disc-gated: the play page's Baka cabinet runs the retail ladder.
//!
//! Warps into the cabinet the way the casino door does, throws the special
//! until the match falls, then follows the cabinet: on a win the "NEXT GAME /
//! PAY OUT" sheet must reach the overlay draw list and NEXT GAME must seat
//! the next rung (the page's opponent follows and its scene generation
//! bumps); on a loss the cabinet must leave through its own exit state.
//! Either branch proves the page hands the cabinet its pad.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;

const CROSS: u16 = 0x4000;
const TRIANGLE: u16 = 0x1000;
const SUB_BAKA: u8 = 4;

fn tick(rt: &mut LegaiaRuntime, n: usize) {
    for _ in 0..n {
        rt.tick_frame().expect("tick_frame");
    }
}

fn press(rt: &mut LegaiaRuntime, mask: u16) {
    rt.set_pad(mask);
    tick(rt, 1);
    rt.set_pad(0);
    tick(rt, 1);
}

fn json(s: String) -> serde_json::Value {
    serde_json::from_str(&s).expect("json")
}

fn overlay_quads(rt: &mut LegaiaRuntime) -> usize {
    json(rt.play_overlay_draws_json(960, 720))["texts"]
        .as_array()
        .map_or(0, |a| a.len())
}

#[test]
fn the_play_page_cabinet_climbs_or_exits_by_itself() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let bytes = std::fs::read(disc).expect("disc");
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load");
    rt.enter_field("koin1").expect("koin1");
    assert!(rt.play_mg_debug_warp(SUB_BAKA));
    tick(&mut rt, 2);
    assert_eq!(rt.scene_mode(), "BakaFighter");
    let g = json(rt.play_mg_game_json());
    assert_eq!(
        g["baka"]["opponent"].as_u64(),
        Some(5),
        "first rung is roster 5: {g}"
    );
    let gen0 = g["gen"].as_u64().expect("gen");

    let mut over = false;
    for _ in 0..20_000 {
        let st = json(rt.play_mg_baka_state_json());
        if st["phase"].as_str() == Some("match_over") {
            over = true;
            break;
        }
        press(&mut rt, TRIANGLE);
    }
    assert!(over, "the duel never resolved");
    let st = json(rt.play_mg_baka_state_json());
    let won = st["winner"].as_u64() == Some(0);
    eprintln!("match over, player won = {won}");

    if !won {
        for _ in 0..0x400 {
            if rt.scene_mode() != "BakaFighter" {
                break;
            }
            tick(&mut rt, 1);
        }
        assert_eq!(
            rt.scene_mode(),
            "Field",
            "GAME OVER leaves through the exit state"
        );
        eprintln!("[ok] loss branch: the cabinet exited by itself");
        return;
    }

    // The tally drains, then the sheet comes up: its labels and pot digits
    // join the overlay.
    let before = overlay_quads(&mut rt);
    let mut sheet = false;
    for _ in 0..0x600 {
        tick(&mut rt, 1);
        if overlay_quads(&mut rt) > before + 10 {
            sheet = true;
            break;
        }
    }
    assert!(
        sheet,
        "the NEXT GAME / PAY OUT sheet never reached the overlay"
    );
    press(&mut rt, CROSS); // NEXT GAME is the default row
    let mut seated = false;
    for _ in 0..0x400 {
        tick(&mut rt, 1);
        let g = json(rt.play_mg_game_json());
        if g["baka"]["opponent"].as_u64() == Some(6) {
            assert!(
                g["gen"].as_u64().expect("gen") > gen0,
                "scene generation bumps: {g}"
            );
            seated = true;
            break;
        }
    }
    assert!(seated, "NEXT GAME must seat roster 6");
    assert_eq!(rt.scene_mode(), "BakaFighter");
    eprintln!("[ok] win branch: sheet drawn, rung 5 -> 6");
}
