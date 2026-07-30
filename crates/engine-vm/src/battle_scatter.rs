//! The battle arena's emitter-driven sprite scatter.
//!
//! PORT: FUN_801E0080
//!
//! The per-frame animator behind the ambient clutter that drifts across a battle
//! arena - dust, sparks, leaves, whatever the scene's script table holds. Two
//! record pools and a byte-script per record, both hanging off the battle scene
//! buffer at `_DAT_8007BD30`:
//!
//! | Pool | Slots | Stride | Base |
//! |---|---|---|---|
//! | Emitters | 32 | `0x1C` | `_DAT_8007BD30 + 0x1010` |
//! | Particles | 128 | `0x20` | `_DAT_8007BD30 + 0x10` |
//!
//! Transcribed from the DISASSEMBLY in
//! `ghidra/scripts/funcs/overlay_battle_action_801e0080.txt` (606
//! instructions). The C is misleading in two places this port depends on: it
//! renders the two script advances as one shape when the **emitter** reads its
//! next delay byte at `old_cursor + 1` and the **particle** reads it at
//! `new_cursor + 1`, and it loses which of the two mirror flag bits drives which
//! UV pair.
//!
//! ## Per-frame sub-stepping
//!
//! The whole emitter + particle update is a loop, not a single pass. A pass over
//! both pools costs `1` if it touched any live countdown and `5` if it did not,
//! and passes repeat while the accumulated cost is below the frame delta
//! `DAT_1F800393`. So an idle pool settles after one pass and a busy one gets
//! one pass per delta unit. [`pass_cost`] is that rule.
//!
//! ## Gate
//!
//! `DAT_8007BD58 != 0 && DAT_8007BD71 == 0xFF` - battle live, no end signal
//! raised. `DAT_8007BD71 = 0xFE` is what the wipe / escape teardowns write (see
//! [`docs/subsystems/battle-action.md`](../../docs/subsystems/battle-action.md)),
//! so the scatter stops the frame a battle ends. A zero frame delta skips
//! straight to the render pass, leaving both pools standing.
//!
//! # NOT WIRED
//!
//! No engine caller. The retail per-frame caller is the battle draw tick
//! `FUN_800480D8` (`jal 0x801e0080` at `0x80048128`,
//! `ghidra/scripts/funcs/800480d8.txt`), whose own port -
//! `engine-render::battle_actor_tick` - is a schedule with no host yet. Two
//! further prerequisites, both outside this crate:
//!
//! * The **records are disc data**. Both pools live inside the per-scene battle
//!   buffer `_DAT_8007BD30` and their scripts are byte streams the scene's
//!   battle build installs; `engine-core`'s battle setup builds no such buffer,
//!   so there is nothing to tick. The [`ScatterEnv`] trait is where that data
//!   would attach.
//! * The **output is a GPU primitive**. The render pass emits one `0x28`-byte
//!   textured quad per live particle into the ordering table at
//!   `_DAT_1F8003A0`. [`sprite_draw`] returns the quad as data rather than
//!   writing an OT, but the consumer that turns it into a draw is
//!   `engine-render`'s.

use core::ops::Range;

/// Emitter slots retail scans (`s5 < 0x20`).
pub const EMITTERS: usize = 32;
/// Particle slots retail scans (`s4 < 0x80`).
pub const PARTICLES: usize = 128;
/// Emitter record stride (`a1*0x1C`, built as `((a1*8)-a1)*4`).
pub const EMITTER_STRIDE: usize = 0x1C;
/// Particle record stride (`s4 << 5`, built as `(s4 << 16) >> 11`).
pub const PARTICLE_STRIDE: usize = 0x20;
/// Bytes an emitter's script advances per spawn (`addiu v0, v0, 0xe`).
pub const EMITTER_SCRIPT_STEP: u32 = 14;
/// Bytes a particle's script advances per step (`addiu v0, v0, 0x6`).
pub const PARTICLE_SCRIPT_STEP: u32 = 6;
/// Countdown units drained per pass (`addiu v0, v0, -0x8`, floored at zero).
pub const COUNTDOWN_STEP: u8 = 8;
/// Shift applied to a script delay byte on load (`sll v0, v0, 0x3`).
pub const DELAY_SHIFT: u32 = 3;
/// The `0x09000000` ordering-table tag word the sprite prims carry.
pub const SPRITE_OT_TAG: u32 = 0x0900_0000;
/// GP0 command byte OR'd over the per-particle brightness (`0x2E000000`).
pub const SPRITE_GP0: u32 = 0x2E00_0000;
/// Sprite prim size in bytes (`t8 += 0x28` - a ten-word textured quad).
pub const SPRITE_PRIM_BYTES: usize = 0x28;
/// Full-scale brightness the fade ramp saturates at (`0x80`).
pub const FADE_MAX: u32 = 0x80;

