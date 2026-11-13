//! Disc-gated: every item description fits the battle Item window's
//! description box, by retail's own rule.
//!
//! The rule is the string primitive's: the MES `0x7C` token is the only line
//! break (`FUN_80036888`, x back to the pen and y down `0xE`), nothing wraps
//! and nothing clips, and a line's width is the pen advance
//! [`legaia_font::Font::measure`] returns (`FUN_80035F04`, glyph pad `0` on a
//! battle surface). So each line must end inside the box's content edge and
//! every line must sit above its foot.
//!
//! It then draws each description through the window's own builder
//! ([`battle_item_window_text`]) and holds every glyph inside the box - the
//! check that failed for the Speed Elixir (issue #586), whose second line was
//! drawn on the first's row and ran past the right edge.
//!
//! Skips + passes without `LEGAIA_DISC_BIN` and an extracted `SCUS_942.54` /
//! font.

use legaia_engine_ui::battle_item_ui::{
    BattleItemMenuFrame, DESC_LINE_PITCH, DESC_SEAT, DESC_WINDOW, battle_item_window_text,
    desc_lines,
};
use legaia_font::{Font, measure::MeasureOptions};
use legaia_game_tables::item_names::ItemNameTable;
use std::path::PathBuf;

/// The window skin's frame: the 9-slice corner cells are 8 px, so the content
/// edge sits 8 px inside the window rect (the pen `(16, 130)` is that edge
/// plus the 8-px content inset on the left).
const FRAME_PX: i32 = 8;

/// Retail's own overrun: item `0xAB`'s `Found in West Voz Forest.` advances
/// 147 px, two past the content edge, into the frame's inner cells - drawn
/// that way on retail too, since nothing clips. It still ends inside the
/// window rect, which is what this test holds it to.
const RETAIL_FRAME_OVERRUN: &[u8] = &[0xAB];

fn load() -> Option<(Vec<u8>, Font)> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
    let scus = std::fs::read(root.join("SCUS_942.54")).ok()?;
    let font = Font::load_from_extracted(&root).ok()?;
    Some((scus, font))
}

#[test]
fn every_item_description_fits_the_battle_description_box() {
    let Some((scus, font)) = load() else {
        eprintln!("LEGAIA_DISC_BIN / extracted SCUS + font unset - skipping");
        return;
    };
    let table = ItemNameTable::from_scus(&scus).expect("item-name table");
    let (wx, wy, ww, wh) = DESC_WINDOW;
    // `DESC_WINDOW` spans x `8..=167`: its content ends one frame short of
    // the last column, exclusive.
    let right = wx + ww + 1 - FRAME_PX;
    let bottom = wy + wh;
    let opts = MeasureOptions::default();
    let mut checked = 0;
    let mut two_line = 0;
    let mut over: Vec<String> = Vec::new();
    for id in 0..=255u8 {
        // An id with no name never reaches a retail bag (the five unnamed
        // ids are expansion-pack aliases, which carry the pack's own text).
        let (Some(_), Some(desc)) = (table.name(id), table.desc(id)) else {
            continue;
        };
        let edge = if RETAIL_FRAME_OVERRUN.contains(&id) {
            wx + ww + 1
        } else {
            right
        };
        checked += 1;
        let lines: Vec<&str> = desc_lines(desc).collect();
        if lines.len() > 1 {
            two_line += 1;
        }
        for (i, line) in lines.iter().enumerate() {
            let bytes: Vec<u8> = line.chars().map(|c| c as u32 as u8).collect();
            let w = font.measure(&bytes, &opts).max_px as i32;
            // The ink ends the fixed one-pixel gap short of the advance.
            if DESC_SEAT.0 + w - 1 > edge {
                over.push(format!("{id:#04X} line {i} {line:?}: advance {w}"));
            }
            // The line's glyph row is 12 px tall under its pen's +2 ink offset.
            let foot = DESC_SEAT.1 + i as i32 * DESC_LINE_PITCH + 2 + 12;
            assert!(
                foot <= bottom,
                "item {id:#04X} line {i} {line:?}: foot {foot} below the box {bottom}"
            );
        }
        // The builder's own draws stay inside the window.
        let frame = BattleItemMenuFrame {
            description: Some(desc),
            ..Default::default()
        };
        for d in battle_item_window_text(&font, &frame, false, (0, 0), 1)
            .iter()
            .filter(|d| d.dst.1 >= DESC_SEAT.1)
        {
            let (x, y, w, h) = d.dst;
            // A draw is a whole font cell, wider than its ink; the advance
            // check above is the width rule, this one holds where each glyph
            // starts and that no row falls out of the box.
            let _ = w;
            if !(x >= wx && x < edge && y + h as i32 <= bottom) {
                over.push(format!("{id:#04X} {desc:?}: drawn glyph at {:?}", d.dst));
                break;
            }
        }
    }
    assert!(
        over.is_empty(),
        "lines past the box edge {right} from x {}:\n{}",
        DESC_SEAT.0,
        over.join("\n")
    );
    assert!(checked > 100, "only {checked} descriptions read");
    assert!(two_line > 0, "no two-line description - the break was lost");
}
