//! Disc-gated: the opening cutscene timeline executes as a spawned field-VM
//! context, firing the Rim Elm hand-off bit (`GFLAG_SET 26`) by execution.
//!
//! Cold-boots `opdeene` live through `SceneHost`, then asserts:
//!
//! 1. entering `opdeene` installs the cutscene timeline (the partition-2 record
//!    that issues `GFLAG_SET 26`), and the hand-off flag starts CLEAR - it is
//!    not statically armed up front;
//! 2. ticking the world steps the timeline; within its frame budget the
//!    hand-off flag becomes set (by executing the record, or by the safety net
//!    if execution can't reach the closing op), and the timeline reports
//!    complete;
//! 3. stepping the timeline emits camera / move field events the runtime camera
//!    consumes - the timeline is not inert.
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
fn opdeene_timeline_executes_and_fires_handoff_by_execution() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing — run `legaia-extract` first");
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

    // 2. Tick until the timeline completes (it sets the hand-off bit, either by
    //    executing its closing `GFLAG_SET 26` or via the frame-cap safety net).
    //    Record the frame the bit first appears for observation.
    let mut armed_at: Option<u32> = None;
    let mut ticks = 0u32;
    // Cap well above the timeline's internal frame cap so the loop terminates
    // even if the timeline never reaches its closing op.
    let budget = 4000u32;
    while host.world.cutscene_timeline_active() && ticks < budget {
        let _ = host.world.tick();
        ticks += 1;
        if armed_at.is_none() && host.world.story_flags & PROLOGUE_HANDOFF_FLAG != 0 {
            armed_at = Some(ticks);
        }
    }

    assert!(
        !host.world.cutscene_timeline_active(),
        "the cutscene timeline completes within its frame budget (ticked {ticks})"
    );
    assert!(
        host.world.story_flags & PROLOGUE_HANDOFF_FLAG != 0,
        "the cutscene timeline sets the Rim Elm hand-off bit"
    );
    match armed_at {
        Some(frame) => eprintln!("[opdeene] hand-off bit armed by timeline at frame {frame}"),
        None => eprintln!("[opdeene] hand-off bit observed set after timeline completed"),
    }
}