/// Drain one pass off a countdown byte: `-8`, floored at zero.
///
/// Retail's shape is `if (c < 8) c = 0; else c -= 8`, which is the same as a
/// saturating subtract - both pools use it.
pub const fn drain_countdown(countdown: u8) -> u8 {
    countdown.saturating_sub(COUNTDOWN_STEP)
}

/// The cost one whole emitter+particle pass charges against the frame delta.
///
/// `touched_any_countdown` is retail's `s6`, incremented once per record whose
/// countdown was still running. The arithmetic is `if (s6 == 0) t1 += 4; t1 += 1`,
/// so a pass that animated nothing costs five and ends the loop immediately at
/// any plausible delta, while a busy pass costs one and lets the loop run
/// `DAT_1F800393` times.
pub const fn pass_cost(touched_any_countdown: bool) -> u8 {
    if touched_any_countdown { 1 } else { 5 }
}

/// Whether another pass runs, given the accumulated cost and the frame delta.
///
/// Retail compares `(t1 & 0xff) < DAT_1F800393` as unsigned bytes.
pub const fn another_pass(accumulated: u8, frame_delta: u8) -> bool {
    accumulated < frame_delta
}

/// One emitter record (`0x1C` bytes at `_DAT_8007BD30 + 0x1010 + i*0x1C`).
///
/// Field names are what the reads and writes fix; offsets are retail's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Emitter {
    /// `+0x00` - total spawns this emitter performs. Zero = slot inactive, and
    /// the terminator arm zeroes it once `spawned` reaches it.
    pub total: u8,
    /// `+0x02` - spawns issued so far.
    pub spawned: u8,
    /// `+0x03` - frames-until-next-spawn countdown, in `COUNTDOWN_STEP` units.
    pub countdown: u8,
    /// `+0x04` - emit heading, 12-bit (`4096` = full turn).
    pub angle: i16,
    /// `+0x08`, `+0x0C`, `+0x10` - base position the spawn copies.
    pub pos: [i32; 3],
    /// `+0x14` - a fourth word copied verbatim into the particle's `+0x18`.
    pub extra: i32,
    /// `+0x18` - cursor into the emitter's own script stream.
    pub cursor: u32,
}

/// One particle record (`0x20` bytes at `_DAT_8007BD30 + 0x10 + i*0x20`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Particle {
    /// `+0x00` - total script steps. Zero = slot free; also the fade ramp's
    /// denominator, and the terminator compares `steps` against it.
    pub total: u8,
    /// `+0x01` - two mirror bits, seeded `rand() % 4`. Bit `0` mirrors U,
    /// bit `1` mirrors V.
    pub mirror: u8,
    /// `+0x02` - script steps taken so far; the fade ramp's numerator.
    pub steps: u8,
    /// `+0x03` - frames-until-next-script-step countdown.
    pub countdown: u8,
    /// `+0x04`, `+0x06`, `+0x08` - per-axis velocity. The middle one is copied
    /// straight from the spawn record and is **not** rotated by the heading.
    pub vel: [i16; 3],
    /// `+0x0C`, `+0x10`, `+0x14` - position, `0x100`-scaled world units.
    pub pos: [i32; 3],
    /// `+0x18` - the emitter's `extra`, carried along untouched.
    pub extra: i32,
    /// `+0x1C` - cursor into this particle's own script stream.
    pub cursor: u32,
}

impl Particle {
    /// A free slot is one whose `total` byte is zero - the only test the spawn
    /// scan makes.
    pub const fn is_free(&self) -> bool {
        self.total == 0
    }
}

/// Everything the animator reads that is not in the two pools: the frame delta,
/// the trig tables, the per-scene script bytes and the RNG.
///
/// Retail resolves the tables by **dereferencing** `_DAT_8007B81C` and
/// `_DAT_8007B7F8` - the globals hold pointers, not the first entries - and
/// indexes them by a 12-bit angle with a halfword stride.
pub trait ScatterEnv {
    /// `DAT_1F800393` - this frame's delta in pass-cost units.
    fn frame_delta(&self) -> u8;
    /// `sin[angle & 0xFFF]`, via the pointer at `_DAT_8007B81C`.
    fn sin(&self, angle: i32) -> i16;
    /// `cos[angle & 0xFFF]`, via the pointer at `_DAT_8007B7F8`.
    fn cos(&self, angle: i32) -> i16;
    /// `*(i16*)(_DAT_8007BD30 + 0)` - the global motion scalar every position
    /// integration multiplies through.
    fn motion_scale(&self) -> i16;
    /// One byte of a script stream at an absolute cursor.
    fn script_u8(&self, cursor: u32) -> u8;
    /// One halfword of a script stream at an absolute cursor.
    fn script_i16(&self, cursor: u32) -> i16;
    /// The spawn-definition pointer table at `_DAT_8007BD30 + 8`, indexed by the
    /// byte at the emitter's cursor.
    fn spawn_def(&self, index: u8) -> u32;
    /// `func_0x80056798` - the SCUS RNG.
    fn rand(&mut self) -> i32;
}

