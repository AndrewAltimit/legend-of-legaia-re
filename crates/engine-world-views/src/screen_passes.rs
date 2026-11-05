//! The seam both play hosts read a running `World` through for everything
//! their draw passes project: [`HostView`], plus the battle screen FX that
//! need only the world and the host's battle projection.
//!
//! A host holds three things a frame's projection needs - the `World`, the
//! scene's [`Camera`] and the scene box its fallback focus comes from - and
//! each host used to spell, per pass, the same three steps over them: a mode
//! gate, `resolve_field_camera` on the box's X / Z centre, a kernel and its
//! prim wrap. [`HostView`] is those three things borrowed together, and each
//! pass is one method on it. What a host still decides alone is what it
//! holds alone: whether a scripted shot owns the frame (and the glide it
//! advances to get there), its boot-title gate, whether the leader's mesh is
//! resident, its own debug vantages, and how a battle point reaches its
//! stage (the two hosts keep different battle model spaces).
//!
//! The fog pool's render *step* is not here: it is simulation (it ages the
//! pool the next tick's spawner reads), so it runs per tick through
//! `legaia_engine_session::BootSession::step_field_fog`. Its *draw* - the
//! quads the step left - is [`HostView::fog_prims`].

use crate::camera_view::{cutscene_view, resolve_field_camera};
use legaia_engine_camera::psx_camera::FieldCameraView;
use legaia_engine_core::world::World;
use legaia_engine_field::camera::Camera;
use legaia_engine_field::camera_frame::{FieldCameraFrame, frame_curve_scale};
use legaia_engine_mode::mode::SceneMode;
use legaia_render_kernels::afterimage::AfterimageQuad;
use legaia_render_kernels::gte::PartCameraPose;
use legaia_render_kernels::screen_prim::{
    ScreenPrim, drop_shadow_prim, fog_puff_prim, light_pool_prims, sky_band_prims,
};

/// A scene box, `(min, max)` in retail world units.
pub type SceneBox = ([f32; 3], [f32; 3]);

/// The follow camera's fallback focus: the scene box's X / Z centre, which
/// every field pass hands `resolve_field_camera`. A pass that pinned
/// `[0, 0]` projected through a camera aimed at the world origin on any
/// frame whose camera fell back to the host centre.
pub fn scene_centre(aabb: SceneBox) -> [f32; 2] {
    [(aabb.0[0] + aabb.1[0]) * 0.5, (aabb.0[2] + aabb.1[2]) * 0.5]
}

/// The `World`, the scene camera and the scene box, borrowed together for
/// one frame's reads.
#[derive(Clone, Copy)]
pub struct HostView<'a> {
    pub world: &'a World,
    pub camera: &'a Camera,
    pub scene_box: SceneBox,
}

impl<'a> HostView<'a> {
    pub fn new(world: &'a World, camera: &'a Camera, scene_box: SceneBox) -> Self {
        Self {
            world,
            camera,
            scene_box,
        }
    }

    /// The resolver's fallback focus ([`scene_centre`] of the box).
    pub fn centre(&self) -> [f32; 2] {
        scene_centre(self.scene_box)
    }

    /// The frame's camera: `cutscene` is the scripted shot when the host's
    /// gate says one owns the frame (already glided - the glide is the
    /// host's step, not a read), `None` otherwise.
    pub fn frame(&self, cutscene: Option<FieldCameraView>) -> FieldCameraFrame {
        resolve_field_camera(self.world, self.camera, cutscene, self.centre())
    }

    /// The non-scripted frame: the camera this frame would have without a
    /// scripted shot. Never advances the cutscene glide, so a draw pass may
    /// ask as often as it likes. A pass whose retail draw goes through the
    /// GTE camera of the frame (the drop shadows, the light pools, the move
    /// strips) projects through [`Self::frame`] with the host's glided shot
    /// instead: under a scripted shot this one is a different camera, and
    /// everything projected through it lands shifted off the meshes.
    pub fn follow_frame(&self) -> FieldCameraFrame {
        self.frame(None)
    }

    /// The scripted shot's target view (`camera_view::cutscene_view`) on
    /// this box's fallback focus - the view a host's glide steps toward, or,
    /// unglided, the view a query that must not step reads.
    pub fn scripted_target(&self) -> FieldCameraView {
        cutscene_view(self.world, self.centre())
    }

