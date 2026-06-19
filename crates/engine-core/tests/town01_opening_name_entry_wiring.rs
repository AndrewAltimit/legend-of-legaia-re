//! Disc-gated integration: the `town01` opening cutscene timeline drives the
//! name-entry overlay open at its pinned op-`0x49` STATE_RESUME, suspends while
//! the player names the lead, then resumes - mirroring retail.
//!
//! Exercises the wiring end to end:
//! 1. entering `town01` via the new-game prologue hand-off installs the opening
//!    cutscene timeline (`World::install_town01_opening_timeline`, gated on
//!    `entering_town01_opening`);
//! 2. ticking the world runs the timeline through the establishing camera sweep
//!    (it steps past the `0x4C` script-alloc parks) until it reaches the pinned
//!    op-`0x49` at body offset `0x02c6`, where it opens name entry and parks;
//! 3. the timeline is frozen while name entry is open, then resumes (and the
//!    cutscene camera reverts) once the player commits a name.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::name_entry::{NameEntryInput, NameEntryState};
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::World;
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

/// Body offset of the name-entry STATE_RESUME the timeline must park on.
const NAME_ENTRY_OP49_OFFSET: usize = 0x02c6;

#[test]
fn town01_opening_timeline_opens_name_entry_at_op49() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");

    // Simulate the opdeene→town01 prologue hand-off: the flag `take_prologue_handoff`
    // would set, telling the town01 entry to install the opening timeline.
    host.world.entering_town01_opening = true;
    host.enter_field_scene(legaia_asset::new_game::OPENING_SCENE, 0)
        .expect("enter town01");

    // 1. The opening timeline installed and is armed to open name entry; the
    //    one-shot entry flag was consumed.
    assert!(
        host.world.cutscene_timeline_active(),
        "town01 opening timeline installs on the prologue hand-off"
    );
    assert!(
        host.world.prologue_naming_pending,
        "the timeline is armed to open name entry at its op-0x49"
    );
    assert!(
        !host.world.entering_town01_opening,
        "the one-shot opening flag is consumed by the entry"
    );
    assert!(
        !host.world.name_entry_active(),
        "name entry is NOT open at scene entry - it opens when the timeline reaches op-0x49"
    );

    // 2. Tick until the timeline reaches its op-0x49 and opens name entry.
    let mut ticks = 0u32;
    while !host.world.name_entry_active() && ticks < 4000 {
        host.world.tick();
        ticks += 1;
    }
    assert!(
        host.world.name_entry_active(),
        "the timeline opens name entry within the establishing sweep (ticked {ticks})"
    );
    assert!(
        ticks > 1,
        "name entry opens partway through the opening (after camera/wait beats), not instantly"
    );
    assert!(
        host.world.prologue_naming_armed,
        "the op-0x49 host hook armed the name-entry handoff"
    );
    // The timeline is parked exactly on the pinned op-0x49 (body 0x02c6).
    let parked_pc = host.world.cutscene_timeline.as_ref().map(|t| t.pc);
    assert_eq!(
        parked_pc,
        Some(NAME_ENTRY_OP49_OFFSET),
        "the timeline is suspended on the pinned name-entry STATE_RESUME"
    );
    eprintln!("[town01] name entry opened by timeline op-0x49 at tick {ticks}");

    // 3. The timeline is frozen while name entry is open: ticking does not
    //    advance its frame counter or PC.
    let frames_before = host.world.cutscene_timeline.as_ref().map(|t| t.frames);
    for _ in 0..30 {
        host.world.tick();
    }
    assert!(
        host.world.name_entry_active(),
        "name entry stays open while the player has not committed"
    );
    assert_eq!(
        host.world.cutscene_timeline.as_ref().map(|t| t.frames),
        frames_before,
        "the timeline is frozen (no frame-cap progress) while name entry is up"
    );
    assert_eq!(
        host.world.cutscene_timeline.as_ref().map(|t| t.pc),
        Some(NAME_ENTRY_OP49_OFFSET),
        "the timeline stays parked on the op-0x49 while name entry is up"
    );

    // 4. Commit a name: type one glyph (so the name is non-empty), then go to
    //    End → confirm → Yes.
    host.world.name_entry.as_mut().unwrap().cursor = 0; // 'A'
    host.world.step_name_entry(NameEntryInput {
        confirm: true,
        ..Default::default()
    });
    let end = legaia_engine_core::name_entry::CHAR_CELLS + 16;
    host.world.name_entry.as_mut().unwrap().cursor = end;
    host.world.step_name_entry(NameEntryInput {
        confirm: true,
        ..Default::default()
    });
    assert_eq!(
        host.world.name_entry.as_ref().unwrap().state,
        NameEntryState::Confirm,
        "End opens the Yes/No confirm"
    );
    let committed = host.world.step_name_entry(NameEntryInput {
        confirm: true,
        ..Default::default()
    });
    assert!(committed, "Yes commits and closes name entry");
    assert!(!host.world.name_entry_active());
    assert!(
        !host.world.party_name(0).is_empty(),
        "the committed lead name persists"
    );

    // 5. The timeline resumes (op-0x49 now Done) and eventually completes,
    //    dropping itself so the view reverts from the cutscene camera.
    let mut more = 0u32;
    while host.world.cutscene_timeline.is_some() && more < 4000 {
        host.world.tick();
        more += 1;
    }
    assert!(
        host.world.cutscene_timeline.is_none(),
        "the opening timeline completes after naming and is dropped (ticked {more} more)"
    );
    // Sanity: the record index constant matches the disc invariant.
    assert_eq!(World::TOWN01_OPENING_TIMELINE_RECORD, 3);
}
