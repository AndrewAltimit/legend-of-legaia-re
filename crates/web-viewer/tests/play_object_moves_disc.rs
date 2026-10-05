//! Disc-gated: the browser play page draws a placed object where its actor
//! is, through `LegaiaRuntime::field_placement_moves` - the table the page's
//! `_applyObjectMoves` folds into each placed draw.
//!
//! `chitei2`'s collapse is the case: the boulder pieces (partition-0 records
//! 28..30) are born parked 700 units up, and the boulder beat the escape walk
//! triggers seats them at the foot of the stairs and drops them to the floor.
//! The runtime is driven the way the page drives it - pad words and
//! `tick_frame` - from a re-entry with the collapse flag up.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` / `extracted/` are missing.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use legaia_web_viewer::runtime::LegaiaRuntime;

const PAD_DOWN: u16 = 0x0040;
const PAD_CROSS: u16 = 0x4000;

fn disc_bytes() -> Option<Vec<u8>> {
    let path = std::env::var_os("LEGAIA_DISC_BIN")?;
    std::fs::read(PathBuf::from(path)).ok()
}

/// `(placement, [dx, dy, dz])` for every moved placement this frame.
fn moves(rt: &LegaiaRuntime) -> Vec<(usize, [i32; 3])> {
    rt.field_placement_moves()
        .chunks(3)
        .enumerate()
        .filter(|(_, d)| d.iter().any(|&v| v != 0.0))
        .map(|(i, d)| (i, [d[0] as i32, d[1] as i32, d[2] as i32]))
        .collect()
}

#[test]
fn the_play_page_drops_the_chitei2_boulder_or_skip() {
    let Some(bytes) = disc_bytes() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load_disc");
    rt.enter_field("chitei2").expect("enter chitei2");
    // The collapse beat plays at entry once the fortress is coming apart.
    assert!(rt.debug_system_flag_set(0x4C6));
    rt.enter_field("chitei2").expect("re-enter chitei2");

    let parked = moves(&rt);
    assert!(
        parked.iter().filter(|(_, d)| d[1] == -700).count() >= 3,
        "the three boulder pieces are parked 700 up at entry: {parked:?}"
    );

    let mut landed: Vec<(usize, [i32; 3])> = Vec::new();
    for f in 0..4000u32 {
        // Advance the collapse beat's dialogue, then run for the stairs.
        let pad = if (900..1500).contains(&f) {
            PAD_DOWN
        } else if f % 20 == 0 {
            PAD_CROSS
        } else {
            0
        };
        rt.set_pad(pad);
        rt.tick_frame().expect("tick_frame");
        let now = moves(&rt);
        let fallen: Vec<_> = now
            .into_iter()
            .filter(|(_, d)| d[1] == 0 && d[0] != 0 && d[2] != 0)
            .collect();
        if fallen.len() >= 3 {
            landed = fallen;
            break;
        }
    }
    eprintln!("[ran] landed boulder placements: {landed:?}");
    assert!(
        landed.len() >= 3,
        "the boulder beat seated and dropped the three pieces"
    );
}
