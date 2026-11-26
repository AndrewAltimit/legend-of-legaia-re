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

use crate::camera_view::{camera_present_lead, cutscene_view, resolve_field_camera_led};
use legaia_engine_camera::psx_camera::FieldCameraView;
use legaia_engine_core::world::World;
use legaia_engine_field::camera::{Camera, DrawLead};
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
    /// Where in the game frame the draw samples an in-flight camera glide
    /// (`camera_view::camera_present_lead`); [`DrawLead::NONE`] unless the
    /// host asked for [`Self::presented`].
    pub lead: DrawLead,
}

impl<'a> HostView<'a> {
    pub fn new(world: &'a World, camera: &'a Camera, scene_box: SceneBox) -> Self {
        Self {
            world,
            camera,
            scene_box,
            lead: DrawLead::NONE,
        }
    }

    /// This view as the frame **draws** it: the follow and overworld arms
    /// sample an in-flight glide at the vsync the frame is drawn on, under
    /// the smooth camera (`WorldToggles::smooth_camera`). Off, or on the
    /// vsync a game frame opens, it is [`Self::new`]'s view exactly. A host
    /// asks for it on its draw side only; nothing the simulation reads
    /// projects through it.
    pub fn presented(mut self) -> Self {
        self.lead = camera_present_lead(self.world, self.camera);
        self
    }

    /// The resolver's fallback focus ([`scene_centre`] of the box).
    pub fn centre(&self) -> [f32; 2] {
        scene_centre(self.scene_box)
    }

