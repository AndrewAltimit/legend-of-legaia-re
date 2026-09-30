//! Disc-gated: the browser play page crosses the Ra-Seru chip out against
//! monster `0xAF`, off the same engine read and through the same builder as
//! the native window.
//!
//! Retail's battle round driver `FUN_801D0748` tests bit `0x200` of the
//! special-battle word on every frame of the command-ring phase and calls
//! `FUN_801DBC30(0xF8, 0x42)` - the red X over the Ra-Seru chip - before it
//! reads the pad (`0x801D12DC..0x801D12F4`). Battle init raises the bit when
//! the formation's first monster is `0xAF` (`FUN_800513F0`,
//! `0x800519C0..0x80051A04`); that monster is Tetsu, whose only formation row
//! is `town0d` row 4.
//!
//! Both hosts draw the menu through `engine-ui`'s
//! `battle_command_ui::battle_command_menu_sprites` with
//! `engine-core::battle_hud::battle_raseru_cross_out` as its switch. The
//! builder's own unit test pins the mark against the switch; this test drives
//! the page's half from a real fight, which pins the two things only the page
//! can get wrong: that its switch reaches the builder, and that its atlas
//! carries the cross-out cell (a host that skips the bake draws the plates
//! and no mark). A control fight on `map01`, whose word stays `0`, opens the
//! same ring and must draw no mark.
//!
//! Rendered at 960x720: stage scale 3, origin `(0, 0)`. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_ui::battle_command_ui as bcu;
use legaia_web_viewer::runtime::LegaiaRuntime;

/// PSX pad Left: the round prompt's `Begin` chip.
const LEFT: u16 = 0x0080;

/// `dst` of the cross-out quad at the page's 960x720 stage, from the shared
/// builder - so the expectation is not transcribed from either host.
fn mark_dst() -> [i64; 4] {
    let d = bcu::cross_out_mark_sprite((0, 0, 64, 16), bcu::RASERU_MARK_ANCHOR, (0, 0), 3).dst;
    [d.0 as i64, d.1 as i64, d.2 as i64, d.3 as i64]
}

/// `(mark drawn, ring plates drawn)` this frame. The ring is recognised by
/// its Up arm's plate - a 20-px-tall (60 surface px) quad on the command
/// cluster's top row, which no other chip cluster uses.
fn frame_probe(rt: &mut LegaiaRuntime) -> (bool, bool) {
    let v: serde_json::Value =
        serde_json::from_str(&rt.play_overlay_draws_json(960, 720)).expect("overlay json");
    let want = mark_dst();
    let sprites = v["sprites"].as_array().cloned().unwrap_or_default();
    let mark = sprites
        .iter()
        .any(|s| (0..4).all(|i| s["dst"][i].as_i64() == Some(want[i])));
    let (_, up_y) = bcu::CLUSTER_COMMAND.plate_origin(bcu::ChipSeat::Up);
    let ring = sprites.iter().any(|s| {
        s["dst"][3].as_i64() == Some(60) && s["dst"][1].as_i64() == Some(i64::from(up_y) * 3)
    });
    (mark, ring)
}

/// A page with a seeded party (the new-game entry - a plain field visit
/// leaves the records empty and no command session opens), in `scene`, with
/// formation `row` forced.
fn forced_fight(scene: &str, row: i32) -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).expect("read disc");
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load disc");
    rt.debug_enter_town01_opening()
        .expect("enter the town01 opening");
    for _ in 0..8 {
        rt.tick_frame().expect("tick");
    }
    rt.enter_field(scene).expect("enter scene");
    for _ in 0..5 {
        rt.tick_frame().expect("tick");
    }
    assert!(!frame_probe(&mut rt).0, "no battle, no mark");
    assert!(rt.debug_force_battle(row), "{scene} row {row} arms");
    Some(rt)
}

/// Tick up to `frames` frames, answering the round prompt with `Begin`, until
/// the command ring's plates draw. Returns whether the mark drew on that
/// first ring frame.
fn first_ring_frame(rt: &mut LegaiaRuntime, frames: u32) -> Option<bool> {
    for f in 0..frames {
        if rt.play_battle_active() {
            let (mark, ring) = frame_probe(rt);
            if ring || mark {
                return Some(mark);
            }
        }
        let pad = if rt.play_battle_active() && f % 20 == 19 {
            LEFT
        } else {
            0
        };
        rt.set_pad(pad);
        rt.tick_frame().expect("tick");
    }
    None
}

#[test]
fn tetsus_fight_crosses_the_raseru_chip_out_on_the_page() {
    let Some(mut rt) = forced_fight("town0d", 4) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mark = first_ring_frame(&mut rt, 1200).expect("Tetsu's fight opened the command ring");
    assert!(mark, "the ring drew without the Ra-Seru cross-out");
    eprintln!("raseru cross-out: drawn on the page's command ring against 0xAF");

    let mut control = forced_fight("map01", -1).expect("disc present");
    let mark = first_ring_frame(&mut control, 1200).expect("the map01 fight opened the ring");
    assert!(!mark, "a fight whose special word is 0 drew the cross-out");
    eprintln!("raseru cross-out: absent from the map01 control ring");
}
