//! The per-tick height arm of the field-actor driver `FUN_8003BC08`, applied
//! to the placement NPCs.
//!
//! Every partition-1 placement carries the seater's class bit `0x20000`
//! (`FUN_8003A1E4`, `0x8003A3A4..0x8003A3B4`), which is one of the two bits of
//! the arm's `flags & 0x20200` gate (`0x8003BC68..0x8003BC74`). So retail
//! rewrites every placement's Y `+0x16` on every actor tick: a snap to the
//! floor sample (`FUN_80019278`) while `flags & 0x2000` is clear, a clamped
//! glide toward it while the bit is set (`0x8003BCA8..0x8003BCF4`, at most
//! `_DAT_1F800393 * 6` per tick). The snap needs no state -
//! [`World::field_npc_render_y`] samples the floor under the NPC every time a
//! host places it - so this pass keeps only the glide's.
//!
//! Ahead of both floor arms, a placement whose `+0x10` carries `0x20000000`
//! takes `+0x16 = -(+0x8E)` outright (`0x8003BC50..0x8003BC64`, the store at
//! `0x8003BCF4`), wherever it stands. A script raises the bit with `31 1D`
//! and writes `+0x8E` with `4C 42` (the immediate arm also writes `+0x16`
//! once): `rikuroa`'s Caruban (`P1[9]`) parks at `540` in its prologue and
//! hovers there while its motion stream walks it, which a floor sample at
//! the walked tile would put back on the ground.

use super::World;
use legaia_engine_field::motion_pause::MOVING_CLASS;
use legaia_engine_motion_vm::motion_vm::{
    FieldActorHeight, FieldActorInputs, field_actor_plan, rotate_toward_clamped,
};

impl World {
    /// Step the height arm one actor tick for every placed field NPC.
    ///
    /// The flag word is retail's one `+0x10`, which the engine splits: the
    /// placement's field-VM context carries what `0x31` / `0x32` wrote
    /// (including the cross-context pokes a cutscene aims at the NPC - how
    /// the disc raises `0x2000`), the ambient channel what its motion ops
    /// wrote, and the seater's class bit is on every placement. The union is
    /// what `FUN_8003BC08` would read. The lifetime `+0x5C` is the channel's
    /// requested-move pair, non-negative for every placement.
    ///
    /// The `flags & 2` hold is retail's per-tick visibility cull
    /// (`FUN_801D79E8` sets bit 1 on an actor outside the camera's region
    /// box / visible tile window and clears it inside, before the arm reads
    /// the word), ported as [`World::field_npc_culled`] over the camera view
    /// the hosts publish: a culled glider keeps the Y it had. Under the
    /// visible-tile crop the hosts also skip the culled NPC's draw
    /// (`legaia_engine_world_views::field_view_window::npc_actor_visible`); with the crop off
    /// the hold shows only on a glider the player walks back into view of.
    ///
    /// A slot a scripted arc is carrying keeps the arc's height: the arc
    /// writes `+0x16` itself and [`World::field_npc_render_y`] reads it first.
    ///
    /// REF: FUN_8003BC08 (the height arm), FUN_80019278, FUN_801D79E8 (ported
    /// as `field_actor_culled`)
    pub fn tick_field_npc_heights(&mut self) {
        let slots = self.field_npc_height_slots();
        if slots.is_empty() {
            self.npcs.glide_y.clear();
            return;
        }
        let step = self.clock.frame_step.max(1);
        for (slot, (x, z)) in slots {
            if self.script_actors.npc_heights.contains_key(&slot) {
                continue;
            }
            let channel = self
                .npcs
                .ambient
                .get(&slot)
                .map(|c| (c.vm.actor_flags, c.vm.move_pair.unwrap_or(0)));
            let (ctx_flags, field_8e) = self.field_channel_flags_and_8e(slot);
            let flags = ctx_flags | channel.map_or(0, |c| c.0) | MOVING_CLASS;
            let flags = if self.field_npc_culled(x, z) {
                flags | 2
            } else {
                flags & !2
            };
            let plan = field_actor_plan(FieldActorInputs {
                lifetime: channel.map_or(0, |c| c.1),
                flags,
                field_8e,
                path_target_present: false,
                scripted_present: channel.is_some(),
                ambient_gate: false,
                frame_step: step,
                scene_guard_clear: true,
                global_suppress: false,
            });
            match plan.height {
                FieldActorHeight::GlideToFloor { rate } => {
                    let floor = self.sample_field_floor_height(i32::from(x), i32::from(z)) as i16;
                    // Entering the glide, `+0x16` holds last tick's snap.
                    let prev = self.npcs.glide_y.get(&slot).copied().unwrap_or(floor);
                    self.npcs
                        .glide_y
                        .insert(slot, rotate_toward_clamped(prev, floor, rate));
                }
                // A dead actor (`+0x5C < 0`) keeps whatever Y it had.
                FieldActorHeight::Hold => {}
                // The script-held height: `-(+0x8E)`, not a floor sample.
                FieldActorHeight::Mirror(y) => {
                    self.npcs.glide_y.insert(slot, y);
                }
                FieldActorHeight::SnapToFloor => {
                    self.npcs.glide_y.remove(&slot);
                }
            }
        }
    }

