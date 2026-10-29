//! Taking a door of the running scene on purpose: the partition-2 record
//! whose `0x3F` scene change lands in a named scene, entered the way the
//! player enters it. A capture-seeding instrument - a state reached through
//! a door carries history a card load does not replay (the music the door
//! kept, `docs/subsystems/audio.md` "A door keeps its music only behind
//! sub-op 6"), and entering through the door lets the engine's own door
//! logic produce it.

use super::*;

/// How [`SceneHost::take_door_to`] set the door off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorTaken {
    /// A walk-on door: the player stands on its gate-1 kind-1 trigger tile
    /// with the last-tile compare cleared, so the next tick's dispatch
    /// (`FUN_801D1EC4` -> `FUN_8003BDE0`) spawns the record as a crossing.
    WalkOn { record: usize, tile: (u8, u8) },
    /// A record no tile trigger spawns (a cutscene's own scene change):
    /// installed through the same gated dispatch body directly.
    Scripted { record: usize },
}

impl SceneHost {
    /// Set off the door of the running field scene that leads to `dest`.
    ///
    /// The door is the partition-2 record carrying a `0x3F` whose
    /// destination is `dest` ([`legaia_scene_formats::man_edit::scene_change_sites`]
    /// joined to [`legaia_scene_formats::man_edit::site_p2_slots`]); `record`
    /// picks one when several do. A record a gate-1 tile trigger spawns is
    /// walked onto; any other is installed through the record dispatch
    /// (`World::install_gated_p2_record`), whose story-flag gates still
    /// apply. Nothing moves until the caller ticks: the record's own script
    /// runs the door, its sound-flag writes included.
    pub fn take_door_to(&mut self, dest: &str, record: Option<usize>) -> Result<DoorTaken> {
        let man = self
            .field_man_cache
            .clone()
            .context("the running scene has no MAN")?;
        let sites: Vec<_> = legaia_scene_formats::man_edit::scene_change_sites(&man)
            .into_iter()
            .filter(|s| s.name == dest)
            .collect();
        let slots = legaia_scene_formats::man_edit::site_p2_slots(&man, &sites);
        let mut candidates: Vec<usize> = slots.into_iter().flatten().collect();
        candidates.dedup();
        let record = match record {
            Some(r) => candidates
                .iter()
                .copied()
                .find(|&c| c == r)
                .with_context(|| format!("P2[{r}] carries no scene change to {dest}"))?,
            None => *candidates
                .first()
                .with_context(|| format!("no partition-2 record changes scene to {dest}"))?,
        };
        let (primary, fallback) = &self.field_triggers;
        let tile = primary
            .iter()
            .chain(fallback.iter())
            .find(|t| t.gate == 1 && usize::from(t.record) == record)
            .map(|t| (t.tile_x, t.tile_z));
        if let Some(tile) = tile {
            let centre = |t: u8| ((i32::from(t) << 7) + 0x40) as i16;
            anyhow::ensure!(
                self.debug_seat_standing(centre(tile.0), centre(tile.1)),
                "the door tile {tile:?} of P2[{record}] is not standable"
            );
            // The seat stamps the tile as the last one compared; clear it so
            // the next dispatch reads the seat as a crossing.
            self.last_trigger_tile = None;
            return Ok(DoorTaken::WalkOn { record, tile });
        }
        let man_file = legaia_scene_formats::man_section::parse(&man)?;
        anyhow::ensure!(
            self.world.install_gated_p2_record(&man_file, &man, record),
            "P2[{record}]'s story-flag gates refuse it"
        );
        Ok(DoorTaken::Scripted { record })
    }
}
