//! Field script actors: the op `0x43` sub-0/1/A/B scripted **arc jump** and
//! the op `0x34` sub-1 **attached light**, the two pool-actor families the
//! field VM spawns onto the actor a script is running on.
//!
//! Retail seats each as a field-overlay pool actor back-linked to its target
//! through `+0x90`. The engine has no field actor pool, so the back-link is a
//! [`ScriptActorRef`] - the player, or a field NPC's placement slot - and the
//! records live in [`FieldScriptActorState`]. The retail arithmetic is the
//! `legaia_engine_vm` ports ([`hop_arc`], [`billboard`]); this file resolves
//! targets, advances the records once per field frame, and projects the
//! lights for the two play hosts, which draw them through
//! `legaia_engine_ui::screen_prim::light_pool_prims`.
//!
//! REF: FUN_801DE840 (op `0x43` arm `0x801DF384..0x801DF5B8`, op `0x34` sub-1 arm
//! `0x801DFEFC..0x801E0018`), FUN_8003CF04, FUN_80019278

use super::*;
use legaia_engine_vm::field_actor_billboard as billboard;
use legaia_engine_vm::field_ledge_hop_arc as hop_arc;
use legaia_engine_vm::psx_camera::FieldCameraView;

/// One attached light, projected for this frame: the blend mode and the
/// light pool's primitives in link order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldLightDraw {
    /// `+0x5A` - see `legaia_engine_vm::field_actor_billboard::light_abr`.
    pub abr: u8,
    /// `FUN_801E3984`'s primitives.
    pub polys: Vec<billboard::LightPoly>,
}

impl World {
    /// Scene-entry reset: no arcs, no NPC heights, no lights. Retail's pool
    /// is re-allocated with the field overlay on every entry.
    pub fn reset_field_script_actors(&mut self) {
        self.script_actors = FieldScriptActorState::default();
    }

    /// Resolve the actor a field-VM op is running against, the way retail's
    /// dispatcher prelude resolves `s5` (`FUN_8003C83C` on the extended
    /// channel byte, `0x801DE8A8`): `0xF8` is the player anchor, any other
    /// resolved channel is the field NPC whose placement script it runs.
    /// `None` for the scene system context and object-bind records, which
    /// have no drawn body the engine can move.
    pub(crate) fn resolve_script_actor(&self, ext: Option<u8>) -> Option<ScriptActorRef> {
        if ext == Some(0xF8) {
            return self.player_actor_slot.map(|_| ScriptActorRef::Player);
        }
        self.field_vm.executing_channel.map(ScriptActorRef::Npc)
    }

    /// A script actor's live `+0x14..+0x18`, raw retail Y-down.
    pub fn script_actor_position(&self, actor: ScriptActorRef) -> Option<(i16, i16, i16)> {
        match actor {
            ScriptActorRef::Player => {
                let slot = self.player_actor_slot? as usize;
                let ms = &self.actors.get(slot)?.move_state;
                Some((ms.world_x, ms.world_y, ms.world_z))
            }
            ScriptActorRef::Npc(slot) => {
                // A drawn NPC's live position first; a placement the talk /
                // draw catalogue does not carry (a lamp-post marker, a
                // scripted helper) stands where its script channel's context
                // says - the `+0x14` / `+0x18` its own `MoveTo`s write.
                let (x, z) = match self.npcs.positions.get(&slot) {
                    Some(&p) => p,
                    None => {
                        let chans = if self.field_vm.channels.is_empty() {
                            &self.field_vm.stepping_view
                        } else {
                            &self.field_vm.channels
                        };
                        let c = chans
                            .iter()
                            .find(|c| !c.object_bind && c.placement_index == usize::from(slot))?;
                        (c.ctx.world_x as i16, c.ctx.world_z as i16)
                    }
                };
                Some((x, self.field_npc_render_y(slot, x, z) as i16, z))
            }
        }
    }

    /// The Y a field NPC draws at, standing at `(x, z)`: the height a
    /// scripted arc left it at while it still stands where the arc put it,
    /// else the floor under it (`FUN_80019278`). Both play hosts place NPCs
    /// through this, so an arcing NPC leaves the ground on either.
    pub fn field_npc_render_y(&self, slot: u8, x: i16, z: i16) -> i32 {
        match self.script_actors.npc_heights.get(&slot) {
            Some(&(hx, hz, y)) if (hx, hz) == (x, z) => i32::from(y),
            _ => self.sample_field_floor_height(i32::from(x), i32::from(z)),
        }
    }

