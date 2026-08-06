//! The narration ladder: the two text presenters a pad-driven run reaches but
//! no reach-report ladder drove.
//!
//! Both rungs are *ladders*, not oracles - each drives a real scene through
//! `SceneHost` with a pad word per tick and reads the presenter's **output**
//! surface every frame, exactly as the two hosts do
//! (`web_viewer::play_cutscene` and the native window's HUD both call
//! `CutsceneNarration::visible_lines` / the inline panel's page bytes). The
//! deeper behaviour of each presenter already has its own disc oracle
//! (`opdeene_narration_playback`, `inline_dialogue_vm_no_panic_disc`); what was
//! missing is a bounded, pad-shaped run of them that the runtime reach report's
//! coverage union can afford to carry.
//!
//! Reading the output is the point. A presenter that installs, ticks and emits
//! nothing is indistinguishable at the call site from one that is not wired -
//! so each rung asserts glyph-bearing lines actually come out, and that they
//! *move*.
//!
//! - Rung 1: the opening-prologue subtitle roller (`FUN_80037174`) in
//!   `opdeene`, ticked until its lines crawl, then dismissed by a pad press.
//! - Rung 2: the inline field-VM conversation (`FUN_8003CF7C`) - the faithful
//!   dialogue path, which runs the record's own bytecode between text boxes,
//!   rather than the pre-decoded dialog panel the other ladders drive.
//!
//! Disc-gated: skip-passes when `LEGAIA_DISC_BIN` is unset or `extracted/` is
//! absent, per the repo convention.

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::{SceneHost, is_world_map_scene};
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

fn gate() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let d = extracted_dir();
    if d.is_none() {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    }
    d
}

/// Rung 1: the opening prologue's narration roller crawls and is dismissable.
///
/// The retail presenter is a bottom-up text crawl owned by one on-screen-text
/// actor (`FUN_80037174`), not a one-line caption: several pages are visible at
/// once and each climbs a pixel every `frames_per_pixel` frames. The ladder
/// reads `visible_lines()` per frame - the host's own read surface - so a
/// roller that installed and then emitted nothing would fail here rather than
/// pass silently.
#[test]
fn the_opening_prologue_roller_crawls_under_a_pad_driven_tick() {
    let Some(extracted) = gate() else { return };

    let scene = legaia_asset::new_game::OPENING_CUTSCENE_SCENE;
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene(scene, 0)
        .expect("enter the prologue");
    assert!(
        host.world.cutscene_timeline_active(),
        "entering {scene} installs the cutscene timeline"
    );

    // Run the timeline to its first narration block with nothing held. The
    // block is script-driven, so this is the beat the player waits through.
    let mut ticks = 0u32;
    while !host.world.cutscene_narration_active() && ticks < 2_000 {
        host.world.set_pad(0);
        let _ = host.world.tick();
        ticks += 1;
    }
    assert!(
        host.world.cutscene_narration_active(),
        "the timeline reached a narration block within {ticks} ticks"
    );
    let pages = host
        .world
        .cutscene_narration
        .as_ref()
        .map(|n| n.page_count())
        .unwrap_or(0);
    assert!(
        pages > 1,
        "the block carries a multi-page crawl (got {pages})"
    );

    // Crawl. Sample the host's read surface every frame: how many lines are on
    // screen, and where the leading line sits.
    let mut max_lines = 0usize;
    let mut glyphs = 0usize;
    let mut first_y: Option<i32> = None;
    let mut last_y: Option<i32> = None;
    let mut admitted = 0usize;
    for _ in 0..1_200 {
        host.world.set_pad(0);
        let _ = host.tick();
        let Some(n) = host.world.cutscene_narration.as_ref() else {
            break;
        };
        admitted = admitted.max(n.current_index() + 1);
        let lines = n.visible_lines();
        max_lines = max_lines.max(lines.len());
        if let Some(top) = lines.first() {
            glyphs += top.text.chars().filter(|c| !c.is_whitespace()).count();
            first_y.get_or_insert(top.y);
            last_y = Some(top.y);
        }
    }

    assert!(
        max_lines > 1,
        "the roller is a crawl - several pages are on screen at once (saw {max_lines})"
    );
    assert!(glyphs > 0, "the visible lines carry no glyphs at all");
    let (a, b) = (
        first_y.expect("a line was on screen"),
        last_y.expect("a line was on screen"),
    );
    assert!(b < a, "the crawl must climb: leading line went {a} -> {b}");
    assert!(
        admitted > 1,
        "more than the first page was admitted (got {admitted})"
    );
    eprintln!(
        "[roller] {scene}: {pages} pages, {admitted} admitted, up to {max_lines} lines on \
         screen, leading line climbed {a} -> {b}"
    );

    // The pad press retail arms near the record top: a confirm mid-narration
    // skips the whole opening.
    let target = host.world.take_prologue_handoff(true);
    assert_eq!(
        target,
        Some(legaia_asset::new_game::OPENING_SCENE),
        "a confirm mid-narration hands off to Rim Elm"
    );
    assert!(
        !host.world.cutscene_narration_active(),
        "the skip tears the roller down"
    );
}

