//! The smooth field actors (port enhancement, `WorldToggles::smooth_actors`):
//! where the hosts **draw** the field's bodies on the vsyncs between two game
//! frames.
//!
//! Retail runs a field script context, an NPC walk leg and the actor pool's
//! anim tick once a game frame, from the actor-list walk (`FUN_8002519C` ->
//! `FUN_8003BC08`), and the frame then waits out its step of `2`..`4` vsyncs
//! (`FUN_80016B6C`). So a scripted walker - `izumi`'s hunters on their
//! rounds, the player a record walks to a mark - moves its whole step on the
//! vsync a frame opens and stands on the vsyncs between, as does its walk
//! clip: 30 poses a second at step `2`, 15 at step `4`. A PCSX-Redux film of
//! `izumi_hunters_field` (`autorun_field_leg_timing.lua`) has each hunter
//! advance `4` units on every second vsync and hold on the one between.
//!
//! The draw runs one game frame behind instead, as the smooth battle camera
//! does ([`super::BattlePresent`]): [`World::observe_field_present`] keeps
//! the state the last game frame closed on beside the live one, and the draw
//! accessors ([`World::field_draw_root`], [`World::field_npc_drawn_pose`],
//! [`World::field_draw_pose_frame`]) step from the one to the other by the
//! share of the current frame's vsyncs already shown. The field camera's
//! stepped moves are drawn on the same phase
//! (`legaia_engine_field::camera::Camera::observe_present`), and a follow
//! focus pinned onto the player follows the drawn player, so a framed body
//! holds still on screen.
//!
//! What draws live:
//!
//! - a body that moved on a vsync between two game frames (the player under
//!   the smooth walk, which already moves every vsync), for the frame it
//!   moved in and the one after;
//! - a body placed rather than moved: a step of [`SNAP_MOVE`] or more, a
//!   turn of [`SNAP_TURN`] or more, a body that appeared or left the hide
//!   box, and every body on a scene change;
//! - a body's height when it changes floor tier ([`SNAP_RISE`]).
//!
//! Presentation-only: the simulation, the frame-cost census, collision and
//! every `rand` draw read the actor records and `World::npcs`, never these.
//! Off (`pin_retail`), outside the field, or before the game-frame clock runs,
//! the accessors return the live values, bit for bit.

use super::*;
use legaia_engine_mode::mode::SceneMode;

/// A body step at least this long (world units) over one game frame is a
/// placement, drawn where it landed. A walk or run step is `4`..`48` units a
/// frame at the field's steps; a warp, a re-seat or a hide is hundreds.
pub const SNAP_MOVE: f32 = 512.0;
/// A heading turn at least this large (12-bit units) over one game frame is
/// a turn-about, drawn at once: a half turn has no short way round. The
/// hunters' quarter turns (`1024` in one frame) are drawn over the frame.
pub const SNAP_TURN: f32 = 1536.0;
/// A pose part translation step at least this long, or a rotation step at
/// least [`SNAP_PART_ANGLE`], draws the part where it landed (a clip change
/// that jumps the limb).
pub const SNAP_PART_MOVE: i32 = 512;
/// See [`SNAP_PART_MOVE`].
pub const SNAP_PART_ANGLE: i32 = 1024;

/// The grid (1/16-frame cursor units) a drawn NPC clip cursor lands on
/// ([`World::field_npc_drawn_clip_cursor`]): a quarter frame, which bounds
/// the hosts' posed-mesh memo at four entries a keyframe. A template-rate
/// clip moves `8` a vsync and a half-rate one `4`, so both draw a new pose
/// every vsync at every frame step.
pub const DRAWN_CLIP_QUANTUM: i32 = 4;

/// The placement slots `World::npcs` keys by (`u8`).
const NPC_SLOTS: usize = 256;

/// One body's root: position `(x, y, z)` and heading (12-bit units).
type Root = [f32; 4];

