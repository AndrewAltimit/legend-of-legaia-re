//! Action framing - `FUN_801D5854` case 6.
//! Split out of `battle_cam_script.rs`.

use super::*;

// ---------------------------------------------------------------------------
// Action framing - `FUN_801D5854` case 6.
// ---------------------------------------------------------------------------

/// Yaw base the **party** arm subtracts the actor facing from: half a turn,
/// i.e. the camera sits behind the acting character (`0x801D5D10`).
pub const ACTION_PARTY_YAW_BASE: i32 = 0x800;
/// Raw eye-space Z the battle-over arm seeds (`0x801D5D00`).
pub const ACTION_PARTY_TR_Z_RAW: i32 = 0x500;
/// The battle-over arm's TR.y is `-5 * actor[+0x3E]` - a `sll 2` + `addu` pair
/// (`0x801D5D24..0x801D5D2C`), not a table lookup.
pub const ACTION_PARTY_HEIGHT_SCALE: f32 = 5.0;
/// Height floor the battle-over arm clamps TR.y up to (`0x801D64A4`).
pub const ACTION_HEIGHT_FLOOR: f32 = 0x280 as f32;
/// TR.y the **fallback** arm seeds (`0x801D64CC`).
pub const ACTION_TR_Y: f32 = 0x500 as f32;
/// TR.y the in-fight arm's style-2/3 tweak substitutes (`0x801D6564`).
pub const ACTION_STYLE_TR_Y: f32 = 0x400 as f32;
/// Pitch the in-fight arm's style-2/3 tweak adds (`0x801D656C`).
pub const ACTION_STYLE_PITCH: f32 = 0x80 as f32;
/// Character id whose fallback framing is overridden wholesale
/// (`0x801D65A4`: pitch `0x80`, raw depth `0xC00`, TR.y `0x300`).
pub const ACTION_OVERRIDE_CHAR_ID: u8 = 4;
pub(super) const ACTION_OVERRIDE_PITCH: f32 = 0x80 as f32;
pub(super) const ACTION_OVERRIDE_TR_Z_RAW: i32 = 0xC00;
pub(super) const ACTION_OVERRIDE_TR_Y: f32 = 0x300 as f32;
/// Camera steps the action framing glides over: retail passes
/// `FUN_801D829C` a duration of `0xC` display frames and a camera step is
/// two frames.
pub const ACTION_STEPS: u32 = 6;

/// The per-art attack camera's per-actor channels, as the hosts hand them in.
///
/// Retail's `FUN_801D71B8` reads them straight off the acting actor record;
/// this is the same three bytes plus the character selector, bundled so a
/// host cannot wire two of the three and leave the arm dispatching on a
/// default. `None` on [`BattleCamInputs::attack`] means the channel is not
/// armed this frame at all - no Attack action, a monster slot, a character
/// with no camera script, or a host with no disc table.
///
/// See [`crate::battle_attack_camera`] for what each byte is and where it
/// comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackCamChannels {
    /// `DAT_8007BD10[ctx[+0x13]]`, resolved.
    pub character: crate::battle_attack_camera::CharacterArm,
    /// `actor[+0x1DB]` - the latched battle-animation id.
    pub art_id: u8,
    /// `actor[+0x21B]` - the arm sub-selector.
    pub arm_select: u8,
    /// `actor[+0x22C][+0x68]` - the animation cursor in sixteenths of a
    /// keyframe.
    pub anim_frame: i16,
}

/// The non-pose inputs `FUN_801D5854` case `6` reads out of the battle
/// context. Every field is a retail context byte / halfword; the engine
/// supplies what it models and leaves the rest at the [`Default`], which
/// reproduces the arm retail takes for an ordinary action in a running fight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionFraming {
    /// `ctx[+0x13] < 3` - the acting slot is a party seat.
    pub party_slot: bool,
    /// `DAT_8007BD71 == 0xFE` - the **battle-end signal** (the `0x5A` wipe
    /// scans and the successful-escape teardown raise it; it reads `0xFF`
    /// for the whole of a running fight). Together with [`Self::party_slot`]
    /// this selects the battle-over arm; while a fight runs every action
    /// takes the in-fight arm. No host raises it yet.
    pub battle_over: bool,
    /// `ctx[+0x6D0]` - the raw eye-space depth `FUN_801F0348` derives from
    /// the framed monster's size class. Only the in-fight arm reads it.
    pub depth_raw: i32,
    /// `ctx[+0x6DA]` - the yaw base the in-fight arm subtracts the actor
    /// facing from.
    ///
    /// It is not a constant: the action SM's prologue advances it every tick
    /// (`0x801E29E4..0x801E2A24`, `+= (4 * frame_step) % 3` or `+= 1` when
    /// that is zero, i.e. about one unit per display frame), so successive
    /// enemy actions frame from a slowly drifting angle. [`BattleCamera`]
    /// owns the live counter and overwrites this field; it stays public so
    /// the framing stays a pure function of its inputs.
    pub yaw_base: i32,
    /// `ctx[+0xD]` - framing style. `1` and `3` add half a turn to the yaw;
    /// `2` and `3` drop TR.y to `0x400` and tilt the pitch by `0x80`.
    pub style: u8,
    /// `DAT_8007BD10[slot]` - the 1-based character id.
    /// [`ACTION_OVERRIDE_CHAR_ID`] replaces the fallback translation.
    pub char_id: u8,
}