    /// Every placement the height arm runs on, at where it stands: the live
    /// position [`crate::world::FieldNpcState::positions`] holds for a slot
    /// that has one, else the placement context's own seat.
    ///
    /// `positions` lists only the text-bearing NPCs and the slots a script or
    /// motion stream has moved, but retail's actor driver runs the arm on
    /// every placement actor. A never-moved prop that holds a scripted height
    /// would otherwise fall to the floor sample: `rayman`'s plank bridges
    /// (`P1[70..=83]`, `31 1D` + `4C 42 40 01`, `+0x8E = 320`) stand on the
    /// chasm floor 64 units above the ledges they span instead of level with
    /// them.
    // REF: FUN_8003BC08 (height arm, 0x8003BC50..0x8003BCF4)
    fn field_npc_height_slots(&self) -> Vec<(u8, (i16, i16))> {
        let mut slots: std::collections::BTreeMap<u8, (i16, i16)> =
            self.npcs.positions.iter().map(|(&s, &p)| (s, p)).collect();
        for c in self.field_vm.channels.iter().filter(|c| !c.object_bind) {
            if let Ok(slot) = u8::try_from(c.placement_index) {
                slots
                    .entry(slot)
                    .or_insert((c.ctx.world_x as i16, c.ctx.world_z as i16));
            }
        }
        slots.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A world whose floor is tier 0 west of tile 4 and tier 1 from tile 4
    /// on, with tier 1 standing `rise` units above tier 0.
    fn stepped_world(rise: i16) -> World {
        let mut w = World::new();
        let stride = crate::world::FIELD_GRID_STRIDE;
        let mut grid = vec![0u8; crate::world::FIELD_GRID_LEN];
        for z in 0..stride {
            for x in 4..stride {
                grid[z * stride + x] = 1;
            }
        }
        w.terrain.collision_grid = grid;
        w.terrain.floor_height_lut[1] = rise;
        w
    }

    /// A placement channel whose context carries `0x2000` - the glide arm.
    fn glide_npc(w: &mut World, slot: u8, x: i16, z: i16) {
        w.npcs.positions.insert(slot, (x, z));
        let mut m = legaia_engine_motion_vm::ambient_motion::AmbientMotion::new(u32::from(slot), 0);
        m.actor_flags = MOVING_CLASS | 0x2000;
        w.npcs.ambient.insert(
            slot,
            crate::world::FieldNpcAmbient {
                defers: false,
                walks: false,
                variants: vec![(
                    legaia_scene_formats::man_motion::SELECTOR_DEFAULT,
                    vec![0x01],
                )],
                live: None,
                vm: m,
            },
        );
    }

    #[test]
    fn a_placement_without_the_glide_bit_snaps() {
        let mut w = stepped_world(-200);
        w.npcs.positions.insert(3, (6 * 128 + 64, 2 * 128 + 64));
        w.tick_field_npc_heights();
        assert!(w.npcs.glide_y.is_empty(), "the snap keeps no state");
        assert_eq!(w.field_npc_render_y(3, 6 * 128 + 64, 2 * 128 + 64), -200);
    }

    #[test]
    fn a_gliding_npc_closes_on_the_floor_at_six_per_frame_step() {
        let mut w = stepped_world(-200);
        w.clock.frame_step = 2;
        let (x, z) = (2 * 128 + 64, 2 * 128 + 64);
        glide_npc(&mut w, 5, x, z);
        w.tick_field_npc_heights();
        assert_eq!(w.field_npc_render_y(5, x, z), 0, "starts on its floor");
        // The NPC steps onto the raised tier: the glide closes 12 per tick.
        let nx = 6 * 128 + 64;
        w.npcs.positions.insert(5, (nx, z));
        w.tick_field_npc_heights();
        assert_eq!(w.field_npc_render_y(5, nx, z), -12);
        w.tick_field_npc_heights();
        assert_eq!(w.field_npc_render_y(5, nx, z), -24);
        for _ in 0..20 {
            w.tick_field_npc_heights();
        }
        assert_eq!(w.field_npc_render_y(5, nx, z), -200, "and lands on it");
    }

    #[test]
    fn clearing_the_bit_hands_the_npc_back_to_the_snap() {
        let mut w = stepped_world(-200);
        let (x, z) = (6 * 128 + 64, 2 * 128 + 64);
        glide_npc(&mut w, 5, 2 * 128 + 64, z);
        w.tick_field_npc_heights();
        w.npcs.positions.insert(5, (x, z));
        w.tick_field_npc_heights();
        assert_ne!(w.field_npc_render_y(5, x, z), -200);
        w.npcs.ambient.get_mut(&5).unwrap().vm.actor_flags = MOVING_CLASS;
        w.tick_field_npc_heights();
        assert_eq!(w.field_npc_render_y(5, x, z), -200);
    }

    /// A placement whose context carries `0x20000000` stands at `-(+0x8E)`
    /// wherever it walks, and falls back to the floor once the bit drops
    /// (`rikuroa`'s Caruban: `31 1D`, `4C 42 1C 02 00 00`).
    #[test]
    fn the_mirror_bit_holds_the_script_height_wherever_the_npc_walks() {
        let mut w = stepped_world(-200);
        let z = 2 * 128 + 64;
        w.npcs.positions.insert(4, (2 * 128 + 64, z));
        w.field_vm
            .channels
            .push(legaia_engine_field::field_channels::FieldChannel {
                placement_index: 4,
                ctx: legaia_engine_vm::field::FieldCtx {
                    flags: 0x2000_0000,
                    field_8e: 540,
                    ..Default::default()
                },
                record_offset: 0,
                pc: 0,
                done: false,
                object_bind: false,
            });
        w.tick_field_npc_heights();
        assert_eq!(w.field_npc_render_y(4, 2 * 128 + 64, z), -540);
        // Walked onto the raised tier: still the script's height.
        let nx = 6 * 128 + 64;
        w.npcs.positions.insert(4, (nx, z));
        w.tick_field_npc_heights();
        assert_eq!(w.field_npc_render_y(4, nx, z), -540);
        w.field_vm.channels[0].ctx.flags = 0;
        w.tick_field_npc_heights();
        assert_eq!(w.field_npc_render_y(4, nx, z), -200);
    }

    /// A placement no script has moved has no `positions` entry, but the
    /// height arm still runs on it: `rayman`'s plank bridges hold
    /// `-(+0x8E)` from their prologue, not the floor.
    #[test]
    fn an_unmoved_held_height_placement_leaves_the_floor() {
        let mut w = stepped_world(64);
        let mut c = legaia_engine_field::field_channels::FieldChannel {
            placement_index: 77,
            ctx: Default::default(),
            record_offset: 0,
            pc: 0,
            done: false,
            object_bind: false,
        };
        c.ctx.world_x = 6 * 128 + 64;
        c.ctx.world_z = 2 * 128 + 64;
        c.ctx.flags = 0x2000_0000;
        c.ctx.field_8e = 320;
        w.field_vm.channels.push(c);
        assert!(w.npcs.positions.is_empty());
        w.tick_field_npc_heights();
        assert_eq!(w.field_npc_render_y(77, 6 * 128 + 64, 2 * 128 + 64), -320);
    }
}
