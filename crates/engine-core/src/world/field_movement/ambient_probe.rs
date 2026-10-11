//! The player-blocking probe the ambient motion VM steps against.

use super::*;

/// The walk half of the ambient motion VM's collision service: retail's
/// `FUN_801cf8ac` box test, whose only subject is the **player actor**.
///
/// Both call sites reduce to "is the player standing where I am about to
/// step". The directional steps `0x03`/`0x19`/`0x20` probe the single
/// `DAT_801F2254` compass point ([`FIELD_FACING_PROBES`]) for their
/// heading-LUT index; the `0x18` wander probes the three-point fan of
/// `DAT_801F21B4` ([`FIELD_ACTOR_PROBES`]) for its cardinal. Both apply the
/// shared `(x + dx, z - dz)` convention around the *walking actor* and
/// accept a hit inside [`FIELD_NPC_BOX_HALF`] on both axes.
///
/// Neither probe reads the walkability grid, so a wandering villager is
/// bounded by its op's authored AABB rather than by walls.
///
/// What this covers of `FUN_801cf8ac` (`0x801CF8AC..0x801CF9F0`) is the
/// **class arm** - `+0x10 & 0x01020000` set, box `0x40 - 0x18` = ±40 about
/// the walker's live position. That is the only arm an ambient walker can
/// take: every placement the MAN spawner `FUN_8003A1E4` seats gets
/// `+0x10 |= 0x20000` (`lui v1, 2; or` at `0x8003A3A8..0x8003A3B4`, plus the
/// party-bank bit `0x01000000` for a `>= 0xF0` special model), and the
/// ambient channels are exactly those placements. The same bit makes the
/// hit's result class `1` (`+0x10 & 0x40020000`), never `4`.
///
/// The routine's **collision-exempt early-out** is ported too
/// ([`Self::exempt`]): `+0x10 & 3` non-zero returns `0` before any box test
/// (`andi v0, v0, 3; bnez` at `0x801CF8B8`), and the directional caller
/// skips the call outright under `+0x10 & 1` (`0x80038484..0x80038490`). A
/// script sets those bits with op `0x31` (`31 00` / `31 01`), and the
/// placement channel's live flag word
/// ([`World::field_channel_flags`]) is where the port keeps them - so a walker
/// a script has made collision-exempt walks through the player, as in retail.
///
/// The rest of the routine changes nothing an ambient walker can observe:
///
/// - the **no-class arm** (`0x801CF8D4..0x801CF930`: box `0x40 + 0x10` = ±80
///   plus the model-bbox offset of the 32-byte record
///   `*(0x1F8003EC) + actor[+0x60] * 32`) is unreachable for a placement,
///   which leaves the class arm only if bit 17 is cleared and no disc script
///   clears it; and its result is `4`, which every caller drops - all three
///   sites (`0x800384C0`, `0x80038A78`, `0x80038F3C`) test `result & 1`;
/// - the hit's mutual contact link (`0x801CF9BC..0x801CF9C8`,
///   `player[+0x98] = actor`, `actor[+0x98] = player`) is overwritten before
///   it is read: the player-side readers (`0x801D0750`, `0x801D0868`) run
///   only on a hit of the player's own probe, which rewrites `+0x98` first.
///
/// PORT: FUN_801cf8ac
/// REF: FUN_801d5a68
pub(super) struct AmbientPlayerProbe {
    pub(super) player: Option<(i16, i16)>,
    /// The walker's `+0x10 & 3` collision-exempt bits are up: every probe
    /// misses.
    pub(super) exempt: bool,
}

/// The 12-bit engine heading that points along `(dx, dz)` - the engine's
/// stand-in for retail's arctangent resolver `FUN_80019B28` (LUT at
/// `DAT_8006F4C8`), in the engine's own convention where `0` faces Z+ rather
/// than retail's `+0x26` space where `0` faces Z-.
///
/// Float `atan2` rather than the LUT, so the result is shape-faithful and not
/// bit-exact; every consumer either draws it or quantises it into a 45°
/// compass sector.
///
/// REF: FUN_80019B28
pub(super) fn engine_bearing(dx: f32, dz: f32) -> i16 {
    ((dx.atan2(dz) / std::f32::consts::TAU * 4096.0).round() as i32 & 0x0FFF) as i16
}

/// Does this motion stream carry a walk op - a directional step
/// (`0x03`/`0x19`/`0x20`) or the `0x18` AABB wander?
pub(super) fn stream_has_walk_op(code: &[u8]) -> bool {
    let mut pc = 0usize;
    while pc < code.len() {
        let Some(w) = legaia_scene_formats::man_motion::op_width(code[pc]) else {
            return false;
        };
        if matches!(code[pc], 0x03 | 0x19 | 0x20 | 0x18) {
            return true;
        }
        pc += w;
    }
    false
}

impl AmbientPlayerProbe {
    fn hit(&self, x: i16, z: i16, dx: i16, dz: i16) -> bool {
        if self.exempt {
            return false;
        }
        let Some((px, pz)) = self.player else {
            return false;
        };
        let qx = x.saturating_add(dx) as i32;
        let qz = z.saturating_sub(dz) as i32;
        (qx - px as i32).abs() < FIELD_NPC_BOX_HALF && (qz - pz as i32).abs() < FIELD_NPC_BOX_HALF
    }
}

impl legaia_engine_motion_vm::ambient_motion::AmbientBlocking for AmbientPlayerProbe {
    fn step_blocked(&self, x: i16, z: i16, lut_index: u8) -> bool {
        let (dx, dz) = FIELD_FACING_PROBES[usize::from(lut_index & 7)];
        self.hit(x, z, dx, dz)
    }

    /// The wander fan, which retail hoists into its own routine.
    ///
    /// `FUN_801d5a68(actor, dir)` indexes `DAT_801F21B4` at `dir * 0x10`,
    /// reads three `(i16 dx, i16 dz)` pairs from `+0x00 / +0x04 / +0x08` of
    /// that row, calls `FUN_801cf8ac(actor, dx, dz)` on each, and returns the
    /// bitwise **or** of the three results - so any one of the fan's three
    /// points hitting the player refuses the step. The row's fourth pair
    /// (`+0x0C`) is never read.
    ///
    /// PORT: FUN_801d5a68
    fn wander_blocked(&self, x: i16, z: i16, dir4: u8) -> bool {
        FIELD_ACTOR_PROBES[usize::from(dir4) & 3]
            .iter()
            .any(|&(dx, dz)| self.hit(x, z, dx, dz))
    }
}
