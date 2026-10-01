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
    /// takes the in-fight arm. The engine core raises it through the
    /// battle-end sequence, the one place retail runs this arm.
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
    /// [`ACTION_OVERRIDE_CHAR_ID`] replaces the fallback translation, and
    /// the battle-over arm keys its per-character win-pose script on it.
    pub char_id: u8,
    /// `actor[+0x1DB]` - the acting actor's latched animation id. Only the
    /// battle-over arm reads it: its win-pose script dispatches on
    /// `id - 0x11` for the eight win poses `0x11..=0x18`
    /// ([`battle_over_script`]).
    pub anim_id: u8,
    /// `ctx[+0x87C]` - the close-up accumulator ([`BattleCamera`] owns the
    /// live value and overwrites this field, like [`Self::yaw_base`]).
    pub accum: u32,
    /// `ctx[+0x26E]` - the capped byte ramp, likewise camera-owned.
    pub ramp: u8,
    /// The acting actor's body radius `actor[+0x22C][+0x58]`, which sizes
    /// case 8's stand-off arm ([`apply_node_gone_reframe`]). Defaults to
    /// the measured party value [`PARTY_BODY_RADIUS`].
    pub body_radius: i32,
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
            anim_id: 0,
            accum: 0,
            ramp: 0,
            body_radius: PARTY_BODY_RADIUS,
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
    /// builder. The battle-over arm's is `0x500` moved by the win-pose
    /// script ([`battle_over_script`], which never reads the actor for it);
    /// the in-fight arm's is `ctx[+0x6D0]`, or the character-`4` override.
    pub fn raw_z(self) -> i32 {
        if self.takes_party_arm() {
            let mut s = BattleOverSlots::seed(0, 0);
            battle_over_script(self, &mut s);
            i32::from(s.tr_z)
        } else if self.char_id == ACTION_OVERRIDE_CHAR_ID && self.party_slot {
            ACTION_OVERRIDE_TR_Z_RAW
        } else {
            self.depth_raw
        }
    }
}

/// The four stack halfwords the battle-over arm edits: `sp+0x10` (pitch),
/// `sp+0x12` (yaw), `sp+0x1A` (TR.y) and `sp+0x1C` (raw TR.z). Retail stores
/// every one with `sh`, so the arithmetic wraps at 16 bits; the floor test
/// reads TR.y back with a signed `lh`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleOverSlots {
    pub pitch: i16,
    pub yaw: i16,
    pub tr_y: i16,
    pub tr_z: i16,
}

impl BattleOverSlots {
    /// The arm's base (`0x801D5CFC..0x801D5D2C`): pitch `0` (the prologue's
    /// zero), yaw `0x800 - actor[+0x46]`, TR.y `-5 * actor[+0x3E]` (an
    /// `sll 2` + `addu` pair, not a table), raw TR.z `0x500`.
    pub fn seed(facing: i32, display_y: i32) -> Self {
        BattleOverSlots {
            pitch: 0,
            yaw: (ACTION_PARTY_YAW_BASE - facing) as i16,
            tr_y: (-display_y).wrapping_mul(ACTION_PARTY_HEIGHT_SCALE as i32) as i16,
            tr_z: ACTION_PARTY_TR_Z_RAW as i16,
        }
    }
}

/// Animation id the battle-over arm's script tables start at: the eight
/// win poses `0x11..=0x18` (`addiu v1,v0,-0x11` / `sltiu v0,v1,0x8`).
pub const WIN_POSE_FIRST: u8 = 0x11;

/// The `(character, pose)` pair that skips the height floor
/// (`0x801D6478..0x801D648C`: character `2` posing `0x16`).
pub const FLOOR_EXEMPT_POSE: (u8, u8) = (2, 0x16);

