//! The Baka Fighter duel's camera and fighter-clip kernels - the pure half
//! of `engine-core`'s `baka_duel_scene`, which re-exports everything here
//! and keeps the mesh assembly (`BakaDuelScene` / `BakaDuelSurface`).
//!
//! Both kernels come from the overlay's own code (PROT 0976, base
//! `0x801CE818`; `see ghidra/scripts/funcs/overlay_baka_fighter_801cf388.txt`
//! for the cabinet and `overlay_baka_fighter_801d3f44.txt` for the combat
//! tick):
//!
//! * **[`DuelCamera`]** - the arena camera. The round setup (cabinet state
//!   `0x32`, `0x801CFF34..0x801CFF7C`) snaps the ten camera globals to
//!   [`ROUND_SETUP_CAMERA`]; state `0x35` (`0x801D0324..0x801D0454`) spins the
//!   yaw by `dt << 6` a frame while raising the eye trio's Y by `dt << 2` and
//!   Z by `dt << 5`, and once the yaw passes `0x1000` it zeroes the yaw and
//!   hands the eye to a camera-relative glide ([`SWEEP_GLIDE`]) that settles
//!   it at `(_, 0x898, 0x3520)`. That settled pose is the camera the duel
//!   (state `0x64`) fights under - nothing in the duel state writes a camera
//!   global. A special commit arms a second glide from the combat tick
//!   (`0x801D4644..0x801D4740`): the player's row of the table at
//!   [`SPECIAL_CAMERA_TABLE_VA`], or [`OPPONENT_SPECIAL_GLIDE`] for the
//!   opponent. Both glides are the SCUS camera-relative glide family
//!   (`FUN_80021248` normalizes, `FUN_8002149C` walks), run here through
//!   [`legaia_engine_vm::camera_rel_actor`] and
//!   [`legaia_engine_vm::camera_rel_glide`]. The world is drawn through the
//!   field view build `FUN_800172C0` with the base matrix at `0x6000` (6x,
//!   captured in the parked `minigame_baka_fighter` state along with
//!   `H = 512`), so [`DuelCamera::view`] divides the eye trio by
//!   [`DUEL_WORLD_SCALE`] - the rule `renderer.md` derives.
//! * **[`FighterMotion`]** - each fighter's display clip: the action record
//!   the actor's `+0x5C` names and the cursor `+0x68` the clip selector
//!   `FUN_800204F8` advances. [`crate::baka_fighter::BakaFight`] owns one per
//!   seat and steps it every tick.

use crate::baka_fighter::ClipHeader;
use legaia_asset::baka_opponents as bo;
use legaia_engine_vm::camera_rel_actor::normalize_camera_relative_params;
use legaia_engine_vm::camera_rel_glide::CameraRelGlide;
use legaia_engine_vm::psx_camera::FieldCameraView;
use legaia_engine_vm::retail_cam::RetailCamGlobals;

// ---------------------------------------------------------------- camera

/// The duel's base-matrix scale: `_DAT_8007BF10` reads `0x6000` on the
/// diagonal in the parked Baka Fighter state (GTE `0x1000` = 1.0).
pub const DUEL_WORLD_SCALE: f32 = 6.0;

/// The camera globals the round setup (cabinet state `0x32`) stores:
/// pitch `0`, yaw `0x2F8`, roll `0` (`0x801CFF34..0x801CFF48`), focus zero
/// (`0x801CFF4C..0x801CFF64`), eye trio `(0xC8, 0x708, 0x1FE0)`
/// (`0x801CFF68..0x801CFF7C`). `H` is the duel init's `0x200`
/// (`FUN_801CF00C`, `sh v0,-0x490c(v1)` at `0x801CF080`).
pub const ROUND_SETUP_CAMERA: RetailCamGlobals =
    RetailCamGlobals([0, 0x2F8, 0, 0xC8, 0x708, 0x1FE0, 0, 0, 0, 0x200]);

/// State `0x35` holds the spin while the sign-extended yaw is below this
/// (`slti a0,a0,0x1001` at `0x801D03A0`).
pub const SWEEP_END_YAW: i32 = 0x1001;

