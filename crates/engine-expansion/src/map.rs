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
//!
//! A `once` trigger is once **ever**: [`ActiveMap::new`] skips the triggers
//! whose [`once_flag`] the pack's saved flags already hold, and the runtime
//! that takes a [`FiredTrigger`] raises its flag.

use std::collections::BTreeSet;
use std::sync::Arc;

use legaia_expansion_format::map::{
    ExitTarget, FloorTop, MapDef, MapEvent, NpcDef, TileGrid, npc_slot, once_flag, rect_contains,
};

/// A trigger zone the player stepped into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiredTrigger {
    pub event: MapEvent,
    /// The pack flag to raise for a `once` trigger ([`once_flag`]), so it
    /// stays quiet on later visits.
    pub once_flag: Option<String>,
}

/// Where a save made on a pack map resumes: the map and the player's world
/// position and facing on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapLocation {
    pub map_id: String,
    pub x: i16,
    pub z: i16,
    /// The 12-bit facing angle.
    pub facing: u16,
}

/// A pack map in play.
#[derive(Debug, Clone)]
pub struct ActiveMap {
    /// The map's id (`maps/<id>.toml`).
    pub map_id: String,
    pub def: Arc<MapDef>,
    pub grid: Arc<TileGrid>,
    /// The tile the last step saw the player on.
    last_tile: Option<(u16, u16)>,
    /// Indices of the `once` triggers already fired.
    fired: BTreeSet<usize>,
    /// The encounter zone the player is in.
    encounter: Option<usize>,
    /// Trigger events raised and not yet taken by their runtime.
    events: Vec<FiredTrigger>,
    /// Each NPC's next patrol stop, an index into its route
    /// ([`Self::patrol_route`]), parallel to `def.npcs`.
    patrol_next: Vec<usize>,
    /// The standable props' tops ([`MapDef::floor_tops`]).
    floor_tops: Vec<FloorTop>,
}

/// What talking to a pack NPC does ([`ActiveMap::talk`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpcTalk {
    /// The NPC id: the quest event `Talked(id)`.
    pub npc: String,
    /// The `dialog.toml` line it says, `None` for a silent NPC.
    pub line: Option<String>,
    /// The card dealer it runs.
    pub shop: Option<String>,
}

impl ActiveMap {
    /// A map entered with the player on map-local tile `spawn`. `flag_set`
    /// answers whether a pack flag is raised: a `once` trigger whose flag
    /// is up has fired on an earlier visit and stays quiet.
    pub fn new(
        map_id: &str,
        def: Arc<MapDef>,
        grid: Arc<TileGrid>,
        spawn: (u16, u16),
        flag_set: impl Fn(&str) -> bool,
    ) -> Self {
        let fired = def
            .triggers
            .iter()
            .enumerate()
            .filter(|(i, t)| t.once && flag_set(&once_flag(map_id, t, *i)))
            .map(|(i, _)| i)
            .collect();
        let mut m = Self {
            map_id: map_id.to_string(),
            def,
            grid,
            last_tile: Some(spawn),
            fired,
            encounter: None,
            events: Vec::new(),
            patrol_next: Vec::new(),
            floor_tops: Vec::new(),
        };
        m.patrol_next = vec![0; m.def.npcs.len()];
        m.floor_tops = m.def.floor_tops();
        m.encounter = m.encounter_zone_at(spawn);
        m
    }

    /// The map-local tile under world `(x, z)` ([`MapDef::tile_at_world`]).
    pub fn tile_at_world(&self, x: i32, z: i32) -> Option<(u16, u16)> {
        self.def.tile_at_world(&self.grid, x, z)
    }