/// One part of a body's pose (`PoseFrame::bone_outputs`).
type Bone = ([i16; 3], [i16; 3]);

/// The draw-side state of one tick.
#[derive(Debug, Default)]
struct Snap {
    /// Every actor slot's root, `None` for an inactive slot.
    actors: Vec<Option<Root>>,
    /// Every actor slot's pose, empty for none.
    poses: Vec<Vec<Bone>>,
    /// Every placement slot's root, `None` when it is not drawn.
    npcs: Vec<Option<Root>>,
    /// Every placement slot's world-owned clip cursor
    /// (`World::npcs.clip_cursors`): `[anim id, frames, cursor, flags]`.
    clips: Vec<Option<Clip>>,
}

/// One NPC clip cursor: `[anim id, frames, cursor (1/16 frame), +0x62 flags]`.
type Clip = [i32; 4];

impl Clone for Snap {
    fn clone(&self) -> Self {
        let mut out = Self::default();
        out.clone_from(self);
        out
    }

    /// Field by field, into the buffers already held: the tick copies one
    /// snapshot into another every vsync, and the derived `clone_from`
    /// would allocate all three tables anew each time.
    fn clone_from(&mut self, src: &Self) {
        self.actors.clone_from(&src.actors);
        self.poses.clone_from(&src.poses);
        self.npcs.clone_from(&src.npcs);
        self.clips.clone_from(&src.clips);
    }
}

/// The smooth field actors' two states and the phase between them
/// ([`World::observe_field_present`]).
#[derive(Debug, Clone, Default)]
pub struct FieldPresent {
    /// The state the last game frame closed on (with every placement since
    /// folded in, so a placement draws at once).
    prev: Snap,
    /// The state the latest tick left.
    cur: Snap,
    /// The game frames observed; `None` while inactive.
    frame: Option<u64>,
    /// The display tick the current game frame opened on.
    open_tick: u64,
    /// The last observed display tick.
    last_tick: u64,
    /// The last tick's live state, for the between-frames move test.
    last: Snap,
    /// The current frame's length in vsyncs.
    span: u8,
    /// The share of the current frame already shown, `0.0..=1.0`.
    phase: f32,
    /// The scene the state belongs to.
    scene: String,
    /// Per actor slot / placement slot: the last game frame that body moved
    /// on a vsync between two frames (it draws live through the next).
    actor_live: Vec<u64>,
    pose_live: Vec<u64>,
    npc_live: Vec<u64>,
    clip_live: Vec<u64>,
    /// Each actor slot's drawn pose for this tick, and whether it holds
    /// one: the buffer outlives the ticks it is not drawn on, so a walk
    /// that draws it every other vsync allocates nothing.
    drawn: Vec<(bool, legaia_anm::PoseFrame)>,
}

/// `d` folded onto the short way round a 12-bit turn.
fn wrap_angle(d: f32) -> f32 {
    (d + 2048.0).rem_euclid(4096.0) - 2048.0
}

/// A height step at least this large (world units) over one game frame is a
/// change of floor tier (an NPC whose render height re-samples onto the next
/// level, a scripted drop): the body draws at its new height at once while
/// its ground position still steps. The opening's walkers step `443` units
/// between two floor levels in one frame of a `12`-unit walk; drawn over the
/// frame they rose through the floor.
pub const SNAP_RISE: f32 = 256.0;

/// Land a height step past [`SNAP_RISE`] in `prev` at once.
fn land_rise(prev: &mut Option<Root>, c: Option<Root>) {
    if let (Some(p), Some(c)) = (prev.as_mut(), c)
        && (c[1] - p[1]).abs() >= SNAP_RISE
    {
        p[1] = c[1];
    }
}

/// Whether `p -> c` is a placement, not a move.
fn placed(p: Option<Root>, c: Option<Root>) -> bool {
    match (p, c) {
        (Some(p), Some(c)) => {
            let d = ((c[0] - p[0]).powi(2) + (c[1] - p[1]).powi(2) + (c[2] - p[2]).powi(2)).sqrt();
            d >= SNAP_MOVE || wrap_angle(c[3] - p[3]).abs() >= SNAP_TURN
        }
        (None, None) => false,
        _ => true,
    }
}

