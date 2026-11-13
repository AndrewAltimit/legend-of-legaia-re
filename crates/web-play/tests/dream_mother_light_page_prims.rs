//! Disc-gated: Noa's first dream draws the light she chases.
//!
//! `dream` `P2[1]` (the first dream, after the `retock` inn) hangs her
//! mother's voice on `P1[2]` - target `0x08`, a marker placement whose spawn
//! prologue zeroes its render scale (`4C 40 00 00`) - as an attached light:
//! `B4 08 10 ..`, colour black with a `0x40` keyframe script that ramps it to
//! `D0 D0 AC` at extents `0x300 x 0x400`, the field VM's op `0x34` sub-1
//! (blend mode 1, additive). `B4 08 20 ..` re-scripts it as the voice speaks
//! and moves, and Noa walks after it.
//!
//! The light's tick `FUN_801E4470` reads two bits of its parent's `+0x10`
//! and nothing else of its state (`0x801E4490..0x801E44B8`): `8` retires it,
//! `2` (the visibility cull) skips the frame. The port had also read a zero
//! render scale as hidden, so the marker's own invisibility took the light
//! with it and the dream showed only Noa and her shadow.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_play::runtime::LegaiaRuntime;

/// Cross, the Confirm that pages a field conversation.
const CROSS: u16 = 0x4000;
/// Screen-prim run class code of an additive (`abr = 1`) run.
const ADDITIVE_RUN: u32 = 1 + 1;

fn loaded_runtime() -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    Some(rt)
}

/// Index count of this frame's additive runs.
fn additive_indices(rt: &LegaiaRuntime) -> u32 {
    rt.play_screen_prim_runs()
        .chunks(3)
        .filter(|r| r.len() == 3 && r[0] == ADDITIVE_RUN)
        .map(|r| r[2])
        .sum()
}

#[test]
fn the_first_dream_draws_the_mothers_light() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    rt.enter_field("dream").expect("enter dream");
    assert!(rt.debug_play_record(2, 1), "dream P2[1] resolves");
    let mut lit = 0u32;
    for f in 0..2400u32 {
        rt.set_pad(if f > 120 && f % 24 < 2 { CROSS } else { 0 });
        let _ = rt.tick_frame();
        if additive_indices(&rt) > 0 {
            lit += 1;
        }
    }
    eprintln!("[ok] dream: the mother's light drew on {lit} frame(s)");
    assert!(
        lit > 300,
        "the first dream's light must draw through the beat ({lit} frames)"
    );
}
