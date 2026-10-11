//! The round-start cameo walk-on.

// ---------------------------------------------------------------------------
// The round-start cameo walk-on (FUN_801D6310)
// ---------------------------------------------------------------------------

/// Pad bit (`_DAT_8007B850` held mask, Legaia's packed layout) whose hold at
/// round setup spawns the cameo: `0x10` = Triangle. The round-setup arm of the
/// cabinet (`0x32`) tests it at `0x801D0190..0x801D01AC` and spawns the
/// `0x801D7624` prototype, whose callback is [`cameo_pose`]'s routine.
pub const CAMEO_HOLD_MASK: u16 = 0x10;
/// Clip the cameo walks on (`+0x5C = 0x1D`).
pub const CAMEO_CLIP_WALK: i16 = 0x1D;
/// Clip it strikes its pose on (`+0x5C = 0x1C`).
pub const CAMEO_CLIP_POSE: i16 = 0x1C;
/// Fixed `y` / `z` of the cameo (`+0x16 = 0x8C`, `+0x18 = 0x400`), in the
/// camera-relative frame `+0x52 |= 0x400` selects.
pub const CAMEO_Y: i16 = 0x8C;
pub const CAMEO_Z: i16 = 0x400;
/// Phase at which the cameo raises its retire bit.
pub const CAMEO_RETIRE_PHASE: i16 = 0xF0;
/// The cameo's scene-bank model: the PROT 1203 stage pack's fourth TMD, the
/// 10-object ring girl. The prototype's `+0x04` half is `0` on the disc; the
/// cabinet init zeroes the bank base `_DAT_8007B6F8` (`0x801CF1C0`) and stamps
/// base `+ 3` into it (`0x801CF2C8..0x801CF2D8`) before `FUN_80020DE0` copies
/// it to `+0x64` (captured: the spawn store at `0x80020E70` writes `3`).
pub const CAMEO_SCENE_MODEL: u16 = 3;

/// The cameo's pose for one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameoPose {
    /// `+0x14` - screen-relative `x`: slides in from `0x100`, holds at `0`,
    /// slides out to the negative side.
    pub x: i16,
    pub y: i16,
    pub z: i16,
    /// `+0x26` - yaw: side-on (`0x400`) while walking, turned to face the
    /// camera (`0`) for the pose.
    pub yaw: i16,
    /// `+0x5C` - [`CAMEO_CLIP_WALK`] or [`CAMEO_CLIP_POSE`].
    pub clip: i16,
    /// `+0x62` bit 3: the pose clip holds its last frame instead of looping.
    pub hold_last_frame: bool,
    /// The VRAM cell the frame leaves showing, through [`sprite_blit`]'s
    /// table index (`0` until the pose, `1` from it on: both blits run every
    /// frame past phase `0x40`, and the second lands last).
    pub blit_index: usize,
    /// The retire bit rose this frame.
    pub retire: bool,
}

// Spawned by `BakaFight` at a round setup under a held Triangle (the host
// hands the duel its packed held word through `BakaFight::set_held_pad`),
// stepped per frame as `crate::baka_fighter::CameoActor`, and drawn by the
// duel's 3D surface (`legaia_engine_minigame_scenes::baka_duel_scene`): stage model
// [`CAMEO_SCENE_MODEL`] posed by records `0x1B` / `0x1C` of the PROT 1203
// clip bank in the camera-relative frame, with [`sprite_blit`]'s wink
// applied to the surface's VRAM.
/// PORT: FUN_801D6310, overlay_baka_fighter_0976_801d6310 - the round-start **cameo walk-on** animator.
///
/// Each frame it forces the actor's step `+0x6A = 8`, raises flag
/// `0x200000` and the camera-relative bit `+0x52 |= 0x400`, then poses the
/// actor from its phase `+0x22` (which it advances by the frame step after
/// the clip selector `FUN_800204F8` has run):
///
/// | phase | `x` | yaw | clip | cell |
/// |---|---|---|---|---|
/// | `0..0x20` | `0x100 - 8p` (walks in) | `0x400` | walk, looping | 0 |
/// | `0x20..0x40` | `0` | `0x400 - 32(p - 0x20)` (turns to camera) | walk | 0 |
/// | `0x40..0x90` | `0` | `0` | pose, held | 1 |
/// | `0x90..0xB0` | `0` | `32(p - 0x90)` (turns back) | walk, looping | 1 |
/// | `0xB0..` | `8(0xB0 - p)` (walks out) | `0x400` | walk | 1 |
///
/// and raises the retire bit from phase [`CAMEO_RETIRE_PHASE`] on. The cell
/// column is the blit helper [`sprite_blit`]: index `0` runs every frame from
/// phase `0`, index `1` every frame from `0x40`, both into the same VRAM cell.
///
/// A negative phase leaves `x` unwritten from an uninitialised register;
/// the spawn zeroes `+0x22` and the phase only ever grows, so no frame reaches
/// it, and the port returns `None` there.
pub fn cameo_pose(phase: i16) -> Option<CameoPose> {
    if phase < 0 {
        return None;
    }
    let p = phase as i32;
    let (mut x, mut yaw, mut clip, mut hold) = (0x100 - p * 8, 0x400, CAMEO_CLIP_WALK, false);
    let mut blit = 0;
    if p >= 0x20 {
        x = 0;
        yaw = 0x400 - (p - 0x20) * 32;
    }
    if p >= 0x40 {
        yaw = 0;
        clip = CAMEO_CLIP_POSE;
        hold = true;
        blit = 1;
    }
    if p >= 0x90 {
        yaw = (p - 0x90) * 32;
        clip = CAMEO_CLIP_WALK;
        hold = false;
    }
    if p >= 0xB0 {
        x = (0xB0 - p) * 8;
        yaw = 0x400;
    }
    Some(CameoPose {
        x: x as i16,
        y: CAMEO_Y,
        z: CAMEO_Z,
        yaw: yaw as i16,
        clip,
        hold_last_frame: hold,
        blit_index: blit,
        retire: phase >= CAMEO_RETIRE_PHASE,
    })
}

/// Whether the round setup spawns the cameo: the held pad word carries
/// [`CAMEO_HOLD_MASK`].
///
/// REF: FUN_801cf388 (`0x801D0190..0x801D01C4`)
pub fn cameo_spawns(held_pad: u16) -> bool {
    held_pad & CAMEO_HOLD_MASK != 0
}
