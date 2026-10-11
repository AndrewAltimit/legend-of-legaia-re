//! `FieldPlayerHost` for [`FieldHostImpl`].

use super::*;

impl<'a> FieldPlayerHost for FieldHostImpl<'a> {
    fn player_cflag(&mut self, bit: u8, set: bool) -> bool {
        self.world.field_player_cflag(bit, set)
    }

    fn player_set_model(&mut self, value: i16) -> bool {
        self.world.field_player_set_model(value)
    }

    // REF: FUN_801DE840 (`4C 3A`, 0x801E10DC..0x801E10F4)
    fn apply_arrival_facing(&mut self) {
        self.world.apply_arrival_facing();
    }

    // `4C 37` (nibble 3 sub 7): copy the player's position and heading onto
    // the executing context - how a cutscene brings a party actor in at the
    // player (`bylon` `P2[9]` `CC 48 37`, then `C7 48 ..` walks it off). The
    // arm skips the player's own context - by context identity, not by the
    // party-bank bit `0x01000000` a party actor's `4C 50 F1` model select
    // raises (placement 37 there carries it). The default hook answered
    // `None`, so the actor kept its parked seat and the walk started at the
    // off-map hide box.
    // REF: FUN_801DE840 (nibble-3 sub-7)
    fn fetch_player_coords(&self, ctx: &FieldCtx) -> Option<vm::field::PlayerCoords> {
        if ctx.script_id == 0xF8 {
            return None;
        }
        let slot = self.world.player_actor_slot?;
        let ms = &self.world.actors.get(slot as usize)?.move_state;
        Some(vm::field::PlayerCoords {
            world_x: ms.world_x as u16,
            world_y: ms.world_y as u16,
            world_z: ms.world_z as u16,
            field_26: ms.render_26 as u16,
        })
    }

    /// Op `4C CE <value>` - store the player clip override `_DAT_8007B6AC`
    /// (`lbu v1, 1(s6); sw v1, -0x4954(v0)` at `0x801E2A24..0x801E2A30`),
    /// which the settle tail and the player clip arms read.
    ///
    /// REF: FUN_801DE840 (the nibble-C sub-`0xE` arm)
    fn set_player_clip_override(&mut self, value: u8) {
        self.world.locomotion.clip_override = u32::from(value);
    }
}
