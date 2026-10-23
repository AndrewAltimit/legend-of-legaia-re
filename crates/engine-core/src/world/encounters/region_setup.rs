//! Per-region battle setup: the stage variant, the backdrop keep and the region
//! door gates.

use super::*;

impl World {
    /// The battle-setup half of `FUN_801D9E1C` for the region the player
    /// stands in under the live story-flag state - retail's `a1 = 0` call.
    /// Leaves the previous setup in place when the tile is in no region,
    /// as retail's globals would be.
    ///
    /// REF: FUN_801D9E1C (ported as
    /// [`legaia_engine_battle::region_encounter::region_battle_setup`])
    pub fn apply_region_battle_setup_at_player(&mut self) {
        let Some((wx, wz)) = self
            .player_actor_slot
            .and_then(|s| self.actors.get(s as usize))
            .map(|a| (a.move_state.world_x, a.move_state.world_z))
        else {
            return;
        };
        let Some(mut tracker) = self.terrain.region_tracker.take() else {
            return;
        };
        tracker.select_group(|flag| self.encounter_condition_flag_test(flag));
        if let Some(region) = tracker.table().region_at_world(wx, wz) {
            let setup = legaia_engine_battle::region_encounter::region_battle_setup(&region.setup);
            self.store_region_battle_setup(setup);
        }
        self.terrain.region_tracker = Some(tracker);
    }

    /// Store a region setup the way retail's globals take it: a short-layout
    /// record leaves `_DAT_8007B64B` and the world-map return triple as they
    /// were, so those two carry over from the previous setup.
    pub(super) fn store_region_battle_setup(
        &mut self,
        mut setup: legaia_engine_battle::region_encounter::RegionBattleSetup,
    ) {
        if let Some(prev) = self.encounters.region_setup {
            setup.keep_backdrop_object_1 =
                setup.keep_backdrop_object_1.or(prev.keep_backdrop_object_1);
            setup.world_map_return = setup.world_map_return.or(prev.world_map_return);
        }
        self.encounters.region_setup = Some(setup);
    }

    /// Stamp the battle-stage variant `_DAT_8007BD60 & 0x1F` directly - the
    /// value the region reader leaves for the tile a fight starts on - for a
    /// caller that enters a fight without standing on that tile: a replay of
    /// a retail battle capture, whose RAM names the variant but whose player
    /// actor is no longer the field walker. The rest of the last setup is
    /// kept, as a short-layout region record would keep it.
    pub fn seed_battle_stage_variant(&mut self, variant: u8) {
        let mut setup = self.encounters.region_setup.unwrap_or(
            legaia_engine_battle::region_encounter::RegionBattleSetup {
                stage_variant: 0,
                keep_backdrop_object_1: None,
                door_of_light_blocked: true,
                door_of_wind_blocked: true,
                world_map_return: None,
            },
        );
        setup.stage_variant = variant & 0x1F;
        self.encounters.region_setup = Some(setup);
    }

    /// Stamp battle init's keep-object-1 byte `_DAT_8007B64B` directly, for
    /// the same caller as [`Self::seed_battle_stage_variant`]: the region
    /// reader leaves it from a long-layout record's `+8` bit 5, and
    /// `FUN_800513F0` skips its drop-object-1 pass when it is set
    /// (`0x80051ABC`). A capture's RAM holds the byte; a replay that cannot
    /// stand on the region tile must carry it over, or the backdrop loses
    /// the object retail drew - nilboa's horizon mist ribbon.
    pub fn seed_battle_backdrop_keep_object_1(&mut self, keep: bool) {
        self.seed_battle_stage_variant(self.encounters.region_setup.map_or(0, |s| s.stage_variant));
        if let Some(setup) = self.encounters.region_setup.as_mut() {
            setup.keep_backdrop_object_1 = Some(keep);
        }
    }

    /// The Door of Light / Door of Wind gates the last region setup left in
    /// scratchpad `0x1F800394` (`0x100000` / `0x200000`): `(light, wind)`,
    /// `true` = blocked. Open until a region has been stood in.
    pub fn region_door_gates(&self) -> (bool, bool) {
        self.encounters.region_setup.map_or((false, false), |s| {
            (s.door_of_light_blocked, s.door_of_wind_blocked)
        })
    }
}
