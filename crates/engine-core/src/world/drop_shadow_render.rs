//! The field actor drop shadows as a `World` method - the seam both play
//! hosts draw the blob through ([`legaia_render_kernels::drop_shadow`]).
//!
//! Retail emits the blob from inside the animated-actor renderer
//! (`FUN_8001B964` -> `FUN_8001C394`), once per drawn actor that passes the
//! gate. The port has no per-actor retail renderer to hang it off, so this
//! walks the same population from engine state: the player actor, and every
//! MAN partition-1 placement's field-VM channel.

use super::*;
use legaia_engine_camera::psx_camera::FieldCameraView;
use legaia_engine_mode::mode::SceneMode;
use legaia_render_kernels::drop_shadow::{
    DropShadowQuad, SHADOW_NEAR_BIT, SHADOW_NEAR_SLOTS, ShadowVertex, casts_shadow, drop_shadow,
};

/// The OT-resolution byte `DAT_1F8003A4` the port sorts the blobs with.
/// The engine keeps no ordering table of its own depth resolution; the blob
/// only has to sort among the other screen primitives (see
/// `legaia_render_kernels::move_strip::FIELD_OT_SHIFT`, the same choice).
pub const DROP_SHADOW_OT_SHIFT: u8 = 0;

/// How far nearer the eye (world units, along the view axis) the blob's
/// **depth** is taken. The projected corners stay on the floor; only the
/// depth a host tests against its scene moves, so the blob wins the tie with
/// the ground it lies on (retail sorts the ground into the far bucket, and
/// the dance hall links the stage floor under a dancer into the blob's own
/// ordering-table slot, drawn first) without pulling in front of the actor.
/// The pull is along the eye ray, not up the world `Y` axis: under the dance
/// hall's nearly level camera (pitch `0x3C`) a height lift moves the depth
/// by under a unit and the floor kept the tie.
pub const DROP_SHADOW_DEPTH_LIFT: f32 = 6.0;

/// The party class bit (`0x01000000`), which retail's player actor carries
/// and the port's player move state does not model.
const PARTY_CLASS_BIT: u32 = 0x0100_0000;
/// The placement class bit (`0x20000`) `FUN_8003A1E4` ORs into every
/// partition-1 actor it seats, which the port's channel flags do not mirror.
pub(super) const PLACEMENT_CLASS_BIT: u32 = 0x0002_0000;

impl World {
    /// Retail's overworld bit `_DAT_1F800394 & 1`, which `FUN_800271A8` sets
    /// on the three kingdom overworlds: the port's [`SceneMode::WorldMap`].
    /// Every overworld consumer of the curvature table keys on it - the prim
    /// leaves (the hosts' mesh shaders, through
    /// `Renderer::set_overworld_curvature` and the page's `u_curve`), the fog
    /// sheets and the drop shadow - so both hosts ask this one predicate.
    pub fn overworld_bit(&self) -> bool {
        self.mode == SceneMode::WorldMap
    }