    /// The overworld curvature's `clip.w`-to-`SZ` factor for the scene pass
    /// (`frame_curve_scale`): `0` off the kingdom overworld or under a frame
    /// with no retail eye.
    pub fn curve_scale(&self, cutscene: Option<FieldCameraView>) -> f32 {
        if !self.world.overworld_bit() {
            return 0.0;
        }
        frame_curve_scale(true, &self.frame(cutscene))
    }

    /// The retail camera a walkable frame's move-VM part draws resolve their
    /// `+0x52` camera-relative bits against (`FUN_8001CF50`'s placement):
    /// the frame's field view, or `None` under a vantage with no retail
    /// rotation (the top view, `HostDebugOrbit`). The battle arm and a
    /// host's own debug orbit are the host's to answer before asking.
    pub fn part_camera_pose(&self, cutscene: Option<FieldCameraView>) -> Option<PartCameraPose> {
        self.frame(cutscene)
            .field_view()
            .map(|v| PartCameraPose::from_field_view(&v))
    }

    /// The fog sheets the last tick's render step left, wrapped by
    /// `fog_puff_prim`. Empty outside game mode 3 or while the script gate
    /// (`_DAT_8007B854`) is clear - the two tests the pass makes at
    /// `0x80026EA4..0x80026EC4`. A read, not a step.
    ///
    /// A pack map's particle emitters ride the same pass
    /// (`World::expansion_particle_quads`, through the follow frame): they
    /// are sheets of the same shape, and drawing them here puts them on both
    /// hosts at once. `enhanced_lighting` is the host's enhanced-lighting
    /// switch: an emitter that belongs to the lighting layer (a glow's
    /// motes) draws only while it is on.
    pub fn fog_prims(&self, enhanced_lighting: bool) -> Vec<ScreenPrim> {
        let world = self.world;
        let mut out: Vec<ScreenPrim> = Vec::new();
        if World::fog_mode(world.mode) && world.fog.gate {
            out.extend(
                world.fog_quads().iter().map(|q| {
                    fog_puff_prim(q.xy, q.uv, q.clut, q.tpage, q.rgb, q.ot_index, q.depth)
                }),
            );
        }
        if world.expansion.map.is_some()
            && let Some(view) = self.follow_frame().field_view()
        {
            let fx = world.field_fx_view(&view);
            out.extend(
                world
                    .expansion_particle_quads(&fx, enhanced_lighting)
                    .iter()
                    .map(|q| {
                        fog_puff_prim(q.xy, q.uv, q.clut, q.tpage, q.rgb, q.ot_index, q.depth)
                    }),
            );
        }
        out
    }

    /// A pack map's light beams (`[[beams]]`, `World::expansion_beam_prims`)
    /// through the follow frame: additive shafts with dust in them, an
    /// enhancement drawn only under `enhanced_lighting` - with it off this
    /// is empty and the frame stays retail-faithful. Both hosts append it to
    /// the field screen-effect pass beside [`Self::fog_prims`].
    pub fn beam_prims(&self, enhanced_lighting: bool) -> Vec<ScreenPrim> {
        let world = self.world;
        if !enhanced_lighting || world.expansion.map.is_none() {
            return Vec::new();
        }
        let Some(view) = self.follow_frame().field_view() else {
            return Vec::new();
        };
        world.expansion_beam_prims(&world.field_fx_view(&view))
    }

    /// This frame's actor drop shadows (`legaia_render_kernels::drop_shadow`,
    /// retail's `FUN_8001C394` blob) through the frame the meshes draw
    /// through, wrapped by `drop_shadow_prim`. Each cell carries its corners'
    /// scene depth, so a host depth-tests it against the scene already drawn:
    /// under the actor standing on it, over the ground. Empty outside game
    /// mode 3.
    ///
    /// `cutscene` is the host's glided scripted shot when one owns the frame
    /// (the same view its scene pass draws through), `None` otherwise.
    /// Retail projects the blob from inside the actor renderer
    /// (`FUN_8001C394` hands `actor+0x14` to `FUN_800460AC` under the GTE
    /// camera the frame loaded), so the blob and the body share one camera;
    /// projecting through the follow frame instead shifted every blob off its
    /// body by the same screen offset during a scripted event.
    pub fn drop_shadow_prims(
        &self,
        cutscene: Option<FieldCameraView>,
        link_order: bool,
    ) -> Vec<ScreenPrim> {
        if !World::fog_mode(self.world.mode)
            && self.world.pack_mode_stage().is_none()
            && self.world.mode != SceneMode::Dance
        {
            return Vec::new();
        }
        let Some(view) = self.frame(cutscene).field_view() else {
            return Vec::new();
        };
        self.world
            .field_drop_shadows(&view, link_order)
            .iter()
            .map(|q| drop_shadow_prim(q.xy, q.uv, q.clut, q.tpage, q.rgb, q.ot_index, q.depth))
            .collect()
    }

