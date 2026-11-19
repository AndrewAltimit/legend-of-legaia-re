//! The smooth battle camera (port enhancement, `WorldToggles::smooth_camera`):
//! what the hosts **draw** a battle with on the vsyncs between two battle
//! frames.
//!
//! Retail runs the whole battle pass once a battle frame - the camera walker
//! `FUN_8002149C`, the anim tick `FUN_80047430` with its root motion, the
//! cast modules - and the frame then waits out its step of `2`..`4` vsyncs
//! (`FUN_80016B6C`), so a battle glide or an approach draws 15 to 30 poses a
//! second. Unlike a field glide ([`legaia_engine_field::camera::Camera::led_delta`])
//! the battle camera has no fixed future pose: every pass re-arms its tween
//! from the live actor records. So the draw runs one battle frame behind
//! instead: [`World::observe_battle_present`] keeps the state the last
//! battle frame closed on beside the live one, and the draw accessors
//! ([`World::battle_draw_pose`], [`World::battle_draw_root`]) step from the
//! one to the other by the share of the current frame's vsyncs already shown.
//! The camera and every body root move on the same phase, so a framed body
//! holds still on screen as it does in retail.
//!
//! Presentation-only: the simulation, the camera ghost pass, the tint pass's
//! latch, the census and every `rand` draw read [`World::battle_cam_pose`]
//! and the actor records, never these. Off (`pin_retail`), on the per-vsync
//! cadence, or outside battle the accessors return the live values, bit for
//! bit.

use super::*;
use legaia_engine_camera::projection::BattleCamPose;
use legaia_engine_mode::mode::SceneMode;

/// A camera step at least this far in either angle (12-bit units) over one
/// battle frame is a cut, drawn at once. The ease-out's opening steps of a
/// glide stay under it (about `450` at step `3` across the film sets); the
/// cast modules' cuts run `512` and up.
pub const CUT_ANGLE: f32 = 480.0;
/// A camera translation step at least this long (eye-space units) is a cut.
pub const CUT_TR: f32 = 1800.0;
/// A camera focus step at least this long (raw battle units) is a cut.
pub const CUT_FOCUS: f32 = 480.0;
/// A body root step at least this long (raw battle units) over one battle
/// frame is a placement, not a move: the body is drawn where it landed.
pub const SNAP_ROOT: f32 = 1024.0;
/// A facing turn at least this large (12-bit units) is a turn-about, drawn at
/// once.
pub const SNAP_FACING: f32 = 1024.0;

/// Which camera owns the frame: a cut between owners is a snap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum CamSource {
    #[default]
    Boot,
    Phase,
    Stage,
}

/// One body's root: position `(x, y, z)` and facing (12-bit units).
type Root = [f32; 4];

/// The draw-side state of one tick.
#[derive(Debug, Clone, Default)]
struct PresentSnap {
    source: CamSource,
    /// The camera pose without the screen shake.
    pose: Option<BattleCamPose>,
    /// Every actor slot's root, `None` for an inactive slot.
    roots: Vec<Option<Root>>,
}

/// The smooth battle camera's two states and the phase between them
/// ([`World::observe_battle_present`]).
#[derive(Debug, Clone, Default)]
pub struct BattlePresent {
    /// The state the last battle frame closed on (with every cut since
    /// folded in, so a cut draws its landing at once).
    prev: PresentSnap,
    /// The state the latest tick left.
    cur: PresentSnap,
    /// The battle frame `cur` belongs to; `None` while inactive.
    frame: Option<u64>,
    /// The display tick that frame opened on.
    open_tick: u64,
    /// The last observed display tick.
    last_tick: u64,
    /// The share of the current frame already shown, `0.0..=1.0`.
    phase: f32,
}

/// `d` folded onto the short way round a 12-bit turn.
fn wrap_angle(d: f32) -> f32 {
    (d + 2048.0).rem_euclid(4096.0) - 2048.0
}

fn dist(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f32>()
        .sqrt()
}

