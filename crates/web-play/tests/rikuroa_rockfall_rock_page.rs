//! Disc-gated: the play page draws Mt. Rikuroa's fallen rocks lying across
//! the cave path, as the native window's field-FX pass does.
//!
//! With the rockfalls behind the party (flags `0x281` / `0x282`), the
//! scene's entry tree seats both rocks (stager records `69` / `70`, pack
//! slots `58` / `59`). Each is a stalactite at rest and arms VDF lanes with
//! move-VM op `0x0A`; the case-5 model draw `FUN_8001ADA4` stages them
//! through `FUN_8001C604` while the part's `+0x10` bit `0x1000` is up. The
//! page baked the part's rest mesh, so each rock hung in the roof and the
//! player hopped over an empty path (`World::part_morphed_tmd`).
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_play::runtime::LegaiaRuntime;

const W: f32 = 320.0;
const H: f32 = 240.0;

/// The second rock's footprint on the path, world X / Z: columns `63..=68`
/// of rows `109..=110`.
const BOX_X: (f32, f32) = (8100.0, 8800.0);
const BOX_Z: (f32, f32) = (13900.0, 14250.0);

/// The rock's baked vertices (retail Y down) after `ticks` page frames in
/// `rikuroa` entered with `flags` raised.
fn rock_vertices(flags: &[u16], ticks: usize) -> Option<Vec<[f32; 3]>> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let bytes = std::fs::read(&disc).expect("read disc");
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load disc");
    for &f in flags {
        assert!(rt.set_system_flag(f, true));
    }
    rt.enter_field("rikuroa").expect("enter rikuroa");
    for _ in 0..ticks {
        rt.set_pad(0);
        rt.tick_frame().expect("tick_frame");
    }
    let vp = rt.play_camera_vp(W, H);
    rt.play_field_fx_sync(&vp);
    let mut pos = rt.play_battle_fx_positions();
    pos.extend(rt.play_battle_fx_far_positions());
    // The page bakes in its Y-up frame: Y is retail's negated.
    Some(
        pos.as_chunks::<3>()
            .0
            .iter()
            .map(|c| [c[0], -c[1], c[2]])
            .filter(|p| (BOX_X.0..BOX_X.1).contains(&p[0]) && (BOX_Z.0..BOX_Z.1).contains(&p[2]))
            .collect(),
    )
}

#[test]
fn the_page_draws_the_fallen_rock_on_the_path() {
    let Some(rock) = rock_vertices(&[0x2FB, 0x281, 0x282], 240) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let (lo, hi) = rock.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
        (lo.min(p[1]), hi.max(p[1]))
    });
    eprintln!("{} rock vertices, retail Y {lo}..{hi}", rock.len());
    assert!(!rock.is_empty(), "the rock is baked over the path");
    // Lying on the path, not hanging in the roof (retail Y down: the rest
    // stalactite spans about -910..-412 there).
    assert!(
        lo > -400.0,
        "the rock is staged, not the rest stalactite: {lo}..{hi}"
    );
}

/// Once the Golem record `P2[47]` raises `0x300` ("the cave is about to
/// collapse"), each rock's program leaves its `X_IF_NOT_FLAG 0x300` loop and
/// halts, and retail draws neither: a card-loaded retail state holding
/// `0x300` keeps both parts in the pool with `+0x10` bit `0x8` up, and a
/// film walked over the rock's tiles from it shows the path bare.
#[test]
fn the_rocks_are_gone_once_the_cave_collapses() {
    let Some(rock) = rock_vertices(&[0x2FB, 0x281, 0x282, 0x283, 0x284, 0x286, 0x300], 240) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(rock.is_empty(), "{} rock vertices baked", rock.len());
}
