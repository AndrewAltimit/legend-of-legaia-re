//! The special-attack afterimage passes.

/// The ghost passes the special's afterimage draws.
pub const AFTERIMAGE_PASSES: usize = 2;
/// How far each pass sets its ghost back along the clip: `0x30` sixteenths,
/// three whole frames (`addiu v0, v0, -0x30` at `0x801D4B7C`, cumulative).
pub const AFTERIMAGE_LAG_STEP: i16 = 0x30;
/// The depth-cue level (`+0x78`, the `IR0` factor the mesh pass hands the
/// colour-blend call) the first ghost draws at - halfway to the colour word.
pub const AFTERIMAGE_CUE_START: u16 = 0x800;
/// Depth-cue step per pass: the second ghost draws three quarters of the way.
pub const AFTERIMAGE_CUE_STEP: u16 = 0x400;
/// Ordering-table push the ghosts draw under (`_DAT_1F8003F4 + 0x40`, the
/// same for both passes): they sort behind the fighter they trail.
pub const AFTERIMAGE_OT_PUSH: i32 = 0x40;
/// The colour word the spawn stores at `+0x74` (`0x801D4588`): the blend
/// target the depth cue pulls the ghost toward - black, with the
/// blend-enable byte `0x81` on top.
pub const AFTERIMAGE_COLOR_WORD: u32 = 0x8100_0000;
/// The frame-rate divisor the special's commit installs at `DAT_1F80037D`
/// (`0x801D4568`): half the round's `8`, so every clip that recomputes its
/// step from the divisor - both fighters' - runs at half speed for the rest of
/// the round.
pub const SPECIAL_RATE_DIVISOR: i32 = 4;

/// One ghost pass of the afterimage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AfterimagePass {
    /// Which live-mask bit this pass belongs to.
    pub bit: u32,
    /// `true` when the pass drew its ghost (`FUN_8001B964`).
    pub drawn: bool,
    /// `true` when the pass instead cleared its live-mask bit (its lagged
    /// cursor ran past the clip end).
    pub expired: bool,
    /// The lagged clip cursor the ghost is posed at (1/16 frame).
    pub cursor: i16,
    /// The depth-cue level it draws at.
    pub cue: u16,
}

/// What [`afterimage_pass`] resolved for one afterimage actor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AfterimageFrame {
    /// The actor retires this frame (its live mask was already empty).
    pub retire: bool,
    /// The live mask after the passes.
    pub live_mask: u16,
    /// The clip cursor the actor keeps (the advanced value; the passes'
    /// lags are scratch and are restored on the way out).
    pub cursor: i16,
    pub passes: Vec<AfterimagePass>,
}