    /// The pack encounter table of the zone covering map-local `tile`.
    pub fn encounter_table_at(&self, tile: (u16, u16)) -> Option<&str> {
        self.encounter_zone_at(tile)
            .map(|i| self.def.encounters[i].table.as_str())
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
            self.events.push(FiredTrigger {
                event: t.event.clone(),
                once_flag: t.once.then(|| once_flag(&self.map_id, t, i)),
            });
        }
        self.def
            .exits
            .iter()
            .find(|e| rect_contains(&e.rect, c, r) && !was_in(&e.rect))
            .map(|e| e.to.clone())
    }

    /// The pack encounter table of the zone the player stood in at the last
    /// step, `None` outside every zone (where the map rolls no encounters).
    /// The world's roll reads the zone under the player's live position
    /// ([`Self::encounter_table_at`]).
    pub fn encounter_table(&self) -> Option<&str> {
        self.encounter
            .and_then(|i| self.def.encounters.get(i))
            .map(|z| z.table.as_str())
    }

    /// Drain the triggers fired since the last call. The scene host hands
    /// each to its runtime (`SceneHost::step_expansion_map`).
    pub fn take_events(&mut self) -> Vec<FiredTrigger> {
        std::mem::take(&mut self.events)
    }

    /// The triggers fired and not yet taken.
    pub fn pending_events(&self) -> &[FiredTrigger] {
        &self.events
    }

    /// Every NPC with the actor placement slot it stands in
    /// ([`npc_slot`]).
    pub fn npcs(&self) -> impl Iterator<Item = (u8, &NpcDef)> + '_ {
        self.def
            .npcs
            .iter()
            .enumerate()
            .filter_map(|(n, d)| Some((npc_slot(n)?, d)))
    }

    /// The NPC standing in placement slot `slot`.
    pub fn npc_at_slot(&self, slot: u8) -> Option<&NpcDef> {
        self.npcs().find(|&(s, _)| s == slot).map(|(_, d)| d)
    }

    /// The player talked to the NPC in `slot`: what it says and does.
    pub fn talk(&self, slot: u8) -> Option<NpcTalk> {
        let npc = self.npc_at_slot(slot)?;
        Some(NpcTalk {
            npc: npc.id.clone(),
            line: (!npc.line.is_empty()).then(|| npc.line.clone()),
            shop: npc.shop.clone(),
        })
    }

    /// The world-space route NPC `n` loops: its own tile, then each patrol
    /// waypoint. Empty for an NPC that stands still.
    pub fn patrol_route(&self, n: usize) -> Vec<(i16, i16)> {
        let Some(npc) = self.def.npcs.get(n) else {
            return Vec::new();
        };
        if npc.patrol.is_empty() {
            return Vec::new();
        }
        std::iter::once(&npc.tile)
            .chain(&npc.patrol)
            .map(|t| {
                let (x, z) = self.def.tile_centre(t[0], t[1]);
                (x as i16, z as i16)
            })
            .collect()
    }

    /// The next stop of NPC `n`'s patrol, advancing its cursor: the
    /// waypoint after the one it last reached, wrapping back to its own
    /// tile. `None` for an NPC that stands still.
    pub fn next_patrol_stop(&mut self, n: usize) -> Option<(i16, i16)> {
        let route = self.patrol_route(n);
        let cursor = self.patrol_next.get_mut(n)?;
        if route.is_empty() {
            return None;
        }
        *cursor = (*cursor + 1) % route.len();
        Some(route[*cursor])
    }

    /// The height (up positive) of the standable top under world `(x, z)`,
    /// the highest when several overlap. `None` off every top.
    pub fn floor_top_at(&self, x: i32, z: i32) -> Option<i16> {
        self.floor_tops
            .iter()
            .filter(|t| t.contains(x, z))
            .map(|t| t.top)
            .max()
    }

    /// Whether the map has any standable top.
    pub fn has_floor_tops(&self) -> bool {
        !self.floor_tops.is_empty()
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
event = { kind = "counter", counter = "step" }

[[encounters]]
rect = [0, 0, 1, 2]
table = "west"
"######;

    fn active(spawn: (u16, u16)) -> ActiveMap {
        let def: MapDef = toml::from_str(MAP).unwrap();
        let grid = def.tile_grid().unwrap();
        ActiveMap::new("m", Arc::new(def), Arc::new(grid), spawn, |_| false)
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
        let step = MapEvent::Counter {
            counter: "step".into(),
            add: 1,
        };
        assert_eq!(
            ev,
            vec![
                FiredTrigger {
                    event: MapEvent::Dialog {
                        line: "hello".into()
                    },
                    once_flag: Some("map.m.0".into()),
                },
                FiredTrigger {
                    event: step.clone(),
                    once_flag: None
                },
                FiredTrigger {
                    event: step,
                    once_flag: None
                },
            ]
        );
        assert!(m.take_events().is_empty());
    }

    #[test]
    fn a_once_trigger_whose_flag_is_saved_stays_quiet() {
        let def: MapDef = toml::from_str(MAP).unwrap();
        let grid = def.tile_grid().unwrap();
        let mut m = ActiveMap::new("m", Arc::new(def), Arc::new(grid), (0, 1), |f| {
            f == "map.m.0"
        });
        m.step(Some((2, 1)));
        assert!(m.take_events().is_empty());
    }

    const NPC_MAP: &str = r######"
[tiles]
origin = [10, 20]
rows = ["......", "......"]
[tiles.legend]
"." = {}

[[npcs]]
id = "elder"
tile = [1, 0]
line = "hello"

[[npcs]]
id = "dealer"
tile = [4, 1]
shop = "cards"
patrol = [[5, 1], [5, 0]]

[[props]]
mesh = { object = 1 }
tile = [2, 1]
top = 64
"######;

    fn npc_map() -> ActiveMap {
        let def: MapDef = toml::from_str(NPC_MAP).unwrap();
        let grid = def.tile_grid().unwrap();
        ActiveMap::new("m", Arc::new(def), Arc::new(grid), (0, 0), |_| false)
    }

    #[test]
    fn npcs_take_slots_from_one_and_talk_reports_line_and_shop() {
        let m = npc_map();
        let slots: Vec<(u8, &str)> = m.npcs().map(|(s, n)| (s, n.id.as_str())).collect();
        assert_eq!(slots, vec![(1, "elder"), (2, "dealer")]);
        assert!(m.talk(0).is_none());
        let t = m.talk(1).unwrap();
        assert_eq!(
            (t.npc.as_str(), t.line.as_deref()),
            ("elder", Some("hello"))
        );
        assert_eq!(t.shop, None);
        let t = m.talk(2).unwrap();
        assert_eq!(t.line, None);
        assert_eq!(t.shop.as_deref(), Some("cards"));
    }

    #[test]
    fn a_patrol_loops_through_its_waypoints_and_home() {
        let mut m = npc_map();
        assert_eq!(m.next_patrol_stop(0), None, "the elder stands still");
        let centre = |c: i16, r: i16| ((10 + c) * 128 + 64, (20 + r) * 128 + 64);
        assert_eq!(m.next_patrol_stop(1), Some(centre(5, 1)));
        assert_eq!(m.next_patrol_stop(1), Some(centre(5, 0)));
        assert_eq!(m.next_patrol_stop(1), Some(centre(4, 1)));
        assert_eq!(m.next_patrol_stop(1), Some(centre(5, 1)));
    }

    #[test]
    fn a_standable_prop_raises_the_floor_over_its_tile() {
        let m = npc_map();
        assert!(m.has_floor_tops());
        let (x0, z0) = (12 * 128, 21 * 128);
        assert_eq!(m.floor_top_at(x0, z0), Some(64));
        assert_eq!(m.floor_top_at(x0 + 127, z0 + 127), Some(64));
        assert_eq!(m.floor_top_at(x0 + 128, z0), None);
        assert_eq!(m.floor_top_at(x0 - 1, z0), None);
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
