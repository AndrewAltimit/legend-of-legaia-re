//! The battle audio **duck** - the live audio level `_DAT_8007B910` both play
//! hosts ramp and re-apply to the BGM sequencer.
//!
//! The action SM sets a target (`ref * 75 / 100` under a summon / capture,
//! `ref` again in the Done band's `0x51` arm) and the level steps one unit per
//! vsync toward it; each step re-applies `master_vol * level / ref`, the
//! `SsSeqSetVol` re-apply that halves the cell into the 0..127 domain the same
//! way `master_vol` already is.
//!
//! One kernel for both hosts, because the two copies disagreed by omission in
//! the same place: each re-applied the volume only while the level was
//! **moving**, so a track attached after the ramp had settled - a battle theme
//! change, a sting under a held summon duck - kept the undiminished master
//! volume until the next target change. Retail applies the live level to a
//! source as it attaches it (`FUN_80026478` hands `_DAT_8007B910 >> 1` to the
//! level primitive `FUN_8002657C`), so a held duck is re-applied every frame
//! here ([`duck_apply`]) rather than only on a step.
//!
//! REF: FUN_80026478, FUN_8002657C, FUN_8001FFA4

/// `_DAT_8007B910`'s reference value (`0xD7`, seeded by the cold reset
/// `FUN_8001FFA4`): the un-ducked level the `0x51` arm ramps back to.
pub const DUCK_LEVEL_REF: u8 = 0xD7;

/// The duck target for `pct` percent of the reference level, retail's floor
/// (`0xD7 * 75 / 100 = 161`). Percentages above 100 clamp.
pub fn duck_target_for_pct(pct: u8) -> u8 {
    let pct = u32::from(pct.min(100));
    (u32::from(DUCK_LEVEL_REF) * pct / 100) as u8
}

/// One vsync of the ramp: step `level` one unit toward `target`. Returns
/// whether it moved.
pub fn step_duck(level: &mut u8, target: u8) -> bool {
    if *level == target {
        return false;
    }
    *level = if *level < target {
        *level + 1
    } else {
        *level - 1
    };
    true
}

/// The sequencer master volume the level stands for: `master * level / ref`,
/// floored (a "75%" duck hands a 100 master `74`).
pub fn ducked_master_vol(master: u8, level: u8) -> u8 {
    (u32::from(master) * u32::from(level) / u32::from(DUCK_LEVEL_REF)).min(127) as u8
}

/// The volume a host hands the live sequencer this frame, or `None` when
/// there is nothing to do: a step always re-applies, and so does a level that
/// **rests** below the reference, so a track attached under a settled duck
/// is brought down on its first frame instead of playing at full volume.
pub fn duck_apply(moved: bool, master: u8, level: u8) -> Option<u8> {
    (moved || level != DUCK_LEVEL_REF).then(|| ducked_master_vol(master, level))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_settled_duck_keeps_re_applying_and_full_level_does_not() {
        let target = duck_target_for_pct(75);
        let mut level = DUCK_LEVEL_REF;
        let mut steps = 0;
        while step_duck(&mut level, target) {
            steps += 1;
        }
        assert_eq!(steps, u32::from(DUCK_LEVEL_REF - target));
        // Settled: no step, but the held level still owes the sequencer its
        // volume - a track attached now starts at 100 and must come down.
        assert!(!step_duck(&mut level, target));
        assert_eq!(duck_apply(false, 100, level), Some(74));
        // At the reference with no step there is nothing to re-apply.
        assert_eq!(duck_apply(false, 100, DUCK_LEVEL_REF), None);
        assert_eq!(duck_apply(true, 100, DUCK_LEVEL_REF), Some(100));
    }

    #[test]
    fn the_target_is_retails_floor_and_clamps() {
        assert_eq!(duck_target_for_pct(75), 161);
        assert_eq!(duck_target_for_pct(250), DUCK_LEVEL_REF);
        assert_eq!(ducked_master_vol(100, 161), 74);
    }
}
