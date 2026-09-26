//! The battle per-actor draw's per-object colour rule, resolved against
//! live world state for the two hosts' battle actor passes.
//!
//! The kernel is [`legaia_engine_vm::battle_actor_draw`] (the port of
//! `FUN_80048A08`); this block supplies what retail reads through its
//! globals: the seat (`+0x5A`), the seated actor's status word (`+0x16E`),
//! the present-party roster id (`0x8007BD10[seat]`) and the render node's
//! colour word (`+0x74`).

use super::*;

use legaia_engine_vm::battle_actor_draw::LimbDimPlan;
use legaia_engine_vm::battle_actor_tick as tick;
use legaia_engine_vm::battle_actor_tint as tint;
use legaia_engine_vm::battle_cam_script::BattleCamPose;

/// The body radius a party seat carries at `*(actor[+0x22C]) + 0x58`:
/// `640` on 212 of the 213 party seats read across the catalogued battle
/// states (the tint pass's depth-cue half-range is `radius / 2`). A monster
/// seat's is its record size class `<< 5`.
pub const PARTY_BODY_RADIUS: i16 = 640;

/// One battle body's draw decision for this frame - the retail tint pass
/// (`FUN_8004A908`) and the draw tick that gates it (`FUN_800480D8`), run
/// over live world state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BattleActorDrawPlan {
    /// Whether retail draws the body this frame: the render dispatcher's
    /// near-plane gate (view depth `>= 0xA1`) and the draw tick's colour /
    /// grey-gate verdict.
    pub drawn: bool,
    /// The tint pass's writes (`+0x74` / `+0x78`).
    pub tint: tint::BattleTint,
    /// The colour word the draw sees - the tint's, or the grey stamp.
    pub draw_colour: u32,
    /// The view depth the tint was computed at.
    pub view_z: i32,
}

impl BattleActorDrawPlan {
    /// The GTE far colour as the hosts' `DrawCue.far` (display `0..1`).
    pub fn cue_far(&self) -> [f32; 3] {
        let c = self.draw_colour;
        [
            f32::from(c as u8) / 255.0,
            f32::from((c >> 8) as u8) / 255.0,
            f32::from((c >> 16) as u8) / 255.0,
        ]
    }

    /// The GTE `IR0` as the hosts' `DrawCue.max_ir0` (`0x1000` = `1.0`).
    pub fn cue_ir0(&self) -> f32 {
        f32::from(self.tint.weight) / 4096.0
    }
}

