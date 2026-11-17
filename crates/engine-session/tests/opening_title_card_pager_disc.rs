//! Disc-gated: a title card's page holds its record as retail's pager does.
//!
//! `CC F8 89 lo hi` is op `4C 89` on the cutscene context - the pager's
//! automatic press `_DAT_80073F00` - and the `0x1F` lines after it are an
//! ordinary text segment, so the runner `FUN_80039B7C` parks the record on
//! the box while it opens, types the lines and presses itself. A per-vsync
//! retail capture of the zero-input New Game opening
//! (`autorun_opdeene_pacing.lua` and `autorun_audio_timeline.lua` with the
//! pager state `0x801F2734`, off `s1_newgame_field`) holds `map01`'s record
//! (`P2[38]`) on the "twilight of humanity" card at `+0x72` for 100 vsyncs,
//! pager states `2` -> `11` -> `0x19` -> `4`, and on the blank card at
//! `+0xF2` for 38 (`5` -> `12` -> `11` -> `15` -> `0x19`: the line loads into
//! the rows the first card's `0x24` continuation left and scrolls them out).
//!
//! The run starts a New Game and plays the opening chain with no input.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};

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
fn map01s_title_cards_hold_the_fly_in_record_as_retails_pager_does() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("open boot session");
    assert!(s.start_new_game(&FieldLiveOpts::default()).is_some());
    // Ticks the map01 record spends at each PC.
    let mut held = std::collections::BTreeMap::<usize, u32>::new();
    for _ in 0..12_000 {
        s.tick().expect("tick");
        let w = &s.host.world;
        if w.active_scene_label == "town01" {
            break;
        }
        if w.active_scene_label == "map01"
            && let Some(tl) = w.cutscene.timeline.as_ref()
        {
            *held.entry(tl.pc).or_default() += 1;
        }
    }
    let card = held.get(&0x72).copied().unwrap_or(0);
    let blank = held.get(&0xF2).copied().unwrap_or(0);
    assert!(
        (92..=108).contains(&card),
        "the card's page holds the record for retail's 100 vsyncs: {card}"
    );
    assert!(
        (32..=44).contains(&blank),
        "the blank card scrolls the held rows out for retail's 38: {blank}"
    );
}