/// Whether the step `from` -> `to` is a cut ([`CUT_ANGLE`], [`CUT_TR`],
/// [`CUT_FOCUS`]).
pub fn is_camera_cut(from: &BattleCamPose, to: &BattleCamPose) -> bool {
    wrap_angle(to.pitch - from.pitch).abs() >= CUT_ANGLE
        || wrap_angle(to.yaw - from.yaw).abs() >= CUT_ANGLE
        || dist(&to.tr, &from.tr) >= CUT_TR
        || dist(&to.focus, &from.focus) >= CUT_FOCUS
}

impl BattlePresent {
    fn reset(&mut self) {
        self.frame = None;
        self.phase = 1.0;
        self.prev.pose = None;
        self.cur.pose = None;
    }

    /// The share of the old frame still to draw, `None` when the draw is the
    /// live state.
    fn lag(&self) -> Option<f32> {
        (self.frame.is_some() && self.phase < 1.0).then_some(1.0 - self.phase)
    }

    /// The camera's carry back toward the frame before: `(pitch, yaw, tr,
    /// focus)` deltas the draw subtracts `lag` of from the live pose.
    fn camera_delta(&self) -> Option<BattleCamPose> {
        let (Some(p), Some(c)) = (self.prev.pose, self.cur.pose) else {
            return None;
        };
        Some(BattleCamPose {
            pitch: wrap_angle(c.pitch - p.pitch),
            yaw: wrap_angle(c.yaw - p.yaw),
            tr: std::array::from_fn(|i| c.tr[i] - p.tr[i]),
            focus: std::array::from_fn(|i| c.focus[i] - p.focus[i]),
        })
    }

    /// Slot `i`'s root carry, `None` when the slot draws live.
    fn root_delta(&self, i: usize) -> Option<Root> {
        let p = (*self.prev.roots.get(i)?)?;
        let c = (*self.cur.roots.get(i)?)?;
        Some([
            c[0] - p[0],
            c[1] - p[1],
            c[2] - p[2],
            wrap_angle(c[3] - p[3]),
        ])
    }
}

impl World {
    /// The phase camera's pose without its shake, the shake, and the owner.
    fn battle_present_camera(&self) -> (CamSource, BattleCamPose, [f32; 2]) {
        if self.battle.stage_camera.is_some() {
            return (CamSource::Stage, self.battle_cam_pose(), [0.0; 2]);
        }
        match self.battle.camera.as_ref() {
            Some(c) => {
                let shaken = c.pose();
                let framing = c.framing_pose();
                (
                    CamSource::Phase,
                    framing,
                    [shaken.tr[0] - framing.tr[0], shaken.tr[1] - framing.tr[1]],
                )
            }
            None => (CamSource::Boot, self.battle_cam_pose(), [0.0; 2]),
        }
    }

    /// Record what this tick left for the smooth battle camera's draw
    /// ([`BattlePresent`]). Runs at the end of every [`World::tick`]; reads
    /// the world and writes only [`World::battle_present`].
    pub fn observe_battle_present(&mut self) {
        let active = self.toggles.smooth_camera
            && self.toggles.battle_sm_frame_cadence
            && self.mode == SceneMode::Battle;
        if !active {
            self.battle_present.reset();
            return;
        }
        let now = self.clock.display_frames;
        let frame = self.battle_frame_id();
        let span = self.battle.frame_clock.span().max(1);
        let (source, pose, _) = self.battle_present_camera();
        let mut bp = std::mem::take(&mut self.battle_present);
        let fresh = bp.frame.is_none() || now.saturating_sub(bp.last_tick) > span;
        if bp.frame != Some(frame) {
            std::mem::swap(&mut bp.prev, &mut bp.cur);
            bp.open_tick = now;
        }
        bp.cur.source = source;
        bp.cur.pose = Some(pose);
        bp.cur.roots.clear();
        bp.cur.roots.extend(self.actors.iter().map(|a| {
            a.active.then(|| {
                [
                    f32::from(a.move_state.world_x),
                    f32::from(a.move_state.world_y),
                    f32::from(a.move_state.world_z),
                    f32::from(a.battle.facing_angle & 0xFFF),
                ]
            })
        }));
        if fresh {
            bp.prev.clone_from(&bp.cur);
        } else {
            // A cut, or a change of owner, draws its landing at once.
            let cut = bp.prev.source != bp.cur.source
                || bp.prev.pose.is_none_or(|p| is_camera_cut(&p, &pose));
            if cut {
                bp.prev.source = bp.cur.source;
                bp.prev.pose = bp.cur.pose;
            }
            // So does a body placed rather than moved, or turned about.
            bp.prev.roots.resize(bp.cur.roots.len(), None);
            for (p, c) in bp.prev.roots.iter_mut().zip(&bp.cur.roots) {
                let snap = match (*p, *c) {
                    (Some(p), Some(c)) => {
                        dist(&p[..3], &c[..3]) >= SNAP_ROOT
                            || wrap_angle(c[3] - p[3]).abs() >= SNAP_FACING
                    }
                    _ => true,
                };
                if snap {
                    *p = *c;
                }
            }
        }
        bp.frame = Some(frame);
        bp.last_tick = now;
        bp.phase = (now.saturating_sub(bp.open_tick) as f32 / span as f32).clamp(0.0, 1.0);
        self.battle_present = bp;
    }

