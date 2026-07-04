//! Disc-gated: the opening cutscene timeline executes as a spawned field-VM
//! context - arming the intro-skip bit (`GFLAG_SET 26`) by execution and
//! chaining to the next opening scene (`opstati`) via its terminal
//! `SceneChange` (`0x3F`) op.
//!
//! Cold-boots `opdeene` live through `SceneHost` (World-only ticking, so the
//! queued scene transition is observable rather than consumed), then asserts:
//!
//! 1. entering `opdeene` installs the cutscene timeline (the partition-2 record
//!    that issues `GFLAG_SET 26`), and the hand-off flag starts CLEAR - it is
//!    not statically armed up front;
//! 2. ticking the world steps the timeline; the skip bit arms by execution
//!    (the `GFLAG_SET 26` near the record top);
//! 3. the timeline plays through its narration suspensions + choreography and
//!    executes its closing `SceneChange` to `opstati` - the natural opening
//!    chain, queued as a pending named scene transition.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::PROLOGUE_HANDOFF_FLAG;
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
fn opdeene_timeline_executes_arms_skip_and_chains_to_opstati() {
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

    // 1. The timeline installed; the hand-off bit is not armed up front.
    assert!(
        host.world.cutscene_timeline_active(),
        "entering opdeene installs the cutscene timeline"
    );
    assert_eq!(
        host.world.story_flags & PROLOGUE_HANDOFF_FLAG,
        0,
        "the hand-off bit is NOT statically armed at scene entry - it fires by execution",
    );

    // 2 + 3. Tick until the timeline chains to the next opening scene. The
    //    budget covers both narration crawls (the timeline suspends while the
    //    roller plays ~22 lines) plus the camera/wait choreography.
    let mut armed_at: Option<u32> = None;
    let mut chained: Option<String> = None;
    let mut ticks = 0u32;
    let budget = 20_000u32;
    while ticks < budget {
        let _ = host.world.tick();
        ticks += 1;
        if armed_at.is_none() && host.world.story_flags & PROLOGUE_HANDOFF_FLAG != 0 {
            armed_at = Some(ticks);
        }
        if let Some((name, _, _, _)) = host.world.pending_named_scene_transition.as_ref() {
            chained = Some(name.clone());
            break;
        }
    }

    assert!(
        armed_at.is_some(),
        "the timeline arms the intro-skip bit by execution (ticked {ticks})"
    );
    assert_eq!(
        chained.as_deref(),
        Some("opstati"),
        "the timeline's terminal SceneChange chains to opstati (ticked {ticks})"
    );
    eprintln!(
        "[opdeene] skip bit armed at tick {:?}; chained to opstati at tick {ticks}",
        armed_at
    );
}