    /// This frame's move-VM strip spans: every extension sub-op `0x2C`
    /// execution of the latest tick (`FUN_801D31B0`,
    /// `MoveVmGlobals::strip_frame`), projected through the follow frame by
    /// `move_strip_prims`. The set is the latest tick's and is not drained, so
    /// a redraw with no tick draws it again. Field scenes only - the
    /// extension dispatcher exists only in the field overlay. `cutscene` is
    /// the host's glided scripted shot, as for [`Self::drop_shadow_prims`].
    pub fn move_strip_prims(&self, cutscene: Option<FieldCameraView>) -> Vec<ScreenPrim> {
        let requests = self.world.move_vm.strip_frame();
        if requests.is_empty() || self.world.mode != SceneMode::Field {
            return Vec::new();
        }
        let (FieldCameraFrame::Follow(view) | FieldCameraFrame::Cutscene(view)) =
            self.frame(cutscene)
        else {
            return Vec::new();
        };
        legaia_render_kernels::move_strip::move_strip_prims(requests, &view)
    }

    /// This frame's attached lights - the field VM's op `0x34` sub-1 light
    /// pools (`World::field_light_draws`, the `FUN_801E4470` draw feeding
    /// `FUN_801E3984`) - wrapped by `light_pool_prims`, which owns the blend
    /// class, vertex order and triangle split. Empty outside a field scene or
    /// with no light live. `cutscene` is the host's glided scripted shot, as
    /// for [`Self::drop_shadow_prims`].
    pub fn light_prims(&self, cutscene: Option<FieldCameraView>) -> Vec<ScreenPrim> {
        let world = self.world;
        if world.mode != SceneMode::Field || world.script_actors.lights.is_empty() {
            return Vec::new();
        }
        let (FieldCameraFrame::Follow(view) | FieldCameraFrame::Cutscene(view)) =
            self.frame(cutscene)
        else {
            return Vec::new();
        };
        world
            .field_light_draws(&view)
            .iter()
            .flat_map(|d| light_pool_prims(d.abr, &d.polys))
            .collect()
    }

    /// This frame's overworld markers ([`crate::world_map_markers::marker_prims`]
    /// through the follow frame). `draw_player` is the host's answer to "did
    /// the leader's real mesh fail to resolve" - the player marker is its
    /// stand-in.
    pub fn world_map_marker_prims(&self, draw_player: bool) -> Vec<ScreenPrim> {
        if self.world.mode != SceneMode::WorldMap {
            return Vec::new();
        }
        crate::world_map_markers::marker_prims(
            self.world,
            &self.follow_frame(),
            self.scene_box,
            draw_player,
        )
    }

    /// This frame's in-world mode pieces - golf balls, the cup and flag,
    /// the aim arrow, the other seats' bodies, boost trails
    /// ([`crate::pack_mode_world::stage_prims`]) - through the frame the
    /// stage's shot resolves to. Empty unless a golf round or race played on
    /// a pack map owns the frame.
    pub fn pack_mode_world_prims(&self) -> Vec<ScreenPrim> {
        if self.world.pack_mode_stage().is_none() {
            return Vec::new();
        }
        crate::pack_mode_world::stage_prims(self.world, &self.follow_frame(), self.scene_box)
    }

    /// This frame's overworld sky band (`FUN_801F73E4`) through the gate
    /// [`crate::world_map_sky::sky_band`] and `sky_band_prims`. Each quad is
    /// depth-tested at the far plane, so it fills only what the terrain left
    /// clear - retail's farthest-bucket draw order.
    pub fn world_map_sky_prims(&self) -> Vec<ScreenPrim> {
        if !self.world.shows_overworld() {
            return Vec::new();
        }
        sky_band_prims(&crate::world_map_sky::sky_band(
            self.world,
            self.camera,
            &self.follow_frame(),
        ))
    }
}