    /// The battle camera pose this vsync **draws** with: the live pose
    /// ([`World::battle_cam_pose`]) carried back toward the last frame's by
    /// the share of the frame still to show, under the smooth camera; the
    /// live pose itself otherwise. The shake stays the live one. A draw-side
    /// read only: nothing the simulation runs takes it.
    pub fn battle_draw_pose(&self) -> BattleCamPose {
        let live = self.battle_cam_pose();
        let (Some(lag), Some(d)) = (
            self.battle_present.lag(),
            self.battle_present.camera_delta(),
        ) else {
            return live;
        };
        BattleCamPose {
            pitch: live.pitch - lag * d.pitch,
            yaw: live.yaw - lag * d.yaw,
            tr: std::array::from_fn(|i| live.tr[i] - lag * d.tr[i]),
            focus: std::array::from_fn(|i| live.focus[i] - lag * d.focus[i]),
        }
    }

    /// Battle body `slot`'s root this vsync **draws** at: `([x, y, z],
    /// facing)` in raw battle units and 12-bit facing units, on the same
    /// phase as [`Self::battle_draw_pose`]; the live root otherwise (the
    /// facing masked to `0..4096`). `None` past the actor table.
    pub fn battle_draw_root(&self, slot: usize) -> Option<([f32; 3], f32)> {
        let a = self.actors.get(slot)?;
        let mut pos = [
            f32::from(a.move_state.world_x),
            f32::from(a.move_state.world_y),
            f32::from(a.move_state.world_z),
        ];
        let mut facing = f32::from(a.battle.facing_angle & 0xFFF);
        if let (Some(lag), Some(d)) = (
            self.battle_present.lag(),
            self.battle_present.root_delta(slot),
        ) {
            for (p, d) in pos.iter_mut().zip(d) {
                *p -= lag * d;
            }
            facing -= lag * d[3];
        }
        Some((pos, facing))
    }
}

impl World {
    /// How far [`Self::battle_draw_root`] carries slot `slot`'s position off
    /// its live trio, `[0.0; 3]` when it draws live: the shift a draw-side
    /// pass anchored on the body (its ground shadow, its weapon trail) adds
    /// to stay on the drawn body.
    pub fn battle_draw_shift(&self, slot: usize) -> [f32; 3] {
        let (Some(lag), Some(d)) = (
            self.battle_present.lag(),
            self.battle_present.root_delta(slot),
        ) else {
            return [0.0; 3];
        };
        [-lag * d[0], -lag * d[1], -lag * d[2]]
    }

