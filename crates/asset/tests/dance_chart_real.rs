//! Disc-gated reproducibility for the dance-minigame step chart.
//!
//! Re-extract the dance overlay (PROT 0980) from the user's `PROT.DAT`, decode
//! the baked step chart, and assert the structural invariants that pin the
//! "baked into the overlay" finding (no Sony bytes asserted):
//!
//! * the chart region is non-zero in the as-loaded image (baked, not `.bss`);
//! * it decodes to [`DANCE_CHART_ROWS`] rows of valid direction symbols;
//! * difficulty climbs with the row (`step_count` is non-decreasing) - the
//!   gauge-selected lane is also the difficulty selector.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / `extracted/PROT.DAT` are absent.

use std::path::PathBuf;

use legaia_asset::dance_chart::{self, DANCE_CHART_ROWS};
use legaia_asset::static_overlay;
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

fn dance_overlay() -> Option<Vec<u8>> {
    let prot = prot_dat()?;
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let rec = static_overlay::overlay_map()
        .by_prot_index(dance_chart::DANCE_OVERLAY_PROT_INDEX as u32)
        .expect("dance overlay in static map");
    let entry = archive
        .entries
        .iter()
        .find(|e| e.index == rec.prot_index)
        .cloned()
        .expect("PROT entry present");
    let mut raw = Vec::new();
    archive.read_entry(&entry, &mut raw).expect("read entry");
    Some(static_overlay::as_loaded(&raw, rec).expect("as-loaded form"))
}

#[test]
fn step_chart_is_baked_and_well_formed() {
    let Some(overlay) = dance_overlay() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };

    // Baked: the chart region is non-zero in the static image.
    let off = dance_chart::DANCE_CHART_FILE_OFFSET;
    let span = &overlay[off..off + DANCE_CHART_ROWS * dance_chart::BEATS_PER_ROW];
    assert!(
        span.iter().any(|&b| b != 0),
        "step chart should be baked (non-zero) in the overlay image"
    );

    let chart = dance_chart::parse(&overlay).expect("step chart parses as a symbol grid");
    assert_eq!(chart.rows.len(), DANCE_CHART_ROWS);

    // Difficulty is non-decreasing with the row (the gauge lane raises density).
    let counts: Vec<usize> = (0..DANCE_CHART_ROWS).map(|r| chart.step_count(r)).collect();
    assert!(counts[0] > 0, "row 0 has at least one step");
    for w in counts.windows(2) {
        assert!(w[1] >= w[0], "row density non-decreasing: {counts:?}");
    }
}
