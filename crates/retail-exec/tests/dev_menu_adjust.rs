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