/// The battle-over arm's **win-pose script** (`0x801D5D50..0x801D645C`): the
/// per-character, per-pose camera moves that frame the victory pose.
///
/// PORT: FUN_801D5854 (case 6's battle-over arm, the script between its base
/// and its floor)
///
/// The arm dispatches on the pose actor's character id
/// `DAT_8007BD10[ctx[+0x13]]`. Characters `1`, `2` and `3` each own an
/// eight-entry jump table indexed by `actor[+0x1DB] - 0x11` (`0x801CEA28`,
/// `0x801CEA48`, `0x801CEA68` in PROT 0898; character 1's entries 6 / 7
/// reuse its arms 0 / 1, and the other two tables' entry 7 reuses arm 1),
/// character `4` takes one fixed arm (`0x801D6440`), and any other id - or a
/// pose outside the table - edits nothing. Every arm reads the close-up
/// accumulator `a = ctx[+0x87C]` (a 32-bit word, shifted logically) and many
/// the capped ramp `r = ctx[+0x26E]`, so the shot keeps moving while the
/// pose is held: `FUN_801D5854`'s prologue advances both by
/// `8 * frame_step` per call, and the results sequencer calls it every
/// frame.
///
/// The `noa_levelup_banner` save state (Vahn posing `0x14`, `a = 616`) reads
/// pitch `-0x20` and yaw `0x800 - actor[+0x46]` exactly, with TR.y / TR.z
/// one tween step short of this arm's `A3` values.
pub fn battle_over_script(f: ActionFraming, s: &mut BattleOverSlots) {
    let a3 = (f.accum >> 3) as i32;
    let a2 = (f.accum >> 2) as i32;
    let r = i32::from(f.ramp);
    let add = |v: i16, d: i32| (i32::from(v) + d) as i16;
    let idx = f.anim_id.wrapping_sub(WIN_POSE_FIRST);
    match f.char_id {
        4 => {
            // A fixed tilt and a depth that only grows.
            s.pitch = 0x80;
            s.tr_z = (a3 + 0x680) as i16;
            return;
        }
        1..=3 if idx < 8 => {}
        _ => return,
    }
    match (f.char_id, idx) {
        // Character 1 - `0x801CEA28`.
        (1, 0 | 6) => {
            s.tr_y = add(s.tr_y, 0x80);
            s.tr_z = add(s.tr_z, -(a3 + 0x280 - 3 * r));
        }
        (1, 1 | 7) => {
            s.tr_y = add(s.tr_y, a3 - 3 * r + 0x1D8);
            s.tr_z = add(s.tr_z, 4 * r - a2 - 0x320);
        }
        (1, 2) => {
            s.pitch = 0x20;
            s.tr_y = add(s.tr_y, 0x30);
            s.yaw = add(s.yaw, -(a2 + 0x80));
            s.tr_z = add(s.tr_z, -(a2 + 0x480 - 6 * r));
        }
        (1, 3) => {
            s.pitch = -0x20;
            s.tr_y = add(s.tr_y, a3 - 0x40);
            s.tr_z = add(s.tr_z, -a2);
        }
        (1, 4) => {
            s.pitch = 0x80;
            s.yaw = add(s.yaw, 0xC0 - a3);
            s.tr_y = add(s.tr_y, -0x80);
            s.tr_z = add(s.tr_z, a3);
        }
        (1, 5) => {
            s.tr_y = add(s.tr_y, -0x40);
            s.yaw = add(s.yaw, 0x80 - a3);
            s.tr_z = add(s.tr_z, a3 - 0x100);
        }
        // Character 2 - `0x801CEA48`.
        (2, 0) => {
            s.tr_y = (a3 + 0x480) as i16;
            s.pitch = -0x20;
            s.tr_z = add(s.tr_z, -(a3 + 0x80));
        }
        (2, 1 | 7) => {
            s.tr_y = (a3 + 0x400) as i16;
            s.pitch = -0x20;
            s.tr_z = add(s.tr_z, -(a3 + 0x80));
            s.yaw = add(s.yaw, -(a3 - 0x80));
        }
        (2, 2) => {
            s.tr_y = add(s.tr_y, a3 - 0x80);
            s.pitch = -0x20;
            s.tr_z = add(s.tr_z, -(a3 + 0x100));
        }
        (2, 3) => {
            s.tr_y = add(s.tr_y, -0x40);
            s.pitch = add(s.pitch, 0xA0 - a3);
            s.tr_z = add(s.tr_z, a3 - 0x200);
        }
        (2, 4) => {
            s.pitch = add(s.pitch, 0x80 - a3);
            s.tr_y = add(s.tr_y, 0x40);
            s.tr_z = add(s.tr_z, -a2);
        }
        (2, 5) => {
            s.tr_y = add(s.tr_y, a3 - 0xC0);
            s.pitch = 0x100;
            s.tr_z = add(s.tr_z, -(a3 - 0x100));
            s.yaw = add(s.yaw, -(a3 - 0x40));
        }
        (2, 6) => {
            s.tr_y = add(s.tr_y, 0x80);
            s.tr_z = add(s.tr_z, a3 - 0x280);
        }
        // Character 3 - `0x801CEA68`.
        (3, 0) => {
            s.tr_y = add(s.tr_y, 0x108 - r);
            s.tr_z = add(s.tr_z, 3 * r + a3 - 0x400);
            s.yaw = add(s.yaw, -a3);
        }
        (3, 1 | 7) => {
            s.tr_y = add(s.tr_y, 0x148 - r);
            s.pitch = -0x20;
            s.tr_z = add(s.tr_z, -(a3 + 0x320 - 4 * r));
        }
        (3, 2) => {
            s.tr_y = add(s.tr_y, a3 - 0xC0);
            s.tr_z = add(s.tr_z, -a3);
            s.yaw = add(s.yaw, a3 - 0x180);
        }
        (3, 3) => {
            s.tr_y = add(s.tr_y, a3);
            s.tr_z = add(s.tr_z, -(a3 + 0x100));
        }
        (3, 4) => {
            s.tr_y = add(s.tr_y, a3 - 0x80);
            s.tr_z = add(s.tr_z, -(a3 + 0x80));
            s.yaw = add(s.yaw, a3 - 0x80);
            s.pitch = -0x40;
        }
        (3, 5) => {
            s.tr_y = add(s.tr_y, a3 - 0x60);
            s.tr_z = add(s.tr_z, -(a3 + 0x320 - 4 * r));
        }
        (3, 6) => {
            s.pitch = -0x40;
            s.yaw = add(s.yaw, a3 + 0x180);
            s.tr_z = add(s.tr_z, -(0x500 - a3 - 4 * r));
        }
        _ => {}
    }
}