/// Rung 2: an NPC conversation runs on the **inline field-VM** path.
///
/// `World::step_inline_dialogue` executes the interaction record's own bytecode
/// between text boxes (prologue flag tests, `SET`/`CLEAR`, scene changes) and
/// fast-forwards to the next `0x1F` text segment - retail's run-to-next-text
/// helper `FUN_8003CF7C`. The other ladders drive the pre-decoded dialog panel
/// instead, which never enters it.
///
/// Bounded on purpose: a handful of scenes, every NPC in each, pad-driven.
#[test]
fn npc_conversations_run_through_the_inline_field_vm() {
    let Some(extracted) = gate() else { return };

    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut scenes: Vec<String> = cdname.values().cloned().collect();
    scenes.sort();
    scenes.dedup();

    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    // The faithful path both browser and native hosts arm.
    host.world.use_vm_dialogue = true;

    const SCENE_BUDGET: usize = 4;
    let mut scenes_driven = 0usize;
    let mut conversations = 0usize;
    let mut boxes_with_text = 0usize;
    let mut vm_steps = 0usize;

    for scene in &scenes {
        if scenes_driven >= SCENE_BUDGET {
            break;
        }
        if is_world_map_scene(scene) {
            continue;
        }
        host.world.use_vm_dialogue = true;
        if host.enter_field_scene(scene, 0).is_err() {
            continue;
        }
        let mut slots: Vec<u8> = host
            .world
            .field_npc_dialog_prologue
            .keys()
            .copied()
            .chain(host.world.field_npc_dialog.keys().copied())
            .collect();
        slots.sort_unstable();
        slots.dedup();
        if slots.is_empty() {
            continue;
        }
        scenes_driven += 1;

        for slot in slots {
            host.world.trigger_field_interact(0, slot);
            let mut ran_inline = false;
            for i in 0..240u32 {
                // Pad-driven: page the box every few frames, nudge a picker.
                let mut mask = 0u16;
                if i % 6 == 5 {
                    mask |= PadButton::Cross.mask();
                }
                if i % 17 == 9 {
                    mask |= PadButton::Down.mask();
                }
                host.world.set_pad(mask);
                host.tick()
                    .unwrap_or_else(|e| panic!("{scene} slot {slot}: tick failed: {e:#}"));
                if let Some(id) = host.world.inline_dialogue.as_ref() {
                    ran_inline = true;
                    vm_steps += 1;
                    // The host's per-frame read surface.
                    let page = id.page_bytes();
                    if page.iter().any(|&b| b.is_ascii_graphic()) {
                        boxes_with_text += 1;
                    }
                    if id.is_done() {
                        break;
                    }
                } else if i > 2 {
                    break;
                }
            }
            if ran_inline {
                conversations += 1;
            }
            host.world.inline_dialogue = None;
            host.world.current_dialog = None;
        }
    }

    assert!(
        scenes_driven > 0,
        "no field scene with NPC interaction records was reachable"
    );
    assert!(
        conversations > 0,
        "no NPC conversation reached the inline field-VM runner"
    );
    assert!(
        boxes_with_text > 0,
        "the inline runner stepped {vm_steps} times and never produced a glyph-bearing page \
         - a runner that emits nothing is indistinguishable from one that is not wired"
    );
    eprintln!(
        "[inline-vm] {conversations} conversations across {scenes_driven} scenes, \
         {vm_steps} runner steps, {boxes_with_text} frames with a text page"
    );
}
