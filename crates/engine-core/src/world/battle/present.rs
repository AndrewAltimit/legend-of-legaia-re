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

/// One part of a body's pose: translation and rotation trio (12-bit units),
/// as `PoseFrame::bone_outputs` holds it.
type Bone = ([i16; 3], [i16; 3]);

/// A part translation step at least this long, or a rotation step at least
/// [`SNAP_PART_ANGLE`], draws the part where it landed.
pub const SNAP_PART_MOVE: i32 = 512;
/// See [`SNAP_PART_MOVE`].
pub const SNAP_PART_ANGLE: i32 = 1024;

/// The draw-side state of one tick.
#[derive(Debug, Clone, Default)]
struct PresentSnap {
    source: CamSource,
    /// The camera pose without the screen shake.
    pose: Option<BattleCamPose>,
    /// Every actor slot's root, `None` for an inactive slot.
    roots: Vec<Option<Root>>,
    /// Every actor slot's pose (`Actor::pose_frame`), empty for none.
    poses: Vec<Vec<Bone>>,
    /// Every live effect node, sorted by key ([`FxRec`]).
    fx: Vec<FxRec>,
}

/// One live effect node as the draw saw it: a move-VM scene part
/// ([`fx_node`]) or an effect-pool child ([`FX_POOL_CHILD`]).
#[derive(Debug, Clone, Copy, PartialEq)]
struct FxRec {
    key: u32,
    /// Its identity: the part's model selector, or the child's anim.
    kind: i32,
    /// Its age, which only climbs while the node lives: the scene's frame
    /// counter, or the child's frame cursor. A lower age under the same key
    /// is a new node in a reused slot.
    age: u32,
    pos: [f32; 3],
    /// Rotation trio, 12-bit units (`+0x24` / `+0x26` / `+0x28`).
    rot: [f32; 3],
}

/// The scene keys [`fx_node`] tags a part's node with: the summon creature's
/// scene, the move-FX scene, the Mystic shield's, a stage module's, and each
/// action-FX scene from [`FX_SCENE_ACTION`] on, in the order the draws list
/// them.
pub const FX_SCENE_SUMMON: u32 = 1;
/// See [`FX_SCENE_SUMMON`].
pub const FX_SCENE_MOVE: u32 = 2;
/// See [`FX_SCENE_SUMMON`].
pub const FX_SCENE_MYSTIC: u32 = 3;
/// See [`FX_SCENE_SUMMON`].
pub const FX_SCENE_STAGE: u32 = 4;
/// See [`FX_SCENE_SUMMON`].
pub const FX_SCENE_ACTION: u32 = 0x10;
/// The key an effect-pool child's node carries above its slot.
pub const FX_POOL_CHILD: u32 = 0xFFFF;
/// The key a cast arm's ribbon ring point carries above its index
/// (`BattleState::cast_ribbons`), and [`FX_FLIGHT_RIBBON`] a landed move
/// projectile's (`BattleState::flight_ribbons`): the anchors
/// `FUN_801E1D98` draws its streak ribbon on, which ride the bodies and
/// projectiles they were taken from.
pub const FX_CAST_RIBBON: u32 = 0xFFFE;
/// See [`FX_CAST_RIBBON`].
pub const FX_FLIGHT_RIBBON: u32 = 0xFFFD;

/// A step against the frame before's ([`World::battle_fx_shift`]'s wrap
/// test) at least this long, and at least twice as long as that step plus
/// this, is a looping node going back to its start, drawn where it landed.
pub const WRAP_MIN_STEP: f32 = 32.0;

