//! Disc-gated end-to-end oracle for the whole new-game opening chain.
//!
//! Retail (pinned by a PCSX-Redux cold-boot pixel capture) plays the opening
//! with ZERO input: `opdeene` (creation-myth crawl, 14+8 lines) chains by its
//! timeline's terminal `SceneChange` into `opstati` (Seru crawl, 3+6 lines,
//! spawned by op-`0x44` in its entry script), then `opurud` (Mist crawl,
//! 12 lines, op-`0x44`), then the `map01` world-map fly-in, then `town01`
//! (establishing pan -> name entry -> Vahn's walk-out). A confirm press at
//! any point after `opdeene` arms `GFLAG 26` skips the rest of the opening
//! straight to `town01` (retail `FUN_801D1344`).
//!
//! This test cold-boots NEW GAME and drives the natural (no-input) chain
//! through `SceneHost::tick`, asserting each scene hand-off and the narration
//! blocks along the way; a second test asserts the skip path into `town01`
//! name entry.
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

fn skip_or_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return None;
    };
    Some(SceneHost::open_extracted(&extracted).expect("open SceneHost"))
}

#[test]
fn new_game_opening_chains_naturally_with_zero_input() {
    let Some(mut host) = skip_or_host() else {
        return;
    };
    let opdeene = legaia_asset::new_game::OPENING_CUTSCENE_SCENE;

    // --- NEW GAME: seed and enter the prologue cutscene ---
    host.world.begin_new_game();
    host.enter_field_scene(opdeene, 0).expect("enter opdeene");
    assert!(host.world.cutscene_timeline_active());
    assert!(host.world.opening_chain_active);
    assert!(
        !host.world.cutscene_narration_active(),
        "narration is script-driven, not a scene-entry install"
    );

    // --- opdeene: two crawls (14 + 8) play, then the timeline chains ---
    //
    // Blocks are observed via `cutscene_narration_seq` (incremented per crawl
    // open), NOT a rising-edge `active` watch: a non-blocking crawl lets the
    // next block open the same tick the prior scrolls out (continuous crawl,
    // no blank frame), which a rising-edge watch would merge into one block.
    let mut seen_block_pages: Vec<usize> = Vec::new();
    let mut last_seq = host.world.cutscene_narration_seq;
    let mut ticks = 0u32;
    while host.world.active_scene_label == opdeene && ticks < 30_000 {
        let _ = host.tick();
        ticks += 1;
        let seq = host.world.cutscene_narration_seq;
        if seq != last_seq {
            seen_block_pages.push(
                host.world
                    .cutscene_narration
                    .as_ref()
                    .map(|n| n.page_count())
                    .unwrap_or(0),
            );
            last_seq = seq;
        }
    }
    assert_eq!(
        host.world.active_scene_label, "opstati",
        "opdeene chains to opstati by its terminal SceneChange (ticked {ticks})"
    );
    assert_eq!(
        seen_block_pages,
        vec![14, 8],
        "opdeene played its two narration crawls in script order"
    );

    // --- opstati: its entry script op-0x44 spawns P2[0]; crawls 3 + 6 ---
    let mut seen: Vec<usize> = Vec::new();
    last_seq = host.world.cutscene_narration_seq;
    ticks = 0;
    while host.world.active_scene_label == "opstati" && ticks < 30_000 {
        let _ = host.tick();
        ticks += 1;
        let seq = host.world.cutscene_narration_seq;
        if seq != last_seq {
            seen.push(
                host.world
                    .cutscene_narration
                    .as_ref()
                    .map(|n| n.page_count())
                    .unwrap_or(0),
            );
            last_seq = seq;
        }
    }
    assert_eq!(
        host.world.active_scene_label, "opurud",
        "opstati chains to opurud (ticked {ticks})"
    );
    assert_eq!(seen, vec![3, 6], "opstati played its two Seru crawls");

    // --- opurud: op-0x44 spawns P2[9]; three Mist crawls; chains to map01 ---
    let mut blocks = 0usize;
    last_seq = host.world.cutscene_narration_seq;
    ticks = 0;
    while host.world.active_scene_label == "opurud" && ticks < 30_000 {
        let _ = host.tick();
        ticks += 1;
        let seq = host.world.cutscene_narration_seq;
        if seq != last_seq {
            blocks += 1;
            last_seq = seq;
        }
    }
    assert_eq!(
        host.world.active_scene_label, "map01",
        "opurud chains to the world-map fly-in (ticked {ticks})"
    );
    assert_eq!(blocks, 3, "opurud played its three Mist crawls");

    // --- map01: the fly-in record (P2[38], walk-on trigger at the arrival
    //     tile) plays its Mist title card + crawl, then scene-changes into
    //     Rim Elm at the town01 opening trigger tile (0x1D,0x5B). ---
    assert!(
        host.world.cutscene_timeline_active(),
        "map01's opening record installs off the arrival tile trigger"
    );
    let mut fly_blocks = 0usize;
    last_seq = host.world.cutscene_narration_seq;
    ticks = 0;
    while host.world.active_scene_label == "map01" && ticks < 30_000 {
        let _ = host.tick();
        ticks += 1;
        let seq = host.world.cutscene_narration_seq;
        if seq != last_seq {
            fly_blocks += 1;
            last_seq = seq;
        }
    }
    assert_eq!(
        host.world.active_scene_label,
        legaia_asset::new_game::OPENING_SCENE,
        "the fly-in chains into Rim Elm (ticked {ticks})"
    );
    assert!(
        fly_blocks >= 1,
        "the fly-in played its Mist narration (saw {fly_blocks} blocks)"
    );

    // --- town01: arriving through the natural chain ends the opening chain
    //     and installs the opening timeline; name entry opens at op-0x49. ---
    assert!(
        !host.world.opening_chain_active,
        "the chain ends at Rim Elm"
    );
    assert!(
        host.world.cutscene_timeline_active(),
        "town01's opening timeline installs off the natural arrival"
    );
    let mut sweep = 0u32;
    while !host.world.name_entry_active() && sweep < 8000 {
        let _ = host.tick();
        sweep += 1;
    }
    assert!(
        host.world.name_entry_active(),
        "the town01 opening opens name entry (swept {sweep})"
    );
    eprintln!("[opening] natural zero-input chain reached town01 name entry");
}