/// PORT: FUN_801d49e8 - the special attack's **afterimage** (two lagged,
/// darkened ghosts of the attacker).
///
/// Wired: [`crate::baka_fighter::BakaFight`] spawns one [`AfterimageActor`]
/// on every special commit, exactly where the combat tick `FUN_801D3F44`
/// spawns the `0x801D7684` prototype (`0x801D4538..0x801D4634`), steps it
/// every tick, and exposes the ghosts through
/// [`crate::baka_fighter::BakaFight::afterimages`]; the minigames page draws
/// them as darkened copies of the attacker's mesh posed at each ghost's frame.
/// The native window and the play page draw the duel as labels with no fighter
/// meshes at all, so neither has a model to ghost - see
/// `docs/tooling/host-drift.md`.
///
/// This was read as a "mirrored two-pass sprite draw" whose second copy was
/// yawed `0x400` further. The field it steps is `+0x78`, which is not a yaw:
/// it is the depth-cue level `FUN_8001B964` hands the colour-blend call with
/// the colour word `+0x74`, and the spawn sets that word to
/// [`AFTERIMAGE_COLOR_WORD`]. The draw call is the animated mesh renderer, not
/// a sprite emitter. So the two copies are the same pose family drawn
/// [`AFTERIMAGE_LAG_STEP`] and twice that behind the cursor, pulled half and
/// three quarters of the way to black, and sorted [`AFTERIMAGE_OT_PUSH`]
/// deeper than the fighter.
///
/// Per frame: an empty live mask (`+0x5A == 0`) retires the actor outright.
/// Otherwise the clip record the id `+0x5C` reaches is cached, the owner's
/// position is copied in, and the cursor advances by `(step * 2 + n - 1) / n *
/// frame_step` for `n` = record byte `+6` - the clip selector's formula with
/// its `+1` flag test dropped, so it always applies. Then each of the
/// [`AFTERIMAGE_PASSES`] passes lags the cursor one more step and, for a set
/// bit, draws while the lagged cursor is in `0 ..< record[+2] * 0x10 - 1` and
/// clears the bit once it has run past that end.
///
/// The body reads `s5` without ever writing it - the transform call
/// `FUN_8003D344(s5 + 0x14, s5 + 0x2C)` runs against whatever the caller left
/// in the register. That is in the disassembly, not a decompiler artifact, so
/// the port takes no such argument rather than reproducing the read.
pub fn afterimage_pass(
    live_mask: u16,
    cursor: i16,
    frame_step: i32,
    step: i16,
    record_n: u8,
    clip_frames: u16,
) -> AfterimageFrame {
    if live_mask == 0 {
        return AfterimageFrame {
            retire: true,
            live_mask,
            cursor,
            passes: Vec::new(),
        };
    }
    let n = record_n.max(1) as i32;
    let advance = ((step as i32 * 2 + n - 1) / n) * frame_step;
    let kept = (cursor as i32 + advance) as i16;
    let mut running = kept;
    let end = (clip_frames as i32 * 0x10 - 1) as i16;
    let mut mask = live_mask;
    let mut passes = Vec::new();
    for i in 0..AFTERIMAGE_PASSES {
        running = running.wrapping_sub(AFTERIMAGE_LAG_STEP);
        let cue = AFTERIMAGE_CUE_START.wrapping_add(AFTERIMAGE_CUE_STEP * i as u16);
        if live_mask >> i & 1 == 0 {
            continue;
        }
        let (mut drawn, mut expired) = (false, false);
        if running >= 0 {
            if running < end {
                drawn = true;
            } else {
                mask &= !(1u16 << i);
                expired = true;
            }
        }
        passes.push(AfterimagePass {
            bit: i as u32,
            drawn,
            expired,
            cursor: running,
            cue,
        });
    }
    AfterimageFrame {
        retire: false,
        live_mask: mask,
        cursor: kept,
        passes,
    }
}

/// One live afterimage actor: the state [`afterimage_pass`] runs over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AfterimageActor {
    /// `+0x50` - the fighter slot it trails (its position is re-copied from
    /// that fighter every frame).
    pub owner: usize,
    /// `+0x5A` - live mask, `3` at spawn: both ghosts.
    pub live_mask: u16,
    /// `+0x68` - its own clip cursor, zeroed by the spawn.
    pub cursor: i16,
    /// `+0x6A` - the step the spawn computes:
    /// `record[+4] * DAT_1F80037D >> 3` against the divisor the special's
    /// commit has just lowered to [`SPECIAL_RATE_DIVISOR`].
    pub step: i16,
}

impl AfterimageActor {
    /// The spawn at a special commit: owner `slot`, the special record's
    /// speed `special_speed` (`+0x04`).
    pub fn spawn(slot: usize, special_speed: i32) -> Self {
        let raw = special_speed * SPECIAL_RATE_DIVISOR;
        // `bgez; addiu 7; sra 3` at `0x801D4624..0x801D4630`.
        let step = (if raw < 0 { raw + 7 } else { raw }) >> 3;
        AfterimageActor {
            owner: slot,
            live_mask: 3,
            cursor: 0,
            step: step as i16,
        }
    }

    /// One frame. `record_n` / `clip_frames` are the special clip's ANM
    /// record byte `+6` and frame count `+2`; `None` when the host did not
    /// stage the fighter's clip bank, in which case the ghosts carry their
    /// cursor but no clip end can expire them and they retire with the
    /// special's exchange instead.
    pub fn tick(&mut self, frame_step: i32, clip: Option<(u8, u16)>) -> AfterimageFrame {
        let (n, frames) = clip.unwrap_or((1, i16::MAX as u16 / 0x10));
        let f = afterimage_pass(
            self.live_mask,
            self.cursor,
            frame_step,
            self.step,
            n,
            frames,
        );
        self.live_mask = f.live_mask;
        self.cursor = f.cursor;
        f
    }
}
