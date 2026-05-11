//! Disc-gated regression test for [`scene_tmd_stream::battle_tim_chunks`]
//! against the canonical `town01` corpus.
//!
//! `0006_town01.BIN` carries the textbook two-list shape: two type-0x01
//! TIM upload chunks inside the `FUN_8001FE70`-walked streaming tail
//! (offsets 0x3840, 0xba64), then a zero-padded gap and two more chunks
//! past the first terminator (0x16c24, 0x1ee48). The continuation pass
//! must surface both halves.
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset or when the extracted
//! PROT entries aren't on disk.

use std::path::PathBuf;

use legaia_asset::scene_tmd_stream::{self, WalkSource};

fn extracted_prot_dir() -> Option<PathBuf> {
    let cands = [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ];
    cands.into_iter().find(|p| p.is_dir())
}

#[test]
fn town01_slot3_two_list_battle_tim_chunks() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot_dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let path = prot_dir.join("0006_town01.BIN");
    if !path.exists() {
        eprintln!("[skip] {} missing", path.display());
        return;
    }
    let raw = std::fs::read(&path).expect("read 0006_town01");

    // Sanity: shape must detect.
    let stream = scene_tmd_stream::detect(&raw).expect("scene_tmd_stream");
    // chunk0 size of the leading TMD body, well-known on retail.
    assert_eq!(stream.tmd_size, 0x383c);

    let chunks = scene_tmd_stream::battle_tim_chunks(&raw);
    assert_eq!(
        chunks.len(),
        4,
        "0006_town01 should carry 4 type-0x01 TIM upload chunks (got {:?})",
        chunks
            .iter()
            .map(|c| (c.header_offset, c.source))
            .collect::<Vec<_>>()
    );

    // Tail and continuation each contribute two; specific offsets are
    // pinned by retail layout and won't shift under disc data.
    let tail: Vec<_> = chunks
        .iter()
        .filter(|c| c.source == WalkSource::Tail)
        .map(|c| c.header_offset)
        .collect();
    let cont: Vec<_> = chunks
        .iter()
        .filter(|c| c.source == WalkSource::Continuation)
        .map(|c| c.header_offset)
        .collect();
    assert_eq!(tail, vec![0x3840, 0xba64], "tail chunks");
    assert_eq!(cont, vec![0x16c24, 0x1ee48], "continuation chunks");

    // Each TIM payload must start with the PSX TIM magic 0x10.
    for c in &chunks {
        let payload = &raw[c.payload_offset..c.payload_offset + 4];
        let magic = u32::from_le_bytes(payload.try_into().unwrap());
        assert_eq!(
            magic, 0x0000_0010,
            "type-0x01 chunk payload must be a PSX TIM"
        );
    }
}

#[test]
fn town01_slot6_single_list_only() {
    // `0009_town01.BIN` is the "slot 6" variant that carries ONLY a
    // single streaming list inside the FUN_8001FE70-walked tail (no
    // continuation past the terminator). Confirms the walker doesn't
    // hallucinate continuation chunks when none exist.
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot_dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let path = prot_dir.join("0009_town01.BIN");
    if !path.exists() {
        eprintln!("[skip] {} missing", path.display());
        return;
    }
    let raw = std::fs::read(&path).expect("read 0009_town01");

    let chunks = scene_tmd_stream::battle_tim_chunks(&raw);
    assert_eq!(chunks.len(), 2, "slot 6 has only the tail list");
    for c in &chunks {
        assert_eq!(c.source, WalkSource::Tail);
    }
}
