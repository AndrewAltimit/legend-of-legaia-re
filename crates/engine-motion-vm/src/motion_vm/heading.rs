//! Heading math: the engine-space compass LUT, walk facing, the rotate steps and the bearing conversion.

/// The 8-direction heading LUT at retail `DAT_80073F04`, expressed in the
/// **engine** heading space (`0` = +Z).
///
/// Retail stores `u16 entry[i] = i * 0x200` for `i` in `0..=7` (a 45° compass
/// in the retail heading space, `0` = -Z), and the ops index it `& 0xF` - the
/// upper eight slots are unrelated SCUS data, not direction entries, so this
/// port defines only the real eight. `engine = (retail + 0x800) & 0xFFF`, the
/// half-turn the `render_26` convention differs by (pinned from the
/// locomotion's pad->facing writes, `FUN_801d01b0` body
/// `0x801d04b8..0x801d0548`).
///
/// Index → direction: `0` = -Z, `2` = -X, `4` = +Z, `6` = +X, odd entries the
/// diagonals between them.
pub fn heading_lut_engine(idx: u8) -> Option<u16> {
    (idx <= 7).then(|| ((u16::from(idx) * 0x200).wrapping_add(0x800)) & 0x0FFF)
}

/// The **walk-direction-implied facing** index retail derives from a step's
/// axis signs - the tail of the `0x47` `MoveTowardTarget` case at
/// `0x80037D4C..0x80037DDC`, which writes `actor+0x26` from
/// [`heading_lut_engine`]. The written heading holds **one value per walk
/// leg** (runtime-pinned: a leg's whole run carries a single heading), so
/// the port issues the write at the leg's first moving frame and on a
/// step-direction change only.
///
/// This is the reason a walking NPC's facing is always one of eight compass
/// points and never an arbitrary bearing: retail quantises here, it does not
/// interpolate. Returns `None` when neither axis moved (retail's `a3 == 0`
/// early-out leaves the previous facing standing).
pub fn walk_facing_index(dx: i32, dz: i32) -> Option<u8> {
    let (sx, sz) = (dx.signum(), dz.signum());
    if sx == 0 && sz == 0 {
        return None;
    }
    Some(match sx {
        0 => (2 * sz + 2) as u8,
        s if s > 0 => (6 - sz) as u8,
        _ => (sz + 2) as u8,
    })
}

/// [`walk_facing_index`] resolved through [`heading_lut_engine`] - the 12-bit
/// engine-space heading a step of `(dx, dz)` snaps an actor to.
pub fn walk_facing_yaw(dx: i32, dz: i32) -> Option<u16> {
    walk_facing_index(dx, dz).and_then(heading_lut_engine)
}

/// One frame of the shared **rotate-toward-angle** law both yaw ops run
/// (`0x38` `RotateToAngle` and `0x4C` `FaceTarget`, retail `0x800379FC` /
/// `0x80037DE0`; the ambient VM's `0x04` facing ramp at `0x800385D0` is the
/// same law with a unit-per-tick cursor).
///
/// Given the arc still to travel and the frames still budgeted, retail steps
/// `arc * speed / remaining` - a linear ease whose per-frame magnitude is
/// recomputed from the *live* heading each tick (the arc is measured modulo
/// `0x1000`, so raw out-of-range headings feed back correctly). The caller
/// handles the terminal frame, which snaps to the exact target rather than
/// stepping.
///
/// The write-back is **plain u16 wrapping arithmetic - no `& 0xFFF`
/// normalisation per tick**. Retail's `+0x26` holds the raw value mid-ramp:
/// a decreasing ramp through zero runs `0xFFxx` headings frame after frame
/// and only the terminal snap lands back in `0..0xFFF` (runtime-pinned
/// frame-exact on the town01 Mei dinner-beat rotate legs, whose per-tick
/// values this law reproduces bit-for-bit including the wrap crossings).
/// Renderers consume the heading modulo `0x1000`, so the raw hold is
/// invisible on screen - but a port that masks every tick diverges from the
/// traced `+0x26` on any ramp that crosses the wrap.
///
/// `decreasing` selects the rotation direction; the arc is measured the long
/// way round when it has to be, so a forced direction still lands.
pub fn rotate_step(current: u16, target: u16, decreasing: bool, speed: u32, remaining: u32) -> u16 {
    let (cur, tgt) = (i32::from(current) & 0xFFF, i32::from(target) & 0xFFF);
    let arc = if decreasing { cur - tgt } else { tgt - cur };
    // Retail's `+0x1000` then `& 0xFFF`: normalise a possibly-negative arc
    // into `0..0xFFF` without a branch.
    let arc = ((arc + 0x1000) & 0xFFF) as u32;
    let inc = (arc * speed / remaining.max(1)) as u16;
    if decreasing {
        current.wrapping_sub(inc)
    } else {
        current.wrapping_add(inc)
    }
}

