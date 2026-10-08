//! Recovery / end-of-action framings - `FUN_801D5854` cases 7 and 8 - and the death reframe.
//! Split out of `battle_cam_script.rs`.

use super::*;

// ---------------------------------------------------------------------------
// Recovery / end-of-action framings - `FUN_801D5854` cases 7 and 8.
// ---------------------------------------------------------------------------

/// The half-turn both post-strike cases add for framing styles `1` and `3`
/// (`0x801D66D4` / `0x801D68D8`) - the same `ctx[+0xD]` fork case 6's
/// in-fight arm runs.
pub(super) const POST_STYLE_HALF_TURN: i32 = 0x800;
/// Case 7's yaw pre-rotation before the unwrap (`0x801D6708`).
pub(super) const RECOVER_YAW_BIAS: i32 = -0x700;
/// Case 8's (`0x801D693C`) - and its extra base offset (`0x801D67EC`).
pub(super) const ACTION_END_YAW_BIAS: i32 = -0x600;
pub(super) const ACTION_END_YAW_OFFSET: i32 = -0x100;
/// TR.y both cases seed (`0x801D65E4` / `0x801D67D8`).
pub(super) const POST_TR_Y: f32 = 0x500 as f32;
/// Case 7's "pull in" tweak (`0x801D6780`): `TR.y += 0x40`, `TR.z = 3z/5`,
/// pitch levelled.
pub(super) const RECOVER_PULL_TR_Y_STEP: f32 = 0x40 as f32;
/// Camera steps both cases glide over: `a3 = 0xC` display frames
/// (`0x801D67C8` / `0x801D6EEC`), two frames per camera step.
pub const POST_ACTION_STEPS: u32 = 6;

/// The one extra context byte cases 7 and 8 read that case 6 does not: the
/// **target** the acting actor is resolved against (`actor[+0x1DD]` indexed
/// into the 8-slot actor table `0x801C9370`).
///
/// Case 7 orbits the midpoint of the two, which is what makes it the only
/// framing in the set that keeps *both* combatants on screen; case 8 orbits
/// the target alone. `None` means retail's own fallback: case 7 degenerates
/// to the acting actor (the midpoint of a point with itself) and case 8 takes
/// its `0x801D6870` arm, which frames the actor.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PostActionTarget {
    /// The framed target's position as both cases read it: X / Z from the
    /// body pair `+0x3C` / `+0x40` (`lh v1,0x3c(v1)` at `0x801D6620`,
    /// `lhu v0,0x3c(v1)` at `0x801D683C`) and Y from the live height `+0x36`
    /// (`lh v1,0x36(v1)` at `0x801D6650`, the case-7 midpoint's only Y and
    /// the death re-frame's falling test).
    pub world: [f32; 3],
    /// The display height `+0x3E` - the live `+0x36` raised by the pose
    /// centroid - which case 8's live-target arm sizes `TR.y` from.
    pub display_y: f32,
    /// The target seat is a party one (`actor[+0x1DD] < 3`,
    /// `0x801D6C04`): the live-target arm's two halves.
    pub party: bool,
    /// A party target's per-character height `0x801F4D2C[char - 1]`, the
    /// live-target arm's `TR.y` when the target is not animating.
    pub height: Option<f32>,
    /// A monster target's formation id `0x8007BD0C[slot - 3]`, which scales
    /// the live-target arm's depth ([`live_target_depth`]).
    pub monster_id: u8,
    /// The target's current anim `+0x1D9` is non-zero (`0x801D6C2C` /
    /// `0x801D6DDC`).
    pub animating: bool,
    /// `actor[+0x1DD] < 8 && actor_table[target][+4] != 0` - case 8's test for
    /// "the target slot is real and its node is live" (`0x801D6808`,
    /// `0x801D682C`). False takes the arm that frames the actor instead.
    pub live: bool,
    /// The target's battle heading `+0x46`, which case 8's dead-target arm
    /// builds its yaw from (`0x801D6A60..0x801D6A98`).
    pub facing: i32,
    /// The target's scene node is gone - the low 24 bits of
    /// `actor_table[target][+4]` read zero (`0x801D6AA8..0x801D6AC0`). A dead
    /// target whose node is gone takes the stand-off arm
    /// ([`apply_node_gone_reframe`]) instead of the death re-frame. Every
    /// dead monster of the `noa_levelup_banner` save state reads a zero
    /// node.
    pub node_gone: bool,
    /// The lone-monster defeat bypass beside the node test
    /// (`0x801D6AC8..0x801D6AF0`): `ctx[+0x287] != 0 && 0x8007BD0D == 0 &&
    /// ctx[+0x288] != 0` - a scripted fight's only monster dying in place -
    /// takes the stand-off arm as a gone node does. It is not read by the
    /// focus fork, which tests the node word alone.
    pub lone_defeat: bool,
}

