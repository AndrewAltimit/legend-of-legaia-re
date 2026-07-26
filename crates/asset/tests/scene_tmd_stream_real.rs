//! Disc-gated regression test for [`scene_tmd_stream::battle_tim_chunks`]
//! against the canonical `town01` corpus.
//!
//! A `scene_tmd_stream` PROT entry holds exactly **one** complete
//! `[chunk0 TMD][type-0x01 TIM chunks][terminator]` stream. `0006_town01.BIN`
//! is the canonical example: TMD body `0x383c`, then two type-0x01 TIM upload
//! chunks at `0x3840` / `0xba64` inside the `FUN_8001FE70`-walked tail, then
//! the terminator and sector padding out to the entry's `0x14000` end.
//!
//! These tests previously expected `0006_town01` to carry *four* TIM chunks
//! and *two* concatenated sub-streams. That reading was an artifact of the
//! superseded PROT entry-size expression, which over-read each entry into its
//! successor's head bytes (see `docs/formats/prot.md`; entry size is the
//! sector gap to the next entry, `toc[p+3] - toc[p+2]`). The "second
//! sub-stream at `0x14000`" was simply PROT entry **0007**, whose TOC start
//! (`0x0011F000`) is exactly `0x14000` past entry 0006's (`0x0010B000`).
//! [`town01_slot4_owns_the_former_continuation_chunks`] pins that ownership
//! arithmetic so the correction cannot silently regress.
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset or when the extracted
//! PROT entries aren't on disk.

use std::path::PathBuf;

use legaia_asset::scene_tmd_stream::{self, WalkSource};

/// Byte length of PROT entry `0006_town01` = its TOC sector gap
/// (`0x0011F000 - 0x0010B000`). Also the offset the pre-correction over-read
/// mislabelled as "sub-stream 1" - it is where entry 0007 begins.
const ENTRY_0006_LEN: usize = 0x14000;

fn extracted_prot_dir() -> Option<PathBuf> {
    let cands = [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ];
    cands.into_iter().find(|p| p.is_dir())
}

/// Read an extracted PROT entry, or `None` to skip the test.
fn read_entry(name: &str) -> Option<Vec<u8>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let prot_dir = match extracted_prot_dir() {
        Some(d) => d,
        None => {
            eprintln!("[skip] extracted/PROT missing");
            return None;
        }
    };
    let path = prot_dir.join(name);
    if !path.exists() {
        eprintln!("[skip] {} missing", path.display());
        return None;
    }
    Some(std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display())))
}

/// Assert every reported chunk's payload opens with the PSX TIM magic.
fn assert_payloads_are_tims(raw: &[u8], chunks: &[scene_tmd_stream::BattleTimChunk]) {
    for c in chunks {
        let payload = &raw[c.payload_offset..c.payload_offset + 4];
        let magic = u32::from_le_bytes(payload.try_into().unwrap());
        assert_eq!(
            magic, 0x0000_0010,
            "type-0x01 chunk payload at {:#x} must be a PSX TIM",
            c.payload_offset
        );
    }
}

#[test]
fn town01_slot3_carries_two_entry_local_tim_chunks() {
    let Some(raw) = read_entry("0006_town01.BIN") else {
        return;
    };

    // Premise guard: the entry's own extent. If the extractor ever regresses
    // to the over-reading size expression this fires before the chunk counts
    // do, naming the real cause instead of looking like a parser bug.
    assert_eq!(
        raw.len(),
        ENTRY_0006_LEN,
        "0006_town01 must be exactly its TOC sector gap - a longer file means \
         the extractor over-read into entry 0007"
    );

    // Sanity: shape must detect.
    let stream = scene_tmd_stream::detect(&raw).expect("scene_tmd_stream");
    // chunk0 size of the leading TMD body, well-known on retail.
    assert_eq!(stream.tmd_size, 0x383c);

    let chunks = scene_tmd_stream::battle_tim_chunks(&raw);
    assert_eq!(
        chunks.len(),
        2,
        "0006_town01 carries 2 type-0x01 TIM upload chunks, both entry-local (got {:?})",
        chunks
            .iter()
            .map(|c| (c.header_offset, c.source))
            .collect::<Vec<_>>()
    );

    // Both live in the FUN_8001FE70-walked tail; nothing follows the
    // terminator but sector padding, so no chunk is a Continuation.
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
    assert_eq!(
        cont,
        Vec::<usize>::new(),
        "no chunk survives past the terminator - the bytes the old reading \
         called a continuation list belong to PROT entry 0007"
    );

    assert_payloads_are_tims(&raw, &chunks);
}