/// This frame's weapon-trail bands (`legaia_engine_world_views::battle_draws::battle_weapon_trail_draws`,
/// retail `FUN_8005112C` / `FUN_80048310`; band packets from
/// `battle_trail::weapon_trail_prims`, the `FUN_800485BC` port).
///
/// Each control point is turned by the body's facing and set on its ground
/// base by retail's own arithmetic (`battle_trail::trail_world_point`), then
/// placed and projected the way both hosts place a battle body
/// (`battle_trail::project_battle_point`: frame flip, `world_scale`, the
/// host's `vp`). The hosts pass only their battle view-projection - the
/// native window's glam composition, which
/// `battle_vp_matches_the_native_glam_composition` pins to the page's
/// `legaia_engine_camera::projection::battle_vp` - so the trail is one
/// kernel on both. A point past the near plane truncates its step's sweep
/// rather than smearing a wrapped vertex.
pub fn weapon_trail_prims(world: &World, vp: &[f32; 16], world_scale: f32) -> Vec<ScreenPrim> {
    use legaia_render_kernels::battle_trail as bt;
    let mut out = Vec::new();
    for d in crate::battle_draws::battle_weapon_trail_draws(world) {
        let Some(actor) = world.actors.get(d.actor_slot as usize) else {
            continue;
        };
        let base = [actor.move_state.world_x, actor.move_state.world_z];
        let yaw = actor.battle.facing_angle;
        let mut steps = Vec::with_capacity(d.steps.len());
        'draw: for s in &d.steps {
            let mut pts = [(0i16, 0i16); bt::TRAIL_POINTS];
            for (i, t) in s.iter().enumerate() {
                let p = bt::trail_world_point(base, yaw, *t);
                match bt::project_battle_point(vp, world_scale, p) {
                    Some(xy) => pts[i] = xy,
                    None => break 'draw,
                }
            }
            steps.push(pts);
        }
        out.extend(bt::weapon_trail_prims(&steps, d.rgb, bt::WEAPON_TRAIL_OT));
    }
    out
}

/// This frame's move-FX afterimage streak, then the clip-tag ribbon, as
/// stage quads through `mvp` (the host's battle model-view-projection at the
/// stage's 4:3).
///
/// The streak is the pass retail runs once per move-FX frame
/// (`FUN_801E09F8`'s schedule over the ported `FUN_801E1AB0` packet): the
/// launch point the action effect script's terminator staged, fanned out at
/// the staged half-width. It is gated on the move-FX scene being live - the
/// trail texpage is scene-scoped, so the streak lasts exactly as long as the
/// effect it trails. The clip-tag ribbon (`FUN_8004CE2C` tag `0x67` ->
/// `FUN_801E1D98(&target[+0x3C], 0xC)`) rides a physical art's clip instead
/// and draws with or without a move-FX scene. Both clock on the world's
/// display-frame counter.
pub fn move_fx_streak_quads(world: &World, mvp: &glam::Mat4) -> Vec<AfterimageQuad> {
    use legaia_render_kernels::streak_pass::{
        StreakSource, clip_ribbon_quads, streak_quads_scheduled,
    };
    let frame = world.clock.display_frames as u32;
    let mut out = Vec::new();
    if let Some(trail) = world.active_move_fx_trail_texpage() {
        let block = world.move_fx_streak();
        if let Some(src) = StreakSource::from_block(block.launch, block.half_width(), Some(trail)) {
            // The schedule keys on the counter word and the acting side:
            // party = afterimage shrinking toward the ribbon, monster =
            // ribbon throughout. `ctx[+0x13] < 3` is a pool-slot test
            // (`0x801E0C78`): a small party's first monster sits below
            // engine slot `3`.
            let party = world
                .battle_seat_map()
                .is_party(world.battle_ctx.active_actor);
            out.extend(streak_quads_scheduled(
                &src,
                mvp,
                frame,
                block.counter_word,
                party,
            ));
        }
    }
    if let Some(c) = world.battle.clip_ribbon {
        out.extend(clip_ribbon_quads(
            c.seat.map(f32::from),
            c.trail_id,
            mvp,
            frame,
        ));
    }
    out
}

