//! The ending theme (BGM id `0x814`) plays its own score over its own bank.
//!
//! Retail's field initialiser `FUN_801D6704` special-cases `0x814`: it
//! stages the score from raw TOC `0x428` (extraction `1062`, one SEQ chunk)
//! and the instruments from raw `0x422` (extraction `1056`, a VAB-only
//! bank) on sound slot 10, and never plays the id from a single bank entry.
//! The ending save states hold exactly that pair (slot 10's sequence buffer
//! is `1062`'s SEQ chunk byte for byte). This pins the engine's side:
//! `SceneHost::music_bank_entry_bytes(0x814)` - the path both play hosts
//! stage a global track through - is that pair, the owned-VAB split finds
//! both halves, and every program the score selects is one the bank
//! defines. Counts only; no disc bytes are asserted.
//!
//! Disc-gated: skip-passes when `LEGAIA_DISC_BIN` is unset or `extracted/`
//! is absent.

use std::path::PathBuf;

use legaia_engine_core::mode_entry_init::FIELD_BGM_TWO_PART_ID;
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

#[test]
fn the_credits_theme_is_score_1062_over_bank_1056() {
    let Some(extracted) = gate() else { return };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let stream = host
        .music_bank_entry_bytes(FIELD_BGM_TWO_PART_ID as u16)
        .expect("read")
        .expect("the ending theme resolves");
    let split = legaia_engine_core::chunk_install::owned_bank_offsets(&stream)
        .expect("the composed stream splits into a bank and a score");
    let report = legaia_vab::parse(&stream, split.vab).expect("the bank parses");
    let seq = legaia_seq::Seq::parse(&stream[split.seq..]).expect("the score parses");

    // The bank is extraction 1056's (53 programs); the score is 1062's.
    assert_eq!(report.header.ps, 53, "extraction 1056's program count");
    let seq_entry = host.index.entry_bytes(1062).expect("entry 1062");
    let seq_len = (u32::from_le_bytes(seq_entry[..4].try_into().unwrap()) & 0xFF_FFFF) as usize;
    assert_eq!(
        &stream[split.seq..split.seq + seq_len],
        &seq_entry[4..4 + seq_len],
        "the score is entry 1062's SEQ chunk"
    );

    // Every program the score selects is defined in the bank.
    let defined: Vec<bool> = report.programs.iter().map(|p| p.tones > 0).collect();
    let mut used = std::collections::BTreeSet::new();
    for ev in &seq.events {
        if let legaia_seq::EventBody::Channel {
            message: legaia_seq::ChannelMessage::ProgramChange { program },
            ..
        } = ev.body
        {
            used.insert(program);
        }
    }
    assert!(seq.termination.is_clean(), "the whole score parses");
    assert!(!used.is_empty(), "the score changes program");
    for p in &used {
        assert!(
            defined.get(*p as usize).copied().unwrap_or(false),
            "program {p} is in the bank"
        );
    }
    eprintln!(
        "[ok] 0x814: {} programs selected, all defined in the {}-program bank",
        used.len(),
        report.header.ps
    );
}