/// The glide state `0x35` arms once the spin ends (`0x801D03D0..0x801D0454`):
/// the angles and the eye X parked (step `0`), eye Y to `0x898` at `0x1E`,
/// eye Z to `0x3520` at `0xC8`, focus and `H` parked. `(step, target)` pairs
/// in the camera-relative record order ([`legaia_engine_vm::camera_rel_glide`]).
pub const SWEEP_GLIDE: [i16; 20] = [
    0, 0, 0, 0, 0, 0, // angles
    0, 0, 0x1E, 0x898, 0xC8, 0x3520, // eye trio
    0, 0, 0, 0, 0, 0, // focus
    0, 0, // H
];

/// The camera the duel settles on: [`ROUND_SETUP_CAMERA`] after the spin and
/// [`SWEEP_GLIDE`]. The only axes the sequence leaves changed are yaw `0`
/// and the eye trio's Y / Z.
pub const DUEL_CAMERA: RetailCamGlobals =
    RetailCamGlobals([0, 0, 0, 0xC8, 0x898, 0x3520, 0, 0, 0, 0x200]);

/// The player-select camera state `0x0A` stores as it spawns the lineup:
/// pitch `0x8C`, yaw `0`, roll `0` (`0x801CF8F4..0x801CF910`), eye trio
/// `(0, 0x2D0, 0x3FC0)` (`0x801CF910..0x801CF924`). The state leaves the
/// focus alone; the parked attract state holds it at zero.
pub const SELECT_CAMERA: RetailCamGlobals =
    RetailCamGlobals([0x8C, 0, 0, 0, 0x2D0, 0x3FC0, 0, 0, 0, 0x200]);

/// The player-select lineup, by party roster id: the 8-byte position records
/// state `0x0A` copies into each fighter actor's `+0x14..+0x1B`
/// (`0x801CF97C..0x801CF998`, table `0x801DBC04`) - Vahn front and centre,
/// Noa to his right, Gala to his left, both a step back.
pub const SELECT_LINEUP: [[i16; 3]; 3] = [[0, 0, -1000], [350, 0, -600], [-350, 0, -600]];
/// The lineup's yaw (`+0x26 = 0x800`, `0x801CF9DC`): all three face the camera.
pub const SELECT_YAW: i32 = 0x800;
/// Depth-cue level the select tick `FUN_801D3390` gives every fighter but the
/// cursor's (`+0x78 = 0x800`, toward the black colour word): half brightness.
pub const SELECT_DIM_KEEP: f32 = 0.5;

/// The result close-up the tally's end snaps to: yaw `0x3D4`, eye
/// `(0, 0x8FC, 0x1900)`, pitch `0` (`0x801D0A38..0x801D0A70`) - or `0x64`
/// on the secret opponent's variant (`0x801D0FBC..0x801D0FF0`). Focus and
/// `H` are left as the duel had them.
pub const RESULT_CAMERA: [i32; 6] = [0, 0x3D4, 0, 0, 0x8FC, 0x1900];
/// The secret variant's pitch.
pub const RESULT_SECRET_PITCH: i32 = 0x64;
/// The fighter the result close-up steps forward: the player actor's `+0x5A`
/// is compared with `1` (`0x801D0A74..0x801D0A90`) - the party's second
/// fighter, Noa.
pub const RESULT_STEP_FIGHTER: usize = 1;
/// How far that step goes, on the actor's `+0x18` (Z).
pub const RESULT_STEP_Z: i32 = 0x3C;

/// The opponent's special-commit glide (`0x801D46CC..0x801D4704`): pitch to
/// `-0x14` at `2`, yaw to `0xA8C` at `0x14`, eye to `(-0x3C, 0x80C, 0x2120)`
/// at `(3, 0x22, 0x41)`.
pub const OPPONENT_SPECIAL_GLIDE: [i16; 20] = [
    2, -0x14, 0x14, 0xA8C, 0, 0, //
    3, -0x3C, 0x22, 0x80C, 0x41, 0x2120, //
    0, 0, 0, 0, 0, 0, //
    0, 0,
];

/// Runtime VA of the player's special-commit camera table: `0x20`-byte rows
/// indexed by the fighter actor's `+0x5A` (`sll v1,s8,0x5` at `0x801D4654`),
/// each four 8-byte rows - angle targets, angle steps, eye targets, eye
/// steps (`0x801D464C..0x801D46C4`).
pub const SPECIAL_CAMERA_TABLE_VA: u32 = 0x801D_7DC8;

