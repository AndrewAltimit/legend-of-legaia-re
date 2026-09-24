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
