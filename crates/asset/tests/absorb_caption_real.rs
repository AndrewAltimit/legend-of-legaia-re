//! Disc-gated: the Seru-absorb caption pieces (`0x801F4DFC` prefix table,
//! `0x801F4C28` suffix) parse out of the real PROT 0898 image at the pinned
//! link base - three non-empty printable prefixes and a non-empty suffix.
//! Skips and passes when `LEGAIA_DISC_BIN` / `extracted/` is absent. No
//! string bytes are asserted or printed.

use std::path::PathBuf;

use legaia_asset::absorb_caption::{self, PREFIX_ROWS};
use legaia_prot::archive::Archive;

fn overlay_0898() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let prot = ["extracted", "../../extracted"]
        .iter()
        .map(|b| PathBuf::from(b).join("PROT.DAT"))
        .find(|p| p.is_file())?;
    let mut archive = Archive::open(&prot).ok()?;
    let entry = archive
        .entries
        .get(absorb_caption::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .cloned()?;
    let mut bytes = Vec::new();
    archive.read_entry(&entry, &mut bytes).ok()?;
    Some(bytes)
}

#[test]
fn absorb_caption_parses_off_the_real_overlay() {
    let Some(img) = overlay_0898() else {
        eprintln!("[skip] LEGAIA_DISC_BIN / extracted/PROT.DAT missing");
        return;
    };
    let cap = absorb_caption::parse(&img).expect("caption pieces parse");
    assert_eq!(cap.prefixes.len(), PREFIX_ROWS);
    assert!(cap.prefixes.iter().all(|p| p.len() > 4));
    assert!(!cap.suffix.is_empty());
    // The three prefixes share one tail (the per-character part is the head).
    let tail = |s: &str| s.split_once(' ').map(|(_, t)| t.to_string());
    assert_eq!(tail(&cap.prefixes[0]), tail(&cap.prefixes[1]));
    assert_eq!(tail(&cap.prefixes[1]), tail(&cap.prefixes[2]));
    eprintln!("[ok] absorb caption: {} prefixes", cap.prefixes.len());
}