impl FieldPresent {
    fn reset(&mut self) {
        if self.frame.is_none() {
            return;
        }
        self.frame = None;
        self.phase = 1.0;
        for s in [&mut self.prev, &mut self.cur, &mut self.last] {
            s.actors.clear();
            s.poses.clear();
            s.npcs.clear();
            s.clips.clear();
        }
        self.drawn.clear();
    }

    /// The share of the old frame still to draw, `None` when the draw is the
    /// live state.
    fn lag(&self) -> Option<f32> {
        (self.frame.is_some() && self.phase < 1.0).then_some(1.0 - self.phase)
    }

    fn delta(p: Option<Root>, c: Option<Root>) -> Option<Root> {
        let (p, c) = (p?, c?);
        let d = [
            c[0] - p[0],
            c[1] - p[1],
            c[2] - p[2],
            wrap_angle(c[3] - p[3]),
        ];
        (d != [0.0; 4]).then_some(d)
    }

    /// The shift `-lag * (cur - prev)` a body draws at, `None` when live.
    fn shift(&self, prev: &[Option<Root>], cur: &[Option<Root>], i: usize) -> Option<Root> {
        let lag = self.lag()?;
        let d = Self::delta(*prev.get(i)?, *cur.get(i)?)?;
        Some(d.map(|v| -lag * v))
    }
}

impl World {
    /// Whether the smooth field actors run on this tick: the toggle, the
    /// field, and the game-frame clock.
    pub fn field_present_active(&self) -> bool {
        self.toggles.smooth_actors && self.mode == SceneMode::Field && self.clock.game_tick_live
    }

    /// Fill `snap` with this tick's bodies.
    fn field_present_fill(&self, snap: &mut Snap) {
        snap.actors.clear();
        snap.actors.extend(self.actors.iter().map(|a| {
            a.active.then(|| {
                let m = &a.move_state;
                [
                    f32::from(m.world_x),
                    f32::from(m.world_y),
                    f32::from(m.world_z),
                    f32::from(m.render_26),
                ]
            })
        }));
        snap.poses.resize_with(self.actors.len(), Vec::new);
        snap.poses.truncate(self.actors.len());
        for (out, a) in snap.poses.iter_mut().zip(&self.actors) {
            out.clear();
            if a.active
                && let Some(p) = a.pose_frame.as_ref()
            {
                out.extend_from_slice(&p.bone_outputs);
            }
        }
        snap.npcs.clear();
        snap.npcs.resize(NPC_SLOTS, None);
        snap.clips.clear();
        snap.clips.resize(NPC_SLOTS, None);
        for (&slot, c) in &self.npcs.clip_cursors {
            snap.clips[usize::from(slot)] = Some([
                i32::from(c.anim_id),
                i32::from(c.frames),
                i32::from(c.cursor),
                i32::from(c.flags),
            ]);
        }
        let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;
        for (&slot, &(x, z)) in &self.npcs.positions {
            if (x, z) == (hide, hide) {
                continue;
            }
            let y = self.field_npc_render_y(slot, x, z);
            let h = self.npcs.headings.get(&slot).copied().unwrap_or(0);
            snap.npcs[usize::from(slot)] =
                Some([f32::from(x), y as f32, f32::from(z), f32::from(h)]);
        }
    }

