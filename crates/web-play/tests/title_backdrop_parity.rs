//! Disc-gated: the browser play page draws retail's **title strips** behind
//! the save-select its title Continue opens.
//!
//! Retail's menu overlay redraws the title's own strips behind the Load
//! window: `FUN_801DD35C` calls `FUN_801E0418` at `0x801E0260`, then the
//! dimmed art `FUN_801E02A4`, only while `_DAT_8007BB00` - the
//! came-from-the-title word - is set (`lw v0,-0x4500(v0)` /
//! `beq v0,zero,0x801E0270` at `0x801E01D0`). The port's
//! `title_strip_rows` / `title_strip_sprites` is that drawer, reached on both
//! hosts through `title_band_sprites(TitleBandState::backdrop(shade), ..)`.
//!
//! This is the browser half of the parity. The native half is the
//! `title_backdrop_tests` module beside `boot_title_band_state` in the
//! `legaia-engine` binary (`window/title_save_draws.rs`): the title's
//! Continue screen resolves to the same backdrop state and composes to the same
//! `title_strip_sprites` list. Both halves compare against the shared builder
//! itself, so neither host's expectation is transcribed from the other.
//!
//! Rendered at a 320x240 surface, where the boot stage transform is the
//! identity. Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_play::runtime::LegaiaRuntime;

const START: u16 = 0x0008;
const CROSS: u16 = 0x4000;
const CIRCLE: u16 = 0x2000;

/// A raw memory card carrying one Legaia save in `block` - synthesised, not
/// disc-derived. The title's Continue row is live only with save data.
fn card_with_save(block: u8) -> Vec<u8> {
    use legaia_save::card;
    let mut buf = vec![0u8; card::CARD_SIZE];
    buf[..2].copy_from_slice(&card::CARD_MAGIC);
    for i in 1..=card::DIR_FRAMES {
        let off = card::DIR_FRAME_SIZE * i;
        buf[off..off + 4].copy_from_slice(&card::state::FREE.to_le_bytes());
    }
    let f = card::DIR_FRAME_SIZE * block as usize;
    buf[f..f + 4].copy_from_slice(&card::state::FIRST_BLOCK.to_le_bytes());
    buf[f + 8..f + 10].copy_from_slice(&0xFFFFu16.to_le_bytes());
    buf[f + 10..f + 28].copy_from_slice(b"BASCUS-94254PRO-00");
    let b = card::BLOCK_SIZE * block as usize;
    let sc = &mut buf[b..b + card::BLOCK_SIZE];
    sc[..2].copy_from_slice(&card::SAVE_BLOCK_MAGIC);
    let mut rec = legaia_save::CharacterRecord::zeroed();
    rec.set_name("Vahn");
    legaia_save::write_retail_char_records(sc, std::slice::from_ref(&rec.raw)).unwrap();
    buf
}

fn backdrop(rt: &LegaiaRuntime) -> serde_json::Value {
    serde_json::from_str(&rt.boot_title_backdrop_draws_json(320, 240)).expect("backdrop json")
}

#[test]
fn the_title_continue_save_select_draws_retails_title_strips() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN").ok() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let bytes = std::fs::read(&disc).expect("read disc");
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load disc");
    rt.enter_field("town01").expect("enter town01");
    rt.insert_card(0, card_with_save(3), "card A".into())
        .expect("insert card into port 1");
    assert!(
        rt.boot_title_has_save_data(),
        "the card makes Continue live"
    );

    rt.boot_title_start();
    assert!(rt.boot_title_has_atlas(), "the disc's title TIM resolved");
    assert_eq!(
        backdrop(&rt)["active"],
        false,
        "the live card is not the backdrop"
    );
    for _ in 0..240 {
        rt.boot_title_step(0);
    }
    rt.boot_title_step(START); // PressStart -> MainMenu, cursor on New Game
    rt.boot_title_step(0);
    rt.boot_title_step(0x0040); // Down: onto Continue
    let mut outcome = String::new();
    for _ in 0..240 {
        rt.boot_title_step(0);
        outcome = rt.boot_title_step(CROSS);
        if !outcome.is_empty() {
            break;
        }
    }
    assert_eq!(outcome, "continue", "Cross on the Continue row hands off");
    assert!(rt.boot_continue_is_open(), "the Load save-select is up");

    // The backdrop is the title-strip drawer's own output at retail's dim,
    // CONTINUE lit: five strips, sprite for sprite, once the window's
    // 32-frame dim-in has run.
    for _ in 0..32 {
        rt.boot_continue_step(0);
    }
    let v = backdrop(&rt);
    assert_eq!(
        v["active"], true,
        "the save-select keeps the title behind it"
    );
    let got = v["sprites"].as_array().expect("sprites");
    let b = legaia_engine_ui::TITLE_BACKDROP_REST;
    let want = legaia_engine_ui::title_strip_sprites(true, b, 1.0, (0, 0), 1);
    assert_eq!(
        got.len(),
        want.len(),
        "wordmark, NEW GAME, CONTINUE, TM, copyright"
    );
    for (g, w) in got.iter().zip(want.iter()) {
        let dst: Vec<i64> = (0..4).map(|i| g["dst"][i].as_i64().unwrap()).collect();
        let src: Vec<i64> = (0..4).map(|i| g["src"][i].as_i64().unwrap()).collect();
        assert_eq!(
            dst,
            vec![
                w.dst.0 as i64,
                w.dst.1 as i64,
                w.dst.2 as i64,
                w.dst.3 as i64
            ]
        );
        assert_eq!(
            src,
            vec![
                w.src.0 as i64,
                w.src.1 as i64,
                w.src.2 as i64,
                w.src.3 as i64
            ]
        );
        for c in 0..4 {
            let gc = g["color"][c].as_f64().unwrap();
            assert!(
                (gc - f64::from(w.color[c])).abs() < 1e-6,
                "tint {c}: {gc} vs {:?}",
                w.color
            );
        }
    }
    eprintln!(
        "title backdrop: {} strips match title_strip_sprites",
        got.len()
    );

    // Backing out closes the Continue screen, and the backdrop goes with it.
    for _ in 0..8 {
        if !rt.boot_continue_is_open() {
            break;
        }
        rt.boot_continue_step(0);
        rt.boot_continue_step(CIRCLE);
    }
    assert!(
        !rt.boot_continue_is_open(),
        "the screen closed back to the title"
    );
    assert_eq!(
        backdrop(&rt)["active"],
        false,
        "no save-select, no backdrop"
    );
}

