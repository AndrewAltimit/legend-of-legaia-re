//! The place-name banner (the `4C E1` text balloon) keeps its line inside
//! its frame for every name the engine shows: the retail scene table's
//! section-2 banners and the Rebirth pack's map names, the longest of which
//! overhang retail's fixed `0x90`-wide frame.
//!
//! Measured in the disc's dialog font when `extracted/font/` is present
//! (the widths the banner really has), else in the placeholder font - whose
//! fixed 8-px advance overstates every name, so the containment half still
//! holds and the retail-identity half is only asserted on the real font.

use legaia_engine_dialog::text_balloon::{BALLOON_FIT_MAX_W, TextBalloon};
use legaia_engine_ui as ui;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// Every `<key> = "<value>"` string on a line of its own in `text`.
fn quoted(text: &str, key: &str) -> Vec<String> {
    let prefix = format!("{key} = \"");
    text.lines()
        .filter_map(|l| l.strip_prefix(&prefix)?.strip_suffix('"'))
        .map(str::to_string)
        .collect()
}

fn retail_banners() -> Vec<String> {
    let text = std::fs::read_to_string(format!("{ROOT}/data/gamedata/scenes.toml")).unwrap();
    quoted(&text, "banner")
}

fn pack_map_names() -> Vec<String> {
    let dir = format!("{ROOT}/packs/rebirth/maps");
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let text = std::fs::read_to_string(e.unwrap().path()).unwrap();
        out.extend(quoted(&text, "name").into_iter().take(1));
    }
    out
}

fn font() -> (legaia_font::Font, bool) {
    match legaia_font::Font::load_from_extracted(format!("{ROOT}/extracted")) {
        Ok(f) => (f, true),
        Err(_) => (legaia_font::Font::placeholder(), false),
    }
}

/// The line lies inside the frame: every glyph starts inside it and the
/// line's advance ends inside it. (A glyph quad is the font's whole cell,
/// wider than its advance; the cell's tail past the advance is blank.)
fn assert_inside(font: &legaia_font::Font, name: &str) -> (i32, i32, i32, i32) {
    let b = TextBalloon::spawn(name.as_bytes());
    let fit = b.fitted(ui::text_balloon_text_width(font, name.as_bytes()));
    let (fx, _, fw, _) = fit.frame;
    let draws = ui::text_balloon_text_draws_fitted(font, name.as_bytes(), fit.pen, fit.text_w);
    assert!(!draws.is_empty(), "{name}");
    for d in &draws {
        assert!(
            d.dst.0 >= fx && d.dst.0 < fx + fw,
            "{name:?}: glyph at {} starts outside frame {fx}..{}",
            d.dst.0,
            fx + fw
        );
    }
    assert!(
        fit.pen.0 >= fx && fit.pen.0 + fit.text_w <= fx + fw,
        "{name:?}: line {}..{} leaves frame {fx}..{}",
        fit.pen.0,
        fit.pen.0 + fit.text_w,
        fx + fw
    );
    fit.frame
}

#[test]
fn every_banner_line_stays_inside_its_frame() {
    let (font, real) = font();
    let retail = retail_banners();
    let pack = pack_map_names();
    assert!(retail.len() > 50 && pack.len() >= 10, "names read");
    assert!(
        pack.iter()
            .any(|n| n == "Tower of Rebirth 7F - Sanctum of Echoes"),
        "the longest pack name is in the set"
    );
    for name in retail.iter().chain(&pack) {
        let frame = assert_inside(&font, name);
        assert!(frame.2 <= i32::from(BALLOON_FIT_MAX_W));
    }
    if real {
        // Every retail banner fits retail's own frame: drawn exactly as retail.
        for name in &retail {
            let b = TextBalloon::spawn_measured(
                name.as_bytes(),
                ui::text_balloon_text_width(&font, name.as_bytes()),
            );
            let fit = b.fitted(ui::text_balloon_text_width(&font, name.as_bytes()));
            assert_eq!(fit.frame, b.frame_rect(), "{name}");
            assert_eq!(Some(fit.pen), b.pen(), "{name}");
        }
        // The long pack names widen the frame but are not condensed.
        for name in &pack {
            let w = ui::text_balloon_text_width(&font, name.as_bytes());
            let fit = TextBalloon::spawn(name.as_bytes()).fitted(w);
            assert_eq!(fit.text_w, i32::from(w), "{name} is drawn at full width");
        }
    }
}