    /// Record what this tick left for the smooth field actors' draw
    /// ([`FieldPresent`]). Runs at the end of every [`World::tick`]; reads
    /// the world and writes only [`World::field_present`].
    pub fn observe_field_present(&mut self) {
        if !self.field_present_active() {
            self.field_present.reset();
            return;
        }
        let now = self.clock.display_frames;
        let fired = self.clock.game_tick_fired;
        let mut fp = std::mem::take(&mut self.field_present);
        let fresh = fp.frame.is_none()
            || now != fp.last_tick.wrapping_add(1)
            || fp.scene != self.active_scene_label;
        if fresh {
            fp.scene.clone_from(&self.active_scene_label);
        }
        let frame = fp.frame.unwrap_or(0) + u64::from(fired);
        if fired {
            std::mem::swap(&mut fp.prev, &mut fp.cur);
            fp.open_tick = now;
            fp.span = self.clock.game_frame_wait();
        }
        self.field_present_fill(&mut fp.cur);
        let n_actors = fp.cur.actors.len();
        fp.actor_live.resize(n_actors, 0);
        fp.pose_live.resize(n_actors, 0);
        fp.npc_live.resize(NPC_SLOTS, 0);
        fp.clip_live.resize(NPC_SLOTS, 0);
        if fresh {
            fp.prev.clone_from(&fp.cur);
            fp.actor_live.fill(0);
            fp.pose_live.fill(0);
            fp.npc_live.fill(0);
            fp.clip_live.fill(0);
        } else {
            fp.prev.actors.resize(n_actors, None);
            fp.prev.poses.resize_with(n_actors, Vec::new);
            fp.prev.poses.truncate(n_actors);
            fp.prev.npcs.resize(NPC_SLOTS, None);
            fp.prev.clips.resize(NPC_SLOTS, None);
            fp.last.actors.resize(n_actors, None);
            fp.last.poses.resize_with(n_actors, Vec::new);
            fp.last.poses.truncate(n_actors);
            fp.last.npcs.resize(NPC_SLOTS, None);
            fp.last.clips.resize(NPC_SLOTS, None);
            // A body that changed on a vsync no game frame opened on moves
            // on the vsync, not the frame: it draws live through the next
            // frame.
            let live_through = frame + 1;
            for i in 0..n_actors {
                let (p, c) = (fp.prev.actors[i], fp.cur.actors[i]);
                if !fired && fp.last.actors[i] != c {
                    fp.actor_live[i] = live_through;
                }
                let place = placed(p, c);
                if fp.actor_live[i] >= frame || place {
                    fp.prev.actors[i] = c;
                }
                land_rise(&mut fp.prev.actors[i], c);
                if !fired && fp.last.poses[i] != fp.cur.poses[i] {
                    fp.pose_live[i] = live_through;
                }
                let skeleton = fp.prev.poses[i].len() != fp.cur.poses[i].len();
                if fp.pose_live[i] >= frame || place || skeleton {
                    let (prev, cur) = (&mut fp.prev.poses, &fp.cur.poses);
                    prev[i].clone_from(&cur[i]);
                }
            }
            for i in 0..NPC_SLOTS {
                let (p, c) = (fp.prev.npcs[i], fp.cur.npcs[i]);
                if !fired && fp.last.npcs[i] != c {
                    fp.npc_live[i] = live_through;
                }
                let place = placed(p, c);
                if fp.npc_live[i] >= frame || place {
                    fp.prev.npcs[i] = c;
                }
                land_rise(&mut fp.prev.npcs[i], c);
                // The NPC's clip: a cursor that moved between two game
                // frames draws live, and so does a re-bound clip.
                let (pc, cc) = (fp.prev.clips[i], fp.cur.clips[i]);
                if !fired && fp.last.clips[i] != cc {
                    fp.clip_live[i] = live_through;
                }
                let rebound = match (pc, cc) {
                    (Some(p), Some(c)) => p[..2] != c[..2],
                    (None, None) => false,
                    _ => true,
                };
                if fp.clip_live[i] >= frame || place || rebound {
                    fp.prev.clips[i] = cc;
                }
            }
        }
        fp.frame = Some(frame);
        fp.last_tick = now;
        fp.phase =
            (now.saturating_sub(fp.open_tick) as f32 / f32::from(fp.span.max(1))).clamp(0.0, 1.0);
        fp.last.clone_from(&fp.cur);
        fp.fill_drawn_poses(&self.actors);
        self.field_present = fp;
    }