    /// This frame's drop-shadow cells through `view`, in link order - four
    /// per shadowed actor. Empty outside game mode `3` (a field scene or the
    /// kingdom overworld; [`World::fog_mode`]).
    ///
    /// The casters are the actors `FUN_8001B964`'s gate passes:
    ///
    /// - the **player** (the party bit), unless its move state carries
    ///   `0x200000` - the jump take-off and the scripted vanish raise it;
    /// - every **partition-1 placement** (the class bit `FUN_8003A1E4` seats
    ///   it with) whose channel names a clip (`+0x5C != 0` - an actor with
    ///   none stays at draw kind `5`, which never reaches the animated
    ///   renderer), unless its channel flags carry `0x200000` or it stands
    ///   at the off-map hide box.
    ///
    /// A zero render scale does **not** drop the blob: the renderer's
    /// `+0x72 == 0` test branches straight to the shadow gate.
    ///
    /// Each cell carries per-corner scene depth taken
    /// [`DROP_SHADOW_DEPTH_LIFT`] nearer the eye, so both hosts draw it
    /// depth-tested against the scene they already drew.
    ///
    /// REF: FUN_8001B964 (the gate at `0x8001BE20..0x8001BE48`)
    ///
    /// `link_order` is a frame that draws each primitive at its table link
    /// (PSX mode): a cell then takes the one depth of its own link, the slot
    /// `FUN_8001C394` stores, against the field's flat primitives.
    pub fn field_drop_shadows(
        &self,
        view: &FieldCameraView,
        link_order: bool,
    ) -> Vec<DropShadowQuad> {
        // An in-world golf round or race keeps the field under it, and the
        // leader on that field keeps its blob.
        if !Self::fog_mode(self.mode)
            && self.pack_mode_stage().is_none()
            && self.mode != SceneMode::Dance
        {
            return Vec::new();
        }
        let curve_bias = self.overworld_curve_bias();
        let proj = self.field_fx_view(view);
        // The cells' corners go through the GTE's own integer `RTPT`
        // (`FUN_8001C394`) under the matrix `FUN_800172C0` loads, rebuilt
        // from the view; only the depth a host tests against its scene comes
        // from the shared view-projection.
        let gte = legaia_render_kernels::gte::FieldGteView::from_field_view(
            view,
            legaia_engine_camera::projection::GTE_OFY as i32,
        );
        let project = |p: [i32; 3]| {
            let r = gte.rtps(p.map(|c| c as i16));
            if r.mac[2] <= 0 {
                return None;
            }
            let depth = proj.ndc_depth_toward_eye(p, DROP_SHADOW_DEPTH_LIFT);
            Some(ShadowVertex {
                sx: r.sxy.0,
                sy: r.sxy.1,
                sz: r.sz,
                depth,
            })
        };
        let mut out = Vec::new();
        let mut cast = |flags: u32, pos: [i32; 3], near: bool| {
            if casts_shadow(flags)
                && let Some(q) = drop_shadow(pos, curve_bias, DROP_SHADOW_OT_SHIFT, near, project)
            {
                out.extend(q.map(|mut cell| {
                    if link_order {
                        // The cell's link in the `SZ` the field's flat primitives
                        // compare in: the slot is `SZ / 4`, and a near actor's
                        // ten slots of the table base are `320` `SZ`, as in
                        // `prim_mean_depth`.
                        let near_undo = if near { SHADOW_NEAR_SLOTS } else { 0 };
                        let sz = (cell.ot_index + near_undo) * 4;
                        let sz = sz.saturating_sub(if near { 320 } else { 0 });
                        cell.depth = proj.ndc_at_sz(sz as f32).map(|d| [d; 4]);
                    }
                    cell
                }));
            }
        };
        // The dance's floor slots: `FUN_801D0190` seats each with the placement
        // class bit, and the hall's actor pass reaches the same renderer. The
        // walked-in scene's player and placements are not on this floor.
        if self.mode == SceneMode::Dance {
            if let Some(g) = self.minigames.dance.as_ref() {
                for a in g.dancer_actors() {
                    cast(PLACEMENT_CLASS_BIT, a.pos.map(i32::from), false);
                }
            }
            return out;
        }
        if let Some(a) = self
            .player_actor_slot
            .and_then(|s| self.actors.get(usize::from(s)))
        {
            let m = &a.move_state;
            cast(
                m.flags | PARTY_CLASS_BIT,
                [
                    i32::from(m.world_x),
                    i32::from(m.world_y),
                    i32::from(m.world_z),
                ],
                m.flags & SHADOW_NEAR_BIT != 0
                    || self.player_near_buckets(i32::from(m.world_x), i32::from(m.world_z)),
            );
        }
        if self.mode == SceneMode::Field {
            let hide = FIELD_OFFMAP_HIDE_XZ;
            for c in self.field_vm.channels.iter().filter(|c| !c.object_bind) {
                if c.ctx.move_id == 0 {
                    continue;
                }
                let Ok(slot) = u8::try_from(c.placement_index) else {
                    continue;
                };
                let (x, z) = self
                    .npcs
                    .positions
                    .get(&slot)
                    .copied()
                    .unwrap_or((c.ctx.world_x as i16, c.ctx.world_z as i16));
                if x == hide && z == hide {
                    continue;
                }
                let y = self.field_npc_render_y(slot, x, z);
                cast(
                    c.ctx.flags | PLACEMENT_CLASS_BIT,
                    [i32::from(x), y, i32::from(z)],
                    c.ctx.flags & SHADOW_NEAR_BIT != 0,
                );
            }
        }
        out
    }
}
