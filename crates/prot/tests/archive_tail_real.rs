//! Disc-gated regression for the PROT TOC tail: the last rows before the
//! zeroed TOC padding underflow the indexed size formula
//! (`toc[p+5] - toc[p+3] + 4`) and must fall back to the LBA footprint
//! instead of being silently dropped. Retail extraction **1231** is exactly
//! this shape - and it is real, reachable content: the dance minigame's SFX
//! sample VAB (`VABp`), loaded by the dance overlay (PROT 0980) as raw TOC
//! `0x4D1`. See `docs/subsystems/minigame-dance.md`.
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset or `extracted/PROT.DAT`
//! is missing.

use std::path::PathBuf;

use legaia_prot::archive::Archive;

fn prot_dat() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT.DAT", "../../extracted/PROT.DAT"] {
        let f = PathBuf::from(p);
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

#[test]
fn toc_tail_entry_1231_is_the_dance_sfx_vab() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    let mut arch = Archive::open(&prot).expect("PROT.DAT parses");

    // The tail rows resolve (they used to be dropped by the underflow).
    let e1231 = arch
        .entries
        .iter()
        .find(|e| e.index == 1231)
        .cloned()
        .expect("extraction 1231 present");
    assert!(
        arch.entries.iter().any(|e| e.index == 1232),
        "extraction 1232 present"
    );

    // 1231 carries a VAB bank header ("pBAV" on disc) near the entry head.
    let mut buf = Vec::new();
    arch.read_entry(&e1231, &mut buf).expect("1231 reads");
    let head = &buf[..buf.len().min(64)];
    assert!(
        head.windows(4).any(|w| w == b"pBAV"),
        "extraction 1231 is a VAB bank (dance SFX samples)"
    );
}
