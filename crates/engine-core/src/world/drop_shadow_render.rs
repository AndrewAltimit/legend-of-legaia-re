//! The field actor drop shadows as a `World` method - the seam both play
//! hosts draw the blob through ([`crate::drop_shadow`]).
//!
//! Retail emits the blob from inside the animated-actor renderer
//! (`FUN_8001B964` -> `FUN_8001C394`), once per drawn actor that passes the
//! gate. The port has no per-actor retail renderer to hang it off, so this
//! walks the same population from engine state: the player actor, and every
//! MAN partition-1 placement's field-VM channel.

use super::*;
use crate::drop_shadow::{
    DropShadowQuad, SHADOW_NEAR_BIT, ShadowVertex, casts_shadow, drop_shadow,
};
use crate::fog_particles::FogView;
use legaia_engine_vm::psx_camera::FieldCameraView;

/// The OT-resolution byte `DAT_1F8003A4` the port sorts the blobs with.
/// The engine keeps no ordering table of its own depth resolution; the blob
/// only has to sort among the other screen primitives (see
/// `legaia_engine_ui::move_strip::FIELD_OT_SHIFT`, the same choice).
pub const DROP_SHADOW_OT_SHIFT: u8 = 0;

/// How far above the actor's floor (retail units, `-Y`) the blob's **depth**
/// is taken from. The projected corners stay on the floor; only the depth a
/// host tests against its scene moves, so the blob wins the tie with the
/// ground it lies on (retail sorts the ground into the far bucket, so the
/// blob always draws over it) without pulling in front of the actor. Clear
/// of the port's own coplanar lifts (under one unit) and the walk-ground
/// sink.
pub const DROP_SHADOW_DEPTH_LIFT: i32 = 6;

/// The party class bit (`0x01000000`), which retail's player actor carries
/// and the port's player move state does not model.
const PARTY_CLASS_BIT: u32 = 0x0100_0000;
/// The placement class bit (`0x20000`) `FUN_8003A1E4` ORs into every
/// partition-1 actor it seats, which the port's channel flags do not mirror.
const PLACEMENT_CLASS_BIT: u32 = 0x0002_0000;

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
    /// [`DROP_SHADOW_DEPTH_LIFT`] above the floor, so both hosts draw it
    /// depth-tested against the scene they already drew.
    ///
    /// REF: FUN_8001B964 (the gate at `0x8001BE20..0x8001BE48`)
    pub fn field_drop_shadows(&self, view: &FieldCameraView) -> Vec<DropShadowQuad> {
        if !Self::fog_mode(self.mode) {
            return Vec::new();
        }
        let overworld = self.overworld_bit();
        let proj = FogView::from_field_view(view);
        let project = |p: [i32; 3]| {
            let (sx, sy, sz) = proj.project_gte(p)?;
            let depth = proj.ndc_depth([p[0], p[1] - DROP_SHADOW_DEPTH_LIFT, p[2]]);
            Some(ShadowVertex { sx, sy, sz, depth })
        };
        let mut out = Vec::new();
        let mut cast = |flags: u32, pos: [i32; 3]| {
            if casts_shadow(flags)
                && let Some(q) = drop_shadow(
                    pos,
                    overworld,
                    DROP_SHADOW_OT_SHIFT,
                    flags & SHADOW_NEAR_BIT != 0,
                    project,
                )
            {
                out.extend(q);
            }
        };
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
                );
            }
        }
        out
    }
}