/// Case 8's dead-target yaw (`0x801D6A20..0x801D6A98`), stored over the
/// unwrapped base yaw before either dead-target arm runs:
/// `-target[+0x46] - ((ctx[+0x26D] << 9) - 0x100)`, with `ctx[+0x26D]` (the
/// per-action track coin) zeroed first when a party seat is character `3`.
pub fn dead_target_yaw(target_facing: i32, phase_cursor: u8, f: ActionFraming) -> i32 {
    let cursor = if f.party_slot && f.char_id == 3 {
        0
    } else {
        i32::from(phase_cursor)
    };
    i32::from((-target_facing - ((cursor << 9) - 0x100)) as i16)
}

/// TR.y of case 8's stand-off arm (`li v0,0x400` at `0x801D6AC4`).
pub const NODE_GONE_TR_Y: i32 = 0x400;

/// A party seat's body radius `actor[+0x22C][+0x58]`: `0x280` for each of
/// Vahn, Noa and Gala in the `noa_levelup_banner` save state.
pub const PARTY_BODY_RADIUS: i32 = 0x280;

/// Case 8's **stand-off** arm (`0x801D6B9C..0x801D6BF8`): a dead target
/// whose node is gone frames the acting actor alone, level, at a depth sized
/// by the actor's own body radius.
///
/// ```text
/// TR    = (0, 0x400, radius * 5 >> 1)       radius = actor[+0x22C][+0x58]
/// pitch = 0
/// yaw   = dead_target_yaw + ctx[+0x6DA]
/// focus = (actor[+0x3C], 0, actor[+0x40])
/// ```
///
/// It is the framing the battle-end sequence's load window lands on: every
/// monster is down and gone, so `FUN_8004E568`'s `FUN_801D5854(seat, 8)`
/// arrives here. Returns the raw TR.z.
///
/// PORT: FUN_801D5854 (case 8's node-gone arm)
pub fn apply_node_gone_reframe(
    pose: &mut BattleCamPose,
    actor: BattleCamActor,
    yaw: i32,
    radius: i32,
) -> i32 {
    let raw_z = i32::from(((radius * 5) >> 1) as i16);
    pose.pitch = 0.0;
    pose.tr = [0.0, NODE_GONE_TR_Y as f32, prescale_tr_z(raw_z)];
    pose.yaw = (yaw & 0xFFF) as f32;
    pose.focus = [actor.world[0], 0.0, actor.world[2]];
    raw_z
}

/// Retail's case-7 framing: the post-strike **two-shot**.
///
/// Ports `0x801D65DC..0x801D67CC`. Base pose (`0x801D65DC..0x801D6694`):
///
/// ```text
/// pitch = 0
/// yaw   = ctx[+0x6DA] - actor[+0x46]
/// TR    = (0, 0x500, ctx[+0x6D0])
/// focus = midpoint(actor[+0x3C/+0x36/+0x40], target[+0x3C/+0x36/+0x40])
/// ```
///
/// then the shared `ctx[+0xD]` style fork (`0x801D6698`), then a yaw
/// pre-rotation with a **one-way unwrap** (`0x801D6700`): `yaw = (yaw -
/// 0x700) & 0xFFF`, and if that lands below the live camera yaw a full turn
/// is added, so the swing always orbits in one direction instead of taking
/// the short arc. Finally the "pull in" tweak (`0x801D6780`), gated on
/// `_DAT_800846C0 == 0` and on the acting actor's anim state: pitch levelled,
/// `TR.y += 0x40`, `TR.z = TR.z * 3 / 5`.
///
/// The midpoint focus is the whole point of the case and it is verified, not
/// inferred - see [`RECOVER_STATES`] for the two retail states that read it
/// back byte-exactly.
///
/// REF: FUN_801D5854 (case 7)
pub fn recover_framing(
    actor: BattleCamActor,
    target: Option<PostActionTarget>,
    f: ActionFraming,
    camera_yaw: f32,
    pull_in: bool,
) -> BattleCamPose {
    let mut pitch = 0.0f32;
    let mut tr_y = POST_TR_Y;
    let mut yaw = f.yaw_base - actor.facing;
    if f.style == 1 || f.style == 3 {
        yaw += POST_STYLE_HALF_TURN;
    }
    if f.style == 2 || f.style == 3 {
        tr_y = ACTION_STYLE_TR_Y;
        pitch += ACTION_STYLE_PITCH;
    }
    let mut tr_z_raw = f.depth_raw;
    if pull_in {
        pitch = 0.0;
        tr_y += RECOVER_PULL_TR_Y_STEP;
        tr_z_raw = tr_z_raw * 3 / 5;
    }
    let other = target.map(|t| t.world).unwrap_or(actor.world);
    BattleCamPose {
        pitch,
        yaw: unwrap_forward(yaw + RECOVER_YAW_BIAS, camera_yaw),
        tr: [0.0, tr_y, prescale_tr_z(tr_z_raw)],
        focus: [
            (actor.world[0] + other[0]) * 0.5,
            (actor.world[1] + other[1]) * 0.5,
            (actor.world[2] + other[2]) * 0.5,
        ],
    }
}

