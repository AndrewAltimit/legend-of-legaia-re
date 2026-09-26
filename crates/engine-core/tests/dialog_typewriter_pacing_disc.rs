//! The field dialog typewriter runs at the retail pager's pace.
//!
//! Retail reference: a PCSX-Redux trace of `town01` placement `P1[16]`'s
//! conversation (`scripts/pcsx-redux/autorun_dialog_typewriter_trace.lua`
//! from `s4_rimelm_door_transition`, lead name four letters long), one row
//! per vsync of the pager's reveal counter `_DAT_801F2748` and short-row hold
//! `_DAT_801F275C` at `DAT_1F800393 = 2`. Only counts and timings are pinned
//! here - no disc text:
//!
//! - the seven rows the conversation types (record offsets `+0x4C` ..) finish
//!   on glyph counts `25, 30, 32, 24, 11, 16, 31` (each read back as the
//!   finish call's hold, `(0x22 - count) * 4`, and the counter value it
//!   passed);
//! - the first box's counter/hold sequence, per vsync, is
//!   `1,1,3,3,..,25,25`, then the finish `(0, 36)` for two vsyncs, one held
//!   call `(0, 0)`, then `2,2,4,4,..,30,30` and the finish `(0, 16)` on which
//!   the page ends.
//!
//! The engine side counts the same rows off the disc with
//! `legaia_font::typewriter_glyph_count` and types the same conversation
//! through the path both hosts share (`World::step_inline_dialogue`), with a
//! four-letter test name in the party roster.
//!
//! Disc-gated: skip-passes when `LEGAIA_DISC_BIN` is unset or `extracted/`
//! is absent.

use std::path::PathBuf;

use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    ["extracted", "../extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
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

const SLOT: u8 = 16;
/// Four letters, as the captured save's lead name. Not disc text.
const NAME: &str = "Zedd";

/// `(record offset of the row's 0x1F lead, retail glyph count)`, captured.
const RETAIL_ROWS: [(usize, u32); 7] = [
    (0x4C, 25),
    (0x63, 30),
    (0x83, 32),
    (0xA4, 24),
    (0xBE, 11),
    (0xCB, 16),
    (0xD9, 31),
];

/// The captured first box, per vsync from the opening call.
fn retail_first_box() -> Vec<(i32, i32)> {
    let mut v = Vec::new();
    let mut pair = |c: i32, h: i32| {
        v.push((c, h));
        v.push((c, h));
    };
    for c in (1..=25).step_by(2) {
        pair(c, 0);
    }
    pair(0, 36);
    pair(0, 0);
    for c in (2..=30).step_by(2) {
        pair(c, 0);
    }
    v.push((0, 16));
    v
}

#[test]
fn town01_npc16_rows_count_as_retail() {
    let Some(extracted) = gate() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("town01", 0).expect("enter town01");
    let prologue = host
        .world
        .npcs
        .dialog_prologue
        .get(&SLOT)
        .cloned()
        .expect("town01 P1[16] is a talkable placement");
    assert_eq!(prologue.first_segment, RETAIL_ROWS[0].0);
    let expand = |op: u8, arg: u8| -> Option<Vec<u8>> {
        (op == 0xC1 && arg == 0).then(|| NAME.as_bytes().to_vec())
    };
    for (lead, want) in RETAIL_ROWS {
        assert_eq!(prologue.body[lead], 0x1F, "row lead at +{lead:#x}");
        let got = legaia_font::typewriter_glyph_count(&prologue.body[lead..], Some(&expand));
        assert!(got.unresolved.is_empty(), "row +{lead:#x} fully resolved");
        assert_eq!(got.count, want, "row +{lead:#x} glyph count");
    }
    eprintln!("[ok] town01 P1[16]: seven row counts match the retail pager");
}

#[test]
fn town01_npc16_first_box_types_at_the_retail_pace() {
    let Some(extracted) = gate() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.world.toggles.use_vm_dialogue = true;
    host.enter_field_scene("town01", 0).expect("enter town01");
    host.world.party.party_names = vec![NAME.to_string()];
    assert_eq!(
        host.world.clock.frame_step, 2,
        "town01 runs at the captured frame step"
    );

    host.world.trigger_field_interact(0, SLOT);
    let mut seq: Vec<(i32, i32)> = Vec::new();
    let mut leads = Vec::new();
    for _ in 0..400 {
        host.world.set_pad(0);
        host.tick().expect("tick");
        let Some(panel) = host
            .world
            .dialog
            .inline
            .as_ref()
            .and_then(|r| r.panel.as_ref())
        else {
            continue;
        };
        let p = panel.pacer();
        if seq.is_empty() && (p.counter, p.hold) == (0, 0) {
            // The box opened this tick; its opening call runs on the next.
            continue;
        }
        leads = panel.row_leads();
        seq.push((p.counter, p.hold));
        if panel.is_waiting_for_input() {
            break;
        }
    }
    assert_eq!(leads, vec![RETAIL_ROWS[0].0, RETAIL_ROWS[1].0]);
    assert_eq!(seq, retail_first_box(), "per-vsync (counter, hold)");
    eprintln!(
        "[ok] town01 P1[16] first box: {} vsyncs, counter/hold sequence matches retail",
        seq.len()
    );
}