    /// The frame's camera: `cutscene` is the scripted shot when the host's
    /// gate says one owns the frame (already glided - the glide is the
    /// host's step, not a read), `None` otherwise.
    pub fn frame(&self, cutscene: Option<FieldCameraView>) -> FieldCameraFrame {
        resolve_field_camera_led(self.world, self.camera, cutscene, self.centre(), self.lead)
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
    pub fn fog_prims(
        &self,
        cutscene: Option<FieldCameraView>,
        enhanced_lighting: bool,
    ) -> Vec<ScreenPrim> {
        let world = self.world;
        let mut out: Vec<ScreenPrim> = Vec::new();
        if World::fog_mode(world.draw_mode()) && world.fog.gate {
            // The sheets follow the camera this frame draws through
            // (`World::fog_reprojection`): the smooth camera moves it on
            // the vsyncs between the pool's passes.
            let drawn = self.frame(cutscene).field_view();
            let reproject = world.fog_reprojection(drawn.as_ref());
            // A `filter_map` gives `extend` no size, and the per-frame vec
            // would grow by doubling: one allocation, as before the cull.
            out.reserve(world.fog_quads().len());
            out.extend(
                world
                    .fog_quads()
                    .iter()
                    .filter_map(|q| match &reproject {
                        Some(v) => q.reproject(v),
                        None => Some(*q),
                    })
                    .map(|q| {
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
        if !World::fog_mode(self.world.draw_mode())
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
        if requests.is_empty() || self.world.draw_mode() != SceneMode::Field {
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
        if world.draw_mode() != SceneMode::Field || world.script_actors.lights.is_empty() {
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
/// projected as the emitter projects it, through the billboard projector
/// under the integer matrix the battle frame loads
/// (`battle_trail::project_trail_point` over
/// `FieldGteView::from_battle_pose(world.battle_cam_pose())`), onto the
/// retail 320x240 stage. No host projects a trail point of its own.
pub fn weapon_trail_prims(world: &World) -> Vec<ScreenPrim> {
    trail_prims(world, None, false)
}

/// [`weapon_trail_prims`] as the hosts draw them: the drawn camera and the
/// drawn body (`World::battle_draw_pose`, `World::battle_draw_root_i16`),
/// the smooth battle camera's frame. [`weapon_trail_prims`] keeps the live
/// ones, which is what the frame census counts.
pub fn weapon_trail_prims_drawn(world: &World) -> Vec<ScreenPrim> {
    trail_prims(world, None, true)
}

/// [`weapon_trail_prims`] with every band depth-tested at the OT entry
/// `FUN_800485BC` links it at, through `vp` (the battle view-projection the
/// scene draws with): PSX mode's battle frame, which sorts the bodies by
/// link too.
pub fn weapon_trail_prims_linked(world: &World, vp: &[f32; 16]) -> Vec<ScreenPrim> {
    trail_prims(world, Some(vp), false)
}

/// [`weapon_trail_prims_linked`] on the drawn frame
/// ([`weapon_trail_prims_drawn`]).
pub fn weapon_trail_prims_linked_drawn(world: &World, vp: &[f32; 16]) -> Vec<ScreenPrim> {
    trail_prims(world, Some(vp), true)
}

/// The battle ordering table's shift byte `0x1F8003A4` (`2` in every
/// catalogued battle state), which the projector `FUN_800195A8` shifts its
/// depth by (`srlv` at `0x80019694`): a band's corners carry `(SZ >> 2) >> 2`.
const BATTLE_OT_SHIFT: u32 = legaia_render_kernels::prim_near_reject::BATTLE_SHIFT_DEFAULT;

fn trail_prims(world: &World, link_vp: Option<&[f32; 16]>, drawn: bool) -> Vec<ScreenPrim> {
    use legaia_render_kernels::battle_trail as bt;
    let draws = crate::battle_draws::battle_weapon_trail_draws(world);
    if draws.is_empty() {
        return Vec::new();
    }
    // On the drawn frame, the drawn camera and the drawn body
    // (`World::battle_draw_pose`, `World::battle_draw_root_i16`); the live
    // ones otherwise.
    let pose = if drawn {
        world.battle_draw_pose()
    } else {
        world.battle_cam_pose()
    };
    let view = legaia_render_kernels::gte::FieldGteView::from_battle_pose(&pose);
    let mut out = Vec::new();
    for d in draws {
        let slot = d.actor_slot as usize;
        let root = if drawn {
            world.battle_draw_root_i16(slot)
        } else {
            world.actors.get(slot).map(|a| {
                (
                    [a.move_state.world_x, a.move_state.world_z],
                    a.battle.facing_angle,
                )
            })
        };
        let Some((base, yaw)) = root else {
            continue;
        };
        let steps: Vec<[bt::TrailVertex; bt::TRAIL_POINTS]> = d
            .steps
            .iter()
            .map(|s| {
                s.map(|t| {
                    bt::project_trail_point(
                        &view,
                        BATTLE_OT_SHIFT,
                        bt::trail_world_point(base, yaw, t),
                    )
                })
            })
            .collect();
        match link_vp {
            Some(vp) => {
                let depth = |entry: i32| {
                    legaia_engine_field::overworld_draw_order::ndc_at_w(
                        vp,
                        bt::ot_entry_view_depth(entry).max(1.0),
                    )
                };
                out.extend(bt::weapon_trail_prims_linked(
                    &steps,
                    d.rgb,
                    bt::WEAPON_TRAIL_OT,
                    &depth,
                ));
            }
            None => out.extend(bt::weapon_trail_prims(&steps, d.rgb, bt::WEAPON_TRAIL_OT)),
        }
    }
    out
}

/// [`move_fx_streak_quads`] as screen primitives at `ot_index`, each
/// depth-tested at its billboard's own depth when `by_link`: retail links the
/// afterimage and every ribbon packet at the projector's return for the
/// anchor (`FUN_800195A8` -> `AddPrim` in `FUN_801E1AB0` / `FUN_801E1D98`),
/// the launch point (lifted `+0x120` for the single afterimage) or the
/// clip-tag ribbon's seat. PSX mode's battle frame takes it; otherwise the
/// packets composite over the scene.
pub fn move_fx_streak_prims(
    world: &World,
    mvp: &glam::Mat4,
    ot_index: u32,
    by_link: bool,
) -> Vec<ScreenPrim> {
    use legaia_render_kernels::screen_prim::CornerDepth;
    let anchor = |p: [f32; 3]| {
        let clip = *mvp * glam::Vec4::new(p[0], p[1], p[2], 1.0);
        (by_link && clip.w > 1e-4).then(|| CornerDepth::new([clip.z / clip.w; 4]))
    };
    let mut out = Vec::new();
    let mut push = |quads: Vec<legaia_render_kernels::afterimage::AfterimageQuad>,
                    depth: Option<CornerDepth>| {
        out.extend(quads.iter().map(|q| {
            let mut prim = q.to_screen_prim(ot_index);
            if let ScreenPrim::Textured(t) = &mut prim {
                t.depth = depth;
            }
            prim
        }));
    };
    // The pass's streak emitter built its packets on the game's stream
    // (`World::deal_move_fx_streak`, `BattleState::move_fx_streak_quads`):
    // drawn as built, at the launch point's depth (lifted `+0x120` for the
    // single afterimage, which is the only one-packet case).
    if let Some(quads) = world.battle.move_fx_streak_quads.as_ref()
        && let Some(launch) = world.move_fx_streak().launch
    {
        let lift = if quads.len() == 1 {
            f32::from(legaia_render_kernels::afterimage::SCREEN_Y_OFFSET)
        } else {
            0.0
        };
        let at = [launch.0 as f32, launch.1 as f32 + lift, launch.2 as f32];
        push(quads.clone(), anchor(at));
    }
    if let (Some(c), Some(quads)) = (
        world.battle.clip_ribbon,
        world.battle.clip_ribbon_quads.as_ref(),
    ) {
        push(quads.clone(), anchor(c.seat.map(f32::from)));
    }
    out
}

/// This frame's move-FX afterimage streak, then the clip-tag ribbon, as
/// stage quads through `mvp` (the host's battle model-view-projection at the
/// stage's 4:3).
///
/// The streak is the packets the pass's emitter built
/// (`BattleState::move_fx_streak_quads`, dealt by `World::deal_move_fx_streak`
/// on `FUN_801E09F8`'s gates: slot `0` in phase `1`, the counter running and
/// the record's `+0x0E` list byte `0xFF`), drawn as built, then the packets
/// of Gala's clip-tag ribbon (`FUN_8004CE2C` tag `0x67` ->
/// `FUN_801E1D98(&target[+0x3C], 0xC)`, `BattleState::clip_ribbon_quads`),
/// which rides Heaven's Drop's clip rather than a move-FX scene.
pub fn move_fx_streak_quads(world: &World, _mvp: &glam::Mat4) -> Vec<AfterimageQuad> {
    let mut out = world
        .battle
        .move_fx_streak_quads
        .clone()
        .unwrap_or_default();
    if let Some(quads) = world.battle.clip_ribbon_quads.as_ref() {
        out.extend(quads.iter().copied());
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
    cast_ribbon_prims_at(world, mvp, ot_index, false)
}

/// [`cast_ribbon_prims`] through the drawn camera
/// (`World::battle_draw_pose`), as the hosts draw it.
pub fn cast_ribbon_prims_drawn(
    world: &World,
    mvp: &glam::Mat4,
    ot_index: u32,
) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
    cast_ribbon_prims_at(world, mvp, ot_index, true)
}

fn cast_ribbon_prims_at(
    world: &World,
    mvp: &glam::Mat4,
    ot_index: u32,
    drawn: bool,
) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
    use legaia_render_kernels::screen_prim::{CornerDepth, ScreenPrim};
    let frame = world.clock.display_frames as u32;
    let pose = if drawn {
        world.battle_draw_pose()
    } else {
        world.battle_cam_pose()
    };
    let view = legaia_render_kernels::gte::FieldGteView::from_battle_pose(&pose);
    let mut out = Vec::new();
    // A landed move projectile's bolts (`BattleState::flight_ribbons`) are the
    // same call on the slot's position, numbered on after the arm's points.
    // Each ring replays the words its pass dealt it
    // (`World::deal_cast_ribbon_rands`, `World::deal_flight_ribbon_rands`).
    //
    // The packets the dealing pass built are what retail's frame shows
    // (`BattleState::cast_ribbon_quads`): built once, through the view and off
    // the point the pass held, and drawn as built. Re-running the call here
    // on the drawn camera and point moves the spans every jitter draw is
    // taken modulo, so the same words draw another ribbon - PROT 0909's arm
    // 12 ring drew sixteen other bolts a frame that way.
    let rings = [
        (
            false,
            world.battle.cast_ribbons,
            world.battle.cast_ribbon_seeds,
            world.battle.cast_ribbon_quads.as_deref(),
        ),
        (
            true,
            world.battle.flight_ribbons,
            world.battle.flight_ribbon_seeds,
            world.battle.flight_ribbon_quads.as_deref(),
        ),
    ];
    let points = rings.iter().flat_map(|(flight, ring, seeds, built)| {
        ring.iter().flat_map(move |ring| {
            ring.points
                .iter()
                .take(usize::from(ring.count))
                .enumerate()
                .map(move |(k, p)| {
                    (
                        (*flight, k),
                        *p,
                        ring.trail_id,
                        seeds.map(|s| s[k]),
                        built.and_then(|b| b.get(k)),
                    )
                })
        })
    });
    for (i, ((flight, k), mut p, trail_id, seed, built)) in points.enumerate() {
        // On the drawn frame each point stands where the drawn phase puts
        // it (`World::battle_ribbon_point_shift`), so a ribbon riding a body
        // or a projectile stays on it; the live point otherwise.
        if drawn && let Some(d) = world.battle_ribbon_point_shift(flight, k) {
            p = std::array::from_fn(|k| (f32::from(p[k]) + d[k]).round() as i16);
        }
        // The points are retail battle space (Y down, the stack trio the arm
        // hands `FUN_801E1D98`); the battle view-projection takes the drawn
        // frame, whose models carry the `scale(1,-1,1)` it cancels
        // (`legaia_engine_camera::projection::battle_vp`), so a point off the
        // floor is lifted by negating its height. PROT 0904's bolts at `-0x80`
        // and PROT 0934's ring at `-0xC0` stand above the ground.
        let at = [f32::from(p[0]), -f32::from(p[1]), f32::from(p[2])];
        let clip = *mvp * glam::Vec4::new(at[0], at[1], at[2], 1.0);
        let depth = (clip.w > 1e-4).then(|| CornerDepth::new([clip.z / clip.w; 4]));
        let quads = match (built, seed) {
            (Some(built), _) => built.clone(),
            (None, Some(seed)) => {
                legaia_render_kernels::streak_pass::ribbon_quads_seeded(&view, p, trail_id, seed).0
            }
            (None, None) => legaia_render_kernels::streak_pass::clip_ribbon_quads(
                at,
                trail_id,
                mvp,
                frame.wrapping_mul(17).wrapping_add(i as u32 + 1),
            ),
        };
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

    /// A module ring's ribbons draw the packets the dealing pass built, not a
    /// rebuild on the drawn camera: the jitter spans come off the projected
    /// box, so a rebuild a unit off draws another bolt on the same words.
    #[test]
    fn a_dealt_ribbon_draws_the_packets_its_call_built() {
        use legaia_render_kernels::afterimage::AfterimageQuad;
        use legaia_render_kernels::screen_prim::ScreenPrim;
        let built = AfterimageQuad {
            xy: [(11, 22), (33, 22), (11, 99), (33, 99)],
            uv: [(0, 0), (0x1F, 0), (0, 0x3F), (0x1F, 0x3F)],
            clut: 0x7705,
            tpage: 0x27,
            color: 0x2E80_8080,
            semi_transparent: true,
        };
        let mut world = World::default();
        let mut ring = world.battle.cast_ribbons.unwrap_or_default();
        ring.points[0] = [0, 0, 0x800];
        ring.trail_id = 5;
        ring.count = 1;
        world.battle.cast_ribbons = Some(ring);
        world.battle.cast_ribbon_seeds = Some([0x1234_5678; 16]);
        world.battle.cast_ribbon_quads = Some(vec![vec![built]]);
        let prims = cast_ribbon_prims(&world, &glam::Mat4::IDENTITY, 7);
        assert_eq!(prims.len(), 1);
        let ScreenPrim::Textured(q) = &prims[0] else {
            panic!("a ribbon packet is a textured quad");
        };
        assert_eq!(q.xy, built.xy);
        assert_eq!((q.clut, q.tpage), (built.clut, built.tpage));
    }

    #[test]
    fn every_pass_is_empty_off_its_mode() {
        let world = World::default();
        let cam = Camera::default();
        let v = HostView::new(&world, &cam, AABB);
        assert!(v.fog_prims(None, true).is_empty());
        assert!(v.drop_shadow_prims(None, false).is_empty());
        assert!(v.move_strip_prims(None).is_empty());
        assert!(v.light_prims(None).is_empty());
        assert!(v.world_map_marker_prims(true).is_empty());
        assert!(v.world_map_sky_prims().is_empty());
        assert_eq!(v.curve_scale(None), 0.0);
        assert!(weapon_trail_prims(&world).is_empty());
        assert!(move_fx_streak_quads(&world, &glam::Mat4::IDENTITY).is_empty());
        assert!(move_fx_streak_prims(&world, &glam::Mat4::IDENTITY, 0x18, true).is_empty());
        assert!(weapon_trail_prims_linked(&world, &[0.0; 16]).is_empty());
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
        // The blob's grid goes through the GTE's integer camera, the matrix
        // `FUN_800172C0` loads for the view.
        let centre = |view: &FieldCameraView, p: [i32; 3]| {
            legaia_render_kernels::gte::FieldGteView::from_field_view(
                view,
                legaia_engine_camera::projection::GTE_OFY as i32,
            )
            .rtps(p.map(|c| c as i16))
            .sxy
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
