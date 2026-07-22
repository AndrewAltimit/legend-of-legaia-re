//! Camera-relative effect-actor spawn: the parameter-block normalizer.
//!
//! PORT: FUN_80021248
//!
//! `FUN_80021248(record)` is the SCUS spawner for the camera-anchored
//! effect-actor family (spawn descriptor `DAT_8007071C`, allocated onto the
//! `_DAT_8007C34C` actor list; the battle overlay is its heaviest caller).
//! It copies the caller's 20-halfword parameter record to `actor+0x80`,
//! seats the actor (`+0x14/16/18` from record halfwords `13/15/17`), links
//! the raw record pointer at `+0x4C`, then **normalizes the copy against
//! the live camera** so the actor's motion axes read as signed offsets
//! from the current view. Ported from the disassembly
//! (`see ghidra/scripts/funcs/80021248.txt`; loop structure corroborated
//! by the static-recomp rendering of `func_80021248`).
//!
//! The record is ten `(magnitude, reference)` halfword pairs:
//!
//! | pairs | reference compared against | rule |
//! |---|---|---|
//! | 0..3 | camera angle triple `DAT_8007B790/92/94` (pitch/yaw/roll) | `d = (ref & 0xFFF) - (cam & 0xFFF)`; the folded `min(\|d\|, 0x1000-\|d\|)` lands in the actor rotation triple (`+0x24/26/28`); magnitude sign = negative iff **exactly one** of (`\|d\| > 0x800`, `d < 0`) |
//! | 3..6 | camera offset words `DAT_800840B8/BC/C0` (i32) | magnitude = `-\|m\|` when `ref < cam`, else `\|m\|` |
//! | 6..9 | negated camera focus `DAT_80089118/1C/20` (i32) | same |
//! | 9 | GTE `H` projection `DAT_8007B6F4` (i16) | same |
//!
//! The spawner also runs a **supersede handshake** on the scratch system
//! word `_DAT_1F800394`: bit `0x80` is cleared; if bit `0x100` was set the
//! *previous* actor of this family (tracked at `gp+0x750`) gets its flags
//! (`+0x10`) ORed with `8`; then bit `0x100` is set and `gp+0x750` points
//! at the new actor - see [`SpawnHandshake`].
//!
//! ## Why no host calls this
//!
//! The camera half is wired: `legaia_engine_core::camera::RetailCamGlobals`
//! carries exactly the ten axes [`CameraSnapshot`] wants and converts to one
//! via `RetailCamGlobals::camera_snapshot`, so a caller can always hand the
//! normalizer the live camera.
//!
//! What is missing is the **record**. This normalizer takes a 20-halfword
//! spawn parameter block, and the engine has nowhere to get one: the only
//! effect-spawn path in `legaia_engine_core` is `World::try_spawn_effect`,
//! which is the *other* family - the PROT 0873 `efect.dat` catalog spawned
//! by `(ui_id, world_pos, angle)` through `FUN_801D8DE8` /
//! `FUN_801DFDF8`. The camera-anchored family this normalizer belongs to
//! (spawn descriptor `DAT_8007071C`, actor list `_DAT_8007C34C`) has no
//! engine counterpart at all, so there is no record to normalize and no
//! honest way to stand a host up - a synthetic record would only exercise
//! the arithmetic the unit tests already cover. Porting that family's
//! allocator (`FUN_80020DE0` and its battle-overlay callers) is the
//! prerequisite, not more plumbing here.

/// The camera state the normalizer reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CameraSnapshot {
    /// `DAT_8007B790/92/94` - pitch / yaw / roll, PSX angle units.
    pub angles: [u16; 3],
    /// `DAT_800840B8/BC/C0` - the op-`0x45` shake/offset words.
    pub offsets: [i32; 3],
    /// `DAT_80089118/1C/20` - the negated camera focus.
    pub focus: [i32; 3],
    /// `DAT_8007B6F4` - GTE `H` projection (zoom).
    pub gte_h: i16,
}

/// Result of normalizing one 20-halfword record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalizedParams {
    /// The normalized copy retail leaves at `actor+0x80`.
    pub params: [i16; 20],
    /// The actor rotation triple (`+0x24/26/28`): folded angle deltas.
    pub rotation: [i16; 3],
}

