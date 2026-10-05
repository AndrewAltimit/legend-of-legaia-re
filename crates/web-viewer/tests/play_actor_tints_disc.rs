//! Disc-gated: the browser play page stages op `4C 81` draw tints on the
//! overworld landmarks and on field NPCs, off the same world tables the
//! native play-window reads (`World::object_draw_tints`,
//! `World::field_npc_draw_tint`).
//!
//! 1. A kingdom MAN's partition-0 records tint their landmark actors in
//!    their spawn prologue (map02's records 2.., black at a flag-picked
//!    blend). The overworld resolves its landmarks without binds, so the
//!    page used to key no landmark to a record and drew none of them tinted.
//! 2. `kor`'s NPC 12 runs `4C 81 .. 00 10 ..` in its own record: the page's
//!    per-NPC tint export carries it.
//!
//! No Sony bytes are asserted. Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;

fn runtime() -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    Some(rt)
}

#[test]
fn overworld_landmarks_carry_their_actor_tint() {
    let Some(mut rt) = runtime() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    rt.enter_field("map02").expect("enter map02");
    let tints = rt.field_placement_tints();
    assert!(!tints.is_empty(), "map02's landmark tints reach the page");
    let tinted = tints.chunks(4).filter(|c| c[3] > 0.0).count();
    let deco = rt.field_decoration_start() as usize;
    assert!(tinted > 0, "at least one landmark draws tinted");
    assert!(
        tints.chunks(4).skip(deco).all(|c| c[3] == 0.0),
        "a decoration has no actor to tint"
    );
    eprintln!("[ran] map02: {tinted} tinted landmark draws of {deco}");
}

#[test]
fn a_field_npc_tint_reaches_the_page() {
    let Some(mut rt) = runtime() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    rt.enter_field("kor").expect("enter kor");
    for _ in 0..30 {
        rt.tick_frame().expect("tick");
    }
    let tints = rt.play_npc_tints();
    let full = tints.chunks(4).filter(|c| c[3] >= 1.0).count();
    assert!(
        full > 0,
        "kor's black-tinted NPC carries its tint: {tints:?}"
    );
    assert!(
        rt.play_player_tint().is_empty(),
        "nothing tints the player on entry"
    );
    eprintln!("[ran] kor: {full} NPC(s) tinted at full blend");
}
