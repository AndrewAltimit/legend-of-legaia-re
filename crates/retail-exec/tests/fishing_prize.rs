//! Two fishing-overlay leaves (PROT 0972) against `engine-fishing`: the prize
//! row's availability `FUN_801D6F90` (affordable, owned count not at the cap,
//! a one-time row not latched in the purchased mask) against
//! `PrizeExchange::is_available`, and the reel-button decoder `FUN_801D7450`
//! against `ReelInput::from_pad_mask`.
//!
//! The availability reads the venue's row table through `0x801D90B8`, the
//! point pool `0x8008444C`, the purchased mask `0x8008446C` and the venue
//! `0x801D90D0`; the owned count comes from the bag counter `FUN_80042F4C`,
//! hooked to answer from the script.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_fishing::fishing::{PrizeExchange, PrizeRow, ReelInput};
use legaia_minigame_formats::fishing_species as fs;
use psx_retail_exec::{A0, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const AVAILABLE: u32 = 0x801D_6F90;
const DECODE: u32 = 0x801D_7450;
const BAG_COUNT: u32 = 0x8004_2F4C;
const TABLE_PTR: u32 = 0x801D_90B8;
const VENUE: u32 = 0x801D_90D0;
const POINTS: u32 = 0x8008_444C;
const MASK: u32 = 0x8008_446C;
const TABLE: u32 = 0x8012_0000;

#[test]
fn prize_availability_matches_retail() {
    differential!(prot(972, AVAILABLE));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    let owned = Rc::new(Cell::new(0u32));
    let asked = Rc::new(Cell::new(0u32));
    let (o, a) = (owned.clone(), asked.clone());
    cpu.hook(BAG_COUNT, move |r, _| {
        a.set(r.r[A0]);
        r.r[V0] = o.get();
    });
    cpu.mem.wr32(TABLE_PTR, TABLE);
    let mut rng = Rng(0x5052_495A);
    let (mut yes, mut no) = (0, 0);
    for run in 0..100_000 {
        let venue = rng.below(2) as usize;
        let rows: Vec<PrizeRow> = (0..6)
            .map(|row| PrizeRow {
                row,
                limit: [1, 99, 1 + rng.below(99)][rng.below(3) as usize],
                price: [0, 1, 50, 300, 1000, 5000, rng.below(10_000)][rng.below(7) as usize],
                item_id: rng.next() as u8,
                name: None,
            })
            .collect();
        for r in &rows {
            let at = TABLE + r.row as u32 * 12;
            cpu.mem.wr32(at, r.limit);
            cpu.mem.wr32(at + 4, r.price);
            cpu.mem.wr32(at + 8, u32::from(r.item_id));
        }
        let ex = PrizeExchange {
            venue,
            rows: rows.clone(),
            cursor: 0,
        };
        let row = rng.below(6) as usize;
        let points = [0, 49, 50, 999, 1000, 1001, rng.below(20_000) as i32][rng.below(7) as usize];
        let own = [0, 1, 98, 99, rng.below(100)][rng.below(5) as usize];
        let mask = rng.next() & rng.next();
        cpu.mem.wr32(VENUE, venue as u32);
        cpu.mem.wr32(POINTS, points as u32);
        cpu.mem.wr32(MASK, mask);
        owned.set(own);
        assert_eq!(cpu.call(AVAILABLE, &[row as u32]), Stop::Returned);
        let retail = cpu.regs.r[V0] != 0;
        let port = ex.is_available(row, points, own, mask);
        assert_eq!(
            retail, port,
            "run {run} venue {venue} row {row} price {} points {points} owned {own} mask {mask:#x}",
            rows[row].price
        );
        if rows[row].price <= points as u32 {
            assert_eq!(
                asked.get(),
                u32::from(rows[row].item_id),
                "the bag count asked"
            );
        }
        yes += u32::from(port);
        no += u32::from(!port);
    }
    assert!(yes > 15_000 && no > 15_000, "{yes} {no}");
}

#[test]
fn the_reel_decoder_matches_retail() {
    differential!(prot(972, DECODE));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    for mask in (0..0x1_0000u32).chain([u32::MAX, 0xFFFF_0040, 0x8000_0080]) {
        assert_eq!(cpu.call(DECODE, &[mask]), Stop::Returned);
        let port = match ReelInput::from_pad_mask(mask) {
            ReelInput::Idle => 0,
            ReelInput::ReelA => 1,
            ReelInput::ReelB => 2,
        };
        assert_eq!(cpu.regs.r[V0], port, "mask {mask:#x}");
    }
}
