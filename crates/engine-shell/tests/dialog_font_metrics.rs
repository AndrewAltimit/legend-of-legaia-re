//! Disc-gated oracle for the **native text metrics source**.
//!
//! Retail's glyph advance is proportional: the single-line renderer
//! `FUN_80036888` accumulates `pen_x += widths[c] + DAT_800740E8 + 1` per
//! byte (body `0x80036B9C`; the width table is the biased `0x80073F3C - 0x20`
//! = `0x80073F1C`, `DAT_800740E8` is the per-string padding override and is
//! zero for ordinary strings). `legaia_font::Font::advance_of` already models
//! that law - what used to go wrong is *which font the native window picked*:
//! `play-window` only ever tried `extracted/font/`, which exists solely after
//! a `font-extract` run against a mednafen save state. A plain
//! `--disc <image>` boot therefore fell through to the fixed-width
//! placeholder (a flat 9px advance for every glyph) and rendered every string
//! roughly a third too wide - the save-screen "Do not remove MEMORY CARD"
//! line measured 225px against retail's 164px.
//!
//! [`BootSession::dialog_font`] closes that: the font is decoded from the
//! boot source itself (the `PROT.DAT` 4bpp font TIM + the `SCUS_942.54`
//! advance table), no save state involved. This test asserts a disc boot
//! actually carries it and that its metrics are the retail proportional ones,
//! not the placeholder's.
//!
//! Skip-pass (CLAUDE.md disc-gated convention) when `LEGAIA_DISC_BIN` is
//! unset.

use legaia_engine_shell::boot::{BootConfig, BootSession};
use legaia_font::Font;
use std::path::PathBuf;

/// The save-screen line whose width the placeholder font used to inflate.
const NOW_CHECKING_LINE: &str = "Do not remove MEMORY CARD";

fn disc_path() -> Option<PathBuf> {
    let p = std::env::var_os("LEGAIA_DISC_BIN")?;
    let p = PathBuf::from(p);
    p.exists().then_some(p)
}

#[test]
fn disc_boot_carries_the_proportional_dialog_font() {
    let Some(disc) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let session = BootSession::open_disc(
        &disc,
        &BootConfig {
            scene: "town01".to_string(),
            enable_audio: false,
        },
    )
    .expect("open BootSession from disc");

    let font = session
        .dialog_font
        .as_ref()
        .expect("a disc boot decodes the retail dialog font with no save state");

    // Per-character advances from the SCUS table (docs/formats/dialog-font.md
    // sample rows), each carrying retail's fixed +1 inter-glyph gap.
    assert_eq!(font.advance_of(b'A'), 7 + 1);
    assert_eq!(font.advance_of(b'I'), 3 + 1);
    assert_eq!(font.advance_of(b'W'), 9 + 1);
    assert_eq!(font.advance_of(b' '), 4 + 1);

    // The whole-line width is the sum of those advances - `FUN_80036888` has
    // no per-line fudge, and the space byte takes the same advance path as a
    // glyph byte (it only skips the sprite emit).
    let expected: u32 = NOW_CHECKING_LINE
        .bytes()
        .map(|c| font.advance_of(c))
        .sum::<u32>();
    let measured = font.layout_ascii(NOW_CHECKING_LINE).advance_x;
    assert_eq!(measured, expected, "layout must sum the retail advances");
    assert_eq!(
        measured, 164,
        "retail width of the save-screen memory-card line"
    );

    // And the regression guard: the placeholder is visibly wider, so a boot
    // that silently fell back to it is detectable by measurement alone.
    let placeholder = Font::placeholder()
        .layout_ascii(NOW_CHECKING_LINE)
        .advance_x;
    assert!(
        placeholder > measured + 40,
        "placeholder ({placeholder}px) must be distinguishable from retail ({measured}px)"
    );
}