/// Convert a 2D displacement `(dx, dz)` to a 12-bit fixed-point yaw
/// (0x000..0xFFF, clockwise, 0x000 = +Z). Retail calls `FUN_80019b28`.
pub(super) fn bearing_to_yaw(dx: i32, dz: i32) -> u16 {
    if dx == 0 && dz == 0 {
        return 0;
    }
    let radians = (dx as f32).atan2(dz as f32);
    let raw = (radians * (0x1000 as f32) / (2.0 * std::f32::consts::PI)).round() as i32;
    // Normalize to 12-bit [0, 0x1000).
    ((raw % 0x1000 + 0x1000) % 0x1000) as u16
}

/// Step a value toward a target by at most `rate`, clamped - the ramp in
/// `FUN_8003BC08` (`ghidra/scripts/funcs/8003bc08.txt`,
/// `0x8003BCA8..0x8003BCF4`).
///
/// The name says "rotate" and the arithmetic is not rotation-specific; what
/// it clamps at this call site is a **height** delta, not an angle. The arm
/// reads `FUN_80019278`'s ground sample, subtracts the actor's current
/// `+0x16`, clamps the difference to `[-rate, rate]` and stores the sum back
/// to `+0x16` - the glide half of [`FieldActorHeight::GlideToFloor`]. The
/// heading reading came from the same mis-identification of `FUN_80019278`
/// the enum's own doc now records.
///
/// Unlike [`rotate_step`] (which distributes an arc over a frame budget and
/// wraps the write-back), this takes the raw signed difference
/// `target - current`, clamps it, and adds it back. `rate` is
/// `_DAT_1F800393 * 6` (the frame-time scalar - vsyncs per game tick - times
/// six) at the retail call site. The arithmetic is 16-bit truncating, matching the
/// halfword store to `+0x16`; there is no `& 0xFFF` normalisation, which is
/// consistent with a height and was the loose end under the heading reading.
///
/// PORT: FUN_8003BC08 (the `0x8003BCA8..0x8003BCF4` height-clamp arm)
///
/// Wired as the glide arm of the field NPCs' height pass:
/// `World::tick_field_npc_heights` (from `World::tick`'s field arm, on the
/// actor tick, both play hosts) steps each glide-class placement's Y with it,
/// and `World::field_npc_render_y` - which both hosts place NPCs through -
/// reads the result. The arm is reachable because every placement carries
/// the seater's `0x20000` (one of the arm's `0x20200` gate bits); the glide
/// itself needs `0x2000`, which the disc raises almost only as cross-context
/// `0x31` pokes from partition-2 cutscene records. The player's settle
/// (`FUN_801D1BA0`, rate `scalar * 12`) is a different routine.
pub fn rotate_toward_clamped(current: i16, target: i16, rate: i32) -> i16 {
    let mut delta = i32::from(target) - i32::from(current);
    if delta > rate {
        delta = rate;
    }
    if delta < -rate {
        delta = -rate;
    }
    (i32::from(current) + delta) as i16
}