/// Normalize a camera-relative parameter record - the `0x80021304..`
/// four-loop body of `FUN_80021248`.
pub fn normalize_camera_relative_params(
    record: &[i16; 20],
    cam: &CameraSnapshot,
) -> NormalizedParams {
    let mut p = *record;
    let mut rot = [0i16; 3];

    // Pairs 0..3: angle-relative axes.
    for i in 0..3 {
        let m = p[i * 2];
        let mut mag = if m < 0 { -m } else { m };
        let d = i32::from(p[i * 2 + 1] as u16 & 0xFFF) - i32::from(cam.angles[i] & 0xFFF);
        let mut a = d.abs() as i16;
        if a > 0x800 {
            a = 0x1000 - a;
            mag = -mag;
        }
        if d < 0 {
            mag = -mag;
        }
        rot[i] = a;
        p[i * 2] = mag;
    }
    // Pairs 3..6 vs camera offsets, 6..9 vs focus, 9 vs GTE H: sign from
    // a signed reference-below-camera compare.
    let refs: [i32; 7] = [
        cam.offsets[0],
        cam.offsets[1],
        cam.offsets[2],
        cam.focus[0],
        cam.focus[1],
        cam.focus[2],
        i32::from(cam.gte_h),
    ];
    for (i, &r) in refs.iter().enumerate() {
        let idx = (3 + i) * 2;
        let m = p[idx];
        let mag = if m < 0 { -m } else { m };
        p[idx] = if i32::from(p[idx + 1]) < r { -mag } else { mag };
    }
    NormalizedParams {
        params: p,
        rotation: rot,
    }
}

/// The supersede handshake on the scratch word `_DAT_1F800394` +
/// previous-actor pointer (`gp+0x750`). Returns whether the previous
/// actor of the family must be flagged (`flags |= 8`).
///
/// Retail: `scratch &= !0x80`; `flag_prev = scratch had 0x100`;
/// `scratch |= 0x100`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpawnHandshake {
    /// The updated scratch word.
    pub scratch: u32,
    /// True = OR `8` into the previous family actor's `+0x10` flags.
    pub flag_previous: bool,
}

/// Run the handshake against the current scratch word.
pub fn spawn_handshake(scratch: u32) -> SpawnHandshake {
    let cleared = scratch & !0x80;
    SpawnHandshake {
        scratch: cleared | 0x100,
        flag_previous: scratch & 0x100 != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(pairs: [(i16, i16); 10]) -> [i16; 20] {
        let mut r = [0i16; 20];
        for (i, (m, a)) in pairs.into_iter().enumerate() {
            r[i * 2] = m;
            r[i * 2 + 1] = a;
        }
        r
    }

    #[test]
    fn angle_axis_folds_and_signs() {
        let cam = CameraSnapshot {
            angles: [0x100, 0xF00, 0x800],
            ..Default::default()
        };
        // Axis 0: ref 0x200, cam 0x100 -> d = +0x100, no fold: mag stays +.
        // Axis 1: ref 0x100, cam 0xF00 -> d = -0xE00, |d| > 0x800: fold to
        //   0x200, one negate from the fold + one from d<0 = positive again.
        // Axis 2: ref 0x700, cam 0x800 -> d = -0x100: mag negates.
        let r = rec([
            (5, 0x200),
            (7, 0x100),
            (9, 0x700),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
        ]);
        let n = normalize_camera_relative_params(&r, &cam);
        assert_eq!(n.rotation, [0x100, 0x200, 0x100]);
        assert_eq!(n.params[0], 5, "no fold, positive delta");
        assert_eq!(n.params[2], 7, "fold + negative delta double-negates");
        assert_eq!(n.params[4], -9, "negative delta alone negates");
    }

    #[test]
    fn angle_magnitude_is_abs_first() {
        // A negative authored magnitude is abs'd before the sign rules.
        let cam = CameraSnapshot::default();
        let r = rec([
            (-5, 0x10),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
        ]);
        let n = normalize_camera_relative_params(&r, &cam);
        assert_eq!(n.params[0], 5, "abs, then positive-delta keeps +");
    }

    #[test]
    fn linear_axes_sign_from_reference_compare() {
        let cam = CameraSnapshot {
            offsets: [100, -50, 0],
            focus: [10, 20, 30],
            gte_h: 0xA0,
            ..Default::default()
        };
        let r = rec([
            (0, 0),
            (0, 0),
            (0, 0),
            (-4, 99),  // ref 99 < cam 100 -> -|m| = -4
            (6, -50),  // ref == cam -> keeps +
            (8, -1),   // ref -1 < cam 0 -> -8
            (3, 9),    // focus: 9 < 10 -> -3
            (3, 20),   // 20 == 20 -> +3
            (3, 31),   // 31 > 30 -> +3
            (2, 0x9F), // 0x9F < 0xA0 -> -2
        ]);
        let n = normalize_camera_relative_params(&r, &cam);
        assert_eq!(n.params[6], -4);
        assert_eq!(n.params[8], 6);
        assert_eq!(n.params[10], -8);
        assert_eq!(n.params[12], -3);
        assert_eq!(n.params[14], 3);
        assert_eq!(n.params[16], 3);
        assert_eq!(n.params[18], -2);
    }

    #[test]
    fn handshake_marks_previous_only_when_latched() {
        let h = spawn_handshake(0);
        assert!(!h.flag_previous);
        assert_eq!(h.scratch, 0x100);
        // Second spawn: the latch is set, previous actor gets flagged;
        // bit 0x80 is cleared on the way.
        let h = spawn_handshake(h.scratch | 0x80);
        assert!(h.flag_previous);
        assert_eq!(h.scratch, 0x100);
    }
}
