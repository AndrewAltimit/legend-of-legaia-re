//! [`FormationCell`]: a fight named by its formation cell rather than by a
//! scene MAN row, with its `play-window --battle cell:...` form.
//!
//! A battle capture carries the fight's monster ids in the formation cell
//! `0x8007BD0C[0..4]` and its scripted bit in `0x8007BD60 & 0x80`. Most cells
//! match a row the scene's MAN registers, and both comparison sides enter the
//! fight through that row. A cell no row carries - a boss or duel installed
//! from another table, a cast-showcase fight on a debug-built scene - is
//! registered as a formation of its own on both sides through
//! [`FormationCell::register`], so the headless seed and the image child arm
//! the same formation id with the same monsters.

use legaia_engine_core::world::World;

/// A fight's formation cell: the monster ids in seat order and the scripted
/// bit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormationCell {
    pub ids: Vec<u8>,
    pub scripted: bool,
}

impl FormationCell {
    /// The `--battle` operand: `cell:` then `s:` when scripted, then the ids
    /// as comma-separated hex (`cell:s:2a,2b`). [`Self::parse_spec`] reads it
    /// back.
    pub fn to_spec(&self) -> String {
        let ids: Vec<String> = self.ids.iter().map(|i| format!("{i:02x}")).collect();
        format!(
            "cell:{}{}",
            if self.scripted { "s:" } else { "" },
            ids.join(",")
        )
    }

    /// The inverse of [`Self::to_spec`]; `None` for any other operand.
    pub fn parse_spec(spec: &str) -> Option<Self> {
        let body = spec.trim().strip_prefix("cell:")?;
        let (scripted, list) = match body.strip_prefix("s:") {
            Some(rest) => (true, rest),
            None => (false, body),
        };
        let ids = list
            .split(',')
            .map(|t| u8::from_str_radix(t.trim(), 16).ok())
            .collect::<Option<Vec<u8>>>()?;
        (!ids.is_empty() && ids.len() <= legaia_scene_formats::encounter_record::FORMATION_SLOTS)
            .then_some(Self { ids, scripted })
    }

    /// Register the cell as a formation of its own and return its id: the
    /// record's synthetic id, stepped past any id the table already holds.
    /// Monsters the world's catalog lacks are read out of the monster
    /// archive (PROT 867), which `archive` supplies on demand. `None` when
    /// the cell does not fit an encounter record.
    pub fn register(
        &self,
        world: &mut World,
        scene: &str,
        archive: impl FnOnce() -> Option<Vec<u8>>,
    ) -> Option<u16> {
        let rec = legaia_scene_formats::encounter_record::EncounterRecord::new(
            self.ids.len() as u8,
            &self.ids,
        )?;
        let mut def =
            legaia_engine_battle::encounter_formation::formation_def_from_record(&rec, scene);
        let mut id = def.formation_id;
        while world.tables.formation_table.formation(id).is_some() {
            id = id.wrapping_add(1);
        }
        def.formation_id = id;
        def.header_flags = u8::from(self.scripted);
        world.tables.formation_table.insert(def);
        ensure_monsters(world, &self.ids, archive);
        Some(id)
    }
}

/// Put every id the world's monster catalog lacks into it, read out of the
/// monster archive (PROT 867).
pub fn ensure_monsters(world: &mut World, ids: &[u8], archive: impl FnOnce() -> Option<Vec<u8>>) {
    let ids: Vec<u16> = ids.iter().map(|&i| u16::from(i)).collect();
    if ids
        .iter()
        .all(|id| world.tables.monster_catalog.get(*id).is_some())
    {
        return;
    }
    let Some(bytes) = archive() else { return };
    let cat = legaia_engine_battle::monster_catalog::catalog_from_monster_archive(&bytes, &ids);
    for def in cat.by_id.into_values() {
        world.tables.monster_catalog.insert(def);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_round_trips() {
        for cell in [
            FormationCell {
                ids: vec![0x2a],
                scripted: true,
            },
            FormationCell {
                ids: vec![0x04, 0x05, 0xff],
                scripted: false,
            },
        ] {
            assert_eq!(FormationCell::parse_spec(&cell.to_spec()), Some(cell));
        }
    }

    #[test]
    fn spec_rejects_rows_and_overfull_cells() {
        assert_eq!(FormationCell::parse_spec("3"), None);
        assert_eq!(FormationCell::parse_spec("first"), None);
        assert_eq!(FormationCell::parse_spec("cell:"), None);
        assert_eq!(FormationCell::parse_spec("cell:1,2,3,4,5"), None);
        assert_eq!(FormationCell::parse_spec("cell:zz"), None);
    }

    #[test]
    fn register_steps_past_a_taken_id() {
        let mut world = World::new();
        let cell = FormationCell {
            ids: vec![0x04],
            scripted: true,
        };
        let a = cell.register(&mut world, "test", || None).unwrap();
        let b = cell.register(&mut world, "test", || None).unwrap();
        assert_ne!(a, b);
        let def = world.tables.formation_table.formation(a).unwrap();
        assert_eq!(def.header_flags, 1);
        assert_eq!(def.slots.len(), 1);
        assert_eq!(def.slots[0].monster_id, 4);
    }
}
