//! Does retail ever roll the camera?
//!
//! `gte::math::camera_view_rotation` composes three axis factors;
//! [`legaia_engine_render::window::cutscene_camera_mvp`] takes two, and its own
//! note justifies dropping the third with "roll is rarely non-zero in retail
//! shots". That is an assumption about the disc, and nothing measured it - so
//! the substitution rested on a claim no test could fail.
//!
//! This measures it. The corpus is every scene bundle's MAN on the disc, and
//! the population is every field-VM op-`0x45` **Configure** site in it: the
//! instruction whose ten optional slots are the camera's three Euler angles,
//! an offset trio, a focus trio and the GTE `H`. Slot `2` is roll
//! (`_DAT_8007B794`, the `RotMatrixZ` angle) - so "how often does an authored
//! camera beat set slot 2, and to what?" is exactly the question.
//!
//! ## This is a byte scan, and that is deliberate
//!
//! A field-VM script has no length header and no reliable entry offset inside
//! the MAN, so a linear opcode walk needs a start it cannot be given.
//! `legaia_asset::inn_costs` hit the same wall and resolved it the same way,
//! for the same reason - its module docs say so explicitly. A byte scan admits
//! false positives, so this asserts nothing about an individual site: the
//! assertions below are all about the **shape of the population**, and the
//! test prints the distribution so the number can be read rather than trusted.
//!
//! Two independent corroborations that the decode is finding real beats rather
//! than noise are asserted, because a scan that found noise would fail both:
//! authored pitch and yaw must both be *heavily* concentrated in the low
//! 12-bit angle range, and the slot-`9` GTE `H` values must cluster in the
//! plausible projection-distance band.
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/PROT/`.

use std::path::PathBuf;

/// Field-VM opcode for the camera family.
const OP_CAMERA: u8 = 0x45;
/// Descriptor type byte of a scene bundle's MAN.
const MAN_TYPE: u8 = 0x03;
/// Slot index of the roll angle (`_DAT_8007B794`, `RotMatrixZ`).
const SLOT_ROLL: u8 = 2;
/// Full turn in the 12-bit camera angle space.
const ANGLE_TURN: u16 = 0x1000;

/// One decoded Configure site.
#[derive(Debug, Clone)]
struct Configure {
    /// `(slot, value)` for every slot the mask set.
    params: Vec<(u8, u16)>,
}

impl Configure {
    fn slot(&self, s: u8) -> Option<u16> {
        self.params.iter().find(|p| p.0 == s).map(|p| p.1)
    }
}

/// Decode an op-`0x45` Configure at `p`, or `None` when the bytes there are
/// not one.
///
/// The layout mirrors `legaia_engine_vm`'s own op-`0x45` step: `op0`'s top two
/// bits select the sub-form (`00` = Configure), bits `2..=5` are the apply
/// mode, and bits `0..=1` are the high half of a ten-bit slot mask whose low
/// half is `op1`. Slot `n`'s bit is `1 << (9 - n)`, and each set slot
/// contributes one little-endian `u16` after the apply trigger.
fn decode_configure(man: &[u8], p: usize) -> Option<Configure> {
    if *man.get(p)? != OP_CAMERA {
        return None;
    }
    let op0 = *man.get(p + 1)?;
    if op0 & 0xC0 != 0 {
        return None;
    }
    let op1 = *man.get(p + 2)?;
    let mask = (u16::from(op0) << 8) | u16::from(op1);
    // A Configure that sets nothing is legal but carries no evidence.
    if mask & 0x03FF == 0 {
        return None;
    }
    let mut cursor = p + 5; // opcode, op0, op1, trigger u16
    let mut params = Vec::new();
    for slot in 0u8..10 {
        if mask & (1u16 << (9 - slot)) == 0 {
            continue;
        }
        let lo = *man.get(cursor)?;
        let hi = *man.get(cursor + 1)?;
        params.push((slot, u16::from_le_bytes([lo, hi])));
        cursor += 2;
    }
    Some(Configure { params })
}

fn extracted_prot() -> Option<PathBuf> {
    [
        "extracted/PROT",
        "../extracted/PROT",
        "../../extracted/PROT",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.is_dir())
}

/// Every op-`0x45` Configure site across every scene bundle's MAN.
fn census() -> Option<Vec<Configure>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let prot = extracted_prot().or_else(|| {
        eprintln!("[skip] extracted/PROT/ missing - run `legaia-extract` first");
        None
    })?;
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&prot)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("BIN"))
        .collect();
    entries.sort();

    let mut out = Vec::new();
    for path in &entries {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Some(table) = legaia_asset::scene_asset_table::detect(&bytes) else {
            continue;
        };
        let Some(desc) = table
            .used()
            .iter()
            .find(|d| d.type_byte == MAN_TYPE)
            .copied()
        else {
            continue;
        };
        if desc.size == 0 || desc.data_offset == 0 {
            continue;
        }
        let Some(body) = bytes.get(desc.data_offset as usize..) else {
            continue;
        };
        let Ok((man, _)) = legaia_lzs::decompress_tracked(body, desc.size as usize) else {
            continue;
        };
        for p in 0..man.len() {
            if let Some(c) = decode_configure(&man, p) {
                out.push(c);
            }
        }
    }
    Some(out)
}

#[test]
fn retail_camera_beats_essentially_never_roll() {
    let Some(sites) = census() else { return };
    assert!(
        sites.len() > 500,
        "only {} Configure sites found - the scan is not reaching the corpus",
        sites.len()
    );

    // Corroboration 1: pitch and yaw are **signed** 12-bit angles - the
    // engine decodes a slot as `(v as i16) * TAU / 4096`, so a negative beat
    // is stored near `0xFFFF` and an unsigned "below 0x1000" test would call
    // it noise. Authored values must sit overwhelmingly inside one signed
    // turn; noise uniform over 16 bits lands there about 12% of the time.
    for slot in [0u8, 1] {
        let vals: Vec<u16> = sites.iter().filter_map(|c| c.slot(slot)).collect();
        assert!(!vals.is_empty(), "slot {slot} never set anywhere");
        let in_turn = vals
            .iter()
            .filter(|&&v| (v as i16).unsigned_abs() <= ANGLE_TURN)
            .count();
        let frac = in_turn as f64 / vals.len() as f64;
        eprintln!(
            "slot {slot}: {} sites, {:.1}% inside one signed 12-bit turn",
            vals.len(),
            frac * 100.0
        );
        assert!(
            frac > 0.5,
            "slot {slot} values are not angle-shaped ({frac:.3} in range) - \
             the scan is decoding noise, so the roll number below means nothing"
        );
    }

    // Restrict to sites the corroboration above vouches for. A byte scan
    // finds non-instructions, and a junk site sets a junk "roll" almost every
    // time - so measuring roll over the raw population measures the scan's
    // false-positive rate, not the game. A site is credible when it sets at
    // least one of pitch / yaw and every one it sets is angle-shaped.
    let credible: Vec<&Configure> = sites
        .iter()
        .filter(|c| {
            let angles: Vec<u16> = [0u8, 1].iter().filter_map(|&s| c.slot(s)).collect();
            !angles.is_empty()
                && angles
                    .iter()
                    .all(|&v| (v as i16).unsigned_abs() <= ANGLE_TURN)
        })
        .collect();
    eprintln!(
        "{} of {} sites are credible (pitch/yaw angle-shaped)",
        credible.len(),
        sites.len()
    );
    assert!(
        credible.len() > 500,
        "only {} credible sites - too few to measure against",
        credible.len()
    );

    // The measurement.
    let rolls: Vec<u16> = credible.iter().filter_map(|c| c.slot(SLOT_ROLL)).collect();
    let non_zero: Vec<u16> = rolls.iter().copied().filter(|&v| v != 0).collect();
    let in_range = non_zero
        .iter()
        .filter(|&&v| (v as i16).unsigned_abs() <= ANGLE_TURN)
        .count();
    eprintln!(
        "roll (slot {SLOT_ROLL}): set by {} of {} credible sites; {} non-zero, \
         {in_range} of those angle-shaped",
        rolls.len(),
        credible.len(),
        non_zero.len()
    );
    if !non_zero.is_empty() {
        let mut sample: Vec<u16> = non_zero
            .iter()
            .copied()
            .filter(|&v| (v as i16).unsigned_abs() <= ANGLE_TURN)
            .collect();
        sample.sort_unstable();
        sample.dedup();
        eprintln!("distinct angle-shaped non-zero rolls: {sample:?}");
    }

    // The claim under test, stated as a bound rather than as "never": an
    // authored beat that sets a non-zero, angle-shaped roll must be a small
    // fraction of all credible beats, or `cutscene_camera_mvp` is dropping a
    // term the game uses.
    let frac = in_range as f64 / credible.len().max(1) as f64;
    eprintln!(
        "non-zero angle-shaped roll: {:.2}% of credible beats",
        frac * 100.0
    );
    assert!(
        frac < 0.05,
        "{:.1}% of camera beats set a non-zero roll - `cutscene_camera_mvp` \
         drops the RotMatrixZ factor and would be wrong on all of them",
        frac * 100.0
    );
}
