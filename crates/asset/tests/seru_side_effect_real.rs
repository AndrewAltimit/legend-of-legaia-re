//! Disc-gated: the Seru-magic side-effect table (`0x801F6870`) parses out of
//! the real PROT 0898 (battle-action overlay) entry at the pinned offset with
//! the retail ladder - `5/10/15/20` percent per level band on the six
//! damaging element rows, cure classes `1..=4` on the light row - and every
//! record's banner pointer lands on a NUL-terminated string inside the same
//! overlay image. Skips and passes when `LEGAIA_DISC_BIN` / `extracted/` is
//! absent (the workspace disc-gated convention).

use std::path::PathBuf;

use legaia_asset::seru_side_effect::{
    self, OVERLAY_LINK_BASE, RETAIL_CURE_CLASS_BY_BAND, RETAIL_PERCENT_BY_BAND,
    SeruSideEffectTable, SideEffectKind,
};
use legaia_prot::archive::Archive;

fn extracted_prot() -> Option<PathBuf> {
    for base in ["extracted", "../../extracted"] {
        let prot = PathBuf::from(base).join("PROT.DAT");
        if prot.is_file() {
            return Some(prot);
        }
    }
    None
}

fn overlay_0898() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let prot = extracted_prot()?;
    let mut archive = Archive::open(&prot).ok()?;
    let entry = archive
        .entries
        .get(seru_side_effect::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .cloned()?;
    let mut bytes = Vec::new();
    archive.read_entry(&entry, &mut bytes).ok()?;
    Some(bytes)
}

#[test]
fn side_effect_table_carries_the_retail_ladder() {
    let Some(bytes) = overlay_0898() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT.DAT missing (disc-gated)");
        return;
    };
    let table = SeruSideEffectTable::parse(&bytes).expect("table parses");
    for (e, row) in table.rows().iter().enumerate() {
        let kind = SideEffectKind::for_element(e as u8);
        for (b, rec) in row.iter().enumerate() {
            let want = if kind == SideEffectKind::Cure {
                RETAIL_CURE_CLASS_BY_BAND[b]
            } else {
                RETAIL_PERCENT_BY_BAND[b]
            };
            assert_eq!(rec.amount, want, "element {e} band {b}");
            // The banner word points at a NUL-terminated string in the overlay.
            let off = rec
                .banner_va
                .checked_sub(OVERLAY_LINK_BASE)
                .expect("in-overlay VA") as usize;
            assert!(
                off < bytes.len(),
                "element {e} band {b}: banner {:#x}",
                rec.banner_va
            );
            let s = &bytes[off..];
            let nul = s.iter().position(|&c| c == 0).expect("terminated");
            assert!(
                nul > 8 && nul < 64,
                "element {e} band {b}: string length {nul}"
            );
            assert!(
                s[..nul].iter().all(|c| c.is_ascii_graphic() || *c == b' '),
                "element {e} band {b}: not text"
            );
        }
    }
    // Level bands: 3-4 / 5-6 / 7-8 / 9; below 3 nothing.
    assert_eq!(table.amount(2, 2), 0);
    assert_eq!(table.amount(2, 3), 5);
    assert_eq!(table.amount(2, 4), 5);
    assert_eq!(table.amount(2, 5), 10);
    assert_eq!(table.amount(3, 8), 15);
    assert_eq!(table.amount(6, 9), 20);
    assert_eq!(table.amount(5, 9), 4);
}
