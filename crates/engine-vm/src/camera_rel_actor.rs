//! Camera-relative effect-actor spawn: the parameter-block normalizer.
//!
//! (The `PORT:` tag for `FUN_80021248` sits on
//! [`normalize_camera_relative_params`], the item that implements it. A
//! module-level tag would make the liveness analysis attribute every
//! `pub` item in the file to that address.)
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
//! ## Where the record comes from
//!
//! The 20-halfword record is **not** script data and has no field-scene
//! source. Its one producer is `FUN_801D829C`, a battle-overlay routine that
//! builds nine `(step, endpoint)` pairs at battle-context `+0x118C` -
//! `step = ceil(|current - target| / duration)` over those same nine globals
//! in the same order - and then tails straight into `FUN_80021248` on that
//! buffer (`jal 0x80021248` at `0x801D84A8`, argument
//! `*(0x8007BD24) + 0x118C`). That routine is already ported as
//! [`crate::battle_camera::build_camera_angle_tween`]; [`glide_spawn_record`]
//! is the lay-out step between the two.
//!
//! Its callers are a **battle-only** population: 525 `jal` sites over 67
//! images, every one PROT 0898, a slot-B cast module (`0903..=0966`) or one
//! SCUS site at `0x80056360`; the normalizer itself has three `jal` sites in
//! two images - PROT 0898 (`0x801D84A8`) and PROT 0976, the Baka Fighter duel.
//! No field or world-map image calls either, so "a from-boot scene walk spawns a
//! camera-relative glide" is false by construction - the family is reached
//! from a battle or a duel, never from a scene load.
//!
//! ## NOT WIRED
//!
//! The camera half is available
//! (`legaia_engine_core::camera::RetailCamGlobals::camera_snapshot` is the ten
//! axes [`CameraSnapshot`] wants) and the record can now be laid out by
//! [`glide_spawn_record`]. What is still missing is the **actor**: the
//! camera-anchored family (spawn descriptor `DAT_8007071C`, actor list
//! `_DAT_8007C34C`) has no engine counterpart, so nothing holds a spawned
//! glide between frames. `engine-shell`'s `window/battle_cam.rs` drives its
//! own `Glide` off `build_camera_angle_tween`'s slots directly instead, which
//! is why the chain stops at the actor rather than at a missing kernel.

/// Halfword pairs in a spawn record - nine tweened globals plus the GTE `H`
/// channel, `20` halfwords in all.
pub const GLIDE_RECORD_PAIRS: usize = 10;

/// Lay a built tween table out as the 20-halfword record
/// [`normalize_camera_relative_params`] consumes.
///
/// `slots` are [`crate::battle_camera::build_camera_angle_tween`]'s nine
/// `(step, endpoint)` pairs in its own order - rotation, shake, focus - which
/// is exactly the order the normalizer's reference list walks
/// (`0x8007B790/92/94`, `0x800840B8/BC/C0`, `0x80089118/1C/20`). `gte_h` is
/// the tenth pair.
///
/// Retail's builder writes only the nine (`sltiu v0,a3,0x12` over a pair loop
/// advancing `a3` by `2`), while `FUN_80021248` copies twenty halfwords, so
/// the tenth pair is whatever the battle context already holds at `+0x11B0`;
/// **which site writes it is not pinned**. A `(0, 0)` pair parks the channel -
/// the glide tick counts a zero-step channel as arrived every frame - which is
/// the safe default for a caller with no zoom to tween.
///
/// PORT: FUN_801D829C
/// REPLACED-BY: `engine-shell`'s `window/battle_cam.rs` `Glide`, which holds
/// the same nine `(step, endpoint)` channels between frames directly off
/// `crate::battle_camera::build_camera_angle_tween`. No host is owed a call.
///
/// This layout is **transport**, not behaviour: retail needs the twenty
/// halfwords because its glide lives on a pool actor allocated from
/// `DAT_8007071C`, so the builder's nine pairs have to be serialised into
/// `actor+0x80` for the tick to find them a frame later. The port's glide is
/// a struct the camera owns, so the slots travel as themselves and the record
/// has no reader - and it would still have none if the actor were added,
/// because the actor would hold the same slots.
pub fn glide_spawn_record(
    slots: &[crate::battle_camera::TweenSlot; crate::battle_camera::TWEEN_SLOTS],
    gte_h: crate::battle_camera::TweenSlot,
) -> [i16; GLIDE_RECORD_PAIRS * 2] {
    let mut out = [0i16; GLIDE_RECORD_PAIRS * 2];
    for (i, slot) in slots.iter().chain(std::iter::once(&gte_h)).enumerate() {
        out[i * 2] = slot.step as i16;
        out[i * 2 + 1] = slot.target as i16;
    }
    out
}

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
///
/// PORT: FUN_80021248
///
/// NOT WIRED: the camera half is available
/// (`legaia_engine_core::camera::RetailCamGlobals::camera_snapshot`) and the
/// record's producer is ported (`FUN_801D829C` ->
/// [`crate::battle_camera::build_camera_angle_tween`] -> [`glide_spawn_record`]),
/// but this normalizer's actor family - spawn descriptor `DAT_8007071C`,
/// actor list `_DAT_8007C34C` - has no engine counterpart, so no host holds
/// the spawned glide. See the module docs.
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

    /// The producer's nine slots land in the record in the order the
    /// normalizer's reference list walks, and the tenth pair is the caller's.
    #[test]
    fn the_builder_lays_out_a_normalizable_record() {
        use crate::battle_camera::{
            CameraAngles, TWEEN_SLOTS, TweenSlot, build_camera_angle_tween,
        };
        let mut cur = CameraAngles {
            rotation: [0x100, 0x200, 0x300],
            shake: [10, 20, 30],
            focus: [-40, -50, -60],
        };
        let mut tgt = CameraAngles {
            rotation: [0x180, 0x200, 0x280],
            shake: [30, 20, 10],
            focus: [-20, -50, -80],
        };
        let slots = build_camera_angle_tween(&mut cur, &mut tgt, 8);
        assert_eq!(slots.len(), TWEEN_SLOTS);
        let record = glide_spawn_record(&slots, TweenSlot::default());
        // Every pair is (step, endpoint) in the builder's own order.
        for (i, slot) in slots.iter().enumerate() {
            assert_eq!(record[i * 2], slot.step as i16, "pair {i} step");
            assert_eq!(record[i * 2 + 1], slot.target as i16, "pair {i} endpoint");
        }
        // The tenth pair is the parked GTE-H channel.
        assert_eq!(record[18], 0);
        assert_eq!(record[19], 0);
        // And the normalizer accepts it: a parked channel keeps its zero
        // magnitude, so nothing is invented by the hand-off.
        let cam = CameraSnapshot {
            angles: [0x100, 0x200, 0x300],
            offsets: [10, 20, 30],
            focus: [-40, -50, -60],
            gte_h: 0,
        };
        let out = normalize_camera_relative_params(&record, &cam);
        assert_eq!(out.params[18], 0);
        // The middle rotation channel had no distance to cover, so its step is
        // zero on both sides of the hand-off.
        assert_eq!(slots[1].step, 0);
        assert_eq!(out.params[2], 0);
    }

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
