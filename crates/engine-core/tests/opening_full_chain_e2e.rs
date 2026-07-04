//! Disc-gated end-to-end oracle for the whole new-game opening chain:
//! `opdeene` prologue cutscene → narration → confirm hand-off → `town01`
//! opening timeline → name entry → free-roam.
//!
//! Every other opening test exercises one leg in isolation (`opdeene_narration_playback`
//! stops at the narration; `town01_opening_name_entry_wiring` jump-starts at
//! `town01` by setting `entering_town01_opening` directly). This test cold-boots
//! the prologue and drives the *complete* chain the windowed host drives, so a
//! regression in the seam between legs (the hand-off returning the wrong scene,
//! the `town01` timeline not installing off the real hand-off flag, name entry
//! never opening) is caught here rather than only in the live window.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::name_entry::{NameEntryInput, NameEntryState};
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
fn new_game_opening_runs_opdeene_narration_then_town01_name_entry() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let opdeene = legaia_asset::new_game::OPENING_CUTSCENE_SCENE;
    let town01 = legaia_asset::new_game::OPENING_SCENE;

    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");

    // --- Beat 1: NEW GAME seeds the party and enters the prologue cutscene ---
    host.world.begin_new_game();
    host.enter_field_scene(opdeene, 0).expect("enter opdeene");
    assert!(
        host.world.cutscene_narration_active(),
        "entering opdeene installs the inline narration"
    );
    assert!(
        host.world.cutscene_timeline_active(),
        "entering opdeene installs the cutscene timeline"
    );

    // --- Beat 2/3: the narration plays; the hand-off is gated until it ends ---
    // A confirm mid-narration must NOT jump to town01.
    assert!(
        host.world.take_prologue_handoff(true).is_none(),
        "the hand-off is gated closed while the narration is on screen"
    );
    // Tick to narration completion (the host freezes input during narration, so
    // the per-page dwell timer advances). Generous cap over 22 pages * 120.
    let mut ticks = 0u32;
    while host.world.cutscene_narration_active() && ticks < 22 * 120 + 600 {
        host.world.tick();
        ticks += 1;
    }
    assert!(
        !host.world.cutscene_narration_active(),
        "the narration finishes within its dwell budget (ticked {ticks})"
    );

    // Still in opdeene, the hand-off flag is armed (the timeline executed its
    // GFLAG_SET 26 - or the safety net set it), so a confirm now hands off.
    let target = host.world.take_prologue_handoff(true);
    assert_eq!(
        target,
        Some(town01),
        "completing the narration + a confirm hands off to town01"
    );
    assert!(
        host.world.entering_town01_opening,
        "the hand-off marks the town01 entry as the new-game opening"
    );

    // --- Beat 4: entering town01 installs the opening timeline + name entry ---
    host.enter_field_scene(town01, 0).expect("enter town01");
    assert!(
        host.world.cutscene_timeline_active(),
        "town01's opening timeline installs off the hand-off flag"
    );
    assert!(
        !host.world.entering_town01_opening,
        "the one-shot opening flag is consumed by the town01 entry"
    );
    // The opdeene narration did not leak into town01.
    assert!(
        !host.world.cutscene_narration_active(),
        "town01 carries no prologue narration"
    );

    // Tick the establishing sweep until the timeline parks on op-0x49 and opens
    // the name-entry overlay.
    let mut sweep = 0u32;
    while !host.world.name_entry_active() && sweep < 4000 {
        host.world.tick();
        sweep += 1;
    }
    assert!(
        host.world.name_entry_active(),
        "the town01 opening timeline opens name entry (swept {sweep})"
    );

    // --- Beat 5: commit a name -> the timeline resumes and completes ---
    // Type one glyph ('A'), then End -> Yes.
    host.world.name_entry.as_mut().unwrap().cursor = 0;
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
    assert!(
        !host.world.party_name(0).is_empty(),
        "the committed lead name persists into the party"
    );

    // The opening choreography `MoveTo`s the townsfolk to the off-map hide box
    // while it plays; track that it engages so the restore assertion below is
    // non-vacuous.
    let hide = legaia_engine_core::world::FIELD_OFFMAP_HIDE_XZ;
    let parked_at_hide = |w: &legaia_engine_core::world::World| {
        w.field_npc_positions
            .values()
            .any(|&(x, z)| x == hide && z == hide)
    };
    let mut more = 0u32;
    let mut saw_hidden_during_sweep = false;
    while host.world.cutscene_timeline.is_some() && more < 4000 {
        host.world.tick();
        saw_hidden_during_sweep |= parked_at_hide(&host.world);
        more += 1;
    }
    assert!(
        host.world.cutscene_timeline.is_none(),
        "the opening timeline completes after naming and reverts to free-roam (ticked {more})"
    );
    assert!(
        saw_hidden_during_sweep,
        "the opening cutscene parks townsfolk at the off-map hide box while it plays"
    );
    // Regression (the "town NPCs vanish after New Game" bug): the timeline
    // completion must un-park every villager the opening hid, or the field
    // render draws them off-screen. Their `field_npc_positions` overrides are
    // dropped so each reverts to its MAN spawn tile.
    assert!(
        !parked_at_hide(&host.world),
        "no field NPC is left at the off-map hide box once free-roam resumes"
    );
    eprintln!("[opening] full chain opdeene->town01 name entry completed");
}
