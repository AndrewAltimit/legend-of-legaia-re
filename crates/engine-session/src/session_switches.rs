//! The session switches both play hosts throw onto a live `World`: the
//! port-only gameplay and overlay toggles that have no persisted option
//! behind them, and how retail-faithful mode masks them.
//!
//! The native window takes them at launch (`--smart-monsters`,
//! `--no-entry-pulse`, `--retail` and the `LEGAIA_*` environment switches,
//! the launcher's rows); the play page takes them at any time (its boxes,
//! `crates/web-play/src/play_toggles.rs`). Both end in
//! [`SessionSwitches::apply`], so "retail-faithful holds this off" has one
//! definition: the switches [`WorldToggles::pin_retail`] lowers are lowered
//! here too while `retail` holds.

use legaia_engine_core::world::WorldToggles;

/// One host's switch set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionSwitches {
    /// A monster's single-target attack picks the lowest-HP living party
    /// member (`WorldToggles::smarter_monster_targeting`). Not retail.
    pub smart_monsters: bool,
    /// The port's per-placement overworld markers, a debug overlay
    /// (`WorldToggles::overworld_marker_overlay`).
    pub world_map_markers: bool,
    /// The scene-entry VDF pulse enhancement
    /// (`WorldToggles::entry_pulse_enabled`).
    pub entry_pulse: bool,
    /// The battle pass on retail's once-a-battle-frame cadence
    /// (`WorldToggles::battle_sm_frame_cadence`). Retail behaviour, so the
    /// retail mask leaves it as set.
    pub battle_frame_cadence: bool,
    /// The Arts arm opens the legacy saved-chain list
    /// (`WorldToggles::arts_saved_list`). Not retail.
    pub arts_saved_list: bool,
    /// Retail-faithful mode: every port-only switch above held off.
    pub retail: bool,
}

impl SessionSwitches {
    /// Write the set onto `toggles`.
    ///
    /// While `retail` holds this is the engine's whole retail definition,
    /// [`WorldToggles::pin_retail`]: besides the switches above it lowers
    /// the port's own guards (the hop landing guard) and restores the
    /// retail kernels (the damage finisher, the battle pass's RNG draw), so
    /// [`WorldToggles::retail_deviations`] comes back empty once the options
    /// (pinned by the host's own retail mode) have been applied. Released,
    /// the guard `pin_retail` lowered returns to the engine default; the
    /// kernels it restored are already their defaults.
    pub fn apply(&self, toggles: &mut WorldToggles) {
        let port = !self.retail;
        toggles.smarter_monster_targeting = self.smart_monsters && port;
        toggles.overworld_marker_overlay = self.world_map_markers && port;
        toggles.entry_pulse_enabled = self.entry_pulse && port;
        toggles.battle_sm_frame_cadence = self.battle_frame_cadence;
        toggles.arts_saved_list = self.arts_saved_list && port;
        if self.retail {
            toggles.pin_retail();
        } else {
            toggles.hop_landing_guard = WorldToggles::new().hop_landing_guard;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_on(retail: bool) -> SessionSwitches {
        SessionSwitches {
            smart_monsters: true,
            world_map_markers: true,
            entry_pulse: true,
            battle_frame_cadence: true,
            arts_saved_list: true,
            retail,
        }
    }

    #[test]
    fn switches_reach_the_world() {
        let mut t = WorldToggles::new();
        all_on(false).apply(&mut t);
        assert!(t.smarter_monster_targeting);
        assert!(t.overworld_marker_overlay);
        assert!(t.entry_pulse_enabled);
        assert!(t.battle_sm_frame_cadence);
        assert!(t.arts_saved_list);
    }

    /// Retail mode lowers exactly what `WorldToggles::pin_retail` lowers
    /// among these switches, and leaves the (retail) cadence alone.
    #[test]
    fn retail_masks_what_pin_retail_lowers() {
        let mut t = WorldToggles::new();
        all_on(true).apply(&mut t);
        let mut pinned = WorldToggles::new();
        all_on(false).apply(&mut pinned);
        pinned.pin_retail();
        assert_eq!(
            t.smarter_monster_targeting,
            pinned.smarter_monster_targeting
        );
        assert_eq!(t.overworld_marker_overlay, pinned.overworld_marker_overlay);
        assert_eq!(t.entry_pulse_enabled, pinned.entry_pulse_enabled);
        assert_eq!(t.arts_saved_list, pinned.arts_saved_list);
        assert!(t.battle_sm_frame_cadence);
    }

    /// Retail mode is the whole `pin_retail`: nothing off its retail value,
    /// whatever the switches and the port's guards said. Released, the hop
    /// guard comes back.
    #[test]
    fn retail_is_the_whole_retail_definition_and_releases() {
        let mut t = WorldToggles::new();
        t.use_damage_finish = false;
        t.battle_pass_rand_draw = false;
        all_on(true).apply(&mut t);
        assert!(
            t.retail_deviations().is_empty(),
            "{:?}",
            t.retail_deviations()
        );
        let mut t = WorldToggles::new();
        all_on(true).apply(&mut t);
        assert!(!t.hop_landing_guard);
        all_on(false).apply(&mut t);
        assert_eq!(t.hop_landing_guard, WorldToggles::new().hop_landing_guard);
    }
}