    /// Start a scripted arc on `actor`: the op `0x43` sub-0/1/A/B arm's call
    /// to `FUN_801D25EC` (`0x801DF5AC`). The landing point is built from the
    /// operand ([`hop_arc::ScriptArcRequest::landing`], the floor under a tile
    /// target standing in for `FUN_80019278`), the clip seeded by
    /// [`hop_arc::spawn_arc_with_emitter`]. A second arc on an actor that is
    /// still arcing replaces the first.
    ///
    /// `release_channel` is the placement slot whose channel(s) the landing
    /// un-halts - see [`FieldScriptArc::release_channel`].
    ///
    /// Returns `false` when the actor has no position to arc from.
    pub fn start_field_script_arc(
        &mut self,
        actor: ScriptActorRef,
        req: &hop_arc::ScriptArcRequest,
        release_channel: Option<u8>,
    ) -> bool {
        let Some(start) = self.script_actor_position(actor) else {
            return false;
        };
        let target = req.landing(start, |x, z| {
            self.sample_field_floor_height(i32::from(x), i32::from(z)) as i16
        });
        let is_player = actor == ScriptActorRef::Player;
        let Some(spawn) = hop_arc::spawn_arc_with_emitter(
            Some(start),
            is_player,
            target,
            req.apex,
            req.frames,
            // The context to release is resolved per actor on landing
            // (`Self::tick_field_script_arcs`), not stored as a pointer.
            0,
            0x400,
            u8::from(req.camera_follow()),
        ) else {
            return false;
        };
        let Some(watcher) = spawn.emitter else {
            return false;
        };
        self.script_actors.arcs.retain(|a| a.actor != actor);
        self.script_actors.arcs.push(FieldScriptArc {
            actor,
            arc: spawn.arc,
            watcher,
            release_channel,
        });
        true
    }

    /// `true` while the player is mid scripted arc - the caller context that
    /// halted with it stays parked until this clears.
    pub fn player_script_arc_live(&self) -> bool {
        self.script_actors
            .arcs
            .iter()
            .any(|a| a.actor == ScriptActorRef::Player)
    }

    /// `true` while `actor` is mid scripted arc.
    pub fn script_arc_live(&self, actor: ScriptActorRef) -> bool {
        self.script_actors.arcs.iter().any(|a| a.actor == actor)
    }

    /// Advance every scripted arc one field frame: the arc helper's
    /// `FUN_801D5C08` ([`hop_arc::advance_hop_arc`]) writes the actor's
    /// position, then the watcher's `FUN_801D5D60`
    /// ([`hop_arc::release_watcher_tick`]) releases the halt once the helper
    /// retires, clearing the halt bit on every channel of
    /// [`FieldScriptArc::release_channel`]; a timeline that started a player
    /// arc parks on [`Self::player_script_arc_live`] instead.
    ///
    /// `DAT_1F800393` is the frame-delta scalar both the arc and the ledge
    /// hop multiply their cursor step by.
    pub fn tick_field_script_arcs(&mut self) {
        if self.script_actors.arcs.is_empty() {
            return;
        }
        let scalar = self.move_vm.ramp_ratio.max(1);
        let mut arcs = std::mem::take(&mut self.script_actors.arcs);
        arcs.retain_mut(|a| {
            let tick = hop_arc::advance_hop_arc(&mut a.arc, scalar);
            let (x, y, z) = tick.position;
            match a.actor {
                ScriptActorRef::Player => {
                    if let Some(slot) = self.player_actor_slot
                        && let Some(actor) = self.actors.get_mut(slot as usize)
                    {
                        actor.move_state.world_x = x;
                        actor.move_state.world_y = y;
                        actor.move_state.world_z = z;
                    }
                }
                ScriptActorRef::Npc(slot) => {
                    self.npcs.positions.insert(slot, (x, z));
                    self.script_actors.npc_heights.insert(slot, (x, z, y));
                }
            }
            let watch = hop_arc::release_watcher_tick(&a.watcher, tick.arrived);
            if let Some(mask) = watch.release_mask {
                if let Some(slot) = a.release_channel {
                    for ch in self
                        .field_vm
                        .channels
                        .iter_mut()
                        .filter(|c| !c.object_bind && c.placement_index == usize::from(slot))
                    {
                        ch.ctx.flags &= !mask;
                    }
                }
                return false;
            }
            true
        });
        // An arc started during this tick's own writes is impossible (no VM
        // runs in here), so the taken list is the whole set.
        self.script_actors.arcs = arcs;
    }

    /// Spawn an attached light on the actor an op `0x34` sub-1 runs against:
    /// the arm's actor-list walk (`FUN_8003CF04` keyed on the tick
    /// `0x801E4470`, `0x801DFF04..0x801DFF48`) refuses a second light on one
    /// parent, then `FUN_801E5668` seeds the record
    /// ([`billboard::spawn_attached_sprite`]) and the arm captures a following
    /// `0x40` block as its keyframe script.
    ///
    /// Returns whether a light was spawned.
    pub fn spawn_field_attached_light(
        &mut self,
        ext: Option<u8>,
        spawn: &billboard::AttachedSpriteSpawn,
        script: Option<&[u8]>,
    ) -> bool {
        let Some(parent) = self.resolve_script_actor(ext) else {
            return false;
        };
        if self.script_actors.lights.iter().any(|l| l.parent == parent) {
            return false;
        }
        let script = script
            .map(|s| s[..billboard::attached_script_extent(s)].to_vec())
            .unwrap_or_default();
        self.script_actors.lights.push(FieldAttachedLight {
            parent,
            sprite: billboard::spawn_attached_sprite(spawn, script),
        });
        true
    }