/// A scene part's node: the scene key over the part's index.
pub fn fx_node(scene: u32, part: u32) -> u32 {
    (scene << 16) | (part & 0xFFFF)
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
    /// The effect nodes of the frame before [`Self::prev`]'s, for the wrap
    /// test ([`World::battle_fx_shift`]): a node's last step tells a looping
    /// node's jump back to its start from a move.
    pprev_fx: Vec<FxRec>,
    /// The battle frame `cur` belongs to; `None` while inactive.
    frame: Option<u64>,
    /// The display tick that frame opened on.
    open_tick: u64,
    /// The last observed display tick.
    last_tick: u64,
    /// The share of the current frame already shown, `0.0..=1.0`.
    phase: f32,
    /// Each slot's drawn pose for this tick
    /// ([`World::battle_draw_pose_frame`]).
    drawn: Vec<Option<legaia_anm::PoseFrame>>,
}

/// Whether a node that stood at `q`, then `p`, then `c` on three battle
/// frames went back to its start on the last: the step `p -> c` turns
/// against the step `q -> p` and is at least twice as long, plus
/// [`WRAP_MIN_STEP`]. A looping particle (a spark that rises and drops back
/// to its emitter, a ring point that runs round and re-seats) steps steadily
/// one way and then jumps the other; drawn on the line between, it would
/// sweep back across its path for the frame. A bounce turns too but keeps
/// its speed, and a node starting from rest has no last step, so both
/// still step.
pub fn is_wrap(q: [f32; 3], p: [f32; 3], c: [f32; 3]) -> bool {
    let last: [f32; 3] = std::array::from_fn(|k| p[k] - q[k]);
    let step: [f32; 3] = std::array::from_fn(|k| c[k] - p[k]);
    let dot: f32 = (0..3).map(|k| last[k] * step[k]).sum();
    let len = |v: &[f32; 3]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot < 0.0 && len(&step) >= 2.0 * len(&last) + WRAP_MIN_STEP
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
        if self.frame.is_none() {
            return;
        }
        self.frame = None;
        self.phase = 1.0;
        self.prev.pose = None;
        self.cur.pose = None;
        self.prev.roots.clear();
        self.cur.roots.clear();
        self.prev.poses.clear();
        self.cur.poses.clear();
        self.prev.fx.clear();
        self.cur.fx.clear();
        self.pprev_fx.clear();
        self.drawn.clear();
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
            // `cur` now holds the frame before `prev`: its nodes are the
            // wrap test's last step. The buffers swap, so a steady fight
            // allocates nothing here.
            std::mem::swap(&mut bp.pprev_fx, &mut bp.cur.fx);
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
        bp.cur.poses.resize_with(self.actors.len(), Vec::new);
        for (out, a) in bp.cur.poses.iter_mut().zip(&self.actors) {
            out.clear();
            if a.active
                && let Some(p) = a.pose_frame.as_ref()
            {
                out.extend_from_slice(&p.bone_outputs);
            }
        }
        bp.cur.fx.clear();
        for (key, scene) in self.battle_fx_scenes() {
            let age = scene.frame;
            bp.cur.fx.extend(
                scene
                    .parts
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| !p.finished)
                    .map(|(i, p)| {
                        let s = &p.state;
                        FxRec {
                            key: fx_node(key, i as u32),
                            kind: i32::from(p.model_sel),
                            age,
                            pos: [s.world_x, s.world_y, s.world_z].map(f32::from),
                            rot: [s.render_24, s.render_26, s.render_28].map(f32::from),
                        }
                    }),
            );
        }
        bp.cur.fx.extend(
            self.effect_pool
                .children
                .iter()
                .enumerate()
                .filter(|(_, c)| c.frame_count != 0)
                .map(|(i, c)| FxRec {
                    key: fx_node(FX_POOL_CHILD, i as u32),
                    kind: i32::from(c.anim_id),
                    age: u32::from(c.frame_cursor),
                    pos: c.pos.map(|v| f32::from((v >> 8) as i16)),
                    rot: [0.0; 3],
                }),
        );
        // The streak ribbon's anchors: a cast arm's ring and a landed
        // projectile's bolts. A ring whose trail or point count changed is a
        // new ring (`kind`), so no point steps from another's.
        for (scene, ring) in [
            (FX_CAST_RIBBON, self.battle.cast_ribbons),
            (FX_FLIGHT_RIBBON, self.battle.flight_ribbons),
        ] {
            let Some(ring) = ring else {
                continue;
            };
            let kind = i32::from(ring.trail_id) | i32::from(ring.count) << 8;
            bp.cur.fx.extend(
                ring.points
                    .iter()
                    .take(usize::from(ring.count))
                    .enumerate()
                    .map(|(k, p)| FxRec {
                        key: fx_node(scene, k as u32),
                        kind,
                        age: 0,
                        pos: p.map(f32::from),
                        rot: [0.0; 3],
                    }),
            );
        }
        bp.cur.fx.sort_unstable_by_key(|r| r.key);
        if fresh {
            bp.prev.clone_from(&bp.cur);
            bp.pprev_fx.clear();
        } else {
            // A cut, or a change of owner, draws its landing at once.
            let cut = bp.prev.source != bp.cur.source
                || bp.prev.pose.is_none_or(|p| is_camera_cut(&p, &pose));
            if cut {
                bp.prev.source = bp.cur.source;
                bp.prev.pose = bp.cur.pose;
            }
            // So does a body placed rather than moved, or turned about, and
            // its pose with it; a pose whose skeleton changed lands too.
            bp.prev.roots.resize(bp.cur.roots.len(), None);
            bp.prev.poses.resize_with(bp.cur.poses.len(), Vec::new);
            for i in 0..bp.cur.roots.len() {
                let (p, c) = (bp.prev.roots[i], bp.cur.roots[i]);
                let placed = match (p, c) {
                    (Some(p), Some(c)) => {
                        dist(&p[..3], &c[..3]) >= SNAP_ROOT
                            || wrap_angle(c[3] - p[3]).abs() >= SNAP_FACING
                    }
                    _ => true,
                };
                if placed {
                    bp.prev.roots[i] = c;
                }
                if placed || bp.prev.poses[i].len() != bp.cur.poses[i].len() {
                    let (prev, cur) = (&mut bp.prev.poses, &bp.cur.poses);
                    prev[i].clone_from(&cur[i]);
                }
            }
        }
        bp.frame = Some(frame);
        bp.last_tick = now;
        bp.phase = (now.saturating_sub(bp.open_tick) as f32 / span as f32).clamp(0.0, 1.0);
        bp.fill_drawn_poses(&self.actors);
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

