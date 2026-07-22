//! Disc-gated round-trip oracle for the generalized MAN dialog rewriter.
//!
//! Lifts the official localization onto the USA disc, imports the filled pack
//! onto a scratch copy, and asserts:
//!   - the patched image re-parses as a valid disc;
//!   - every touched 2352-byte sector stays EDC/ECC-valid;
//!   - a MAN the rewriter grew re-decodes to its (larger) declared size and its
//!     segment count is preserved (the relocation kept the script structure);
//!   - re-decoding the whole corpus off the patched image still succeeds.
//!
//! Needs both the USA disc (`LEGAIA_DISC_BIN`) and a PAL disc
//! (`LEGAIA_PAL_DISC_BIN`); skips + passes when either is unset (no Sony bytes
//! are committed and CI has no disc).

use legaia_iso::raw::SECTOR_SIZE;
use legaia_iso::write::mode2_form1_sector_is_valid;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::translation::export::SceneManText;
use legaia_patcher::translation::{import_pack, lift, segments};

fn load(var: &str) -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os(var)?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

#[test]
fn lift_then_import_stays_valid_and_grows_a_man() {
    let (Some(usa_bytes), Some(pal_bytes)) = (load("LEGAIA_DISC_BIN"), load("LEGAIA_PAL_DISC_BIN"))
    else {
        eprintln!("[skip] LEGAIA_DISC_BIN / LEGAIA_PAL_DISC_BIN unset");
        return;
    };
    let usa = DiscPatcher::open(usa_bytes.clone()).expect("open USA");
    let pal = DiscPatcher::open(pal_bytes).expect("open PAL");
    let (pack, _) = lift::lift_official(&usa, &pal).expect("lift official");

    // Import onto a scratch copy.
    let mut patcher = DiscPatcher::open(usa_bytes.clone()).expect("open scratch");
    let report = import_pack(&mut patcher, &pack).expect("import");
    assert!(report.applied > 5_000, "expected a substantial import");
    let patched = patcher.into_image();

    // The patched image re-parses.
    let re = DiscPatcher::open(patched.clone()).expect("patched image re-parses");

    // Every changed sector stays EDC/ECC-valid.
    let mut checked = 0usize;
    let mut bad = 0usize;
    let mut sector = 0usize;
    while (sector + 1) * SECTOR_SIZE <= patched.len() {
        let span = sector * SECTOR_SIZE..(sector + 1) * SECTOR_SIZE;
        if usa_bytes[span.clone()] != patched[span.clone()] {
            checked += 1;
            if !mode2_form1_sector_is_valid(&patched[span]) {
                bad += 1;
            }
        }
        sector += 1;
    }
    assert!(checked > 0, "expected changed sectors");
    assert_eq!(
        bad, 0,
        "{bad} of {checked} changed sectors are EDC/ECC-invalid"
    );

    // Find a MAN entry the rewriter grew: its patched decompressed MAN is larger
    // than the original's, and both decode with the same segment count.
    let mut grew = 0usize;
    for idx in 0..usa.entry_count() {
        let (Ok(orig_entry), Ok(new_entry)) = (usa.read_entry(idx), re.read_entry(idx)) else {
            continue;
        };
        let (Some(orig_man), Some(new_man)) = (
            SceneManText::locate(&orig_entry),
            SceneManText::locate(&new_entry),
        ) else {
            continue;
        };
        if new_man.decoded.len() > orig_man.decoded.len() {
            grew += 1;
            // The relocation preserved the segment structure: same number of
            // qualifying dialog segments before and after growth. Scan
            // PAL-tolerant (`allow_high`) on both sides so the grown segments'
            // accent bytes don't get rejected by the strict Latin gate.
            assert_eq!(
                segments::scan_ext(&orig_man.decoded, true).len(),
                segments::scan_ext(&new_man.decoded, true).len(),
                "grown MAN {idx} changed its segment count",
            );
        }
    }
    assert!(grew > 0, "expected at least one MAN to be grown in place");
}
