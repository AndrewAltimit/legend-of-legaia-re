//! Page ladder for the SFX ring's **replace-last** op (`FUN_80035BD0`,
//! `legaia_engine_audio::sfx_ring::SfxCueRing::replace_last`) - a
//! `docs/tooling/reach-triage.md` row whose producer every headless ladder
//! runs and whose consumer none of them holds.
//!
//! `World::replace_last_sfx_cue` is the producer: the world-map panel's
//! sub-list confirm (`jal 0x80035BD0` at `0x801ED65C`, cue `0x20`), its text
//! box, the tile-board bonk, the Baka hub's confirm stings, the Incense
//! notice. It only queues an `SfxRingOp::ReplaceLast`; the ring itself lives
//! in each host's SFX scheduler - the native `AudioBgmDirector`, which exists
//! only with a live audio device, and the browser play page's `PlaySfx`,
//! which exists on every build. So the play page is the one host a test can
//! drive to the ring, and this is that drive: enter the overworld, open the
//! sub-list with Square (the chord the world-map controller installs it on
//! while its debug flag is up, which both hosts raise on world-map entry),
//! confirm with Cross, and require the page's scheduler to bring cue `0x20`
//! due.
//!
//! The assertion is the cue, not the count: ambient producers push ring cues
//! too, so "some ring cue came due" would pass without the replace. Cue
//! `0x20` is the sub-list confirm's own id, and the replace zeroes its slot's
//! countdown, so it is due on the next drain.
//!
//! Coverage export: the recipe in `scripts/ci/replay-port-coverage.py`.
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::input::PadButton;
use legaia_engine_vm::world_map_panel_actors::SUBLIST_CONFIRM_SFX;
use legaia_web_viewer::runtime::LegaiaRuntime;

fn tick(rt: &mut LegaiaRuntime, n: usize) {
    for _ in 0..n {
        rt.tick_frame().expect("tick_frame");
    }
}

/// One press: a frame with the bit down, then one at neutral.
fn tap(rt: &mut LegaiaRuntime, mask: u16) {
    rt.set_pad(mask);
    tick(rt, 1);
    rt.set_pad(0);
    tick(rt, 1);
}

fn sfx_state(rt: &LegaiaRuntime) -> serde_json::Value {
    serde_json::from_str(&rt.play_sfx_state_json()).expect("sfx state json")
}

#[test]
fn world_map_sublist_confirm_replaces_the_last_ring_cue_on_the_page() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let bytes = std::fs::read(&disc).expect("read disc");
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load disc");
    rt.enter_field("map01").expect("enter map01");
    assert_eq!(rt.scene_mode(), "WorldMap", "map01 is the overworld");
    tick(&mut rt, 30);

    // Square opens the sub-list; its open arm runs on the next frame.
    tap(&mut rt, PadButton::Square.mask());
    tick(&mut rt, 4);
    // Cross confirms row 0 - the close row - which is the replace-last.
    tap(&mut rt, PadButton::Cross.mask());

    let mut due = None;
    for _ in 0..8 {
        let st = sfx_state(&rt);
        if st["last_ring_cue"].as_i64() == Some(i64::from(SUBLIST_CONFIRM_SFX)) {
            due = Some(st);
            break;
        }
        tick(&mut rt, 1);
    }
    let st = due.unwrap_or_else(|| {
        panic!(
            "the sub-list confirm never brought cue {SUBLIST_CONFIRM_SFX:#x} due on the page's \
             ring: {}",
            sfx_state(&rt)
        )
    });
    assert!(st["ring_due"].as_u64().unwrap_or(0) > 0, "{st}");
    eprintln!("[ok] sub-list confirm cue due on the page ring: {st}");
}