    /// A light's parent flags as `FUN_801E4470` tests them: `8` when the
    /// parent is gone (the tear-down the light follows), `2` when it is
    /// parked off-map or collapsed to a point (hidden - the light skips its
    /// draw and its script).
    fn attached_light_parent_flags(
        &self,
        parent: ScriptActorRef,
    ) -> Option<(u32, (i16, i16, i16))> {
        let Some(pos) = self.script_actor_position(parent) else {
            return Some((billboard::parent_flag::TEARDOWN, (0, 0, 0)));
        };
        let hidden = match parent {
            ScriptActorRef::Player => false,
            ScriptActorRef::Npc(slot) => {
                let hide = crate::world::FIELD_OFFMAP_HIDE_XZ;
                (pos.0 == hide && pos.2 == hide)
                    || self.field_npc_render_scale(usize::from(slot)) == Some(0)
            }
        };
        Some((
            if hidden {
                billboard::parent_flag::HIDDEN
            } else {
                0
            },
            pos,
        ))
    }

    /// Advance every attached light one field frame: the tear-down half of
    /// `FUN_801E4470` (a light whose parent is gone retires with it) and its
    /// keyframe script `FUN_801E3E00`, which retail runs only on a frame the
    /// light draws - a hidden parent freezes the script as well.
    pub fn tick_field_attached_lights(&mut self) {
        if self.script_actors.lights.is_empty() {
            return;
        }
        let mut lights = std::mem::take(&mut self.script_actors.lights);
        lights.retain_mut(|l| {
            let flags = self
                .attached_light_parent_flags(l.parent)
                .map_or(billboard::parent_flag::TEARDOWN, |f| f.0);
            if flags & billboard::parent_flag::TEARDOWN != 0 {
                return false;
            }
            if flags & billboard::parent_flag::HIDDEN == 0 {
                billboard::attached_sprite_script_tick(&mut l.sprite, 1);
            }
            !l.sprite.torn_down
        });
        self.script_actors.lights = lights;
    }

    /// This frame's attached lights, projected through `view` and built into
    /// the light pool's primitives - the draw half of `FUN_801E4470`
    /// ([`billboard::attached_sprite_tick`]) feeding `FUN_801E3984`
    /// ([`billboard::light_pool_polys`]).
    ///
    /// The projection stands in for `FUN_800195A8`: the parent-plus-offset
    /// point goes through the same view-projection the fog sheets use, and
    /// the light's view-space half extents scale by `H / z` at that point's
    /// eye depth - the billboard's four corners are that point `+-` the
    /// extents in view space, divided by one shared `z`. Empty outside a
    /// field scene.
    pub fn field_light_draws(&self, view: &FieldCameraView) -> Vec<FieldLightDraw> {
        if self.mode != SceneMode::Field || self.script_actors.lights.is_empty() {
            return Vec::new();
        }
        let fog_view = crate::fog_particles::FogView::from_field_view(view);
        let mut out = Vec::new();
        for l in &self.script_actors.lights {
            let parent = self.attached_light_parent_flags(l.parent);
            let mut rect = None;
            billboard::attached_sprite_tick(
                parent,
                l.sprite.offset,
                l.sprite.half_extent,
                false,
                |world, (hw, hh)| {
                    let p = [i32::from(world.0), i32::from(world.1), i32::from(world.2)];
                    let eye = view.eye_space([p[0] as f32, p[1] as f32, p[2] as f32]);
                    match fog_view.project(p) {
                        Some((sx, sy, _)) if eye[2] > 1.0 => {
                            let k = view.h / eye[2];
                            let ex = (f32::from(hw) * k).round() as i32;
                            let ey = (f32::from(hh) * k).round() as i32;
                            let c = |v: i32| v.clamp(-1024, 1023) as i16;
                            billboard::ProjectedQuad {
                                p0: (c(sx - ex), c(sy - ey)),
                                p1: (c(sx + ex), c(sy - ey)),
                                p2: (c(sx - ex), c(sy + ey)),
                                p3: (c(sx + ex), c(sy + ey)),
                            }
                        }
                        // Behind the eye: a zero rect, dropped below.
                        _ => billboard::ProjectedQuad::default(),
                    }
                },
                |r| rect = Some(r),
            );
            let Some(rect) = rect else { continue };
            if rect.width <= 0 || rect.height <= 0 {
                continue;
            }
            out.push(FieldLightDraw {
                abr: l.sprite.abr,
                polys: billboard::light_pool_polys(&rect, l.sprite.color_a, l.sprite.color_b),
            });
        }
        out
    }
}
