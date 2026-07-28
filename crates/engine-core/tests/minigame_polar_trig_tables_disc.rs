//! Disc-gated: the two quadrature tables
//! [`legaia_engine_core::minigame_floor::polar_offset`] indexes are **static
//! `SCUS_942.54` rodata**, not overlay-installed runtime data.
//!
//! This is the oracle behind that port's corrected `NOT WIRED:` reason. The
//! reason it replaced said "nothing in the engine decodes either table - so
//! there is no table pair to index and no caller can supply one"; the tables
//! are in fact a fixed pair of SCUS addresses that `FUN_80026be0` publishes
//! into `_DAT_8007B81C` / `_DAT_8007B7F8` once at boot, so a caller can always
//! supply one. Asserting that here keeps the corrected reason from rotting
//! back.
//!
//! Three facts, each read straight off the disc:
//!
//! 1. `_DAT_8007B81C` -> `DAT_80070A2C` is a 4096-entry sine table in 4.12
//!    fixed point, and `_DAT_8007B7F8` -> `DAT_8007122C` is the cosine one, so
//!    `polar_offset`'s `table_a` is sine and `table_b` cosine.
//! 2. The two are `0x800` bytes apart - one quarter turn - so they overlap:
//!    the pair is *one* table read at two phases.
//! 3. Entries are `trunc(0x1000 * sin)`, truncating toward zero. The analytic
//!    reproduction in `legaia_asset::minigame_slot_scene` rounds instead, which
//!    differs on about half the table by one LSB.
//!
//! Skips and passes without `LEGAIA_DISC_BIN`.

use legaia_asset::minigame_slot_scene::{ANGLE_FULL, COS_TABLE_VA, SIN_TABLE_VA};
use legaia_engine_core::Vfs;
use legaia_engine_core::minigame_floor::{POLAR_SHIFT, POLAR_TABLE_LEN, polar_offset};
use std::path::PathBuf;

/// `SCUS_942.54` text-segment load address; the PS-EXE header is `0x800`
/// bytes, so `file_offset = va - T_ADDR + 0x800`.
const T_ADDR: u32 = 0x8001_0000;
const EXE_HEADER: usize = 0x800;

/// Entries the retail tables hold, in `i16`s.
const ENTRIES: usize = 4096;

fn read_table(scus: &[u8], va: u32) -> Vec<i16> {
    let off = (va - T_ADDR) as usize + EXE_HEADER;
    scus[off..off + ENTRIES * 2]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

fn scus() -> Option<Vec<u8>> {
    let path = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    if !path.is_file() {
        eprintln!("[skip] LEGAIA_DISC_BIN is not a file");
        return None;
    }
    Some(
        legaia_engine_core::DiscVfs::open(&path)
            .expect("open disc")
            .read("SCUS_942.54")
            .expect("SCUS_942.54 present"),
    )
}

#[test]
fn the_polar_helpers_tables_are_static_scus_rodata() {
    let Some(scus) = scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let sin = read_table(&scus, SIN_TABLE_VA);
    let cos = read_table(&scus, COS_TABLE_VA);
    assert_eq!(sin.len(), POLAR_TABLE_LEN, "one turn of 12-bit angles");
    assert_eq!(ANGLE_FULL as usize, POLAR_TABLE_LEN);

    // Fact 1: the amplitude is 4.12 fixed point and the phases are the ones
    // that make `table_a` sine and `table_b` cosine.
    assert_eq!(sin[0], 0, "sin(0)");
    assert_eq!(sin[ENTRIES / 4], 0x1000, "sin(quarter turn)");
    assert_eq!(cos[0], 0x1000, "cos(0)");
    assert_eq!(cos[ENTRIES / 4], 0, "cos(quarter turn)");

    // Fact 2: the cosine pointer is the sine table advanced a quarter turn,
    // so the "two tables" are one 5120-entry run.
    assert_eq!(COS_TABLE_VA - SIN_TABLE_VA, (ENTRIES as u32 / 4) * 2);
    for i in 0..ENTRIES {
        assert_eq!(
            cos[i],
            sin[(i + ENTRIES / 4) % ENTRIES],
            "cos[{i}] is sin one quarter turn ahead"
        );
    }

    // Fact 3: truncation toward zero, not rounding.
    let mut round_mismatches = 0usize;
    for (i, &v) in sin.iter().enumerate() {
        let exact = 4096.0 * (i as f64 * std::f64::consts::TAU / ENTRIES as f64).sin();
        assert_eq!(v as i32, exact as i32, "sin[{i}] truncates toward zero");
        if v as i32 != exact.round() as i32 {
            round_mismatches += 1;
        }
    }
    assert!(
        round_mismatches > ENTRIES / 4,
        "rounding is a materially different table ({round_mismatches} entries differ), \
         so an analytic stand-in has to truncate"
    );
}

#[test]
fn the_real_tables_drive_the_ported_polar_arithmetic() {
    let Some(scus) = scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let sin = read_table(&scus, SIN_TABLE_VA);
    let cos = read_table(&scus, COS_TABLE_VA);

    // The `FUN_801CF3BC` case-`0xD` camera dolly: facing at the middle of the
    // fishing arc, radius 0x14, one frame of delta.
    let facing = 0x800u32;
    let (a, b) = polar_offset(facing, 0x14, 1, &sin, &cos).expect("tables cover the angle");
    let fold = |t: i32| (t * 0x14) >> POLAR_SHIFT;
    assert_eq!(a, fold(sin[facing as usize] as i32));
    assert_eq!(b, fold(cos[facing as usize] as i32));

    // Half a turn from there the sine has flipped sign and the `sra` rounds
    // toward minus infinity, which is the whole reason the port keeps the
    // shift unbiased.
    let (a2, _) = polar_offset(facing + 0x800, 0x14, 1, &sin, &cos).expect("still in range");
    assert!(a2 <= 0, "the opposite phase is non-positive");

    // The mask is a full turn, so an unmasked angle wraps rather than
    // panicking - retail's `andi a0,a0,0xfff`.
    assert_eq!(
        polar_offset(facing + ANGLE_FULL as u32, 0x14, 1, &sin, &cos),
        Some((a, b))
    );
}