    /// Slot `slot`'s drawn root on the integer grid the retail-arithmetic
    /// passes take: `([x, z], facing)`, rounded. The live record's own
    /// values when it draws live.
    pub fn battle_draw_root_i16(&self, slot: usize) -> Option<([i16; 2], u16)> {
        let a = self.actors.get(slot)?;
        if self.battle_present.lag().is_none() {
            return Some((
                [a.move_state.world_x, a.move_state.world_z],
                a.battle.facing_angle,
            ));
        }
        let (p, f) = self.battle_draw_root(slot)?;
        Some((
            [p[0].round() as i16, p[2].round() as i16],
            (f.round() as i32).rem_euclid(4096) as u16,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn battle_world() -> World {
        let mut w = World::new();
        w.mode = SceneMode::Battle;
        w.toggles.battle_sm_frame_cadence = true;
        w.toggles.smooth_camera = true;
        w.battle.stage_camera = Some(legaia_engine_battle::battle_stage_module::StageCamera {
            pitch: 32,
            yaw: 100,
            tr: [0, 1280, 7680],
            focus: [0, 0, 0],
        });
        w.set_battle_frame_step(3);
        w.advance_battle_pass();
        w
    }

    /// Step the display clock one vsync and observe, as a tick does.
    fn vsync(w: &mut World) {
        w.clock.display_frames += 1;
        w.observe_battle_present();
    }

    fn set_yaw(w: &mut World, yaw: i32) {
        w.battle.stage_camera.as_mut().unwrap().yaw = yaw;
    }

    /// Across a frame of three vsyncs the draw walks from the old frame's
    /// pose to the new one a third at a time, while the live pose has
    /// already landed.
    #[test]
    fn the_draw_walks_one_frame_behind() {
        let mut w = battle_world();
        w.observe_battle_present();
        // Run out the opening frame.
        while w.battle_present.phase > 0.0 {
            vsync(&mut w);
        }
        for _ in 0..3 {
            vsync(&mut w);
        }
        assert_eq!(w.battle_present.phase, 0.0, "a frame opens");
        set_yaw(&mut w, 190);
        w.observe_battle_present();
        let mut drawn = vec![w.battle_draw_pose().yaw];
        for _ in 0..2 {
            vsync(&mut w);
            drawn.push(w.battle_draw_pose().yaw);
        }
        assert_eq!(drawn, [100.0, 130.0, 160.0]);
        assert_eq!(w.battle_cam_pose().yaw, 190.0, "the simulation's pose");
        vsync(&mut w);
        assert_eq!(
            w.battle_draw_pose().yaw,
            190.0,
            "the next frame opens on it"
        );
    }

    /// A yaw across the 12-bit wrap moves the short way round.
    #[test]
    fn the_yaw_wraps_the_short_way() {
        let mut w = battle_world();
        set_yaw(&mut w, 4090);
        w.observe_battle_present();
        while w.battle_present.phase > 0.0 {
            vsync(&mut w);
        }
        for _ in 0..3 {
            vsync(&mut w);
        }
        set_yaw(&mut w, 4096 + 30);
        w.observe_battle_present();
        vsync(&mut w);
        let y = w.battle_draw_pose().yaw;
        assert!((y - (4126.0 - 24.0)).abs() < 1e-3, "{y}");
    }

    /// A cut draws its landing at once; so does a body placed across the
    /// stage, while one that walks is carried with the camera.
    #[test]
    fn cuts_and_placements_snap() {
        let mut w = battle_world();
        w.actors[0].active = true;
        w.actors[1].active = true;
        w.observe_battle_present();
        while w.battle_present.phase > 0.0 {
            vsync(&mut w);
        }
        for _ in 0..3 {
            vsync(&mut w);
        }
        set_yaw(&mut w, 100 + 1024);
        w.actors[0].move_state.world_x = 90;
        w.actors[1].move_state.world_x = 3000;
        w.observe_battle_present();
        vsync(&mut w);
        assert_eq!(w.battle_draw_pose().yaw, 1124.0);
        assert!((w.battle_draw_root(0).unwrap().0[0] - 30.0).abs() < 1e-3);
        assert_eq!(w.battle_draw_root(1).unwrap().0[0], 3000.0);
    }

    /// Retail mode, the per-vsync cadence and the field draw the live state.
    #[test]
    fn off_draws_the_live_state() {
        for flip in 0..3 {
            let mut w = battle_world();
            match flip {
                0 => w.toggles.pin_retail(),
                1 => w.toggles.battle_sm_frame_cadence = false,
                _ => w.mode = SceneMode::Field,
            }
            w.observe_battle_present();
            for _ in 0..3 {
                vsync(&mut w);
            }
            set_yaw(&mut w, 300);
            w.observe_battle_present();
            vsync(&mut w);
            assert_eq!(w.battle_draw_pose(), w.battle_cam_pose());
        }
    }
}