impl BattlePresent {
    /// Fill [`Self::drawn`] for this tick: each slot's pose stepped `lag` of
    /// the way back toward the frame before, part by part (a part that moved
    /// past [`SNAP_PART_MOVE`] or turned past [`SNAP_PART_ANGLE`] draws where
    /// it landed), `None` where the slot draws its live pose. The buffers are
    /// kept from tick to tick, so a steady fight allocates nothing here.
    fn fill_drawn_poses(&mut self, actors: &[Actor]) {
        let lag = self.lag();
        self.drawn.resize_with(actors.len(), || None);
        for (i, out) in self.drawn.iter_mut().enumerate() {
            let (Some(lag), Some(p), Some(c), Some(live)) = (
                lag,
                self.prev.poses.get(i),
                self.cur.poses.get(i),
                actors[i].pose_frame.as_ref(),
            ) else {
                *out = None;
                continue;
            };
            if c.is_empty() || p.len() != c.len() || p == c || live.bone_outputs.len() != c.len() {
                *out = None;
                continue;
            }
            let frame = out.get_or_insert_with(|| legaia_anm::PoseFrame {
                bone_outputs: Vec::new(),
                factor: 0,
                finished: false,
            });
            frame.factor = live.factor;
            frame.finished = live.finished;
            frame.bone_outputs.clear();
            let back = |c: i16, d: i32| (f32::from(c) - lag * d as f32).round() as i16;
            frame.bone_outputs.extend(p.iter().zip(c).map(|(p, c)| {
                let dt: [i32; 3] = std::array::from_fn(|k| i32::from(c.0[k]) - i32::from(p.0[k]));
                let dr: [i32; 3] = std::array::from_fn(|k| {
                    (i32::from(c.1[k]) - i32::from(p.1[k]) + 2048).rem_euclid(4096) - 2048
                });
                if dt.iter().any(|d| d.abs() >= SNAP_PART_MOVE)
                    || dr.iter().any(|d| d.abs() >= SNAP_PART_ANGLE)
                {
                    return *c;
                }
                (
                    std::array::from_fn(|k| back(c.0[k], dt[k])),
                    std::array::from_fn(|k| back(c.1[k], dr[k])),
                )
            }));
        }
    }
}