/// The signed `rand() % 4` idiom retail uses for the mirror bits
/// (`bgez`, `+3`, `sra 2`, `sll 2`, `subu`) - a truncating-toward-zero
/// remainder, so a negative draw yields a non-positive remainder.
pub const fn rand_mod4(r: i32) -> i32 {
    let q = if r >= 0 { r >> 2 } else { (r + 3) >> 2 };
    r - (q << 2)
}

/// Rotate a `(x, z)` offset pair by a 12-bit heading, at the position shift.
///
/// Retail's two blocks are, with `sin`/`cos` from the LUT pointers and the
/// `0xFFF - angle` mirror standing in for a negated angle:
///
/// ```text
///   x += (sin[a] * oz) >> shift  +  (cos[0xfff-a] * ox) >> shift
///   z += (sin[0xfff-a] * ox) >> shift  +  (cos[a] * oz) >> shift
/// ```
///
/// which is the standard planar rotation `(ox*cos + oz*sin, -ox*sin + oz*cos)`,
/// `sin[0xfff-a]` standing in for `sin(-a)`. Each product is shifted **before**
/// the two halves are summed, so the port keeps the two shifts separate.
pub fn scatter_rotate_offset(
    env: &impl ScatterEnv,
    angle: i16,
    off_x: i16,
    off_z: i16,
    shift: u32,
) -> (i32, i32) {
    let a = angle as i32;
    let mirror = 0xFFF - a;
    let x = ((env.sin(a) as i32 * off_z as i32) >> shift)
        + ((env.cos(mirror) as i32 * off_x as i32) >> shift);
    let z = ((env.sin(mirror) as i32 * off_x as i32) >> shift)
        + ((env.cos(a) as i32 * off_z as i32) >> shift);
    (x, z)
}

/// Shift the position rotation uses (`sra ..., 0x4`).
pub const POS_ROT_SHIFT: u32 = 4;
/// Shift the velocity rotation uses (`sra ..., 0xC`).
pub const VEL_ROT_SHIFT: u32 = 12;

/// One axis of the per-pass position integration.
///
/// `pos += ((vel * speed * motion_scale) << 3) >> 15`, with every step a 32-bit
/// register operation - retail computes the two `mult`s into `LO` and shifts the
/// result, so the `<< 3` can overflow and the `>> 15` is arithmetic.
pub fn integrate_axis(pos: i32, vel: i16, speed: u8, motion_scale: i16) -> i32 {
    let prod = (vel as i32)
        .wrapping_mul(speed as i32)
        .wrapping_mul(motion_scale as i32);
    pos.wrapping_add(prod.wrapping_shl(3) >> 15)
}

/// The per-particle brightness ramp the render pass splats into all three
/// colour bytes.
///
/// `total >> 3` is the fade-in length: below it the level rises as
/// `((steps + 1) << 7) / ramp`, at or above it the level falls as
/// `((total - steps) << 7) / (total - ramp)`. Either result saturates at
/// [`FADE_MAX`] once it reaches `0x81`.
///
/// Retail traps (`break 0x1C00`) when a divisor is zero, which needs
/// `total < 8`; this saturates the divisor at 1 instead. The clamp is retail's
/// `sltiu a2, 0x81`, i.e. **unsigned** - a level that came out negative
/// (`steps > total`, which the terminator prevents) clamps to the cap rather
/// than to zero, and the port keeps that.
pub fn fade_level(total: u8, steps: u8) -> i32 {
    let ramp = (total >> 3) as i32;
    let level = if (steps as i32) < ramp {
        ((steps as i32 + 1) << 7) / ramp.max(1)
    } else {
        let span = total as i32 - ramp;
        ((total as i32 - steps as i32) << 7) / span.max(1)
    };
    if (level as u32) > FADE_MAX {
        FADE_MAX as i32
    } else {
        level
    }
}

/// Splat a brightness level across the three colour bytes and fold in the GP0
/// command, exactly as the `sll 16` / `sll 8` / `or` / `or` / `addu` chain does.
pub const fn fade_colour_word(level: i32) -> u32 {
    let l = level as u32;
    let grey = l | (l << 8) | (l << 16);
    grey.wrapping_add(SPRITE_GP0)
}

