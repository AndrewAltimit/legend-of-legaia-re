//! Disc-gated pacing oracle for the `opdeene` cutscene record itself, at the
//! resolution of its own ops rather than the scene leg.
//!
//! A per-vsync PCSX-Redux capture of the zero-input leg
//! (`scripts/pcsx-redux/autorun_opdeene_pacing.lua`, from the
//! `s1_newgame_field` state) has the record leave its second opening wait
//! (`+0x289`) and execute its terminal SceneChange (`+0x111B`, the scene
//! label flip) 3828 display frames apart, and reach the `apply 4800` dolly
//! wait (`+0x4C9`) 1515 frames in. The span is set by the record's own
//! mechanisms - NPC turns that run while the record advances, end-latch
//! spins on the poked NPC clips, halt clears that end an actor's leg, `4C 45`
//! ramps that do not yield, and a `3F` that does not wait for the crawl - so
//! a regression in any of them moves it by tens to hundreds of frames. The
//! band allows for retail's three-frame step (`DAT_1F800393 == 3` in the
//! capture), which the engine's one-frame step does not reproduce.
//!
//! Skip-passes without disc data (CLAUDE.md convention).

use legaia_engine_core::scene::SceneHost;
use std::path::PathBuf;

/// Retail display frames from the record's arrival on `+0x289` to its `3F`.
const RETAIL_WAIT_TO_SCENE_CHANGE: f64 = 3828.0;
/// Retail display frames from the same arrival to the `apply 4800` wait.
const RETAIL_WAIT_TO_DOLLY: f64 = 1515.0;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn opdeene_record_reaches_its_scene_change_at_retail_pace() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.world.begin_new_game();
    host.enter_field_scene(legaia_asset::new_game::OPENING_CUTSCENE_SCENE, 0)
        .expect("enter opdeene");

    let (mut at_wait, mut at_dolly, mut at_change) = (None, None, None);
    for tick in 1..=8000u32 {
        let _ = host.tick();
        if let Some(tl) = host.world.cutscene.timeline.as_ref() {
            if at_wait.is_none() && tl.pc == 0x289 {
                at_wait = Some(tick);
            }
            if at_dolly.is_none() && tl.pc == 0x4C9 {
                at_dolly = Some(tick);
            }
        }
        if host.world.scene_transition_hold.is_some() {
            at_change = Some(tick);
            break;
        }
    }
    let at_wait = at_wait.expect("the record reaches its second opening wait");
    let dolly = f64::from(at_dolly.expect("the record reaches the dolly wait") - at_wait);
    let change = f64::from(at_change.expect("the record runs its SceneChange") - at_wait);
    eprintln!(
        "[opdeene] dolly {dolly} vs retail {RETAIL_WAIT_TO_DOLLY}; \
         scene change {change} vs retail {RETAIL_WAIT_TO_SCENE_CHANGE}"
    );
    assert!(
        (dolly - RETAIL_WAIT_TO_DOLLY).abs() / RETAIL_WAIT_TO_DOLLY < 0.03,
        "the record reaches the apply-4800 wait {dolly} frames in, retail {RETAIL_WAIT_TO_DOLLY}"
    );
    assert!(
        (change - RETAIL_WAIT_TO_SCENE_CHANGE).abs() / RETAIL_WAIT_TO_SCENE_CHANGE < 0.03,
        "the record runs its SceneChange {change} frames in, retail \
         {RETAIL_WAIT_TO_SCENE_CHANGE}"
    );
}