/// Retail's case-8 framing: the end-of-action shot, orbiting the **target**.
///
/// Ports case 8's base (`0x801D67D0..0x801D69A4`): the same
/// `(0, 0x500, ctx[+0x6D0])` translation and `ctx[+0x6DA] - actor[+0x46]`
/// yaw as case 7 with an extra `-0x100` bias, `focus.y` forced to zero, the
/// target/actor focus fork, the `ctx[+0xD]` style tweaks (style `2` reaches
/// `TR.y = 0x400` by subtracting `0x100` from the `0x500` seed rather than
/// storing it, which is the same value) and the `-0x600` one-way yaw unwrap.
///
/// The per-liveness tail from `0x801D69A8` splits in two. The **death
/// re-frame** - the arm a dead target takes, with the `ctx[+0x270]` ramp - is
/// ported as [`apply_death_reframe`] and applied by
/// [`BattleCamera::action_end_pose`]. What stays out is the counter-attack
/// fork (`ctx[+0x287]` / `ctx[+0x288]` / `_DAT_8007BD0D`, `0x801D6AC8`) and
/// the live-target arm's own re-aim at `0x801D6BFC`, both of which read
/// channels the engine's battle actor does not carry.
///
/// REF: FUN_801D5854 (case 8)
pub fn action_end_framing(
    actor: BattleCamActor,
    target: Option<PostActionTarget>,
    f: ActionFraming,
    camera_yaw: f32,
) -> BattleCamPose {
    let mut pitch = 0.0f32;
    let mut tr_y = POST_TR_Y;
    let mut yaw = f.yaw_base - actor.facing + ACTION_END_YAW_OFFSET;
    if f.style == 1 || f.style == 3 {
        yaw += POST_STYLE_HALF_TURN;
    }
    if f.style == 2 || f.style == 3 {
        tr_y = ACTION_STYLE_TR_Y;
        pitch += ACTION_STYLE_PITCH;
    }
    let framed = match target {
        // The focus fork tests the target's node word `+0x4`
        // (`0x801D682C`), not its HP: a target killed on the return is
        // still drawn, and the death re-frame looks at it.
        Some(t) if t.live || !t.node_gone => t.world,
        _ => actor.world,
    };
    BattleCamPose {
        pitch,
        yaw: unwrap_forward(yaw + ACTION_END_YAW_BIAS, camera_yaw),
        tr: [0.0, tr_y, prescale_tr_z(f.depth_raw)],
        // Retail writes `sh zero,0x22(sp)` before the fork: the focus height
        // is pinned to the stage floor, not to the framed actor's own Y.
        focus: [framed[0], 0.0, framed[2]],
    }
}

/// A monster target's depth in case 8's live-target arm
/// (`0x801D6D1C..0x801D6DBC`), from the formation id of the framed seat:
/// `0xB4` pulls out to `9z/10`, `0xA2` / `0xA7` keep `z`, `0x1F..=0x21` take
/// `8z/10` and every other monster `7z/10`. The divides are retail's
/// `0x66666667` multiply-high, which truncates toward zero.
pub fn live_target_depth(monster_id: u8, depth_raw: i32) -> i32 {
    let z = i32::from(depth_raw as i16);
    let tenths = |n: i32| (n * z) / 10;
    match monster_id {
        0xB4 => tenths(9),
        0xA2 | 0xA7 => z,
        0x1F..=0x21 => tenths(8),
        _ => tenths(7),
    }
}

