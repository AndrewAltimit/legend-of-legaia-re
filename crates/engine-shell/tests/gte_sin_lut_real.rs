//! Pin the clean-room trig reproduction against the retail GTE sin/cos LUT.
//!
//! `FUN_8004638c` (RotMatrixZ) and its X/Y siblings index a q3.12 sine LUT
//! pair inside `SCUS_942.54`: sine at VA `0x80070A2C + 2*angle`, "cosine" at
//! `0x8007122C + 2*angle` (the same table read 0x400 entries / 90 degrees
//! ahead - the combined span is 5120 entries, 1.25 turns). The engine's
//! `legaia_engine_render::billboard::{psx_sin, psx_cos}` compute the values
//! trigonometrically instead of shipping the table; this oracle compares all
//! 4096 angles of BOTH access patterns against the user's own executable.
//!
//! Skips and passes when `extracted/SCUS_942.54` isn't present - same gating
//! pattern as the other disc-dependent tests.

use legaia_engine_render::billboard::{psx_cos, psx_sin};
use std::path::PathBuf;

const SIN_VA: u32 = 0x8007_0A2C;
const COS_VA: u32 = 0x8007_122C;

fn scus_path() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("SCUS_942.54");
    p.is_file().then_some(p)
}

/// PS-X EXE VA -> file offset (header is 0x800 bytes, text base at +0x18).
fn exe_off(scus: &[u8], va: u32) -> Option<usize> {
    if scus.len() < 0x800 || &scus[0..8] != b"PS-X EXE" {
        return None;
    }
    let t_addr = u32::from_le_bytes(scus[0x18..0x1C].try_into().ok()?);
    va.checked_sub(t_addr)
        .map(|d| d as usize + 0x800)
        .filter(|&o| o + 2 <= scus.len())
}

fn lut_i16(scus: &[u8], base_va: u32, angle: u16) -> i16 {
    let off = exe_off(scus, base_va + 2 * angle as u32).expect("LUT offset in image");
    i16::from_le_bytes([scus[off], scus[off + 1]])
}

#[test]
fn psx_sin_cos_match_the_retail_lut_or_skip() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let scus = std::fs::read(&path).expect("read SCUS");

    // Identity anchors disambiguate which table is which: sin[0] = 0,
    // cos[0] = 4096.
    assert_eq!(lut_i16(&scus, SIN_VA, 0), 0, "sin table base");
    assert_eq!(lut_i16(&scus, COS_VA, 0), 4096, "cos table base");

    let mut max_delta = 0i32;
    let mut mismatches = 0usize;
    for angle in 0..0x1000u16 {
        let want_sin = lut_i16(&scus, SIN_VA, angle) as i32;
        let want_cos = lut_i16(&scus, COS_VA, angle) as i32;
        let got_sin = psx_sin(angle);
        let got_cos = psx_cos(angle);
        for (got, want, label) in [(got_sin, want_sin, "sin"), (got_cos, want_cos, "cos")] {
            let delta = (got - want).abs();
            if delta != 0 {
                if mismatches < 8 {
                    eprintln!("{label}[{angle:#05x}]: engine {got} vs retail {want}");
                }
                mismatches += 1;
                max_delta = max_delta.max(delta);
            }
        }
    }
    assert_eq!(
        mismatches, 0,
        "trig reproduction diverges from the retail LUT (max |delta| = {max_delta})"
    );
}
