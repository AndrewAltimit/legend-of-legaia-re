//! Disc-gated: multi-segment dialog **box packing** decodes faithfully on real
//! town01 NPC inline pools.
//!
//! Closes the last open sub-question of the inline-dialog thread: how the
//! per-actor dialog SM `FUN_80039B7C` / window pager `FUN_801D84D0` group
//! consecutive `0x1F`-lead lines into a `_DAT_801F2740 = 3`-row box, and how the
//! `0xC0..=0xCF` 2-byte escapes the SM's advance loop skips keep a line from
//! terminating early. `legaia_mes::pack_box` / `pack_boxes` decode it; this test
//! pins the behaviour against the real disc bytes. Skips when `LEGAIA_DISC_BIN`
//! is unset.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::scene::{ProtIndex, Scene};

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
fn town01_dialog_boxes_pack_at_most_three_lines() {
    use legaia_asset::man_section::parse as parse_man;
    use legaia_engine_core::man_field_scripts::{PlacementKind, classify_placements};
    use legaia_mes::{LINES_PER_BOX, pack_box};

    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open prot index"));
    let scene = Scene::load(&index, "town01").expect("load town01");
    let man_bytes = scene
        .field_man_payload(&index)
        .expect("read MAN")
        .expect("town01 MAN");
    let man = parse_man(&man_bytes).expect("parse MAN");

    // Walk EVERY 0x1F lead in EVERY NPC pool and pack a box there - not just the
    // first - so the cap is exercised against the whole corpus of lines, not one
    // entry point.
    let mut boxes_checked = 0usize;
    let mut pools = 0usize;
    for (_p, kind) in classify_placements(&man, &man_bytes) {
        let PlacementKind::Npc {
            dialog_inline: Some(inline),
            ..
        } = kind
        else {
            continue;
        };
        pools += 1;
        for i in 0..inline.len() {
            if inline[i] != 0x1F {
                continue;
            }
            // Only treat it as a box start if the prior byte is a line
            // terminator / control byte (mirrors how the pager only enters a box
            // from a yielded `& 0x7F < 0x20` position) - otherwise this 0x1F is
            // a wide-glyph argument inside another line.
            if i > 0 && inline[i - 1] >= 0x20 {
                continue;
            }
            if let Some(bx) = pack_box(&inline, i) {
                boxes_checked += 1;
                assert!(
                    (1..=LINES_PER_BOX).contains(&bx.lines.len()),
                    "[town01] box at 0x{i:04X} packed {} lines (cap {LINES_PER_BOX})",
                    bx.lines.len()
                );
                // Every line range is well-formed and inside the buffer.
                for r in &bx.lines {
                    assert!(r.start <= r.end && r.end <= inline.len());
                }
            }
        }
    }
    assert!(
        pools >= 10,
        "town01 must classify enough NPC pools (got {pools})"
    );
    assert!(
        boxes_checked >= 100,
        "expected many packable boxes (got {boxes_checked})"
    );
    eprintln!(
        "town01: {pools} NPC pools, {boxes_checked} boxes packed, all <= {LINES_PER_BOX} lines"
    );
}

#[test]
fn rim_elm_sparring_opening_packs_three_pages_then_a_four_option_menu() {
    use legaia_asset::man_section::parse as parse_man;
    use legaia_engine_core::encounter_record::{
        RIM_ELM_SPARRING_CARRIER_MODEL, RIM_ELM_SPARRING_CARRIER_TILE,
    };
    use legaia_engine_core::man_field_scripts::{PlacementKind, classify_placements};
    use legaia_mes::{Dispatch, pack_boxes};

    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open prot index"));
    let scene = Scene::load(&index, "town01").expect("load town01");
    let man_bytes = scene
        .field_man_payload(&index)
        .expect("read MAN")
        .expect("town01 MAN");
    let man = parse_man(&man_bytes).expect("parse MAN");

    // Find the sparring partner (Tetsu) carrier by its pinned tile + model.
    let (ctx, ctz) = RIM_ELM_SPARRING_CARRIER_TILE;
    let inline = classify_placements(&man, &man_bytes)
        .into_iter()
        .find_map(|(p, kind)| {
            if (p.tile_x, p.tile_z) == (ctx, ctz)
                && p.model_index == RIM_ELM_SPARRING_CARRIER_MODEL
                && let PlacementKind::Npc {
                    dialog_inline: Some(inline),
                    ..
                } = kind
            {
                Some(inline)
            } else {
                None
            }
        })
        .expect("sparring carrier NPC with inline dialogue");

    let lead = inline
        .iter()
        .position(|&b| b == 0x1F)
        .expect("carrier pool has a text segment");
    let boxes = pack_boxes(&inline, lead, 64);

    // The opening branch is three full pages of narration (each capped at 3
    // lines, chained by `0x24` NextPage) then a 2-line box that opens the
    // "do you want something today?" 4-option menu.
    assert_eq!(
        boxes.len(),
        4,
        "opening branch = 3 narration pages + 1 menu box"
    );
    for (i, b) in boxes.iter().take(3).enumerate() {
        assert_eq!(b.lines.len(), 3, "narration page {i} fills three rows");
        assert_eq!(
            b.dispatch,
            Dispatch::NextPage,
            "narration page {i} chains via NextPage"
        );
    }
    assert_eq!(boxes[3].lines.len(), 2);
    assert_eq!(
        boxes[3].dispatch,
        Dispatch::Picker(4),
        "the opening narration ends on the 4-option topic menu"
    );

    // The 0xC? escape: page 0 line 1 is "Mist appeared, .., but" - it carries a
    // 0xC1 (character-name substitution) whose argument byte is 0x00. Packing
    // must NOT truncate the line at that 0x00; the bytes after the escape must
    // survive (the line ends ", but"). This is exactly the case the SM's
    // `(byte & 0xF0) == 0xC0` advance skip exists to handle.
    let line = &inline[boxes[0].lines[1].clone()];
    assert!(
        line.windows(2).any(|w| w[0] & 0xF0 == 0xC0),
        "the second narration line carries a 0xC? escape, got {line:02X?}"
    );
    let printable: String = line
        .iter()
        .map(|&c| {
            if (0x20..=0x7E).contains(&c) {
                c as char
            } else {
                '.'
            }
        })
        .collect();
    assert!(
        printable.contains(", but"),
        "the 0xC? escape must not truncate the line; got \"{printable}\""
    );
}
