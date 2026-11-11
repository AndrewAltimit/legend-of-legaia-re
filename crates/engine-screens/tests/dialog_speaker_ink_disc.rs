//! Disc-gated: a Ra-Seru speaker tag draws in its ink, on both hosts' reading
//! box.
//!
//! The `rikuroa` event-script carrier (extraction PROT 0157) opens a line at
//! `+0x3A33` on `1F CF 04 C7 01 CF 07`: ink 4, the Ra-Seru name substitution
//! `C7 01`, back to ink 7, then the rest of the line. Retail's renderer
//! `FUN_80036888` restages the ink from each `0xCF` pair
//! (`0x80036944..0x8003695C`), so the name prints green and the line white.
//! The port read the pair as "draw the operand alone" and drew the whole tag
//! white (GitHub issue 581).
//!
//! The panel runs the disc's own line through `OwnedDialogPanel` and the
//! shared snapshot + reading-box builder both hosts draw from. The
//! substituted name is a test string, so no disc text is asserted - only the
//! escape bytes and the inks.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_dialog::dialog::OwnedDialogPanel;
use legaia_engine_scene::scene::ProtIndex;

/// Extraction index of the `rikuroa` carrier.
const CARRIER: u32 = 157;
/// The `0x1F` lead of the speaker-tagged line.
const LEAD: usize = 0x3A33;
/// A stand-in name for the `C7 01` substitution (not disc text).
const NAME: &[u8] = b"Zed";

fn extracted_dir() -> Option<PathBuf> {
    ["extracted", "../extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

#[test]
fn a_ra_seru_speaker_tag_draws_in_its_ink() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("PROT index");
    let bytes = index.entry_bytes(CARRIER).expect("carrier bytes");
    assert_eq!(
        &bytes[LEAD..LEAD + 7],
        &[0x1F, 0xCF, 0x04, 0xC7, 0x01, 0xCF, 0x07],
        "the line opens on the speaker tag's colour pairs"
    );

    let mut panel = OwnedDialogPanel::at_segment(Arc::new(bytes.to_vec()), LEAD);
    let mut subs = std::collections::HashMap::new();
    subs.insert((7u8, 1u8), NAME.to_vec());
    panel.substitutions = Some(Arc::new(subs));
    for _ in 0..400 {
        panel.tick();
    }
    let page = legaia_engine_ui::dialog_page_string(&panel.page_bytes());
    let font = legaia_font::synthetic_for_tests();
    let draws = legaia_engine_ui::dialog_reading_box_text_draws_for(&font, &page, (0, 0), 0, None);
    assert!(draws.len() > NAME.len() + 1, "the line typed out");
    let green = legaia_engine_ui::menu_ink_color(4);
    for (i, d) in draws.iter().take(NAME.len()).enumerate() {
        assert_eq!(d.color, green, "name glyph {i} in ink 4");
    }
    assert_eq!(
        draws[NAME.len()].color,
        legaia_engine_ui::MENU_TEXT_WHITE,
        "the line after the tag is back in ink 7"
    );
}
