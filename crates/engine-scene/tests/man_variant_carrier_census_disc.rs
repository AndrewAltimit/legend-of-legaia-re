//! Disc-gated: the streaming **variant MAN carriers** and the story-spine
//! flag writers the carrier-complete SYSTEM-flag census surfaces.
//!
//! Thirteen retail blocks ship a second MAN as the type-3 chunk of a
//! standalone `data_field_streaming` entry - story-state variants with
//! different partition counts and different scripts than the bundle MAN
//! (for the v12-family dungeons `rikuroa` / `dolk2` the streaming carrier
//! is the scene's ONLY MAN; the live script heap at the Mt. Rikuroa Caruban
//! beat byte-matches PROT `0157`'s chunk). A bundle-only census is
//! structurally blind to every script op these carriers hold - which is
//! exactly where the chapter-1 story-spine writers turned out to live:
//!
//!   - `0x142` (Caruban beat / dolk->dolk2 switch): SET by the rikuroa
//!     carrier's post-victory record P2[50] (the C1 self-latch the firehose
//!     caught live, `51 42`, `ra 0x801E3598`), re-asserted by dolk2's
//!     carrier P1[0]. The same carriers' P1[10..12] / P1[1] / dolk P1[26]
//!     hold the op as developer flag-menu arms, not as the beat.
//!   - `0x482` (Drake mist walls): NO genuine script site. The earlier
//!     "SET by the other7 script pool, CLEARed by the edbalden/eddoman
//!     epilogue carriers" reading is falsified - every census site for
//!     0x482 is a desynced-walker alias of Shift-JIS text (full-width
//!     digits / an ＥＸＩＴ label table); see the per-site breakdown in
//!     `spine_flag_writers_surface_in_the_carrier_census`. The writer is a
//!     direct code path (capture target), like flag 549.
//!   - `0x1BE`: SET by `geremi` P2[0] (Jeremi's arrival one-shot "Meta's
//!     warning", C1 = itself) - surfaced by the partition-2 named-record
//!     header fix, NOT a rikuroa/Zeto flag.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing
//! (CLAUDE.md disc-gated convention).

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_disc_audit::categorize::Class;
use legaia_engine_field::man_field_scripts::flag_test_bytescan;
use legaia_engine_scene::man_census::{scene_man_carriers, system_flag_census};
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_scene_formats::field_disasm::FlagKind;

mod carrier_census;
mod chapter2_gates;
mod chapter2_puzzle_flags;
mod chapter3_gates;
mod flag_classification;
mod region_gate_order;
mod rim_elm_flag_549;
mod spine_flags;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_index() -> Option<ProtIndex> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir()?;
    Some(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"))
}