/// The four UV corners a sprite quad gets, in vertex order `uv0..uv3`.
///
/// The mirror bits are **negative** logic: with `mirror == 0` retail writes the
/// *high* U into corners 0 and 2 and the *high* V into corners 0 and 1, so bit
/// `0` set is what produces the un-mirrored U run and bit `1` set the
/// un-mirrored V run. Transcribed store-by-store from `0x801E0884..0x801E0970`
/// (prim UV slots `+0x0C`, `+0x14`, `+0x1C`, `+0x24`).
pub fn mirror_uv(mirror: u8, u: u8, v: u8, w: u8, h: u8) -> [(u8, u8); 4] {
    let u_hi = u.wrapping_add(w).wrapping_sub(1);
    let v_hi = v.wrapping_add(h).wrapping_sub(1);
    let (u0, u1) = if mirror & 1 != 0 {
        (u, u_hi)
    } else {
        (u_hi, u)
    };
    let (v0, v2) = if mirror & 2 != 0 {
        (v, v_hi)
    } else {
        (v_hi, v)
    };
    // corners 0 and 2 share a U, corners 1 and 3 share the other; corners 0
    // and 1 share a V, corners 2 and 3 share the other.
    [(u0, v0), (u1, v0), (u0, v2), (u1, v2)]
}

/// A sprite quad the render pass would link into the ordering table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpriteDraw {
    /// The `0x09000000` tag word.
    pub tag: u32,
    /// Command + brightness word (`+0x04`).
    pub code_colour: u32,
    /// Screen-space centre the projector is handed, `>> 8` off the position.
    pub centre: [i16; 3],
    /// Half-extents fed to the projector: `(descriptor_byte * scene_scale) >> 8`.
    pub half_extents: (i32, i32),
    /// UV corners in vertex order.
    pub uv: [(u8, u8); 4],
    /// CLUT word (`+0x0E`) and texture page (`+0x16`).
    pub clut_tpage: (u16, u16),
}

/// Build the sprite quad for one particle.
///
/// `descriptor` is the 8-byte sprite record at `_DAT_8007BD30 + 4 + kind*8`:
/// `(u, v, w, h)` in its first four bytes, CLUT at `+4` and tpage at `+6`.
/// `scene_scale` is `*(i16*)(_DAT_8007BD30 + 2)`.
pub fn sprite_draw(
    p: &Particle,
    descriptor: (u8, u8, u8, u8, u16, u16),
    scene_scale: i16,
) -> SpriteDraw {
    let (u, v, w, h, clut, tpage) = descriptor;
    SpriteDraw {
        tag: SPRITE_OT_TAG,
        code_colour: fade_colour_word(fade_level(p.total, p.steps)),
        centre: [
            (p.pos[0] >> 8) as i16,
            (p.pos[1] >> 8) as i16,
            (p.pos[2] >> 8) as i16,
        ],
        half_extents: (
            (w as i32 * scene_scale as i32) >> 8,
            (h as i32 * scene_scale as i32) >> 8,
        ),
        uv: mirror_uv(p.mirror, u, v, w, h),
        clut_tpage: (clut, tpage),
    }
}

/// Advance an emitter's script one spawn.
///
/// The delay byte is read at `cursor + 1` **before** the cursor moves, then the
/// cursor advances by [`EMITTER_SCRIPT_STEP`]. When the spawn count reaches
/// `total` the slot deactivates (`total = 0`) and the countdown is forced to
/// exactly `COUNTDOWN_STEP` rather than the script's value.
pub fn advance_emitter_script(e: &mut Emitter, env: &impl ScatterEnv) {
    let delay = env.script_u8(e.cursor.wrapping_add(1));
    e.cursor = e.cursor.wrapping_add(EMITTER_SCRIPT_STEP);
    e.spawned = e.spawned.wrapping_add(1);
    e.countdown = delay.wrapping_shl(DELAY_SHIFT);
    if e.spawned == e.total {
        e.total = 0;
        e.countdown = COUNTDOWN_STEP;
    }
}

/// Advance a particle's script one step.
///
/// Mirror image of [`advance_emitter_script`]: the cursor moves **first** and
/// the delay byte is read at the *new* `cursor + 1`. On the terminator the
/// countdown and `total` both clear, which frees the slot.
pub fn advance_particle_script(p: &mut Particle, env: &impl ScatterEnv) {
    p.cursor = p.cursor.wrapping_add(PARTICLE_SCRIPT_STEP);
    let delay = env.script_u8(p.cursor.wrapping_add(1));
    p.steps = p.steps.wrapping_add(1);
    p.countdown = delay.wrapping_shl(DELAY_SHIFT);
    if p.steps == p.total {
        p.countdown = 0;
        p.total = 0;
    }
}

