//! Smoke test that loads the real extracted dialog font from
//! `extracted/font/` if present. Skips and passes when the artifacts aren't
//! on disk — same gating pattern as the disc-dependent integration tests so
//! CI doesn't need redistributed Sony bytes.

use legaia_font::{COLS, Font, GLYPH_H, GLYPH_W, LINE_HEIGHT, ROWS};
use std::path::PathBuf;

fn extracted_root() -> Option<PathBuf> {
    // Climb out of the workspace's per-crate cargo test cwd. The repo root
    // is two parents up from this crate's manifest dir.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let candidate = workspace.join("extracted");
    if candidate.join("font").is_dir() {
        Some(candidate)
    } else {
        None
    }
}

#[test]
fn loads_real_extracted_font_or_skips() {
    let Some(root) = extracted_root() else {
        eprintln!("extracted/font not present — skipping");
        return;
    };
    let font = Font::load_from_extracted(&root).expect("load extracted font");
    let (w, h) = font.atlas_dimensions();
    assert_eq!(w, COLS * GLYPH_W);
    assert_eq!(h, ROWS * GLYPH_H);

    // Spot-check a known width from docs/formats/dialog-font.md.
    assert_eq!(
        font.advance_of(b'A'),
        7 + 1,
        "'A' width should be 7 (+1 pad)"
    );
    assert_eq!(
        font.advance_of(b'I'),
        3 + 1,
        "'I' width should be 3 (+1 pad)"
    );
    assert_eq!(
        font.advance_of(b'M'),
        8 + 1,
        "'M' width should be 8 (+1 pad)"
    );

    // Layout a real string and sanity-check it.
    let layout = font.layout_ascii("Hello, world!");
    assert!(layout.glyphs.len() >= 10);
    assert!(layout.advance_x > 0);
    assert_eq!(layout.advance_y, LINE_HEIGHT);
}
