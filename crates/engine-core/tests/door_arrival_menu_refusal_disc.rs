//! A Door of Light arrival on `map01` refuses the pause menu while the
//! cave-mouth record runs - pinned to a retail capture.
//!
//! `scripts/pcsx-redux/autorun_door_menu_refusal.lua` used a Door of Light in
//! cave01 (`cave01_attached_light`) and kept pressing the menu button on the
//! destination. The resolve seats the party on `map01` tile `(37, 109)`, the
//! cave mouth, and that tile carries a gate-1 walk-on trigger: the first field
//! frame spawns `P2[9]` (`FUN_801D1EC4` -> `FUN_8003BDE0`, ra `0x801D218C`).
//! The record waits 40 frames, walks the player out (`A2 F8 01`), applies a
//! camera and parks on the player's move-done flag (`AD F8 08`) for most of
//! its run; it reaches its closing `21` 294 vsyncs (frame step 2) after the
//! arrival.
//!
//! For that whole span the per-actor script runner `FUN_80039B7C` holds the
//! player's engaged bit `+0x10 & 0x80000` (raised at `0x80039DD4` every frame
//! it steps the context, cleared at `0x80039F14` only once the running count
//! drains on the `0x21`), so `FUN_801D1344` never calls the pad controller
//! `FUN_801D01B0` (`0x801D1694`). The capture reads zero controller entries
//! on every press inside the span - no deny buzz, no accept - and the first
//! press after it opens the menu (`ACCEPT` at `0x801D02E8`, then the
//! installer `FUN_801F1278`). The refusal is bounded: it lasts exactly as
//! long as the record.
//!
//! The engine's counterpart of "a spawned record is mid-script" is an active
//! cutscene timeline, and `World::field_menu_open_allowed` refuses on it.

use std::path::PathBuf;

use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// `P2[9]`'s first two ops (`52 FC` SysFlag.Set `0x2FC`, `65 27`
/// SysFlag.Clear `0x527`) at its `pc0 = 14`.
const P2_9_HEAD: [u8; 4] = [0x52, 0xFC, 0x65, 0x27];
const P2_9_PC0: usize = 14;

#[test]
fn a_door_of_light_arrival_on_map01_refuses_the_menu_until_its_record_ends() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    // What the Riremito resolve hands the scene host for cave01's return
    // triple `0x55 @ (37, 109)` (tests/door_item_retail_timeline.rs).
    host.world.pending_named_scene_transition = Some(("map01".to_string(), 37, 109, 0));
    host.tick().expect("arrival tick");

    let mut ran = 0usize;
    let mut refused = 0usize;
    let mut seen_record = false;
    for _ in 0..600 {
        host.world.set_pad(0);
        host.tick().expect("tick");
        if !host.world.cutscene_timeline_active() {
            break;
        }
        if let Some(tl) = host.world.cutscene.timeline.as_ref() {
            seen_record |= tl.bytecode.get(P2_9_PC0..P2_9_PC0 + 4) == Some(&P2_9_HEAD[..]);
        }
        ran += 1;
        if !host.world.field_menu_open_allowed() {
            refused += 1;
        }
    }
    assert!(seen_record, "the arrival tile spawned map01 P2[9]");
    assert!(ran > 0, "the cave-mouth record ran after the arrival");
    assert_eq!(
        refused, ran,
        "every tick the record runs refuses the menu, as retail's engaged bit does"
    );
    assert!(
        !host.world.cutscene_timeline_active(),
        "the record ends on its own"
    );
    assert!(
        host.world.field_menu_open_allowed(),
        "the first tick after the record opens the menu again - the refusal is bounded"
    );
    eprintln!("[ok] map01 P2[9] ran {ran} ticks with the menu refused, then released it");
}
