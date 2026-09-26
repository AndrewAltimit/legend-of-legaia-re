//! The static `SCUS_942.54` progression + battle-presentation tables, installed
//! through one entry point so every play host boots the same game.
//!
//! These six tables were installed by the native window's boot
//! (`legaia_engine_shell::boot`) one read at a time, and the browser play page's
//! `load_disc` installed none of them. Nothing failed: each consumer has a
//! disc-free fallback, so the page quietly ran a different game - the flat
//! 10/5 placeholder growth, no Noa / Gala threshold correction, summon spells
//! that never level, accessories with no passives, silent melee grunts and
//! cast voices (a zero XA cue duration), and a victory pose pick that skipped
//! its `rand()` so the RNG stream diverged from native after every battle.
//! See `docs/tooling/host-drift.md#a-boot-install-only-one-host-ran`.

use crate::world::World;

/// What [`World::install_retail_progression_tables`] managed to decode. Each
/// field is `true` when that table parsed off the supplied executable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetailProgressionTables {
    /// The XP threshold curve (`FUN_801E9504`'s static table).
    pub xp_curve: bool,
    /// The slots-1/2 threshold correction divisors (`_DAT_8007B81C`).
    pub xp_corrections: bool,
    /// The per-character stat-growth curves (`DAT_800769CC` / `DAT_80076918`).
    pub growth: bool,
    /// The leader's victory-pose table (`0x800788A0`).
    pub victory_pose: bool,
    /// The XA cue duration table (`DAT_800788B8`).
    pub xa_cue_durations: bool,
    /// The summon-magic spell-XP thresholds (`0x8007656C`).
    pub magic_xp: bool,
    /// The accessory ("Goods") passive-effect catalog.
    pub accessory_passives: bool,
}

impl RetailProgressionTables {
    /// `true` when every table decoded.
    pub fn all(&self) -> bool {
        self.xp_curve
            && self.xp_corrections
            && self.growth
            && self.victory_pose
            && self.xa_cue_durations
            && self.magic_xp
            && self.accessory_passives
    }
}

impl World {
    /// Install every static-SCUS progression table the battle and level-up
    /// kernels read: XP curve + correction divisors, stat growth, victory
    /// pose, XA cue durations, magic-XP thresholds and accessory passives.
    ///
    /// Both play hosts call this once at boot with the disc's own executable.
    /// A table that does not decode keeps its disc-free default, exactly as
    /// the per-table installs did. The tracker and table state persist across
    /// New Game (`begin_new_game` resets neither).
    pub fn install_retail_progression_tables(&mut self, scus: &[u8]) -> RetailProgressionTables {
        use legaia_asset::level_up_tables as lut;
        let mut got = RetailProgressionTables::default();

        if let Some(curve) = lut::xp_thresholds_from_scus(scus) {
            self.party.level_up_tracker.xp_table = curve;
            got.xp_curve = true;
            // The divisor table's runtime pointer is constant across the whole
            // save corpus, so it rides with the curve as plain SCUS data.
            let corrections = lut::xp_correction_divisors_from_scus(scus);
            got.xp_corrections = corrections.is_some();
            self.party.level_up_tracker.xp_corrections = corrections;
        }
        if let Some(tables) = lut::growth_tables_from_scus(scus) {
            let tracker = std::mem::take(&mut self.party.level_up_tracker);
            self.party.level_up_tracker = tracker.with_growth_tables(&tables);
            got.growth = true;
        }

        self.tables.victory_pose_table =
            legaia_asset::victory_pose::victory_pose_table_from_scus(scus);
        got.victory_pose = self.tables.victory_pose_table.is_some();
        self.audio.xa_cue_durations = legaia_asset::xa_cue_table::xa_cue_durations_from_scus(scus);
        got.xa_cue_durations = self.audio.xa_cue_durations.is_some();

        got.magic_xp = self.install_magic_xp_thresholds(scus);

        if let Some(table) = legaia_asset::accessory_passive::AccessoryPassiveTable::from_scus(scus)
        {
            self.set_accessory_passives(crate::accessory_passives::AccessoryPassives::from_disc(
                &table,
            ));
            got.accessory_passives = true;
        }
        got
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_executable_installs_nothing_and_keeps_defaults() {
        let mut w = World::default();
        let before = w.party.level_up_tracker.xp_table.clone();
        let got = w.install_retail_progression_tables(&[0u8; 64]);
        assert_eq!(got, RetailProgressionTables::default());
        assert!(!got.all());
        assert_eq!(w.party.level_up_tracker.xp_table, before);
        assert!(w.tables.victory_pose_table.is_none());
        assert!(w.audio.xa_cue_durations.is_none());
        assert!(w.tables.magic_xp_thresholds.is_none());
    }
}
