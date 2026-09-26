//! Disc-gated: the play page's Baka cabinet runs the retail ladder.
//!
//! Warps into the cabinet the way the casino door does, throws the special
//! until the match falls, then follows the cabinet: on a win the "NEXT GAME /
//! PAY OUT" sheet must reach the overlay draw list and NEXT GAME must seat
//! the next rung (the page's opponent follows and the duel surface's
//! generation bumps, so the page re-reads the buffers and the VRAM); on a
//! loss the cabinet must leave through its own exit state.
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
    // The duel surface poses on the page's frame call, one buffer set and
    // one arena view-projection - the same kernel the native window drives.
    let gen0 = rt.play_mg_baka_scene_frame();
    assert!(gen0 >= 0, "the duel surface builds");
    let pos = rt.play_mg_baka_scene_positions();
    assert!(!pos.is_empty() && pos.len().is_multiple_of(3));
    assert_eq!(rt.play_mg_baka_scene_uvs().len() / 2, pos.len() / 3);
    assert_eq!(rt.play_mg_baka_scene_flat_rgba().len() / 4, pos.len() / 3);
    assert!(!rt.play_mg_baka_scene_indices().is_empty());
    assert_eq!(rt.play_mg_baka_scene_vp(4.0 / 3.0).len(), 16);
    assert_eq!(rt.play_mg_baka_scene_vram().len(), 1024 * 512 * 2);
    // The state JSON is the standalone page's builder: strike clock, display
    // clips and afterimage passes included.
    let st = json(rt.play_mg_baka_state_json());
    for key in ["clock", "motion", "ghosts"] {
        assert!(st.get(key).is_some(), "state JSON carries {key}: {st}");
    }

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
            let seated_gen = rt.play_mg_baka_scene_frame();
            assert!(
                seated_gen > gen0,
                "the duel surface rebuilds: {gen0} -> {seated_gen}"
            );
            seated = true;
            break;
        }
    }
    assert!(seated, "NEXT GAME must seat roster 6");
    assert_eq!(rt.scene_mode(), "BakaFighter");
    eprintln!("[ok] win branch: sheet drawn, rung 5 -> 6");
}
