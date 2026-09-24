//! Disc-gated: the play page composites `dolk`'s attached light - the field
//! VM's op `0x34` sub-1 darkness mask - for as long as the scene runs.
//!
//! `World::field_light_draws` is shared, and the native window's twin draw is
//! pinned by `engine-core`'s `attached_light_retail_capture_disc`. What no
//! page test asserted is that the page's per-tick screen-prim cache
//! (`tick_battle_intro` -> `field_light_prims`) carries the mask as a
//! **subtractive** run (class code `1 + abr` with `abr = 2`) on a field
//! frame, and keeps carrying it across the span the native window draws it
//! through. The run is what the page's `ScreenPrimPass` turns into
//! `FUNC_REVERSE_SUBTRACT`.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;

/// Screen-prim run class code of a subtractive (`abr = 2`) run.
const SUBTRACTIVE_RUN: u32 = 1 + 2;

fn loaded_runtime() -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    Some(rt)
}

/// Index count of this frame's subtractive runs.
fn subtractive_indices(rt: &LegaiaRuntime) -> u32 {
    rt.play_screen_prim_runs()
        .chunks(3)
        .filter(|r| r.len() == 3 && r[0] == SUBTRACTIVE_RUN)
        .map(|r| r[2])
        .sum()
}

#[test]
fn dolk_darkness_mask_stays_on_the_page_screen_prim_pass() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    rt.enter_field("dolk").expect("enter dolk");
    let mut first = None;
    let mut missing_after_first = Vec::new();
    for tick in 0..1300u32 {
        let _ = rt.tick_frame();
        let n = subtractive_indices(&rt);
        if n > 0 {
            first.get_or_insert(tick);
        } else if first.is_some() {
            missing_after_first.push(tick);
        }
    }
    eprintln!("[ok] dolk mask first on the page pass at tick {first:?}");
    assert!(
        first.is_some_and(|t| t < 30),
        "the page never carried dolk's darkness mask early (first {first:?})"
    );
    assert!(
        missing_after_first.is_empty(),
        "the mask dropped off the page pass on {} tick(s), first {:?}",
        missing_after_first.len(),
        missing_after_first.first()
    );
}