impl Default for ActionFraming {
    /// A party seat in a running battle at the `FUN_801F0348` depth floor -
    /// the arm and the value retail takes for an ordinary party attack.
    fn default() -> Self {
        ActionFraming {
            party_slot: true,
            battle_over: false,
            depth_raw: crate::battle_formulas::CAMERA_HEIGHT_MIN as i32,
            yaw_base: 0,
            style: 0,
            char_id: 0,
        }
    }
}

impl ActionFraming {
    /// Which of case 6's two arms this input takes: the battle-over arm
    /// (`0x801D5CFC`) needs a party seat **and** the battle-end signal;
    /// anything else - every action of a running fight - is the in-fight
    /// arm (`0x801D64C4`).
    pub const fn takes_party_arm(self) -> bool {
        self.party_slot && self.battle_over
    }

    /// The framing's eye-space Z in **raw world units**, before the
    /// projection prescale - the value retail's case 6 hands the tween
    /// builder. The battle-over arm's is a constant; the in-fight arm's is
    /// `ctx[+0x6D0]`, or the character-`4` override.
    pub const fn raw_z(self) -> i32 {
        if self.takes_party_arm() {
            ACTION_PARTY_TR_Z_RAW
        } else if self.char_id == ACTION_OVERRIDE_CHAR_ID && self.party_slot {
            ACTION_OVERRIDE_TR_Z_RAW
        } else {
            self.depth_raw
        }
    }
}

/// Retail's case-6 action framing for the acting actor.
///
/// Ports `0x801D5CE8..0x801D65D8`: the arm fork, both arms' base poses, the
/// in-fight arm's style tweaks and character override, and the battle-over
/// arm's height floor with its pitch compensation. What is **not** here is
/// the battle-over arm's per-character / per-anim script between its base
/// and its floor. (The per-art attack camera is a different routine,
/// `FUN_801D71B8`, run from the shared tail after either arm - see
/// [`crate::battle_attack_camera`].)
///
/// `actor.world` stands in for both position trios retail reads: the
/// battle-over arm focuses on the display position
/// `actor[+0x3C/+0x3E/+0x40]` and the in-fight arm on the live position
/// `actor[+0x34/+0x38]`, whose focus height stays at the stage floor - the
/// prologue zeroes `sp+0x22` and the arm never writes it. The engine's
/// battle actors carry one position, so both resolve to it (the in-fight
/// arm with its Y dropped).
///
/// REF: FUN_801D5854 (case 6)
pub fn action_framing(actor: BattleCamActor, f: ActionFraming) -> BattleCamPose {
    let wrap = |a: i32| a.rem_euclid(4096) as f32;
    if f.takes_party_arm() {
        // `tr[1] = -actor[+0x3E] * 5`, then the floor + pitch compensation.
        let mut pitch = 0.0f32;
        let mut height = -actor.world[1] * ACTION_PARTY_HEIGHT_SCALE;
        if height < ACTION_HEIGHT_FLOOR {
            // Retail writes the floor first and computes the compensation
            // from the *old* value (`0x801D64A8..0x801D64BC`); the `sra 2` is
            // an arithmetic halving of a positive shortfall here.
            pitch += ((ACTION_HEIGHT_FLOOR - height) as i32 >> 2) as f32;
            height = ACTION_HEIGHT_FLOOR;
        }
        return BattleCamPose {
            pitch,
            yaw: wrap(ACTION_PARTY_YAW_BASE - actor.facing),
            tr: [0.0, height, prescale_tr_z(ACTION_PARTY_TR_Z_RAW)],
            focus: actor.world,
        };
    }
    let mut pitch = 0.0f32;
    let mut yaw = f.yaw_base - actor.facing;
    let mut tr_y = ACTION_TR_Y;
    // `ctx[+0xD]`: 1 and 3 add the half turn; 2 and 3 share the body that
    // drops the height and tilts the pitch (retail reaches it by falling out
    // of the `== 3` arm into the `== 2` arm).
    if f.style == 1 || f.style == 3 {
        yaw += 0x800;
    }
    if f.style == 2 || f.style == 3 {
        tr_y = ACTION_STYLE_TR_Y;
        pitch += ACTION_STYLE_PITCH;
    }
    let mut tr_z_raw = f.depth_raw;
    if f.party_slot && f.char_id == ACTION_OVERRIDE_CHAR_ID {
        pitch = ACTION_OVERRIDE_PITCH;
        tr_z_raw = ACTION_OVERRIDE_TR_Z_RAW;
        tr_y = ACTION_OVERRIDE_TR_Y;
    }
    BattleCamPose {
        pitch,
        yaw: wrap(yaw),
        tr: [0.0, tr_y, prescale_tr_z(tr_z_raw)],
        // `sh v0,0x20(sp)` / `sh v0,0x24(sp)` only (`0x801D64F0..0x801D650C`):
        // X and Z from `+0x34`/`+0x38`, the focus height left at zero.
        focus: [actor.world[0], 0.0, actor.world[2]],
    }
}