/// Raw TR.z of the live-target arm on a party target (`li v0,0x600` at
/// `0x801D6C0C`).
pub const LIVE_PARTY_TARGET_TR_Z: i32 = 0x600;
/// The live-target arm's `TR.y` floor on a party target (`slti v0,v1,0x281`).
pub const LIVE_PARTY_TARGET_FLOOR: i32 = 0x280;
/// The live-target arm's `TR.y` floor on a monster target (`slti v0,v1,0x301`).
pub const LIVE_MONSTER_TARGET_FLOOR: i32 = 0x300;

/// Case 8's **live-target** arm (`0x801D6BFC..0x801D6E80`): the end-of-action
/// shot of a target that is still standing, re-aimed by how the target
/// stands. Applied over [`action_end_framing`]'s base; `live` is the camera's
/// own pose (`_DAT_8007B790` / `_DAT_800840BC`), whose pitch and `TR.y` the
/// arm holds when a monster target is not animating. Returns the raw TR.z.
///
/// ```text
/// party target (slot < 3):   TR.z = 0x600
///   animating:   TR.y = pitch == 0 ? -4 * y : -7 * y / 2      (y = +0x3E)
///   else:        TR.y = height[char] - (pitch == 0 ? 0x140 : 0xC0)
///   floor 0x280: TR.y <= 0x280 -> pitch += (0x280 - TR.y) >> 2, TR.y = 0x280
/// monster target:            TR.z = live_target_depth(id, ctx[+0x6D0])
///   animating or _DAT_8007BD84 != 0:
///                TR.y = pitch == 0 ? -7 * y / 2 : -3 * y
///                floor 0x300 (same law)
///   else:        TR.y, pitch = the live camera's
/// ```
///
/// The pitch tested is the staged one, after the `ctx[+0xD]` style tweak.
/// `_DAT_8007BD84` is the Mystic Shield effect handle, which this arm does
/// not carry; it reads null outside Cort's fight, so the port takes the
/// animating test alone.
///
/// The `battle_melee_hit_spark` capture (`ctx[7] == 0x20`, Vahn's swing on a
/// monster held on its knockdown) reads this arm's tween targets: raw depth
/// `7 * 0xC00 / 10 = 0x866` (`TR.z 3440`) and `TR.y` within a few units of
/// `-7 * y / 2` over the target's display height.
///
/// PORT: FUN_801D5854 (case 8's live-target arm)
pub fn apply_live_target_reframe(
    pose: &mut BattleCamPose,
    t: PostActionTarget,
    depth_raw: i32,
    live: BattleCamPose,
) -> i32 {
    let y = i32::from(t.display_y as i16);
    let pitch = pose.pitch as i32;
    let half = |v: i32| i32::from(v as i16);
    let floored = |pose: &mut BattleCamPose, tr_y: i32, floor: i32| {
        if tr_y <= floor {
            pose.pitch = half(pose.pitch as i32 + ((floor - tr_y) >> 2)) as f32;
            pose.tr[1] = floor as f32;
        } else {
            pose.tr[1] = tr_y as f32;
        }
    };
    if t.party {
        let raw_z = LIVE_PARTY_TARGET_TR_Z;
        pose.tr[2] = prescale_tr_z(raw_z);
        let tr_y = if t.animating {
            if pitch == 0 {
                half(-4 * y)
            } else {
                half((-7 * y) / 2)
            }
        } else {
            let h = t.height.unwrap_or(SUBMENU_HEIGHT_FALLBACK) as i32;
            half(h - if pitch == 0 { 0x140 } else { 0xC0 })
        };
        floored(pose, tr_y, LIVE_PARTY_TARGET_FLOOR);
        return raw_z;
    }
    let raw_z = live_target_depth(t.monster_id, depth_raw);
    pose.tr[2] = prescale_tr_z(raw_z);
    if t.animating {
        let tr_y = if pitch == 0 {
            half((-7 * y) / 2)
        } else {
            half(-3 * y)
        };
        floored(pose, tr_y, LIVE_MONSTER_TARGET_FLOOR);
    } else {
        pose.tr[1] = live.tr[1];
        pose.pitch = live.pitch;
    }
    raw_z
}

