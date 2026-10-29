//! The runtime of a pack map the player is standing in: which exit, trigger
//! zone and encounter zone the player's tile falls in, step by step.
//!
//! The scene host enters the map (`SceneHost::enter_expansion_map`), seats
//! an [`ActiveMap`] on `World::expansion.map`, and feeds it the player's
//! map-local tile once per field tick ([`ActiveMap::step`]). The map's
//! walkability is not here: it is baked into the scene's collision grid, and
//! the normal locomotion walks it.
//!
//! Zones fire on **entry**, the way a retail walk-on trigger fires on a tile
//! crossing: standing still on an exit does nothing, and the spawn tile does
//! not fire on the first tick ([`ActiveMap::new`] takes the spawn tile as the
//! last one seen).

use std::collections::BTreeSet;
use std::sync::Arc;

use legaia_expansion_format::map::{ExitTarget, MapDef, MapEvent, TileGrid, rect_contains};

/// A pack map in play.
#[derive(Debug, Clone)]
pub struct ActiveMap {
    /// The map's id (`maps/<id>.toml`).
    pub map_id: String,
    pub def: Arc<MapDef>,
    pub grid: Arc<TileGrid>,
    /// The tile the last step saw the player on.
    last_tile: Option<(u16, u16)>,
    /// Indices of the `once` triggers already fired this visit.
    fired: BTreeSet<usize>,
    /// The encounter zone the player is in.
    encounter: Option<usize>,
    /// Trigger events raised and not yet taken by their runtime.
    events: Vec<MapEvent>,
}

impl ActiveMap {
    /// A map entered with the player on map-local tile `spawn`.
    pub fn new(map_id: &str, def: Arc<MapDef>, grid: Arc<TileGrid>, spawn: (u16, u16)) -> Self {
        let mut m = Self {
            map_id: map_id.to_string(),
            def,
            grid,
            last_tile: Some(spawn),
            fired: BTreeSet::new(),
            encounter: None,
            events: Vec::new(),
        };
        m.encounter = m.encounter_zone_at(spawn);
        m
    }

    fn encounter_zone_at(&self, (c, r): (u16, u16)) -> Option<usize> {
        self.def
            .encounters
            .iter()
            .position(|z| rect_contains(&z.rect, c, r))
    }

    /// One field tick with the player on map-local `tile` (`None` when off
    /// the map's rectangle). Returns the exit the player just stepped onto,
    /// if any; trigger events queue for [`Self::take_events`].
    pub fn step(&mut self, tile: Option<(u16, u16)>) -> Option<ExitTarget> {
        if tile == self.last_tile {
            return None;
        }
        let prev = self.last_tile;
        self.last_tile = tile;
        let (c, r) = tile?;
        let was_in = |rect| prev.is_some_and(|(pc, pr)| rect_contains(rect, pc, pr));
        self.encounter = self.encounter_zone_at((c, r));
        for (i, t) in self.def.triggers.iter().enumerate() {
            if !rect_contains(&t.rect, c, r) || was_in(&t.rect) {
                continue;
            }
            if t.once && !self.fired.insert(i) {
                continue;
            }
            self.events.push(t.event.clone());
        }
        self.def
            .exits
            .iter()
            .find(|e| rect_contains(&e.rect, c, r) && !was_in(&e.rect))
            .map(|e| e.to.clone())
    }

    /// The pack encounter table of the zone the player stands in, `None`
    /// outside every zone (where the map rolls no encounters).
    ///
    /// Integration point: the battle runtime's encounter roll reads this.
    pub fn encounter_table(&self) -> Option<&str> {
        self.encounter
            .and_then(|i| self.def.encounters.get(i))
            .map(|z| z.table.as_str())
    }

    /// Drain the trigger events raised since the last call.
    ///
    /// Integration point: the battle, dialog and quest runtimes take these.
    pub fn take_events(&mut self) -> Vec<MapEvent> {
        std::mem::take(&mut self.events)
    }

    /// The trigger events raised and not yet taken.
    pub fn pending_events(&self) -> &[MapEvent] {
        &self.events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: &str = r######"
[tiles]
rows = ["......", "......", "......"]
[tiles.legend]
"." = {}

[[exits]]
rect = [5, 0, 5, 2]
to = { kind = "map", map = "next" }

[[triggers]]
rect = [2, 0, 2, 2]
event = { kind = "dialog", line = "hello" }
once = true

[[triggers]]
rect = [3, 1, 3, 1]
event = { kind = "quest", event = "step" }

[[encounters]]
rect = [0, 0, 1, 2]
table = "west"
"######;

    fn active(spawn: (u16, u16)) -> ActiveMap {
        let def: MapDef = toml::from_str(MAP).unwrap();
        let grid = def.tile_grid().unwrap();
        ActiveMap::new("m", Arc::new(def), Arc::new(grid), spawn)
    }

    #[test]
    fn the_spawn_tile_does_not_fire_and_standing_still_does_nothing() {
        let mut m = active((5, 1));
        assert_eq!(m.step(Some((5, 1))), None);
        assert!(m.pending_events().is_empty());
        // Leaving and coming back onto the exit fires it.
        assert_eq!(m.step(Some((4, 1))), None);
        assert!(matches!(m.step(Some((5, 1))), Some(ExitTarget::Map { .. })));
        // Moving within the exit band is not a new entry.
        assert_eq!(m.step(Some((5, 2))), None);
    }

    #[test]
    fn triggers_fire_on_entry_and_once_triggers_fire_once() {
        let mut m = active((0, 1));
        m.step(Some((1, 1)));
        m.step(Some((2, 1)));
        m.step(Some((2, 2)));
        m.step(Some((3, 1)));
        m.step(Some((2, 1)));
        m.step(Some((3, 1)));
        let ev = m.take_events();
        assert_eq!(
            ev,
            vec![
                MapEvent::Dialog {
                    line: "hello".into()
                },
                MapEvent::Quest {
                    event: "step".into()
                },
                MapEvent::Quest {
                    event: "step".into()
                },
            ]
        );
        assert!(m.take_events().is_empty());
    }

    #[test]
    fn the_encounter_table_follows_the_player() {
        let mut m = active((0, 0));
        assert_eq!(m.encounter_table(), Some("west"));
        m.step(Some((2, 0)));
        assert_eq!(m.encounter_table(), None);
        m.step(Some((1, 2)));
        assert_eq!(m.encounter_table(), Some("west"));
        m.step(None);
        assert_eq!(
            m.encounter_table(),
            Some("west"),
            "off-map keeps the last zone"
        );
    }
}