    /// Actor slot `slot`'s root this vsync **draws** at: `([x, y, z],
    /// heading)` in raw field units and `render_26` units, one game frame
    /// behind under the smooth field actors; the live root otherwise. `None`
    /// past the actor table.
    pub fn field_draw_root(&self, slot: usize) -> Option<([f32; 3], f32)> {
        let a = self.actors.get(slot)?;
        let m = &a.move_state;
        let mut root = [
            f32::from(m.world_x),
            f32::from(m.world_y),
            f32::from(m.world_z),
            f32::from(m.render_26),
        ];
        if let Some(d) = self.field_draw_shift(slot) {
            for (r, d) in root.iter_mut().zip(d) {
                *r += d;
            }
        }
        Some(([root[0], root[1], root[2]], root[3]))
    }

    /// How far actor slot `slot` draws off its live root this vsync:
    /// `[dx, dy, dz, dheading]`, `None` when it draws live.
    pub fn field_draw_shift(&self, slot: usize) -> Option<[f32; 4]> {
        let fp = &self.field_present;
        fp.shift(&fp.prev.actors, &fp.cur.actors, slot)
    }

    /// How far placement slot `slot` draws off its live root this vsync:
    /// `[dx, dy, dz, dheading]`, `None` when it draws live.
    pub fn field_npc_draw_shift(&self, slot: u8) -> Option<[f32; 4]> {
        let fp = &self.field_present;
        fp.shift(&fp.prev.npcs, &fp.cur.npcs, usize::from(slot))
    }

    /// Placement slot `slot`'s clip cursor as this vsync **draws** it, in
    /// the cursor's 1/16-frame units: the world-owned cursor
    /// (`World::npcs.clip_cursors`) one game frame behind, stepped the way
    /// the clip runs (forward, or back under the reverse bit, across the
    /// loop's wrap), on the phase [`Self::field_npc_drawn_pose`] draws the
    /// body at, a walker's clip and a standing NPC's idle loop alike. `None`
    /// when the clip draws its live cursor: off, a held or re-bound clip, a
    /// cursor that did not move, or a step past half the clip (a restart).
    ///
    /// Retail's anim tick (`FUN_800204F8`) moves the cursor once a game
    /// frame by the rate times the frame step, so at step `4` a template
    /// clip jumps two keyframes a frame; a host poses the drawn cursor
    /// through the frame blender at its sub-frame, gate or no gate
    /// (`FieldClipPlayer::drawn_pose`).
    pub fn field_npc_drawn_clip_cursor(&self, slot: u8) -> Option<u32> {
        let fp = &self.field_present;
        let lag = fp.lag()?;
        let i = usize::from(slot);
        let (p, c) = ((*fp.prev.clips.get(i)?)?, (*fp.cur.clips.get(i)?)?);
        if p[..2] != c[..2] {
            return None;
        }
        // Every running clip, a standing NPC's idle loop too: each
        // in-between pose is one more posed mesh in the hosts' memo, which
        // keys the clip (`field_anim::NpcPoseKey`) and keeps it across a
        // wanderer's idle / walk switches, so the memo fills once a clip.
        let span = c[1] * 16;
        if span <= 0 {
            return None;
        }
        let flags = c[3] as u16;
        if flags & legaia_engine_vm::actor_tick::CLIP_HOLD != 0 {
            return None;
        }
        let mut d = c[2] - p[2];
        let clamp = flags & legaia_engine_vm::actor_tick::CLIP_CLAMP != 0;
        if flags & legaia_engine_vm::actor_tick::CLIP_REVERSE == 0 {
            if d < 0 && !clamp {
                d += span;
            }
        } else if d > 0 && !clamp {
            d -= span;
        }
        if d == 0 || d.abs() * 2 > span {
            return None;
        }
        // On the quarter-frame grid: the hosts memoise a posed mesh per
        // drawn cursor.
        let q = DRAWN_CLIP_QUANTUM as f32;
        let drawn = ((c[2] as f32 - lag * d as f32) / q).round() as i32 * DRAWN_CLIP_QUANTUM;
        Some(drawn.rem_euclid(span) as u32)
    }

