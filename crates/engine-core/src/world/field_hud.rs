//! The field overlay's **passive-ability indicator HUD** - the badge column
//! retail floats over the player's head while an accessory passive is active
//! (`FUN_801d095c`).
//!
//! `legaia_engine_vm::field_passive_hud` carries both halves of the routine:
//! the three head-relative lifts, and the icon list a resolved anchor
//! produces. Neither had a caller, and the reason was never a missing
//! subsystem - the ability bits, the world-to-screen projection and the
//! pictogram primitive all existed. What was missing was the pass that puts
//! them in a line. This module is that pass, split the way every other
//! renderer-free World seat is: `World` answers the **world points** and the
//! **icon list**, and each host does its own projection between the two,
//! because the camera is the host's.
//!
//! Retail's own shape, kept exactly:
//!
//! * three points sharing the player's X and Z, each lifted a different
//!   fraction of the player's `+0x72` halfword;
//! * a **mixed** anchor - X from the first projected point, Y from the third,
//!   not a single point's pair;
//! * the encounter badge driven by the **parity** of the two encounter bits,
//!   so a party carrying both High and Low Encounter shows neither.

use legaia_engine_vm::field_passive_hud::{HudIcon, hud_anchor_offsets, passive_hud_icons};

use crate::world::World;

/// The three world points the badge anchor is projected from, in retail's
/// order: point `0` supplies the anchor's X and point `2` its Y.
pub type PassiveHudPoints = [[f32; 3]; 3];

impl World {
    /// The three head-relative world points, or `None` when no player actor
    /// is seated (the title screen, a cutscene with the party unloaded).
    ///
    /// Each shares the player's X / Z and lifts the Y by its own fraction of
    /// the player's `+0x72` halfword, which is what glues the column to the
    /// head rather than to the feet. Retail **subtracts** the lift, its world
    /// Y growing downward, and the port keeps that sign because the value it
    /// subtracts from is retail's own `+0x16`.
    ///
    /// REF: FUN_801d095c (`0x801D098C..0x801D09F4`, ported as
    /// `legaia_engine_vm::field_passive_hud::hud_anchor_offsets`)
    pub fn passive_hud_points(&self) -> Option<PassiveHudPoints> {
        let slot = self.player_actor_slot? as usize;
        let a = self.actors.get(slot)?;
        let lifts = hud_anchor_offsets(a.move_state.field_72);
        let (x, y, z) = (
            a.move_state.world_x as f32,
            a.move_state.world_y as f32,
            a.move_state.world_z as f32,
        );
        Some([
            [x, y - lifts[0] as f32, z],
            [x, y - lifts[1] as f32, z],
            [x, y - lifts[2] as f32, z],
        ])
    }

    /// The icon list for an already-projected anchor - the pair
    /// `(point 0's screen X, point 2's screen Y)`.
    ///
    /// The bit source is [`Self::party_has_ability`], the engine's
    /// `FUN_800431D0`: the passive bitfield rebuilt from equipment, which is
    /// the same source the encounter-rate scaler reads for bits `0x3B` /
    /// `0x3C`.
    ///
    /// REF: FUN_801d095c (ported as
    /// `legaia_engine_vm::field_passive_hud::passive_hud_icons`)
    pub fn passive_hud_icons(&self, anchor: (i32, i32)) -> Vec<HudIcon> {
        passive_hud_icons(anchor, |bit| self.party_has_ability(bit))
    }

    /// `true` when at least one of the six bits the HUD tests is set, so a
    /// host can skip the projection entirely on a frame that would draw
    /// nothing.
    pub fn passive_hud_active(&self) -> bool {
        use legaia_engine_vm::field_passive_hud::ability_bit as b;
        [
            b::STACK_A,
            b::STACK_B,
            b::STACK_C,
            b::ENCOUNTER_HIGH,
            b::ENCOUNTER_LOW,
            b::BADGE_LEFT,
        ]
        .iter()
        .any(|&bit| self.party_has_ability(bit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_vm::field_passive_hud::{ability_bit, icon};

    fn world_with_bits(bits: &[u8]) -> World {
        let mut w = World::new();
        for &b in bits {
            let word = (b >> 5) as usize;
            w.party.party_ability_mask[word] |= 1u32 << (b & 0x1F);
        }
        w
    }

    #[test]
    fn no_player_actor_means_no_points() {
        let w = World::new();
        assert!(w.player_actor_slot.is_none());
        assert!(w.passive_hud_points().is_none());
    }

    #[test]
    fn the_three_points_share_x_and_z_and_differ_only_in_lift() {
        let mut w = World::new();
        w.player_actor_slot = Some(0);
        w.actors[0].move_state.world_x = 100;
        w.actors[0].move_state.world_y = 2000;
        w.actors[0].move_state.world_z = -50;
        w.actors[0].move_state.field_72 = 0x200;
        let p = w.passive_hud_points().expect("player seated");
        assert!(p.iter().all(|q| q[0] == 100.0 && q[2] == -50.0));
        // hud_anchor_offsets(0x200) == [13, 8, 21]; a lift raises the point,
        // so the largest lift gives the smallest Y.
        assert_eq!(p[0][1], 2000.0 - 13.0);
        assert_eq!(p[1][1], 2000.0 - 8.0);
        assert_eq!(p[2][1], 2000.0 - 21.0);
    }

    #[test]
    fn the_bit_source_is_the_party_passive_mask() {
        let w = world_with_bits(&[ability_bit::STACK_B]);
        assert!(w.passive_hud_active());
        let v = w.passive_hud_icons((100, 50));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, icon::STACK_B);
    }

    #[test]
    fn nothing_active_skips_the_pass() {
        let w = World::new();
        assert!(!w.passive_hud_active());
        assert!(w.passive_hud_icons((0, 0)).is_empty());
    }

    /// Both encounter bits together cancel - the parity rule - so the HUD
    /// stays empty and a host that gated on "either bit" would draw a badge
    /// retail does not.
    #[test]
    fn both_encounter_bits_cancel_but_still_count_as_active() {
        let w = world_with_bits(&[ability_bit::ENCOUNTER_HIGH, ability_bit::ENCOUNTER_LOW]);
        assert!(w.passive_hud_active());
        assert!(w.passive_hud_icons((100, 50)).is_empty());
    }
}