/// Rows of that table the disc populates - one per party fighter; the fourth
/// row onward is not a camera record.
pub const SPECIAL_CAMERA_ROWS: usize = 3;

/// Read the player's special-commit glides out of the as-loaded overlay
/// (PROT 0976 at [`bo::BAKA_OVERLAY_BASE_VA`]).
pub fn parse_special_cameras(overlay: &[u8]) -> Vec<[i16; 20]> {
    let Some(base) = SPECIAL_CAMERA_TABLE_VA
        .checked_sub(bo::BAKA_OVERLAY_BASE_VA)
        .map(|o| o as usize)
    else {
        return Vec::new();
    };
    let half = |off: usize| -> Option<i16> {
        overlay
            .get(off..off + 2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
    };
    (0..SPECIAL_CAMERA_ROWS)
        .map_while(|row| {
            let r = base + row * 0x20;
            let h = |i: usize| half(r + i * 2);
            let mut rec = [0i16; 20];
            for axis in 0..3 {
                // angles: step at +8, target at +0
                rec[axis * 2] = h(4 + axis)?;
                rec[axis * 2 + 1] = h(axis)?;
                // eye trio: step at +0x18, target at +0x10
                rec[6 + axis * 2] = h(12 + axis)?;
                rec[6 + axis * 2 + 1] = h(8 + axis)?;
            }
            Some(rec)
        })
        .collect()
}

/// The arena camera: the ten retail globals plus the one camera-relative
/// glide the duel can have live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuelCamera {
    globals: RetailCamGlobals,
    glide: Option<CameraRelGlide>,
    /// Cabinet state `0x35`'s spin is running.
    sweeping: bool,
}

impl Default for DuelCamera {
    fn default() -> Self {
        let mut c = Self {
            globals: ROUND_SETUP_CAMERA,
            glide: None,
            sweeping: false,
        };
        c.round_setup();
        c
    }
}

impl DuelCamera {
    /// The round setup's snap, which also starts the round-start spin.
    ///
    /// REF: FUN_801CF388 (state `0x32`, `0x801CFF34..0x801CFF7C`)
    pub fn round_setup(&mut self) {
        self.globals = ROUND_SETUP_CAMERA;
        self.glide = None;
        self.sweeping = true;
    }

    /// The player-select camera ([`SELECT_CAMERA`]), held still.
    ///
    /// REF: FUN_801CF388 (state `0x0A`, `0x801CF8CC..0x801CF924`)
    pub fn select_screen(&mut self) {
        self.globals = SELECT_CAMERA;
        self.glide = None;
        self.sweeping = false;
    }

    /// The tally's end: snap to [`RESULT_CAMERA`] and drop any glide.
    ///
    /// REF: FUN_801CF388 (states `0x66` / `0x6D`)
    pub fn result_close_up(&mut self, secret: bool) {
        let g = &mut self.globals.0;
        g[..6].copy_from_slice(&RESULT_CAMERA);
        if secret {
            g[0] = RESULT_SECRET_PITCH;
        }
        self.glide = None;
        self.sweeping = false;
    }

    /// Hand the camera to a camera-relative glide record (`(step, target)`
    /// pairs), normalized against the live globals exactly as `FUN_80021248`
    /// does. A new glide supersedes a live one (the spawner flags the
    /// previous family actor for retirement).
    pub fn arm_glide(&mut self, record: &[i16; 20]) {
        let n = normalize_camera_relative_params(record, &self.globals.camera_snapshot());
        self.glide = Some(CameraRelGlide::from_normalized(&n));
    }

    /// One frame: the spin while it runs, then the live glide.
    ///
    /// REF: FUN_801CF388 (state `0x35`, `0x801D0324..0x801D0454`)
    pub fn tick(&mut self, frame_step: i32) {
        let dt = frame_step.max(0);
        if self.sweeping {
            let g = &mut self.globals.0;
            g[1] = i32::from((g[1] as i16).wrapping_add((dt << 6) as i16));
            g[4] = g[4].wrapping_add(dt << 2);
            g[5] = g[5].wrapping_add(dt << 5);
            if g[1] >= SWEEP_END_YAW {
                g[1] = 0;
                self.sweeping = false;
                self.arm_glide(&SWEEP_GLIDE);
            }
        }
        if let Some(glide) = self.glide.as_mut() {
            let done = glide.tick(&mut self.globals, dt.min(255) as u8).finished;
            if done {
                self.glide = None;
            }
        }
    }

