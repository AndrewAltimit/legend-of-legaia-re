//! Disc-gated invariants for the screen-element placement table
//! (`SCUS_942.54` VA `0x80076C10`, `0x18` stride).
//!
//! The table is the seat book the battle chrome derives its plates from, and
//! the seats are disc data. This asserts the structural facts a consumer
//! relies on - no Sony bytes are reproduced, only geometry the format docs
//! already state:
//!
//! * the initialised run decodes, every coordinate in it stays on-screen-ish,
//!   and the record index that would reach the steal table is where the
//!   format doc's `0x18` stride says it is;
//! * the named battle-chrome records sit where the packet walk found them -
//!   the actor-name plaque at `(16, 14)` live / `(16, -24)` parked, the
//!   active-actor bar 288 wide at `(16, 194)`, the three roster panels
//!   `88 x 50` on a 102-pixel pitch, the four command chips 48 wide around
//!   `(228, 70)` on a `44 x 32` diamond;
//! * the plaque's width is `0` on the disc, because the runtime measures it
//!   from the actor's name;
//! * the derived plate for each of those boxes is the rect the packets drew.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / `extracted/` are absent.

use std::path::PathBuf;

use legaia_asset::screen_elements::{
    LINE_HEIGHT, RECORD_ACTIVE_BAR, RECORD_COUNT, RECORD_NAME_PLAQUE, RECORD_STRIDE,
    RECORDS_COMMAND_CHIP, RECORDS_PARTY_PANEL, STEAL_TABLE_RECORD_INDEX, ScreenElement,
    ScreenElementTable, TABLE_VA,
};

fn scus() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for dir in ["extracted", "../../extracted"] {
        let f = PathBuf::from(dir).join("SCUS_942.54");
        if f.is_file() {
            return std::fs::read(f).ok();
        }
    }
    None
}

#[test]
fn placement_table_decodes_and_seats_the_battle_chrome() {
    let Some(scus) = scus() else {
        eprintln!("skip: no LEGAIA_DISC_BIN / extracted/SCUS_942.54");
        return;
    };
    let table = ScreenElementTable::from_scus(&scus).expect("placement table decodes");
    assert_eq!(table.records().len(), RECORD_COUNT);

    // The run's hard ceiling: one more stride past record 128 is the steal
    // table, so a 200-record read would walk straight through it.
    assert_eq!(
        TABLE_VA as usize + STEAL_TABLE_RECORD_INDEX * RECORD_STRIDE,
        legaia_asset::steal_table::TABLE_VA as usize
    );
    const _: () = assert!(RECORD_COUNT < STEAL_TABLE_RECORD_INDEX);

    // Everything inside the run is a screen seat, not arbitrary data.
    for (i, r) in table.records().iter().enumerate() {
        for v in [r.seat.0, r.seat.1, r.alt_seat.0, r.alt_seat.1] {
            assert!(v.abs() <= 640, "record {i} coordinate {v} is off the map");
        }
        assert!(r.height >= 0 && r.height <= 120, "record {i} box height");
    }
    // The plate-run family all carry the 0x0C line height.
    for i in [RECORD_NAME_PLAQUE, RECORD_ACTIVE_BAR]
        .into_iter()
        .chain(RECORDS_COMMAND_CHIP)
    {
        assert_eq!(table.get(i).unwrap().height, LINE_HEIGHT, "record {i}");
    }

    // The actor-name plaque: seats fixed on the disc, width measured live.
    let plaque = table.get(RECORD_NAME_PLAQUE).unwrap();
    assert_eq!(plaque.alt_seat, (16, 14), "plaque live seat");
    assert_eq!(plaque.seat, (16, -24), "plaque parks above the screen");
    assert_eq!(plaque.width, 0, "plaque width is measured from the name");
    assert_eq!(plaque.alt_pen(), (16, 12));
    // A 63-pixel interior (the captured `Gimard` plaque) frames to (8, 8).
    assert_eq!(
        ScreenElement::plate_at(plaque.alt_pen(), 63),
        (8, 8, 79, 20)
    );

    // The full-width active-actor bar.
    let bar = table.get(RECORD_ACTIVE_BAR).unwrap();
    assert_eq!(bar.alt_seat, (16, 194));
    assert_eq!(bar.width, 288);
    assert_eq!(
        ScreenElement::plate_at(bar.alt_pen(), bar.width),
        (8, 188, 304, 20)
    );

    // The three roster panels, on a 102-pixel pitch at one row.
    let panels: Vec<_> = RECORDS_PARTY_PANEL
        .iter()
        .map(|&i| table.get(i).unwrap())
        .collect();
    for p in &panels {
        assert_eq!((p.width, p.height), (88, 50), "roster panel box");
        assert_eq!(p.alt_seat.1, 170, "roster panel row");
    }
    assert_eq!(panels[2].alt_seat.0 - panels[1].alt_seat.0, 102);

    // The command diamond: one interior width, 44 x 32 around (228, 70).
    let chips: Vec<_> = RECORDS_COMMAND_CHIP
        .iter()
        .map(|&i| table.get(i).unwrap())
        .collect();
    for c in &chips {
        assert_eq!(c.width, 48, "command chip interior");
    }
    let centre = |c: &ScreenElement| (c.alt_seat.0 + 24, c.alt_seat.1 - 2 + 6);
    let (up, left, right, down) = (
        centre(&chips[0]),
        centre(&chips[1]),
        centre(&chips[2]),
        centre(&chips[3]),
    );
    assert_eq!(up, (228, 38));
    assert_eq!(down, (228, 102));
    assert_eq!(left, (184, 70));
    assert_eq!(right, (272, 70));
    assert_eq!(right.0 - left.0, 88, "diamond width = 2 * dx");
    assert_eq!(down.1 - up.1, 64, "diamond height = 2 * dy");
}
