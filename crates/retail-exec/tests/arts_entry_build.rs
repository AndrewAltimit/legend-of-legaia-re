//! The arts-entry screen build, `FUN_801D388C` cases `9` and `0x2C` (one
//! body), driven on its own: the four direction chips' records against
//! `retail_chip_icons` (the seat loop's icon words, Noa's swap, an empty
//! arm's plain word) and `ArtsInputFrame::chip_anchor` (each chip's seat A
//! and seat B `x` re-seated by its cost), the chip widths (`cost -
//! COST_WIDTH_BIAS`) and the four cost bytes `ctx[+0x14..]` the entry reads.
//!
//! Every character id, both arms filled or empty, costs from 0 to past the
//! zero point. The four chip records (placement records `0x1B..=0x1E`) are
//! compared whole. The name plate and the bar are outside this arm, and the
//! run stops where the case jumps to the shared step-script tail.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_engine_battle::arts_command_input::retail_chip_icons;
use legaia_engine_ui::arts_input::{
    ArtsInputFrame, ArtsInputScreen, COST_WIDTH_BIAS, ChipDirection,
};
use psx_retail_exec::{RETURN_SENTINEL, Stop};

const BUILD: u32 = 0x801D_388C;
const MEASURE: u32 = 0x8003_5F04;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CASE_TAIL: u32 = 0x801D_4BA4;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const SEATS: u32 = 0x8007_BD10;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const COST_TABLE: u32 = 0x801C_9360;
const COST_ROWS: u32 = 0x8016_0000;
const SEAT_COMMANDS: u32 = 0x801F_4B8C;
const PLACEMENTS: u32 = 0x8007_6C10;
const CHIP_RECORDS: u32 = PLACEMENTS + 0x1B * 0x18;

const SEAT_DIRS: [ChipDirection; 4] = [
    ChipDirection::Left,
    ChipDirection::High,
    ChipDirection::Low,
    ChipDirection::Right,
];

#[test]
fn the_arts_entry_seats_its_chips_as_retail() {
    differential!(prot(898, BUILD));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    // The case's body ends in a jump to the shared step-script tail; the run
    // stops there.
    cpu.hook(CASE_TAIL, |r, _| r.r[31] = RETURN_SENTINEL);
    cpu.stub(MEASURE, 0x40);
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + 4 * s, ACTOR + 0x400 * s);
    }
    let seat_cmds: Vec<u8> = cpu.mem.read(SEAT_COMMANDS, 4);
    let mut rng = Rng(0x4152_5453);
    let (mut swapped, mut plain, mut wide) = (0u32, 0u32, 0u32);
    for n in 0..20_000 {
        let case = if rng.below(2) == 0 { 9 } else { 0x2C };
        let acting = rng.below(3) as u8;
        let char_id = 1 + rng.below(4) as u8;
        cpu.mem.wr8(SEATS + u32::from(acting), char_id);
        cpu.mem.wr8(CTX + 0x13, acting);
        let mut equip = [0u8; 8];
        for b in equip.iter_mut() {
            *b = if rng.below(3) == 0 {
                0
            } else {
                1 + rng.below(0xFF) as u8
            };
        }
        cpu.mem
            .load(RECORDS + (u32::from(char_id) - 1) * STRIDE + 0x196, &equip);
        // The per-seat cost row: `*(*(0x801C9360[seat]) + cmd * 4) + 0x74`.
        let row = COST_ROWS + 0x400 * u32::from(acting);
        cpu.mem.wr32(COST_TABLE + 4 * u32::from(acting), row);
        let mut costs = [0u8; 4];
        for (seat, &cmd) in seat_cmds.iter().enumerate() {
            let cell = row + 0x100 + 0x80 * seat as u32;
            cpu.mem.wr32(row + 4 * u32::from(cmd), cell);
            costs[seat] = match rng.below(3) {
                0 => 30,
                1 => rng.below(0x100) as u8,
                _ => 20 + rng.below(30) as u8,
            };
            cpu.mem.wr8(cell + 0x74, costs[seat]);
        }
        let chips: Vec<u8> = (0..4 * 0x18).map(|_| rng.next() as u8).collect();
        cpu.mem.load(CHIP_RECORDS, &chips);
        cpu.mem
            .wr32(ACTOR + 0x400 * u32::from(acting) + 0x154, rng.below(0x100));
        assert_eq!(cpu.call(BUILD, &[case, 0]), Stop::Returned, "n {n}");

        // The port.
        let icons = retail_chip_icons(char_id, &equip);
        let mut chip_costs = [0u16; 4];
        for (seat, d) in SEAT_DIRS.iter().enumerate() {
            chip_costs[d.command_index()] = u16::from(costs[seat]);
        }
        let frame = |slide| ArtsInputFrame {
            buffer: &[],
            spent: &[],
            chip_costs,
            chip_icons: icons,
            pool: 0,
            pool_max: 0,
            plate_value: 0,
            list_page: None,
            list_pages: 0,
            phase: ArtsInputScreen::Entering,
            glides: &[],
            chip_slide: slide,
        };
        let mut want = chips.clone();
        for (seat, d) in SEAT_DIRS.iter().enumerate() {
            let r = &mut want[seat * 0x18..seat * 0x18 + 0x18];
            let (ax, _) = frame(Some((0, 16))).chip_anchor(*d);
            let (bx, _) = frame(None).chip_anchor(*d);
            let w = i32::from(costs[seat]) - COST_WIDTH_BIAS;
            r[2..4].copy_from_slice(&(ax as i16).to_le_bytes());
            r[6..8].copy_from_slice(&(w as i16).to_le_bytes());
            r[0xA..0xC].copy_from_slice(&(bx as i16).to_le_bytes());
            let icon = icons[d.command_index()];
            r[0xE] = icon;
            r[0xF] = icon;
        }
        let got = cpu.mem.read(CHIP_RECORDS, 4 * 0x18);
        assert!(
            got == want,
            "n {n} char {char_id} equip {equip:?} costs {costs:?}: {:?}",
            diff_offsets(&got, &want)
        );
        assert_eq!(cpu.mem.read(CTX + 0x14, 4), costs, "n {n}");
        swapped += u32::from(char_id == 2 && equip[2] != 0);
        plain += u32::from(equip[2] == 0 || equip[3] == 0);
        wide += u32::from(costs.iter().any(|&c| c != 30));
    }
    assert!(swapped > 1000 && plain > 1000 && wide > 1000);
}