    /// The live ten globals.
    pub fn globals(&self) -> RetailCamGlobals {
        self.globals
    }

    /// Whether a spin or glide is still moving the camera.
    pub fn moving(&self) -> bool {
        self.sweeping || self.glide.is_some()
    }

    /// The pose a host projects through, at the 1x world scale the duel
    /// vertices are in (the eye trio divided by [`DUEL_WORLD_SCALE`]).
    pub fn view(&self) -> FieldCameraView {
        let g = self.globals;
        let rad = |a: i32| (a as i16) as f32 / 4096.0 * std::f32::consts::TAU;
        let f = g.focus_world();
        let tr = g.tr_eye();
        FieldCameraView {
            focus: [f[0] as f32, f[1] as f32, f[2] as f32],
            pitch: rad(g.0[0]),
            yaw: rad(g.0[1]),
            roll: rad(g.0[2]),
            h: g.h() as f32,
            tr_eye: [
                tr[0] as f32 / DUEL_WORLD_SCALE,
                tr[1] as f32 / DUEL_WORLD_SCALE,
                tr[2] as f32 / DUEL_WORLD_SCALE,
            ],
        }
    }

    /// The **retail** eye-space depth of a world point (the `MAC3` the GTE
    /// leaves, 6x scale): what the wall draw's cull compares.
    pub fn eye_depth(&self, p: [f32; 3]) -> f32 {
        self.view().eye_space(p)[2] * DUEL_WORLD_SCALE
    }

    /// The column-major view-projection a host multiplies a raw (Y-down)
    /// world vertex by: [`FieldCameraView::vp`] times the Y negation it
    /// expects of a model matrix. Both hosts upload exactly this.
    pub fn vp_raw(&self, aspect: f32) -> [f32; 16] {
        legaia_engine_vm::psx_camera::mat4_mul(
            &self.view().vp(aspect),
            &legaia_engine_vm::psx_camera::WORLD_FLIP,
        )
    }
}

// ---------------------------------------------------------------- motion

/// Action record of the hit reaction the damage kernel plays on the struck
/// side (`base + 6`, `0x801D3C60..0x801D3C70`).
pub const MOTION_HIT: usize = 5;
/// The knockdown: `base + 6 + 2`, played instead of the hit when the landed
/// keyframe is the special's last (`0x801D3C00..0x801D3C18`).
pub const MOTION_KNOCKDOWN: usize = 7;
/// The win flourish the result state plays (`base + 9`, `0x801D100C`).
pub const MOTION_WIN: usize = bo::ACTION_WIN;

/// One fighter's display clip - the actor's clip id `+0x5C`, cursor `+0x68`
/// and the hold / clip-end bits of `+0x62` the clip selector reads and
/// writes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FighterMotion {
    /// Action record (`+0x5C` minus the fighter's clip base, minus one).
    pub record: usize,
    /// `+0x68`, 1/16-frame fixed point.
    pub cursor: i32,
    /// `+0x62 & 8` - hold the last frame instead of wrapping.
    pub hold: bool,
    /// `+0x62 & 0x100` - the selector reached the clip end on its last run.
    pub ended: bool,
    /// Block `+0x2C` - the knockdown latch that keeps the idle reset off.
    pub down: bool,
    /// The win flourish is showing; no idle reset.
    pub pinned: bool,
    /// `+0x5E` - the clip the selector last bound.
    bound: Option<usize>,
}

impl FighterMotion {
    /// Start `record` from its first frame (retail zeroes `+0x68` at every
    /// clip store: the commit at `0x801D44D8`, the damage kernel at
    /// `0x801D3CA0`).
    pub fn play(&mut self, record: usize, hold: bool) {
        self.record = record;
        self.cursor = 0;
        self.hold = hold;
        self.ended = false;
        self.bound = Some(record);
    }

    /// Back to the looping idle (the round setup's `base + 1` store).
    pub fn idle(&mut self) {
        *self = Self::default();
    }