/// Seed a free particle slot from an emitter, as the spawn arm does.
///
/// The spawn record is the 14-byte block at the emitter's cursor: `+0` the
/// definition index, `+2`/`+4`/`+6` a position offset triple, `+8`/`+0xA`/`+0xC`
/// a velocity triple. The X/Z members of both triples are rotated by the
/// emitter's heading (`>> 4` for the offset, `>> 12` for the velocity) while the
/// Y members are not: the offset's Y is *subtracted* scaled by `0x100` and the
/// velocity's Y is copied through untouched.
pub fn spawn_particle(p: &mut Particle, e: &Emitter, env: &mut impl ScatterEnv) {
    let def_index = env.script_u8(e.cursor);
    let def = env.spawn_def(def_index);

    p.total = env.script_u8(def);
    p.mirror = rand_mod4(env.rand()) as u8;
    p.steps = 0;

    p.pos = [e.pos[0], e.pos[1], e.pos[2]];
    p.extra = e.extra;

    let off_x = env.script_i16(e.cursor.wrapping_add(2));
    let off_y = env.script_i16(e.cursor.wrapping_add(4));
    let off_z = env.script_i16(e.cursor.wrapping_add(6));
    p.pos[1] = p.pos[1].wrapping_sub((off_y as i32).wrapping_shl(8));

    let (dx, dz) = scatter_rotate_offset(env, e.angle, off_x, off_z, POS_ROT_SHIFT);
    p.pos[0] = p.pos[0].wrapping_add(dx);
    p.pos[2] = p.pos[2].wrapping_add(dz);

    let vel_x = env.script_i16(e.cursor.wrapping_add(8));
    let vel_y = env.script_i16(e.cursor.wrapping_add(10));
    let vel_z = env.script_i16(e.cursor.wrapping_add(12));
    let (vx, vz) = scatter_rotate_offset(env, e.angle, vel_x, vel_z, VEL_ROT_SHIFT);
    p.vel = [vx as i16, vel_y, vz as i16];

    p.cursor = def.wrapping_add(2);
    p.countdown = env.script_u8(def.wrapping_add(3)).wrapping_shl(DELAY_SHIFT);
}

/// Run one pass of the particle half: drain each live countdown and integrate,
/// or advance the script when the countdown has expired.
///
/// Returns whether any countdown was live, which is the input to
/// [`pass_cost`].
pub fn tick_particles(particles: &mut [Particle], env: &impl ScatterEnv) -> bool {
    let mut touched = false;
    let scale = env.motion_scale();
    for p in particles.iter_mut() {
        if p.total == 0 {
            continue;
        }
        if p.countdown != 0 {
            touched = true;
            p.countdown = drain_countdown(p.countdown);
        } else {
            advance_particle_script(p, env);
        }
        // Both arms integrate. Retail's expired-countdown arm loops back into
        // the script advance while the new countdown reads zero, so a script
        // whose delay bytes are zero drains its whole stream in one pass; the
        // port takes one step per pass, which differs only for that
        // degenerate stream.
        let speed = env.script_u8(p.cursor.wrapping_add(2));
        for axis in 0..3 {
            p.pos[axis] = integrate_axis(p.pos[axis], p.vel[axis], speed, scale);
        }
    }
    touched
}

/// Index range of the emitter slots retail scans, exposed so a caller can
/// mirror the bound without hard-coding it.
pub const fn emitter_slots() -> Range<usize> {
    0..EMITTERS
}