/// Retail's title row counter `_DAT_8007B820` outlives the title: a second
/// title the page raises opens its menu on the row the first one held, so
/// one Cross there answers CONTINUE without a Down.
#[test]
fn a_reopened_title_keeps_the_row_the_last_one_held() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN").ok() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let bytes = std::fs::read(&disc).expect("read disc");
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load disc");
    rt.enter_field("town01").expect("enter town01");
    rt.insert_card(0, card_with_save(3), "card A".into())
        .expect("insert card into port 1");
    rt.boot_title_start();
    for _ in 0..240 {
        rt.boot_title_step(0);
    }
    rt.boot_title_step(START);
    rt.boot_title_step(0);
    rt.boot_title_step(0x0040); // Down: onto Continue
    rt.boot_title_step(0);
    rt.boot_title_close();

    rt.boot_title_start();
    for _ in 0..240 {
        rt.boot_title_step(0);
    }
    rt.boot_title_step(START);
    let mut outcome = String::new();
    for _ in 0..240 {
        rt.boot_title_step(0);
        outcome = rt.boot_title_step(CROSS);
        if !outcome.is_empty() {
            break;
        }
    }
    assert_eq!(outcome, "continue", "the reopened title opened on CONTINUE");
    eprintln!("[ran] title row memory");
}

/// The title Continue's card pills rise out of black on the page as the
/// session's model shades them, and the pointing hand waits for them: mode
/// `2` of `FUN_801E1C1C` draws the browsing pills at the grey
/// `min(0x801EF120 >> 5, 0x80)` (`0x801E1E0C..0x801E1E40`) and the hand only
/// at a pre-roll of `0x1000` (`0x801E1DE0..0x801E1DE8`). The page reaches the
/// shared `save_select_overlay_draws` through `boot_continue_draws_json`, so
/// the quads it blits carry that grey.
#[test]
fn the_title_continue_pills_fade_in_and_the_hand_waits() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN").ok() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let bytes = std::fs::read(&disc).expect("read disc");
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load disc");
    rt.enter_field("town01").expect("enter town01");
    rt.insert_card(0, card_with_save(3), "card A".into())
        .expect("insert card into port 1");
    rt.boot_title_start();
    for _ in 0..240 {
        rt.boot_title_step(0);
    }
    rt.boot_title_step(START);
    rt.boot_title_step(0);
    rt.boot_title_step(0x0040);
    let mut outcome = String::new();
    for _ in 0..240 {
        rt.boot_title_step(0);
        outcome = rt.boot_title_step(CROSS);
        if !outcome.is_empty() {
            break;
        }
    }
    assert_eq!(outcome, "continue");
    let greys = |rt: &LegaiaRuntime| -> Vec<f64> {
        let v: serde_json::Value =
            serde_json::from_str(&rt.boot_continue_draws_json(320, 240)).expect("draws json");
        v["sprites"]
            .as_array()
            .expect("sprites")
            .iter()
            .map(|s| s["color"][0].as_f64().unwrap_or(1.0))
            .collect()
    };
    let mut frames = Vec::new();
    for _ in 0..40 {
        rt.boot_continue_step(0);
        frames.push(greys(&rt));
    }
    // The pills are the quads below full grey; the hand is the one quad
    // that joins once they are up.
    let dim = |g: &Vec<f64>| g.iter().copied().filter(|&c| c < 0.999).collect::<Vec<_>>();
    let first = dim(&frames[0]);
    assert!(
        !first.is_empty() && first.iter().all(|&c| c <= 0.04),
        "the pills open out of black: {first:?}"
    );
    let base = frames[0].len();
    let mut last = 0.0f64;
    let mut up_at = None;
    for (i, g) in frames.iter().enumerate() {
        let d = dim(g);
        if d.is_empty() {
            up_at.get_or_insert(i);
            assert_eq!(
                g.len(),
                base + 1,
                "frame {i}: the hand joins the risen pills"
            );
            continue;
        }
        assert!(up_at.is_none(), "frame {i}: the pills dim again");
        assert_eq!(g.len(), base, "frame {i}: no hand while the pills fade");
        assert!(
            d[0] > last,
            "frame {i}: the grey rises ({} after {last})",
            d[0]
        );
        last = d[0];
    }
    let up_at = up_at.expect("the pills reach full grey");
    eprintln!(
        "[page-continue] pills full and hand up on step {}",
        up_at + 1
    );
    assert_eq!(up_at + 1, 32, "32 frame steps of the 0x80 pre-roll ramp");
}