impl World {
    /// The Rot limb dimming battle actor `actor_idx` draws with this frame,
    /// for a mesh of `object_count` objects - `None` when nothing on it dims
    /// (not a party seat, no Rot limb bit set, or no table installed).
    ///
    /// Both hosts call this from their battle actor pass: the native window
    /// re-colours the per-frame posed mesh, the browser play page re-uploads
    /// the actor's packet-colour stream when [`LimbDimPlan::key`] changes.
    ///
    /// The colour word is the render node's `+0x74`, which the tint pass
    /// `FUN_8004A908` packs from the actor's `+0x04` lanes (`>> 2` each). The
    /// engine's resting `render_color` is `0` where retail's is the neutral
    /// `0x20080200`, so a zero word reads as neutral here. An active tint is
    /// the host's per-draw cue on the whole mesh; retail replaces it on a
    /// dimmed object instead, so a limb dimmed during a tint flash also takes
    /// the flash in the port.
    pub fn battle_limb_dim_plan(
        &self,
        actor_idx: usize,
        object_count: usize,
    ) -> Option<LimbDimPlan> {
        let table = self.tables.rot_limb_table.as_ref()?;
        if self.mode != SceneMode::Battle || actor_idx >= usize::from(self.party.party_count) {
            return None;
        }
        let actor = self.actors.get(actor_idx)?;
        if actor.battle_monster_id.is_some() {
            return None;
        }
        let roster_id = u8::try_from(self.party_roster_slot(actor_idx) + 1).ok()?;
        let status = self.battle.status_effects.display_flags(actor_idx as u8);
        let word = match actor.battle.render_color {
            0 => legaia_engine_vm::battle_impact_fx::IMPACT_NEUTRAL_STATE,
            w => w,
        };
        let [r, g, b] = legaia_engine_vm::battle_impact_fx::unpack_actor_state_rgb(word);
        let colour = u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16);
        LimbDimPlan::resolve(
            colour,
            actor_idx as i16,
            status,
            table.row(roster_id),
            object_count,
        )
    }

    /// This frame's draw decision for battle body `actor_idx` - `None` for an
    /// actor that is not a battle combatant (party seat or seated monster).
    ///
    /// `pose` is the battle camera the host projects with (`None` for the
    /// monster-framing fallback outside a stage-dome battle, where the body is
    /// judged at retail's parked depth instead); `outdoor` is the stage's
    /// `DAT_80078C1C` outdoor-table membership. Both hosts call this per body
    /// per frame: the native window's battle actor pass and the browser play
    /// page's `play_battle_actor_transforms` (draw gate) and
    /// `play_battle_actor_cursor` (cue).
    pub fn battle_actor_draw_plan(
        &self,
        actor_idx: usize,
        pose: Option<&BattleCamPose>,
        world_scale: f32,
        outdoor: bool,
    ) -> Option<BattleActorDrawPlan> {
        if self.mode != SceneMode::Battle {
            return None;
        }
        let actor = self.actors.get(actor_idx)?;
        let party_count = usize::from(self.party.party_count);
        let (seat, radius, formation_cell) = match actor.battle_monster_id {
            Some(id) => {
                let slot = actor_idx.checked_sub(party_count)?;
                let size = self
                    .tables
                    .monster_catalog
                    .get(id)
                    .map_or(0, |def| def.size_class);
                (3 + slot as i16, i16::from(size) << 5, id as u8)
            }
            None if actor_idx < party_count => (actor_idx as i16, PARTY_BODY_RADIUS, 0),
            None => return None,
        };
        let b = &actor.battle;
        let view_z = match pose {
            Some(p) => legaia_engine_vm::battle_cam_script::battle_view_depth(
                p,
                world_scale,
                [
                    f32::from(actor.move_state.world_x),
                    f32::from(actor.move_state.world_y),
                    f32::from(actor.move_state.world_z),
                ],
            ) as i32,
            None => tick::PARKED_VIEW_DEPTH,
        };
        // The engine's resting colour word is `0` where retail's is the
        // neutral `0x20080200`; a genuine zero only exists under a
        // presentation arm (the capture / defeat fade ramps the lanes down
        // with `+0x21C` raised).
        let lanes = match (b.render_color, b.render_flag) {
            (0, 0) => legaia_engine_vm::battle_impact_fx::IMPACT_NEUTRAL_STATE,
            (w, _) => w,
        };
        let t = tint::battle_actor_tint(&tint::BattleTintInputs {
            lanes,
            top: 0,
            blend: b.render_blend,
            render_flag: b.render_flag,
            status: self.battle.status_effects.display_flags(actor_idx as u8),
            fade: 0,
            radius,
            view_z,
            seat,
            formation_cell,
            outdoor,
            ctx_243: self.battle_ctx.gauge_rearm_latch != 0,
            prev_colour: 0,
            prev_weight: 0,
        });
        let verdict = tick::battle_actor_tick(
            &tick::BattleActorView {
                flags: 0,
                colour: t.colour,
                seat,
            },
            &tick::BattleTickGates {
                no_escape: self.battle.no_escape,
                second_monster: self.battle_monster_slots().len() > 1,
                seat_state: b.render_flag,
            },
        );
        let near_ok = pose.is_none() || view_z >= tick::NEAR_REJECT_DEPTH;
        Some(BattleActorDrawPlan {
            drawn: near_ok && verdict.drawn(),
            tint: t,
            draw_colour: verdict.colour,
            view_z,
        })
    }

    /// Apply [`Self::battle_limb_dim_plan`] to a per-frame posed battle mesh's
    /// packet colours - the native window's seat. `colors` must be the
    /// stream `legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot` built from the
    /// same `tmd` / `raw` (it walks the textured prims in the order
    /// `tmd_to_vram_mesh_with_object_ids` does, which supplies the object of
    /// each vertex). Returns whether anything was dimmed.
    pub fn dim_posed_battle_mesh(
        &self,
        actor_idx: usize,
        tmd: &legaia_tmd::Tmd,
        raw: &[u8],
        colors: &mut [[u8; 3]],
    ) -> bool {
        let Some(plan) = self.battle_limb_dim_plan(actor_idx, tmd.objects.len()) else {
            return false;
        };
        let ids = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(tmd, raw).1;
        if ids.len() != colors.len() {
            return false;
        }
        plan.apply_rgb(colors, &ids);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_vm::battle_actor_draw::RotLimbTable;
    use legaia_engine_vm::status_effects::StatusKind;

    fn world_with_table() -> World {
        let mut world = World::default();
        world.enter_battle(3, 2);
        let mut rows = [[0u8; 5]; 4];
        rows[0] = [2, 4, 5, 7, 10];
        rows[1] = [1, 1, 2, 2, 3];
        world.tables.rot_limb_table = Some(RotLimbTable { rows });
        world
    }

    #[test]
    fn a_rotted_party_limb_dims_its_own_object_range() {
        let mut world = world_with_table();
        assert!(world.battle_limb_dim_plan(0, 12).is_none());
        world.battle.status_effects.apply(0, StatusKind::Rot);
        world.battle.status_effects.set_rot_limb(0, 1);
        let plan = world.battle_limb_dim_plan(0, 12).expect("rotted limb");
        let dimmed: Vec<usize> = (0..12).filter(|&o| plan.is_dimmed(o as u32)).collect();
        assert_eq!(dimmed, vec![5, 6, 7]);
        // Neutral node colour 0x808080 -> far (0x20, 0x20, 0x40).
        assert_eq!(plan.far, [0x20, 0x20, 0x40]);
    }

    #[test]
    fn the_row_follows_the_roster_id_not_the_seat() {
        let mut world = world_with_table();
        world.set_active_party(vec![1, 0, 2]);
        world.battle.status_effects.apply(0, StatusKind::Rot);
        world.battle.status_effects.set_rot_limb(0, 0);
        let plan = world.battle_limb_dim_plan(0, 12).expect("rotted limb");
        // Seat 0 holds roster slot 1 (id 2): row [1, 1, ...].
        assert!(plan.is_dimmed(1) && !plan.is_dimmed(2));
    }

    fn battle_world() -> World {
        let mut world = World::default();
        world.enter_battle(3, 1);
        world.actors[3].battle_monster_id = Some(4);
        world
    }

    fn pose(tr_z: f32) -> BattleCamPose {
        BattleCamPose {
            pitch: 0.0,
            yaw: 0.0,
            tr: [0.0, 0.0, tr_z],
            focus: [0.0; 3],
        }
    }

    #[test]
    fn a_near_resting_party_body_draws_neutral() {
        let mut world = battle_world();
        world.actors[0].move_state.world_z = 0;
        // Party radius 640 -> half-range 320; view 4000 / 16 = 250 is near.
        let p = world
            .battle_actor_draw_plan(0, Some(&pose(4000.0)), 4.0, false)
            .expect("party seat");
        assert!(p.drawn);
        assert_eq!(p.view_z, 4000);
        assert_eq!(p.tint.arm, tint::TintArm::Plain);
        assert_eq!(p.draw_colour & 0xFF_FFFF, 0x80_8080);
        assert!((p.cue_ir0() - 1000.0 / 4096.0).abs() < 1e-6);
    }

    #[test]
    fn a_far_body_fades_darker_indoors_and_brighter_outdoors() {
        let mut world = battle_world();
        world.actors[0].move_state.world_z = 0;
        let indoor = world
            .battle_actor_draw_plan(0, Some(&pose(8117.0)), 4.0, false)
            .unwrap();
        assert_eq!(indoor.tint.arm, tint::TintArm::DepthCue);
        assert_eq!(indoor.draw_colour & 0xFF_FFFF, 0x50_5050);
        let outdoor = world
            .battle_actor_draw_plan(0, Some(&pose(8117.0)), 4.0, true)
            .unwrap();
        assert!(outdoor.draw_colour & 0xFF > 0x80);
    }

    #[test]
    fn a_body_behind_the_near_plane_is_not_drawn() {
        let world = battle_world();
        let p = world
            .battle_actor_draw_plan(0, Some(&pose(0x80 as f32)), 4.0, false)
            .unwrap();
        assert!(!p.drawn);
        // Without a camera the body is judged at the parked depth.
        assert!(
            world
                .battle_actor_draw_plan(0, None, 4.0, false)
                .unwrap()
                .drawn
        );
    }

    #[test]
    fn a_faded_out_monster_drops_unless_the_lone_monster_gate_greys_it() {
        let mut world = battle_world();
        world.actors[3].battle.render_color = 0;
        world.actors[3].battle.render_flag = 2;
        let p = world.battle_actor_draw_plan(3, None, 4.0, false).unwrap();
        assert!(!p.drawn, "zero lanes in an ordinary fight: gone");
        world.battle.no_escape = true;
        let p = world.battle_actor_draw_plan(3, None, 4.0, false).unwrap();
        assert!(p.drawn, "lone monster, scripted fight: stamped grey");
        assert_eq!(p.draw_colour, tick::DEFEATED_GREY);
    }

    #[test]
    fn non_combatants_have_no_plan() {
        let world = battle_world();
        assert!(world.battle_actor_draw_plan(5, None, 4.0, false).is_none());
        let mut field = World::default();
        field.enter_battle(3, 1);
        field.mode = SceneMode::Field;
        assert!(field.battle_actor_draw_plan(0, None, 4.0, false).is_none());
    }

    #[test]
    fn monsters_and_a_disc_free_world_never_dim() {
        let mut world = world_with_table();
        world.battle.status_effects.apply(3, StatusKind::Rot);
        assert!(world.battle_limb_dim_plan(3, 12).is_none());
        world.tables.rot_limb_table = None;
        world.battle.status_effects.apply(0, StatusKind::Rot);
        assert!(world.battle_limb_dim_plan(0, 12).is_none());
    }
}
