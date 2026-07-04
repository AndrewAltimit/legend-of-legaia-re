//! Disc-gated: the opening cutscene's narration is script-driven - the
//! timeline suspends at each inline narration block while the roller
//! presenter plays it, and the intro skip is available mid-narration.
//!
//! Cold-boots the prologue scene `opdeene` live through `SceneHost`, then
//! asserts the opening sequence the engine drives:
//!
//! 1. entering `opdeene` installs the cutscene timeline with its two inline
//!    narration blocks (14 + 8 pages) parsed as suspend sites - and does NOT
//!    pre-install any narration;
//! 2. ticking the world executes the timeline up to the first block, which
//!    installs the roller presenter (14 pages) and suspends the timeline;
//! 3. the roller crawls on its own timer; when the block completes the
//!    timeline resumes and later reaches the second block (8 pages);
//! 4. at any point after the timeline arms `GFLAG 26` (near its top), a
//!    confirm press skips the WHOLE remaining opening to `town01` - the
//!    retail `FUN_801D1344` intro-skip packet, available mid-narration.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

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
fn opdeene_narration_is_script_driven_and_skippable() {
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

    // 1. The timeline installed with the narration blocks parsed as suspend
    //    sites; no narration is on screen yet (script-driven, not
    //    scene-entry-driven).
    assert!(
        host.world.cutscene_timeline_active(),
        "entering opdeene installs the cutscene timeline"
    );
    let blocks: Vec<usize> = host
        .world
        .cutscene_timeline
        .as_ref()
        .map(|tl| tl.narration_blocks.iter().map(|b| b.pages.len()).collect())
        .unwrap_or_default();
    assert_eq!(
        blocks,
        vec![14, 8],
        "opdeene's timeline carries the 14-page + 8-page narration blocks"
    );
    assert!(
        !host.world.cutscene_narration_active(),
        "no narration before the timeline reaches its first block"
    );
    assert!(host.world.opening_chain_active, "the opening chain started");

    // 2. Ticking reaches the first block: the roller installs (14 pages) and
    //    the timeline suspends at the block op.
    let mut ticked = 0u32;
    while !host.world.cutscene_narration_active() && ticked < 600 {
        let _ = host.world.tick();
        ticked += 1;
    }
    assert!(
        host.world.cutscene_narration_active(),
        "the timeline reaches narration block 1 within {ticked} ticks"
    );
    let pages = host
        .world
        .cutscene_narration
        .as_ref()
        .map(|n| n.page_count())
        .unwrap_or(0);
    assert_eq!(pages, 14, "block 1 is the 14-page creation prologue");
    let suspended_at = host
        .world
        .cutscene_timeline
        .as_ref()
        .and_then(|tl| tl.narration_pc);
    assert!(
        suspended_at.is_some(),
        "the timeline is suspended at the block op"
    );
    eprintln!("[opdeene] block 1 (14 pages) installed after {ticked} ticks");

    // 3. The crawl completes on its own timer and the timeline resumes toward
    //    block 2. Budget: (pages + ring + slack) line steps at 64 frames each,
    //    plus generous slack for the choreography between blocks.
    let mut resumed = false;
    for _ in 0..12_000 {
        let _ = host.world.tick();
        if !host.world.cutscene_narration_active()
            && host
                .world
                .cutscene_timeline
                .as_ref()
                .is_some_and(|tl| tl.narration_pc.is_none())
        {
            resumed = true;
            break;
        }
    }
    assert!(resumed, "block 1 completes and the timeline resumes");

    // Reach block 2 (8 pages).
    let mut saw_block_2 = false;
    for _ in 0..12_000 {
        let _ = host.world.tick();
        if host
            .world
            .cutscene_narration
            .as_ref()
            .is_some_and(|n| n.page_count() == 8)
        {
            saw_block_2 = true;
            break;
        }
    }
    assert!(saw_block_2, "the timeline reaches the 8-page Seru block");

    // 4. Mid-narration intro skip: the hand-off bit was armed near the record
    //    top, so a confirm press now skips straight to town01.
    assert!(host.world.cutscene_narration_active());
    let target = host.world.take_prologue_handoff(true);
    assert_eq!(
        target,
        Some(legaia_asset::new_game::OPENING_SCENE),
        "a confirm mid-narration skips the opening to Rim Elm"
    );
    assert!(
        !host.world.cutscene_narration_active(),
        "the skip tears the narration down"
    );
}
