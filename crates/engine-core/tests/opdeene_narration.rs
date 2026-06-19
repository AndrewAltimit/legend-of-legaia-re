//! Disc-gated: pin the opening prologue's inline narration to real disc bytes.
//!
//! The cutscene scene `opdeene` carries its on-screen narration ("Genesis"
//! prologue) as inline ASCII text pages embedded directly in the
//! cutscene-timeline field-VM script - the same partition-2 record that
//! raises the `town01` hand-off `GFLAG_SET 26`. Each page is framed
//! `0x1F <ascii> 0x00` and introduced by a `0x4C` narration op whose operand
//! declares the page count (see `legaia_asset::cutscene_text`).
//!
//! This asserts the parse's *structural* invariants - two blocks of 14 and 8
//! pages, every page non-empty 7-bit ASCII, page counts matching the op - so
//! it ground-truths the format against the disc without baking any of the
//! (Sony-owned) narration text into the source. Skip-passes without disc
//! data / extracted assets (CLAUDE.md convention).

use legaia_asset::cutscene_text::parse_narration;
use legaia_engine_core::man_field_scripts::partition_record_span;
use legaia_engine_core::scene::{ProtIndex, Scene};
use std::path::PathBuf;

/// Partition / record where the `opdeene` cutscene timeline lives.
const TIMELINE_PARTITION: usize = 2;
const TIMELINE_RECORD: usize = 18;

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
fn opdeene_timeline_carries_two_inline_narration_blocks() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let cutscene = legaia_asset::new_game::OPENING_CUTSCENE_SCENE;
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let scene = Scene::load(&index, cutscene).expect("load opdeene");
    let man = scene
        .field_man_payload(&index)
        .expect("man payload fetch")
        .expect("opdeene has a MAN payload");
    let man_file = legaia_asset::man_section::parse(&man).expect("man parse");

    let (script_start, pc0, body_len) =
        partition_record_span(&man_file, &man, TIMELINE_PARTITION, TIMELINE_RECORD)
            .expect("opdeene cutscene-timeline record span");
    let body = &man[script_start..script_start + body_len];

    // The partition-2 named-record header decodes to entry PC 0x10 (name "6
    // chars" + three empty condition-blocks), where the timeline opens with
    // op 0x34 EFFECT, immediately followed by GFLAG_SET 26 at +0x17. This
    // ground-truths the named-record header walk against the disc.
    assert_eq!(pc0, 0x10, "partition-2 named-record entry PC");
    assert_eq!(body[pc0], 0x34, "timeline opens with op 0x34 (EFFECT)");
    assert_eq!(
        (body[0x17], body[0x18]),
        (0x2E, 0x1A),
        "GFLAG_SET 26 follows the opening EFFECT op"
    );

    let blocks = parse_narration(body);
    for (i, b) in blocks.iter().enumerate() {
        eprintln!(
            "block {i} @ 0x{:05X}: declared {} pages, decoded {}",
            script_start + b.op_offset,
            b.declared_pages,
            b.pages.len()
        );
    }

    // Two narration blocks: a 14-page creation prologue and an 8-page
    // Seru-history block.
    assert_eq!(
        blocks.len(),
        2,
        "opdeene carries exactly two narration blocks"
    );
    assert_eq!(blocks[0].declared_pages, 14, "block 0 declares 14 pages");
    assert_eq!(blocks[1].declared_pages, 8, "block 1 declares 8 pages");

    let total: usize = blocks.iter().map(|b| b.pages.len()).sum();
    assert_eq!(total, 22, "22 narration pages total");

    for b in &blocks {
        // The decoded page count matches the op's declaration - the
        // parse-validation invariant.
        assert!(b.count_matches(), "decoded page count matches declared");
        for page in &b.pages {
            assert!(!page.text.is_empty(), "no empty narration page");
            assert!(
                page.text.bytes().all(|c| (0x20..0x7F).contains(&c)),
                "narration pages are 7-bit printable ASCII"
            );
        }
    }

    // Blocks appear in timeline order (the creation prologue precedes the
    // Seru-history block).
    assert!(
        blocks[0].op_offset < blocks[1].op_offset,
        "narration blocks are ordered by timeline position"
    );
}