    /// [`Self::field_npc_draw_pose`] as this vsync **draws** it: the
    /// position and the yaw one game frame behind under the smooth field
    /// actors ([`Self::field_npc_draw_shift`]); the live pose otherwise.
    pub fn field_npc_drawn_pose(&self, slot: u8, spawn: (i16, i16)) -> Option<NpcDrawPose> {
        let mut pose = self.field_npc_draw_pose(slot, spawn)?;
        if let Some(d) = self.field_npc_draw_shift(slot) {
            for (p, d) in pose.pos.iter_mut().zip(d) {
                *p += d;
            }
            pose.yaw = ((f32::from(pose.yaw) + d[3]).round() as i32).rem_euclid(4096) as u16;
        }
        Some(pose)
    }

    /// Actor slot `slot`'s pose this vsync **draws**: the live `pose_frame`
    /// carried back toward the last frame's on the same phase as
    /// [`Self::field_draw_root`]. `None` when the body draws its live pose.
    pub fn field_draw_pose_frame(&self, slot: usize) -> Option<&legaia_anm::PoseFrame> {
        let fp = &self.field_present;
        fp.lag()?;
        let drawn = fp.drawn.get(slot).filter(|d| d.0).map(|d| &d.1)?;
        let live = self.actors.get(slot)?.pose_frame.as_ref()?;
        (drawn.bone_outputs.len() == live.bone_outputs.len()).then_some(drawn)
    }
}