#[test]
fn confirm_skips_the_opening_to_town01_name_entry() {
    let Some(mut host) = skip_or_host() else {
        return;
    };
    let opdeene = legaia_asset::new_game::OPENING_CUTSCENE_SCENE;
    let town01 = legaia_asset::new_game::OPENING_SCENE;

    host.world.begin_new_game();
    host.enter_field_scene(opdeene, 0).expect("enter opdeene");

    // Let the timeline arm the skip bit (GFLAG_SET 26 near the record top).
    let mut armed = false;
    for _ in 0..600 {
        let _ = host.tick();
        if host.world.story_flags & legaia_engine_core::world::PROLOGUE_HANDOFF_FLAG != 0 {
            armed = true;
            break;
        }
    }
    assert!(armed, "opdeene arms the intro-skip bit by execution");

    // The skip fires mid-opening (mid-narration included).
    let target = host.world.take_prologue_handoff(true);
    assert_eq!(target, Some(town01), "confirm skips the opening to Rim Elm");
    assert!(host.world.entering_town01_opening);

    // --- town01: opening timeline installs, name entry opens at op-0x49 ---
    host.enter_field_scene(town01, 0).expect("enter town01");
    assert!(
        host.world.cutscene_timeline_active(),
        "town01's opening timeline installs off the hand-off flag"
    );
    assert!(!host.world.entering_town01_opening);
    assert!(!host.world.opening_chain_active);
    assert!(!host.world.cutscene_narration_active());

    let mut sweep = 0u32;
    while !host.world.name_entry_active() && sweep < 4000 {
        host.world.tick();
        sweep += 1;
    }
    assert!(
        host.world.name_entry_active(),
        "the town01 opening timeline opens name entry (swept {sweep})"
    );

    // Commit a name -> the timeline resumes, completes, and un-parks the
    // townsfolk the establishing shot hid.
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
    assert!(!host.world.party_name(0).is_empty());

    let hide = legaia_engine_core::world::FIELD_OFFMAP_HIDE_XZ;
    let parked_at_hide = |w: &legaia_engine_core::world::World| {
        w.field_npc_positions
            .values()
            .any(|&(x, z)| x == hide && z == hide)
    };
    let mut more = 0u32;
    let mut saw_hidden = false;
    while host.world.cutscene_timeline.is_some() && more < 4000 {
        host.world.tick();
        saw_hidden |= parked_at_hide(&host.world);
        more += 1;
    }
    assert!(
        host.world.cutscene_timeline.is_none(),
        "the opening timeline completes after naming (ticked {more})"
    );
    assert!(saw_hidden, "the establishing shot parks townsfolk off-map");
    // STORY-parked villagers legitimately stay at the hide box - the retail
    // town01 field-actor list keeps a parked cohort in free roam (the
    // spawn-prologue `MoveTo (0x7F,0x7F)` despawns; see
    // `field_npc_entry_positions_disc`). What the teardown must restore is
    // every villager the CUTSCENE hid: any slot still at the hide box must
    // be one the scene-entry pre-run itself parked.
    let leftover: Vec<u8> = host
        .world
        .field_npc_positions
        .iter()
        .filter(|&(_, &(x, z))| x == hide && z == hide)
        .map(|(&slot, _)| slot)
        .collect();
    assert!(
        leftover
            .iter()
            .all(|slot| host.world.field_npc_entry_positions.get(slot) == Some(&(hide, hide))),
        "free-roam restores every cutscene-hidden villager (story-parked slots stay): {leftover:?}"
    );
    eprintln!("[opening] skip path reached town01 name entry + free-roam");
}
