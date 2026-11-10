//! The developer menu's per-row value adjuster `FUN_801E9F64` (PROT 0897)
//! against `engine-dev-menu`'s two ported arms: the 12-bit ring on
//! `_DAT_8007B6D0` (row 7) and the `[1, 255]` clamp on `_DAT_801F2E8C`
//! (row 10), stepped off the newly-pressed pad word `_DAT_8007BB84`.
//!
//! Rows whose jump-table entry is the shared tail (`0x801EA7AC`) are run as
//! well: they leave both words alone.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_dev_menu::dev_menu_adjust::{clamp1_255_step, wrap12_step};
use psx_retail_exec::Stop;

const OVERLAY_BASE: u32 = 0x801C_E818;
const ADJUST: u32 = 0x801E_9F64;
const PAD_PRESSED: u32 = 0x8007_BB84;
const RING: u32 = 0x8007_B6D0;
const RATE: u32 = 0x801F_2E8C;
const ACTOR: u32 = 0x8012_0000;

#[test]
fn the_ring_and_clamp_rows_step_as_retail_steps() {
    differential!(prot(897, ADJUST));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let mut rng = Rng(0x4144_4A55);
    let (mut both, mut wrapped, mut clamped, mut idle) = (0, 0, 0, 0);
    for run in 0..12_000 {
        // The two ported rows, and the rows that go straight to the tail.
        let row = [7u16, 7, 10, 10, 4, 5, 8, 9, 11, 15, 16, 2, 30][rng.below(13) as usize];
        cpu.mem.wr16(ACTOR + 0x9E, row);
        let pad = match rng.below(5) {
            0 => 0,
            1 => 0x2000,
            2 => 0x8000,
            3 => 0xA000,
            _ => rng.next() & 0xFFFF,
        };
        cpu.mem.wr32(PAD_PRESSED, pad);
        let ring = match rng.below(4) {
            0 => [0u32, 0xFFF, 0x1234, 0xF000][rng.below(4) as usize],
            _ => rng.below(0x1000),
        };
        let rate = match rng.below(4) {
            0 => [0i32, 1, 255, 256, -3, 500][rng.below(6) as usize],
            _ => 1 + rng.below(255) as i32,
        };
        cpu.mem.wr32(RING, ring);
        cpu.mem.wr32(RATE, rate as u32);
        assert_eq!(cpu.call(ADJUST, &[ACTOR]), Stop::Returned, "run {run}");
        let what = format!("run {run}: row {row} pad {pad:#x} ring {ring:#x} rate {rate}");
        let (want_ring, want_rate) = match row {
            7 => (u32::from(wrap12_step(ring as u16, pad)), rate),
            10 => (ring, clamp1_255_step(rate, pad)),
            _ => (ring, rate),
        };
        assert_eq!(cpu.mem.rd32(RING), want_ring, "{what}");
        assert_eq!(cpu.mem.rd32(RATE) as i32, want_rate, "{what}");
        both += u32::from(pad & 0xA000 == 0xA000 && (row == 7 || row == 10));
        wrapped +=
            u32::from(row == 7 && want_ring != ring && (want_ring == 0 || want_ring == 0xFFF));
        clamped += u32::from(row == 10 && (want_rate == 1 || want_rate == 255));
        idle += u32::from(row != 7 && row != 10);
    }
    assert!(
        both > 300 && wrapped > 20 && clamped > 200 && idle > 3000,
        "{both} {wrapped} {clamped} {idle}"
    );
}

const ACTION: u32 = 0x801E_A9B0;

/// The row-action dispatcher `FUN_801EA9B0` against `dev_menu_action` and
/// `dev_menu_action_phase`: the bound, the phase every path stores to
/// `ctx[+0x54]`, and the `1` every path returns. The arms' own pokes are let
/// run, their calls out of the overlay answered with zero.
#[test]
fn the_row_action_parks_and_returns_as_retail_does() {
    differential!(prot(897, ACTION));
    use legaia_engine_dev_menu::dev_menu_action::{dev_menu_action, dev_menu_action_phase};
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let image = d.read_entry(897).expect("PROT 0897");
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    /// Row 1's gate word.
    const ROW_ONE_GATE: u32 = 0x8007_B868;
    let mut rng = Rng(0x4143_5449);
    let (mut parked, mut ran, mut gated) = (0, 0, 0);
    for run in 0..2_000 {
        // The arms poke the overlay's own data; each run starts clean.
        cpu.mem.load(OVERLAY_BASE, &image);
        let row = match rng.below(4) {
            0 => [-1i16, -2, 0x18, 0x19, 0x400, i16::MIN][rng.below(6) as usize],
            1 => 1,
            _ => rng.below(0x18) as i16,
        };
        let phase = 0x40 + rng.below(8) as i16;
        let gate = rng.below(2) * (1 + rng.below(5));
        cpu.mem.wr32(ROW_ONE_GATE, gate);
        cpu.mem.wr16(ACTOR + 0x9E, row as u16);
        cpu.mem.wr16(ACTOR + 0x54, phase as u16);
        let stop = cpu.call(ACTION, &[ACTOR]);
        assert_eq!(stop, Stop::Returned, "run {run}: row {row}");
        let (park, result) = dev_menu_action(row);
        assert_eq!(
            cpu.regs.r[psx_retail_exec::V0] as i32,
            result,
            "run {run}: row {row}"
        );
        let left = dev_menu_action_phase(row, gate != 0);
        assert_eq!(
            cpu.mem.rd16(ACTOR + 0x54) as i16,
            left,
            "run {run}: row {row} gate {gate}: the phase word"
        );
        if let Some(p) = park {
            assert_eq!(p, left, "run {run}: the park phase is the one stored");
        }
        assert_eq!(
            park.is_some(),
            !(0..0x18).contains(&row),
            "run {run}: row {row}"
        );
        parked += u32::from(park.is_some());
        ran += u32::from(park.is_none());
        gated += u32::from(row == 1 && gate == 0);
    }
    assert!(
        parked > 300 && ran > 1000 && gated > 100,
        "{parked} {ran} {gated}"
    );
}