impl FieldPresent {
    /// Fill [`Self::drawn`] for this tick: each slot's pose stepped `lag` of
    /// the way back toward the frame before, part by part (a part that moved
    /// past [`SNAP_PART_MOVE`] or turned past [`SNAP_PART_ANGLE`] draws where
    /// it landed), unheld where the slot draws its live pose. The buffers
    /// are kept from tick to tick, so a steady scene allocates nothing here.
    fn fill_drawn_poses(&mut self, actors: &[Actor]) {
        let lag = self.lag();
        self.drawn.resize_with(actors.len(), || {
            (
                false,
                legaia_anm::PoseFrame {
                    bone_outputs: Vec::new(),
                    factor: 0,
                    finished: false,
                },
            )
        });
        self.drawn.truncate(actors.len());
        for (i, (held, frame)) in self.drawn.iter_mut().enumerate() {
            *held = false;
            let (Some(lag), Some(p), Some(c), Some(live)) = (
                lag,
                self.prev.poses.get(i),
                self.cur.poses.get(i),
                actors[i].pose_frame.as_ref(),
            ) else {
                continue;
            };
            if c.is_empty() || p.len() != c.len() || p == c || live.bone_outputs.len() != c.len() {
                continue;
            }
            *held = true;
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

#[cfg(test)]
mod tests {
    use super::*;

    const NPC: u8 = 7;

    fn field_world() -> World {
        let mut w = World::new();
        w.mode = SceneMode::Field;
        w.toggles.smooth_actors = true;
        w.clock.game_tick_live = true;
        w.clock.frame_step = 3;
        w.npcs.positions.insert(NPC, (1000, 2000));
        w
    }

    /// One vsync as a tick leaves it: `fired` opens a game frame, on which
    /// `step` moves the NPC.
    fn vsync(w: &mut World, fired: bool, step: (i16, i16)) {
        w.clock.display_frames += 1;
        w.clock.game_tick_fired = fired;
        if fired {
            let p = w.npcs.positions.get_mut(&NPC).unwrap();
            *p = (p.0 + step.0, p.1 + step.1);
        }
        w.observe_field_present();
    }

    fn drawn_x(w: &World) -> f32 {
        w.field_npc_drawn_pose(NPC, (0, 0)).unwrap().pos[0]
    }

    /// Run `frames` game frames of three vsyncs, the walker stepping `dx`
    /// on each; the drawn X per vsync.
    fn walk(w: &mut World, frames: usize, dx: i16) -> Vec<f32> {
        let mut out = Vec::new();
        for _ in 0..frames {
            for v in 0..3 {
                vsync(w, v == 0, (dx, 0));
                out.push(drawn_x(w));
            }
        }
        out
    }

    /// A walker that steps `12` units a game frame draws `4` a vsync, one
    /// frame behind, while the record keeps the whole step.
    #[test]
    fn a_frame_stepped_walker_draws_every_vsync() {
        let mut w = field_world();
        vsync(&mut w, true, (0, 0));
        let drawn = walk(&mut w, 3, 12);
        assert_eq!(
            drawn,
            [
                1000.0, 1004.0, 1008.0, 1012.0, 1016.0, 1020.0, 1024.0, 1028.0, 1032.0
            ]
        );
        assert_eq!(w.npcs.positions[&NPC], (1036, 2000), "the stored record");
    }

    /// A step past [`SNAP_MOVE`] is a placement: drawn where it landed.
    #[test]
    fn a_warp_draws_at_once() {
        let mut w = field_world();
        vsync(&mut w, true, (0, 0));
        walk(&mut w, 2, 12);
        let drawn = walk(&mut w, 1, 900);
        assert_eq!(drawn, [1924.0; 3]);
        // And the walk after it steps from the landing.
        assert_eq!(walk(&mut w, 1, 12), [1924.0, 1928.0, 1932.0]);
    }

    /// A body that moves on the vsyncs between two game frames (the smooth
    /// walk) draws live from the first such vsync on.
    #[test]
    fn a_per_vsync_mover_draws_live() {
        let mut w = field_world();
        vsync(&mut w, true, (0, 0));
        for v in 0..9 {
            w.npcs.positions.get_mut(&NPC).unwrap().0 += 4;
            w.clock.display_frames += 1;
            w.clock.game_tick_fired = v % 3 == 0;
            w.observe_field_present();
            if v > 0 {
                assert_eq!(
                    drawn_x(&w),
                    f32::from(w.npcs.positions[&NPC].0),
                    "vsync {v}"
                );
            }
        }
    }

    /// A quarter turn is drawn over the frame; a half turn lands.
    #[test]
    fn a_quarter_turn_steps_and_a_half_turn_lands() {
        let mut w = field_world();
        w.npcs.headings.insert(NPC, 0);
        vsync(&mut w, true, (0, 0));
        walk(&mut w, 1, 0);
        w.npcs.headings.insert(NPC, 1024);
        let yaw = |w: &World| w.field_npc_drawn_pose(NPC, (0, 0)).unwrap().yaw;
        let mut quarter = Vec::new();
        for v in 0..3 {
            vsync(&mut w, v == 0, (0, 0));
            quarter.push(yaw(&w));
        }
        // `field_npc_draw_pose` composes the half turn onto the heading.
        assert_eq!(quarter, [2048, 2389, 2731]);
        walk(&mut w, 1, 0);
        w.npcs.headings.insert(NPC, 1024 + 2048);
        vsync(&mut w, true, (0, 0));
        assert_eq!(yaw(&w), 1024);
    }

    /// Off (`pin_retail`), every accessor is the live record.
    #[test]
    fn off_draws_the_stored_record() {
        let mut w = field_world();
        w.toggles.pin_retail();
        vsync(&mut w, true, (0, 0));
        for x in walk(&mut w, 2, 12)
            .into_iter()
            .zip([1012.0, 1012.0, 1012.0, 1024.0, 1024.0, 1024.0])
        {
            assert_eq!(x.0, x.1);
        }
        assert!(w.field_npc_draw_shift(NPC).is_none());
        assert!(w.field_draw_shift(0).is_none());
    }

    /// A scene change starts over from the new scene's bodies.
    #[test]
    fn a_scene_change_draws_the_new_scene_live() {
        let mut w = field_world();
        vsync(&mut w, true, (0, 0));
        walk(&mut w, 2, 12);
        w.active_scene_label = "next".into();
        vsync(&mut w, true, (12, 0));
        assert_eq!(drawn_x(&w), f32::from(w.npcs.positions[&NPC].0));
    }

    /// A world-driven clip stepping `32` cursor units (two keyframes) a
    /// three-vsync frame draws a third of that a vsync, on the quarter-frame
    /// grid, across the loop's wrap; under the reverse bit it runs back.
    #[test]
    fn a_frame_stepped_clip_draws_between_its_keyframes() {
        let mut w = field_world();
        let anim = legaia_engine_scene::field_env::PropAnim::spawned(1, 4, false, 0);
        w.npcs.clip_cursors.insert(NPC, anim);
        vsync(&mut w, true, (0, 0));
        let mut drawn = Vec::new();
        for _ in 0..3 {
            for v in 0..3 {
                if v == 0 {
                    let c = w.npcs.clip_cursors.get_mut(&NPC).unwrap();
                    c.cursor = (c.cursor + 32) % 64;
                }
                vsync(&mut w, v == 0, (12, 0));
                drawn.push(w.field_npc_drawn_clip_cursor(NPC));
            }
        }
        // 0 -> 32 -> 0 (the wrap, forward) -> 32.
        assert_eq!(
            drawn,
            [
                Some(0),
                Some(12),
                Some(20),
                Some(32),
                Some(44),
                Some(52),
                Some(0),
                Some(12),
                Some(20)
            ]
        );
        // Under the reverse bit the clip runs back (32 -> 16).
        w.npcs.clip_cursors.get_mut(&NPC).unwrap().flags |=
            legaia_engine_vm::actor_tick::CLIP_REVERSE;
        w.npcs.clip_cursors.get_mut(&NPC).unwrap().cursor = 16;
        vsync(&mut w, true, (12, 0));
        vsync(&mut w, false, (0, 0));
        assert_eq!(w.field_npc_drawn_clip_cursor(NPC), Some(28));
        // A body that stands steps its idle loop between keyframes too.
        w.npcs.clip_cursors.get_mut(&NPC).unwrap().cursor = 0;
        vsync(&mut w, true, (0, 0));
        vsync(&mut w, false, (0, 0));
        assert_eq!(w.field_npc_drawn_clip_cursor(NPC), Some(12));
        // A held clip, and the retail pin, draw the playhead.
        w.npcs.clip_cursors.get_mut(&NPC).unwrap().flags |= legaia_engine_vm::actor_tick::CLIP_HOLD;
        vsync(&mut w, true, (12, 0));
        assert_eq!(w.field_npc_drawn_clip_cursor(NPC), None);
        w.toggles.pin_retail();
        vsync(&mut w, true, (0, 0));
        assert_eq!(w.field_npc_drawn_clip_cursor(NPC), None);
    }

    /// A floor-tier step lands at once; the ground position still steps.
    #[test]
    fn a_floor_tier_change_lands_while_the_walk_steps() {
        let mut w = field_world();
        w.actors[0].active = true;
        vsync(&mut w, true, (0, 0));
        walk(&mut w, 1, 0);
        let m = &mut w.actors[0].move_state;
        m.world_x += 12;
        m.world_y -= 400;
        vsync(&mut w, true, (0, 0));
        vsync(&mut w, false, (0, 0));
        let (p, _) = w.field_draw_root(0).unwrap();
        let m = &w.actors[0].move_state;
        assert_eq!(p[1], f32::from(m.world_y), "the height lands");
        assert!(
            (p[0] - (f32::from(m.world_x) - 8.0)).abs() < 1e-3,
            "the walk steps"
        );
    }
}