/// `TR.y` the death re-frame seeds before the ramp (`li v0,0x300`).
pub const DEATH_TR_Y: i32 = 0x300;
/// Pitch the death re-frame seeds before the ramp (`li v0,0x140`).
pub const DEATH_PITCH_FLAT: i32 = 0x140;
/// Pitch the ramp counts **down** from (`li v0,0x180`), which is why a ramped
/// re-frame starts steeper than the flat one and settles below it.
pub const DEATH_PITCH_BASE: i32 = 0x180;

/// Case 8's **death re-frame** (`0x801D6AF8..0x801D6B98`): the pose retail
/// swaps in once the framed target's HP has reached zero.
///
/// PORT: FUN_801D5854 (case 8's dead-target arm)
///
/// Three constants land first, unconditionally:
///
/// ```text
/// TR.y  = 0x300          pitch = 0x140          TR.z = ctx[+0x6D0]
/// ```
///
/// and `ctx[+0x6DA]` - the per-action yaw ladder - is zeroed alongside them
/// (`sh zero,0x4(t0)`, `t0 = ctx + 0x6D6`), so a death shot does not inherit
/// the swing's accumulated orbit. The port's ladder lives on
/// [`BattleCamera::action_yaw`], which is why the reset is the caller's half
/// of this and not this function's.
///
/// Then the fork on the target's own anchor height `+0x36`
/// (`lh v0,0x36(v0)` at `0x801D6B38`), which is the Y of the same world
/// triple case 7 takes its focus midpoint from:
///
/// - **height `0`** - the body is on the stage floor. The flat pose above is
///   final and `ctx[+0x270]` is re-zeroed (`sb zero,0x270(a0)`). Returns
///   `true`, the caller's cue to clear the ramp.
/// - **height non-zero** - the body is still falling, and the ramp `r =
///   ctx[+0x270]` tightens all three components at once:
///
/// ```text
/// TR.z   = ctx[+0x6D0] - 4 * r
/// TR.y   = 0x300 - r
/// pitch  = 0x180 - (3 * r >> 1)
/// ```
///
/// At the saturated `r = 0xC8` that is `TR.z - 0x320`, `TR.y = 0x238` and
/// `pitch = 0x54` - the camera drops, levels off and pushes in on the falling
/// body. Retail does the three subtractions in 16-bit stack slots
/// (`lhu` / `subu` / `sh`); the port keeps them in `i32` because every live
/// input is far enough from the wrap for the two to agree, and a wrapped
/// depth would be a garbage pose either way.
///
/// `depth_raw` is retail's `ctx[+0x6D0]` in world units - the same space
/// [`ActionFraming::depth_raw`] carries - so the `4 * r` comes off *before*
/// [`prescale_tr_z`], exactly as retail subtracts before `FUN_801D829C`.
pub fn apply_death_reframe(
    pose: &mut BattleCamPose,
    depth_raw: i32,
    ramp: u8,
    target_height: f32,
) -> bool {
    pose.tr[1] = DEATH_TR_Y as f32;
    pose.pitch = DEATH_PITCH_FLAT as f32;
    pose.tr[2] = prescale_tr_z(depth_raw);
    if target_height == 0.0 {
        return true;
    }
    let r = i32::from(ramp);
    pose.tr[2] = prescale_tr_z(depth_raw - 4 * r);
    pose.tr[1] = (DEATH_TR_Y - r) as f32;
    pose.pitch = (DEATH_PITCH_BASE - ((3 * r) >> 1)) as f32;
    false
}

/// Both post-action cases' one-way yaw unwrap (`0x801D6700` / `0x801D6930`):
/// wrap into 12 bits, then add a full turn if the result would make the tween
/// rotate *backwards* past the live camera yaw. Retail compares against
/// `_DAT_8007B792` itself, so the direction of the swing depends on where the
/// orbit happens to be - which is why successive end-of-action shots do not
/// all swing the same way.
pub(super) fn unwrap_forward(yaw: i32, camera_yaw: f32) -> f32 {
    let wrapped = yaw.rem_euclid(4096);
    if (wrapped as f32) < camera_yaw {
        (wrapped + 0x1000) as f32
    } else {
        wrapped as f32
    }
}
