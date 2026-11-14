//! The developer menu's `World` seam.
//!
//! The screen - its rows, pages, value transforms and the equip commit's bag
//! host - is `legaia_engine_menus::dev_menu_host`. This module holds the two
//! calls that need the live world: the frame both hosts run against a
//! `World` and its camera, and the routing of the screen's cues onto the
//! world's SFX ring.

use legaia_engine_dev_menu::dev_equip_commit::EquipCommit;
use legaia_engine_menus::dev_menu_host::{DevMenuRow, DevMenuSession, WorldEquipHost};
use legaia_engine_system::pad::PACK_CROSS;

/// The dev-menu calls that take the live `World`, on
/// [`DevMenuSession`].
pub trait DevMenuWorldExt {
    /// One frame of the screen against a live world and its camera.
    fn tick_host(
        &mut self,
        world: &mut legaia_engine_core::world::World,
        camera: &mut legaia_engine_field::camera::Camera,
        pad_edge: u16,
        pad_held: u16,
    ) -> Option<Option<EquipCommit>>;
    /// Hand the cues raised since the last call to `world`'s SFX ring.
    fn route_sfx(&mut self, world: &mut legaia_engine_core::world::World) -> Vec<u8>;
}

impl DevMenuWorldExt for DevMenuSession {
    /// One frame of the screen against a live world and its camera - the
    /// call both hosts make.
    ///
    /// Wraps [`Self::tick`] with the state the `CAMERA` row reads and writes,
    /// so neither host has to know it exists: the switch is loaded from the
    /// follow camera's [`legaia_engine_field::camera::ZoneFollow::follow_enabled`] (retail
    /// `_DAT_8007B606`) and the box word from the world's published walk-region
    /// attributes (`0x1F800384..87`) before the tick, and the switch is written
    /// back after it. A switch turned on by this tick also queues the snap the
    /// retail edit arm runs.
    ///
    /// It also runs the `EQUIP` row's confirm (Cross while the row is
    /// hovered) against the engine's own bag through [`WorldEquipHost`], and
    /// returns that attempt: `None` when no commit was attempted this tick,
    /// `Some(None)` when the staged item was not in the bag, `Some(Some(c))`
    /// on a commit. The cues the commit raised join
    /// [`Self::pending_sfx`]. Both hosts carried this arm verbatim.
    fn tick_host(
        &mut self,
        world: &mut legaia_engine_core::world::World,
        camera: &mut legaia_engine_field::camera::Camera,
        pad_edge: u16,
        pad_held: u16,
    ) -> Option<Option<EquipCommit>> {
        self.camera_follow = camera.zone.follow_enabled;
        self.camera_box_word = u32::from_le_bytes(world.terrain.region_attributes.box_bytes);
        self.camera_snap = false;
        {
            let mut records: Vec<&mut [u8]> = world
                .party
                .roster
                .members
                .iter_mut()
                .map(|m| m.raw.as_mut_slice())
                .collect();
            self.tick(pad_edge, pad_held, &mut records);
        }
        camera.zone.follow_enabled = self.camera_follow;
        if std::mem::take(&mut self.camera_snap) {
            camera.zone.request_snap();
        }
        if self.current_row() != DevMenuRow::Equip || pad_edge & PACK_CROSS == 0 {
            return None;
        }
        let character = self.chars.character as usize;
        // The destination is the staged item's equip class, not the row:
        // the applier reads equip record `+7` through the item table
        // (`0x801E5A58..0x801E5A8C`), and a weapon lands on the
        // per-character byte of `0x8007B42C` (`0x801E5AC0..0x801E5AD8`).
        self.equip_slot_bits = world
            .tables
            .equip_stats
            .as_ref()
            .and_then(|t| t.row_for_item(self.equip_item))
            .map_or(0, |row| row.raw[7]);
        let mut weapon_slots: Vec<i16> =
            legaia_engine_core::field_submode_screen::RETAIL_WEAPON_SLOTS.to_vec();
        weapon_slots.resize(world.party.roster.members.len().max(4), 0);
        let member = world.party.roster.members.get_mut(character)?;
        let mut raw = std::mem::take(&mut member.raw);
        let mut host = WorldEquipHost {
            inventory: &mut world.party.inventory,
            sfx: Vec::new(),
        };
        let committed = self.commit_equip_row(&mut host, &mut raw, &weapon_slots);
        let cues = std::mem::take(&mut host.sfx);
        world.party.roster.members[character].raw = raw;
        self.pending_sfx.extend(cues);
        Some(committed)
    }