// ---------------------------------------------------------------------------
// Summon cast close-up - `FUN_801DC0A0` case `0x12`.
// ---------------------------------------------------------------------------

/// The action-SM states whose every pass calls `FUN_801DC0A0(caster, 0x12)`:
/// the summon band's flash-in `0x33` (`0x801E4A48`) and actor-freeze `0x34`
/// (`0x801E4ACC`), while the caster plays its cast clip. Neither calls
/// `FUN_801D5854`, so case 6 does not frame them.
pub const SUMMON_CAST_STATES: [u8; 2] = [0x33, 0x34];

/// `FUN_801DC0A0` case `0x12`'s tween duration `a3` (`li a3,0x3` at
/// `0x801DCD5C`), in display frames.
pub const SUMMON_CAST_TWEEN_FRAMES: u32 = 3;

/// The summon cast close-up, `FUN_801DC0A0` case `0x12`
/// (`0x801DCCF0..0x801DCD94`): a camera low beside the caster looking up at
/// it, swinging round and rising as the context accumulator `ctx[+0x87C]`
/// runs.
///
/// ```text
/// pitch = -(ctx[+0x26E] * 2)                     // the capped ramp: down to -400
/// yaw   = -actor[+0x46] + ctx[+0x87C] * 2 + 0x500
/// TR    = (0, ctx[+0x87C] * 2 + 0x300, 0x680 - ctx[+0x87C] * 3)
/// focus = -(actor[+0x3C], 0, actor[+0x40])       // display X/Z, floor height
/// ```
///
/// Every component is stored as a halfword (`sh`), so the accumulator terms
/// wrap at 16 bits exactly as retail's do. The routine's prologue advances
/// `ctx[+0x26E]` / `ctx[+0x87C]` on the same law `FUN_801D5854`'s does
/// ([`crate::battle_attack_camera::AttackCamCtx::advance`]), and it hands the
/// three vectors to `FUN_801D829C` with [`SUMMON_CAST_TWEEN_FRAMES`]. Returns
/// the pose (TR.z prescaled) and the raw TR.z.
///
/// REF: FUN_801DC0A0 (case `0x12`), FUN_801D829C
pub fn summon_cast_framing(
    actor: BattleCamActor,
    body: Option<[f32; 2]>,
    accum: u32,
    ramp: u8,
) -> (BattleCamPose, i32) {
    let half = |v: i64| i32::from(v as i16);
    let acc = i64::from(accum);
    let pitch = half(-(i64::from(ramp) * 2));
    let yaw = half(-i64::from(actor.facing) + acc * 2 + 0x500);
    let tr_y = half(acc * 2 + 0x300);
    let raw_z = half(0x680 - acc * 3);
    (
        BattleCamPose {
            pitch: pitch as f32,
            yaw: yaw.rem_euclid(4096) as f32,
            tr: [0.0, tr_y as f32, prescale_tr_z(raw_z)],
            // `lhu v0,0x3c(s2)` / `lhu v0,0x40(s2)` (`0x801DCD74..0x801DCD84`):
            // the body pair, not the live `+0x34` / `+0x38`.
            focus: match body {
                Some([x, z]) => [x, 0.0, z],
                None => [actor.world[0], 0.0, actor.world[2]],
            },
        },
        raw_z,
    )
}