impl World {
    /// Battle body `slot`'s pose this vsync **draws**: the live
    /// `pose_frame` carried back toward the last frame's on the same phase
    /// as [`Self::battle_draw_pose`], so a body's parts move with its root
    /// and the camera. `None` when the body draws its live pose (off, on the
    /// vsync a frame opens on its last frame's pose, or no pose).
    pub fn battle_draw_pose_frame(&self, slot: usize) -> Option<&legaia_anm::PoseFrame> {
        self.battle_present.lag()?;
        let drawn = self.battle_present.drawn.get(slot)?.as_ref()?;
        let live = self.actors.get(slot)?.pose_frame.as_ref()?;
        (drawn.bone_outputs.len() == live.bone_outputs.len()).then_some(drawn)
    }
}

impl World {
    /// The move-VM scenes whose parts the battle draws, keyed for
    /// [`fx_node`], in the order the draw lists walk them.
    pub(crate) fn battle_fx_scenes(
        &self,
    ) -> impl Iterator<Item = (u32, &legaia_engine_effects::summon::SummonScene)> {
        let c = &self.casting;
        c.active_summon
            .iter()
            .map(|s| (FX_SCENE_SUMMON, s))
            .chain(c.active_move_fx.iter().map(|s| (FX_SCENE_MOVE, s)))
            .chain(
                c.active_action_fx
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (FX_SCENE_ACTION + i as u32, s)),
            )
            .chain(c.mystic_shield_fx.iter().map(|s| (FX_SCENE_MYSTIC, s)))
            .chain(c.stage_fx.iter().map(|s| (FX_SCENE_STAGE, s)))
    }

    /// How far the smooth battle camera carries effect node `node` (a draw's
    /// `node`, [`fx_node`]) off its live placement this vsync: `(position
    /// shift, rotation shift in radians)`, both added to the draw's own. A
    /// node on the same phase as the bodies and the camera stays on the body
    /// it rides. `None` when it draws live: off, no frame behind, a node
    /// born this frame (it draws where it spawned), a new node in a reused
    /// slot, one moved [`SNAP_ROOT`] in a frame, or a looping node going back
    /// to its start ([`is_wrap`]). A rotation turned [`SNAP_PART_ANGLE`]
    /// lands; the position still steps.
    pub fn battle_fx_shift(&self, node: u32) -> Option<([f32; 3], [f32; 3])> {
        let bp = &self.battle_present;
        let lag = bp.lag()?;
        let find = |v: &[FxRec]| v.binary_search_by_key(&node, |r| r.key).ok().map(|i| v[i]);
        let (p, c) = (find(&bp.prev.fx)?, find(&bp.cur.fx)?);
        if p.kind != c.kind || c.age < p.age || dist(&p.pos, &c.pos) >= SNAP_ROOT {
            return None;
        }
        // The same node one frame further back: its last step.
        if let Some(q) = find(&bp.pprev_fx).filter(|q| q.kind == p.kind && q.age <= p.age)
            && is_wrap(q.pos, p.pos, c.pos)
        {
            return None;
        }
        let dr: [f32; 3] = std::array::from_fn(|k| wrap_angle(c.rot[k] - p.rot[k]));
        let turn = if dr.iter().any(|d| d.abs() >= SNAP_PART_ANGLE as f32) {
            [0.0; 3]
        } else {
            dr.map(|d| -lag * d * std::f32::consts::TAU / 4096.0)
        };
        Some((std::array::from_fn(|k| -lag * (c.pos[k] - p.pos[k])), turn))
    }

    /// [`Self::battle_fx_shift`]'s position half for point `k` of the cast
    /// arm's ribbon ring (`flight = false`) or of a landed projectile's
    /// (`flight = true`), `None` when it draws live.
    pub fn battle_ribbon_point_shift(&self, flight: bool, k: usize) -> Option<[f32; 3]> {
        let scene = if flight {
            FX_FLIGHT_RIBBON
        } else {
            FX_CAST_RIBBON
        };
        self.battle_fx_shift(fx_node(scene, k as u32))
            .map(|(d, _)| d)
    }

    /// [`Self::battle_fx_shift`]'s position half for an effect-pool child
    /// in slot `slot`, `[0.0; 3]` when it draws live.
    pub fn battle_sprite_shift(&self, slot: u16) -> [f32; 3] {
        self.battle_fx_shift(fx_node(FX_POOL_CHILD, u32::from(slot)))
            .map_or([0.0; 3], |(d, _)| d)
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

    /// A body's parts step with its root: a part between two frames draws
    /// part way along, the vsync a frame opens draws the old frame's pose,
    /// and a part that swung past the snap draws where it landed.
    #[test]
    fn the_pose_steps_with_the_camera() {
        let pose = |t: i16, r: i16| legaia_anm::PoseFrame {
            bone_outputs: vec![([t, 0, 0], [0, r, 0]), ([0, 0, 0], [0, 0, 0])],
            factor: 0,
            finished: false,
        };
        let mut w = battle_world();
        w.actors[0].active = true;
        w.actors[0].pose_frame = Some(pose(0, 4090));
        w.observe_battle_present();
        while w.battle_present.phase > 0.0 {
            vsync(&mut w);
        }
        for _ in 0..3 {
            vsync(&mut w);
        }
        w.actors[0].pose_frame = Some(pose(30, 4096 + 26));
        if let Some(p) = w.actors[0].pose_frame.as_mut() {
            p.bone_outputs[1].1[0] = 2000;
        }
        w.observe_battle_present();
        let drawn = w.battle_draw_pose_frame(0).unwrap();
        assert_eq!(drawn.bone_outputs[0], ([0, 0, 0], [0, 4090, 0]));
        assert_eq!(
            drawn.bone_outputs[1].1[0], 2000,
            "a swing past the snap lands"
        );
        vsync(&mut w);
        let drawn = w.battle_draw_pose_frame(0).unwrap();
        assert_eq!(drawn.bone_outputs[0], ([10, 0, 0], [0, 4101, 0]));
        vsync(&mut w);
        vsync(&mut w);
        assert!(
            w.battle_draw_pose_frame(0).is_none(),
            "the next frame opens on it"
        );
    }

    /// An effect-pool child rides the drawn phase: one that moved steps
    /// with the bodies, one born this frame draws where it spawned, and a
    /// new child in a reused slot does not inherit the old one's path.
    #[test]
    fn effect_children_step_with_the_camera() {
        let mut w = battle_world();
        let child = |w: &mut World, x: i32, cursor: u8| {
            let c = &mut w.effect_pool.children[0];
            c.frame_count = 8;
            c.anim_id = 3;
            c.frame_cursor = cursor;
            c.pos = [x << 8, 0, 0];
        };
        child(&mut w, 100, 2);
        w.observe_battle_present();
        while w.battle_present.phase > 0.0 {
            vsync(&mut w);
        }
        for _ in 0..3 {
            vsync(&mut w);
        }
        child(&mut w, 130, 3);
        w.observe_battle_present();
        vsync(&mut w);
        let d = w.battle_sprite_shift(0);
        assert!((d[0] + 20.0).abs() < 1e-3, "{d:?}");
        assert_eq!(w.battle_sprite_shift(1), [0.0; 3], "no child there");
        // A new child in the same slot (its cursor back at the start).
        child(&mut w, 400, 0);
        w.observe_battle_present();
        assert_eq!(w.battle_sprite_shift(0), [0.0; 3]);
    }

    /// Run the current frame out and open the next, observing each vsync.
    fn next_frame(w: &mut World) {
        for _ in 0..8 {
            vsync(w);
            if w.battle_present.phase == 0.0 {
                return;
            }
        }
        panic!("no frame opened");
    }

    /// A looping child that steps steadily up and then drops back to its
    /// start draws where it landed on the frame it wraps, not on the line
    /// back across its path; a steady step and a bounce still step.
    #[test]
    fn a_looping_node_snaps_when_it_wraps() {
        assert!(
            !is_wrap([0.0; 3], [0.0, 40.0, 0.0], [0.0, -10.0, 0.0]),
            "a bounce"
        );
        assert!(!is_wrap([0.0; 3], [0.0, 40.0, 0.0], [0.0, 0.0, 0.0]));
        assert!(is_wrap(
            [0.0, 80.0, 0.0],
            [0.0, 120.0, 0.0],
            [0.0, 0.0, 0.0]
        ));
        assert!(
            !is_wrap([0.0; 3], [0.0, 0.0, 0.0], [0.0, 300.0, 0.0]),
            "from rest"
        );
        let mut w = battle_world();
        let child = |w: &mut World, y: i32, cursor: u8| {
            let c = &mut w.effect_pool.children[0];
            c.frame_count = 200;
            c.anim_id = 3;
            c.frame_cursor = cursor;
            c.pos = [0, y << 8, 0];
        };
        child(&mut w, 0, 1);
        w.observe_battle_present();
        for (k, y) in [40, 80, 120].into_iter().enumerate() {
            next_frame(&mut w);
            child(&mut w, y, 2 + k as u8);
            w.observe_battle_present();
            vsync(&mut w);
            let d = w.battle_sprite_shift(0);
            assert!(d[1] < 0.0, "a steady rise steps: {d:?}");
        }
        // Back to its start: the wrap.
        next_frame(&mut w);
        child(&mut w, 0, 5);
        w.observe_battle_present();
        vsync(&mut w);
        assert_eq!(w.battle_sprite_shift(0), [0.0; 3], "it lands at once");
    }

    /// A cast arm's ribbon ring point steps with the bodies; a ring whose
    /// point count changed draws live.
    #[test]
    fn ribbon_points_step_with_the_camera() {
        let ring = |x: i16, count: u8| legaia_engine_cast::cast_module_camera::CastRibbonRing {
            points: [[x, -192, 0]; 16],
            trail_id: 5,
            count,
        };
        let mut w = battle_world();
        w.battle.cast_ribbons = Some(ring(100, 4));
        w.observe_battle_present();
        next_frame(&mut w);
        w.battle.cast_ribbons = Some(ring(160, 4));
        w.observe_battle_present();
        vsync(&mut w);
        let d = w.battle_ribbon_point_shift(false, 2).unwrap();
        assert!((d[0] + 40.0).abs() < 1e-3, "{d:?}");
        assert!(
            w.battle_ribbon_point_shift(true, 2).is_none(),
            "no flight ring"
        );
        next_frame(&mut w);
        w.battle.cast_ribbons = Some(ring(200, 5));
        w.observe_battle_present();
        vsync(&mut w);
        assert!(
            w.battle_ribbon_point_shift(false, 2).is_none(),
            "a new ring"
        );
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
