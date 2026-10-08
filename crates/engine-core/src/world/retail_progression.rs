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
        // Not a progression table, but the same boot-time SCUS read both
        // hosts make: the dialog's `0xC7` names.
        self.tables.inline_names = inline_names_from_scus(scus);

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

/// The dialog's `0xC7 XX` name table: `XX * 8` bytes from `0x80073F24`,
/// each entry a NUL-terminated name inside its 8 bytes. Retail bounds the
/// index nowhere; the entries that hold a name are the leading run whose
/// bytes are printable up to the terminator (the three Ra-Seru names), and
/// the reader stops at the first that is not.
// REF: FUN_80036044 (the `0xC7` substitution arm)
pub fn inline_names_from_scus(scus: &[u8]) -> Vec<Vec<u8>> {
    const TABLE_VA: u32 = 0x8007_3F24;
    let mut out = Vec::new();
    for i in 0..32u32 {
        let Some(off) = legaia_asset::item_names::file_offset_for_va(scus, TABLE_VA + i * 8) else {
            break;
        };
        let Some(entry) = scus.get(off..off + 8) else {
            break;
        };
        let len = entry.iter().position(|&b| b == 0).unwrap_or(8);
        let name = &entry[..len];
        if name.is_empty() || !name.iter().all(|b| (0x20..0x7F).contains(b)) {
            break;
        }
        out.push(name.to_vec());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `0xC7` table reads the leading printable names and stops at the
    /// first entry that is not one; the dialog substitution resolves them.
    #[test]
    fn inline_names_read_the_leading_printable_run() {
        const T_ADDR: u32 = 0x8001_0000;
        let table = (0x8007_3F24 - T_ADDR) as usize + 0x800;
        let mut exe = vec![0u8; table + 0x40];
        exe[0..8].copy_from_slice(b"PS-X EXE");
        exe[0x18..0x1C].copy_from_slice(&T_ADDR.to_le_bytes());
        let t_size = (exe.len() - 0x800) as u32;
        exe[0x1C..0x20].copy_from_slice(&t_size.to_le_bytes());
        for (i, n) in [&b"Abc"[..], b"Defgh", b"Ij"].iter().enumerate() {
            exe[table + i * 8..table + i * 8 + n.len()].copy_from_slice(n);
        }
        exe[table + 24..table + 32].copy_from_slice(&[4, 4, 5, 7, 9, 8, 7, 3]);
        let names = inline_names_from_scus(&exe);
        assert_eq!(
            names,
            vec![b"Abc".to_vec(), b"Defgh".to_vec(), b"Ij".to_vec()]
        );

        let mut w = World::default();
        w.tables.inline_names = names;
        let subs = w
            .dialog_substitutions(&[0x1F, 0xC7, 0x01, b':', 0x00])
            .expect("the escape resolves");
        assert_eq!(subs.get(&(7, 1)), Some(&b"Defgh".to_vec()));
    }

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