    /// One combat tick: the idle reset, then the clip selector's advance.
    ///
    /// REF: FUN_801D3F44 (`0x801D411C..0x801D415C` the idle reset,
    /// `0x801D4744..0x801D47E8` the step), FUN_800204F8 (the selector)
    pub fn step(&mut self, speed: i32, divisor: i32, clip: Option<ClipHeader>, frame_step: i32) {
        if self.ended && !self.down && !self.pinned && self.record != 0 {
            self.record = 0;
            self.hold = false;
        }
        if self.bound != Some(self.record) {
            self.cursor = 0;
            self.bound = Some(self.record);
        }
        let raw = speed.wrapping_mul(divisor);
        let step = (if raw < 0 { raw + 7 } else { raw }) >> 3;
        let step = clip.map_or(step, |c| c.selector_step(step));
        self.ended = false;
        self.cursor = self.cursor.wrapping_add(step.wrapping_mul(frame_step));
        if let Some(c) = clip {
            let end = i32::from(c.frames) * 16 - 1;
            if self.cursor >= end {
                self.cursor = if self.hold { end } else { 0 };
                self.ended = true;
            }
        }
    }

    /// The whole clip frame the renderer poses (`cursor >> 4`).
    pub fn frame(&self) -> usize {
        (self.cursor.max(0) >> 4) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_round_spin_settles_on_the_duel_camera() {
        let mut c = DuelCamera::default();
        assert_eq!(c.globals(), ROUND_SETUP_CAMERA);
        for _ in 0..400 {
            c.tick(1);
        }
        assert!(!c.moving(), "spin and glide both finish");
        assert_eq!(c.globals(), DUEL_CAMERA);
    }

    #[test]
    fn the_spin_stops_past_a_full_turn() {
        let mut c = DuelCamera::default();
        let mut frames = 0;
        while c.sweeping {
            c.tick(1);
            frames += 1;
        }
        // (0x1001 - 0x2F8) / 0x40 rounded up.
        assert_eq!(frames, (SWEEP_END_YAW - 0x2F8 + 0x3F) / 0x40);
        assert_eq!(c.globals().0[1], 0);
    }

    #[test]
    fn an_opponent_special_glides_to_its_fixed_pose() {
        let mut c = DuelCamera::default();
        for _ in 0..400 {
            c.tick(1);
        }
        c.arm_glide(&OPPONENT_SPECIAL_GLIDE);
        for _ in 0..600 {
            c.tick(1);
        }
        let g = c.globals().0;
        assert_eq!((g[0] as i16, g[1] as i16), (-0x14, 0xA8C));
        assert_eq!([g[3], g[4], g[5]], [-0x3C, 0x80C, 0x2120]);
    }

    #[test]
    fn motion_plays_an_attack_out_then_idles() {
        let clip = ClipHeader::from_record_words(0, 4, 1);
        let mut m = FighterMotion::default();
        m.play(1, false);
        let mut ticks = 0;
        while m.record == 1 {
            m.step(16, 8, Some(clip), 1);
            ticks += 1;
            assert!(ticks < 100);
        }
        // 4 frames at a whole frame per tick: the selector reports the end on
        // the 4th step, the idle reset lands on the 5th.
        assert_eq!(ticks, 5);
        assert_eq!(m.record, 0);
    }

    #[test]
    fn a_held_knockdown_stays_down() {
        let clip = ClipHeader::from_record_words(0, 3, 1);
        let mut m = FighterMotion::default();
        m.play(MOTION_KNOCKDOWN, true);
        m.down = true;
        for _ in 0..20 {
            m.step(16, 8, Some(clip), 1);
        }
        assert_eq!(m.record, MOTION_KNOCKDOWN);
        assert_eq!(m.frame(), 2, "held on the last frame");
    }

    #[test]
    fn special_camera_rows_read_targets_then_steps() {
        let mut img = vec![0u8; 0xA000];
        let base = (SPECIAL_CAMERA_TABLE_VA - bo::BAKA_OVERLAY_BASE_VA) as usize;
        let row: [i16; 16] = [-2, 300, 5, 0, 1, 7, 3, 0, -40, 900, 4000, 0, 2, 11, 21, 0];
        for (i, v) in row.iter().enumerate() {
            img[base + i * 2..base + i * 2 + 2].copy_from_slice(&v.to_le_bytes());
        }
        let rows = parse_special_cameras(&img);
        assert_eq!(rows.len(), SPECIAL_CAMERA_ROWS);
        assert_eq!(
            &rows[0][..12],
            &[1, -2, 7, 300, 3, 5, 2, -40, 11, 900, 21, 4000]
        );
    }
}
