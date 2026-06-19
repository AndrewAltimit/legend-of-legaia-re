//! Disc-gated: the opening cutscene installs and plays its inline narration,
//! gating the Rim Elm hand-off until the narration finishes.
//!
//! Cold-boots the prologue scene `opdeene` live through `SceneHost`, then
//! asserts the full opening sequence the engine drives:
//!
//! 1. entering `opdeene` installs the inline narration (the 22 subtitle pages
//!    decoded from the cutscene-timeline partition) and arms the hand-off;
//! 2. while the narration is on screen the hand-off gate stays closed - a
//!    confirm press does **not** jump to `town01`;
//! 3. ticking advances the narration's per-page timer to completion;
//! 4. once complete, a confirm press releases the hand-off to `town01`.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::cutscene_narration::DEFAULT_PAGE_FRAMES;
use legaia_engine_core::scene::SceneHost;
use std::path::PathBuf;

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
fn opdeene_plays_narration_then_releases_the_handoff() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let cutscene = legaia_asset::new_game::OPENING_CUTSCENE_SCENE;
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene(cutscene, 0).expect("enter opdeene");

    // 1. The narration installed (the 22 inline subtitle pages) and the
    //    cutscene timeline installed (the spawned context that fires the
    //    hand-off bit by execution). The hand-off flag is NOT armed yet - it
    //    arms once the timeline executes its `GFLAG_SET 26`.
    assert!(
        host.world.cutscene_narration_active(),
        "entering opdeene installs the inline narration"
    );
    let pages = host
        .world
        .cutscene_narration
        .as_ref()
        .map(|n| n.page_count())
        .unwrap_or(0);
    assert_eq!(pages, 22, "opdeene carries 22 inline narration pages");
    assert!(
        host.world.cutscene_timeline_active(),
        "entering opdeene installs the cutscene timeline"
    );

    // 2. While the narration plays, the hand-off gate stays closed even on a
    //    confirm press - independent of whether the timeline has armed the bit.
    assert!(
        host.world.take_prologue_handoff(true).is_none(),
        "the hand-off is gated until the narration finishes"
    );

    // 3. Tick the world until the narration completes. Each page dwells
    //    DEFAULT_PAGE_FRAMES; 22 pages need at most 22 * that many ticks plus
    //    slack. The host freezes input here, so World::tick advances the
    //    per-page timer.
    let budget = (pages as u32 + 2) * DEFAULT_PAGE_FRAMES;
    let mut ticked = 0u32;
    while host.world.cutscene_narration_active() && ticked < budget {
        let _ = host.world.tick();
        ticked += 1;
    }
    assert!(
        !host.world.cutscene_narration_active(),
        "narration completes within its per-page timer budget (ticked {ticked})"
    );
    eprintln!("[opdeene] narration completed after {ticked} ticks");

    // 4. With the narration done and the scene still `opdeene`, a confirm
    //    press releases the hand-off to town01.
    let target = host.world.take_prologue_handoff(true);
    assert_eq!(
        target,
        Some(legaia_asset::new_game::OPENING_SCENE),
        "completing the narration releases the Rim Elm hand-off"
    );
}