#[test]
fn town01_slot4_owns_the_former_continuation_chunks() {
    // The two chunks the pre-correction test expected at 0x16c24 / 0x1ee48
    // inside 0006 are PROT entry 0007's own tail chunks. Entry 0007's TOC
    // start is exactly ENTRY_0006_LEN past entry 0006's, so subtracting that
    // constant maps the stale coordinates onto 0007-local ones exactly.
    //
    // Same for the TMD: the "sub-stream 1 leading TMD, body 0x2c20" is just
    // entry 0007's own chunk0. This test is what makes the count change in
    // `town01_slot3_carries_two_entry_local_tim_chunks` an ownership
    // correction rather than a loss of coverage - the bytes are still
    // asserted, under their true owner.
    const STALE_CONT_OFFSETS: [usize; 2] = [0x16c24, 0x1ee48];

    let Some(raw) = read_entry("0007_town01.BIN") else {
        return;
    };

    let stream = scene_tmd_stream::detect(&raw).expect("0007_town01 is a scene_tmd_stream");
    assert_eq!(
        stream.tmd_size, 0x2c20,
        "entry 0007's own leading TMD - the body the old reading attributed \
         to a second sub-stream inside 0006"
    );

    let chunks = scene_tmd_stream::battle_tim_chunks(&raw);
    let tail: Vec<_> = chunks
        .iter()
        .filter(|c| c.source == WalkSource::Tail)
        .map(|c| c.header_offset)
        .collect();
    assert_eq!(tail, vec![0x2c24, 0xae48], "entry 0007's own tail chunks");

    // The ownership arithmetic, stated explicitly.
    let rebased: Vec<usize> = STALE_CONT_OFFSETS
        .iter()
        .map(|o| o - ENTRY_0006_LEN)
        .collect();
    assert_eq!(
        rebased, tail,
        "the stale 0006-relative offsets are entry 0007's chunks shifted by \
         the length of entry 0006"
    );

    assert_eq!(chunks.len(), 2, "entry 0007 is itself a single-list entry");
    assert_payloads_are_tims(&raw, &chunks);
}

#[test]
fn town01_slot6_single_list_only() {
    // `0009_town01.BIN` was historically read as the "slot 6 variant" that
    // carries only a single streaming list, in contrast to 0006's apparent
    // two-list shape. With entry sizes corrected this is simply the shape
    // every scene_tmd_stream entry has; the test is kept as a second witness
    // on a different entry of the same cluster.
    let Some(raw) = read_entry("0009_town01.BIN") else {
        return;
    };

    let chunks = scene_tmd_stream::battle_tim_chunks(&raw);
    assert_eq!(chunks.len(), 2, "one list, two TIM chunks");
    for c in &chunks {
        assert_eq!(c.source, WalkSource::Tail);
    }
}

#[test]
fn town01_scene_tmd_stream_entries_hold_exactly_one_substream() {
    // A scene_tmd_stream PROT entry is one self-contained sub-stream. The
    // "two concatenated sub-streams" reading of 0006 was the over-read
    // spilling into 0007; both entries independently enumerate as a single
    // sub-stream based at 0.
    let Some(raw6) = read_entry("0006_town01.BIN") else {
        return;
    };
    let Some(raw7) = read_entry("0007_town01.BIN") else {
        return;
    };

    for (label, raw, expect_tmd) in [
        ("0006_town01", &raw6, 0x383c_usize),
        ("0007_town01", &raw7, 0x2c20_usize),
    ] {
        let subs = scene_tmd_stream::sub_streams(raw);
        assert_eq!(
            subs.len(),
            1,
            "{label} holds exactly one sub-stream (got bases {:?})",
            subs.iter().map(|s| s.base).collect::<Vec<_>>()
        );
        assert_eq!(subs[0].base, 0, "{label} sub-stream is based at 0");
        assert_eq!(subs[0].stream.tmd_size, expect_tmd, "{label} leading TMD");

        // The leading TMD parses as a real Legaia TMD.
        let tmd_abs = subs[0].base + 4;
        let magic = u32::from_le_bytes(raw[tmd_abs..tmd_abs + 4].try_into().unwrap());
        assert_eq!(magic, 0x8000_0002, "{label} opens with a Legaia TMD");
    }
}