    /// Hand the cues raised since the last call to `world`'s SFX ring, and
    /// return them for a host that logs them.
    ///
    /// Every cue this screen raises is a `jal 0x80035B50` in retail - the
    /// cursor move (`li a0,0x21` in the delay slot of each arm of
    /// `FUN_801E9DC8`) and the confirm row's `0x25` (`0x801D7270` in
    /// `FUN_801D6E18`) - which is the field SFX-ring producer
    /// [`legaia_engine_core::world::World::push_sfx_cue`] ports. Both hosts replay that
    /// ring, so routing here is what makes the menu audible on both; each
    /// host used to drain the queue and drop it.
    fn route_sfx(&mut self, world: &mut legaia_engine_core::world::World) -> Vec<u8> {
        let cues = self.drain_sfx();
        for &cue in &cues {
            world.push_sfx_cue(i16::from(cue));
        }
        cues
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_dev_menu::dev_menu_leaves::DevMenuRow as RetailRow;
    use legaia_engine_dev_menu::dev_menu_leaves::SFX_CURSOR_MOVE;
    use legaia_engine_system::pad::{PACK_DOWN, PACK_LEFT, PACK_RIGHT};

    fn records() -> Vec<Vec<u8>> {
        (0..4).map(|_| vec![0u8; 0x414]).collect()
    }

    fn drive(s: &mut DevMenuSession, recs: &mut [Vec<u8>], edge: u16, held: u16) {
        let mut views: Vec<&mut [u8]> = recs.iter_mut().map(|r| r.as_mut_slice()).collect();
        s.tick(edge, held, &mut views);
    }

    /// The menu's cues reach the world's SFX ring - the `FUN_80035B50`
    /// producer both hosts replay - rather than a queue the hosts dropped.
    #[test]
    fn dev_menu_cues_route_onto_the_world_sfx_ring() {
        let mut s = DevMenuSession::new();
        let mut r = records();
        let mut w = legaia_engine_core::world::World::default();
        drive(&mut s, &mut r, PACK_DOWN, 0);
        assert_eq!(s.route_sfx(&mut w), vec![SFX_CURSOR_MOVE as u8]);
        assert_eq!(
            w.take_sfx_ring_ops(),
            vec![legaia_engine_system::sound_state::SfxRingOp::Push(
                SFX_CURSOR_MOVE as i16
            )]
        );
        assert!(s.drain_sfx().is_empty(), "routing drains the queue");
    }

    /// The `CAMERA` row is retail row 3 and flips the follow switch on either
    /// horizontal edge, and `tick_host` carries the switch onto the camera.
    #[test]
    fn the_camera_row_switches_the_follow_camera() {
        let mut w = legaia_engine_core::world::World::new();
        let mut cam = legaia_engine_field::camera::Camera::default();
        assert!(cam.zone.follow_enabled, "retail boots the switch on");
        let mut s = DevMenuSession::new();
        s.select(DevMenuRow::Camera);
        assert_eq!(DevMenuRow::Camera.retail_index(), 0x03);
        assert_eq!(
            DevMenuRow::Camera.retail_row(),
            Some(RetailRow::Camera),
            "the row maps onto retail's CAMERA arm"
        );
        s.tick_host(&mut w, &mut cam, PACK_RIGHT, 0);
        assert!(
            !cam.zone.follow_enabled,
            "Right turns the follow camera off"
        );
        assert_eq!(s.row_value(DevMenuRow::Camera).as_deref(), Some("OFF"));
        s.tick_host(&mut w, &mut cam, PACK_LEFT, 0);
        assert!(cam.zone.follow_enabled, "Left turns it back on");
        // Other edges leave it alone.
        s.tick_host(&mut w, &mut cam, PACK_CROSS, 0);
        assert!(cam.zone.follow_enabled);
    }

    /// The `EQUIP` row's confirm lands each item on the equip byte its class
    /// names - body `0`, head `1`, the character's weapon byte (`2`, `3`,
    /// `2`), footwear `4` - as `FUN_801E5A08` resolves it from the equipment
    /// table, whatever row the menu confirmed on.
    #[test]
    fn the_equip_row_lands_an_item_on_its_class_slot() {
        use legaia_game_tables::equip_stats::EquipStatTable;
        const EQUIP_BASE: usize = 0x196;
        let mut w = legaia_engine_core::world::World::new();
        w.set_equip_stats(EquipStatTable::from_entries(&[
            (0x10, [0, 0, 0, 0, 0, 0, 0, 0x00]),
            (0x11, [0, 0, 0, 0, 0, 0, 0, 0x20]),
            (0x12, [0, 0, 0, 0, 0, 0, 0, 0x40]),
            (0x13, [0, 0, 0, 0, 0, 0, 0, 0x60]),
        ]));
        while w.party.roster.members.len() < 3 {
            w.party
                .roster
                .members
                .push(legaia_save::CharacterRecord::zeroed());
        }
        let mut cam = legaia_engine_field::camera::Camera::default();
        for (character, item, slot) in [
            (0u8, 0x10u8, 0usize),
            (0, 0x11, 1),
            (0, 0x12, 2),
            (1, 0x12, 3),
            (2, 0x12, 2),
            (0, 0x13, 4),
        ] {
            for m in &mut w.party.roster.members {
                m.raw[EQUIP_BASE..EQUIP_BASE + 8].fill(0);
            }
            w.party.inventory.insert(item, 1);
            let mut s = DevMenuSession::new();
            s.select(DevMenuRow::Equip);
            s.equip_item = item;
            s.chars.character = character.into();
            let out = s.tick_host(&mut w, &mut cam, PACK_CROSS, 0);
            assert!(
                matches!(out, Some(Some(_))),
                "item {item:#x} commits on character {character}: {out:?}"
            );
            let raw = &w.party.roster.members[character as usize].raw;
            let worn: Vec<usize> = (0..8).filter(|&i| raw[EQUIP_BASE + i] == item).collect();
            assert_eq!(worn, vec![slot], "item {item:#x} character {character}");
        }
    }

    /// `tick_host` loads the world's published region box into the readout.
    #[test]
    fn tick_host_loads_the_world_region_box() {
        let mut w = legaia_engine_core::world::World::new();
        let mut cam = legaia_engine_field::camera::Camera::default();
        w.terrain.region_attributes.box_bytes = [0x10, 0x20, 0x30, 0x40];
        let mut s = DevMenuSession::new();
        s.tick_host(&mut w, &mut cam, 0, 0);
        assert_eq!(
            s.row_value(DevMenuRow::Camera).as_deref(),
            Some("ON 032 048"),
            "tick_host loads the world's published box"
        );
    }
}