/// The battle-over arm end to end: base, [`battle_over_script`], then the
/// height floor (`0x801D6494..0x801D64BC`, skipped for
/// [`FLOOR_EXEMPT_POSE`]) - a signed TR.y below `0x281` is raised to
/// `0x280` and a quarter of the shortfall (`sra 2`, computed from the old
/// value) is added to the pitch.
pub fn battle_over_slots(actor: BattleCamActor, f: ActionFraming) -> BattleOverSlots {
    let mut s = BattleOverSlots::seed(actor.facing, actor.world[1] as i32);
    battle_over_script(f, &mut s);
    let floor = ACTION_HEIGHT_FLOOR as i32;
    if (f.char_id, f.anim_id) != FLOOR_EXEMPT_POSE && i32::from(s.tr_y) <= floor {
        s.pitch = (i32::from(s.pitch) + ((floor - i32::from(s.tr_y)) >> 2)) as i16;
        s.tr_y = floor as i16;
    }
    s
}

/// Retail's case-6 action framing for the acting actor.
///
/// Ports `0x801D5CE8..0x801D65D8`: the arm fork, both arms' base poses, the
/// in-fight arm's style tweaks and character override, and the battle-over
/// arm's win-pose script ([`battle_over_script`]) and height floor with its
/// pitch compensation. (The per-art attack camera is a different routine,
/// `FUN_801D71B8`, run from the shared tail after either arm - see
/// [`crate::battle_attack_camera`].)
///
/// `actor.world` stands in for both position trios retail reads: the
/// battle-over arm reads the display position `actor[+0x3C/+0x3E/+0x40]`
/// (the live pair plus the pose centroid, `FUN_8004998C`) and the in-fight
/// arm the live position `actor[+0x34/+0x38]`. Neither arm writes the focus
/// height `sp+0x22`, which the prologue zeroes, so both focus on the stage
/// floor; the battle-over arm's only use of the display Y is its TR.y. The
/// caller hands the trio the arm wants (the engine core passes the display
/// trio while the battle-end sequence runs).
///
/// REF: FUN_801D5854 (case 6)
pub fn action_framing(actor: BattleCamActor, f: ActionFraming) -> BattleCamPose {
    let wrap = |a: i32| a.rem_euclid(4096) as f32;
    if f.takes_party_arm() {
        // `tr[1] = -actor[+0x3E] * 5`, the win-pose script, then the floor +
        // pitch compensation.
        let s = battle_over_slots(actor, f);
        return BattleCamPose {
            pitch: f32::from(s.pitch),
            yaw: wrap(i32::from(s.yaw)),
            tr: [0.0, f32::from(s.tr_y), prescale_tr_z(i32::from(s.tr_z))],
            // `sh v0,0x20(sp)` / `sh v0,0x24(sp)` only
            // (`0x801D5D3C..0x801D5D4C`): the display X / Z, the focus
            // height left at the prologue's zero.
            focus: [actor.world[0], 0.0, actor.world[2]],
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
