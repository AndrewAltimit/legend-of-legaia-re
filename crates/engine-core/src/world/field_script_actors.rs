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
//! `legaia_render_kernels::screen_prim::light_pool_prims`.
//!
//! REF: FUN_801DE840 (op `0x43` arm `0x801DF384..0x801DF5B8`, op `0x34` sub-1 arm
//! `0x801DFEFC..0x801E0018`), FUN_8003CF04, FUN_80019278

use super::*;
use legaia_engine_camera::psx_camera::FieldCameraView;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_vm::field_actor_billboard as billboard;
use legaia_engine_vm::field_ledge_hop_arc as hop_arc;

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
        // The talk runner steps a talker's own op with no channel marked
        // executing: no target byte, or a target byte naming the talker.
        let talker = self
            .dialog
            .stepping_inline_npc
            .filter(|&slot| ext.is_none_or(|t| self.placement_slot_for_target(t) == Some(slot)));
        self.field_vm
            .executing_channel
            .or(talker)
            .map(ScriptActorRef::Npc)
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
    /// else the glide height when its flag word has it on `FUN_8003BC08`'s
    /// glide arm ([`Self::tick_field_npc_heights`]), else the floor under it
    /// (`FUN_80019278`) - the snap arm every other placement takes. Both play
    /// hosts place NPCs through this, so an arcing or gliding NPC leaves the
    /// ground on either.
    pub fn field_npc_render_y(&self, slot: u8, x: i16, z: i16) -> i32 {
        match self.script_actors.npc_heights.get(&slot) {
            Some(&(hx, hz, y)) if (hx, hz) == (x, z) => i32::from(y),
            _ => match self.npcs.glide_y.get(&slot) {
                Some(&y) => i32::from(y),
                None => self.sample_field_floor_height(i32::from(x), i32::from(z)),
            },
        }
    }

    /// Where a field NPC / scene-actor prop stands this frame, as both play
    /// hosts draw it: its live `(x, z)` (the placement anchor `spawn` for one
    /// that never moved), at [`Self::field_npc_render_y`]. `None` while it is
    /// not drawn - render scale `0` ([`Self::field_npc_render_scale`]) or
    /// parked in the off-map hide box. The anchor enhanced lighting moves a
    /// prop's lamp set to (`legaia_render_kernels::scene_lighting::place_prop_lights`).
    pub fn field_npc_live_anchor(&self, slot: u8, spawn: (i16, i16)) -> Option<[f32; 3]> {
        let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;
        let (x, z) = self.npcs.positions.get(&slot).copied().unwrap_or(spawn);
        if (x, z) == (hide, hide) || self.field_npc_render_scale(usize::from(slot)) == Some(0) {
            return None;
        }
        let y = self.field_npc_render_y(slot, x, z);
        Some([f32::from(x), y as f32, f32::from(z)])
    }

    /// A field NPC's whole draw pose this frame, as both play hosts build its
    /// model matrix: where ([`Self::field_npc_live_anchor`], retail Y-down),
    /// which way it faces, its tilt and its render scale. `None` while it is
    /// not drawn.
    ///
    /// Retail's dispatcher (`FUN_8001ADA4`) hands the actor's angle triple at
    /// `+0x24` whole to the composer (`addiu a0,s0,0x24` / `jal 0x80026988`
    /// at `0x8001AF04`, X at `+0`, Y at `+2`, Z at `+4`), and before it folds
    /// `actor[+0x72]` in as a uniform scale whenever it is not `0x1000`
    /// (`lhu v1,0x72(s0)` / `li v0,0x1000` / `beq` at `0x8001B240`, the
    /// three `sw` of the word and `jal 0x8005B4E8` at `0x8001B288`). Both
    /// hosts drew every NPC at unit scale and composed the yaw from two
    /// different encodings of the same rule; this is the one rule.
    // REF: FUN_8001ADA4, FUN_80026988
    pub fn field_npc_draw_pose(&self, slot: u8, spawn: (i16, i16)) -> Option<NpcDrawPose> {
        let pos = self.field_npc_live_anchor(slot, spawn)?;
        let u = |v: i32| v.rem_euclid(4096) as u16;
        // Engine headings compose with the half turn (`render_26`); an NPC
        // nothing has turned stands at the identity yaw, which is where the
        // spawn default (`SPAWN_HEADING`, retail `+0x26 = 0`) composes to.
        let yaw = u(self
            .npcs
            .headings
            .get(&slot)
            .map_or(0, |&h| i32::from(h) + 2048));
        let (pitch, roll) = self.field_npc_tilt(slot).unwrap_or((0, 0));
        let scale = match self.field_npc_render_scale(usize::from(slot)) {
            Some(s) if s != 0x1000 => f32::from(s) / 4096.0,
            _ => 1.0,
        };
        // An op-`43 07` deformation does not move the pose: it breaks the
        // mesh (`Self::field_npc_shatter`).
        Some(NpcDrawPose {
            pos,
            yaw,
            pitch: u(i32::from(pitch)),
            roll: u(i32::from(roll)),
            scale,
        })
    }

    /// Field NPC `slot`'s op-`43 07` deformation this frame, `None` while its
    /// `+0x7A` is zero. Retail's object draw (`FUN_8001B964`) hands every
    /// object of a body whose `+0x7A` is non-zero to the shatter renderer
    /// `FUN_80029888` with `+0x7A | row[+8] << 16`, `row` the deformation
    /// table row its `+0x6D` names; the pose ([`Self::field_npc_draw_pose`])
    /// is unchanged. Both hosts break the NPC's mesh with
    /// `legaia_render_kernels::shatter` from this.
    // REF: FUN_8001B964, FUN_80029888
    pub fn field_npc_shatter(
        &self,
        slot: u8,
    ) -> Option<legaia_render_kernels::shatter::ShatterInput> {
        let &(id, value) = self.npcs.faces.get(&slot)?;
        legaia_render_kernels::shatter::ShatterInput::new(value, self.shatter_row(id))
    }

    /// Row `id` of the deformation table `0x8007BE60` (op `43 07`'s writes;
    /// global in retail, so the battle reads the same rows). A row nothing
    /// wrote reads zero.
    pub fn shatter_row(&self, id: u8) -> legaia_render_kernels::shatter::ShatterRow {
        let r = self.npcs.face_rows.get(&id).copied().unwrap_or_default();
        legaia_render_kernels::shatter::ShatterRow {
            off: [r.offset[0], r.offset[1], r.offset[2]],
            h3: r.offset[3],
            flags: r.flags,
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
            halt: true,
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

    /// `true` while `actor` is mid scripted arc **and** still carries the
    /// arc's halt bit `+0x10 & 0x400` - what the field VM's busy gate
    /// (`0x801DE90C..0x801DE944`) and the arc acquire's refusal read.
    // REF: FUN_801DE840
    pub fn script_arc_halts(&self, actor: ScriptActorRef) -> bool {
        self.script_actors
            .arcs
            .iter()
            .any(|a| a.actor == actor && a.halt)
    }

    /// The halt clear `32 <id> 0A` (`ctx[+0x10] &= ~0x400`) on an arced
    /// actor: the bit drops, the arc helper keeps flying it to its landing.
    // REF: FUN_801DE840 (op 0x32)
    pub fn clear_script_arc_halt(&mut self, actor: ScriptActorRef) {
        for a in self
            .script_actors
            .arcs
            .iter_mut()
            .filter(|a| a.actor == actor)
        {
            a.halt = false;
        }
    }

    /// `true` while a live arc's release watcher runs the follow camera: a
    /// sub-`1` / `0xB` arc on the player (`+0x5C` and `+0x50` both set), for
    /// which `FUN_801D5D60` calls the follow ease `FUN_801DB510(player)` and
    /// the focus clamp `FUN_801DAA50` every frame the arc flies - a cutscene
    /// that owns the shot included. The camera reads this to run its follow
    /// step through the cutscene ([`legaia_engine_field::camera::Camera`]).
    ///
    /// REF: FUN_801D5D60 (ported as [`hop_arc::release_watcher_tick`])
    pub fn script_arc_follow_camera(&self) -> bool {
        self.script_actors
            .arcs
            .iter()
            .any(|a| hop_arc::release_watcher_tick(&a.watcher, false).camera_follow)
    }

    /// The op-`0x43` arc arm's acquire refusal: the arced actor already
    /// carries the halt bit `0x400` and the scene word `*(_DAT_801C6EA4) + 8`
    /// is zero (`0x801DF3A4..0x801DF3CC`). The field VM then leaves the PC
    /// on the instruction and retries it next frame, so a second arc on an
    /// actor still in the air waits for the first to land instead of
    /// replacing it.
    ///
    /// The scene word is non-zero only while a placement's spawn section is
    /// being pre-run (SCUS `0x8003B73C` / `0x8003B928`, and the two field-VM
    /// re-runs at `0x801E2820` / `0x801E2BBC`); the port runs no arc there,
    /// so it reads as zero. The halt the port can see is an arc still in
    /// flight on that actor.
    ///
    /// REF: FUN_801DE840 (the op-`0x43` sub-`0`/`1`/`0xA`/`0xB` acquire)
    pub fn script_arc_target_halted(&self, ext: Option<u8>) -> bool {
        self.resolve_script_actor(ext)
            .is_some_and(|actor| self.script_arc_halts(actor))
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
                    // A parent that is not the player also takes `-y` into
                    // its held height `+0x8E`, every flight frame and on the
                    // landing (`0x801D5CB0..0x801D5CBC`, `0x801D5D40..
                    // 0x801D5D4C`), so a placement on the held-height arm
                    // (`+0x10 & 0x20000000`) stays where the arc put it.
                    let held = y.wrapping_neg();
                    for ch in self
                        .field_vm
                        .channels
                        .iter_mut()
                        .filter(|c| !c.object_bind && c.placement_index == usize::from(slot))
                    {
                        ch.ctx.field_8e = held;
                    }
                }
            }
            let watch = hop_arc::release_watcher_tick(&a.watcher, tick.arrived);
            if tick.arrived
                && let ScriptActorRef::Npc(slot) = a.actor
            {
                // Landed: the actor driver's height arm (`FUN_8003BC08`) owns
                // `+0x16` again from the next tick, starting from the landing.
                self.script_actors.npc_heights.remove(&slot);
                self.npcs.glide_y.insert(slot, y);
            }
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

    /// Op `34 2x` (`0x801E001C..0x801E0098`): the first live attached light
    /// on the context's actor takes `script` as its keyframe script
    /// (`+0x94`), restarted (`+0x9E = +0x9C = 0`). `false` when the actor
    /// carries no light. `cave01`'s darkness mask and the night lamp pools
    /// change shape this way, the player's light most of all (`B4 F8 20`).
    ///
    /// REF: FUN_801E4470 (the handler the lookup keys on), FUN_801E3E00
    pub fn rescript_field_attached_light(&mut self, ext: Option<u8>, script: &[u8]) -> bool {
        let Some(parent) = self.resolve_script_actor(ext) else {
            return false;
        };
        let Some(light) = self
            .script_actors
            .lights
            .iter_mut()
            .find(|l| l.parent == parent && !l.sprite.torn_down)
        else {
            return false;
        };
        light.sprite.script = script[..billboard::attached_script_extent(script)].to_vec();
        light.sprite.cursor = 0;
        light.sprite.frame = 0;
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
                let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;
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
    /// The projection is `FUN_800195A8`'s. `FUN_801E4470` rebuilds the field
    /// view matrix (`FUN_800172C0`) immediately before the call, so the
    /// parent-plus-offset point goes through `S * Rot` and `TR` - the view
    /// the fog sheets use, at retail's scale - and the four corners are that
    /// view-space point `+-` the extents, projected through an identity
    /// rotation (`FUN_8003D178`, `FUN_8005BAC8`) at one shared depth. The
    /// extents are therefore **view-space units of the scaled view**: the rim
    /// radius is `H * ext / vz` with `vz` the eye depth in retail's `6x`
    /// space. The engine's eye space is the `1x` reduction of that one
    /// (`tr_eye / S`, [`legaia_engine_field::camera::CUTSCENE_WORLD_SCALE`]), so the
    /// extents divide by the same `S` before the `H / z` scale - an extent
    /// of `0x1000` is `0x1000 / 6` world units, and `dolk`'s mask rim lands
    /// where retail's does. The port runs that projector itself
    /// ([`legaia_render_kernels::billboard::project_billboard`]) through the
    /// integer camera ([`legaia_render_kernels::gte::FieldGteView`]), so the
    /// rim lands on retail's pixels; a light at or behind the eye takes the
    /// saturated corners retail's divide gives it. Empty outside a field scene.
    pub fn field_light_draws(&self, view: &FieldCameraView) -> Vec<FieldLightDraw> {
        if self.mode != SceneMode::Field || self.script_actors.lights.is_empty() {
            return Vec::new();
        }
        // The camera matrix `FUN_800172C0` loads, in the GTE's integers.
        let gte = legaia_render_kernels::gte::FieldGteView::from_field_view(
            view,
            legaia_engine_camera::projection::GTE_OFY as i32,
        );
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
                    let c = legaia_render_kernels::billboard::project_billboard(
                        &gte.rot,
                        legaia_render_kernels::gte::GteVec3::new(gte.tr[0], gte.tr[1], gte.tr[2]),
                        legaia_render_kernels::gte::GteVec3::new(
                            i32::from(world.0),
                            i32::from(world.1),
                            i32::from(world.2),
                        ),
                        hw,
                        hh,
                        0,
                        i32::from(gte.h),
                        gte.ofx,
                        gte.ofy,
                        0,
                    );
                    billboard::ProjectedQuad {
                        p0: c.xy[0],
                        p1: c.xy[1],
                        p2: c.xy[2],
                        p3: c.xy[3],
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

/// One field NPC's draw pose ([`World::field_npc_draw_pose`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NpcDrawPose {
    /// World position, retail Y-down.
    pub pos: [f32; 3],
    /// Composed yaw, 12-bit units (`0` = the identity model).
    pub yaw: u16,
    /// Pitch (`actor+0x24`), 12-bit units.
    pub pitch: u16,
    /// Roll (`actor+0x28`), 12-bit units.
    pub roll: u16,
    /// Uniform render scale (`actor+0x72 / 0x1000`).
    pub scale: f32,
}

impl NpcDrawPose {
    /// Whether the pose carries a pitch or roll, so a host needs the full
    /// `Rx * Ry * Rz` composer rather than its yaw-only model.
    pub fn tilted(&self) -> bool {
        self.pitch != 0 || self.roll != 0
    }
}

#[cfg(test)]
mod npc_draw_pose_tests {
    use super::*;

    /// `B4 F8 20 40 <len> ..` re-scripts the player's light (`cave01`'s
    /// darkness mask): the block becomes `+0x94` from its first record and
    /// the op steps past it; with no light the op only advances 2.
    #[test]
    fn op_34_2x_rescripts_the_attached_light() {
        use crate::world::vm_hosts::{FieldHostImpl, field_step_routed};
        use legaia_engine_vm::field::{FieldCtx, StepResult};
        let mut w = World::new();
        w.mode = legaia_engine_mode::mode::SceneMode::Field;
        w.install_field_player(0);
        // 34 2x, 0x40 block of one keyframe record + retire.
        let mut op = vec![0xB4, 0xF8, 0x20, 0x40, 16, 0x02];
        op.extend_from_slice(&[0u8; 14]);
        op.push(0x00);
        let mut ctx = FieldCtx::default();
        let mut host = FieldHostImpl { world: &mut w };
        let r = field_step_routed(&mut host, &mut ctx, &op, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 3 }, "no light: +2");
        let spawn = billboard::AttachedSpriteSpawn {
            op0: 0x11,
            packed_rgb: 0x808080,
            half_extent: (32, 32),
            height: 0,
        };
        assert!(w.spawn_field_attached_light(Some(0xF8), &spawn, None));
        let mut host = FieldHostImpl { world: &mut w };
        let r = field_step_routed(&mut host, &mut ctx, &op, 0);
        assert_eq!(
            r,
            StepResult::Advance {
                next_pc: 3 + 2 + 16
            }
        );
        let light = &w.script_actors.lights[0];
        assert_eq!(light.sprite.script.len(), 16, "the record and the retire");
        assert_eq!((light.sprite.cursor, light.sprite.frame), (0, 0));
    }

    /// Op `43 07`'s row and ramp reach the draw as a shatter input: the pose
    /// is untouched, the row's flags ride the word's high half.
    #[test]
    fn a_face_ramp_becomes_a_shatter_input() {
        let mut w = World::new();
        assert!(w.field_npc_shatter(3).is_none());
        w.npcs.face_rows.insert(
            1,
            crate::world::FieldFaceRow {
                offset: [0, -32, 64, 0],
                flags: 4,
            },
        );
        w.npcs.faces.insert(3, (1, 0));
        assert!(w.field_npc_shatter(3).is_none(), "+0x7A zero: plain draw");
        w.npcs.faces.insert(3, (1, 0x800));
        let s = w.field_npc_shatter(3).expect("shattering");
        assert_eq!(s.word, 0x0004_0800);
        assert_eq!(s.row.off, [0, -32, 64]);
        let p = w.field_npc_draw_pose(3, (100, 200)).expect("drawn");
        assert_eq!(p.scale, 1.0, "no whole-model fold");
    }

    /// The yaw rule both hosts used to spell differently: an NPC nothing has
    /// turned draws at the identity yaw, a seeded heading composes with the
    /// half turn, and the hide box is not drawn at all.
    #[test]
    fn draw_pose_yaw_and_hide() {
        let mut w = World::new();
        let p = w
            .field_npc_draw_pose(3, (100, 200))
            .expect("drawn at spawn");
        assert_eq!((p.pos[0], p.pos[2]), (100.0, 200.0));
        assert_eq!(p.yaw, 0, "unturned = identity");
        assert_eq!(p.scale, 1.0, "no channel = unit scale");
        assert!(!p.tilted());
        w.npcs.headings.insert(3, 0);
        assert_eq!(w.field_npc_draw_pose(3, (100, 200)).unwrap().yaw, 2048);
        w.npcs
            .headings
            .insert(3, crate::world::field_npc_state::SPAWN_HEADING);
        assert_eq!(w.field_npc_draw_pose(3, (100, 200)).unwrap().yaw, 0);
        w.npcs.tilts.insert(3, (-1, 0));
        let t = w.field_npc_draw_pose(3, (100, 200)).unwrap();
        assert!(t.tilted());
        assert_eq!(t.pitch, 4095);
        let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;
        w.npcs.positions.insert(3, (hide, hide));
        assert!(w.field_npc_draw_pose(3, (100, 200)).is_none());
    }

    /// An arc on a placement writes `-y` into the placement's held height
    /// `+0x8E` (`FUN_801D5C08`, `0x801D5CB0` / `0x801D5D40`), so one on the
    /// held-height arm (`+0x10 & 0x20000000`) stands at the landing after it
    /// walks off it. `nilboa` `P2[7]`'s Delilas siblings leap down from
    /// ledges `470..580` up, land on the floor and pose: the port had drawn
    /// them back at their ledge heights once their next `MoveTo` took them
    /// off the landing spot.
    #[test]
    fn an_arc_carries_a_held_placement_s_height_to_its_landing() {
        let mut w = World::new();
        w.mode = legaia_engine_mode::mode::SceneMode::Field;
        w.field_vm
            .channels
            .push(legaia_engine_field::field_channels::FieldChannel {
                placement_index: 4,
                ctx: legaia_engine_vm::field::FieldCtx {
                    world_x: 0x0840,
                    world_y: (-470i16) as u16,
                    world_z: 0x0840,
                    flags: 0x2003_0880,
                    field_8e: 470,
                    ..Default::default()
                },
                record_offset: 0,
                pc: 0,
                done: false,
                object_bind: false,
            });
        w.npcs.positions.insert(4, (0x0840, 0x0840));
        w.npcs.glide_y.insert(4, -470);
        let req = hop_arc::ScriptArcRequest {
            sub_op: 0xA,
            tile_x: 0x12,
            tile_z: 0x10,
            apex: 64,
            frames: 20,
            explicit_y: Some(0),
        };
        assert!(w.start_field_script_arc(ScriptActorRef::Npc(4), &req, Some(4)));
        let mut flown = Vec::new();
        while w.script_arc_live(ScriptActorRef::Npc(4)) {
            w.tick_field_script_arcs();
            flown.push(w.field_vm.channels[0].ctx.field_8e);
            assert!(flown.len() < 100, "the arc lands");
        }
        assert!(
            flown.iter().any(|&h| h > 470),
            "the apex lifts the held height"
        );
        assert_eq!(*flown.last().unwrap(), 0, "landed: `+0x8E = -end.y`");
        // Off the landing spot, the height arm holds it at the landing.
        w.npcs.positions.insert(4, (0x0900, 0x0840));
        w.tick_field_npc_heights();
        assert_eq!(w.field_npc_render_y(4, 0x0900, 0x0840), 0);
    }
}