/// A cast arm's ribbon ring as screen primitives (PROT 0962 `0xA3` arm 4, PROT
/// 0934's arms 5..9): one ribbon call per point, each drawing its own run of
/// BIOS `rand` - so each point takes its own seed off the frame. Retail links
/// each packet at its anchor's own depth bucket (`FUN_801E1D98`'s `AddPrim`,
/// `0x801E20D8`), so a body nearer the camera hides it: every corner carries the
/// anchor's scene depth.
pub fn cast_ribbon_prims(
    world: &World,
    mvp: &glam::Mat4,
    ot_index: u32,
) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
    use legaia_render_kernels::screen_prim::{CornerDepth, ScreenPrim};
    use legaia_render_kernels::streak_pass::clip_ribbon_quads;
    let Some(ring) = world.battle.cast_ribbons else {
        return Vec::new();
    };
    let frame = world.clock.display_frames as u32;
    let mut out = Vec::new();
    for (i, p) in ring.points.iter().take(usize::from(ring.count)).enumerate() {
        let at = p.map(f32::from);
        let clip = *mvp * glam::Vec4::new(at[0], at[1], at[2], 1.0);
        let depth = (clip.w > 1e-4).then(|| CornerDepth::new([clip.z / clip.w; 4]));
        let quads = clip_ribbon_quads(
            at,
            ring.trail_id,
            mvp,
            frame.wrapping_mul(17).wrapping_add(i as u32 + 1),
        );
        out.extend(quads.iter().map(|q| {
            let mut prim = q.to_screen_prim(ot_index);
            if let ScreenPrim::Textured(t) = &mut prim {
                t.depth = depth;
            }
            prim
        }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const AABB: SceneBox = ([0.0, -500.0, 0.0], [4000.0, 0.0, 2000.0]);

    #[test]
    fn the_fallback_focus_is_the_box_centre() {
        assert_eq!(scene_centre(AABB), [2000.0, 1000.0]);
    }

    #[test]
    fn every_pass_is_empty_off_its_mode() {
        let world = World::default();
        let cam = Camera::default();
        let v = HostView::new(&world, &cam, AABB);
        assert!(v.fog_prims(true).is_empty());
        assert!(v.drop_shadow_prims(None, false).is_empty());
        assert!(v.move_strip_prims(None).is_empty());
        assert!(v.light_prims(None).is_empty());
        assert!(v.world_map_marker_prims(true).is_empty());
        assert!(v.world_map_sky_prims().is_empty());
        assert_eq!(v.curve_scale(None), 0.0);
        assert!(weapon_trail_prims(&world, &glam::Mat4::IDENTITY.to_cols_array(), 4.0).is_empty());
        assert!(move_fx_streak_quads(&world, &glam::Mat4::IDENTITY).is_empty());
    }

    /// Under a scripted shot each blob's centre grid point (`FUN_800460AC`'s
    /// middle `RTPT` point, the shared corner of the four cells) is the
    /// body's own origin projected through the shot the scene pass draws -
    /// for the player and for a placement the scripted mover walked off its
    /// channel's anchor alike. Through the follow frame instead, both blobs
    /// shift by one screen offset and keep their spacing: the formation
    /// drawn beside the bodies.
    #[test]
    fn a_scripted_shot_lays_each_blob_under_its_body() {
        use legaia_engine_field::field_channels::FieldChannel;
        let mut world = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        let a = world.spawn_actor(0);
        a.move_state.world_x = 1000;
        a.move_state.world_z = 600;
        world.player_actor_slot = Some(0);
        // A placement whose channel still holds its seat while the scripted
        // mover has walked it on (`npcs.positions`, what its mesh draws at).
        let mut c = FieldChannel {
            placement_index: 3,
            ctx: Default::default(),
            record_offset: 0,
            pc: 0,
            done: false,
            object_bind: false,
        };
        c.ctx.move_id = 1;
        c.ctx.world_x = 200;
        c.ctx.world_z = 200;
        c.ctx.field_72 = 0x1000;
        world.field_vm.channels.push(c);
        world.npcs.positions.insert(3, (1300, 900));
        let cam = Camera::default();
        let v = HostView::new(&world, &cam, AABB);
        let follow = v.follow_frame().field_view().expect("follow view");
        let shot = FieldCameraView {
            focus: [
                follow.focus[0] + 300.0,
                follow.focus[1],
                follow.focus[2] + 500.0,
            ],
            ..follow
        };
        let player = [1000, world.actors[0].move_state.world_y.into(), 600];
        let npc = world
            .field_npc_draw_pose(3, (200, 200))
            .expect("drawn")
            .pos
            .map(|c| c as i32);
        let centre = |view: &FieldCameraView, p: [i32; 3]| {
            let (sx, sy, _) = world.field_fx_view(view).project_gte(p).expect("in front");
            (sx, sy)
        };
        let prims = v.drop_shadow_prims(Some(shot), false);
        assert_eq!(prims.len(), 8, "four cells per caster");
        let drawn = v.frame(Some(shot)).field_view().expect("shot");
        assert_eq!(prims[0].corners()[3], centre(&drawn, player));
        assert_eq!(prims[4].corners()[3], centre(&drawn, npc));
        // The follow frame is a different camera: its blobs sit elsewhere.
        let stale = v.drop_shadow_prims(None, false);
        assert_eq!(stale[0].corners()[3], centre(&follow, player));
        assert_ne!(stale[0].corners()[3], prims[0].corners()[3]);
    }
}