/// Index range of the particle slots retail scans.
pub const fn particle_slots() -> Range<usize> {
    0..PARTICLES
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic environment: a flat script buffer plus quarter-turn trig.
    struct Env {
        script: Vec<u8>,
        defs: Vec<u32>,
        rand_queue: Vec<i32>,
        delta: u8,
        scale: i16,
    }

    impl Env {
        fn new(script: Vec<u8>) -> Self {
            Self {
                script,
                defs: vec![0; 8],
                rand_queue: Vec::new(),
                delta: 1,
                scale: 0x1000,
            }
        }
    }

    impl ScatterEnv for Env {
        fn frame_delta(&self) -> u8 {
            self.delta
        }
        fn sin(&self, angle: i32) -> i16 {
            // A 12-bit-angle sine scaled to 0x1000, computed rather than tabled.
            let rad = (angle & 0xFFF) as f64 * std::f64::consts::TAU / 4096.0;
            (rad.sin() * 4096.0).round() as i16
        }
        fn cos(&self, angle: i32) -> i16 {
            let rad = (angle & 0xFFF) as f64 * std::f64::consts::TAU / 4096.0;
            (rad.cos() * 4096.0).round() as i16
        }
        fn motion_scale(&self) -> i16 {
            self.scale
        }
        fn script_u8(&self, cursor: u32) -> u8 {
            self.script.get(cursor as usize).copied().unwrap_or(0)
        }
        fn script_i16(&self, cursor: u32) -> i16 {
            let lo = self.script_u8(cursor) as u16;
            let hi = self.script_u8(cursor.wrapping_add(1)) as u16;
            (lo | (hi << 8)) as i16
        }
        fn spawn_def(&self, index: u8) -> u32 {
            self.defs.get(index as usize).copied().unwrap_or(0)
        }
        fn rand(&mut self) -> i32 {
            self.rand_queue.pop().unwrap_or(0)
        }
    }

    #[test]
    fn pool_geometry_matches_the_stride_arithmetic() {
        // `((i*8) - i) * 4` is `i * 0x1C`, and `(i << 16) >> 11` is `i * 0x20`.
        for i in 0u32..32 {
            assert_eq!(((i * 8) - i) * 4, i * EMITTER_STRIDE as u32);
        }
        for i in 0u32..128 {
            assert_eq!(((i << 16) as i32 >> 11) as u32, i * PARTICLE_STRIDE as u32);
        }
    }

    #[test]
    fn countdown_drain_is_a_saturating_subtract() {
        assert_eq!(drain_countdown(0x40), 0x38);
        assert_eq!(drain_countdown(8), 0);
        assert_eq!(drain_countdown(7), 0);
        assert_eq!(drain_countdown(1), 0);
        assert_eq!(drain_countdown(0), 0);
    }

    #[test]
    fn an_idle_pass_costs_five_and_stops_the_loop_at_delta_one() {
        assert_eq!(pass_cost(false), 5);
        assert_eq!(pass_cost(true), 1);
        assert!(!another_pass(pass_cost(false), 1));
        assert!(!another_pass(pass_cost(true), 1));
        // A busy pool at delta 3 gets three passes.
        let mut acc = 0u8;
        let mut passes = 0;
        while another_pass(acc, 3) {
            acc = acc.wrapping_add(pass_cost(true));
            passes += 1;
        }
        assert_eq!(passes, 3);
    }

    #[test]
    fn rand_mod4_truncates_toward_zero() {
        assert_eq!(rand_mod4(0), 0);
        assert_eq!(rand_mod4(7), 3);
        assert_eq!(rand_mod4(8), 0);
        assert_eq!(rand_mod4(-1), -1);
        assert_eq!(rand_mod4(-5), -1);
        assert_eq!(rand_mod4(-8), 0);
        for r in -64i32..64 {
            assert_eq!(rand_mod4(r), r % 4, "r={r}");
        }
    }

    #[test]
    fn fade_ramps_up_over_an_eighth_then_down_over_the_rest() {
        let total = 64u8; // ramp = 8
        let cap = FADE_MAX as i32;
        assert_eq!(fade_level(total, 0), (1 << 7) / 8);
        assert_eq!(fade_level(total, 7), cap);
        // At the crossover the falling arm takes over.
        assert_eq!(fade_level(total, 8), ((64 - 8) << 7) / (64 - 8));
        assert_eq!(fade_level(total, 8), cap);
        assert_eq!(fade_level(total, 63), (1 << 7) / 56);
        assert_eq!(fade_level(total, 64), 0);
        // Never above the cap.
        for s in 0..=total {
            assert!(fade_level(total, s) <= cap, "steps={s}");
        }
    }

    #[test]
    fn fade_does_not_divide_by_zero_on_a_short_script() {
        // total < 8 makes the fade-in denominator zero; retail would trap.
        for total in 0u8..8 {
            for steps in 0u8..8 {
                let _ = fade_level(total, steps);
            }
        }
    }

    #[test]
    fn fade_colour_splats_grey_and_ors_the_command() {
        let w = fade_colour_word(0x40);
        assert_eq!(w & 0xFF, 0x40);
        assert_eq!((w >> 8) & 0xFF, 0x40);
        assert_eq!((w >> 16) & 0xFF, 0x40);
        assert_eq!(w & 0xFF00_0000, SPRITE_GP0);
    }

    #[test]
    fn mirror_bits_are_negative_logic() {
        let plain = mirror_uv(3, 0x10, 0x20, 8, 4);
        assert_eq!(
            plain,
            [(0x10, 0x20), (0x17, 0x20), (0x10, 0x23), (0x17, 0x23)]
        );
        // mirror == 0 flips both runs.
        let flipped = mirror_uv(0, 0x10, 0x20, 8, 4);
        assert_eq!(
            flipped,
            [(0x17, 0x23), (0x10, 0x23), (0x17, 0x20), (0x10, 0x20)]
        );
        // Bit 0 alone flips only U.
        let u_only = mirror_uv(1, 0x10, 0x20, 8, 4);
        assert_eq!(u_only[0].0, 0x10);
        assert_eq!(u_only[0].1, 0x23);
        // Bit 1 alone flips only V.
        let v_only = mirror_uv(2, 0x10, 0x20, 8, 4);
        assert_eq!(v_only[0].0, 0x17);
        assert_eq!(v_only[0].1, 0x20);
    }

    #[test]
    fn every_mirror_value_names_the_same_four_texel_corners() {
        let want: std::collections::HashSet<(u8, u8)> =
            [(0x10, 0x20), (0x17, 0x20), (0x10, 0x23), (0x17, 0x23)]
                .into_iter()
                .collect();
        for m in 0u8..4 {
            let got: std::collections::HashSet<(u8, u8)> =
                mirror_uv(m, 0x10, 0x20, 8, 4).into_iter().collect();
            assert_eq!(got, want, "mirror={m}");
        }
    }

    /// The `0xFFF - a` mirror is one table entry short of a true negation, so a
    /// rotation lands within a couple of units of the ideal - in retail as much
    /// as here, because retail indexes the same 4096-entry tables.
    fn near(got: i32, want: i32) {
        assert!(
            (got - want).abs() <= 8,
            "got {got}, want {want} (tolerance 8)"
        );
    }

    #[test]
    fn rotation_at_zero_heading_is_the_identity_up_to_the_shift() {
        let env = Env::new(vec![]);
        // cos(0) = 0x1000, sin(0) = 0 -> each axis keeps its own offset.
        let (x, z) = scatter_rotate_offset(&env, 0, 100, 0, VEL_ROT_SHIFT);
        near(x, 100);
        near(z, 0);
        let (x, z) = scatter_rotate_offset(&env, 0, 0, 100, VEL_ROT_SHIFT);
        near(x, 0);
        near(z, 100);
    }

    #[test]
    fn rotation_at_a_quarter_turn_swaps_the_axes() {
        let env = Env::new(vec![]);
        // 1024 units = 90 degrees: cos = 0, sin = 0x1000.
        let (x, z) = scatter_rotate_offset(&env, 1024, 4096, 0, VEL_ROT_SHIFT);
        near(x, 0);
        // sin[0xfff - 1024] = sin(-1024) = -0x1000 -> z = -ox.
        near(z, -4096);
        // And the other input axis maps the other way round.
        let (x, z) = scatter_rotate_offset(&env, 1024, 0, 4096, VEL_ROT_SHIFT);
        near(x, 4096);
        near(z, 0);
    }

    #[test]
    fn rotation_preserves_the_offset_magnitude_across_a_full_turn() {
        let env = Env::new(vec![]);
        for step in 0..16 {
            let angle = (step * 256) as i16;
            let (x, z) = scatter_rotate_offset(&env, angle, 4096, 0, VEL_ROT_SHIFT);
            let mag = ((x * x + z * z) as f64).sqrt();
            assert!(
                (mag - 4096.0).abs() < 8.0,
                "angle {angle}: magnitude {mag} drifted"
            );
        }
    }

    #[test]
    fn integration_is_a_wrapping_shift_pair() {
        assert_eq!(
            integrate_axis(0, 0x100, 0x10, 0x1000),
            (0x100 * 0x10 * 0x1000) >> 12
        );
        assert_eq!(integrate_axis(1000, 0, 0x10, 0x1000), 1000);
        // Negative velocity moves the other way.
        assert!(integrate_axis(0, -0x100, 0x10, 0x1000) < 0);
    }

    #[test]
    fn emitter_script_reads_its_delay_before_advancing() {
        // Byte 1 = 4, byte 15 = 9. The first advance must pick up 4, not 9.
        let mut script = vec![0u8; 32];
        script[1] = 4;
        script[15] = 9;
        let env = Env::new(script);
        let mut e = Emitter {
            total: 3,
            cursor: 0,
            ..Default::default()
        };
        advance_emitter_script(&mut e, &env);
        assert_eq!(e.countdown, 4 << DELAY_SHIFT);
        assert_eq!(e.cursor, EMITTER_SCRIPT_STEP);
        assert_eq!(e.spawned, 1);
        assert_eq!(e.total, 3, "still active");
        advance_emitter_script(&mut e, &env);
        assert_eq!(e.countdown, 9 << DELAY_SHIFT);
    }

    #[test]
    fn emitter_deactivates_with_a_forced_countdown_on_the_last_spawn() {
        let mut script = vec![0u8; 32];
        script[1] = 20;
        let env = Env::new(script);
        let mut e = Emitter {
            total: 1,
            cursor: 0,
            ..Default::default()
        };
        advance_emitter_script(&mut e, &env);
        assert_eq!(e.total, 0, "slot released");
        assert_eq!(
            e.countdown, COUNTDOWN_STEP,
            "the terminator overrides the script delay"
        );
    }

    #[test]
    fn particle_script_reads_its_delay_after_advancing() {
        // Byte 1 = 4 (never read), byte 7 = 9 (the first delay).
        let mut script = vec![0u8; 32];
        script[1] = 4;
        script[7] = 9;
        let env = Env::new(script);
        let mut p = Particle {
            total: 3,
            cursor: 0,
            ..Default::default()
        };
        advance_particle_script(&mut p, &env);
        assert_eq!(p.cursor, PARTICLE_SCRIPT_STEP);
        assert_eq!(p.countdown, 9 << DELAY_SHIFT);
        assert_eq!(p.steps, 1);
    }

    #[test]
    fn particle_terminator_frees_the_slot() {
        let env = Env::new(vec![0u8; 32]);
        let mut p = Particle {
            total: 1,
            cursor: 0,
            ..Default::default()
        };
        advance_particle_script(&mut p, &env);
        assert!(p.is_free());
        assert_eq!(p.countdown, 0);
    }

    #[test]
    fn spawn_copies_the_base_and_rotates_only_the_planar_offset() {
        // Definition 0 sits at script offset 16: total 40, delay byte at +3.
        let mut script = vec![0u8; 64];
        script[16] = 40; // def total
        script[19] = 2; // def delay
        // Emitter script at cursor 0: def index 0, offset (0, 3, 0), vel 0.
        script[0] = 0;
        script[4] = 3; // off_y low byte
        let mut env = Env::new(script);
        env.defs[0] = 16;
        env.rand_queue.push(6); // rand() -> mirror = 6 % 4 = 2

        let e = Emitter {
            total: 2,
            angle: 0,
            pos: [1000, 2000, 3000],
            extra: 0x1234,
            cursor: 0,
            ..Default::default()
        };
        let mut p = Particle::default();
        spawn_particle(&mut p, &e, &mut env);

        assert_eq!(p.total, 40);
        assert_eq!(p.mirror, 2);
        assert_eq!(p.steps, 0);
        assert_eq!(p.extra, 0x1234);
        assert_eq!(p.pos[0], 1000, "no planar offset at zero heading");
        assert_eq!(p.pos[1], 2000 - (3 << 8), "y is subtracted, not rotated");
        assert_eq!(p.pos[2], 3000);
        assert_eq!(p.cursor, 18, "def + 2");
        assert_eq!(p.countdown, 2 << DELAY_SHIFT);
    }

    #[test]
    fn tick_reports_whether_anything_was_animating() {
        let env = Env::new(vec![0u8; 32]);
        let mut pool = vec![Particle::default(); 4];
        assert!(
            !tick_particles(&mut pool, &env),
            "an empty pool touches nothing"
        );
        pool[1] = Particle {
            total: 40,
            countdown: 0x20,
            ..Default::default()
        };
        assert!(tick_particles(&mut pool, &env));
        assert_eq!(pool[1].countdown, 0x18);
    }

    #[test]
    fn tick_advances_the_script_when_the_countdown_has_expired() {
        let mut script = vec![0u8; 32];
        script[7] = 3;
        let env = Env::new(script);
        let mut pool = vec![Particle {
            total: 40,
            countdown: 0,
            cursor: 0,
            ..Default::default()
        }];
        let touched = tick_particles(&mut pool, &env);
        assert!(!touched, "an expired countdown is not a live one");
        assert_eq!(pool[0].steps, 1);
        assert_eq!(pool[0].countdown, 3 << DELAY_SHIFT);
    }

    #[test]
    fn sprite_draw_carries_the_scaled_extents_and_the_ramp() {
        let p = Particle {
            total: 64,
            steps: 7,
            mirror: 3,
            pos: [0x1_0000, 0x2_0000, 0x3_0000],
            ..Default::default()
        };
        let d = sprite_draw(&p, (0x10, 0x20, 8, 4, 0x7703, 0x27), 0x100);
        assert_eq!(d.tag, SPRITE_OT_TAG);
        assert_eq!(d.centre, [0x100, 0x200, 0x300]);
        assert_eq!(d.half_extents, (8, 4));
        assert_eq!(d.clut_tpage, (0x7703, 0x27));
        assert_eq!(d.code_colour, fade_colour_word(FADE_MAX as i32));
        assert_eq!(d.uv[0], (0x10, 0x20));
    }

    #[test]
    fn slot_ranges_match_the_loop_bounds() {
        assert_eq!(emitter_slots(), 0..0x20);
        assert_eq!(particle_slots(), 0..0x80);
    }
}
